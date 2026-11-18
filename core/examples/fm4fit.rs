//! `fm4fit FIRMWARE.fwsc [ACTION...] play:NOTE:VEL:HOLD`: drives a 4-operator firmware (OPERATOR) in the emulator with fwprobe's actions (`enc:NAME:CLICKS`,
//! `key:NAME`, `wait:S`, `midi:HEX,...`, `shot:FILE.png`) and then `play:NOTE:VEL:HOLD`, which plays a MIDI note over the jack and prints what it
//! sounded like: the level, the fundamental, and the first 12 partials in dB relative to the strongest (docs/PORTING.md). The firmware is read as data.
use fm1ntt::{devices, fm4, engine::Engine, fm4::Fm4, machine::Machine};
use std::{f64::consts::PI, fs, process::ExitCode};

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        for i in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (a, b) = (i + k, i + k + len / 2);
                let (tr, ti) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// The amplitude (0 to 1 of full scale, as a sine's peak) of the spectrum near `f` (the largest bin within 1.5%), from a Hann window of 32768 samples at `at`.
fn partial(x: &[f64], at: usize, f: f64) -> (f64, f64) {
    const N: usize = 32768;
    let mut re = vec![0.0; N];
    let mut im = vec![0.0; N];
    for (i, r) in re.iter_mut().enumerate() {
        *r = x.get(at + i).copied().unwrap_or(0.0) * (0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos());
    }
    fft(&mut re, &mut im);
    let bin = |hz: f64| hz * N as f64 / 44_100.0;
    let (lo, hi) = ((bin(f * 0.985)).floor() as usize, (bin(f * 1.015)).ceil() as usize);
    let mut best = (0.0, f);
    for k in lo.max(1)..hi.min(N / 2 - 1) {
        let a = re[k].hypot(im[k]) / (N as f64 / 4.0);
        if a > best.0 {
            best = (a, k as f64 * 44_100.0 / N as f64);
        }
    }
    best
}

/// The amplitude of a sine at exactly `f` Hz in 32768 samples from `at` (a single-frequency Fourier sum, Hann-weighted).
fn tone_amp(x: &[f64], at: usize, f: f64) -> f64 {
    const N: usize = 32768;
    let (mut c, mut s) = (0.0, 0.0);
    let w = 2.0 * PI * f / 44_100.0;
    for i in 0..N {
        let v = x.get(at + i).copied().unwrap_or(0.0) * (0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos());
        c += v * (w * i as f64).cos();
        s += v * (w * i as f64).sin();
    }
    c.hypot(s) / (N as f64 / 4.0)
}

