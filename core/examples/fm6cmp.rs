//! `fm6cmp FIRMWARE.fwsc [--note N] [--vel V] [--voice I] [--enc NAME:CLICKS]... [--encn NAME:CLICKS] [--pot 0-1023] [--master 0-1023] [--channel C] [--key] [--fm6-note N] [--hold S] [--out DIR]`: plays one note on a firmware in the emulator
//! and the same voice (the firmware's own bank, found by the extractor) on the fm6 engine, and compares the two (docs/FM6.md).
//! The emulator only reads the firmware as data. Prints levels, pitch, spectrum and decay; `--out` writes both as WAV for listening.
use fm1ntt::{devices, extract, fm6, machine::Machine};
use std::{f64::consts::PI, fs, path::Path, process::ExitCode};

const SR: usize = 44_100;

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

/// Magnitude spectrum (Hann window) of 16384 samples from `at`, in dB relative to full scale, bins 0..8192.
fn spectrum(x: &[f64], at: usize) -> Vec<f64> {
    const N: usize = 16384;
    let mut re = vec![0.0; N];
    let mut im = vec![0.0; N];
    for (i, r) in re.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos();
        *r = x.get(at + i).copied().unwrap_or(0.0) * w;
    }
    fft(&mut re, &mut im);
    (0..N / 2).map(|k| 20.0 * ((re[k].hypot(im[k]) / (N as f64 / 4.0)).max(1e-9)).log10()).collect()
}

/// The frequency of the strongest peak within 3% of `f` in a 65536-point (1.5 s) spectrum from `at`, parabolic interpolation: about 0.02 Hz.
fn peak_near(x: &[f64], at: usize, f: f64) -> Option<(f64, f64)> {
    const N: usize = 65536;
    let mut re = vec![0.0; N];
    let mut im = vec![0.0; N];
    for (i, r) in re.iter_mut().enumerate() { *r = x.get(at + i).copied().unwrap_or(0.0) * (0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos()); }
    fft(&mut re, &mut im);
    let mag: Vec<f64> = (0..N / 2).map(|k| re[k].hypot(im[k])).collect();
    let bin = SR as f64 / N as f64;
    let (lo, hi) = (((f * 0.97 / bin) as usize).max(2), ((f * 1.03 / bin) as usize).min(N / 2 - 2));
    let k = (lo..hi).max_by(|&a, &b| mag[a].partial_cmp(&mag[b]).unwrap())?;
    if mag[k] < 1e-4 { return None }
    let (a, b, g) = (mag[k - 1].ln(), mag[k].ln(), mag[k + 1].ln());
    Some(((k as f64 + 0.5 * (a - g) / (a - 2.0 * b + g)) * bin, 20.0 * (mag[k] / (N as f64 / 4.0)).log10()))
}

/// The two strongest peaks within 10% of `f0` in a 65536-point (1.5 s) spectrum from `at`: their frequencies in Hz, parabolic interpolation (0.67 Hz bins, so about 0.05 Hz).
fn two_peaks(x: &[f64], at: usize, f0: f64) -> Option<(f64, f64, f64)> {
    const N: usize = 65536;
    let mut re = vec![0.0; N];
    let mut im = vec![0.0; N];
    for (i, r) in re.iter_mut().enumerate() { *r = x.get(at + i).copied().unwrap_or(0.0) * (0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos()); }
    fft(&mut re, &mut im);
    let mag: Vec<f64> = (0..N / 2).map(|k| re[k].hypot(im[k])).collect();
    let bin = SR as f64 / N as f64;
    let (lo, hi) = ((f0 * 0.9 / bin) as usize, (f0 * 1.1 / bin) as usize);
    let mut pk: Vec<(usize, f64)> = (lo.max(2)..hi.min(N / 2 - 2)).filter(|&k| mag[k] > mag[k - 1] && mag[k] >= mag[k + 1] && mag[k] > 1e-5).map(|k| (k, mag[k])).collect();
    pk.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let refine = |k: usize| { let (a, b, g) = (mag[k - 1].ln(), mag[k].ln(), mag[k + 1].ln()); (k as f64 + 0.5 * (a - g) / (a - 2.0 * b + g)) * bin };
    if pk.len() < 2 { return None }
    let (a, b) = (refine(pk[0].0), refine(pk[1].0));
    Some((a.min(b), a.max(b), 20.0 * (mag[pk[1].0] / mag[pk[0].0]).log10()))
}

/// The strongest partial between `lo` and `hi` Hz in a 4096-point Hann window at `at`: its frequency (parabolic interpolation) and level in dB.
fn peak_in(x: &[f64], at: usize, lo: f64, hi: f64) -> Option<(f64, f64)> {
    const N: usize = 4096;
    let mut re = vec![0.0; N];
    let mut im = vec![0.0; N];
    for (i, r) in re.iter_mut().enumerate() { *r = x.get(at + i).copied().unwrap_or(0.0) * (0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos()); }
    fft(&mut re, &mut im);
    let mag: Vec<f64> = (0..N / 2).map(|k| re[k].hypot(im[k])).collect();
    let bin = SR as f64 / N as f64;
    let (a, b) = (((lo / bin) as usize).max(2), ((hi / bin) as usize).min(N / 2 - 2));
    let k = (a..b).max_by(|&p, &q| mag[p].partial_cmp(&mag[q]).unwrap())?;
    if mag[k] < 1e-5 { return None }
    let (l, c, r) = (mag[k - 1].ln(), mag[k].ln(), mag[k + 1].ln());
    Some(((k as f64 + 0.5 * (l - r) / (l - 2.0 * c + r)) * bin, 20.0 * (mag[k] / (N as f64 / 4.0)).log10()))
}

fn rms_db(x: &[f64]) -> f64 {
    let m = x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    10.0 * m.max(1e-18).log10()
}

