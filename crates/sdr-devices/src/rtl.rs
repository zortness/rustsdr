//! Small dynamic librtlsdr boundary. No EEPROM writes, bias tee, or TX functions.
use libloading::Library;
use sdr_core::Iq;
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SAMPLES: usize = 16_384;
type Device = *mut c_void;
type SetU32 = unsafe extern "C" fn(Device, u32) -> c_int;
type SetInt = unsafe extern "C" fn(Device, c_int) -> c_int;
type Callback = unsafe extern "C" fn(*mut u8, u32, *mut c_void);

struct Api {
    count: unsafe extern "C" fn() -> u32,
    name: unsafe extern "C" fn(u32) -> *const c_char,
    open: unsafe extern "C" fn(*mut Device, u32) -> c_int,
    close: unsafe extern "C" fn(Device) -> c_int,
    rate: SetU32,
    frequency: SetU32,
    actual_gain: unsafe extern "C" fn(Device) -> c_int,
    actual_rate: unsafe extern "C" fn(Device) -> u32,
    actual_frequency: unsafe extern "C" fn(Device) -> u32,
    gain_mode: SetInt,
    gain: SetInt,
    direct: SetInt,
    reset: unsafe extern "C" fn(Device) -> c_int,
    read: unsafe extern "C" fn(Device, Callback, *mut c_void, u32, u32) -> c_int,
    cancel: unsafe extern "C" fn(Device) -> c_int,
    _library: Library,
}

/// An explicit environment path overrides the bundled development runtime.
pub fn library_path() -> PathBuf {
    if let Some(path) = std::env::var_os("RUSTSDR_RTLSDR_LIBRARY")
        .or_else(|| std::env::var_os("RUSTSDR_RTLSDR_DLL"))
    {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "macos")]
    {
        return PathBuf::from("librtlsdr.dylib");
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        return PathBuf::from("librtlsdr.so.0");
    }
    #[cfg(windows)]
    {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bundled = root.join("third-party/rtl-sdr-blog-v1.4.0/runtime/x64/rtlsdr.dll");
        if bundled.exists() {
            bundled
        } else {
            root.join("third-party/rtlsdr.dll")
        }
    }
}

impl Api {
    fn load() -> Result<Arc<Self>, String> {
        let path = library_path();
        let path = if cfg!(windows) || path.components().count() > 1 {
            path.canonicalize().map_err(|e| {
            format!(
                "RTL runtime missing: {e}. Set RUSTSDR_RTLSDR_LIBRARY to a matching native library."
            )
        })?
        } else {
            path
        };
        // SAFETY: user-provided/native vendor library is executable code. Use an
        // absolute path, and on Windows resolve dependencies beside it/system dirs.
        let library: Library = unsafe {
            #[cfg(windows)]
            { libloading::os::windows::Library::load_with_flags(&path, 0x100 | 0x1000).map(Into::into) }
            #[cfg(not(windows))]
            { Library::new(&path) }
        }.map_err(|e| format!("Cannot load {}: {e}. The native library must match the app architecture and have its dependencies installed.", path.display()))?;
        macro_rules! symbol {
            ($name:literal) => {
                // SAFETY: signatures match the public librtlsdr C API. _library keeps
                // the code mapped for the entire lifetime of these function pointers.
                unsafe {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| e.to_string())?
                }
            };
        }
        Ok(Arc::new(Self {
            count: symbol!("rtlsdr_get_device_count"),
            name: symbol!("rtlsdr_get_device_name"),
            open: symbol!("rtlsdr_open"),
            close: symbol!("rtlsdr_close"),
            rate: symbol!("rtlsdr_set_sample_rate"),
            frequency: symbol!("rtlsdr_set_center_freq"),
            actual_gain: symbol!("rtlsdr_get_tuner_gain"),
            actual_rate: symbol!("rtlsdr_get_sample_rate"),
            actual_frequency: symbol!("rtlsdr_get_center_freq"),
            gain_mode: symbol!("rtlsdr_set_tuner_gain_mode"),
            direct: symbol!("rtlsdr_set_direct_sampling"),
            gain: symbol!("rtlsdr_set_tuner_gain"),
            reset: symbol!("rtlsdr_reset_buffer"),
            read: symbol!("rtlsdr_read_async"),
            cancel: symbol!("rtlsdr_cancel_async"),
            _library: library,
        }))
    }
}

pub fn devices() -> Result<Vec<String>, String> {
    let api = Api::load()?;
    // SAFETY: API has been resolved and device-name pointers are library-owned.
    unsafe {
        Ok((0..(api.count)())
            .map(|index| {
                let name = (api.name)(index);
                if name.is_null() {
                    format!("RTL-SDR {index}")
                } else {
                    CStr::from_ptr(name).to_string_lossy().into_owned()
                }
            })
            .collect())
    }
}

