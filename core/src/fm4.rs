//! `fm4`: a 4-operator FM engine, written fresh for FM1-NTT (docs/PORTING.md; the engine the OPERATOR / Mighty Arp family needs).
//!
//! Four sine operators, OPERATOR's eleven algorithms (how they modulate each other and which are heard), self-feedback on each operator, a ratio, a fine
//! detune, a level, a velocity sensitivity and an attack / decay / sustain / release envelope for each. Eight voices, oldest stolen. Floating point, deterministic on one
//! platform, no allocation after `new`. **Fitted to no firmware** (OPERATOR and Mighty Arp boot in the emulator but their sound has not been compared).

use crate::engine::{Engine, ParamDesc};
use std::f64::consts::TAU;

pub const VOICES: usize = 8;
pub const OPS: usize = 4;
/// The decibels a decay or release time spans (see `render`).
const ENV_DB: f64 = 64.0;

pub const P_ALGORITHM: u16 = 0;
/// The self-feedback phase swing at 100 %, in hundredths of a radian (OPERATOR 0.7: about 310, a two-sample average as the DX7 does it).
pub const P_FEEDBACK: u16 = 1;
pub const P_LEVEL: u16 = 2;
/// The phase swing of a modulator at full level, in hundredths of a radian (OPERATOR 0.7: 628, one whole cycle; the generic default is 4 radians).
pub const P_INDEX: u16 = 3;
/// How an operator's level maps to amplitude: 0 a DX-style law (0.75 dB a step down from 99, OPERATOR 0.7), 1 linear (level / 100, Mighty Arp 1.5).
pub const P_LEVEL_LAW: u16 = 4;
/// How FINE RATIO acts: 0 a percentage of the coarse ratio (generic; 0 to 99), 1 thousandths added to it (OPERATOR 0.7: coarse 1 and fine 26 is 1.026; 0 to 999).
pub const P_RATIO_LAW: u16 = 5;
/// How SUSTAIN reads: 0 linear (value / 99, generic), 1 decibels (99 is 0 dB and each step below is 1 dB down; OPERATOR 0.7 shows its sustain in dB).
pub const P_SUSTAIN_LAW: u16 = 6;
/// Operator `n` (0 to 3, shown as OP1 to OP4) has `OP_PARAMS` parameters from `OP_BASE + n * OP_PARAMS`.
pub const OP_BASE: u16 = 10;
pub const OP_PARAMS: u16 = 11;
pub const O_RATIO: u16 = 0;
pub const O_FINE: u16 = 1;
pub const O_DETUNE: u16 = 2;
pub const O_LEVEL: u16 = 3;
pub const O_ATTACK: u16 = 4;
pub const O_DECAY: u16 = 5;
pub const O_SUSTAIN: u16 = 6;
pub const O_RELEASE: u16 = 7;
pub const O_VELOCITY: u16 = 8;
pub const O_KEYSCALE: u16 = 9;
pub const O_FEEDBACK: u16 = 10;
/// The second per-operator block (2026-11-19, from OPERATOR 0.7's pages): `EXT_PARAMS` parameters per operator from `EXT_BASE`, after the first block so every older id
/// keeps its number. `ext_param(op, E_*)` is the id.
pub const EXT_BASE: u16 = OP_BASE + OPS as u16 * OP_PARAMS;
pub const EXT_PARAMS: u16 = 6;
/// The waveform, `WAVES` (0 to 18).
pub const E_WAVE: u16 = 0;
/// The phase a note starts at, in percent of a cycle (OPERATOR's Phase; 66 % starts the sine at -0.83).
pub const E_PHASE: u16 = 1;
/// Key scaling in percent, -100 to 100, about note 60: the level changes by `2^(percent / 100 * octaves)` (OPERATOR's Key: +100 % is 6 dB an octave up).
pub const E_KEYSCALE: u16 = 2;
/// Fixed-frequency mode (OPERATOR's Fixed): 1 plays `FIXED FREQ` x `FIXED MULTI` Hz whatever the note.
pub const E_FIXED: u16 = 3;
/// The fixed frequency in Hz, 10 to 1024 (OPERATOR's Freq).
pub const E_FIXFREQ: u16 = 4;
/// The fixed frequency's multiplier: 0 to 4 for x0.001, x0.01, x0.1, x1, x10 (OPERATOR's Multi).
pub const E_FIXMULTI: u16 = 5;
/// The voice filter (OPERATOR's Filter page, fitted 2026-11-19): after the operators, before the master level. Off by default so older patches are unchanged.
pub const FILT_BASE: u16 = EXT_BASE + OPS as u16 * EXT_PARAMS;
pub const F_ON: u16 = 0;
/// 0 low-pass, 1 high-pass, 2 band-pass, 3 notch.
pub const F_TYPE: u16 = 1;
/// The cutoff as OPERATOR displays it, Hz (30 to 20000). The filter's own corner is this times `2^(key / 100 x (note - 60) / 12)`.
pub const F_FREQ: u16 = 2;
/// OPERATOR's Res, in hundredths (0 to 125); the quality factor is `0.5 / (1 - 0.97 x min(res, 0.93))` (0.5 at 0, 0.65 at 0.27).
pub const F_RES: u16 = 3;
/// 0 for 12 dB an octave (one two-pole stage), 1 for 24 dB (two stages).
pub const F_SLOPE: u16 = 4;
/// Key tracking in percent (OPERATOR's Freq<Key; its default is +100).
pub const F_KEY: u16 = 5;
pub const FILT_PARAMS: u16 = 6;
/// The LFO (OPERATOR's LFO page, measured 2026-11-20): pitch only so far. Off by default so older patches are unchanged.
pub const LFO_BASE: u16 = FILT_BASE + FILT_PARAMS;
pub const L_ON: u16 = 0;
/// 0 sine, 1 square, 2 triangle, 3 saw up, 4 saw down, 5 sample and hold (a new value each cycle), 6 noise (a new value 16 times a cycle). Types 5 and 6 are not fitted.
pub const L_TYPE: u16 = 1;
/// Hertz times 100 (OPERATOR's default is 6.32 Hz).
pub const L_RATE: u16 = 2;
/// Percent: the pitch swings by plus and minus an octave times this (measured: 10 % gave 120 cents, 69 % gave 827, 12 cents a percent).
pub const L_AMOUNT: u16 = 3;
/// 1 each note restarts its own LFO at phase 0, 0 one LFO runs free for every voice.
pub const L_RETRIG: u16 = 4;
/// Bit k set: operator k's pitch follows the LFO (OPERATOR's Osc A to D buttons; all four by default); bit 4 the filter's cutoff.
pub const L_DEST: u16 = 5;
/// OPERATOR's Amount A, percent (-100 to 100): scales the LFO's depth on every target of destination A (the operators and the filter); the pitch and cutoff swing are proportional to it (34 % gave 0.35 of the 100 % swing).
pub const L_AMOUNT_A: u16 = 6;
/// OPERATOR's Dest B (one target, its own Amount B): 0 Off, 1 to 4 Osc Vol A to D, 5 X-Fade A/C, 6 X-Fade B/D, 7 Feedback, 8 Fixed Freq, 9 FM Drive, 10 Filter Freq, 11 Filter Q,
/// 12 Filter Res, 13 Morph, 14 Flt Drive, 15 Filter Env, 16 Shp Drive, 17 LFO Rate, 18 LFO Amt, 19 Pitch Env, 20 Volume, 21 Pan, 22 Tone, 23 Time (the menu's order). **Only Osc Vol A to D
/// and Volume act so far**: the gain is 1 plus the LFO's amount times Amount B times its value (measured on Osc Vol A: 10 % gave 0.9 to 1.1, 69 % 0.32 to 1.69, 100 % 0 to 2).
pub const L_DEST_B: u16 = 7;
pub const L_AMOUNT_B: u16 = 8;
pub const LFO_PARAMS: u16 = 9;
/// Bit 4 of `L_DEST`: the filter's cutoff follows the LFO. Measured: the cutoff swings by plus and minus this many octaves at 100 % (a sine, geometrically about its own value).
pub const L_FILTER_BIT: i32 = 16;
const L_FILTER_OCTAVES: f64 = 2.84;
pub const P_COUNT: usize = LFO_BASE as usize + LFO_PARAMS as usize;
const MULTI: [f64; 5] = [0.001, 0.01, 0.1, 1.0, 10.0];

/// Parameter id of operator `op` (0 to 3), field `E_*` of the second block.
pub const fn ext_param(op: usize, field: u16) -> u16 {
    EXT_BASE + op as u16 * EXT_PARAMS + field
}
/// The waveform names, in OPERATOR 0.7's order: Sine, the sine in 4 and 8 bits, a saw of N harmonics (3 4 6 8 16 32 64) and an ideal saw (D), a square of N odd
/// harmonics and an ideal square. A table of 2048 points each, built once, peak 1 (measured: harmonics at 1/k, every wave the same peak level).
pub const WAVES: [&str; 19] = [
    "SINE", "SINE 4 BIT", "SINE 8 BIT", "SAW 3", "SAW 4", "SAW 6", "SAW 8", "SAW 16", "SAW 32", "SAW 64", "SAW D", "SQUARE 3", "SQUARE 4", "SQUARE 6", "SQUARE 8", "SQUARE 16",
    "SQUARE 32", "SQUARE 64", "SQUARE D",
];
const TABLE: usize = 2048;

