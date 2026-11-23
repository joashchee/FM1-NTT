//! `pcmap FIRMWARE.fwsc CHANNEL PROGRAMS [stock|-] [msfa|felucca|stock]`  (the 4th argument `stock` reads the stock firmware's bank from the running machine): which of the port's voices does each Program Change of a firmware play? For each program 0..PROGRAMS it plays
//! note 60 on the firmware and every voice of the port's `fm6` and compares the timbre (the first 12 harmonics, in dB relative to the strongest) and, separately, the level,
//! then prints the best voice, its timbre distance (dB, mean over the audible harmonics) and the level difference. A small distance with a constant level offset means the
//! program is that voice and the output stage differs; a large one means no voice here is what the program plays.
use fm1ntt::{
    engine::{Engine, Fm6Engine},
    port::{self, Model},
    verify,
};

fn harmonics(x: &[f64], f0: f64) -> Vec<f64> {
    let (from, n) = (x.len() / 8, x.len() * 6 / 8);
    let seg = &x[from..from + n];
    let amp = |hz: f64| {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, v) in seg.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let ph = std::f64::consts::TAU * hz * i as f64 / 44_100.0;
            re += v * w * ph.cos();
            im += v * w * ph.sin();
        }
        (re * re + im * im).sqrt() / n as f64
    };
    // the strongest of the first 12 harmonics, within a few cents of where the note should be (the firmware's pitch may be a little off), as a dB profile
    let a: Vec<f64> = (1..=12).map(|k| (-6..=6).map(|c| amp(f0 * k as f64 * 2f64.powf(c as f64 * 2.0 / 1200.0))).fold(0.0, f64::max)).collect();
    let top = a.iter().cloned().fold(1e-12, f64::max);
    a.iter().map(|v| 20.0 * (v / top).max(1e-4).log10()).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fw = std::fs::read(&args[0]).expect("read");
    let ch: u8 = args[1].parse().unwrap();
    let n: u8 = args[2].parse().unwrap();
    let model = match args.get(4).or(args.get(3)).map(String::as_str) {
        Some("felucca") => Model::Felucca { dtun: 0 },
        Some("stock") => Model::Stock,
        _ => Model::Msfa,
    };
    let (pkg, _) = if args.get(3).map(String::as_str) == Some("stock") { port::port_stock(&fw, "x", "x", "MIT").expect("port") } else { port::port_fm6(&fw, "x", "x", "MIT", model).expect("port") };
    let f0 = verify::note_hz(60);
    let native: Vec<(Vec<f64>, f64)> = pkg
        .voices
        .iter()
        .map(|v| {
            let mut e = Fm6Engine::new(44_100.0, v).unwrap();
            for &(id, val) in &pkg.params {
                e.set(id, val);
            }
            let x = verify::render_note(&mut e, 60, 100, 2.0);
            (harmonics(&x, f0), verify::rms_db(&x))
        })
        .collect();
    for p in 0..n {
        let Ok(r) = verify::emulator_note(&fw, ch, Some(p), &[], 60, 100, 2.0) else { continue };
        let (hr, lr) = (harmonics(&r, f0), verify::rms_db(&r));
        let mut best = (f64::MAX, 0usize);
        for (v, (hn, _)) in native.iter().enumerate() {
            let d = hr.iter().zip(hn).map(|(a, b)| (a - b).abs()).sum::<f64>() / 12.0;
            if d < best.0 {
                best = (d, v);
            }
        }
        println!("program {p:2}: level {lr:6.1} dB; best voice {} timbre {:.1} dB, level difference {:+.1} dB", best.1, best.0, native[best.1].1 - lr);
    }
}
