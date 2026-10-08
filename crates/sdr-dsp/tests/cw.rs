use sdr_dsp::cw::{CwConfig, CwDecoder};

fn run(decoder: &mut CwDecoder, on: bool, ms: usize) {
    let samples = vec![if on { 0.25 } else { 0.000001 }; ms * 48];
    for block in samples.chunks(137) {
        decoder.process(block);
    }
}

fn send_group(decoder: &mut CwDecoder, pattern: &str) {
    for (i, symbol) in pattern.chars().enumerate() {
        let jitter = [0.9, 1.1, 0.95, 1.05][i % 4];
        run(
            decoder,
            true,
            ((if symbol == '-' { 180.0 } else { 60.0 }) * jitter) as usize,
        );
        run(decoder, false, 60);
    }
    run(decoder, false, 120);
}

#[test]
fn joined_bk_is_a_prosign_but_spaced_letters_remain_bk() {
    let mut decoder = CwDecoder::new(CwConfig::default()).unwrap();
    send_group(&mut decoder, "-...-.-");
    run(&mut decoder, false, 400);
    send_group(&mut decoder, "-...");
    send_group(&mut decoder, "-.-");
    assert_eq!(decoder.status().text, "<BK> BK");
    assert_eq!(decoder.status().unknown_characters, 0);
}

#[test]
fn common_prosigns_and_punctuation_have_explicit_labels_without_guessing_unknowns() {
    let mut decoder = CwDecoder::new(CwConfig::default()).unwrap();
    for pattern in [
        ".-.-.",
        ".-...",
        "-...-",
        "...-.-",
        "-.--.",
        "-.-.-",
        ".-.-",
        "...-.",
        "........",
        "...---...",
        "..--..",
    ] {
        send_group(&mut decoder, pattern);
    }
    assert_eq!(
        decoder.status().text,
        "<AR><AS><BT><SK><KN><KA><AA><VE><HH><SOS>?"
    );
    assert_eq!(
        decoder.status().unknown_characters,
        0,
        "a transmitted question mark is recognized"
    );
    send_group(&mut decoder, "...---...."); // SOS plus an extra dit, not a valid prosign.
    assert!(decoder.status().text.ends_with("??"));
    assert_eq!(decoder.status().unknown_characters, 1);
}

#[test]
fn bounded_history_does_not_cut_a_prosign_label_in_half() {
    let mut decoder = CwDecoder::new(CwConfig::default()).unwrap();
    for _ in 0..1030 {
        send_group(&mut decoder, "-...-.-");
    }
    send_group(&mut decoder, ".");
    assert!(decoder.status().text.len() <= 4096);
    assert!(decoder.status().text.starts_with("<BK>"));
    assert!(decoder.status().text.ends_with("<BK>E"));
    assert_eq!(
        decoder.status().text.matches('<').count(),
        decoder.status().text.matches('>').count()
    );
}

#[test]
fn uneven_sending_and_long_silence_finish_without_inventing_characters() {
    let mut decoder = CwDecoder::new(CwConfig::default()).unwrap();
    run(&mut decoder, false, 500);
    // SOS, 20 WPM with irregular marks and spaces.
    for pattern in [vec![52, 68, 59], vec![165, 205, 180], vec![65, 55, 61]] {
        for (i, mark) in pattern.iter().enumerate() {
            run(&mut decoder, true, *mark);
            run(&mut decoder, false, if i == 2 { 185 } else { 62 });
        }
    }
    run(&mut decoder, false, 10_000);
    assert_eq!(decoder.status().text.trim(), "SOS");
    assert!((16.0..24.0).contains(&decoder.status().wpm));
}

#[test]
fn silence_spikes_and_a_continuous_carrier_do_not_make_text() {
    let mut decoder = CwDecoder::new(CwConfig::default()).unwrap();
    run(&mut decoder, false, 1000);
    for _ in 0..20 {
        run(&mut decoder, true, 1);
        run(&mut decoder, false, 100);
    }
    assert!(decoder.status().text.is_empty());
    run(&mut decoder, true, 3000);
    run(&mut decoder, false, 1000);
    assert!(decoder.status().text.is_empty());
}

#[test]
fn stretched_spacing_and_unknown_patterns_are_explicit() {
    let mut decoder = CwDecoder::new(CwConfig {
        spacing: 2.0,
        ..CwConfig::default()
    })
    .unwrap();
    run(&mut decoder, true, 60);
    run(&mut decoder, false, 360);
    run(&mut decoder, true, 180);
    run(&mut decoder, false, 840);
    // Eight dits is now the recognized HH/error prosign; nine is unknown.
    for _ in 0..9 {
        run(&mut decoder, true, 60);
        run(&mut decoder, false, 60);
    }
    run(&mut decoder, false, 1000);
    assert_eq!(decoder.status().text, "ET ?");
    assert_eq!(decoder.status().unknown_characters, 1);
    assert!(
        CwDecoder::new(CwConfig {
            seed_wpm: f32::NAN,
            ..CwConfig::default()
        })
        .is_err()
    );
}