/// One cycle of waveform `w`, `TABLE` points plus a repeated first for interpolation, scaled to peak 1.
fn make_wave(w: usize) -> Vec<f32> {
    let n_of = |i: usize| [3usize, 4, 6, 8, 16, 32, 64][i];
    let mut t: Vec<f64> = (0..TABLE)
        .map(|i| {
            let th = TAU * i as f64 / TABLE as f64;
            let ph = i as f64 / TABLE as f64;
            match w {
                0 => th.sin(),
                1 | 2 => {
                    let q = if w == 1 { 16.0 } else { 256.0 };
                    ((th.sin() + 1.0) * 0.5 * (q - 1.0)).round() / (q - 1.0) * 2.0 - 1.0
                }
                3..=9 => (1..=n_of(w - 3)).map(|k| (th * k as f64).sin() / k as f64).sum(),
                10 => 1.0 - 2.0 * ph,
                11..=17 => (0..n_of(w - 11)).map(|j| (th * (2 * j + 1) as f64).sin() / (2 * j + 1) as f64).sum(),
                _ => {
                    if ph < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
            }
        })
        .collect();
    let peak = t.iter().fold(0.0f64, |a, v| a.max(v.abs())).max(1e-9);
    t.iter_mut().for_each(|v| *v /= peak);
    let mut out: Vec<f32> = t.iter().map(|v| *v as f32).collect();
    out.push(out[0]);
    out
}

/// Parameter id of operator `op` (0 to 3), field `field` (`O_*`).
pub const fn op_param(op: usize, field: u16) -> u16 {
    OP_BASE + op as u16 * OP_PARAMS + field
}

/// name, min, max, default, unit
const GLOBAL: [(&str, i32, i32, i32, &str); 7] = [("ALGORITHM (1-11)", 0, 10, 0, ""), ("FEEDBACK AT 100% (0.01 RAD)", 0, 1000, 310, ""), ("LEVEL", 0, 100, 70, "%"), ("MOD INDEX (0.01 RAD)", 1, 2000, 400, ""), ("LEVEL LAW (0 DB STEPS 1 LINEAR)", 0, 1, 0, ""), ("RATIO LAW (0 PERCENT 1 THOUSANDTHS)", 0, 1, 0, ""), ("SUSTAIN LAW (0 LINEAR 1 DB)", 0, 1, 0, "")];
const PER_OP: [(&str, i32, i32, i32, &str); OP_PARAMS as usize] = [
    ("RATIO (0 = 0.5)", 0, 48, 1, ""),
    ("FINE RATIO", 0, 999, 0, ""),
    ("DETUNE", -50, 50, 0, "cents"),
    ("LEVEL", 0, 100, 99, ""),
    ("ATTACK", 0, 5000, 2, "ms"),
    ("DECAY", 1, 5000, 400, "ms"),
    ("SUSTAIN", 0, 99, 60, ""),
    ("RELEASE", 1, 5000, 250, "ms"),
    ("VELOCITY SENS", 0, 100, 50, "%"),
    ("KEY SCALING", 0, 7, 0, ""),
    ("FEEDBACK", 0, 100, 0, "%"),
];

fn desc(id: usize) -> Option<(String, i32, i32, i32, &'static str)> {
    if id < GLOBAL.len() {
        let d = GLOBAL[id];
        return Some((d.0.to_string(), d.1, d.2, d.3, d.4));
    }
    if id >= LFO_BASE as usize {
        let (name, lo, hi, def, unit): (&str, i32, i32, i32, &'static str) = match (id - LFO_BASE as usize) as u16 {
            L_ON => ("LFO (0 OFF 1 ON)", 0, 1, 0, ""),
            L_TYPE => ("LFO TYPE (0 SINE 1 SQR 2 TRI 3 SAW UP 4 SAW DN 5 S&H 6 NOISE)", 0, 6, 0, ""),
            L_RATE => ("LFO RATE (0.01 HZ)", 1, 10_000, 632, ""),
            L_AMOUNT => ("LFO AMOUNT (PITCH)", 0, 100, 10, "%"),
            L_RETRIG => ("LFO RETRIG (0 FREE 1 PER NOTE)", 0, 1, 1, ""),
            L_DEST => ("LFO TARGETS (BITS: 1-8 OPERATORS 16 FILTER)", 0, 31, 15, ""),
            L_AMOUNT_A => ("LFO AMOUNT A", -100, 100, 100, "%"),
            L_DEST_B => ("LFO DEST B (1-4 OSC VOL, 20 VOLUME; REST NOT YET)", 0, 23, 0, ""),
            L_AMOUNT_B => ("LFO AMOUNT B", -100, 100, 100, "%"),
            _ => return None,
        };
        return Some((name.to_string(), lo, hi, def, unit));
    }
    if id >= FILT_BASE as usize {
        let (name, lo, hi, def, unit): (&str, i32, i32, i32, &'static str) = match (id - FILT_BASE as usize) as u16 {
            F_ON => ("FILTER (0 OFF 1 ON)", 0, 1, 0, ""),
            F_TYPE => ("FILTER TYPE (0 LP 1 HP 2 BP 3 NOTCH)", 0, 3, 0, ""),
            F_FREQ => ("FILTER FREQ", 30, 20000, 12000, "Hz"),
            F_RES => ("FILTER RES (0.01)", 0, 125, 27, ""),
            F_SLOPE => ("FILTER SLOPE (0 12 DB 1 24 DB)", 0, 1, 1, ""),
            F_KEY => ("FILTER KEY TRACK", -100, 100, 100, "%"),
            _ => return None,
        };
        return Some((name.to_string(), lo, hi, def, unit));
    }
    if id >= EXT_BASE as usize {
        let i = id - EXT_BASE as usize;
        if i >= OPS * EXT_PARAMS as usize {
            return None;
        }
        let (op, f) = (i / EXT_PARAMS as usize, i % EXT_PARAMS as usize);
        let (name, lo, hi, def, unit): (&str, i32, i32, i32, &'static str) = match f as u16 {
            E_WAVE => ("WAVE (0 SINE ... 18 SQUARE D)", 0, WAVES.len() as i32 - 1, 0, ""),
            E_PHASE => ("PHASE", 0, 100, 0, "%"),
            E_KEYSCALE => ("KEY SCALE (ABOUT NOTE 60)", -100, 100, 0, "%"),
            E_FIXED => ("FIXED (0 OFF 1 ON)", 0, 1, 0, ""),
            E_FIXFREQ => ("FIXED FREQ", 10, 1024, 100, "Hz"),
            _ => ("FIXED MULTI (0 X0.001 ... 4 X10)", 0, 4, 3, ""),
        };
        return Some((format!("OP{} {}", op + 1, name), lo, hi, def, unit));
    }
    let i = id.checked_sub(OP_BASE as usize)?;
    if i >= OPS * OP_PARAMS as usize {
        return None;
    }
    let (op, f) = (i / OP_PARAMS as usize, i % OP_PARAMS as usize);
    let d = PER_OP[f];
    // the default of a quiet operator: only OP1 is loud, so a fresh patch is one clean sine until the player opens the others
    let default = if f == O_LEVEL as usize && op > 0 { 0 } else { d.3 };
    Some((format!("OP{} {}", op + 1, d.0), d.1, d.2, default, d.4))
}

/// For each algorithm: `modulators[op]` is the mask of operators (bit n = OPn+1) that modulate `op`, and `carriers` the mask of those heard. Operators are
/// computed 4, 3, 2, 1, so an operator is modulated only by a higher-numbered one.
struct Algorithm {
    modulators: [u8; OPS],
    carriers: u8,
}

const fn m(ops: &[usize]) -> u8 {
    let mut i = 0;
    let mut v = 0u8;
    while i < ops.len() {
        v |= 1 << ops[i];
        i += 1;
    }
    v
}

/// OPERATOR 0.7's eleven algorithms, read off its panel's diagrams (2026-11-18; A is OP1, D is OP4; algorithms 1 and 2 are confirmed by their sound, the rest
/// by the drawing and, where `fm4fit` was run, by the spectrum, docs/PORTING.md).
const ALGORITHMS: [Algorithm; 11] = [
    Algorithm { modulators: [m(&[1]), m(&[2]), m(&[3]), 0], carriers: m(&[0]) },       //  1: D > C > B > A
    Algorithm { modulators: [m(&[1]), m(&[2, 3]), 0, 0], carriers: m(&[0]) },          //  2: (C + D) > B > A
    Algorithm { modulators: [m(&[1, 3]), m(&[2]), 0, 0], carriers: m(&[0]) },          //  3: C > B > A, D > A
    Algorithm { modulators: [m(&[1, 2]), 0, m(&[3]), 0], carriers: m(&[0]) },          //  4: D > C > A, B > A
    Algorithm { modulators: [m(&[1, 2]), m(&[3]), m(&[3]), 0], carriers: m(&[0]) },    //  5: D > B > A, D > C > A
    Algorithm { modulators: [m(&[1, 2, 3]), 0, 0, 0], carriers: m(&[0]) },             //  6: B + C + D > A
    Algorithm { modulators: [m(&[1]), 0, m(&[3]), 0], carriers: m(&[0, 2]) },          //  7: B > A and D > C
    Algorithm { modulators: [0, m(&[2]), m(&[3]), 0], carriers: m(&[0, 1]) },          //  8: D > C > B, A alone
    Algorithm { modulators: [m(&[3]), m(&[3]), m(&[3]), 0], carriers: m(&[0, 1, 2]) }, //  9: D > A, B, C
    Algorithm { modulators: [0, 0, m(&[3]), 0], carriers: m(&[0, 1, 2]) },             // 10: D > C, A and B alone
    Algorithm { modulators: [0, 0, 0, 0], carriers: m(&[0, 1, 2, 3]) },                // 11: all four alone
];

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy)]
struct Op {
    stage: Stage,
    ph: f64,
    env: f64,
    out: f64,
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    note: u8,
    held: bool,
    age: u64,
    f0: f64,
    /// where the pitch is going (`f0` slides to it when glide is on)
    tf0: f64,
    /// this note's LFO phase (0 to 1) and its held random value
    lph: f64,
    lval: f64,
    vel: f64,
    ops: [Op; OPS],
    fb: [[f64; 2]; OPS],
    /// the filter's two stages, transposed direct form II: (z1, z2) each
    fz: [[f64; 2]; 2],
}

