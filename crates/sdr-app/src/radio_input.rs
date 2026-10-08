//! Radio AF capture must bypass speech enhancement and noise suppression.
//! Windows uses per-stream WASAPI RAW; other platforms retain CPAL capture.
use cpal::traits::DeviceTrait;
#[cfg(not(windows))]
use cpal::traits::StreamTrait;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub struct RadioInput {
    gaps: Arc<AtomicU64>,
    #[cfg(windows)]
    worker: windows_raw::Worker,
    #[cfg(not(windows))]
    _stream: cpal::Stream,
}
impl RadioInput {
    pub fn open(
        device: &cpal::Device,
        format: cpal::SupportedStreamConfig,
        samples: impl FnMut(&[f32]) + Send + 'static,
        failure: impl FnMut() + Send + 'static,
    ) -> Result<Self, String> {
        if format.sample_format() != cpal::SampleFormat::F32
            || format.sample_rate() != 48_000
            || !(1..=2).contains(&format.channels())
        {
            return Err("Radio input requires 48 kHz mono/stereo float32".into());
        }
        let gaps = Arc::new(AtomicU64::new(0));
        #[cfg(windows)]
        {
            let id = device.id().map_err(|e| e.to_string())?.1;
            let worker =
                windows_raw::Worker::open(id, format.channels(), gaps.clone(), samples, failure)?;
            Ok(Self { gaps, worker })
        }
        #[cfg(not(windows))]
        {
            let mut samples = samples;
            let mut failure = failure;
            let stream = device
                .build_input_stream(
                    &format.into(),
                    move |v: &[f32], _| samples(v),
                    move |_| failure(),
                    None,
                )
                .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            Ok(Self {
                gaps,
                _stream: stream,
            })
        }
    }
    /// Driver-reported discontinuities, not a count of missing samples.
    pub fn discontinuities(&self) -> u64 {
        self.gaps.load(Ordering::Acquire)
    }
}
#[cfg(windows)]
impl Drop for RadioInput {
    fn drop(&mut self) {
        self.worker.stop();
    }
}

#[cfg(windows)]
mod windows_raw {
    use super::*;
    use std::{
        sync::{atomic::AtomicBool, mpsc},
        thread::{self, JoinHandle},
        time::Duration,
    };
    use windows::{
        Win32::{Media::Audio::*, System::Com::*},
        core::{GUID, HSTRING, Interface},
    };

