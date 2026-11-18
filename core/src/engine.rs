//! The engine interface: what an instrument package is played by (docs/PORTING.md).
//!
//! An engine describes its parameters (`ParamDesc`), so an editor, a MIDI map and a modulation matrix are generic over every engine, and a ported
//! firmware gets an editor for free. `set` clamps, never fails; `render` allocates nothing in the engines here after `new`.

use crate::{
    fm6,
    sampler::{Kit, Sample, Sampler},
    Error, Result,
};

/// One parameter of an engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamDesc {
    pub id: u16,
    pub name: String,
    pub min: i32,
    pub max: i32,
    pub default: i32,
    /// "", "ms", "%", "Hz", "cents", "pot"
    pub unit: &'static str,
}

fn pd(id: u16, name: &str, min: i32, max: i32, default: i32, unit: &'static str) -> ParamDesc {
    ParamDesc { id, name: name.to_string(), min, max, default, unit }
}

pub trait Engine {
    /// The name a package uses: "fm6", "sampler", "kit", "analog".
    fn kind(&self) -> &'static str;
    fn params(&self) -> Vec<ParamDesc>;
    fn get(&self, id: u16) -> Option<i32>;
    /// Sets a parameter, clamped to its range; false when the id is unknown.
    fn set(&mut self, id: u16, value: i32) -> bool;
    fn note_on(&mut self, note: u8, velocity: u8);
    fn note_off(&mut self, note: u8);
    fn all_off(&mut self);
    /// 0 to 16383, centre 8192.
    fn pitch_bend(&mut self, _value: i32) {}
    /// Replaces `out` with mono samples.
    fn render(&mut self, out: &mut [i16]);
    fn sounding(&self) -> usize;
    /// Loads voice `i` of a bank (engines with banks); false when there is none.
    fn load_voice(&mut self, _packed: &[u8; 128]) -> bool {
        false
    }
    /// The voice now loaded, packed (engines with banks), so an edited package can be written back.
    fn current_voice(&self) -> Option<[u8; 128]> {
        None
    }
}

// ---------------------------------------------------------------------------------------------------------------
// fm6

/// The id of the first model parameter of `fm6`: the master pot (0 to 1023), the output model, the tuning model and DTUN.
pub const FM6_POT: u16 = 200;
pub const FM6_OUTPUT: u16 = 201;
pub const FM6_TUNING: u16 = 202;
pub const FM6_DTUN: u16 = 203;

/// The range of byte `i` (0 to 154) of an unpacked DX7 voice.
pub fn fm6_range(i: usize) -> (i32, i32) {
    match i {
        0..=125 => (
            0,
            match i % 21 {
                0..=7 | 8..=10 | 16 | 19 => 99,
                11 | 12 => 3,
                13 | 15 => 7,
                14 => 3,
                17 => 1,
                18 => 31,
                20 => 14,
                _ => 99,
            },
        ),
        126..=133 => (0, 99),
        134 => (0, 31),
        135 => (0, 7),
        136 => (0, 1),
        137..=140 => (0, 99),
        141 => (0, 1),
        142 => (0, 5),
        143 => (0, 7),
        144 => (0, 48),
        145..=154 => (32, 126),
        _ => (0, 0),
    }
}

pub struct Fm6Engine {
    synth: fm6::Synth,
    patch: [u8; 156],
    pot: i32,
    output: i32,
    tuning: i32,
    dtun: i32,
}

impl Fm6Engine {
    pub fn new(sample_rate: f64, voice: &[u8; 128]) -> Result<Fm6Engine> {
        let synth = fm6::Synth::new(sample_rate, voice).map_err(|e| Error::BadImage(format!("fm6: {e}")))?;
        let mut e = Fm6Engine { synth, patch: fm6::unpack_patch(voice), pot: 900, output: 0, tuning: 0, dtun: 0 };
        e.apply_model();
        Ok(e)
    }

    fn apply_model(&mut self) {
        let pot = self.pot as u16;
        self.synth.set_output(match self.output {
            1 => fm6::Output::Felucca { pot },
            2 => fm6::Output::Stock { pot },
            _ => fm6::Output::Msfa,
        });
        self.synth.set_tuning(match self.tuning {
            1 => fm6::Tuning::Felucca { dtun: self.dtun as u8 },
            2 => fm6::Tuning::Stock,
            _ => fm6::Tuning::Msfa,
        });
    }