const IDLE_OP: Op = Op { stage: Stage::Idle, ph: 0.0, env: 0.0, out: 0.0 };
const IDLE: Voice = Voice { active: false, note: 0, held: false, age: 0, f0: 0.0, tf0: 0.0, lph: 0.0, lval: 0.0, vel: 0.0, ops: [IDLE_OP; OPS], fb: [[0.0; 2]; OPS], fz: [[0.0; 2]; 2] };

pub struct Fm4 {
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
    /// the free-running LFO's phase and the noise generator
    lfo_ph: f64,
    rng: u32,
    waves: Vec<Vec<f32>>,
}

/// A two-pole filter's coefficients `[b0, b1, b2, a1, a2]` (a0 = 1), the bilinear form of `1 / (s^2 + s / Q + 1)` and its high-pass, band-pass (peak gain 1) and
/// notch relatives, for a corner of `fc` Hz.
fn biquad(kind: i32, fc: f64, q: f64, rate: f64) -> [f64; 5] {
    let w = TAU * fc / rate;
    let (sn, cs) = (w.sin(), w.cos());
    let al = sn / (2.0 * q);
    let a0 = 1.0 + al;
    let (b0, b1, b2) = match kind {
        1 => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0),
        2 => (al, 0.0, -al),
        3 => (1.0, -2.0 * cs, 1.0),
        _ => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0),
    };
    [b0 / a0, b1 / a0, b2 / a0, -2.0 * cs / a0, (1.0 - al) / a0]
}

/// The amplitude of a level 0 to 99: 0.75 dB a step down from 99 (a DX-like law), and silence at 0.
fn amp_of(level: i32, linear: bool) -> f64 {
    if linear {
        return level.clamp(0, 100) as f64 / 100.0;
    }
    let level = level.min(99);
    if level <= 0 {
        0.0
    } else {
        10f64.powf((level - 99) as f64 * 0.75 / 20.0)
    }
}

impl Fm4 {
    pub fn new(rate: f64) -> Fm4 {
        let rate = rate.clamp(8000.0, 192_000.0);
        let mut p = [0; P_COUNT];
        for (i, v) in p.iter_mut().enumerate() {
            if let Some(d) = desc(i) {
                *v = d.3;
            }
        }
        Fm4 { rate, p, voices: [IDLE; VOICES], clock: 0, bend: 1.0, bend_raw: 8192, bend_semis: 2.0, glide_ms: 0, last_f0: 0.0, lfo_ph: 0.0, rng: 0x1234_5678, waves: (0..WAVES.len()).map(make_wave).collect() }
    }

    fn op(&self, op: usize, field: u16) -> i32 {
        self.p[op_param(op, field) as usize]
    }

    fn ratio(&self, op: usize) -> f64 {
        let c = self.op(op, O_RATIO);
        let base = if c == 0 { 0.5 } else { c as f64 };
        let fine = self.op(op, O_FINE) as f64;
        let ratio = if self.p[P_RATIO_LAW as usize] == 1 { base + fine / 1000.0 } else { base * (1.0 + fine.min(99.0) / 100.0) };
        ratio * 2f64.powf(self.op(op, O_DETUNE) as f64 / 1200.0)
    }
}