fn partial_wide(x: &[f64], at: usize, lo: f64, hi: f64) -> (f64, f64) {
    let steps = 120;
    let mut best = (0.0, lo);
    for i in 0..=steps {
        let f = lo * (hi / lo).powf(i as f64 / steps as f64);
        let r = partial(x, at, f);
        if r.0 > best.0 {
            best = r;
        }
    }
    // refine: a Hann window of 2^15 points has 1.35 Hz bins, so look at the sine's frequency by a phase-free fit around the peak
    let (mut lo2, mut hi2) = (best.1 * 0.99, best.1 * 1.01);
    let mut bf = best.1;
    let mut ba = 0.0;
    for _ in 0..40 {
        let step = (hi2 - lo2) / 20.0;
        for i in 0..=20 {
            let f = lo2 + step * i as f64;
            let a = tone_amp(x, at, f);
            if a > ba {
                ba = a;
                bf = f;
            }
        }
        lo2 = bf - step;
        hi2 = bf + step;
        if step < 0.0005 {
            break;
        }
    }
    (ba, bf)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else { eprintln!("usage: fm4fit FIRMWARE.fwsc ACTION... play:NOTE:VEL:HOLD"); return ExitCode::from(2) };
    let Ok(bytes) = fs::read(file) else { eprintln!("{file}: cannot read"); return ExitCode::from(1) };
    let Ok(mut m) = Machine::from_package(&bytes) else { eprintln!("not a package"); return ExitCode::from(1) };
    m.set_jit(true);
    let mut rec: Vec<f64> = Vec::new();
    let run = |m: &mut Machine, secs: f64, rec: Option<&mut Vec<f64>>| {
        let target = m.ms() + (secs * 1000.0) as u64;
        let mut out = vec![0i16; 2 * 1024];
        let mut keep = rec;
        let mut guard = 0;
        while m.ms() < target && !m.halted() && guard < 1_000_000 {
            guard += 1;
            let n = m.render(&mut out, 2_000_000);
            if let Some(r) = keep.as_deref_mut() {
                r.extend((0..n).map(|i| (out[2 * i] as f64 + out[2 * i + 1] as f64) * 0.5 / 32768.0));
            }
        }
    };
    run(&mut m, 3.0, None);
    for a in &args[1..] {
        let f: Vec<&str> = a.split(':').collect();
        match f[0] {
            "wait" => run(&mut m, f[1].parse().unwrap_or(0.5), None),
            "enc" => {
                let Some(e) = devices::ENCODER_NAMES.iter().position(|n| *n == f[1]) else { eprintln!("no encoder {}", f[1]); return ExitCode::from(2) };
                m.encoder(e as u8, f.get(2).and_then(|c| c.parse().ok()).unwrap_or(1));
                run(&mut m, 0.4, None);
            }
            "key" => {
                let Some(k) = devices::BUTTON_NAMES.iter().position(|n| *n == f[1]) else { eprintln!("no button {}", f[1]); return ExitCode::from(2) };
                m.key(k as u8, true);
                run(&mut m, 0.15, None);
                m.key(k as u8, false);
                run(&mut m, 0.4, None);
            }
            "midi" => {
                for h in f[1].split(',') {
                    m.midi_in(u8::from_str_radix(h, 16).unwrap_or(0));
                }
                run(&mut m, 0.3, None);
            }
            "shot" => {
                let mut px = vec![0u32; 240 * 240];
                m.screen(&mut px);
                let mut s = String::from("P6\n240 240\n255\n").into_bytes();
                for p in px {
                    s.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
                }
                let _ = fs::write(f[1], s);
            }
            "play" | "fm4" => {
                let off = usize::from(f[0] == "fm4");
                let note: u8 = f[1 + off].parse().unwrap_or(57);
                let vel: u8 = f.get(2 + off).and_then(|v| v.parse().ok()).unwrap_or(127);
                let hold: f64 = f.get(3 + off).and_then(|v| v.parse().ok()).unwrap_or(2.0);
                rec.clear();
                if f[0] == "fm4" {
                    // `fm4:ALG:NOTE:VEL:HOLD` (ALG 1 to 11): our engine set up like OPERATOR with every operator open at full level and full sustain, for the same analysis
                    let alg: i32 = f[1].parse().unwrap_or(1);
                    let mut e = Fm4::new(44_100.0);
                    e.set(fm4::P_ALGORITHM, alg - 1);
                    e.set(fm4::P_LEVEL, 16);
                    e.set(fm4::P_INDEX, 628);
                    for op in 0..fm4::OPS {
                        for (field, v) in [(fm4::O_ATTACK, 1), (fm4::O_DECAY, 998), (fm4::O_SUSTAIN, 99), (fm4::O_RELEASE, 399), (fm4::O_VELOCITY, 50), (fm4::O_LEVEL, 99)] {
                            e.set(fm4::op_param(op, field), v);
                        }
                    }
                    e.pitch_bend(2180); // OPERATOR sounds 146.8 cents flat here
                    e.note_on(note, vel);
                    let mut blk = vec![0i16; 441];
                    for i in 0..((hold + 1.5) * 100.0) as usize {
                        if i == (hold * 100.0) as usize {
                            e.note_off(note);
                        }
                        e.render(&mut blk);
                        rec.extend(blk.iter().map(|&v| v as f64 / 32768.0));
                    }
                } else {
                    for b in [0x90, note, vel] {
                        m.midi_in(b);
                    }
                    run(&mut m, hold, Some(&mut rec));
                    for b in [0x80, note, 0] {
                        m.midi_in(b);
                    }
                    run(&mut m, 1.5, Some(&mut rec));
                }
                let mut f0 = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
                let at = (0.6 * 44_100.0) as usize;
                // the true fundamental: the strongest peak in 0.5x to 2x of nominal, so a firmware's tuning offset does not hide it
                let wide = partial_wide(&rec, at, f0 * 0.5, f0 * 2.0);
                println!("  strongest peak {:.3} Hz amp {:.4}", wide.1, wide.0);
                f0 = wide.1;
                let seg = &rec[at.min(rec.len())..(at + 32768).min(rec.len())];
                let peak = seg.iter().fold(0.0f64, |a, &v| a.max(v.abs()));
                let (a1, fr) = partial(&rec, at, f0);
                println!("note {note} vel {vel}: peak {:.4} ({:.1} dBFS), fundamental {:.3} Hz (nominal {:.3}, {:+.2} cents) amp {:.4}", peak, 20.0 * peak.max(1e-9).log10(), fr, f0, 1200.0 * (fr / f0).log2(), a1);
                let mut line = String::from("  partials dB re fundamental:");
                for h in 2..=12 {
                    let (a, _) = partial(&rec, at, f0 * h as f64);
                    line += &format!(" {h}:{:.1}", 20.0 * (a.max(1e-9) / a1.max(1e-9)).log10());
                }
                println!("{line}");
                // the envelope: the RMS in 100 ms blocks over the hold and release
                let blocks: Vec<String> = rec.chunks(4410).take(((hold + 1.5) * 10.0) as usize).map(|c| format!("{:.0}", 20.0 * ((c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt().max(1e-6)).log10())).collect();
                println!("  rms dBFS per 100 ms: {}", blocks.join(" "));
            }
            x => {
                eprintln!("unknown action {x}");
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::SUCCESS
}