fn write_wav(path: &Path, x: &[f64]) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(44 + x.len() * 2);
    let n = (x.len() * 2) as u32;
    b.extend(b"RIFF");
    b.extend((36 + n).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend([1, 0, 1, 0]);
    b.extend((SR as u32).to_le_bytes());
    b.extend((SR as u32 * 2).to_le_bytes());
    b.extend([2, 0, 16, 0]);
    b.extend(b"data");
    b.extend(n.to_le_bytes());
    for v in x {
        b.extend(((v * 32767.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes());
    }
    let tmp = path.with_extension("wav.tmp");
    fs::write(&tmp, b)?;
    fs::rename(tmp, path)
}

/// A test voice made for this comparison (original): by default one carrier on algorithm 32 so nothing modulates it, and the kind changes one thing.
/// Written into the firmware with Felucca's FM6_PUT over USB-MIDI (docs/FM6.md) and played on fm6 from the same bytes.
fn make_voice(kind: &str) -> Option<[u8; 128]> {
    #[derive(Clone, Copy)]
    struct Op { ol: u8, coarse: u8, fine: u8, det: u8, kvs: u8, bp: u8, ld: u8, rd: u8, lc: u8, rc: u8, rs: u8, ams: u8, mode: u8 }
    let (mut rates, mut levels) = ([99u8; 4], [99u8, 99, 99, 0]);
    let (mut alg, mut fb) = (31u8, 0u8);
    let mut ops = [Op { ol: 0, coarse: 1, fine: 0, det: 7, kvs: 0, bp: 39, ld: 0, rd: 0, lc: 0, rc: 0, rs: 0, ams: 0, mode: 0 }; 6];
    // the header: the LFO (speed, delay, pitch depth, amp depth, sync, wave, pitch sensitivity) and the pitch envelope (rates, levels)
    let (mut lfo, mut peg) = ([35u8, 0, 0, 0, 1, 0, 0], ([99u8; 4], [50u8; 4]));   // ops[k] is operator k + 1
    ops[0].ol = 99;
    if let Some(rest) = kind.strip_prefix("solo") {
        // "solo32:K" or "solo5:K": operator K alone, as a carrier, on algorithm 32 or algorithm 5 (coarse 1, detune 7)
        let (a, k) = rest.split_once(':')?;
        alg = match a { "32" => 31, "5" => 4, _ => return None };
        ops[0].ol = 0;
        ops[k.parse::<usize>().ok().filter(|k| (1..=6).contains(k))? - 1].ol = 99;
    } else if let Some(rest) = kind.strip_prefix("pair") {
        // "pair12" "pair34" "pair56" (optionally ":D"): algorithm 5, the second operator modulates the first, both at detune D (7 by default), the others silent
        let (pr, d) = rest.split_once(':').map(|(a, b)| (a, b.parse().unwrap_or(7u8))).unwrap_or((rest, 7));
        let (c, m) = match pr { "12" => (0, 1), "34" => (2, 3), "56" => (4, 5), _ => return None };
        alg = 4;
        ops[0].ol = 0;
        ops[c].ol = 99; ops[m].ol = 75;
        ops[c].det = d.min(14); ops[m].det = d.min(14);
    } else if let Some(d) = kind.strip_prefix("six:") {
        let d: u8 = d.parse().ok().filter(|d| *d <= 14)?;
        for (k, op) in ops.iter_mut().enumerate() { op.ol = 85; op.coarse = k as u8 + 1; op.det = d; }
        // fall through to the build below with the default envelope and algorithm 32
    } else {
    match kind {
        "fast" => {}
        "slowatk" => { rates = [30, 99, 99, 60]; }
        "slowdec" => { rates = [99, 35, 99, 45]; levels = [99, 70, 70, 0]; }
        "rise" => { rates = [45, 40, 99, 50]; levels = [70, 99, 99, 0]; }
        "slowrel" => { rates = [99, 99, 99, 30]; }
        "stages" => { rates = [60, 45, 35, 40]; levels = [99, 80, 60, 0]; }
        "detune10" => ops[0].det = 10,
        "detune3" => ops[0].det = 3,
        "fine50" => ops[0].fine = 50,
        "coarse3" => ops[0].coarse = 3,
        "coarse0" => ops[0].coarse = 0,
        "mod80" => { alg = 4; ops[1].ol = 80; }                       // algorithm 5: operator 2 modulates operator 1 (and 4 -> 3, 6 -> 5, silent here)
        "mod60" => { alg = 4; ops[1].ol = 60; }
        "same2" => { ops[1].ol = 99; }                                      // operators 1 and 2, identical (both detune 7)
        "same3" => { ops[2].ol = 99; }
        "same4" => { ops[3].ol = 99; }
        "same5" => { ops[4].ol = 99; }
        "same6" => { ops[5].ol = 99; }
        "same2c2" => { ops[0].coarse = 2; ops[1].ol = 99; ops[1].coarse = 2; }
        "beat" => { ops[1].ol = 99; ops[1].det = 14; }                     // two carriers, seven detune steps apart: they beat
        "beat3" => { ops[1].ol = 99; ops[1].det = 10; }                   // three steps apart
        "moddet10" => { alg = 4; ops[1].ol = 80; ops[1].det = 10; }      // the modulator detuned up three steps
        "moddet3" => { alg = 4; ops[1].ol = 80; ops[1].det = 3; }
        "moddet14" => { alg = 4; ops[1].ol = 80; ops[1].det = 14; }
        "mod80r2" => { alg = 4; ops[1].ol = 80; ops[1].coarse = 2; }
        "mod80r3" => { alg = 4; ops[1].ol = 80; ops[1].coarse = 3; }
        "fb3" => { alg = 4; ops[0].ol = 0; ops[4].ol = 99; ops[5].ol = 80; fb = 3; }    // operator 6 (feedback) modulates operator 5
        "fb6" => { alg = 4; ops[0].ol = 0; ops[4].ol = 99; ops[5].ol = 80; fb = 6; }
        "rs7" => { ops[0].rs = 7; rates = [99, 40, 99, 99]; levels = [99, 40, 40, 0]; }
        "rs0" => { rates = [99, 40, 99, 99]; levels = [99, 40, 40, 0]; }
        k if k.starts_with("am:") => { let mut it = k[3..].split(':'); let a: u8 = it.next().and_then(|x| x.parse().ok()).unwrap_or(50); let m: u8 = it.next().and_then(|x| x.parse().ok()).unwrap_or(3); ops[0].ams = m.min(3); let w: u8 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0); lfo = [if w == 3 { 20 } else { 30 }, 0, 0, a.min(99), 1, w, 0]; }   // amplitude modulation: depth A, sensitivity M, wave W (0 triangle, 3 square)
        "amd" => { ops[0].ams = 2; lfo = [30, 60, 0, 99, 1, 0, 0]; }                    // amplitude modulation with an LFO delay
        "ams3" => { ops[0].ams = 3; lfo = [30, 0, 0, 80, 1, 0, 0]; }                    // amplitude modulation: triangle, depth 80, sensitivity 3
        "ams1" => { ops[0].ams = 1; lfo = [30, 0, 0, 80, 1, 0, 0]; }
        "pm0" | "pm1" | "pm2" | "pm3" | "pm4" | "pm5" => { lfo = [30, 0, 40, 0, 1, kind[2..].parse().unwrap_or(0), 3]; }   // pitch modulation, wave 0 to 5
        "pmd" => { lfo = [30, 50, 40, 0, 1, 0, 3]; }                                       // with a delay
        k if k.starts_with("pegA:") => { let r: u8 = k[5..].parse().unwrap_or(50); peg = ([r, 99, 99, 99], [54, 54, 54, 50]); }   // one ramp from level 50 to 54 at rate R, then held
        k if k.starts_with("pegB:") => { let r: u8 = k[5..].parse().unwrap_or(50); peg = ([99, 99, 99, 99], [54, 54, 54, 50]); lfo[0] = r; }
        "peg" => { peg = ([70, 70, 70, 70], [56, 44, 50, 50]); }                          // a pitch envelope sweep
        "peg2" => { peg = ([60, 60, 60, 60], [44, 56, 50, 50]); }
        "fixed" => { ops[0].mode = 1; ops[0].coarse = 2; ops[0].fine = 50; }              // fixed frequency: coarse 2 (100 Hz), fine 50
        "fixed3" => { ops[0].mode = 1; ops[0].coarse = 3; ops[0].fine = 0; }
        "ls1" => { ops[0].ld = 60; ops[0].rd = 60; ops[0].lc = 0; ops[0].rc = 3; }        // -LIN left, +LIN right of the break point
        "ls2" => { ops[0].ld = 80; ops[0].rd = 80; ops[0].lc = 1; ops[0].rc = 2; }        // -EXP left, +EXP right
        "ls4" => { ops[0].ld = 30; ops[0].lc = 0; ops[0].bp = 60; }                       // -LIN, depth 30 left of break point 60
        "ls5" => { ops[0].ld = 30; ops[0].lc = 1; ops[0].bp = 60; }                       // -EXP
        "ls6" => { ops[0].rd = 30; ops[0].rc = 0; ops[0].bp = 30; }                       // -LIN right of break point 30
        "ls7" => { ops[0].rd = 30; ops[0].rc = 3; ops[0].bp = 30; }                       // +LIN right
        "ls3" => { ops[0].ld = 99; ops[0].rd = 0; ops[0].lc = 0; ops[0].bp = 60; }
        "vel7" => ops[0].kvs = 7,
        "vel3" => ops[0].kvs = 3,
        "vel1" => ops[0].kvs = 1,
        "keysc" => { ops[0].rd = 99; ops[0].rc = 0; ops[0].ld = 99; ops[0].lc = 3; }     // level falls to the right of the break point (-LIN) and to the left (+LIN)
        _ => return None,
    }
    }
    let mut v = [0u8; 128];
    for (k, op) in ops.iter().enumerate() {
        let o = &mut v[(5 - k) * 17..(5 - k) * 17 + 17];             // the record holds operator 6 first
        let (r, l) = if k == 0 { (rates, levels) } else { ([99; 4], [99, 99, 99, 0]) };
        o[..4].copy_from_slice(&r);
        o[4..8].copy_from_slice(&l);
        o[8] = op.bp; o[9] = op.ld; o[10] = op.rd;
        o[11] = op.lc | (op.rc << 2);
        o[12] = op.rs | (op.det << 3);                               // rate scale, detune
        o[13] = op.ams | (op.kvs << 2);                              // amp mod sensitivity, velocity sensitivity
        o[14] = op.ol;
        o[15] = op.mode | (op.coarse << 1);                          // mode (0 ratio, 1 fixed), coarse
        o[16] = op.fine;
    }
    v[102..106].copy_from_slice(&peg.0);
    v[106..110].copy_from_slice(&peg.1);
    v[110] = alg;
    v[111] = fb | 8;                                                 // feedback, oscillator key sync
    v[112..116].copy_from_slice(&lfo[..4]);                          // LFO speed, delay, pitch depth, amp depth (0 by default: no vibrato, no tremolo)
    v[116] = lfo[4] | (lfo[5] << 1) | (lfo[6] << 4);                 // sync, wave, pitch sensitivity
    v[117] = 24;                                                     // no transpose
    let name = format!("T {kind}");
    for (i, b) in name.bytes().take(10).enumerate() { v[118 + i] = b; }
    for b in v[118 + name.len().min(10)..128].iter_mut() { *b = b' '; }
    Some(v)
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d { c ^= b as u32; for _ in 0..8 { c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 }; } }
    !c
}
fn write_png(path: &str, px: &[u32], w: usize, h: usize, scale: usize) -> std::io::Result<()> {
    let mut raw = Vec::new();
    for y in 0..h * scale { raw.push(0); for x in 0..w * scale { let p = px[(y / scale) * w + x / scale]; raw.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]); } }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() { z.push(u8::from(i + 1 == chunks.len())); z.extend((c.len() as u16).to_le_bytes()); z.extend((!(c.len() as u16)).to_le_bytes()); z.extend(*c); }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw { a = (a + v as u32) % 65521; b = (b + a) % 65521; }
    z.extend(((b << 16) | a).to_be_bytes());
    let chunk = |tag: &[u8; 4], data: &[u8]| { let mut c = (data.len() as u32).to_be_bytes().to_vec(); c.extend(tag); c.extend(data); let crc = crc32(&c[4..]); c.extend(crc.to_be_bytes()); c };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = ((w * scale) as u32).to_be_bytes().to_vec();
    ihdr.extend(((h * scale) as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &z));
    out.extend(chunk(b"IEND", &[]));
    fs::write(path, out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut file, mut note, mut vel, mut voice, mut program, mut hold, mut out) = (None, 60u8, 100u8, 0usize, None::<u8>, 1.5f64, None);
    let (mut brief, mut no_lfo, mut no_peg, mut make) = (false, false, false, None::<String>);
    let (mut gain, mut chord) = (false, Vec::<u8>::new());
    let mut unpacked = Vec::<usize>::new();
    let mut vced = None::<String>;
    let mut vput = None::<u8>;
    let mut bank_from = None::<String>;
    let mut track = false;
    let mut dump_track = false;
    let (mut shot, mut dtun, mut pot, mut master) = (None::<String>, 0u8, None::<u16>, None::<u16>);
    let mut stock_master = None::<u16>;
    let (mut put, mut strip, mut beat, mut partials, mut peaks, mut tuning) = (false, String::new(), false, false, false, fm6::Tuning::Msfa);
    let (mut steps, mut channel, mut by_key, mut fm6_note) = (Vec::<(bool, usize, i32)>::new(), 0u8, false, None::<u8>);
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| { *i += 1; args.get(*i).cloned().unwrap_or_default() };
        match args[i].as_str() {
            "--note" => note = next(&mut i).parse().unwrap_or(60),
            "--vel" => vel = next(&mut i).parse().unwrap_or(100),
            "--voice" => voice = next(&mut i).parse().unwrap_or(0),
            "--program" => program = next(&mut i).parse().ok(),
            "--hold" => hold = next(&mut i).parse().unwrap_or(1.5),
            "--out" => out = Some(next(&mut i)),
            "--enc" => {
                let v = next(&mut i);
                let (n, c) = v.split_once(':').unwrap_or((&v, "1"));
                match devices::ENCODER_NAMES.iter().position(|e| *e == n) { Some(e) => steps.push((true, e, c.parse().unwrap_or(1))), None => { eprintln!("no encoder {n}"); return ExitCode::from(2) } }
            }
            "--btn" => {
                let n = next(&mut i);
                match devices::BUTTON_NAMES.iter().position(|e| *e == n) { Some(b) => steps.push((false, b, 0)), None => { eprintln!("no button {n}"); return ExitCode::from(2) } }
            }
            "--encn" => {
                let v = next(&mut i);
                let (n, c) = v.split_once(':').unwrap_or((&v, "1"));
                match devices::ENCODER_NAMES.iter().position(|e| *e == n) { Some(e) => steps.push((true, e, 1_000_000 + c.parse::<i32>().unwrap_or(1))), None => { eprintln!("no encoder {n}"); return ExitCode::from(2) } }
            }
            "--channel" => channel = next(&mut i).parse::<u8>().unwrap_or(1).saturating_sub(1).min(15),
            "--key" => by_key = true,
            "--brief" => brief = true,
            "--beat" => beat = true,
            "--partials" => partials = true,
            "--track" => track = true,
            "--dump-track" => dump_track = true,
            "--bank-from" => bank_from = Some(next(&mut i)),
            "--vced" => vced = Some(next(&mut i)),
            "--vput" => vput = next(&mut i).parse().ok(),
            "--unpacked" => unpacked = next(&mut i).split(':').filter_map(|v| usize::from_str_radix(v.trim_start_matches("0x"), if v.starts_with("0x") { 16 } else { 10 }).ok()).collect(),
            "--gain" => gain = true,
            "--chord" => chord = next(&mut i).split(',').filter_map(|n| n.parse().ok()).collect(),
            "--pot" => pot = next(&mut i).parse().ok(),
            "--master" => master = next(&mut i).parse().ok(),
            "--stock-master" => stock_master = next(&mut i).parse().ok(),
            "--shot" => shot = Some(next(&mut i)),
            "--peaks" => peaks = true,
            "--tuning" => tuning = match next(&mut i).as_str() { "felucca" => fm6::Tuning::Felucca { dtun: 0 }, "stock" => fm6::Tuning::Stock, "msfa" => fm6::Tuning::Msfa, x => { eprintln!("--tuning msfa, felucca or stock, not {x}"); return ExitCode::from(2) } },
            "--dtun" => dtun = next(&mut i).parse().unwrap_or(0),
            "--make" => make = Some(next(&mut i)),
            "--put" => put = true,
            "--strip" => strip = next(&mut i),
            "--no-lfo" => no_lfo = true,
            "--no-peg" => no_peg = true,
            "--fm6-note" => fm6_note = next(&mut i).parse().ok(),
            f => file = Some(f.to_string()),
        }
        i += 1;
    }
    let Some(file) = file else { eprintln!("usage: fm6cmp FIRMWARE.fwsc [--note N] [--vel V] [--voice I] [--program P] [--hold S] [--out DIR]"); return ExitCode::from(2) };
    let bytes = match fs::read(&file) { Ok(b) => b, Err(e) => { eprintln!("{file}: {e}"); return ExitCode::from(1) } };

    // the voice: made here (written into the firmware over USB-MIDI), or the firmware's own bank, from the app image
    let (mut vbytes, vname): ([u8; 128], String);
    if let Some(kind) = &make {
        let Some(v) = make_voice(kind) else { eprintln!("no test voice {kind} (see make_voice)"); return ExitCode::from(2) };
        vbytes = v;
        vname = String::from_utf8_lossy(&v[118..128]).trim().to_string();
        println!("test voice {vname:?}, one carrier on algorithm 32, written into the firmware with FM6_PUT");
    } else if let Some(&off) = unpacked.first() {
            // --unpacked OFFSET[:STRIDE]: the voices are 155-byte unpacked records in the app image from OFFSET, STRIDE apart (156 by default)
            let stride = unpacked.get(1).copied().unwrap_or(156);
            let app = extract::app_image(&bytes).unwrap();
            let at = off + voice * stride;
            let Some(rec) = app.get(at..at + 155) else { eprintln!("voice {voice} is past the image"); return ExitCode::from(1) };
            if !extract::valid_unpacked(rec) { eprintln!("voice {voice} at app+{at:#x} is not a valid unpacked DX7 voice"); return ExitCode::from(1) }
            let mut p = [0u8; 156];
            p[..155].copy_from_slice(rec);
            p[155] = 0x3f;
            vbytes = fm6::pack_patch(&p);
            vname = String::from_utf8_lossy(&vbytes[118..128]).trim().to_string();
            println!("voice {voice} of the unpacked bank at app+{off:#x}: {vname:?}, algorithm {}", vbytes[110] + 1);
    } else {
        // --bank-from FILE: the voices come from that firmware's bank (for firmwares that take FM6_PUT, to play Felucca's own voices on them)
        let bank_bytes = match &bank_from { Some(f) => match fs::read(f) { Ok(b) => b, Err(e) => { eprintln!("{f}: {e}"); return ExitCode::from(1) } }, None => bytes.clone() };
        let rep = match extract::report(&bank_bytes, 4) { Ok(r) => r, Err(e) => { eprintln!("{e}"); return ExitCode::from(1) } };
        let Some(bank) = rep.banks.iter().find(|b| b.image == "app") else { eprintln!("no DX7 voice bank in this firmware's app image"); return ExitCode::from(1) };
        let app = extract::app_image(&bank_bytes).unwrap();
        let vo = bank.offset + voice * extract::VOICE_LEN;
        let Some(vb) = app.get(vo..vo + 128) else { eprintln!("voice {voice} is past the bank"); return ExitCode::from(1) };
        vbytes = vb.try_into().unwrap();
        vname = bank.names.get(voice).cloned().unwrap_or_else(|| "?".into());
        if !brief { println!("bank: {:?}", bank.names); }
        println!("voice {voice} of the bank at app+{:#x}: {:?}, algorithm {}", bank.offset, vname, bank.algorithms.get(voice).copied().unwrap_or(0) + 1);
    }
    // --strip lfo,peg,fb,det,kvs,rs,ls: parts of the voice made neutral in the bytes themselves (so, with --put or --make, on both sides)
    for part in strip.split(',').filter(|p| !p.is_empty()) {
        match part {
            "lfo" => { vbytes[114] = 0; vbytes[115] = 0; vbytes[116] &= 0x0f; }
            "peg" => { vbytes[102..106].copy_from_slice(&[99; 4]); vbytes[106..110].copy_from_slice(&[50; 4]); }
            "fb" => vbytes[111] &= !7,
            "det" => for op in 0..6 { vbytes[op * 17 + 12] = (vbytes[op * 17 + 12] & 7) | (7 << 3); },
            "kvs" => for op in 0..6 { vbytes[op * 17 + 13] = 0; },
            "rs" => for op in 0..6 { vbytes[op * 17 + 12] &= !7; },
            "ls" => for op in 0..6 { vbytes[op * 17 + 9] = 0; vbytes[op * 17 + 10] = 0; },
            // "only3": operator 3 alone (the others silent); "ol-12": every output level 12 lower; "coarse2=1": operator 2's coarse ratio set to 1; "fine1=0": operator 1's fine to 0
            x if x.starts_with("only") => { let k: usize = x[4..].parse().unwrap_or(0); if !(1..=6).contains(&k) { eprintln!("only1 to only6"); return ExitCode::from(2) } for op in 1..=6 { if op != k { vbytes[(6 - op) * 17 + 14] = 0; } } }
            x if x.starts_with("ol-") => { let d: u8 = x[3..].parse().unwrap_or(0); for op in 0..6 { let o = &mut vbytes[op * 17 + 14]; *o = o.saturating_sub(d); } }
            x if x.starts_with("coarse") => { let (k, v) = x[6..].split_once('=').unwrap_or(("0", "0")); let (k, v): (usize, u8) = (k.parse().unwrap_or(0), v.parse().unwrap_or(0)); if !(1..=6).contains(&k) { eprintln!("coarseK=V"); return ExitCode::from(2) } let o = &mut vbytes[(6 - k) * 17 + 15]; *o = (*o & 1) | (v.min(31) << 1); }
            x if x.starts_with("fine") => { let (k, v) = x[4..].split_once('=').unwrap_or(("0", "0")); let (k, v): (usize, u8) = (k.parse().unwrap_or(0), v.parse().unwrap_or(0)); if !(1..=6).contains(&k) { eprintln!("fineK=V"); return ExitCode::from(2) } vbytes[(6 - k) * 17 + 16] = v.min(99); }
            x => { eprintln!("no part {x} (lfo peg fb det kvs rs ls only1..6 ol-N coarseK=V fineK=V)"); return ExitCode::from(2) }
        }
    }
    // experiments on the fm6 side only: the voice's own pitch modulation (LFO depth and sensitivity) and its pitch envelope made neutral
    if no_lfo { vbytes[114] = 0; vbytes[116] &= 0x0f; }
    if no_peg { vbytes[102..106].copy_from_slice(&[99; 4]); vbytes[106..110].copy_from_slice(&[50; 4]); }

    // the emulator: boot, play the note, release it
    let mut m = match Machine::from_package(&bytes) { Ok(m) => m, Err(e) => { eprintln!("{e}"); return ExitCode::from(1) } };
    m.set_jit(true);
    let put_voice = make.is_some() || put || vput.is_some();
    if put_voice { m.usb_plug(); }     // before the first render: a firmware that starts USB with no cable gives up on it
    let mut buf = vec![0i16; 2 * 4096];
    let mut run = |m: &mut Machine, secs: f64, keep: &mut Vec<f64>, stereo_diff: &mut f64| {
        let mut left = (secs * SR as f64) as usize;
        let mut guard = 0;
        while left > 0 && !m.halted() && guard < 100_000 {
            guard += 1;
            let n = left.min(4096);
            let got = m.render(&mut buf[..2 * n], 8_000_000);
            for f in 0..got {
                let (l, r) = (buf[2 * f] as f64 / 32768.0, buf[2 * f + 1] as f64 / 32768.0);
                keep.push((l + r) / 2.0);
                *stereo_diff = stereo_diff.max((l - r).abs());
            }
            left -= got;
        }
    };
    let (mut boot, mut emu, mut sd) = (Vec::new(), Vec::new(), 0.0f64);
    run(&mut m, 3.0, &mut boot, &mut sd);
    if let Some(p) = program { m.midi_in(0xC0 | channel); m.midi_in(p); run(&mut m, 0.2, &mut boot, &mut sd); }
    if let Some(slot) = vput {
        // sloopDX's VOICE_PUT (command 43) first: the user slots are in the preset list only once a bank exists, and this makes one
        run(&mut m, 1.5, &mut boot, &mut sd);
        let mut frame = vec![0xF0, 0x7D, 0x46, 0x4C, 43, slot];
        frame.extend_from_slice(&vbytes);
        frame.push(0xF7);
        println!("USB product string: {:?}", m.usb_product());
        if !m.usb_midi_send(&frame) { eprintln!("the SysEx was not queued"); return ExitCode::from(1) }
        run(&mut m, 0.8, &mut boot, &mut sd);
        let replies = m.usb_midi_take();
        let ok = replies.iter().any(|r| r.len() >= 8 && r[..5] == [0xF0, 0x7D, 0x46, 0x4C, 43] && r[r.len() - 2] == 0);
        println!("VOICE_PUT reply: {}", if ok { "rc 0".to_string() } else { format!("none or refused: {replies:02x?}") });
        if !ok { return ExitCode::from(1) }
        run(&mut m, 0.5, &mut boot, &mut sd);
    }
    if let Some(v) = pot { m.pot(v); run(&mut m, 0.6, &mut boot, &mut sd); }
    for &(is_enc, n, c) in &steps {
        if is_enc && c.abs() >= 1_000_000 - 4096 {
            let clicks = if c > 0 { c - 1_000_000 } else { c + 1_000_000 };   // --encn: one click at a time
            for _ in 0..clicks.abs() { m.encoder(n as u8, clicks.signum()); run(&mut m, 0.12, &mut boot, &mut sd); }
        } else if is_enc { m.encoder(n as u8, c); } else { m.key(n as u8, true); run(&mut m, 0.15, &mut boot, &mut sd); m.key(n as u8, false); }
        run(&mut m, 1.2, &mut boot, &mut sd);
    }
    if put_voice && vput.is_none() {
        run(&mut m, 1.5, &mut boot, &mut sd);                                    // the host reads the descriptors first (about a second)
        let mut frame = vec![0xF0, 0x7D, 0x46, 0x4C, 69, 0, channel];            // FM6_PUT, target 0 (a track's own patch), the track
        frame.extend_from_slice(&vbytes);
        frame.push(0xF7);
        let mut cmd = 69u8;
        if let Some(slot) = vput {
            // sloopDX's VOICE_PUT (command 43): a user slot 0..31 and the 128 packed bytes, in place of FM6_PUT
            cmd = 43;
            frame = vec![0xF0, 0x7D, 0x46, 0x4C, 43, slot];
            frame.extend_from_slice(&vbytes);
            frame.push(0xF7);
        }
        if let Some(how) = &vced {
            // a standard DX7 single-voice dump: F0 43 0n 00 01 1B, the 155 data bytes, their checksum (two's complement, 7 bits), F7
            let p = fm6::unpack_patch(&vbytes);
            let sum: u32 = p[..155].iter().map(|&b| b as u32).sum();
            let mut d = vec![0xF0, 0x43, channel & 15, 0x00, 0x01, 0x1B];
            d.extend_from_slice(&p[..155]);
            d.push(((128 - (sum & 127)) & 127) as u8);
            d.push(0xF7);
            if how == "uart" { for b in d { m.midi_in(b); } run(&mut m, 1.0, &mut boot, &mut sd); println!("DX7 voice dump sent to the MIDI IN jack ({} bytes)", 163); }
            else { println!("DX7 voice dump sent over USB-MIDI: {}", if m.usb_midi_send(&d) { "queued" } else { "not queued" }); run(&mut m, 0.8, &mut boot, &mut sd); }
            println!("replies: {:02x?}", m.usb_midi_take());
            run(&mut m, 0.5, &mut boot, &mut sd);
            frame.clear();
        }
        if !frame.is_empty() {
        println!("USB product string: {:?}", m.usb_product());
        if !m.usb_midi_send(&frame) { eprintln!("the SysEx was not queued"); return ExitCode::from(1) }
        run(&mut m, 0.6, &mut boot, &mut sd);
        let replies = m.usb_midi_take();
        let ok = replies.iter().any(|r| r.len() >= 8 && r[..4] == [0xF0, 0x7D, 0x46, 0x4C] && r[4] == cmd && r[r.len() - 2] == 0);
        println!("{} reply: {}", if cmd == 43 { "VOICE_PUT" } else { "FM6_PUT" }, if ok { "rc 0".to_string() } else { format!("none or refused: {replies:02x?}") });
        if !ok { return ExitCode::from(1) }
        run(&mut m, 0.5, &mut boot, &mut sd);
        }
    }
    if let Some(path) = &shot {
        let mut px = vec![0u32; 240 * 240];
        m.screen(&mut px);
        if let Err(e) = write_png(path, &px, 240, 240, 2) { eprintln!("{path}: {e}"); return ExitCode::from(1) }
        println!("wrote {path}");
        return ExitCode::SUCCESS;
    }
    let boot_peak = boot.iter().fold(0.0f64, |a, &v| a.max(v.abs()));
    let key_id = devices::KEY_ID_BASE + note.saturating_sub(53).min(26);
    if by_key { m.key(key_id, true) } else { m.midi_in(0x90 | channel); m.midi_in(note); m.midi_in(vel); for &c in &chord { m.midi_in(0x90 | channel); m.midi_in(c); m.midi_in(vel); } }
    run(&mut m, hold, &mut emu, &mut sd);
    if by_key { m.key(key_id, false) } else { m.midi_in(0x80 | channel); m.midi_in(note); m.midi_in(0); for &c in &chord { m.midi_in(0x80 | channel); m.midi_in(c); m.midi_in(0); } }
    run(&mut m, 1.5, &mut emu, &mut sd);
    if m.halted() { eprintln!("the emulated machine halted"); return ExitCode::from(1) }

    // fm6: the same voice, note and release time
    let mut syn = fm6::Synth::new(SR as f64, &vbytes).unwrap();
    if let fm6::Tuning::Felucca { .. } = tuning { tuning = fm6::Tuning::Felucca { dtun }; }
    syn.set_tuning(tuning);
    if let Some(pot) = stock_master { syn.set_output(fm6::Output::Stock { pot }); }
    if let Some(pot) = master { syn.set_output(fm6::Output::Felucca { pot }); }   // --master POT: fm6 with Felucca's output stage at that pot (the emulator's is --pot, default 900)
    let note = fm6_note.unwrap_or(note);
    syn.note_on(note, vel);
    for &c in &chord { syn.note_on(c, vel); }
    let mut f6 = vec![0i16; (hold * SR as f64) as usize];
    syn.render(&mut f6);
    syn.note_off(note);
    for &c in &chord { syn.note_off(c); }
    let mut tail = vec![0i16; (1.5 * SR as f64) as usize];
    syn.render(&mut tail);
    let f6: Vec<f64> = f6.iter().chain(tail.iter()).map(|&v| v as f64 / 32768.0).collect();

    if peaks {
        // the strongest peaks of each, from a long spectrum (1.5 s from 0.4 s), matched by frequency: where the sidebands sit, emulator against fm6
        let spec = |x: &[f64]| -> Vec<(f64, f64)> {
            const N: usize = 65536;
            let mut re = vec![0.0; N];
            let mut im = vec![0.0; N];
            for (i, r) in re.iter_mut().enumerate() { *r = x.get((0.4 * SR as f64) as usize + i).copied().unwrap_or(0.0) * (0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos()); }
            fft(&mut re, &mut im);
            let mag: Vec<f64> = (0..N / 2).map(|k| re[k].hypot(im[k])).collect();
            let bin = SR as f64 / N as f64;
            let mut p: Vec<(f64, f64)> = (40..N / 2 - 2).filter(|&k| mag[k] > mag[k - 1] && mag[k] >= mag[k + 1] && 20.0 * (mag[k] / (N as f64 / 4.0)).log10() > -70.0)
                .map(|k| { let (a, b, g) = (mag[k - 1].ln(), mag[k].ln(), mag[k + 1].ln()); ((k as f64 + 0.5 * (a - g) / (a - 2.0 * b + g)) * bin, 20.0 * (mag[k] / (N as f64 / 4.0)).log10()) }).collect();
            p.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            p.truncate(10);
            p.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            p
        };
        let (pe, pf) = (spec(&emu), spec(&f6));
        println!("peaks above -70 dBFS, emulator against the nearest fm6 peak (Hz, dB, then the frequency difference in cents and the level difference):");
        let mut worst = 0.0f64;
        for (f, d) in &pe {
            match pf.iter().min_by(|a, b| (a.0 - f).abs().partial_cmp(&(b.0 - f).abs()).unwrap()) {
                Some((g, h)) if (g / f - 1.0).abs() < 0.02 => { let c = 1200.0 * (f / g).log2(); if *d > -50.0 { worst = worst.max(c.abs()); } println!("  {:9.3} Hz {:6.1} dB   fm6 {:9.3} Hz {:6.1} dB   {:+7.2} cents {:+6.1} dB", f, d, g, h, c, h - d) }
                _ => println!("  {:9.3} Hz {:6.1} dB   no fm6 peak within 2%", f, d),
            }
        }
        println!("worst frequency difference of the matched peaks above -50 dBFS: {worst:.2} cents");
        return ExitCode::SUCCESS;
    }
    if track {
        // the strongest partial's frequency (cents) and level (dB) over the held note, in windows of 93 ms every 23 ms from 0.1 s: emulator against fm6
        let f_exp = 440.0 * 2f64.powf((fm6_note.unwrap_or(note) as f64 - 69.0) / 12.0);
        let first = peak_in(&emu, (0.1 * SR as f64) as usize, 40.0, 8000.0).map(|p| p.0).unwrap_or(f_exp);
        let (lo, hi) = (first * 0.9, first * 1.1);
        let n = ((hold - 0.3) * SR as f64 / 1024.0) as usize;
        let series = |x: &[f64]| -> Vec<(f64, f64)> { (0..n).filter_map(|i| peak_in(x, (0.1 * SR as f64) as usize + i * 1024, lo, hi)).collect() };
        let (se, sf) = (series(&emu), series(&f6));
        let len = se.len().min(sf.len());
        if len < 8 { println!("TRACK too short"); return ExitCode::SUCCESS }
        let cents = |s: &[(f64, f64)]| -> Vec<f64> { let m = s.iter().map(|p| p.0).sum::<f64>() / s.len() as f64; s.iter().map(|p| 1200.0 * (p.0 / m).log2()).collect() };
        let (ce, cf) = (cents(&se[..len]), cents(&sf[..len]));
        let p2p = |v: &[f64]| v.iter().cloned().fold(-1e9, f64::max) - v.iter().cloned().fold(1e9, f64::min);
        if dump_track { for i in 0..len.min(40) { println!("  t={:.3}s  emulator {:+7.1} cents  fm6 {:+7.1} cents", 0.1 + 0.0232 * i as f64 + 0.0464, ce[i], cf[i]); } }
        let (de, df): (Vec<f64>, Vec<f64>) = (se[..len].iter().map(|p| p.1).collect(), sf[..len].iter().map(|p| p.1).collect());
        let corr = |a: &[f64], b: &[f64]| { let (ma, mb) = (a.iter().sum::<f64>() / a.len() as f64, b.iter().sum::<f64>() / b.len() as f64); let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0); for (x, y) in a.iter().zip(b) { sxy += (x - ma) * (y - mb); sxx += (x - ma).powi(2); syy += (y - mb).powi(2); } sxy / (sxx * syy).sqrt().max(1e-12) };
        let maxdiff = ce.iter().zip(&cf).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        println!("TRACK partial {:.1} Hz: pitch peak-to-peak emulator {:.1} cents, fm6 {:.1}; correlation {:.3}, largest difference {:.1} cents | level peak-to-peak emulator {:.2} dB, fm6 {:.2}; correlation {:.3}", first, p2p(&ce), p2p(&cf), corr(&ce, &cf), maxdiff, p2p(&de), p2p(&df), corr(&de, &df));
        return ExitCode::SUCCESS;
    }
    if gain {
        // the steady level difference to 0.001 dB, fm6 minus emulator, RMS over 0.3 s to 1.3 s of the held note (a pure tone: this is its gain)
        let at = |t: f64| (t * SR as f64) as usize;
        let (e, f) = (rms_db(&emu[at(0.3)..at(1.3)]), rms_db(&f6[at(0.3)..at(1.3)]));
        println!("GAIN {:.4}", f - e);
        return ExitCode::SUCCESS;
    }
    if partials {
        // one row per partial k = 1..6 (operator k at ratio k): emulator Hz, fm6 Hz, the emulator against fm6 in cents (CSV: note, k, emu_hz, fm6_hz, cents, emu_db)
        let f0 = 440.0 * 2f64.powf((fm6_note.unwrap_or(note) as f64 - 69.0) / 12.0);
        let at = (0.4 * SR as f64) as usize;
        for k in 1..=6usize {
            match (peak_near(&emu, at, f0 * k as f64), peak_near(&f6, at, f0 * k as f64)) {
                (Some((e, ed)), Some((f, _))) => println!("P,{},{},{:.3},{:.3},{:.2},{:.0}", note, k, e, f, 1200.0 * (e / f).log2(), ed),
                _ => println!("P,{},{},,,,", note, k),
            }
        }
        return ExitCode::SUCCESS;
    }
    if beat {
        // the two carriers' frequencies from a long spectrum (1.5 s from 0.3 s), and so how far apart the detune put them
        let f0 = 440.0 * 2f64.powf((fm6_note.unwrap_or(note) as f64 - 69.0) / 12.0);
        let (pe, pf) = (two_peaks(&emu, (0.3 * SR as f64) as usize, f0), two_peaks(&f6, (0.3 * SR as f64) as usize, f0));
        match (pe, pf) {
            (Some((a, b, ea)), Some((c, d, fa))) => println!("note {:>3} ({:7.2} Hz): emulator peaks {:8.3} and {:8.3} Hz (second {:.0} dB), apart {:.3} Hz ({:.2} cents); fm6 {:8.3} and {:8.3} (second {:.0} dB), apart {:.3} Hz ({:.2} cents); emulator / fm6 = {:.3}",
                note, f0, a, b, ea, b - a, 1200.0 * (b / a).log2(), c, d, fa, d - c, 1200.0 * (d / c).log2(), (b - a) / (d - c)),
            _ => println!("note {note}: fewer than two peaks near {f0:.1} Hz"),
        }
        return ExitCode::SUCCESS;
    }
    if brief {
        let at = |t: f64| (t * SR as f64) as usize;
        let f0 = 440.0 * 2f64.powf((fm6_note.unwrap_or(note) as f64 - 69.0) / 12.0);
        // level: fm6 minus emulator, RMS per 100 ms from 0.1 s to 1.3 s: the mean and the spread (a constant gain is the mean; the spread is shape)
        let d: Vec<f64> = (1..13).map(|k| rms_db(&f6[at(k as f64 / 10.0)..at((k + 1) as f64 / 10.0)]) - rms_db(&emu[at(k as f64 / 10.0)..at((k + 1) as f64 / 10.0)])).collect();
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        let spread = d.iter().cloned().fold(-1e9, f64::max) - d.iter().cloned().fold(1e9, f64::min);
        // pitch: the strongest partial anywhere from 20 Hz to 8 kHz, refined by parabolic interpolation: its frequency in Hz
        let pitch = |x: &[f64]| { let sp = spectrum(x, at(0.2)); let (lo, hi) = (8, 2970); let k = (lo..hi).max_by(|&a, &b| sp[a].partial_cmp(&sp[b]).unwrap()).unwrap(); let (a, b, g) = (sp[k - 1], sp[k], sp[k + 1]); let off = 0.5 * (a - g) / (a - 2.0 * b + g); (k as f64 + off) * SR as f64 / 16384.0 };
        let (pe, pf) = (pitch(&emu), pitch(&f6));
        // shape: harmonics 2 to 4 relative to the fundamental, 0.10-0.47 s, where both are above -100 dB
        let (se, sf) = (spectrum(&emu, at(0.1)), spectrum(&f6, at(0.1)));
        let part = |db: &[f64], k: usize| { let c = (f0 * k as f64 * 16384.0 / SR as f64).round() as usize; (c.saturating_sub(3)..(c + 4).min(db.len())).map(|b| db[b]).fold(-200.0f64, f64::max) };
        let rel: Vec<String> = (2..=4).map(|k| format!("{:+.0}", (part(&sf, k) - part(&sf, 1)) - (part(&se, k) - part(&se, 1)))).collect();
        let dec = |x: &[f64]| (rms_db(&x[at(1.1)..at(1.3)]) - rms_db(&x[at(0.3)..at(0.5)])) / 0.8;
        let rel_on = rms_db(&emu[at(hold + 0.3)..at(hold + 0.4)]) - rms_db(&f6[at(hold + 0.3)..at(hold + 0.4)]);
        println!("{:<11} note {:>3} vel {:>3}: level fm6-emu {:+5.1} dB (spread {:4.1}); strongest partial emu {:.2} Hz fm6 {:.2} Hz ({:+.1} cents); harmonics 2-4 rel. fundamental (fm6-emu) {} dB; decay emu {:+6.1} fm6 {:+6.1} dB/s; 0.3 s after release emu-fm6 {:+5.1} dB",
            vname, note, vel, mean, spread, pe, pf, 1200.0 * (pe / pf).log2(), rel.join(" "), dec(&emu), dec(&f6), rel_on);
        return ExitCode::SUCCESS;
    }
    // report
    let peak = |x: &[f64]| x.iter().fold(0.0f64, |a, &v| a.max(v.abs()));
    println!("emulator: boot-time peak {:.4} (before the note), left-right difference {:.4}; fm6 peak {:.3}, emulator peak {:.3}", boot_peak, sd, peak(&f6), peak(&emu));
    let onset = |x: &[f64]| x.iter().position(|v| v.abs() > 0.002).map(|p| p as f64 * 1000.0 / SR as f64);
    println!("onset (first sample over 0.002): emulator {:?} ms, fm6 {:?} ms", onset(&emu).map(|v| v.round()), onset(&f6).map(|v| v.round()));
    println!("\nlevel (RMS dB per 100 ms; the key goes up at {:.1} s):\n  t(s)   emulator     fm6   (diff)", hold);
    let win = SR / 10;
    for k in 0..(emu.len().min(f6.len()) / win).min(30) {
        let (a, b) = (rms_db(&emu[k * win..(k + 1) * win]), rms_db(&f6[k * win..(k + 1) * win]));
        println!("  {:>4.1}  {:>8.1} {:>7.1}   ({:+.1})", k as f64 / 10.0, a, b, b - a);
    }
    // partials: the level of each harmonic of the note (the largest bin within +-3 of k * f0), in dB, at three times while the key is down
    let f0 = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
    let part = |db: &[f64], k: usize| { let c = (f0 * k as f64 * 16384.0 / SR as f64).round() as usize; (c.saturating_sub(3)..(c + 4).min(db.len())).map(|b| db[b]).fold(-200.0f64, f64::max) };
    println!("\npartials, dB (emulator / fm6 / difference), the note is {:.1} Hz; the emulator's firmware effects are part of its figures:", f0);
    println!("  harmonic   {:>22}   {:>22}   {:>22}", "0.10-0.47 s", "0.50-0.87 s", "0.90-1.27 s");
    let spec: Vec<(Vec<f64>, Vec<f64>)> = [0.10, 0.50, 0.90].iter().map(|t| { let at = (t * SR as f64) as usize; (spectrum(&emu, at), spectrum(&f6, at)) }).collect();
    let mut shape = 0.0;
    for k in 1..=8 {
        let cells: Vec<String> = spec.iter().map(|(e, f)| { let (a, b) = (part(e, k), part(f, k)); format!("{:>6.1} /{:>6.1} /{:>+6.1}", a, b, b - a) }).collect();
        println!("  {:>5}x    {}", k, cells.join("   "));
        // shape: each harmonic relative to the fundamental, emulator against fm6, at the first window
        let (e, f) = (&spec[0].0, &spec[0].1);
        shape += ((part(e, k) - part(e, 1)) - (part(f, k) - part(f, 1))).abs();
    }
    println!("mean |difference| of the first eight harmonics' levels relative to the fundamental, 0.10-0.47 s: {:.1} dB", shape / 8.0);
    // decay: the level slope of the held note, 0.3 s to 1.3 s, and the level offset there
    let slope = |x: &[f64]| { let (a, b) = (rms_db(&x[(0.3 * SR as f64) as usize..(0.5 * SR as f64) as usize]), rms_db(&x[(1.1 * SR as f64) as usize..(1.3 * SR as f64) as usize])); (b - a) / 0.8 };
    println!("decay while held (0.3-0.5 s against 1.1-1.3 s): emulator {:+.1} dB/s, fm6 {:+.1} dB/s; level at 0.3-0.5 s: emulator {:.1} dB, fm6 {:.1} dB",
        slope(&emu), slope(&f6), rms_db(&emu[(0.3 * SR as f64) as usize..(0.5 * SR as f64) as usize]), rms_db(&f6[(0.3 * SR as f64) as usize..(0.5 * SR as f64) as usize]));
    if let Some(dir) = out {
        let d = Path::new(&dir);
        if fs::create_dir_all(d).is_ok() {
            let _ = write_wav(&d.join("emulator.wav"), &emu);
            let _ = write_wav(&d.join("fm6.wav"), &f6);
            println!("\nwrote {dir}/emulator.wav and {dir}/fm6.wav");
        }
    }
    ExitCode::SUCCESS
}
