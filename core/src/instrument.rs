//! The instrument host: a package becomes something that plays (docs/PORTING.md).
//!
//! It builds the package's engine and effects, applies the parameters, reads MIDI bytes (note on/off, controllers through the package's map, pitch bend, program
//! change across the bank, all notes off), renders mono or interleaved stereo with a safety limiter last, and writes the edited instrument back as a package.
//! Deterministic. Nothing here allocates while rendering (the scratch buffer is sized at `new`; a larger request is rendered in slices).

use crate::{
    analog::Analog,
    fm4::Fm4,
    engine::{Engine, Fm6Engine, KitEngine, ParamDesc, SamplerEngine},
    factory,
    fx::{self, Effect, Limiter},
    package::{Package, Target},
    sampler::{parse_wav, Kit},
    Error, Result,
};

/// Reads a sample file named in a package (a plain relative path): the host's job, since the core does no I/O.
pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

const SLICE: usize = 1024;

pub struct Instrument {
    name: String,
    licence: String,
    verify: String,
    engine: Box<dyn Engine>,
    fx: Vec<Box<dyn Effect>>,
    fx_names: Vec<String>,
    safety: Limiter,
    map: Vec<crate::package::MapEntry>,
    voices: Vec<[u8; 128]>,
    program: usize,
    mono: Vec<i16>,
    // MIDI parser state
    status: u8,
    data: [u8; 2],
    have: usize,
    in_sysex: bool,
}

fn fail<T>(why: impl Into<String>) -> Result<T> {
    Err(Error::BadImage(format!("instrument: {}", why.into())))
}

impl Instrument {
    pub fn from_package(pkg: &Package, rate: f64, resolve: Resolver) -> Result<Instrument> {
        pkg.tier()?;
        let mut voices = pkg.voices.clone();
        let engine: Box<dyn Engine> = match pkg.engine.as_str() {
            "fm6" => {
                if voices.is_empty() {
                    voices = factory::voices();
                }
                let v = voices[pkg.program.min(voices.len() - 1)];
                Box::new(Fm6Engine::new(rate, &v)?)
            }
            "analog" => Box::new(Analog::new(rate)),
            "fm4" => Box::new(Fm4::new(rate)),
            "sampler" => {
                let Some(s) = pkg.samples.first() else { return fail("a sampler package needs a `sample` line") };
                let Some(bytes) = resolve(&s.file) else { return fail(format!("cannot read sample {:?}", s.file)) };
                Box::new(SamplerEngine::new(parse_wav(&bytes)?, rate, pkg.root)?)
            }
            "kit" => {
                let mut k = Kit::new(rate)?;
                if pkg.samples.is_empty() {
                    return fail("a kit package needs `sample` lines");
                }
                for s in &pkg.samples {
                    let Some(bytes) = resolve(&s.file) else { return fail(format!("cannot read sample {:?}", s.file)) };
                    k.add(s.note, parse_wav(&bytes)?, &s.chokes)?;
                }
                Box::new(KitEngine::new(k))
            }
            other => return fail(format!("no engine called {other:?} (fm6, fm4, analog, sampler, kit)")),
        };
        let mut inst = Instrument {
            name: pkg.name.clone(),
            licence: pkg.licence.clone(),
            verify: pkg.verify.clone(),
            engine,
            fx: Vec::new(),
            fx_names: Vec::new(),
            safety: {
                let mut l = Limiter::new(rate);
                l.set(0, 95);
                l
            },
            map: pkg.map.clone(),
            voices,
            program: pkg.program,
            mono: vec![0; SLICE],
            status: 0,
            data: [0; 2],
            have: 0,
            in_sysex: false,
        };
        for &(id, v) in &pkg.params {
            if !inst.engine.set(id, v) {
                return fail(format!("the {} engine has no parameter {id}", pkg.engine));
            }
        }
        for f in &pkg.fx {
            let Some(mut e) = fx::make(&f.name, rate) else { return fail(format!("no effect called {:?}", f.name)) };
            for &(id, v) in &f.params {
                if !e.set(id, v) {
                    return fail(format!("the {} effect has no parameter {id}", f.name));
                }
            }
            inst.fx_names.push(f.name.clone());
            inst.fx.push(e);
        }
        for m in &pkg.map {
            match m.target {
                Target::Engine(id) if inst.engine.get(id).is_none() => return fail(format!("map: no engine parameter {id}")),
                Target::Fx(i, id) if inst.fx.get(i).and_then(|f| f.get(id)).is_none() => return fail(format!("map: no effect parameter {i}.{id}")),
                _ => {}
            }
        }
        Ok(inst)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn licence(&self) -> &str {
        &self.licence
    }

    /// What verification measured when the package was made; empty means unverified.
    pub fn verify_note(&self) -> &str {
        &self.verify
    }

    pub fn kind(&self) -> &'static str {
        self.engine.kind()
    }

