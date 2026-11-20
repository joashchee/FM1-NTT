//! `analog`: a subtractive engine, written fresh for FM1-NTT (nothing of it is fitted to a firmware; docs/PORTING.md).
//!
//! Two band-limited oscillators (polyBLEP saw, pulse, triangle, sine; the second detuned), a state-variable low-pass (Zavalishin's topology-preserving
//! form) driven by the amplitude envelope, and an ADSR. Eight voices, oldest stolen. Floating point, deterministic on one platform, no allocation after `new`.

use crate::engine::{Engine, ParamDesc};
use std::f64::consts::PI;

pub const VOICES: usize = 8;

pub const P_WAVE: u16 = 0;
pub const P_PULSE_WIDTH: u16 = 1;
pub const P_CUTOFF: u16 = 2;
pub const P_RESONANCE: u16 = 3;
pub const P_ENV_AMOUNT: u16 = 4;
pub const P_ATTACK: u16 = 5;
pub const P_DECAY: u16 = 6;
pub const P_SUSTAIN: u16 = 7;
pub const P_RELEASE: u16 = 8;
pub const P_DETUNE: u16 = 9;
pub const P_LEVEL: u16 = 10;
pub const P_COUNT: usize = 11;

/// name, min, max, default, unit
const DESC: [(&str, i32, i32, i32, &str); P_COUNT] = [
    ("WAVE (0 SAW 1 PULSE 2 TRI 3 SINE)", 0, 3, 0, ""),
    ("PULSE WIDTH", 5, 95, 50, "%"),
    ("CUTOFF", 0, 127, 90, ""),
    ("RESONANCE", 0, 99, 20, ""),
    ("ENV TO CUTOFF", 0, 99, 40, ""),
    ("ATTACK", 0, 5000, 5, "ms"),
    ("DECAY", 1, 5000, 300, "ms"),
    ("SUSTAIN", 0, 99, 70, ""),
    ("RELEASE", 1, 5000, 250, "ms"),
    ("OSC 2 DETUNE", 0, 50, 7, "cents"),
    ("LEVEL", 0, 100, 70, "%"),
];

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy)]
struct Voice {
    stage: Stage,
    note: u8,
    held: bool,
    age: u64,
    f0: f64,
    /// where the pitch is going (`f0` slides to it when glide is on)
    tf0: f64,
    ph: [f64; 2],
    tri: [f64; 2],
    env: f64,
    gain: f64,
    ic1: f64,
    ic2: f64,
    /// the filter's coefficients, refreshed every eighth sample (the envelope moves slowly against that)
    co: [f64; 3],
}

const IDLE: Voice = Voice { stage: Stage::Idle, note: 0, held: false, age: 0, f0: 0.0, tf0: 0.0, ph: [0.0; 2], tri: [0.0; 2], env: 0.0, gain: 0.0, ic1: 0.0, ic2: 0.0, co: [0.0; 3] };

pub struct Analog {
    rate: f64,
    p: [i32; P_COUNT],
    voices: [Voice; VOICES],
    clock: u64,
    bend: f64,
    bend_raw: i32,
    bend_semis: f64,
    /// portamento time in ms (0 off), and the pitch of the last note started
    glide_ms: i32,
    last_f0: f64,
}

fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

impl Analog {
    pub fn new(rate: f64) -> Analog {
        let rate = rate.clamp(8000.0, 192_000.0);
        let mut p = [0; P_COUNT];
        for (i, d) in DESC.iter().enumerate() {
            p[i] = d.3;
        }
        Analog { rate, p, voices: [IDLE; VOICES], clock: 0, bend: 1.0, bend_raw: 8192, bend_semis: 2.0, glide_ms: 0, last_f0: 0.0 }
    }