    /// The voice as the DX7 packs it.
    pub fn packed(&self) -> [u8; 128] {
        fm6::pack_patch(&self.patch)
    }

    pub fn voice_name(&self) -> String {
        fm6::voice_name(&self.patch)
    }
}

impl Engine for Fm6Engine {
    fn kind(&self) -> &'static str {
        "fm6"
    }

    fn params(&self) -> Vec<ParamDesc> {
        let mut v: Vec<ParamDesc> = (0..155)
            .map(|i| {
                let (lo, hi) = fm6_range(i);
                pd(i as u16, &fm6::param_name(i), lo, hi, self.patch[i] as i32, "")
            })
            .collect();
        v.push(pd(FM6_POT, "MASTER POT", 0, 1023, 900, "pot"));
        v.push(pd(FM6_OUTPUT, "OUTPUT MODEL", 0, 2, 0, ""));
        v.push(pd(FM6_TUNING, "TUNING MODEL", 0, 2, 0, ""));
        v.push(pd(FM6_DTUN, "DTUN", 0, 127, 0, ""));
        v
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0..=154 => Some(self.patch[id as usize] as i32),
            FM6_POT => Some(self.pot),
            FM6_OUTPUT => Some(self.output),
            FM6_TUNING => Some(self.tuning),
            FM6_DTUN => Some(self.dtun),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, value: i32) -> bool {
        match id {
            0..=154 => {
                let (lo, hi) = fm6_range(id as usize);
                self.patch[id as usize] = value.clamp(lo, hi) as u8;
                self.synth.set_voice(&fm6::pack_patch(&self.patch));
            }
            FM6_POT => self.pot = value.clamp(0, 1023),
            FM6_OUTPUT => self.output = value.clamp(0, 2),
            FM6_TUNING => self.tuning = value.clamp(0, 2),
            FM6_DTUN => self.dtun = value.clamp(0, 127),
            _ => return false,
        }
        if id >= FM6_POT {
            self.apply_model();
        }
        true
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.synth.note_on(note, velocity);
    }

    fn note_off(&mut self, note: u8) {
        self.synth.note_off(note);
    }

    fn all_off(&mut self) {
        self.synth.all_off();
    }

    fn pitch_bend(&mut self, value: i32) {
        self.synth.set_pitch_bend(value);
    }

    fn render(&mut self, out: &mut [i16]) {
        self.synth.render(out);
    }

    fn sounding(&self) -> usize {
        self.synth.live_voices()
    }

    fn load_voice(&mut self, packed: &[u8; 128]) -> bool {
        self.patch = fm6::unpack_patch(packed);
        self.synth.set_voice(packed);
        true
    }

    fn current_voice(&self) -> Option<[u8; 128]> {
        Some(self.packed())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// sampler and kit

pub const SMP_MASTER: u16 = 0;
pub const SMP_RELEASE: u16 = 1;
pub const SMP_VOICES: u16 = 2;

pub struct SamplerEngine {
    s: Sampler,
    master: i32,
    release: i32,
    voices: i32,
}

impl SamplerEngine {
    pub fn new(sample: Sample, out_rate: f64, root: u8) -> Result<SamplerEngine> {
        Ok(SamplerEngine { s: Sampler::new(sample, out_rate, root)?, master: 100, release: 5, voices: 8 })
    }
}

impl Engine for SamplerEngine {
    fn kind(&self) -> &'static str {
        "sampler"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(SMP_MASTER, "MASTER", 0, 400, 100, "%"), pd(SMP_RELEASE, "RELEASE", 1, 5000, 5, "ms"), pd(SMP_VOICES, "VOICES", 1, 32, 8, "")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            SMP_MASTER => Some(self.master),
            SMP_RELEASE => Some(self.release),
            SMP_VOICES => Some(self.voices),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, value: i32) -> bool {
        match id {
            SMP_MASTER => {
                self.master = value.clamp(0, 400);
                self.s.set_master(self.master as f64 / 100.0);
            }
            SMP_RELEASE => {
                self.release = value.clamp(1, 5000);
                self.s.set_release(self.release as f64 / 1000.0);
            }
            SMP_VOICES => {
                self.voices = value.clamp(1, 32);
                self.s.set_voices(self.voices as usize);
            }
            _ => return false,
        }
        true
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.s.note_on(note, velocity);
    }

    fn note_off(&mut self, note: u8) {
        self.s.note_off(note);
    }

    fn all_off(&mut self) {
        self.s.all_off();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.s.render(out);
    }

    fn sounding(&self) -> usize {
        self.s.sounding()
    }
}

