//! `fm4`: a 4-operator FM engine, written fresh for FM1-NTT (docs/PORTING.md; the engine the OPERATOR / Mighty Arp family needs).
//!
//! Four sine operators, eight algorithms (how they modulate each other and which are heard), feedback on operator 4, a ratio, a fine detune, a level, a
//! velocity sensitivity and an attack / decay / sustain / release envelope for each. Eight voices, oldest stolen. Floating point, deterministic on one
//! platform, no allocation after `new`. **Fitted to no firmware** (OPERATOR and Mighty Arp boot in the emulator but their sound has not been compared).

use crate::engine::{Engine, ParamDesc};
use std::f64::consts::TAU;

pub const VOICES: usize = 8;
pub const OPS: usize = 4;

pub const P_ALGORITHM: u16 = 0;
pub const P_FEEDBACK: u16 = 1;
pub const P_LEVEL: u16 = 2;
/// Operator `n` (0 to 3, shown as OP1 to OP4) has `OP_PARAMS` parameters from `OP_BASE + n * OP_PARAMS`.
pub const OP_BASE: u16 = 10;
pub const OP_PARAMS: u16 = 10;
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
pub const P_COUNT: usize = OP_BASE as usize + OPS * OP_PARAMS as usize;

/// Parameter id of operator `op` (0 to 3), field `field` (`O_*`).
pub const fn op_param(op: usize, field: u16) -> u16 {
    OP_BASE + op as u16 * OP_PARAMS + field
}