    fn osc(&mut self, vi: usize, k: usize, ratio: f64) -> f64 {
        let dt = (self.voices[vi].f0 * ratio / self.rate).min(0.45);
        let wave = self.p[P_WAVE as usize];
        let pw = self.p[P_PULSE_WIDTH as usize] as f64 / 100.0;
        let v = &mut self.voices[vi];
        let t = v.ph[k];
        let y = match wave {
            0 => 2.0 * t - 1.0 - poly_blep(t, dt),
            1 => {
                let mut y = if t < pw { 1.0 } else { -1.0 };
                y += poly_blep(t, dt);
                y -= poly_blep((t + 1.0 - pw) % 1.0, dt);
                y
            }
            2 => {
                // a leaky integral of a band-limited square
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t + 0.5) % 1.0, dt);
                v.tri[k] = v.tri[k] * 0.999_95 + 4.0 * dt * sq;
                v.tri[k]
            }
            _ => (2.0 * PI * t).sin(),
        };
        v.ph[k] += dt;
        if v.ph[k] >= 1.0 {
            v.ph[k] -= 1.0;
        }
        y
    }
}

impl Engine for Analog {
    fn kind(&self) -> &'static str {
        "analog"
    }

    fn params(&self) -> Vec<ParamDesc> {
        DESC.iter().enumerate().map(|(i, d)| ParamDesc { id: i as u16, name: d.0.to_string(), min: d.1, max: d.2, default: d.3, unit: d.4 }).collect()
    }

    fn get(&self, id: u16) -> Option<i32> {
        self.p.get(id as usize).copied()
    }

    fn set(&mut self, id: u16, value: i32) -> bool {
        let Some(d) = DESC.get(id as usize) else { return false };
        self.p[id as usize] = value.clamp(d.1, d.2);
        true
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(note);
        }
        let note = note.min(127);
        self.clock += 1;
        let i = match self.voices.iter().position(|v| v.stage == Stage::Idle) {
            Some(i) => i,
            None => (0..VOICES).min_by_key(|&i| (self.voices[i].held, self.voices[i].age)).unwrap_or(0),
        };
        let f0 = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
        // a velocity law of its own: 0.2 to 1.0, convex
        let gain = 0.2 + 0.8 * (velocity.min(127) as f64 / 127.0).powf(1.5);
        let start = if self.glide_ms > 0 && self.last_f0 > 0.0 { self.last_f0 } else { f0 };
        self.last_f0 = f0;
        self.voices[i] = Voice { stage: Stage::Attack, note, held: true, age: self.clock, f0: start, tf0: f0, ph: [0.0, 0.0], tri: [0.0; 2], env: 0.0, gain, ic1: 0.0, ic2: 0.0, co: [0.0; 3] };
    }

    fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.held && v.note == note && v.stage != Stage::Idle {
                v.held = false;
                v.stage = Stage::Release;
            }
        }
    }

    fn all_off(&mut self) {
        for v in &mut self.voices {
            if v.stage != Stage::Idle {
                v.held = false;
                v.stage = Stage::Release;
            }
        }
    }

    fn glide(&mut self, ms: i32) {
        self.glide_ms = ms.clamp(0, 5000);
    }

    fn legato(&mut self, from: u8, to: u8) -> bool {
        let to = to.min(127);
        let Some(v) = self.voices.iter_mut().find(|v| v.held && v.note == from && v.stage != Stage::Idle) else { return false };
        v.note = to;
        v.tf0 = 440.0 * 2f64.powf((f64::from(to) - 69.0) / 12.0);
        self.last_f0 = v.tf0;
        true
    }

    fn bend_range(&mut self, semitones: i32) {
        self.bend_semis = f64::from(semitones.clamp(0, 24));
        self.bend = 2f64.powf(f64::from(self.bend_raw - 8192) / 8192.0 * self.bend_semis / 12.0);
    }

    fn pitch_bend(&mut self, value: i32) {
        self.bend_raw = value.clamp(0, 16383);
        // two semitones either way
        self.bend = 2f64.powf(f64::from(self.bend_raw - 8192) / 8192.0 * self.bend_semis / 12.0);
    }

    fn render(&mut self, out: &mut [i16]) {
        let rate = self.rate;
        let step = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / 4.6)).exp();
        let (a_att, a_dec, a_rel) = (step(self.p[P_ATTACK as usize].max(1)), step(self.p[P_DECAY as usize]), step(self.p[P_RELEASE as usize]));
        let sustain = self.p[P_SUSTAIN as usize] as f64 / 99.0;
        let cut = self.p[P_CUTOFF as usize] as f64;
        let amount = self.p[P_ENV_AMOUNT as usize] as f64 / 99.0;
        let k = 2.0 - 1.9 * self.p[P_RESONANCE as usize] as f64 / 99.0;
        let level = self.p[P_LEVEL as usize] as f64 / 100.0 * 0.25;
        let det = 2f64.powf(self.p[P_DETUNE as usize] as f64 / 1200.0);
        let live = self.voices.iter().filter(|v| v.stage != Stage::Idle).count().max(1);
        let poly = if live > 4 { (4.0 / live as f64).sqrt() } else { 1.0 };
        let bend = self.bend;
        // the pitch slides in the log domain, 99% of the way in the glide time (done every eighth sample: a pitch moves slowly against that)
        let slide = if self.glide_ms > 0 { (-4.6 * 8.0 / (f64::from(self.glide_ms) * 0.001 * rate)).exp() } else { 0.0 };
        for (n, o) in out.iter_mut().enumerate() {
            let refresh = n & 7 == 0;
            let mut acc = 0.0;
            for i in 0..VOICES {
                if self.voices[i].stage == Stage::Idle {
                    continue;
                }
                {
                    let v = &mut self.voices[i];
                    match v.stage {
                        Stage::Attack => {
                            v.env += (1.2 - v.env) * a_att;
                            if v.env >= 1.0 {
                                v.env = 1.0;
                                v.stage = Stage::Decay;
                            }
                        }
                        Stage::Decay => {
                            v.env += (sustain - v.env) * a_dec;
                        }
                        Stage::Release => {
                            v.env -= v.env * a_rel;
                            if v.env < 1e-4 {
                                v.stage = Stage::Idle;
                                continue;
                            }
                        }
                        Stage::Idle => continue,
                    }
                }
                if refresh {
                    let v = &mut self.voices[i];
                    if v.f0 != v.tf0 {
                        v.f0 = if slide == 0.0 { v.tf0 } else { v.tf0 * (v.f0 / v.tf0).powf(slide) };
                        if (v.f0 / v.tf0 - 1.0).abs() < 1e-5 {
                            v.f0 = v.tf0;
                        }
                    }
                }
                let f0 = self.voices[i].f0;
                self.voices[i].f0 = f0 * bend;
                let x = 0.5 * (self.osc(i, 0, 1.0) + self.osc(i, 1, det));
                self.voices[i].f0 = f0;
                let v = &mut self.voices[i];
                if refresh || v.co[0] == 0.0 {
                    // cutoff: the knob (0..127 over 40 Hz .. 16 kHz, exponential), opened by the envelope
                    let open = (cut / 127.0 + amount * v.env * (1.0 - cut / 127.0)).clamp(0.0, 1.0);
                    let fc = (40.0 * (400f64).powf(open)).min(rate * 0.45);
                    let g = (PI * fc / rate).tan();
                    let a1 = 1.0 / (1.0 + g * (g + k));
                    v.co = [a1, g * a1, g * g * a1];
                }
                let [a1, a2, a3] = v.co;
                let v3 = x - v.ic2;
                let v1 = a1 * v.ic1 + a2 * v3;
                let v2 = v.ic2 + a2 * v.ic1 + a3 * v3;
                v.ic1 = 2.0 * v1 - v.ic1;
                v.ic2 = 2.0 * v2 - v.ic2;
                acc += v2 * v.env * v.gain;
            }
            *o = (acc * poly * level * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
        }
    }

    fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.stage != Stage::Idle).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(x: &[i16]) -> u16 {
        x.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0)
    }

    /// The frequency of a sine from its rising zero crossings over `x`.
    fn hz(x: &[i16]) -> f64 {
        let n = x.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
        n as f64 * 44_100.0 / x.len() as f64
    }

    fn sine() -> Analog {
        let mut a = Analog::new(44_100.0);
        for (id, v) in [(P_WAVE, 3), (P_ATTACK, 1), (P_RELEASE, 1), (P_SUSTAIN, 99), (P_DETUNE, 0), (P_CUTOFF, 127), (P_LEVEL, 100)] {
            a.set(id, v);
        }
        a
    }

    #[test]
    fn glide_slides_from_the_last_note_to_the_next() {
        let play = |ms: i32| {
            let mut a = sine();
            a.glide(ms);
            let mut buf = vec![0i16; 4410];
            a.note_on(57, 100); // 220 Hz
            a.render(&mut buf);
            a.note_off(57);
            a.render(&mut buf);
            a.note_on(69, 100); // 440 Hz
            let mut out = vec![0i16; 22_050];
            a.render(&mut out);
            out
        };
        let none = play(0);
        assert!((hz(&none[2048..8192]) - 440.0).abs() < 10.0, "no glide: the new pitch at once, {}", hz(&none[2048..8192]));
        let g = play(300);
        let early = hz(&g[1024..3072]);
        assert!(early > 235.0 && early < 400.0, "part way up: {early}");
        let mid = hz(&g[6000..8048]);
        assert!(mid > early + 20.0 && mid < 440.0, "still rising: {early} then {mid}");
        let late = hz(&g[14_000..22_050]);
        assert!((late - 440.0).abs() < 9.0, "arrived: {late}");
        // the first note ever has nothing to slide from
        let mut a = sine();
        a.glide(500);
        a.note_on(69, 100);
        let mut out = vec![0i16; 4096];
        a.render(&mut out);
        assert!((hz(&out[512..4096]) - 440.0).abs() < 14.0, "first note: {}", hz(&out[512..4096]));
        // down as well as up, and a huge value is clamped, not trusted
        let mut a = sine();
        a.glide(i32::MAX);
        a.note_on(81, 100);
        a.note_off(81);
        a.note_on(45, 100);
        let mut out = vec![0i16; 2048];
        a.render(&mut out);
        assert!(out.iter().any(|&v| v != 0));
    }

    #[test]
    fn plays_releases_and_frees() {
        let mut a = Analog::new(44_100.0);
        let mut buf = vec![0i16; 4410];
        a.note_on(57, 100);
        a.render(&mut buf);
        assert!(peak(&buf) > 500);
        assert_eq!(a.sounding(), 1);
        a.note_off(57);
        for _ in 0..20 {
            a.render(&mut buf);
        }
        assert_eq!(a.sounding(), 0, "a released voice is freed");
        a.render(&mut buf);
        assert_eq!(peak(&buf), 0);
    }

    #[test]
    fn pitch_is_right_for_every_wave() {
        for wave in 0..4 {
            let mut a = Analog::new(44_100.0);
            a.set(P_WAVE, wave);
            a.set(P_DETUNE, 0);
            a.set(P_CUTOFF, 127);
            a.set(P_ATTACK, 1);
            a.note_on(69, 127); // 440 Hz
            let mut buf = vec![0i16; 44_100];
            a.render(&mut buf);
            // count rising zero crossings over the last half second
            let seg = &buf[22_050..];
            let n = seg.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
            assert!((218..=222).contains(&n), "wave {wave}: {n} crossings in 0.5 s");
        }
    }

    #[test]
    fn filter_cutoff_darkens_and_params_clamp() {
        let rms = |cut: i32| {
            let mut a = Analog::new(44_100.0);
            a.set(P_CUTOFF, cut);
            a.set(P_ENV_AMOUNT, 0);
            a.note_on(48, 127);
            let mut buf = vec![0i16; 8820];
            a.render(&mut buf);
            (buf.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / buf.len() as f64).sqrt()
        };
        assert!(rms(20) < rms(127) * 0.6);
        let mut a = Analog::new(44_100.0);
        assert!(a.set(P_LEVEL, 1000));
        assert_eq!(a.get(P_LEVEL), Some(100));
        assert!(!a.set(99, 1));
    }

    #[test]
    fn nine_notes_steal_and_stay_bounded() {
        let mut a = Analog::new(44_100.0);
        for n in 40..52 {
            a.note_on(n, 127);
        }
        assert!(a.sounding() <= VOICES);
        let mut buf = vec![0i16; 2048];
        a.render(&mut buf);
        a.pitch_bend(0);
        a.render(&mut buf);
        a.all_off();
    }
}