    pub fn params(&self) -> Vec<ParamDesc> {
        self.engine.params()
    }

    pub fn get_param(&self, id: u16) -> Option<i32> {
        self.engine.get(id)
    }

    pub fn set_param(&mut self, id: u16, v: i32) -> bool {
        self.engine.set(id, v)
    }

    pub fn fx_count(&self) -> usize {
        self.fx.len()
    }

    pub fn fx_name(&self, i: usize) -> Option<&str> {
        self.fx_names.get(i).map(|s| s.as_str())
    }

    pub fn fx_params(&self, i: usize) -> Vec<ParamDesc> {
        self.fx.get(i).map(|f| f.params()).unwrap_or_default()
    }

    pub fn get_fx_param(&self, i: usize, id: u16) -> Option<i32> {
        self.fx.get(i)?.get(id)
    }

    pub fn set_fx_param(&mut self, i: usize, id: u16, v: i32) -> bool {
        self.fx.get_mut(i).is_some_and(|f| f.set(id, v))
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    pub fn program(&self) -> usize {
        self.program
    }

    /// Loads voice `i` of the bank (wrapping), for engines with banks. The edits of the voice before are kept in the bank.
    pub fn set_program(&mut self, i: usize) {
        if self.voices.is_empty() {
            return;
        }
        if let Some(v) = self.engine.current_voice() {
            if let Some(slot) = self.voices.get_mut(self.program) {
                *slot = v;
            }
        }
        self.program = i % self.voices.len();
        let v = self.voices[self.program];
        self.engine.load_voice(&v);
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        self.engine.note_on(note, velocity);
    }

    pub fn note_off(&mut self, note: u8) {
        self.engine.note_off(note);
    }

    pub fn all_off(&mut self) {
        self.engine.all_off();
    }

    pub fn sounding(&self) -> usize {
        self.engine.sounding()
    }

    /// A controller change through the map; false when nothing is mapped to it.
    pub fn cc(&mut self, cc: u8, value: u8) -> bool {
        let mut hit = false;
        for k in 0..self.map.len() {
            let m = self.map[k];
            if m.cc != cc {
                continue;
            }
            let v = m.min + ((m.max - m.min) as f64 * value.min(127) as f64 / 127.0).round() as i32;
            match m.target {
                Target::Engine(id) => self.engine.set(id, v),
                Target::Fx(i, id) => self.set_fx_param(i, id, v),
            };
            hit = true;
        }
        hit
    }

    /// One MIDI byte. Running status works, real-time bytes are ignored, a system-exclusive message is skipped to its end, the channel is ignored (omni).
    pub fn midi(&mut self, b: u8) {
        if b >= 0xF8 {
            return;
        }
        if self.in_sysex {
            if b == 0xF7 || b >= 0x80 {
                self.in_sysex = false;
                self.status = 0;
                self.have = 0;
                if b != 0xF7 {
                    return self.midi(b);
                }
            }
            return;
        }
        if b >= 0x80 {
            if b == 0xF0 {
                self.in_sysex = true;
                self.status = 0;
                return;
            }
            self.status = if b >= 0xF0 { 0 } else { b };
            self.have = 0;
            return;
        }
        let need = match self.status & 0xF0 {
            0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
            0xC0 | 0xD0 => 1,
            _ => return,
        };
        self.data[self.have] = b;
        self.have += 1;
        if self.have < need {
            return;
        }
        self.have = 0;
        let (d0, d1) = (self.data[0], self.data[1]);
        match self.status & 0xF0 {
            0x80 => self.note_off(d0),
            0x90 => {
                if d1 == 0 {
                    self.note_off(d0)
                } else {
                    self.note_on(d0, d1)
                }
            }
            0xB0 => match d0 {
                120 | 123 => self.all_off(),
                _ => {
                    self.cc(d0, d1);
                }
            },
            0xC0 => self.set_program(d0 as usize),
            0xE0 => self.engine.pitch_bend(d0 as i32 | (d1 as i32) << 7),
            _ => {}
        }
    }

    pub fn midi_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.midi(b);
        }
    }

    /// Replaces `out` with mono samples: the engine, the effects in order, then the safety limiter (95%).
    pub fn render(&mut self, out: &mut [i16]) {
        for chunk in out.chunks_mut(SLICE) {
            self.engine.render(chunk);
            for f in &mut self.fx {
                f.process(chunk);
            }
            self.safety.process(chunk);
        }
    }

    /// Replaces `out` with interleaved stereo (the mono mix on both sides); an odd length leaves its last sample silent.
    pub fn render_stereo(&mut self, out: &mut [i16]) {
        let frames = out.len() / 2;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(SLICE);
            let mut m = std::mem::take(&mut self.mono);
            self.render(&mut m[..n]);
            for (i, &s) in m[..n].iter().enumerate() {
                out[(done + i) * 2] = s;
                out[(done + i) * 2 + 1] = s;
            }
            self.mono = m;
            done += n;
        }
        if out.len() % 2 == 1 {
            out[out.len() - 1] = 0;
        }
    }

    /// The instrument as it is now, as a package (the edits included): the voice bank for an engine with one, and every parameter that differs from its default
    /// for the rest. The provenance and the verification note are kept, since the data still comes from the same source; **a voice you edited is no longer what was
    /// verified**, so the note says so.
    pub fn to_package(&self, source: &str, source_sha256: &str) -> Package {
        let mut p = Package::new(&self.name, self.engine.kind(), &self.licence);
        p.source = source.into();
        p.source_sha256 = source_sha256.into();
        p.verify = self.verify.clone();
        p.map = self.map.clone();
        p.voices = self.voices.clone();
        p.program = self.program;
        if let Some(v) = self.engine.current_voice() {
            if p.voices.is_empty() {
                p.voices.push(v);
                p.program = 0;
            } else if let Some(slot) = p.voices.get_mut(self.program) {
                if *slot != v && !p.verify.is_empty() {
                    p.verify = format!("{} (edited since)", p.verify);
                }
                *slot = v;
            }
        }
        let skip_patch = self.engine.current_voice().is_some();
        for d in self.engine.params() {
            if skip_patch && d.id < 200 {
                continue;
            }
            if let Some(v) = self.engine.get(d.id) {
                if v != d.default {
                    p.params.push((d.id, v));
                }
            }
        }
        for (i, f) in self.fx.iter().enumerate() {
            let mut e = crate::package::FxEntry { name: self.fx_names[i].clone(), params: Vec::new() };
            for d in f.params() {
                e.params.push((d.id, f.get(d.id).unwrap_or(d.default)));
            }
            p.fx.push(e);
        }
        p
    }
}

