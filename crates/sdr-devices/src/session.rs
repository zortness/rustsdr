use crate::{ToneConfig, ToneSource};
use sdr_core::{BlockMetadata, Iq, RadioCommand, RadioError, SessionState};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

/// Returning a lease recycles its storage without blocking. Old-generation leases
/// return to their original pool, never to a restarted stream.
pub struct BlockLease {
    samples: Vec<Iq>,
    metadata: BlockMetadata,
    recycle: mpsc::SyncSender<Vec<Iq>>,
}

impl BlockLease {
    pub fn samples(&self) -> &[Iq] {
        &self.samples
    }
    pub fn metadata(&self) -> BlockMetadata {
        self.metadata
    }
}

impl Drop for BlockLease {
    fn drop(&mut self) {
        let _ = self.recycle.try_send(std::mem::take(&mut self.samples));
    }
}

struct Running {
    ready: mpsc::Receiver<BlockLease>,
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

/// An unpaced, backpressured synthetic source for deterministic headless work.
/// This is not the overflow policy for a future live hardware source.
pub struct SyntheticSession {
    config: ToneConfig,
    block_size: usize,
    pool_size: usize,
    stream_id: u64,
    generation: u64,
    running: Option<Running>,
}

impl SyntheticSession {
    pub fn state(&self) -> SessionState {
        if self.running.is_some() {
            SessionState::Running
        } else {
            SessionState::Stopped
        }
    }

    pub fn command(&mut self, command: RadioCommand) -> Result<(), RadioError> {
        match command {
            RadioCommand::Start => self.start(),
            RadioCommand::Stop => self.stop(),
            RadioCommand::SetRxCenterHz(_) => Err(RadioError::Unsupported(
                "synthetic center frequency is fixed for the session",
            )),
        }
    }

    pub fn new(
        config: ToneConfig,
        block_size: usize,
        pool_size: usize,
    ) -> Result<Self, RadioError> {
        ToneSource::new(config).map_err(RadioError::InvalidConfiguration)?;
        if !(1..=65_536).contains(&block_size) || !(1..=64).contains(&pool_size) {
            return Err(RadioError::InvalidConfiguration(
                "block size must be 1..=65536 and pool size 1..=64",
            ));
        }
        Ok(Self {
            config,
            block_size,
            pool_size,
            stream_id: NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed),
            generation: 0,
            running: None,
        })
    }

    pub fn start(&mut self) -> Result<(), RadioError> {
        if self.running.is_some() {
            return Err(RadioError::AlreadyRunning);
        }
        let mut source = ToneSource::new(self.config).map_err(RadioError::InvalidConfiguration)?;
        let (recycle, free) = mpsc::sync_channel::<Vec<Iq>>(self.pool_size);
        for _ in 0..self.pool_size {
            recycle
                .try_send(vec![Iq::default(); self.block_size])
                .map_err(|_| RadioError::WorkerFailed)?;
        }
        let (output, ready) = mpsc::sync_channel(self.pool_size);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(RadioError::WorkerFailed)?;
        let mut metadata = BlockMetadata {
            stream_id: self.stream_id,
            generation,
            sample_index: 0,
            sample_rate: self.config.sample_rate,
            center_hz: self.config.center_hz,
            discontinuity: true,
        };
        let worker = thread::Builder::new()
            .name("synthetic-iq".into())
            .spawn(move || {
                while !worker_cancel.load(Ordering::Acquire) {
                    let mut samples = match free.recv_timeout(Duration::from_millis(10)) {
                        Ok(samples) => samples,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    source.fill(&mut samples);
                    let count = samples.len() as u64;
                    let block = BlockLease {
                        samples,
                        metadata,
                        recycle: recycle.clone(),
                    };
                    // There are only pool_size buffers, so the ready queue can always
                    // hold every block. Never wait on a consumer during shutdown.
                    if output.try_send(block).is_err() {
                        break;
                    }
                    metadata.sample_index += count;
                    metadata.discontinuity = false;
                }
            })
            .map_err(|_| RadioError::WorkerFailed)?;
        self.generation = generation;
        self.running = Some(Running {
            ready,
            cancel,
            worker,
        });
        Ok(())
    }

    pub fn receive(&self, timeout: Duration) -> Result<BlockLease, RadioError> {
        let running = self.running.as_ref().ok_or(RadioError::NotRunning)?;
        running
            .ready
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => RadioError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => RadioError::WorkerFailed,
            })
    }

    /// Idempotent; cancellation works even when every buffer is held by a consumer.
    pub fn stop(&mut self) -> Result<(), RadioError> {
        if let Some(running) = self.running.take() {
            running.cancel.store(true, Ordering::Release);
            running
                .worker
                .join()
                .map_err(|_| RadioError::WorkerFailed)?;
        }
        Ok(())
    }
}

impl Drop for SyntheticSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
