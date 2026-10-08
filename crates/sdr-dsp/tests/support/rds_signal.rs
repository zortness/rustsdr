use std::f64::consts::TAU;
/// A generated, receive-only IQ fixture. No device or transmitter is opened.
#[allow(dead_code)]
pub fn write_recording(path: &std::path::Path, repeats: usize) {
    use std::io::Write;
    let signal = multiplex(&bits(&program().repeat(repeats)), 15.0, 30.0);
    let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut phase = 0.0f64;
    for value in signal {
        for _ in 0..10 {
            phase = (phase + TAU * (-250_000.0 + 75_000.0 * value as f64) / 2_400_000.0)
                .rem_euclid(TAU);
            file.write_all(&(0.6 * phase.cos() as f32).to_le_bytes())
                .unwrap();
            file.write_all(&(0.6 * phase.sin() as f32).to_le_bytes())
                .unwrap();
        }
    }
}
pub fn bits(groups: &[[u16; 4]]) -> Vec<bool> {
    let mut result = Vec::new();
    for words in groups {
        for (i, &word) in words.iter().enumerate() {
            let offset = [
                0x0fc,
                0x198,
                if words[1] & 0x800 != 0 { 0x350 } else { 0x168 },
                0x1b4,
            ][i];
            let mut remainder = 0u16;
            for n in (0..26).rev() {
                remainder = (remainder << 1) | (((word as u32) << 10 >> n) & 1) as u16;
                if remainder & 0x400 != 0 {
                    remainder ^= 0x5b9;
                }
            }
            let block = ((word as u32) << 10) | u32::from(remainder ^ offset);
            result.extend((0..26).rev().map(|n| block & (1 << n) != 0));
        }
    }
    result
}

pub fn program() -> Vec<[u16; 4]> {
    let mut groups = Vec::new();
    for i in 0..4 {
        groups.push([
            0x1234,
            0x400 | (10 << 5) | i,
            0,
            u16::from_be_bytes(
                [b'R', b'U', b'S', b'T', b' ', b'F', b'M', b' ']
                    [i as usize * 2..i as usize * 2 + 2]
                    .try_into()
                    .unwrap(),
            ),
        ]);
    }
    for (i, chunk) in b"Hello radio!\r   ".chunks_exact(4).enumerate() {
        groups.push([
            0x1234,
            0x2400 | (10 << 5) | i as u16,
            u16::from_be_bytes([chunk[0], chunk[1]]),
            u16::from_be_bytes([chunk[2], chunk[3]]),
        ]);
    }
    groups
}

pub fn multiplex(bits: &[bool], carrier_offset: f64, ppm: f64) -> Vec<f32> {
    let mut differential = false;
    let symbols: Vec<f64> = bits
        .iter()
        .map(|&bit| {
            differential ^= bit;
            if differential { -1.0 } else { 1.0 }
        })
        .collect();
    let rate = 1187.5 * (1.0 + ppm * 1e-6);
    let count = (symbols.len() as f64 / rate * 240_000.0) as usize;
    (0..count)
        .map(|n| {
            let t = n as f64 / 240_000.0;
            let position = t * rate;
            let symbol = symbols[position as usize];
            let biphase = if position.fract() < 0.5 {
                symbol
            } else {
                -symbol
            };
            (0.055 * biphase * (TAU * (57_000.0 + carrier_offset) * t + 1.13).cos()
                + 0.3 * (TAU * 1000.0 * t).sin()
                + 0.1 * (TAU * 19_000.0 * t).cos()
                + 0.1 * (TAU * 38_000.0 * t).cos() * (TAU * 2200.0 * t).sin()) as f32
        })
        .collect()
}