/// name, min, max, default, unit
const GLOBAL: [(&str, i32, i32, i32, &str); 3] = [("ALGORITHM (1-8)", 0, 7, 0, ""), ("FEEDBACK (OP4)", 0, 7, 0, ""), ("LEVEL", 0, 100, 70, "%")];
const PER_OP: [(&str, i32, i32, i32, &str); OP_PARAMS as usize] = [
    ("RATIO (0 = 0.5)", 0, 31, 1, ""),
    ("FINE RATIO", 0, 99, 0, "%"),
    ("DETUNE", -50, 50, 0, "cents"),
    ("LEVEL", 0, 99, 99, ""),
    ("ATTACK", 0, 5000, 2, "ms"),
    ("DECAY", 1, 5000, 400, "ms"),
    ("SUSTAIN", 0, 99, 60, ""),
    ("RELEASE", 1, 5000, 250, "ms"),
    ("VELOCITY SENS", 0, 7, 2, ""),
    ("KEY SCALING", 0, 7, 0, ""),
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

const ALGORITHMS: [Algorithm; 8] = [
    Algorithm { modulators: [m(&[1]), m(&[2]), m(&[3]), 0], carriers: m(&[0]) },       // 4 > 3 > 2 > 1
    Algorithm { modulators: [m(&[1]), m(&[2, 3]), 0, 0], carriers: m(&[0]) },          // (4 + 3) > 2 > 1
    Algorithm { modulators: [m(&[1, 2]), 0, m(&[3]), 0], carriers: m(&[0]) },          // (4 > 3) + 2 > 1
    Algorithm { modulators: [m(&[1]), 0, m(&[3]), 0], carriers: m(&[0, 2]) },          // 4 > 3 and 2 > 1, both pairs heard
    Algorithm { modulators: [m(&[3]), m(&[3]), m(&[3]), 0], carriers: m(&[0, 1, 2]) }, // 4 > 1, 2, 3
    Algorithm { modulators: [0, m(&[2]), m(&[3]), 0], carriers: m(&[0, 1]) },          // 4 > 3 > 2, and 1 alone
    Algorithm { modulators: [0, 0, m(&[3]), 0], carriers: m(&[0, 1, 2]) },             // 4 > 3, with 2 and 1 alone
    Algorithm { modulators: [0, 0, 0, 0], carriers: m(&[0, 1, 2, 3]) },                // all four alone (additive)
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
    fb: [f64; 2],
}

const IDLE_OP: Op = Op { stage: Stage::Idle, ph: 0.0, env: 0.0, out: 0.0 };
const IDLE: Voice = Voice { active: false, note: 0, held: false, age: 0, f0: 0.0, vel: 0.0, ops: [IDLE_OP; OPS], fb: [0.0; 2] };

pub struct Fm4 {
    rate: f64,
    p: [i32; P_COUNT],
    voices: [Voice; VOICES],
    clock: u64,
    bend: f64,
}

/// The amplitude of a level 0 to 99: 0.75 dB a step down from 99 (a DX-like law), and silence at 0.
fn amp_of(level: i32) -> f64 {
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
        Fm4 { rate, p, voices: [IDLE; VOICES], clock: 0, bend: 1.0 }
    }

    fn op(&self, op: usize, field: u16) -> i32 {
        self.p[op_param(op, field) as usize]
    }

    fn ratio(&self, op: usize) -> f64 {
        let c = self.op(op, O_RATIO);
        let base = if c == 0 { 0.5 } else { c as f64 };
        base * (1.0 + self.op(op, O_FINE) as f64 / 100.0) * 2f64.powf(self.op(op, O_DETUNE) as f64 / 1200.0)
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

    fn pitch_bend(&mut self, value: i32) {
        self.bend = 2f64.powf((value.clamp(0, 16383) - 8192) as f64 / 8192.0 * 2.0 / 12.0);
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
        let step = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / 4.6)).exp();
        for op in 0..OPS {
            ratio[op] = self.ratio(op);
            a[op] = [step(self.op(op, O_ATTACK).max(1)), step(self.op(op, O_DECAY)), step(self.op(op, O_RELEASE))];
            sustain[op] = self.op(op, O_SUSTAIN) as f64 / 99.0;
            level[op] = amp_of(self.op(op, O_LEVEL));
            vsens[op] = self.op(op, O_VELOCITY) as f64 / 7.0;
            kscale[op] = self.op(op, O_KEYSCALE) as f64 / 7.0;
        }
        let alg = &ALGORITHMS[self.p[P_ALGORITHM as usize].clamp(0, 7) as usize];
        // feedback: the operator-4 output fed back into its phase, 0 to about 3 radians
        let fb = self.p[P_FEEDBACK as usize] as f64 / 7.0 * 3.0;
        let master = self.p[P_LEVEL as usize] as f64 / 100.0 * 0.3;
        let live = self.voices.iter().filter(|v| v.active).count().max(1);
        let poly = if live > 4 { (4.0 / live as f64).sqrt() } else { 1.0 };
        let bend = self.bend;
        const INDEX: f64 = 4.0; // the phase swing (radians) of a modulator at full level
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
                            pm += INDEX * v.ops[k].out;
                        }
                    }
                    let s = &mut v.ops[op];
                    if op == OPS - 1 {
                        pm += fb * 0.5 * (v.fb[0] + v.fb[1]);
                    }
                    let inc = v.f0 * bend * ratio[op] / rate;
                    s.ph += inc;
                    s.ph -= s.ph.floor();
                    // velocity: a sensitivity of 7 takes a soft note down to a fifth; key scaling: higher notes quieter by up to 12 dB an octave-set
                    let vel = 1.0 - vsens[op] * (1.0 - v.vel * v.vel);
                    let key = 1.0 - kscale[op] * ((v.note as f64 - 36.0) / 91.0).clamp(0.0, 1.0) * 0.75;
                    let amp = level[op] * s.env * vel.max(0.05) * key;
                    s.out = (TAU * (s.ph) + pm).sin() * amp;
                    if op == OPS - 1 {
                        v.fb[1] = v.fb[0];
                        v.fb[0] = s.out;
                    }
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
        e.set(P_ALGORITHM, 7);
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
        for alg in 0..8 {
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
        for i in 0..8 {
            for j in i + 1..8 {
                assert_ne!(shapes[i], shapes[j], "algorithms {} and {} sound the same", i + 1, j + 1);
            }
        }
    }

    #[test]
    fn feedback_changes_the_tone_and_stays_bounded() {
        let run = |fb: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, 7);
            e.set(op_param(3, O_LEVEL), 99);
            e.set(op_param(0, O_LEVEL), 0);
            e.set(P_FEEDBACK, fb);
            e.note_on(48, 127);
            let mut buf = vec![0i16; 8820];
            e.render(&mut buf);
            buf
        };
        let (clean, fed) = (run(0), run(7));
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
        assert!(level(20, 7) < level(127, 7) / 2);
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
        assert!(!e.set(P_COUNT as u16, 1) && e.get(5).is_none() && e.get(P_COUNT as u16).is_none());
        let fresh = Fm4::new(44_100.0);
        let ps = fresh.params();
        assert_eq!(ps.len(), 3 + OPS * OP_PARAMS as usize);
        assert!(ps.iter().all(|p| p.min <= p.default && p.default <= p.max && fresh.get(p.id) == Some(p.default)));
        assert_eq!(ps.iter().find(|p| p.id == op_param(2, O_LEVEL)).map(|p| p.name.as_str()), Some("OP3 LEVEL"));
    }
}