impl Engine for Fm4 {
    fn kind(&self) -> &'static str {
        "fm4"
    }

    fn params(&self) -> Vec<ParamDesc> {
        (0..P_COUNT)
            .filter_map(|i| desc(i).map(|d| ParamDesc { id: i as u16, name: d.0, min: d.1, max: d.2, default: d.3, unit: d.4 }))
            .collect()
    }

    fn get(&self, id: u16) -> Option<i32> {
        desc(id as usize).map(|_| self.p[id as usize])
    }

    fn set(&mut self, id: u16, value: i32) -> bool {
        let Some(d) = desc(id as usize) else { return false };
        self.p[id as usize] = value.clamp(d.1, d.2);
        true
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(note);
        }
        let note = note.min(127);
        self.clock += 1;
        let i = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => (0..VOICES).min_by_key(|&i| (self.voices[i].held, self.voices[i].age)).unwrap_or(0),
        };
        let mut v = IDLE;
        v.active = true;
        v.note = note;
        v.held = true;
        v.age = self.clock;
        let f0 = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
        v.f0 = if self.glide_ms > 0 && self.last_f0 > 0.0 { self.last_f0 } else { f0 };
        v.tf0 = f0;
        v.lph = 0.0;
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        v.lval = f64::from(self.rng >> 8) / f64::from(1u32 << 23) - 1.0;
        self.last_f0 = f0;
        v.vel = velocity.min(127) as f64 / 127.0;
        for (k, o) in v.ops.iter_mut().enumerate() {
            o.stage = Stage::Attack;
            o.ph = self.p[ext_param(k, E_PHASE) as usize] as f64 / 100.0;
        }
        self.voices[i] = v;
    }

    fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.active && v.held && v.note == note {
                v.held = false;
                for o in &mut v.ops {
                    o.stage = Stage::Release;
                }
            }
        }
    }

    fn all_off(&mut self) {
        for v in &mut self.voices {
            if v.active {
                v.held = false;
                for o in &mut v.ops {
                    o.stage = Stage::Release;
                }
            }
        }
    }

    fn glide(&mut self, ms: i32) {
        self.glide_ms = ms.clamp(0, 5000);
    }

    fn legato(&mut self, from: u8, to: u8) -> bool {
        let to = to.min(127);
        let Some(v) = self.voices.iter_mut().find(|v| v.active && v.held && v.note == from) else { return false };
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
        self.bend = 2f64.powf(f64::from(self.bend_raw - 8192) / 8192.0 * self.bend_semis / 12.0);
    }

    fn render(&mut self, out: &mut [i16]) {
        let rate = self.rate;
        // per operator, once a block: the envelope's three rates, the sustain, the level, the ratio and the velocity law
        let mut ratio = [0.0; OPS];
        let mut a = [[0.0; 3]; OPS];
        let mut sustain = [0.0; OPS];
        let mut wave = [0usize; OPS];
        let mut fixed = [0.0; OPS]; // a fixed operator's frequency in Hz, or 0
        let mut kscale2 = [0.0; OPS];
        let mut level = [0.0; OPS];
        let mut vsens = [0.0; OPS];
        let mut kscale = [0.0; OPS];
        // decay and release times are the time to fall 64 dB (fitted to OPERATOR 0.7: 399 ms fell 64 dB, 16 dB a 100 ms); the attack aims past 1 and stops at 1
        let fall = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / (ENV_DB / 8.685_889_6))).exp();
        let step = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / 4.6)).exp();
        for op in 0..OPS {
            ratio[op] = self.ratio(op);
            a[op] = [step(self.op(op, O_ATTACK).max(1)), fall(self.op(op, O_DECAY)), fall(self.op(op, O_RELEASE))];
            let sv = self.op(op, O_SUSTAIN);
            sustain[op] = if self.p[P_SUSTAIN_LAW as usize] == 1 { 10f64.powf((sv - 99) as f64 / 20.0) } else { sv as f64 / 99.0 };
            wave[op] = self.p[ext_param(op, E_WAVE) as usize].clamp(0, WAVES.len() as i32 - 1) as usize;
            if self.p[ext_param(op, E_FIXED) as usize] == 1 {
                fixed[op] = self.p[ext_param(op, E_FIXFREQ) as usize] as f64 * MULTI[self.p[ext_param(op, E_FIXMULTI) as usize].clamp(0, 4) as usize];
            }
            kscale2[op] = self.p[ext_param(op, E_KEYSCALE) as usize] as f64 / 100.0;
            level[op] = amp_of(self.op(op, O_LEVEL), self.p[P_LEVEL_LAW as usize] == 1);
            vsens[op] = self.op(op, O_VELOCITY) as f64 / 100.0;
            kscale[op] = self.op(op, O_KEYSCALE) as f64 / 7.0;
        }
        let alg = &ALGORITHMS[self.p[P_ALGORITHM as usize].clamp(0, 10) as usize];
        // feedback: each operator's own output (level and envelope included) fed back into its phase, the mean of the last two samples, up to P_FEEDBACK at 100 %
        let fb_max = self.p[P_FEEDBACK as usize] as f64 / 100.0;
        let mut fb = [0.0; OPS];
        for (op, f) in fb.iter_mut().enumerate() {
            *f = self.op(op, O_FEEDBACK) as f64 / 100.0 * fb_max;
        }
        let master = self.p[P_LEVEL as usize] as f64 / 100.0 * 0.3;
        let live = self.voices.iter().filter(|v| v.active).count().max(1);
        let poly = if live > 4 { (4.0 / live as f64).sqrt() } else { 1.0 };
        let bend = self.bend;
        // the filter: per voice (the corner follows the note), computed once a block
        let f_on = self.p[(FILT_BASE + F_ON) as usize] == 1;
        let f_stages = if self.p[(FILT_BASE + F_SLOPE) as usize] == 1 { 2 } else { 1 };
        let f_res = (self.p[(FILT_BASE + F_RES) as usize] as f64 / 100.0).min(0.93);
        let f_q = 0.5 / (1.0 - 0.97 * f_res);
        let mut fco = [[0.0f64; 5]; VOICES];
        let mut fkey = [0.0f64; VOICES];
        let f_kind = self.p[(FILT_BASE + F_TYPE) as usize];
        if f_on {
            let kind = self.p[(FILT_BASE + F_TYPE) as usize];
            for (i, v) in self.voices.iter().enumerate() {
                let keyed = self.p[(FILT_BASE + F_FREQ) as usize] as f64 * 2f64.powf(self.p[(FILT_BASE + F_KEY) as usize] as f64 / 100.0 * (v.note as f64 - 60.0) / 12.0);
                fkey[i] = keyed;
                fco[i] = biquad(kind, keyed.clamp(5.0, rate * 0.45), f_q, rate);
            }
        }
        // the pitch slides in the log domain, 99% of the way in the glide time, stepped every eighth sample
        let slide = if self.glide_ms > 0 { (-4.6 * 8.0 / (f64::from(self.glide_ms) * 0.001 * rate)).exp() } else { 0.0 };
        // the LFO (pitch): its phase step a sample, its depth in octaves at a value of 1
        let lfo_on = self.p[(LFO_BASE + L_ON) as usize] == 1;
        let (l_type, l_retrig, l_dest) = (self.p[(LFO_BASE + L_TYPE) as usize], self.p[(LFO_BASE + L_RETRIG) as usize] == 1, self.p[(LFO_BASE + L_DEST) as usize]);
        let l_dph = f64::from(self.p[(LFO_BASE + L_RATE) as usize]) / 100.0 / rate;
        let l_depth = f64::from(self.p[(LFO_BASE + L_AMOUNT) as usize]) / 100.0 * f64::from(self.p[(LFO_BASE + L_AMOUNT_A) as usize]) / 100.0;
        let l_filter = lfo_on && l_dest & L_FILTER_BIT != 0;
        let l_dest_b = self.p[(LFO_BASE + L_DEST_B) as usize];
        let l_depth_b = f64::from(self.p[(LFO_BASE + L_AMOUNT_B) as usize]) / 100.0 * f64::from(self.p[(LFO_BASE + L_AMOUNT) as usize]) / 100.0;
        let (mut free_ph, mut rng) = (self.lfo_ph, self.rng);
        let steps = if l_type == 6 { 16.0 } else { 1.0 };
        let index = self.p[P_INDEX as usize] as f64 / 100.0; // the phase swing (radians) of a modulator at full level
        for (n, o) in out.iter_mut().enumerate() {
            let refresh = n & 7 == 0;
            let mut acc = 0.0;
            for (vi, v) in self.voices.iter_mut().enumerate().filter(|(_, v)| v.active) {
                if refresh && v.f0 != v.tf0 {
                    v.f0 = if slide == 0.0 { v.tf0 } else { v.tf0 * (v.f0 / v.tf0).powf(slide) };
                    if (v.f0 / v.tf0 - 1.0).abs() < 1e-5 {
                        v.f0 = v.tf0;
                    }
                }
                let mut lmul = 1.0;
                let mut lshape = 0.0;
                let mut gain_b = 1.0;
                if lfo_on {
                    let prev = if l_retrig { v.lph } else { free_ph };
                    let ph = if l_retrig { v.lph } else { free_ph };
                    let shape = match l_type {
                        0 => (std::f64::consts::TAU * ph).sin(),
                        1 => {
                            if ph < 0.5 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                        2 => {
                            if ph < 0.5 {
                                1.0 - 4.0 * ph
                            } else {
                                4.0 * ph - 3.0
                            }
                        }
                        3 => 2.0 * ph - 1.0,
                        4 => 1.0 - 2.0 * ph,
                        _ => v.lval,
                    };
                    lshape = shape;
                    gain_b = (1.0 + l_depth_b * shape).max(0.0);
                    lmul = 2f64.powf(l_depth * shape);
                    let next = ph + l_dph;
                    let nph = next - next.floor();
                    if l_type >= 5 && ((nph * steps).floor() != (prev * steps).floor() || nph < prev) {
                        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        v.lval = f64::from(rng >> 8) / f64::from(1u32 << 23) - 1.0;
                    }
                    if l_retrig {
                        v.lph = nph;
                    }
                }
                let mut heard = 0.0;
                for op in (0..OPS).rev() {
                    let s = &mut v.ops[op];
                    match s.stage {
                        Stage::Attack => {
                            s.env += (1.2 - s.env) * a[op][0];
                            if s.env >= 1.0 {
                                s.env = 1.0;
                                s.stage = Stage::Decay;
                            }
                        }
                        Stage::Decay => s.env += (sustain[op] - s.env) * a[op][1],
                        Stage::Release => s.env -= s.env * a[op][2],
                        Stage::Idle => {}
                    }
                    let mut pm = 0.0;
                    for (k, o) in v.ops.iter().enumerate().take(OPS) {
                        if alg.modulators[op] >> k & 1 == 1 {
                            pm += index * o.out;
                        }
                    }
                    let s = &mut v.ops[op];
                    pm += fb[op] * 0.5 * (v.fb[op][0] + v.fb[op][1]);
                    let inc = if fixed[op] > 0.0 { fixed[op] / rate } else { v.f0 * bend * ratio[op] * if l_dest >> op & 1 == 1 { lmul } else { 1.0 } / rate };
                    s.ph += inc;
                    s.ph -= s.ph.floor();
                    // velocity: linear, from (1 - sensitivity) at velocity 0 to 1 at 127 (OPERATOR 0.7 at its default of 50%: half at 0, measured); key scaling: higher notes quieter
                    let vel = 1.0 - vsens[op] * (1.0 - v.vel);
                    let key = 1.0 - kscale[op] * ((v.note as f64 - 36.0) / 91.0).clamp(0.0, 1.0) * 0.75;
                    let key2 = 2f64.powf(kscale2[op] * (v.note as f64 - 60.0) / 12.0);
                    let amp = level[op] * s.env * vel.max(0.05) * key * key2 * if l_dest_b == 1 + op as i32 { gain_b } else { 1.0 };
                    s.out = if wave[op] == 0 {
                        (TAU * (s.ph) + pm).sin() * amp
                    } else {
                        // a table lookup at the phase (cycles) plus the modulation, linearly interpolated
                        let x = (s.ph + pm / TAU).rem_euclid(1.0) * TABLE as f64;
                        let (i, f) = ((x as usize).min(TABLE - 1), x - (x as usize).min(TABLE - 1) as f64);
                        let t = &self.waves[wave[op]];
                        (t[i] as f64 + (t[i + 1] - t[i]) as f64 * f) * amp
                    };
                    v.fb[op][1] = v.fb[op][0];
                    v.fb[op][0] = s.out;
                    if alg.carriers >> op & 1 == 1 {
                        heard += s.out;
                    }
                }
                // a voice ends once every heard operator has released to silence
                if !v.held && (0..OPS).all(|k| alg.carriers >> k & 1 == 0 || v.ops[k].env < 1e-4) {
                    v.active = false;
                }
                if f_on && l_filter && n & 7 == 0 {
                    fco[vi] = biquad(f_kind, (fkey[vi] * 2f64.powf(L_FILTER_OCTAVES * l_depth * lshape)).clamp(5.0, rate * 0.45), f_q, rate);
                }
                if f_on {
                    let c = &fco[vi];
                    for z in v.fz.iter_mut().take(f_stages) {
                        let y = c[0] * heard + z[0];
                        z[0] = c[1] * heard - c[3] * y + z[1];
                        z[1] = c[2] * heard - c[4] * y;
                        heard = y;
                    }
                }
                acc += heard * if l_dest_b == 20 { gain_b } else { 1.0 };
            }
            *o = (acc * poly * master * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
            if lfo_on && !l_retrig {
                free_ph += l_dph;
                free_ph -= free_ph.floor();
            }
        }
        self.lfo_ph = free_ph;
        self.rng = rng;
    }

    fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(x: &[i16]) -> u16 {
        x.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0)
    }

    fn crossings(x: &[i16]) -> usize {
        x.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count()
    }

    /// A patch of one audible sine: algorithm 8 (all alone), only OP1 open.
    fn sine() -> Fm4 {
        let mut e = Fm4::new(44_100.0);
        e.set(P_ALGORITHM, 10);
        e.set(op_param(0, O_ATTACK), 1);
        e.set(op_param(0, O_SUSTAIN), 99);
        e
    }

    #[test]
    fn a_fresh_patch_is_one_sine_at_the_right_pitch() {
        let mut e = sine();
        e.note_on(69, 127); // 440 Hz
        let mut buf = vec![0i16; 44_100];
        e.render(&mut buf);
        assert!(peak(&buf) > 1000);
        let n = crossings(&buf[22_050..]);
        assert!((218..=222).contains(&n), "{n} crossings in 0.5 s");
    }

    #[test]
    fn ratio_fine_and_detune_move_the_pitch() {
        let at = |c: i32, fine: i32, cents: i32| {
            let mut e = sine();
            e.set(op_param(0, O_RATIO), c);
            e.set(op_param(0, O_FINE), fine);
            e.set(op_param(0, O_DETUNE), cents);
            e.note_on(57, 127); // 220 Hz
            let mut buf = vec![0i16; 44_100];
            e.render(&mut buf);
            crossings(&buf[22_050..]) as f64 * 2.0
        };
        assert!((at(1, 0, 0) - 220.0).abs() < 6.0);
        assert!((at(2, 0, 0) - 440.0).abs() < 8.0);
        assert!((at(0, 0, 0) - 110.0).abs() < 6.0, "ratio 0 is one half");
        assert!((at(1, 50, 0) - 330.0).abs() < 8.0, "fine 50% is a fifth up");
        assert!(at(1, 0, 50) > at(1, 0, 0));
    }

    #[test]
    fn a_modulator_adds_harmonics_and_more_level_adds_more() {
        // the sum of absolute sample-to-sample steps grows as the spectrum brightens (at a fixed carrier level)
        let bright = |mod_level: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, 0); // 4 > 3 > 2 > 1
            for op in 0..3 {
                e.set(op_param(op, O_LEVEL), 0);
            }
            e.set(op_param(0, O_LEVEL), 99);
            e.set(op_param(1, O_LEVEL), mod_level);
            e.set(op_param(1, O_RATIO), 3);
            e.set(op_param(1, O_SUSTAIN), 99);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.note_on(48, 127);
            let mut buf = vec![0i16; 8820];
            e.render(&mut buf);
            buf[4000..].windows(2).map(|w| (w[1] as f64 - w[0] as f64).abs()).sum::<f64>()
        };
        let none = bright(0);
        let some = bright(80);
        let lots = bright(99);
        assert!(some > none * 1.2 && lots > some, "{none} {some} {lots}");
    }

    #[test]
    fn every_algorithm_sounds_and_the_algorithms_differ() {
        let mut shapes = Vec::new();
        for alg in 0..11 {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, alg);
            for op in 0..OPS {
                e.set(op_param(op, O_LEVEL), 90);
                e.set(op_param(op, O_RATIO), (op as i32 + 1) * 2 - 1);
                e.set(op_param(op, O_SUSTAIN), 90);
            }
            e.note_on(52, 100);
            let mut buf = vec![0i16; 6000];
            e.render(&mut buf);
            assert!(peak(&buf) > 300, "algorithm {} is silent", alg + 1);
            shapes.push(buf);
        }
        for i in 0..11 {
            for j in i + 1..11 {
                assert_ne!(shapes[i], shapes[j], "algorithms {} and {} sound the same", i + 1, j + 1);
            }
        }
    }

    #[test]
    fn feedback_changes_the_tone_and_stays_bounded() {
        let run = |fb: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, 10);
            e.set(op_param(3, O_LEVEL), 99);
            e.set(op_param(0, O_LEVEL), 0);
            e.set(op_param(3, O_FEEDBACK), fb);
            e.note_on(48, 127);
            let mut buf = vec![0i16; 8820];
            e.render(&mut buf);
            buf
        };
        let (clean, fed) = (run(0), run(100));
        assert_ne!(clean, fed);
        let step = |b: &[i16]| b[2000..].windows(2).map(|w| (w[1] as f64 - w[0] as f64).abs()).sum::<f64>();
        assert!(step(&fed) > step(&clean));
    }

    #[test]
    fn plays_releases_frees_and_goes_silent() {
        let mut e = sine();
        let mut buf = vec![0i16; 4410];
        e.note_on(60, 100);
        e.render(&mut buf);
        assert_eq!(e.sounding(), 1);
        e.note_off(60);
        for _ in 0..30 {
            e.render(&mut buf);
        }
        assert_eq!(e.sounding(), 0);
        e.render(&mut buf);
        assert_eq!(peak(&buf), 0);
    }

    #[test]
    fn velocity_sensitivity_and_the_envelope_shape() {
        let level = |vel: u8, sens: i32| {
            let mut e = sine();
            e.set(op_param(0, O_VELOCITY), sens);
            e.note_on(60, vel);
            let mut buf = vec![0i16; 4410];
            e.render(&mut buf);
            peak(&buf)
        };
        assert!(level(20, 100) < level(127, 100) / 2);
        assert!((level(1, 50) as f64 / level(127, 50) as f64 - 0.5).abs() < 0.02, "the default: half at velocity 1");
        let flat = (level(20, 0) as f64 - level(127, 0) as f64).abs() / level(127, 0) as f64;
        assert!(flat < 0.05, "no sensitivity: velocity does nothing ({flat})");
        // a slow attack starts quiet; a decay to a low sustain falls
        let mut e = sine();
        e.set(op_param(0, O_ATTACK), 400);
        e.note_on(60, 127);
        let mut buf = vec![0i16; 22_050];
        e.render(&mut buf);
        assert!(peak(&buf[..1000]) < peak(&buf[18_000..]) / 3);
        let mut d = sine();
        d.set(op_param(0, O_SUSTAIN), 10);
        d.set(op_param(0, O_DECAY), 100);
        d.note_on(60, 127);
        let mut b = vec![0i16; 22_050];
        d.render(&mut b);
        assert!(peak(&b[18_000..]) < peak(&b[..2000]) / 3);
    }

    #[test]
    fn nine_notes_steal_params_clamp_and_the_list_is_whole() {
        let mut e = sine();
        for n in 40..52 {
            e.note_on(n, 127);
        }
        assert!(e.sounding() <= VOICES);
        let mut buf = vec![0i16; 2048];
        e.render(&mut buf);
        e.pitch_bend(0);
        e.render(&mut buf);
        e.pitch_bend(16383);
        e.render(&mut buf);
        e.all_off();
        assert!(e.set(P_LEVEL, 9999) && e.get(P_LEVEL) == Some(100));
        assert!(e.set(op_param(3, O_DETUNE), -999) && e.get(op_param(3, O_DETUNE)) == Some(-50));
        assert!(!e.set(P_COUNT as u16, 1) && e.get(7).is_none() && e.get(P_COUNT as u16).is_none());
        let fresh = Fm4::new(44_100.0);
        let ps = fresh.params();
        assert_eq!(ps.len(), 7 + OPS * OP_PARAMS as usize + OPS * EXT_PARAMS as usize + FILT_PARAMS as usize + LFO_PARAMS as usize);
        assert!(ps.iter().all(|p| p.min <= p.default && p.default <= p.max && fresh.get(p.id) == Some(p.default)));
        assert_eq!(ps.iter().find(|p| p.id == op_param(2, O_LEVEL)).map(|p| p.name.as_str()), Some("OP3 LEVEL"));
    }

    /// The amplitude of a sine at `f` Hz in `x` (a single-frequency Fourier sum over whole periods' worth, Hann-weighted).
    fn tone(x: &[i16], f: f64) -> f64 {
        let n = x.len();
        let (mut c, mut s) = (0.0, 0.0);
        for (i, &v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let a = std::f64::consts::TAU * f * i as f64 / 44_100.0;
            c += v as f64 / 32768.0 * w * a.cos();
            s += v as f64 / 32768.0 * w * a.sin();
        }
        c.hypot(s) / (n as f64 / 4.0)
    }

    /// The numbers measured on OPERATOR 0.7 in the emulator (docs/PORTING.md, 2026-11-18): algorithm 1, note 57 at velocity 127: A alone
    /// 0.0484 peak (-26.3 dBFS), a modulator at full level a whole cycle (6.28 radians) of phase, linear in the level's amplitude, amplitude linear in velocity down to half at velocity 0, and a 399 ms release that falls 16 dB every 100 ms.
    fn operator() -> Fm4 {
        let mut e = Fm4::new(44_100.0);
        e.set(P_ALGORITHM, 0);
        e.set(P_LEVEL, 16);
        e.set(P_INDEX, 628);
        for op in 0..OPS {
            e.set(op_param(op, O_ATTACK), 1);
            e.set(op_param(op, O_DECAY), 998);
            e.set(op_param(op, O_SUSTAIN), 99);
            e.set(op_param(op, O_RELEASE), 399);
            e.set(op_param(op, O_VELOCITY), 50);
            e.set(op_param(op, O_LEVEL), 0);
        }
        e.set(op_param(0, O_LEVEL), 99);
        e
    }

    /// OPERATOR 0.7's waveforms, measured in the emulator (2026-11-19, `fm4fit`, A alone at 0 dB, note 57): a saw of N harmonics has them at 1/k and nothing above N,
    /// a square of N has the first N odd ones at 1/k, and every wave peaks at the same level as the sine (0.0484), so the fundamental sits at 0.0335 (Saw 3), 0.0316
    /// (Saw 4), 0.0298 (Saw 6), 0.0518 (Square 3) and 0.0520 (Square 4) of full scale.
    #[test]
    fn fitted_to_operator_the_waveforms() {
        // (wave, [(harmonic, dB under the fundamental, or None for absent)], fundamental amplitude)
        type Case<'a> = (i32, &'a [(usize, Option<f64>)], f64);
        let cases: [Case; 5] = [
            (3, &[(2, Some(-6.1)), (3, Some(-9.7)), (4, None)], 0.0335),
            (4, &[(4, Some(-12.3)), (5, None)], 0.0316),
            (5, &[(5, Some(-14.4)), (6, Some(-16.2)), (7, None)], 0.0298),
            (11, &[(2, None), (3, Some(-9.7)), (5, Some(-14.4)), (7, None)], 0.0518),
            (12, &[(7, Some(-17.7)), (9, None)], 0.0520),
        ];
        for (w, harmonics, fund) in cases {
            let mut e = operator();
            e.set(ext_param(0, E_WAVE), w);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let x = &b[10_000..10_000 + 22_050];
            assert!((peak(&b[10_000..]) as f64 / 32768.0 - 0.0484).abs() < 0.0015, "wave {w} peak");
            let f1 = tone(x, 220.0);
            assert!((f1 - fund).abs() < 0.0015, "wave {w} fundamental {f1} against {fund}");
            for (h, db) in harmonics {
                let rel = 20.0 * (tone(x, 220.0 * *h as f64) / f1).max(1e-9).log10();
                match db {
                    Some(d) => assert!((rel - d).abs() < 0.8, "wave {w} harmonic {h}: {rel:.1} dB, measured {d}"),
                    None => assert!(rel < -50.0, "wave {w} harmonic {h} should be absent, got {rel:.1}"),
                }
            }
        }
        // the ideal waves and the 4-bit sine are in the table and sound; an out-of-range wave is clamped
        let mut e = operator();
        assert!(e.set(ext_param(3, E_WAVE), 99) && e.get(ext_param(3, E_WAVE)) == Some(18) && e.get(P_COUNT as u16).is_none());
        for w in [1, 2, 10, 18] {
            let mut e = operator();
            e.set(ext_param(0, E_WAVE), w);
            e.note_on(57, 127);
            let mut b = vec![0i16; 22_050];
            e.render(&mut b);
            assert!(peak(&b[5_000..]) > 1000, "wave {w} silent");
        }
    }

    /// The numbers measured on OPERATOR 0.7 (2026-11-19, `fm4fit`): Key scaling pivots on note 60 and doubles the level an octave at +100 % (notes 24 to 96 at +100 %
    /// peaked 0.0061 0.0122 0.0243 0.0484 0.0966 0.1929 0.3847; -100 % mirrored), +73 % gave 0.0292 at note 48 and 0.0802 at 72; Fixed plays Freq x Multi whatever
    /// the note (100 Hz for notes 57 and 69); Phase 66 % starts a sine at -0.83 and falling.
    #[test]
    fn fitted_to_operator_key_scaling_fixed_and_phase() {
        let peak_at = |note: u8, key: i32| {
            let mut e = operator();
            e.set(ext_param(0, E_KEYSCALE), key);
            e.note_on(note, 127);
            let mut b = vec![0i16; 22_050];
            e.render(&mut b);
            peak(&b[8_000..]) as f64 / 32768.0
        };
        for (note, want) in [(24, 0.0061), (48, 0.0243), (60, 0.0484), (84, 0.1929), (96, 0.3847)] {
            assert!((peak_at(note, 100) - want).abs() < want * 0.03, "key +100 note {note}");
            assert!((peak_at(120 - note, -100) - want).abs() < want * 0.03, "key -100 note {}", 120 - note);
        }
        assert!((peak_at(72, 73) - 0.0802).abs() < 0.0025 && (peak_at(48, 73) - 0.0292).abs() < 0.001);
        for note in [57, 69] {
            let mut e = operator();
            e.set(ext_param(0, E_FIXED), 1);
            e.note_on(note, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let x = &b[10_000..10_000 + 22_050];
            assert!(tone(x, 100.0) > 0.04 && tone(x, 200.0) < 0.002, "fixed note {note}");
        }
        let mut e = operator();
        e.set(ext_param(0, E_FIXED), 1);
        e.set(ext_param(0, E_FIXMULTI), 4); // x10
        e.set(ext_param(0, E_FIXFREQ), 127);
        e.note_on(57, 127);
        let mut b = vec![0i16; 44_100];
        e.render(&mut b);
        assert!(tone(&b[10_000..10_000 + 22_050], 1270.0) > 0.04);
        let mut e = operator();
        e.set(ext_param(0, E_PHASE), 66);
        e.note_on(57, 127);
        let mut b = vec![0i16; 2_000];
        e.render(&mut b);
        assert!(b[30] < b[10] && b[10] < 0, "phase 66 starts negative and falling: {} {}", b[10], b[30]);
    }

    /// OPERATOR 0.7's filter (2026-11-19, `fm4fit`; operator A on Saw D, note 45 or 57, Freq<Key at its default +100, Res 0.27): the harmonics' levels agree with two
    /// cascaded two-pole stages (one for 12 dB) whose corner is the displayed Freq times `2^((note - 60) / 12)` and whose Q is 0.65 there, 0.5 at Res 0.
    #[test]
    fn fitted_to_operator_the_filter() {
        let rel = |wave_filter: &dyn Fn(&mut Fm4), note: u8, f0: f64, ks: [usize; 3]| {
            let mut e = operator();
            e.set(ext_param(0, E_WAVE), 10);
            e.set(FILT_BASE + F_ON, 1);
            wave_filter(&mut e);
            e.note_on(note, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let x = &b[8_000..8_000 + 22_050];
            let a: Vec<f64> = ks.iter().map(|&k| 20.0 * (tone(x, f0 * k as f64) * k as f64).log10()).collect();
            a // each harmonic's gain against the saw's own 1/k
        };
        // low-pass 24 dB, display 670 Hz, note 45 (measured gain differences, harmonic 2, 3, 4 against 1: -3.5, -9.9, -17.1)
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 670); }, 45, 110.0, [1, 2, 4]);
        assert!(((g[1] - g[0]) + 3.5).abs() < 1.5 && ((g[2] - g[0]) + 17.1).abs() < 2.0, "lp24 {g:?}");
        // low-pass 12 dB (display about 1.22k at note 45): harmonic 2, 4, 8 against 1: -0.4, -4.6, -10.3 measured
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 1220); e.set(FILT_BASE + F_SLOPE, 0); }, 45, 110.0, [1, 2, 8]);
        assert!(((g[1] - g[0]) + 0.4).abs() < 1.5 && ((g[2] - g[0]) + 10.3).abs() < 2.0, "lp12 {g:?}");
        // high-pass 24 dB, display 670, note 57: harmonic 4 and 6 against 2: +10.3 and +11.8 dB
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 670); e.set(FILT_BASE + F_TYPE, 1); }, 57, 220.0, [2, 4, 6]);
        assert!(((g[1] - g[0]) - 10.3).abs() < 2.0 && ((g[2] - g[0]) - 11.8).abs() < 2.0, "hp24 {g:?}");
        // the notch cuts the harmonics near its corner: 2 and 3 are 20 dB and 35 dB under (note 57)
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 670); e.set(FILT_BASE + F_TYPE, 3); }, 57, 220.0, [1, 2, 3]);
        assert!(g[0] - g[1] > 8.0 && g[0] - g[2] > 12.0, "notch {g:?}");
        // off by default: the first column of the saw is untouched
        let mut e = operator();
        e.set(ext_param(0, E_WAVE), 10);
        e.note_on(45, 127);
        let mut b = vec![0i16; 44_100];
        e.render(&mut b);
        let x = &b[8_000..8_000 + 22_050];
        assert!((20.0 * (tone(x, 220.0) / tone(x, 110.0)).log10() + 6.0).abs() < 0.8);
    }

    /// OPERATOR shows its sustain in decibels: with the dB law 99 is 0 dB and each step is 1 dB down.
    #[test]
    fn sustain_in_decibels() {
        let level = |v: i32| {
            let mut e = operator();
            e.set(P_SUSTAIN_LAW, 1);
            e.set(op_param(0, O_SUSTAIN), v);
            e.set(op_param(0, O_DECAY), 5);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            peak(&b[30_000..]) as f64 / 32768.0
        };
        let top = level(99);
        for down in [6, 20, 40] {
            let d = 20.0 * (level(99 - down) / top).log10();
            assert!((d + down as f64).abs() < 0.5, "{down} steps gave {d:.2} dB");
        }
    }

    #[test]
    fn fitted_to_operator_a_single_carrier() {
        let mut e = operator();
        e.note_on(57, 127);
        let mut b = vec![0i16; 44_100];
        e.render(&mut b);
        let pk = peak(&b[10_000..]) as f64 / 32768.0;
        assert!((pk / 0.0484 - 1.0).abs() < 0.05, "A alone peaks at {pk}, OPERATOR at 0.0484");
        let mut q = operator();
        q.note_on(57, 1);
        let mut c = vec![0i16; 44_100];
        q.render(&mut c);
        let half = peak(&c[10_000..]) as f64 / peak(&b[10_000..]) as f64;
        assert!((half - 0.5).abs() < 0.02, "velocity 1 is {half} of velocity 127, OPERATOR 0.504");
    }

    #[test]
    fn fitted_to_operator_the_modulation_index() {
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0);
        let spectrum = |e: &mut Fm4| {
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 32_768];
            let h1 = tone(seg, f0);
            (h1, (2..=8).map(|h| 20.0 * (tone(seg, f0 * h as f64) / h1).log10()).collect::<Vec<f64>>())
        };
        // B (OP2) into A at full level and full sustain: OPERATOR's fundamental is 0.0245 of A's 0.0484, harmonics 2 to 8 at these dB under it
        let mut e = operator();
        e.set(op_param(1, O_LEVEL), 99);
        let (h1, db) = spectrum(&mut e);
        assert!((h1 / 0.0245 - 1.0).abs() < 0.08, "the fundamental is {h1}, OPERATOR 0.0245");
        for (h, want) in [(2, -8.9), (3, 1.4), (4, -2.3), (6, -0.2), (7, -8.7), (8, -9.8)] {
            assert!((db[h - 2] - want).abs() < 1.2, "harmonic {h} at {:.1} dB, OPERATOR {want}", db[h - 2]);
        }
        // a small index, from the level scan: B at -54 dB with its sustain at -24 dB (levels 27 and 6) had harmonic 2 at -67.7 dB
        let mut q = operator();
        q.set(op_param(1, O_LEVEL), 27);
        q.set(op_param(1, O_SUSTAIN), 6);
        let (_, small) = spectrum(&mut q);
        assert!((small[0] + 67.7).abs() < 2.5, "harmonic 2 at {:.1} dB, OPERATOR -67.7", small[0]);
    }

    /// OPERATOR's self-feedback (2026-11-18, operators B, C and D; A showed none): alone, at 13 %, 61 % and 100 %, its fundamental was 0.977, 0.64 and 0.452 of the
    /// feedback-free level, with harmonics 2 and 3 at -14.2/-25.0, -6.9/-11.0 and -6.5/-10.4 dB: the mean of the last two outputs times 3.1 radians at 100 %.
    #[test]
    fn fitted_to_operator_the_self_feedback() {
        // OPERATOR sounds 146.8 cents flat here (a 48 kHz engine at 44.1 kHz, docs/PORTING.md): bend this engine the same, so a feedback loop of one or two samples spans as many
        // samples of the period as it does there
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0) * 44_100.0 / 48_000.0;
        let run = |pct: i32| {
            let mut e = operator();
            e.set(P_ALGORITHM, 10);
            e.set(op_param(0, O_LEVEL), 0);
            e.set(op_param(1, O_LEVEL), 99);
            e.set(op_param(1, O_FEEDBACK), pct);
            e.set(P_FEEDBACK, 310);
            e.pitch_bend(2180);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 32_768];
            let h1 = tone(seg, f0);
            (h1, 20.0 * (tone(seg, f0 * 2.0) / h1).log10(), 20.0 * (tone(seg, f0 * 3.0) / h1).log10())
        };
        let clean = run(0).0;
        for (pct, rel, h2, h3) in [(13, 0.977, -14.2, -25.0), (61, 0.64, -6.9, -11.0), (100, 0.452, -6.5, -10.4)] {
            let (h1, a, b) = run(pct);
            assert!((h1 / clean / rel - 1.0).abs() < 0.06, "{pct} %: the fundamental is {:.3} of clean, OPERATOR {rel}", h1 / clean);
            // at 100 % harmonic 2 is 2 dB higher here than there (-4.5 against -6.5; harmonic 3 and the fundamental agree): the top of OPERATOR's knob is not quite this law
            let slack = if pct == 100 { 2.3 } else { 1.0 };
            assert!((a - h2).abs() < slack && (b - h3).abs() < 1.5, "{pct} %: harmonics 2 and 3 at {a:.1} and {b:.1}, OPERATOR {h2} and {h3}");
        }
    }

    #[test]
    fn fitted_to_operator_the_release_slope() {
        let mut e = operator();
        e.note_on(57, 127);
        let mut b = vec![0i16; 22_050];
        e.render(&mut b);
        e.note_off(57);
        let rms = |x: &[i16]| 20.0 * ((x.iter().map(|&v| (v as f64 / 32768.0).powi(2)).sum::<f64>() / x.len() as f64).sqrt()).log10();
        let mut tail = vec![0i16; 4410 * 4];
        e.render(&mut tail);
        let (a, c) = (rms(&tail[..4410]), rms(&tail[4410..8820]));
        assert!((a - c - 16.0).abs() < 2.0, "{a:.1} then {c:.1}: OPERATOR falls 16 dB in 100 ms");
    }

    /// OPERATOR's algorithms 2 to 11 (not 1: see the docs), each with all four operators at 0 dB and full sustain, note 57, as measured in the emulator: the fundamental's
    /// amplitude and harmonics 2 to 4 in dB under it. Algorithms 3 and 4 (and 7 and 9) are the same shape with the letters swapped, so they sound alike when
    /// the operators are equal, and the test cannot tell them apart; their diagrams do.
    #[test]
    fn fitted_to_operator_the_algorithms() {
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0) * 44_100.0 / 48_000.0;
        let table: [(usize, f64, [f64; 3]); 9] = [
            (2, 0.0185, [-24.2, -4.0, -16.3]),
            (3, 0.0172, [-9.0, 1.6, -1.5]),
            (5, 0.0214, [-15.6, -3.0, -4.4]),
            (6, 0.0130, [-19.0, 1.2, -11.5]),
            (7, 0.0490, [-8.9, 1.4, -2.3]),
            (8, 0.0733, [-26.3, -13.4, -13.9]),
            (9, 0.0735, [-8.9, 1.4, -2.3]),
            (10, 0.1212, [-22.8, -12.5, -16.1]),
            (11, 0.1933, [-120.0, -120.0, -120.0]),
        ];
        for (alg, h1_want, db_want) in table {
            let mut e = operator();
            e.set(P_ALGORITHM, alg as i32 - 1);
            for op in 0..OPS {
                e.set(op_param(op, O_LEVEL), 99);
            }
            e.pitch_bend(2180);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 32_768];
            let h1 = tone(seg, f0);
            assert!((h1 / h1_want - 1.0).abs() < 0.08, "algorithm {alg}: fundamental {h1:.4}, OPERATOR {h1_want}");
            for (k, want) in db_want.iter().enumerate() {
                let got = 20.0 * (tone(seg, f0 * (k + 2) as f64) / h1).log10();
                if *want > -60.0 {
                    let tol = if *want < -20.0 { 4.5 } else { 2.5 }; // a deep harmonic moves more with a small error in a modulator
                    assert!((got - want).abs() < tol, "algorithm {alg}: harmonic {} at {got:.1} dB, OPERATOR {want}", k + 2);
                } else {
                    assert!(got < -60.0, "algorithm {alg}: harmonic {} at {got:.1} dB, OPERATOR has none", k + 2);
                }
            }
        }
    }

    /// Mighty Arp 1.5 (2026-11-18, measured in the emulator through its arpeggiator with chance 97 %, one octave, gate 98 %, the effects at 0 and the other operators
    /// silent): its algorithms 1 to 4, 5, 6, 7 and 8 are OPERATOR's 1 to 4, 7, 9, 10 and 11; the level is linear in amplitude (100 % gave 0.0708 of full scale, 50 % half); and
    /// a modulator's index is about 5.4 radians at 100 % (OPERATOR's was 6.28). Algorithm 5 (OP2 into OP1, OP4 into OP3), OP1 at 100 %, OP2 at 5, 10, 15, 25 %: harmonics 2 and 3 under the fundamental.
    #[test]
    fn fitted_to_mighty_arp_the_linear_level_and_the_index() {
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0);
        for (l2, h2, h3) in [(5, -15.9, -40.6), (10, -9.4, -28.4), (15, -5.1, -20.5), (25, 2.1, -8.7)] {
            let mut e = operator();
            e.set(P_LEVEL_LAW, 1);
            e.set(P_LEVEL, 24);
            e.set(P_INDEX, 540); // about 1.75 pi: a little under OPERATOR's whole cycle, and a little compressive towards 100 % (best fit 520 to 620)
            e.set(P_ALGORITHM, 6); // OPERATOR's 7 is Mighty Arp's 5
            for op in 0..OPS {
                e.set(op_param(op, O_LEVEL), 0);
                e.set(op_param(op, O_VELOCITY), 0);
            }
            e.set(op_param(0, O_LEVEL), 100);
            e.set(op_param(1, O_LEVEL), l2);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 8192];
            let h1 = tone(seg, f0);
            let db = |h: f64| 20.0 * (tone(seg, f0 * h) / h1).log10();
            assert!((db(2.0) - h2).abs() < 1.8 && (db(3.0) - h3).abs() < 3.5, "OP2 at {l2} %: harmonics 2 and 3 at {:.1} and {:.1}, Mighty Arp {h2} and {h3}", db(2.0), db(3.0));
        }
        let carrier = |level: i32| {
            let mut c = operator();
            c.set(P_LEVEL_LAW, 1);
            c.set(P_LEVEL, 24);
            c.set(P_ALGORITHM, 10);
            for op in 0..OPS {
                c.set(op_param(op, O_LEVEL), 0);
                c.set(op_param(op, O_VELOCITY), 0);
            }
            c.set(op_param(0, O_LEVEL), level);
            c.note_on(57, 127);
            let mut b = vec![0i16; 22_050];
            c.render(&mut b);
            peak(&b[10_000..]) as f64 / 32768.0
        };
        let full = carrier(100);
        assert!((full / 0.0708 - 1.0).abs() < 0.06, "one carrier at 100 % peaks at {full}, Mighty Arp 0.0708");
        assert!((carrier(50) / full - 0.5).abs() < 0.03, "50 % is half");
    }

    /// OPERATOR 0.7's ratio (measured by the zero crossings of operator A alone, note 57): the coarse knob steps 0.5, 1, 2, 3, 4, 5, 6, 7 ... to 48 (a click each), and the
    /// fine value adds thousandths (coarse 1 and fine 26 played 1.026 times the pitch, fine 146 played 1.146; the screen read "1.026" and "1.146").
    #[test]
    fn fitted_to_operator_the_ratio_table() {
        let freq = |coarse: i32, fine: i32| {
            let mut e = sine();
            e.set(P_RATIO_LAW, 1);
            e.set(op_param(0, O_RATIO), coarse);
            e.set(op_param(0, O_FINE), fine);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let z = &b[8000..];
            z.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count() as f64 * 44_100.0 / z.len() as f64 / 220.0
        };
        for (coarse, ratio) in [(0, 0.5), (1, 1.0), (2, 2.0), (3, 3.0), (7, 7.0), (20, 20.0)] {
            assert!((freq(coarse, 0) / ratio - 1.0).abs() < 0.02, "coarse {coarse}: {:.3}", freq(coarse, 0));
        }
        assert!((freq(1, 26) - 1.026).abs() < 0.006 && (freq(1, 146) - 1.146).abs() < 0.006 && (freq(18, 248) - 18.248).abs() < 0.12, "fine adds thousandths");
        // the generic law is still a percentage of the ratio
        let mut g = sine();
        g.set(op_param(0, O_RATIO), 2);
        g.set(op_param(0, O_FINE), 50);
        g.note_on(57, 127);
        let mut b = vec![0i16; 44_100];
        g.render(&mut b);
        let z = &b[8000..];
        let f = z.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count() as f64 * 44_100.0 / z.len() as f64 / 220.0;
        assert!((f - 3.0).abs() < 0.04, "coarse 2 fine 50 % is 3x: {f}");
    }

    #[test]
    fn glide_slides_from_the_last_note_to_the_next() {
        let play = |ms: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, 7);
            e.glide(ms);
            let mut buf = vec![0i16; 4410];
            e.note_on(57, 100);
            e.render(&mut buf);
            e.note_off(57);
            e.render(&mut buf);
            e.note_on(69, 100);
            let mut out = vec![0i16; 22_050];
            e.render(&mut out);
            out
        };
        let none = play(0);
        let g = play(300);
        let f = |x: &[i16], lo: usize, hi: usize| {
            let seg = &x[lo..hi];
            let z = seg.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
            z as f64 / 2.0 * 44_100.0 / seg.len() as f64
        };
        let (n_hz, early, late) = (f(&none, 2048, 8192), f(&g, 1024, 3072), f(&g, 14_000, 22_050));
        assert!((n_hz / early) > 1.2, "glide starts below the target: {n_hz} vs {early}");
        assert!((late / n_hz - 1.0).abs() < 0.03, "and arrives: {late} vs {n_hz}");
        // a huge value is clamped, not trusted
        let mut e = Fm4::new(44_100.0);
        e.glide(i32::MAX);
        e.note_on(81, 100);
        e.note_off(81);
        e.note_on(45, 100);
        let mut out = vec![0i16; 2048];
        e.render(&mut out);
        assert!(out.iter().any(|&v| v != 0));
    }

    /// OPERATOR 0.7's LFO (2026-11-20, `fm4fit` and its LFO page): default Sine, 6.32 Hz, Amount 10 %, on pitch; 10 % swung the pitch +-120 cents and 69 % +-827
    /// (12 cents a percent, an octave at 100 %); a square starts high and switches at half a cycle, a saw up starts low, a saw down starts high.
    #[test]
    fn fitted_to_operator_the_lfo() {
        // cents of each cycle's frequency, from the zero crossings of a clean sine on OP1
        let cents = |amount: i32, ty: i32, hz100: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_TYPE, ty);
            e.set(LFO_BASE + L_RATE, hz100);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let mut ups = vec![];
            for i in 1..x.len() {
                if x[i - 1] < 0 && x[i] >= 0 {
                    ups.push(i as f64 - 1.0 + f64::from(-x[i - 1]) / f64::from(x[i] - x[i - 1]));
                }
            }
            ups.windows(2).map(|w| (1200.0 * (44_100.0 / (w[1] - w[0]) / 220.0).log2(), (w[0] + w[1]) / 2.0 / 44_100.0)).collect::<Vec<_>>()
        };
        let swing = |c: &[(f64, f64)]| (c.iter().map(|v| v.0).fold(f64::MIN, f64::max), c.iter().map(|v| v.0).fold(f64::MAX, f64::min));
        let c = cents(10, 0, 632);
        let (hi, lo) = swing(&c);
        assert!((hi - 120.0).abs() < 10.0 && (lo + 120.0).abs() < 10.0, "10 percent: {hi} {lo}");
        let c = cents(69, 0, 632);
        let (hi, lo) = swing(&c);
        assert!((hi - 828.0).abs() < 60.0 && (lo + 828.0).abs() < 60.0, "69 percent: {hi} {lo}");
        // the rate: ten cycles take a second and a half at 6.32 Hz
        let c = cents(10, 0, 632);
        let mean = c.iter().map(|v| v.0).sum::<f64>() / c.len() as f64;
        let ups: Vec<f64> = (1..c.len()).filter(|&k| c[k - 1].0 - mean < 0.0 && c[k].0 - mean >= 0.0).map(|k| c[k].1).collect();
        let hz = (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0]);
        assert!((hz - 6.32).abs() < 0.3, "rate {hz}");
        // the shapes' first half cycle: square high, saw up low (rising), saw down high
        let first = |ty: i32| cents(10, ty, 632).iter().filter(|v| v.1 < 0.05).map(|v| v.0).sum::<f64>();
        assert!(first(1) > 100.0 && first(3) < -50.0 && first(4) > 50.0, "{} {} {}", first(1), first(3), first(4));
        // off by default: the pitch stays put
        let mut e = Fm4::new(44_100.0);
        e.note_on(57, 127);
        let mut x = vec![0i16; 8_000];
        e.render(&mut x);
        assert!(x.iter().any(|&v| v != 0));
        // random types stay bounded and the free-running one renders
        for (ty, retrig) in [(5, 1), (6, 1), (6, 0), (0, 0)] {
            let mut e = Fm4::new(44_100.0);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_TYPE, ty);
            e.set(LFO_BASE + L_RETRIG, retrig);
            e.set(LFO_BASE + L_AMOUNT, 100);
            e.note_on(60, 100);
            e.note_on(64, 100);
            let mut x = vec![0i16; 20_000];
            e.render(&mut x);
            assert!(x.iter().any(|&v| v != 0));
        }
    }

    /// OPERATOR 0.7's LFO on the filter (2026-11-20; operator A as Saw D, a 24 dB low-pass at 846 Hz, Freq<Key +100, Res 0.27, only the filter a target): the fitted cutoff
    /// swung 537 to 800 Hz about 655 (the displayed corner 712 Hz at the note, times the engine factor) at Amount 10 %, so **plus and minus 0.285 octave, 2.85 octaves at 100 %**,
    /// a sine geometrically about its own value; Amount A at 34 % gave 0.35 of that swing.
    #[test]
    fn fitted_to_operator_the_lfo_on_the_filter() {
        let swing = |amount: i32, amount_a: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_WAVE), 10); // Saw D
            e.set(FILT_BASE + F_ON, 1);
            e.set(FILT_BASE + F_FREQ, 846);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_AMOUNT_A, amount_a);
            e.set(LFO_BASE + L_DEST, L_FILTER_BIT); // the filter only
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100 + 3000];
            e.render(&mut x);
            let x: Vec<f64> = x[3000..].iter().map(|&v| f64::from(v) / 32768.0).collect();
            let f0 = 220.0;
            let q = 0.5 / (1.0 - 0.97 * 0.27);
            let h = |f: f64, fc: f64| {
                let xx = f / fc;
                1.0 / ((1.0 - xx * xx).powi(2) + (xx / q).powi(2))
            };
            let mut fcs = vec![];
            for a in (0..x.len() - 1764).step_by(882) {
                let db: Vec<f64> = (1..=10)
                    .map(|k| {
                        let w = std::f64::consts::TAU * k as f64 * f0 / 44_100.0;
                        let (mut c, mut s) = (0.0, 0.0);
                        for i in 0..1764 {
                            let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 1764.0).cos();
                            c += x[a + i] * win * (w * i as f64).cos();
                            s += x[a + i] * win * (w * i as f64).sin();
                        }
                        20.0 * (c.hypot(s) / 441.0 * k as f64).max(1e-9).log10()
                    })
                    .collect();
                let mut best = (f64::MAX, 0.0);
                for g in 0..200 {
                    let fc = 200.0 * 2f64.powf(g as f64 / 40.0);
                    let m: Vec<f64> = (1..=10).map(|k| 20.0 * h(k as f64 * f0, fc).log10()).collect();
                    let off = db.iter().zip(&m).map(|(d, m)| d - m).sum::<f64>() / 10.0;
                    let e: f64 = db.iter().zip(&m).map(|(d, m)| (d - m - off).powi(2)).sum();
                    if e < best.0 {
                        best = (e, fc);
                    }
                }
                fcs.push(best.1);
            }
            let (lo, hi) = (fcs.iter().cloned().fold(f64::MAX, f64::min), fcs.iter().cloned().fold(0.0, f64::max));
            ((hi / lo).log2(), (hi * lo).sqrt())
        };
        let (oct, centre) = swing(10, 100);
        assert!((oct - 0.57).abs() < 0.15, "10 percent swung {oct} octaves");
        assert!((centre / 712.0 - 1.0).abs() < 0.12, "about {centre} Hz");
        let (oct34, _) = swing(10, 34);
        assert!((oct34 / oct - 0.35).abs() < 0.15, "Amount A at 34 percent: {oct34} against {oct}");
        let (oct0, _) = swing(0, 100);
        assert!(oct0 < 0.1, "no amount, no swing: {oct0}");
    }

    /// OPERATOR 0.7's LFO Dest B on Osc Vol A (2026-11-20, one sine carrier, LFO amount 10, 69 and 100 %, Amount B +100 %): the peak went 0.0436 to 0.0533, 0.0156 to
    /// 0.0818 and 0 to 0.0968 of a clean 0.0484, a gain of 1 plus amount times the sine; Volume gives the same for a lone carrier. Dest B on Osc Vol A leaves the other operators alone.
    #[test]
    fn fitted_to_operator_lfo_dest_b_on_osc_volume() {
        let range = |target: i32, amount: i32, amount_b: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_DEST, 0); // no pitch targets
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_DEST_B, target);
            e.set(LFO_BASE + L_AMOUNT_B, amount_b);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let peaks: Vec<f64> = x[4410..].chunks(441).map(|c| f64::from(peak(c)) / 32768.0).collect();
            (peaks.iter().cloned().fold(f64::MAX, f64::min), peaks.iter().cloned().fold(0.0, f64::max))
        };
        let (clean_lo, clean_hi) = range(0, 100, 100);
        assert!((clean_hi / clean_lo - 1.0).abs() < 0.03, "target Off leaves the level alone");
        let one = clean_hi;
        for (amount, lo, hi) in [(10, 0.9, 1.1), (69, 0.31, 1.69), (100, 0.0, 2.0)] {
            let (a, b) = range(1, amount, 100);
            assert!((a / one - lo).abs() < 0.06 && (b / one - hi).abs() < 0.06, "amount {amount}: {} to {}", a / one, b / one);
        }
        // Volume on a lone carrier is the same; another operator's Osc Vol does nothing to it; Amount B scales
        let (a, b) = range(20, 69, 100);
        assert!((a / one - 0.31).abs() < 0.06 && (b / one - 1.69).abs() < 0.06);
        let (a, b) = range(3, 100, 100);
        assert!((a / b - 1.0).abs() < 0.03, "Osc Vol C leaves operator A alone");
        let (a, b) = range(1, 100, 50);
        assert!((a / one - 0.5).abs() < 0.06 && (b / one - 1.5).abs() < 0.06, "Amount B at 50 percent");
    }
}