pub const KIT_CEILING: u16 = 0;

pub struct KitEngine {
    k: Kit,
    ceiling: i32,
}

impl KitEngine {
    pub fn new(k: Kit) -> KitEngine {
        KitEngine { k, ceiling: 0 }
    }
}

impl Engine for KitEngine {
    fn kind(&self) -> &'static str {
        "kit"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(KIT_CEILING, "LIMITER CEILING (0 = OFF)", 0, 100, 0, "%")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        (id == KIT_CEILING).then_some(self.ceiling)
    }

    fn set(&mut self, id: u16, value: i32) -> bool {
        if id != KIT_CEILING {
            return false;
        }
        self.ceiling = value.clamp(0, 100);
        self.k.set_ceiling((self.ceiling > 0).then(|| self.ceiling as f64 / 100.0), 0.05);
        true
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.k.note_on(note, velocity);
    }

    fn note_off(&mut self, note: u8) {
        self.k.note_off(note);
    }

    fn all_off(&mut self) {
        self.k.all_off();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.k.render(out);
    }

    fn sounding(&self) -> usize {
        self.k.sounding()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory;

    #[test]
    fn fm6_params_match_the_patch_and_clamp() {
        let bank = factory::voices();
        let mut e = Fm6Engine::new(44_100.0, &bank[0]).unwrap();
        let p = e.params();
        assert_eq!(p.len(), 155 + 4);
        assert_eq!(p[134].name, "ALGORITHM");
        assert_eq!(e.get(134), Some(31), "SINE is algorithm 32");
        assert!(e.set(134, 99));
        assert_eq!(e.get(134), Some(31), "clamped to the range");
        assert!(!e.set(5000, 1));
        assert_eq!(e.get(5000), None);
        for d in &p {
            assert!(d.min <= d.default && d.default <= d.max, "{} default in range", d.name);
        }
    }

    #[test]
    fn fm6_engine_plays_and_an_edit_changes_the_sound() {
        let bank = factory::voices();
        let mut e = Fm6Engine::new(44_100.0, &bank[0]).unwrap();
        let mut a = vec![0i16; 4410];
        e.note_on(60, 100);
        e.render(&mut a);
        assert!(a.iter().any(|&x| x.unsigned_abs() > 1000));
        assert_eq!(e.sounding(), 1);
        e.all_off();
        // edit the first carrier's output level to 0: new notes are silent
        let mut f = Fm6Engine::new(44_100.0, &bank[0]).unwrap();
        for op in 0..6 {
            assert!(f.set(16 + 21 * op, 0)); // every operator's output level
        }
        let mut b = vec![0i16; 4410];
        f.note_on(60, 100);
        f.render(&mut b);
        let peak = |x: &[i16]| x.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
        assert!(peak(&b) < peak(&a) / 4, "{} vs {}", peak(&b), peak(&a));
    }

    #[test]
    fn model_params_change_the_level() {
        let bank = factory::voices();
        let peak = |model: i32| {
            let mut e = Fm6Engine::new(44_100.0, &bank[1]).unwrap();
            e.set(FM6_OUTPUT, model);
            e.note_on(60, 100);
            let mut a = vec![0i16; 8820];
            e.render(&mut a);
            a.iter().map(|v| v.unsigned_abs()).max().unwrap()
        };
        assert_ne!(peak(0), peak(2));
    }

    #[test]
    fn sampler_and_kit_engines() {
        let tone: Vec<i16> = (0..4000).map(|i| ((i as f64 * 0.1).sin() * 8000.0) as i16).collect();
        let mut s = SamplerEngine::new(Sample::new(44_100, tone.clone()), 44_100.0, 60).unwrap();
        assert!(s.set(SMP_MASTER, 9999));
        assert_eq!(s.get(SMP_MASTER), Some(400));
        s.note_on(60, 100);
        let mut a = vec![0i16; 512];
        s.render(&mut a);
        assert!(a.iter().any(|&x| x != 0));
        let mut k = Kit::new(44_100.0).unwrap();
        k.add(36, Sample::new(44_100, tone), &[]).unwrap();
        let mut k = KitEngine::new(k);
        assert!(k.set(KIT_CEILING, 40));
        k.note_on(36, 127);
        k.render(&mut a);
        assert!(a.iter().any(|&x| x != 0));
        assert!(!k.set(9, 1));
    }
}