struct Native {
    api: Arc<Api>,
    device: Device,
}
// SAFETY: configured before sharing. Only read_async on the acquisition thread
// and the API's thread-safe cancel_async on its owner run concurrently. Close
// occurs after the final Arc (including the acquisition thread's) is dropped.
unsafe impl Send for Native {}
unsafe impl Sync for Native {}
impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.device);
        }
    }
}

pub struct RtlBlock {
    samples: Vec<Iq>,
    len: usize,
    pub sample_index: u64,
    recycle: mpsc::SyncSender<Vec<Iq>>,
}
impl RtlBlock {
    pub fn samples(&self) -> &[Iq] {
        &self.samples[..self.len]
    }
}
impl Drop for RtlBlock {
    fn drop(&mut self) {
        let _ = self.recycle.try_send(std::mem::take(&mut self.samples));
    }
}

struct Context {
    retuning: Arc<AtomicBool>,
    retune_discarded: Arc<AtomicU64>,
    free: mpsc::Receiver<Vec<Iq>>,
    recycle: mpsc::SyncSender<Vec<Iq>>,
    ready: mpsc::SyncSender<RtlBlock>,
    index: u64,
    dropped: Arc<AtomicU64>,
}

/// Convert unsigned interleaved I/Q to centered complex samples.
pub fn convert_u8(bytes: &[u8], output: &mut [Iq]) -> Result<(), &'static str> {
    if bytes.len() != output.len() * 2 {
        return Err("IQ byte count must be twice the output sample count");
    }
    for (pair, sample) in bytes.chunks_exact(2).zip(output) {
        *sample = Iq {
            i: (pair[0] as f32 - 127.5) / 128.0,
            q: (pair[1] as f32 - 127.5) / 128.0,
        };
    }
    Ok(())
}

unsafe extern "C" fn callback(bytes: *mut u8, len: u32, context: *mut c_void) {
    // SAFETY: read_async calls serially with a valid buffer and the live Context
    // on its stack. Neither reference escapes the callback.
    let ctx = unsafe { &mut *context.cast::<Context>() };
    if bytes.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    deliver(bytes, ctx);
}

fn deliver(bytes: &[u8], ctx: &mut Context) {
    let count = bytes.len() / 2;
    let index = ctx.index;
    ctx.index = ctx.index.saturating_add(count as u64);
    if ctx.retuning.load(Ordering::Acquire) {
        ctx.retune_discarded
            .fetch_add(count as u64, Ordering::Relaxed);
        return;
    }
    if !bytes.len().is_multiple_of(2) || count > SAMPLES {
        ctx.dropped.fetch_add(count as u64, Ordering::Relaxed);
        return;
    }
    let Ok(mut samples) = ctx.free.try_recv() else {
        ctx.dropped.fetch_add(count as u64, Ordering::Relaxed);
        return;
    };
    let _ = convert_u8(bytes, &mut samples[..count]);
    let block = RtlBlock {
        samples,
        len: count,
        sample_index: index,
        recycle: ctx.recycle.clone(),
    };
    if ctx.ready.try_send(block).is_err() {
        ctx.dropped.fetch_add(count as u64, Ordering::Relaxed);
    }
}

pub struct NativeSession {
    retuning: Arc<AtomicBool>,
    retune_discarded: Arc<AtomicU64>,
    native: Arc<Native>,
    ready: mpsc::Receiver<RtlBlock>,
    worker: Option<JoinHandle<Result<Context, String>>>,
    context: Option<Context>,
    dropped: Arc<AtomicU64>,
}

fn check(code: c_int, operation: &str) -> Result<(), String> {
    if code < 0 {
        Err(format!(
            "RTL-SDR {operation} failed ({code}); check WinUSB and close other SDR applications"
        ))
    } else {
        Ok(())
    }
}

