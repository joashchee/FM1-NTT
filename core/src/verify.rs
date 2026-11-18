//! Verification of a port (docs/PORTING.md, step 5): render a note natively and in the emulator, measure both, compare.
//!
//! The measurements (RMS level in dB, pitch in Hz by a spectral peak with parabolic interpolation) are plain functions; `Comparison` is what a package's
//! `verify` line holds. `emulator_note` plays a note on a firmware in the emulated machine (the oracle: the Mac and CI, never the R36S, where it runs at a
//! quarter of real time). Only a level and a pitch are compared: a package whose timbre is wrong can still pass, and the line says what it measured.

use crate::{
    engine::Engine,
    machine::Machine,
    Error, Result,
};
use std::f64::consts::PI;

pub const RATE: usize = 44_100;

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

/// The RMS level of `x` (samples in -1..1) in dB full scale.
pub fn rms_db(x: &[f64]) -> f64 {
    let m = x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    10.0 * m.max(1e-18).log10()
}

/// The frequency of the strongest spectral peak within 3% of `near` Hz, over the largest power-of-two window (at most 65536) that fits in `x` from `from`; `None`
/// when the window is shorter than 4096 samples or there is no peak.
pub fn pitch_near(x: &[f64], from: usize, near: f64) -> Option<f64> {
    let avail = x.len().saturating_sub(from);
    let mut n = 4096usize;
    if avail < n {
        return None;
    }
    while n * 2 <= avail && n < 65536 {
        n *= 2;
    }
    let mut re: Vec<f64> = (0..n).map(|i| x[from + i] * (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos())).collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let mag: Vec<f64> = (0..n / 2).map(|k| re[k].hypot(im[k])).collect();
    let bin = RATE as f64 / n as f64;
    let (lo, hi) = (((near * 0.97) / bin) as usize, ((near * 1.03) / bin) as usize + 1);
    let k = (lo.max(1)..hi.min(n / 2 - 1)).max_by(|&a, &b| mag[a].total_cmp(&mag[b]))?;
    if mag[k] < 1e-6 || mag[k - 1] <= 0.0 || mag[k + 1] <= 0.0 {
        return None;
    }
    let (l, c, r) = (mag[k - 1].ln(), mag[k].ln(), mag[k + 1].ln());
    Some((k as f64 + 0.5 * (l - r) / (l - 2.0 * c + r)) * bin)
}

/// The equal-tempered frequency of a MIDI note.
pub fn note_hz(note: u8) -> f64 {
    440.0 * 2f64.powf((note as f64 - 69.0) / 12.0)
}

/// What a comparison of two renders of one note found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Comparison {
    pub note: u8,
    pub velocity: u8,
    /// native minus reference, dB RMS over the held note
    pub level_db: f64,
    /// native minus reference, cents (None when either has no peak near the note)
    pub pitch_cents: Option<f64>,
}

impl Comparison {
    pub fn within(&self, level_db: f64, cents: f64) -> bool {
        self.level_db.abs() <= level_db && self.pitch_cents.is_some_and(|c| c.abs() <= cents)
    }

    /// The line a package's `verify` holds.
    pub fn summary(&self, what: &str) -> String {
        let pitch = match self.pitch_cents {
            Some(c) => format!("pitch {c:+.2} cent"),
            None => "pitch not measured".to_string(),
        };
        format!("{what}: level {:+.2} dB, {pitch} (note {}, velocity {}; level and pitch only)", self.level_db, self.note, self.velocity)
    }
}

/// Compares two renders (mono, -1..1, the note held for the whole of both) of `note`: levels over the second half, pitch from the middle.
pub fn compare(reference: &[f64], native: &[f64], note: u8, velocity: u8) -> Comparison {
    let half = |x: &[f64]| x[x.len() / 2..].to_vec();
    let level_db = rms_db(&half(native)) - rms_db(&half(reference));
    let f = note_hz(note);
    let pitch_cents = match (pitch_near(reference, reference.len() / 4, f), pitch_near(native, native.len() / 4, f)) {
        (Some(a), Some(b)) => Some(1200.0 * (b / a).log2()),
        _ => None,
    };
    Comparison { note, velocity, level_db, pitch_cents }
}

