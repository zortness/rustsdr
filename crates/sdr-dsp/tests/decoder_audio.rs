use sdr_dsp::DecoderAudio;
fn tone_rms(hz: f64) -> f64 {
    let mut filter = DecoderAudio::default();
    let mut power = 0.0;
    let mut count = 0;
    for n in 0..48000 {
        let sample = (std::f64::consts::TAU * hz * n as f64 / 48000.0).sin() as f32 * 0.4;
        if let Some(v) = filter.push(sample)
            && n > 1000
        {
            power += (v as f64 / 32768.0).powi(2);
            count += 1;
        }
    }
    (power / count as f64).sqrt()
}
#[test]
fn decoder_audio_preserves_passband_and_rejects_aliases() {
    let pass = tone_rms(1500.0);
    assert!((0.27..0.29).contains(&pass));
    assert!(tone_rms(8000.0) / pass < 0.001);
}
