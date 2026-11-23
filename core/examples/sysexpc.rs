//! `sysexpc FIRMWARE.fwsc`: does a SysEx parameter write survive a Program Change? Program 0, write the algorithm (parameter 134 = 31) over the jack, play note 60 (A); then
//! Program Change away and back to 0 and play it again (B); the plain voice with no write is the control (C). B near A: the write survived; B near C: it did not.
use fm1ntt::machine::Machine;

const NOTE: u8 = 60;

fn run(m: &mut Machine, secs: f64, keep: Option<&mut Vec<f64>>) {
    let mut left = (secs * 44_100.0) as usize;
    let mut buf = vec![0i16; 2 * 2048];
    let mut keep = keep;
    while left > 0 {
        let n = left.min(2048);
        let got = m.render(&mut buf[..2 * n], 8_000_000);
        if let Some(k) = keep.as_mut() {
            for f in 0..got {
                k.push((buf[2 * f] as f64 + buf[2 * f + 1] as f64) / 65536.0);
            }
        }
        left -= got.max(1).min(left);
    }
}

fn send(m: &mut Machine, bytes: &[u8]) {
    for &b in bytes {
        m.midi_in(b);
        // a real wire is 31250 baud: a byte is 0.32 ms
        run(m, 0.0004, None);
    }
}

fn note(m: &mut Machine) -> Vec<f64> {
    send(m, &[0x90, NOTE, 100]);
    let mut out = Vec::new();
    run(m, 1.5, Some(&mut out));
    send(m, &[0x80, NOTE, 0]);
    run(m, 0.6, None);
    out
}

/// The first 12 harmonics of the held note in dB below the strongest (a timbre, not a waveform: a run that starts a moment later has another phase).
fn timbre(x: &[f64]) -> Vec<f64> {
    let (from, n) = (x.len() / 8, x.len() * 6 / 8);
    let f0 = 261.63;
    let a: Vec<f64> = (1..=12)
        .map(|k| {
            (-8..=8)
                .map(|c| {
                    let hz = f0 * k as f64 * 2f64.powf(c as f64 * 2.0 / 1200.0);
                    let (mut re, mut im) = (0.0, 0.0);
                    for (i, v) in x[from..from + n].iter().enumerate() {
                        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
                        let ph = std::f64::consts::TAU * hz * i as f64 / 44_100.0;
                        re += v * w * ph.cos();
                        im += v * w * ph.sin();
                    }
                    (re * re + im * im).sqrt() / n as f64
                })
                .fold(0.0, f64::max)
        })
        .collect();
    let top = a.iter().cloned().fold(1e-12, f64::max);
    a.iter().map(|v| 20.0 * (v / top).max(1e-4).log10()).collect()
}

fn dist(a: &[f64], b: &[f64]) -> f64 {
    let (x, y) = (timbre(a), timbre(b));
    x.iter().zip(&y).map(|(p, q)| (p - q).abs()).sum::<f64>() / 12.0
}

fn main() {
    let pkg = std::fs::read(std::env::args().nth(1).expect("firmware")).expect("read");
    let boot = || {
        let mut m = Machine::from_package(&pkg).expect("load");
        m.set_jit(true);
        run(&mut m, 3.0, None);
        send(&mut m, &[0xC0, 0]);
        run(&mut m, 0.3, None);
        m
    };
    let mut c = boot();
    let control = note(&mut c);
    let mut m = boot();
    send(&mut m, &[0xF0, 0x43, 0x10, 0x01, 0x06, 31, 0xF7]); // parameter 1 x 128 + 6 = 134: the algorithm
    run(&mut m, 0.3, None);
    let written = note(&mut m);
    send(&mut m, &[0xC0, 5]);
    run(&mut m, 0.3, None);
    send(&mut m, &[0xC0, 0]);
    run(&mut m, 0.3, None);
    let after = note(&mut m);
    println!("written vs control {:.1} dB (mean difference of the first 12 harmonics; audible if over a few dB)", dist(&written, &control));
    println!("after a Program Change away and back: vs written {:.1} dB, vs control {:.1} dB", dist(&after, &written), dist(&after, &control));
}