/// Plays `note` on `engine` for `secs` seconds at 44.1 kHz and returns it (mono, -1..1). The engine should be at 44.1 kHz.
pub fn render_note(engine: &mut dyn Engine, note: u8, velocity: u8, secs: f64) -> Vec<f64> {
    let mut buf = vec![0i16; (secs * RATE as f64) as usize];
    engine.note_on(note, velocity);
    engine.render(&mut buf);
    engine.note_off(note);
    buf.iter().map(|&v| v as f64 / 32768.0).collect()
}

/// Plays `note` on a firmware in the emulated machine: boots it for 3 s, selects `program` (a MIDI program change on `channel`) if given, holds the note for
/// `secs` s on the MIDI IN jack, and returns the held audio (mono, -1..1). The emulator runs at the speed of the host; on an R36S this is far slower than real time.
pub fn emulator_note(firmware: &[u8], channel: u8, program: Option<u8>, note: u8, velocity: u8, secs: f64) -> Result<Vec<f64>> {
    let mut m = Machine::from_package(firmware)?;
    m.set_jit(true);
    let mut buf = vec![0i16; 2 * 4096];
    let mut run = |m: &mut Machine, secs: f64, keep: Option<&mut Vec<f64>>| {
        let mut left = (secs * RATE as f64) as usize;
        let mut guard = 0;
        let mut keep = keep;
        while left > 0 && !m.halted() && guard < 100_000 {
            guard += 1;
            let n = left.min(4096);
            let got = m.render(&mut buf[..2 * n], 8_000_000);
            if let Some(k) = keep.as_mut() {
                for f in 0..got {
                    k.push((buf[2 * f] as f64 + buf[2 * f + 1] as f64) / 65536.0);
                }
            }
            left -= got.max(1).min(left);
        }
    };
    run(&mut m, 3.0, None);
    if let Some(p) = program {
        m.midi_in(0xC0 | (channel & 15));
        m.midi_in(p & 127);
        run(&mut m, 0.2, None);
    }
    let mut out = Vec::new();
    m.midi_in(0x90 | (channel & 15));
    m.midi_in(note & 127);
    m.midi_in(velocity.clamp(1, 127));
    run(&mut m, secs, Some(&mut out));
    if m.halted() {
        return Err(Error::BadImage("the emulated machine halted".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::{Analog, P_ATTACK, P_CUTOFF, P_DETUNE};

    fn tone(f: f64, amp: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| amp * (2.0 * PI * f * i as f64 / RATE as f64).sin()).collect()
    }

    #[test]
    fn measures_level_and_pitch() {
        let x = tone(440.0, 0.5, 44_100);
        assert!((rms_db(&x) - (-9.03)).abs() < 0.05, "{}", rms_db(&x));
        let f = pitch_near(&x, 1000, 440.0).unwrap();
        assert!((f - 440.0).abs() < 0.05, "{f}");
        assert!(pitch_near(&x[..1000], 0, 440.0).is_none(), "a window too short");
        assert!(pitch_near(&vec![0.0; 20_000], 0, 440.0).is_none(), "silence has no peak");
    }

    #[test]
    fn compare_finds_a_known_offset() {
        let a = tone(440.0, 0.5, 88_200);
        let mut b = tone(440.0 * 2f64.powf(5.0 / 1200.0), 0.25, 88_200);
        let c = compare(&a, &b, 69, 100);
        assert!((c.level_db + 6.02).abs() < 0.1, "{}", c.level_db);
        assert!((c.pitch_cents.unwrap() - 5.0).abs() < 0.3, "{:?}", c.pitch_cents);
        assert!(!c.within(1.0, 1.0));
        assert!(c.summary("x").contains("level -6.0"));
        b.fill(0.0);
        assert!(compare(&a, &b, 69, 100).pitch_cents.is_none());
        let same = compare(&a, &a, 69, 100);
        assert!(same.within(0.001, 0.001));
    }

    #[test]
    fn a_native_engine_renders_the_right_pitch() {
        let mut e = Analog::new(44_100.0);
        e.set(P_CUTOFF, 127);
        e.set(P_DETUNE, 0);
        e.set(P_ATTACK, 1);
        let x = render_note(&mut e, 57, 100, 1.5);
        let f = pitch_near(&x, 20_000, note_hz(57)).unwrap();
        let cents = 1200.0 * (f / note_hz(57)).log2();
        assert!(cents.abs() < 3.0, "{cents} cents");
    }
}
