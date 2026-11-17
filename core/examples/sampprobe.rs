//! `sampprobe FIRMWARE.fwsc CHANNEL NOTE[,NOTE...] [--hold S] [--secs S] [--vel V] [--wav FILE]`: boots a firmware, plays notes on a MIDI channel (0 to 15) and
//! prints what comes out (peak, level, how long it sounds, the strongest partial), to find out whether a firmware has sample content to measure
//! (docs/SAMPLER.md). The firmware is read as data. `--wav` writes the first note's audio (16-bit mono, 48 kHz) under a temp name, then renames it.
use fm1ntt::machine::Machine;
use std::{fs, process::ExitCode};

const SR: usize = 48000;

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: sampprobe FIRMWARE.fwsc CHANNEL NOTE[,NOTE...] [--hold S] [--secs S] [--wav FILE]");
        return ExitCode::from(2);
    }
    let (mut hold, mut secs, mut wav, mut vel) = (0.3f64, 2.0f64, None::<String>, 100u8);
    let mut then: Vec<(u8, f64)> = Vec::new();   // further note-ons: NOTE@SECONDS after the first
    let mut i = 3;
    while i < a.len() {
        match a[i].as_str() {
            "--hold" => { i += 1; hold = a.get(i).and_then(|s| s.parse().ok()).unwrap_or(hold); }
            "--secs" => { i += 1; secs = a.get(i).and_then(|s| s.parse().ok()).unwrap_or(secs); }
            "--vel" => { i += 1; vel = a.get(i).and_then(|s| s.parse().ok()).unwrap_or(vel); }
            "--then" => { i += 1; if let Some((n, t)) = a.get(i).and_then(|s| s.split_once('@')) { if let (Ok(n), Ok(t)) = (n.parse(), t.parse()) { then.push((n, t)); } } }
            "--wav" => { i += 1; wav = a.get(i).cloned(); }
            _ => {}
        }
        i += 1;
    }
    let channel: u8 = a[1].parse().unwrap_or(0) & 15;
    let notes: Vec<u8> = a[2].split(',').filter_map(|s| s.parse().ok()).collect();
    let bytes = match fs::read(&a[0]) { Ok(b) => b, Err(e) => { eprintln!("{}: {e}", a[0]); return ExitCode::from(1); } };
    let mut m = match Machine::from_package(&bytes) { Ok(m) => m, Err(e) => { eprintln!("{e}"); return ExitCode::from(1); } };
    m.set_jit(true);
    let mut buf = vec![0i16; 2 * 4096];
    let mut run = |m: &mut Machine, s: f64, keep: &mut Vec<f64>| {
        let mut left = (s * SR as f64) as usize;
        let mut guard = 0;
        while left > 0 && !m.halted() && guard < 100_000 {
            guard += 1;
            let n = left.min(4096);
            let got = m.render(&mut buf[..2 * n], 8_000_000);
            for f in 0..got { keep.push((buf[2 * f] as f64 + buf[2 * f + 1] as f64) / 65536.0); }
            left -= got;
        }
    };
    let mut boot = Vec::new();
    run(&mut m, 3.0, &mut boot);
    for (k, &n) in notes.iter().enumerate() {
        let mut x = Vec::new();
        m.midi_in(0x90 | channel); m.midi_in(n); m.midi_in(vel);
        let mut t = 0.0;
        let mut later = then.clone();
        later.sort_by(|p, q| p.1.partial_cmp(&q.1).unwrap());
        for (nn, at) in later {
            if at > t { run(&mut m, at - t, &mut x); t = at; }
            m.midi_in(0x90 | channel); m.midi_in(nn); m.midi_in(vel);
        }
        if hold > t { run(&mut m, hold - t, &mut x); t = hold; }
        m.midi_in(0x80 | channel); m.midi_in(n); m.midi_in(0);
        run(&mut m, (secs - t).max(0.0), &mut x);
        if !then.is_empty() {
            let w = SR / 20;
            let line: Vec<String> = (0..x.len() / w).map(|k| format!("{:.0}", 20.0 * ((x[k * w..(k + 1) * w].iter().map(|v| v * v).sum::<f64>() / w as f64).sqrt().max(1e-9)).log10())).collect();
            println!("rms dBFS per 50 ms: {}", line.join(" "));
        }
        let peak = x.iter().fold(0.0f64, |p, v| p.max(v.abs()));
        let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt();
        // the sound lasts until the last 10 ms window above -60 dB of the peak
        let win = SR / 100;
        let last = (0..x.len() / win).rev().find(|&w| x[w * win..(w + 1) * win].iter().fold(0.0f64, |p, v| p.max(v.abs())) > peak * 0.001).map(|w| (w + 1) as f64 / 100.0).unwrap_or(0.0);
        // zero-crossing rate over the first 100 ms of sound as a pitch hint
        let start = x.iter().position(|v| v.abs() > peak * 0.1).unwrap_or(0);
        let seg = &x[start..(start + SR / 10).min(x.len())];
        let zc = seg.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f64 * 10.0;
        println!("ch {channel} note {n:3}: peak {:.3} ({:+.1} dBFS), rms {:.4}, sounds {:.2} s, starts {:.3} s after the key, rising crossings {:.0}/s", peak, 20.0 * peak.max(1e-9).log10(), rms, last, start as f64 / SR as f64, zc);
        if k == 0 {
            if let Some(path) = &wav {
                let mut d = Vec::new();
                for v in &x { d.extend_from_slice(&((v * 32767.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes()); }
                let mut f = Vec::new();
                f.extend_from_slice(b"RIFF"); f.extend_from_slice(&(36 + d.len() as u32).to_le_bytes()); f.extend_from_slice(b"WAVEfmt ");
                f.extend_from_slice(&16u32.to_le_bytes()); f.extend_from_slice(&1u16.to_le_bytes()); f.extend_from_slice(&1u16.to_le_bytes());
                f.extend_from_slice(&(SR as u32).to_le_bytes()); f.extend_from_slice(&(SR as u32 * 2).to_le_bytes()); f.extend_from_slice(&2u16.to_le_bytes()); f.extend_from_slice(&16u16.to_le_bytes());
                f.extend_from_slice(b"data"); f.extend_from_slice(&(d.len() as u32).to_le_bytes()); f.extend_from_slice(&d);
                let tmp = format!("{path}.tmp");
                if fs::write(&tmp, &f).is_ok() { let _ = fs::rename(&tmp, path); }
            }
        }
    }
    ExitCode::SUCCESS
}
