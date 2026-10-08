//! rs-rtl adapter. Always drain upstream while stopping: 0.5.0 joins its USB
//! worker before dropping the bounded receiver, whose sender can be blocked.
use super::*;
use rs_rtl::{DeviceId, RtlSdr, rtlsdr::TryRecv};
use std::sync::Mutex;

pub(super) struct RustSession {
    stop: Arc<AtomicBool>,
    retuning: Arc<AtomicBool>,
    pub dropped: Arc<AtomicU64>,
    pub discarded: Arc<AtomicU64>,
    ready: mpsc::Receiver<RtlBlock>,
    error: Arc<Mutex<Option<String>>>,
    worker: Option<JoinHandle<Result<(RtlSdr, Context), String>>>,
    stopped: Option<(RtlSdr, Context)>,
}
impl RustSession {
    pub fn open(index: u32, hz: u32, rate: u32) -> Result<(Self, Option<i32>), String> {
        let mut device = RtlSdr::open(DeviceId::Index(index as usize))
            .map_err(|e| format!("Rust RTL open: {e}"))?;
        device.set_sample_rate(rate).map_err(|e| e.to_string())?;
        device.set_center_freq(hz).map_err(|e| e.to_string())?;
        device.set_gain_manual(280).map_err(|e| e.to_string())?;
        // rs-rtl 0.5 read_gain exposes an encoded register value, not verified dB.
        // Report no gain readback instead of treating that code as a measurement.
        if device.sample_rate() != rate || device.center_freq() != hz {
            return Err("Rust RTL configuration readback mismatch".into());
        }
        let (recycle, free) = mpsc::sync_channel(8);
        for _ in 0..8 {
            recycle
                .try_send(vec![Iq::default(); SAMPLES])
                .map_err(|e| e.to_string())?;
        }
        let (output, ready) = mpsc::sync_channel(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let discarded = Arc::new(AtomicU64::new(0));
        let retuning = Arc::new(AtomicBool::new(false));
        let context = Context {
            retuning: retuning.clone(),
            retune_discarded: discarded.clone(),
            free,
            recycle,
            ready: output,
            index: 0,
            dropped: dropped.clone(),
        };
        let mut session = Self {
            stop: Arc::new(AtomicBool::new(false)),
            retuning,
            dropped,
            discarded,
            ready,
            error: Arc::new(Mutex::new(None)),
            worker: None,
            stopped: Some((device, context)),
        };
        session.start()?;
        Ok((session, None))
    }
    fn start(&mut self) -> Result<(), String> {
        let (mut device, mut context) = self.stopped.take().ok_or("Rust RTL device unavailable")?;
        self.stop.store(false, Ordering::Release);
        let stop = self.stop.clone();
        let error = self.error.clone();
        self.worker = Some(
            thread::Builder::new()
                .name("rtl-rust-bridge".into())
                .spawn(move || {
                    let result: Result<(RtlSdr, Context), String> = (|| {
                        let reader = device
                            .start_streaming_with(8, SAMPLES * 2, 8)
                            .map_err(|e| e.to_string())?;
                        let outcome = loop {
                            if stop.load(Ordering::Acquire) {
                                break Ok(());
                            }
                            match reader.try_recv() {
                                Ok(TryRecv::Item(bytes)) => deliver(&bytes, &mut context),
                                Ok(TryRecv::Empty) => thread::sleep(Duration::from_millis(1)),
                                Ok(TryRecv::End) => {
                                    break Err("Rust RTL stream ended unexpectedly".to_owned());
                                }
                                Err(e) => break Err(format!("Rust RTL receive: {e}")),
                            }
                        };
                        reader.stop();
                        loop {
                            match reader.try_recv() {
                                Ok(TryRecv::Item(bytes)) => {
                                    let count = bytes.len() as u64 / 2;
                                    context.index += count;
                                    context.retune_discarded.fetch_add(count, Ordering::Relaxed);
                                }
                                Ok(TryRecv::Empty) => thread::sleep(Duration::from_millis(1)),
                                Ok(TryRecv::End) => break,
                                Err(_) => continue,
                            }
                        }
                        drop(reader); // Sender has closed; joining cannot deadlock on a full queue.
                        outcome?;
                        Ok((device, context))
                    })();
                    if let Err(ref e) = result {
                        *error.lock().expect("RTL error lock") = Some(e.clone());
                    }
                    result
                })
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    pub fn receive(&self, timeout: Duration) -> Result<RtlBlock, String> {
        self.ready.recv_timeout(timeout).map_err(|e| {
            self.error
                .lock()
                .ok()
                .and_then(|e| e.clone())
                .unwrap_or_else(|| format!("Rust RTL receive: {e}"))
        })
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !worker.is_finished() {
                if Instant::now() >= deadline {
                    return Err("Rust RTL cancellation timed out; device may remain busy until process exit".into());
                }
                thread::sleep(Duration::from_millis(2));
            }
            self.stopped = Some(worker.join().map_err(|_| "Rust RTL bridge panicked")??);
        }
        Ok(())
    }
    pub fn retune(&mut self, hz: u32) -> Result<(), String> {
        if !(sdr_core::RTL_MIN_HZ..=sdr_core::RX_MAX_HZ).contains(&hz) {
            return Err("RTL center outside normal tuner range".into());
        }
        self.retuning.store(true, Ordering::Release);
        self.stop()?;
        while let Ok(block) = self.ready.try_recv() {
            self.discarded
                .fetch_add(block.samples().len() as u64, Ordering::Relaxed);
        }
        let (device, _) = self.stopped.as_mut().ok_or("Rust RTL device unavailable")?;
        device.set_center_freq(hz).map_err(|e| e.to_string())?;
        if device.center_freq() != hz {
            return Err("Rust RTL frequency readback mismatch".into());
        }
        self.retuning.store(false, Ordering::Release);
        self.start()
    }
}
impl Drop for RustSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