    pub(super) struct Worker {
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }
    impl Worker {
        pub fn open(
            id: String,
            channels: u16,
            gaps: Arc<AtomicU64>,
            mut samples: impl FnMut(&[f32]) + Send + 'static,
            mut failure: impl FnMut() + Send + 'static,
        ) -> Result<Self, String> {
            let stop = Arc::new(AtomicBool::new(false));
            let cancel = stop.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            let join = thread::Builder::new()
                .name("radio-raw-audio".into())
                .spawn(move || {
                    let result = (|| -> Result<(), String> {
                        let _com = Apartment::new()?;
                        let mut input = Capture::open(&id, channels)?;
                        let _ = sender.send(Ok(()));
                        while !cancel.load(Ordering::Acquire) {
                            input
                                .drain(&mut samples, &gaps)
                                .map_err(|e| e.to_string())?;
                            thread::sleep(Duration::from_millis(2));
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        let _ = sender.try_send(Err(error));
                        failure();
                    }
                })
                .map_err(|e| e.to_string())?;
            let mut worker = Self {
                stop,
                join: Some(join),
            };
            match receiver.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(())) => Ok(worker),
                result => {
                    worker.stop();
                    Err(format!(
                        "Unprocessed radio capture could not start: {}",
                        match result {
                            Ok(Err(e)) => e,
                            Err(e) => e.to_string(),
                            _ => unreachable!(),
                        }
                    ))
                }
            }
        }
        pub fn stop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }
    struct Apartment;
    impl Apartment {
        fn new() -> Result<Self, String> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED)
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
            Ok(Self)
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    // Validate the driver format before treating packet bytes as float samples.
    fn valid_float_format(format: &WAVEFORMATEX, channels: u16, subtype: Option<GUID>) -> bool {
        (1..=2).contains(&channels)
            && format.nSamplesPerSec == 48_000
            && format.nChannels == channels
            && format.wBitsPerSample == 32
            && format.nBlockAlign == channels * 4
            && match format.wFormatTag {
                3 => true,
                0xfffe => {
                    format.cbSize >= 22
                        && subtype == Some(GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71))
                }
                _ => false,
            }
    }
    struct Capture {
        audio: IAudioClient,
        capture: IAudioCaptureClient,
        channels: usize,
        zeros: Vec<f32>,
        first: bool,
    }
    impl Capture {
        fn open(id: &str, channels: u16) -> Result<Self, String> {
            unsafe {
                let result = (|| -> windows::core::Result<Self> {
                    let enumerator: IMMDeviceEnumerator =
                        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                    let device = enumerator.GetDevice(&HSTRING::from(id))?;
                    let audio: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
                    let client: IAudioClient2 = audio.cast()?;
                    client.SetClientProperties(&AudioClientProperties {
                        cbSize: std::mem::size_of::<AudioClientProperties>() as u32,
                        bIsOffload: false.into(),
                        eCategory: AudioCategory_Other,
                        Options: AUDCLNT_STREAMOPTIONS_RAW,
                    })?;
                    let format = audio.GetMixFormat()?;
                    let subtype = if (*format).wFormatTag == 0xfffe && (*format).cbSize >= 22 {
                        Some((*format.cast::<WAVEFORMATEXTENSIBLE>()).SubFormat)
                    } else {
                        None
                    };
                    let valid = valid_float_format(&*format, channels, subtype);
                    let init = if valid {
                        audio.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1_000_000, 0, format, None)
                    } else {
                        Err(windows::core::Error::new(
                            windows::core::HRESULT(0x80070057u32 as i32),
                            "Radio mix format changed; select 48 kHz mono/stereo float32",
                        ))
                    };
                    CoTaskMemFree(Some(format.cast()));
                    init?;
                    let capture: IAudioCaptureClient = audio.GetService()?;
                    let zeros = vec![0.0; audio.GetBufferSize()? as usize * usize::from(channels)];
                    audio.Start()?;
                    Ok(Self {
                        audio,
                        capture,
                        channels: usize::from(channels),
                        zeros,
                        first: true,
                    })
                })();
                result.map_err(|e| e.to_string())
            }
        }
        fn drain(
            &mut self,
            samples: &mut impl FnMut(&[f32]),
            gaps: &AtomicU64,
        ) -> windows::core::Result<()> {
            unsafe {
                for _ in 0..32 {
                    if self.capture.GetNextPacketSize()? == 0 {
                        break;
                    }
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let mut frames = 0;
                    let mut flags = 0;
                    self.capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let packet = Packet {
                        capture: &self.capture,
                        frames,
                    };
                    let count = frames as usize * self.channels;
                    if !self.first && flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0 {
                        gaps.fetch_add(1, Ordering::Release);
                    }
                    self.first = false;
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        for zero_chunk in (0..count).step_by(self.zeros.len().max(1)) {
                            samples(&self.zeros[..(count - zero_chunk).min(self.zeros.len())]);
                        }
                    } else if !data.is_null() && count > 0 {
                        samples(std::slice::from_raw_parts(data.cast::<f32>(), count));
                    }
                    drop(packet);
                }
                Ok(())
            }
        }
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            unsafe {
                let _ = self.audio.Stop();
            }
        }
    }
    struct Packet<'a> {
        capture: &'a IAudioCaptureClient,
        frames: u32,
    }
    impl Drop for Packet<'_> {
        fn drop(&mut self) {
            unsafe {
                let _ = self.capture.ReleaseBuffer(self.frames);
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn capture_accepts_float_mono_stereo_but_rejects_integer_and_changed_formats() {
            for channels in [1, 2] {
                let mut f = WAVEFORMATEX {
                    wFormatTag: 3,
                    nChannels: channels,
                    nSamplesPerSec: 48_000,
                    wBitsPerSample: 32,
                    nBlockAlign: channels * 4,
                    ..Default::default()
                };
                assert!(valid_float_format(&f, channels, None));
                f.wFormatTag = 1;
                assert!(!valid_float_format(&f, channels, None)); // 32-bit integer is not float.
                f.wFormatTag = 0xfffe;
                f.cbSize = 22;
                let float = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
                assert!(valid_float_format(&f, channels, Some(float)));
                assert!(!valid_float_format(
                    &f,
                    channels,
                    Some(GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71))
                ));
                f.cbSize = 0;
                assert!(!valid_float_format(&f, channels, Some(float)));
                f.wFormatTag = 3;
                f.nSamplesPerSec = 44_100;
                assert!(!valid_float_format(&f, channels, None));
                f.nSamplesPerSec = 48_000;
                f.nBlockAlign = 1;
                assert!(!valid_float_format(&f, channels, None));
            }
        }
    }
}