impl NativeSession {
    pub fn open(index: u32, frequency_hz: u32, sample_rate: u32) -> Result<Self, String> {
        if !(sdr_core::RTL_MIN_HZ..=sdr_core::RX_MAX_HZ).contains(&frequency_hz)
            || sample_rate != 2_400_000
        {
            return Err("RTL adapter supports normal tuner operation from 28.8 to 1700 MHz at 2.4 MS/s; HF direct sampling is not enabled".into());
        }
        let api = Api::load()?;
        let mut device = std::ptr::null_mut();
        // SAFETY: valid out-pointer; no device functions before successful open.
        unsafe {
            check((api.open)(&mut device, index), "open")?;
        }
        if device.is_null() {
            return Err("RTL-SDR returned a null device".into());
        }
        let native = Arc::new(Native { api, device });
        unsafe {
            check((native.api.frequency)(device, frequency_hz), "tune")?;
            check((native.api.direct)(device, 0), "disable direct sampling")?;
            check((native.api.rate)(device, sample_rate), "sample rate")?;
            check((native.api.gain_mode)(device, 1), "manual tuner gain")?;
            check((native.api.gain)(device, 280), "28 dB tuner gain")?;
            check((native.api.reset)(device), "reset buffer")?;
            let actual_rate = (native.api.actual_rate)(device);
            let actual_frequency = (native.api.actual_frequency)(device);
            if actual_rate != sample_rate || actual_frequency != frequency_hz {
                return Err(format!(
                    "RTL configuration mismatch: device reports {actual_frequency} Hz at {actual_rate} samples/s"
                ));
            }
        }
        let (recycle, free) = mpsc::sync_channel(8);
        for _ in 0..8 {
            recycle
                .try_send(vec![Iq::default(); SAMPLES])
                .map_err(|e| e.to_string())?;
        }
        let (output, ready) = mpsc::sync_channel(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let retuning = Arc::new(AtomicBool::new(false));
        let retune_discarded = Arc::new(AtomicU64::new(0));
        let context = Context {
            retuning: retuning.clone(),
            retune_discarded: retune_discarded.clone(),
            free,
            recycle,
            ready: output,
            index: 0,
            dropped: dropped.clone(),
        };
        let worker = start_reader(native.clone(), context)?;
        Ok(Self {
            retuning,
            retune_discarded,
            native,
            ready,
            worker: Some(worker),
            context: None,
            dropped,
        })
    }

    /// Keep the device open. Stop/join USB reads before touching tuner state,
    /// discard old queued blocks, then resume with the same sample-index timeline.
    pub fn retune(&mut self, frequency_hz: u32) -> Result<(), String> {
        if !(sdr_core::RTL_MIN_HZ..=sdr_core::RX_MAX_HZ).contains(&frequency_hz) {
            return Err("RTL center outside supported normal-tuner range".into());
        }
        self.retuning.store(true, Ordering::Release);
        self.stop()?;
        while let Ok(block) = self.ready.try_recv() {
            self.retune_discarded
                .fetch_add(block.samples().len() as u64, Ordering::Relaxed);
        }
        // SAFETY: acquisition has joined; Native and its library remain owned.
        unsafe {
            check(
                (self.native.api.frequency)(self.native.device, frequency_hz),
                "retune",
            )?;
            check(
                (self.native.api.reset)(self.native.device),
                "reset after retune",
            )?;
            if (self.native.api.actual_frequency)(self.native.device) != frequency_hz {
                return Err("RTL center-frequency readback mismatch".into());
            }
        }
        let context = self
            .context
            .take()
            .ok_or("RTL reader unavailable after stop")?;
        self.retuning.store(false, Ordering::Release);
        self.worker = Some(start_reader(self.native.clone(), context)?);
        Ok(())
    }

    pub fn receive(&self, timeout: Duration) -> Result<RtlBlock, String> {
        self.ready
            .recv_timeout(timeout)
            .map_err(|e| format!("RTL-SDR receive: {e}"))
    }
    pub fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    pub fn retune_discarded_samples(&self) -> u64 {
        self.retune_discarded.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(worker) = self.worker.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !worker.is_finished() {
                unsafe {
                    (self.native.api.cancel)(self.native.device);
                }
                if Instant::now() >= deadline {
                    return Err(
                        "RTL-SDR cancellation timed out; device may remain busy until process exit"
                            .into(),
                    );
                }
                thread::sleep(Duration::from_millis(5));
            }
            self.context = Some(
                worker
                    .join()
                    .map_err(|_| "RTL acquisition thread panicked".to_owned())??,
            );
        }
        Ok(())
    }
}

fn start_reader(
    reader: Arc<Native>,
    mut context: Context,
) -> Result<JoinHandle<Result<Context, String>>, String> {
    thread::Builder::new()
        .name("rtl-usb".into())
        .spawn(move || {
            // SAFETY: context lives until the blocking read returns; reader owns
            // the device and library for that entire interval.
            unsafe {
                check(
                    (reader.api.read)(
                        reader.device,
                        callback,
                        (&mut context as *mut Context).cast(),
                        8,
                        (SAMPLES * 2) as u32,
                    ),
                    "read",
                )?;
            }
            Ok(context)
        })
        .map_err(|e| e.to_string())
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(feature = "rtl-rust")]
#[path = "rtl_rust.rs"]
mod rust_backend;

