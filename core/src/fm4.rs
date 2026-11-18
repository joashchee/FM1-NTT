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
pub const P_COUNT: usize = OP_BASE as usize + OPS * OP_PARAMS as usize;

/// Parameter id of operator `op` (0 to 3), field `field` (`O_*`).
pub const fn op_param(op: usize, field: u16) -> u16 {
    OP_BASE + op as u16 * OP_PARAMS + field
}

/// name, min, max, default, unit
const GLOBAL: [(&str, i32, i32, i32, &str); 6] = [("ALGORITHM (1-11)", 0, 10, 0, ""), ("FEEDBACK AT 100% (0.01 RAD)", 0, 1000, 310, ""), ("LEVEL", 0, 100, 70, "%"), ("MOD INDEX (0.01 RAD)", 1, 2000, 400, ""), ("LEVEL LAW (0 DB STEPS 1 LINEAR)", 0, 1, 0, ""), ("RATIO LAW (0 PERCENT 1 THOUSANDTHS)", 0, 1, 0, "")];
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
    vel: f64,
    ops: [Op; OPS],
    fb: [[f64; 2]; OPS],
}

const IDLE_OP: Op = Op { stage: Stage::Idle, ph: 0.0, env: 0.0, out: 0.0 };
const IDLE: Voice = Voice { active: false, note: 0, held: false, age: 0, f0: 0.0, vel: 0.0, ops: [IDLE_OP; OPS], fb: [[0.0; 2]; OPS] };

pub struct Fm4 {
    rate: f64,
    p: [i32; P_COUNT],
    voices: [Voice; VOICES],
    clock: u64,
    bend: f64,
    bend_raw: i32,
    bend_semis: f64,
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
        Fm4 { rate, p, voices: [IDLE; VOICES], clock: 0, bend: 1.0, bend_raw: 8192, bend_semis: 2.0 }
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
        v.f0 = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
        v.vel = velocity.min(127) as f64 / 127.0;
        for o in &mut v.ops {
            o.stage = Stage::Attack;
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
        let mut level = [0.0; OPS];
        let mut vsens = [0.0; OPS];
        let mut kscale = [0.0; OPS];
        // decay and release times are the time to fall 64 dB (fitted to OPERATOR 0.7: 399 ms fell 64 dB, 16 dB a 100 ms); the attack aims past 1 and stops at 1
        let fall = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / (ENV_DB / 8.685_889_6))).exp();
        let step = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / 4.6)).exp();
        for op in 0..OPS {
            ratio[op] = self.ratio(op);
            a[op] = [step(self.op(op, O_ATTACK).max(1)), fall(self.op(op, O_DECAY)), fall(self.op(op, O_RELEASE))];
            sustain[op] = self.op(op, O_SUSTAIN) as f64 / 99.0;
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
        let index = self.p[P_INDEX as usize] as f64 / 100.0; // the phase swing (radians) of a modulator at full level
        for o in out.iter_mut() {
            let mut acc = 0.0;
            for v in self.voices.iter_mut().filter(|v| v.active) {
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
                    for k in 0..OPS {
                        if alg.modulators[op] >> k & 1 == 1 {
                            pm += index * v.ops[k].out;
                        }
                    }
                    let s = &mut v.ops[op];
                    pm += fb[op] * 0.5 * (v.fb[op][0] + v.fb[op][1]);
                    let inc = v.f0 * bend * ratio[op] / rate;
                    s.ph += inc;
                    s.ph -= s.ph.floor();
                    // velocity: linear, from (1 - sensitivity) at velocity 0 to 1 at 127 (OPERATOR 0.7 at its default of 50%: half at 0, measured); key scaling: higher notes quieter
                    let vel = 1.0 - vsens[op] * (1.0 - v.vel);
                    let key = 1.0 - kscale[op] * ((v.note as f64 - 36.0) / 91.0).clamp(0.0, 1.0) * 0.75;
                    let amp = level[op] * s.env * vel.max(0.05) * key;
                    s.out = (TAU * (s.ph) + pm).sin() * amp;
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
                acc += heard;
            }
            *o = (acc * poly * master * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
        }
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
        assert!(!e.set(P_COUNT as u16, 1) && e.get(6).is_none() && e.get(P_COUNT as u16).is_none());
        let fresh = Fm4::new(44_100.0);
        let ps = fresh.params();
        assert_eq!(ps.len(), 6 + OPS * OP_PARAMS as usize);
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
}