/// The eight built-in voices as an `fm6` package: the project's own, MIT (`factory.rs`).
pub fn factory_package() -> Package {
    let mut p = Package::new("Built-in voices", "fm6", "MIT");
    p.voices = factory::voices();
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{FxEntry, MapEntry};

    fn none(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn peak(x: &[i16]) -> u16 {
        x.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0)
    }

    #[test]
    fn factory_package_plays_from_midi_with_running_status() {
        let mut i = Instrument::from_package(&factory_package(), 44_100.0, &none).unwrap();
        assert_eq!(i.kind(), "fm6");
        assert_eq!(i.voice_count(), 8);
        i.midi_bytes(&[0x90, 60, 100, 64, 100, 0xF8, 67, 100]); // running status, a clock byte between
        assert_eq!(i.sounding(), 3);
        let mut out = vec![0i16; 4410];
        i.render(&mut out);
        assert!(peak(&out) > 500);
        i.midi_bytes(&[0xB0, 123, 0]); // all notes off
        i.midi_bytes(&[0x80, 60, 0]);
        let mut st = vec![0i16; 2048];
        i.render_stereo(&mut st);
        assert_eq!(st.len(), 2048);
        assert_eq!(st[10], st[11]);
    }

    #[test]
    fn sysex_and_garbage_do_not_play_or_panic() {
        let mut i = Instrument::from_package(&factory_package(), 44_100.0, &none).unwrap();
        i.midi_bytes(&[0x90, 60, 100]);
        i.midi_bytes(&[0xF0, 0x43, 0x3C, 0x64, 0xF7, 0x3D, 0x64]); // a sysex of data bytes, then data bytes that no status owns
        assert_eq!(i.sounding(), 1, "neither the sysex's bytes nor the orphans are notes");
        for b in 0..=255u8 {
            i.midi(b);
            i.midi(b.wrapping_mul(7));
        }
        let mut out = vec![0i16; 3000];
        i.render(&mut out);
    }

    #[test]
    fn program_change_and_edit_round_trip_through_a_package() {
        let mut i = Instrument::from_package(&factory_package(), 44_100.0, &none).unwrap();
        i.midi_bytes(&[0xC0, 3]);
        assert_eq!(i.program(), 3);
        assert!(i.set_param(135, 5)); // feedback
        let p = i.to_package("by hand", "");
        let q = Package::parse(&p.to_text()).unwrap();
        let j = Instrument::from_package(&q, 44_100.0, &none).unwrap();
        assert_eq!(j.program(), 3);
        assert_eq!(j.get_param(135), Some(5), "the edit survived the round trip");
        assert_eq!(p.tier().unwrap(), crate::package::Tier::Permissive);
    }

    #[test]
    fn map_and_effects() {
        let mut p = Package::new("A", "analog", "MIT");
        p.fx.push(FxEntry { name: "delay".into(), params: vec![(2, 50)] });
        p.map.push(MapEntry { cc: 74, target: Target::Engine(2), min: 0, max: 127 });
        p.map.push(MapEntry { cc: 91, target: Target::Fx(0, 2), min: 0, max: 100 });
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        assert!(i.cc(74, 127));
        assert_eq!(i.get_param(2), Some(127));
        i.midi_bytes(&[0xB0, 91, 0]);
        assert_eq!(i.get_fx_param(0, 2), Some(0));
        assert!(!i.cc(5, 5));
        assert_eq!(i.fx_name(0), Some("delay"));
        // a map to nothing is refused at load
        p.map.push(MapEntry { cc: 1, target: Target::Engine(999), min: 0, max: 1 });
        assert!(Instrument::from_package(&p, 44_100.0, &none).is_err());
    }

    #[test]
    fn an_fm4_package_plays_with_a_map_and_a_filter() {
        let mut p = Package::new("F4", "fm4", "MIT");
        p.params.push((crate::fm4::P_ALGORITHM, 7));
        p.fx.push(FxEntry { name: "filter".into(), params: vec![(0, 0)] });
        p.map.push(MapEntry { cc: 74, target: Target::Engine(crate::fm4::op_param(0, crate::fm4::O_LEVEL)), min: 0, max: 99 });
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        assert_eq!(i.kind(), "fm4");
        i.note_on(60, 100);
        let mut b = vec![0i16; 4410];
        i.render(&mut b);
        assert!(b.iter().any(|&x| x.abs() > 300));
        assert!(i.cc(74, 0), "the map reaches an operator's level");
        let back = i.to_package("test", "0");
        assert_eq!(back.engine, "fm4");
    }

    #[test]
    fn bad_packages_are_refused() {
        let mut p = Package::new("A", "nonesuch", "MIT");
        assert!(Instrument::from_package(&p, 44_100.0, &none).is_err());
        p.engine = "sampler".into();
        assert!(Instrument::from_package(&p, 44_100.0, &none).is_err(), "no sample");
        p.samples.push(crate::package::SampleRef { note: 36, file: "a.wav".into(), chokes: vec![] });
        assert!(Instrument::from_package(&p, 44_100.0, &none).is_err(), "unreadable sample");
        let mut q = Package::new("A", "analog", "MIT");
        q.params.push((77, 1));
        assert!(Instrument::from_package(&q, 44_100.0, &none).is_err(), "unknown parameter");
        let mut r = Package::new("A", "analog", "MIT");
        r.fx.push(FxEntry { name: "reverbish".into(), params: vec![] });
        assert!(Instrument::from_package(&r, 44_100.0, &none).is_err());
    }

    #[test]
    fn a_sampler_package_loads_its_wav() {
        // a minimal 16-bit mono PCM WAV of a short tone
        let data: Vec<u8> = (0..2000i32).flat_map(|k| (((k as f64 * 0.2).sin() * 9000.0) as i16).to_le_bytes()).collect();
        let mut w = b"RIFF".to_vec();
        w.extend((36 + data.len() as u32).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(44_100u32.to_le_bytes());
        w.extend(88_200u32.to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend((data.len() as u32).to_le_bytes());
        w.extend(&data);
        let mut p = Package::new("S", "kit", "CC0-1.0");
        p.samples.push(crate::package::SampleRef { note: 36, file: "kick.wav".into(), chokes: vec![] });
        let resolve = |f: &str| (f == "kick.wav").then(|| w.clone());
        let mut i = Instrument::from_package(&p, 44_100.0, &resolve).unwrap();
        i.midi_bytes(&[0x99, 36, 127]);
        let mut out = vec![0i16; 1024];
        i.render(&mut out);
        assert!(peak(&out) > 100);
    }

    #[test]
    fn safety_limiter_holds_a_loud_engine() {
        let mut p = Package::new("A", "analog", "MIT");
        p.params.push((10, 100));
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        for n in 40..48 {
            i.note_on(n, 127);
        }
        let mut out = vec![0i16; 8192];
        i.render(&mut out);
        assert!(peak(&out) <= (0.95 * 32767.0) as u16 + 1);
    }
}