pub struct RtlSession {
    implementation: Implementation,
    info: crate::RtlInfo,
}
enum Implementation {
    Native(NativeSession),
    #[cfg(feature = "rtl-rust")]
    Rust(rust_backend::RustSession),
}
impl RtlSession {
    pub fn open(index: u32, hz: u32, rate: u32) -> Result<Self, String> {
        Self::open_with(crate::RtlBackend::default(), index, hz, rate)
    }
    pub fn open_with(
        backend: crate::RtlBackend,
        index: u32,
        hz: u32,
        rate: u32,
    ) -> Result<Self, String> {
        if !(sdr_core::RTL_MIN_HZ..=sdr_core::RX_MAX_HZ).contains(&hz) || rate != 2_400_000 {
            return Err("RTL requires normal tuner coverage at 2.4 MS/s".into());
        }
        let (implementation, gain) = match backend {
            crate::RtlBackend::Librtlsdr => {
                let session = NativeSession::open(index, hz, rate)?;
                // SAFETY: getter accesses the configured device; no tuner writes occur during reads.
                let gain = unsafe { (session.native.api.actual_gain)(session.native.device) };
                (Implementation::Native(session), Some(gain))
            }
            crate::RtlBackend::Rust => {
                #[cfg(feature = "rtl-rust")]
                {
                    let (session, gain) = rust_backend::RustSession::open(index, hz, rate)?;
                    (Implementation::Rust(session), gain)
                }
                #[cfg(not(feature = "rtl-rust"))]
                return Err(
                    "Rust RTL backend was not compiled in; enable rtl-rust or desktop".into(),
                );
            }
        };
        Ok(Self {
            implementation,
            info: crate::RtlInfo {
                backend,
                center_hz: hz,
                sample_rate: rate,
                gain_tenth_db: gain,
            },
        })
    }
    pub fn info(&self) -> crate::RtlInfo {
        self.info.clone()
    }
    pub fn retune(&mut self, hz: u32) -> Result<(), String> {
        match &mut self.implementation {
            Implementation::Native(s) => s.retune(hz)?,
            #[cfg(feature = "rtl-rust")]
            Implementation::Rust(s) => s.retune(hz)?,
        }
        self.info.center_hz = hz;
        Ok(())
    }
    pub fn receive(&self, timeout: Duration) -> Result<RtlBlock, String> {
        match &self.implementation {
            Implementation::Native(s) => s.receive(timeout),
            #[cfg(feature = "rtl-rust")]
            Implementation::Rust(s) => s.receive(timeout),
        }
    }
    pub fn dropped_samples(&self) -> u64 {
        match &self.implementation {
            Implementation::Native(s) => s.dropped_samples(),
            #[cfg(feature = "rtl-rust")]
            Implementation::Rust(s) => s.dropped.load(Ordering::Relaxed),
        }
    }
    pub fn retune_discarded_samples(&self) -> u64 {
        match &self.implementation {
            Implementation::Native(s) => s.retune_discarded_samples(),
            #[cfg(feature = "rtl-rust")]
            Implementation::Rust(s) => s.discarded.load(Ordering::Relaxed),
        }
    }
    pub fn stop(&mut self) -> Result<(), String> {
        match &mut self.implementation {
            Implementation::Native(s) => s.stop(),
            #[cfg(feature = "rtl-rust")]
            Implementation::Rust(s) => s.stop(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_delivery_preserves_sample_positions_across_overflow_and_retune() {
        let (recycle, free) = mpsc::sync_channel(1);
        recycle.send(vec![Iq::default(); SAMPLES]).unwrap();
        let (ready, output) = mpsc::sync_channel(1);
        let mut ctx = Context {
            retuning: Arc::new(AtomicBool::new(false)),
            retune_discarded: Arc::new(AtomicU64::new(0)),
            free,
            recycle,
            ready,
            index: 0,
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let bytes = [0, 255, 127, 128];
        deliver(&bytes, &mut ctx);
        deliver(&bytes, &mut ctx); // No free buffer; count loss, never block.
        let first = output.try_recv().unwrap();
        assert_eq!(first.sample_index, 0);
        assert_eq!(first.samples()[0].i, -127.5 / 128.0);
        drop(first);
        deliver(&bytes, &mut ctx);
        let after_gap = output.try_recv().unwrap();
        assert_eq!(after_gap.sample_index, 4);
        drop(after_gap);
        assert_eq!(ctx.dropped.load(Ordering::Relaxed), 2);
        ctx.retuning.store(true, Ordering::Release);
        deliver(&bytes, &mut ctx);
        assert!(output.try_recv().is_err());
        assert_eq!(ctx.retune_discarded.load(Ordering::Relaxed), 2);
        ctx.retuning.store(false, Ordering::Release);
        deliver(&bytes, &mut ctx);
        assert_eq!(output.try_recv().unwrap().sample_index, 8);
    }
}