#[test]
fn iq_to_text_is_independent_of_audio_squelch_and_has_a_600_hz_tone() {
    use sdr_core::Iq;
    use sdr_dsp::{Mode, Receiver, ReceiverConfig};
    for (width, squelch) in [(200, None), (500, None), (1000, None), (500, Some(0.0))] {
        let mut config = ReceiverConfig::for_mode(Mode::Cw);
        config.bandwidth_hz = width;
        config.agc = false;
        config.squelch_db = squelch;
        let mut receiver = Receiver::new(-250_000.0, config).unwrap();
        let mut decoder = CwDecoder::new(CwConfig::default()).unwrap();
        let mut sample = 0;
        let mut audio = Vec::new();
        let mut power = Vec::new();
        let mut tone = Vec::new();
        // CQ + joined BK; fixed blocks exercise stream state across transitions.
        let mut runs = vec![(false, 200)];
        for pattern in ["-.-.", "--.-", "-...-.-"] {
            for symbol in pattern.chars() {
                runs.push((true, if symbol == '-' { 180 } else { 60 }));
                runs.push((false, 60));
            }
            runs.push((false, 120));
        }
        runs.push((false, 500));
        for (on, ms) in runs {
            for _ in 0..ms {
                let iq: Vec<_> = (0..2400)
                    .map(|_| {
                        let phase =
                            -std::f64::consts::TAU * 250_000.0 * sample as f64 / 2_400_000.0;
                        sample += 1;
                        let a = if on { 0.5 } else { 0.0 };
                        Iq {
                            i: phase.cos() as f32 * a,
                            q: phase.sin() as f32 * a,
                        }
                    })
                    .collect();
                audio.clear();
                power.clear();
                receiver.process_with_cw_power(&iq, &mut audio, &mut power);
                assert_eq!(audio.len(), 48);
                decoder.process(&power);
                tone.extend(audio.iter().map(|f| f.left));
            }
        }
        assert_eq!(decoder.status().text, "CQ<BK>", "width {width}");
        let segment = &tone[48 * 270..48 * 330];
        let energy = segment.iter().map(|v| v * v).sum::<f32>() / segment.len() as f32;
        if squelch.is_some() {
            assert!(energy < 1e-10);
            continue;
        }
        assert!(energy > 0.05, "CW should be audible");
        let (mut re, mut im) = (0.0, 0.0);
        for (n, v) in segment.iter().enumerate() {
            let p = std::f64::consts::TAU * 600.0 * n as f64 / 48_000.0;
            re += *v as f64 * p.cos();
            im += *v as f64 * p.sin();
        }
        assert!(2.0 * re.hypot(im) / segment.len() as f64 > 0.4);
    }
}

#[test]
fn tracks_gradual_speed_change_with_weighted_dashes_and_brief_dropouts() {
    let mut decoder = CwDecoder::new(CwConfig::default()).unwrap();
    for n in 0..30 {
        let dot = 60.0 - n as f32 * 0.65;
        for (i, symbol) in "-.-.".chars().enumerate() {
            let duration =
                (dot * if symbol == '-' { 2.7 } else { 1.0 } * [0.9, 1.1, 0.95, 1.05][i]) as usize;
            run(&mut decoder, true, duration / 2);
            run(&mut decoder, false, 1);
            run(&mut decoder, true, duration - duration / 2 - 1);
            run(&mut decoder, false, dot as usize);
        }
        run(&mut decoder, false, (dot * 2.0) as usize);
    }
    run(&mut decoder, false, 1000);
    assert_eq!(decoder.status().text, "C".repeat(30));
    assert!(decoder.status().wpm > 26.0);
}

#[test]
fn narrow_channel_rejects_an_equal_strength_carrier_1_khz_away() {
    use sdr_core::Iq;
    use sdr_dsp::{Mode, Receiver, ReceiverConfig};
    let mut receiver = Receiver::new(0.0, ReceiverConfig::for_mode(Mode::Cw)).unwrap();
    let input: Vec<_> = (0..480_000)
        .map(|n| {
            let p = std::f64::consts::TAU * 1000.0 * n as f64 / 2_400_000.0;
            Iq {
                i: 0.5 * p.cos() as f32,
                q: 0.5 * p.sin() as f32,
            }
        })
        .collect();
    let mut power = Vec::new();
    receiver.process_with_cw_power(&input, &mut Vec::new(), &mut power);
    let mean = power[4800..].iter().sum::<f32>() / 4800.0;
    assert!(10.0 * mean.log10() < -65.0);
}

#[test]
fn transcript_is_bounded_and_incomplete_marks_are_not_flushed() {
    let mut decoder = CwDecoder::new(CwConfig {
        seed_wpm: 60.0,
        adaptive: false,
        ..CwConfig::default()
    })
    .unwrap();
    for _ in 0..4200 {
        run(&mut decoder, true, 20);
        run(&mut decoder, false, 60);
    }
    assert_eq!(decoder.status().text.len(), 4096);
    let mut incomplete = CwDecoder::new(CwConfig::default()).unwrap();
    run(&mut incomplete, true, 60);
    assert!(incomplete.status().text.is_empty());
}
