//! The instrument host: a package becomes something that plays (docs/PORTING.md).
//!
//! It builds the package's engine and effects, applies the parameters, reads MIDI bytes (note on/off, controllers through the package's map, pitch bend, program
//! change across the bank, all notes off), renders mono or interleaved stereo with a safety limiter last, and writes the edited instrument back as a package.
//! Deterministic. Nothing here allocates while rendering (the scratch buffer is sized at `new`; a larger request is rendered in slices).

use crate::{
    analog::Analog,
    arp::{Arp, Event},
    seq::Seq,
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
    rate: f64,
    clock: crate::evq::Clock,
    arp: Arp,
    seq: Seq,
    /// frames rendered so far, the last clock tick's frame and the smoothed spacing of the ticks (MIDI clock sync for the arpeggiator and the sequencer)
    frames_total: u64,
    last_tick: Option<u64>,
    tick_frames: f64,
    arp_events: Vec<(usize, Event)>,
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
            rate,
            clock: crate::evq::Clock::new(),
            arp: Arp::new(rate),
            seq: Seq::new(rate),
            frames_total: 0,
            last_tick: None,
            tick_frames: rate * 60.0 / 120.0 / 24.0,
            arp_events: Vec::with_capacity(64),
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
        for &(id, v) in &pkg.arp {
            if !inst.arp.set(id, v) {
                return fail(format!("the arpeggiator has no parameter {id}"));
            }
        }
        for &(id, v) in &pkg.seq {
            if !inst.seq.set(id, v) {
                return fail(format!("the sequencer has no parameter {id}"));
            }
        }
        for &(p, st, note, vel) in &pkg.steps {
            if !inst.seq.set_step(usize::from(p), usize::from(st), note, vel) {
                return fail(format!("step {p}.{st} is outside 16 patterns of 16 steps"));
            }
        }
        for &(p, v) in &pkg.seq_voices {
            if !inst.seq.set_voice(usize::from(p), v) {
                return fail(format!("pattern {p} is outside 16 patterns"));
            }
        }
        if inst.arp.on() && inst.seq.on() {
            return fail("the arpeggiator and the sequencer exclude each other: a package has only one on");
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

    /// A key goes down: to the arpeggiator when it is on (which plays the engine from its own clock), else straight to the engine.
    pub fn note_on(&mut self, note: u8, velocity: u8) {
        if self.arp.on() {
            self.arp.note_on(note, velocity);
        } else if self.seq.on() {
            self.seq.record_note(note, velocity); // when armed, into the step it is nearest to; either way it is played live over the sequence
            self.engine.note_on(note, velocity);
        } else {
            self.engine.note_on(note, velocity);
        }
    }

    pub fn note_off(&mut self, note: u8) {
        if self.arp.on() {
            self.arp.note_off(note);
        } else {
            self.engine.note_off(note);
        }
    }

    pub fn all_off(&mut self) {
        self.arp.all_off();
        self.seq.all_off();
        self.flush_arp();
        self.engine.all_off();
    }

    /// The arpeggiator's parameters (`arp.rs`), described like an engine's.
    pub fn arp_params(&self) -> Vec<ParamDesc> {
        self.arp.params()
    }

    pub fn get_arp_param(&self, id: u16) -> Option<i32> {
        self.arp.get(id)
    }

    /// Sets an arpeggiator parameter. Switching it off releases the note it was holding; switching it on does not touch notes the engine is already playing.
    pub fn set_arp_param(&mut self, id: u16, v: i32) -> bool {
        let ok = self.arp.set(id, v);
        if ok && id == crate::arp::P_ON && v == 1 {
            self.seq.all_off(); // the arpeggiator and the sequencer exclude each other
        }
        self.flush_arp();
        ok
    }

    /// One event of the arpeggiator or the sequencer into the engine: a note on or off, or a pattern's voice.
    fn apply_event(&mut self, ev: Event) {
        match ev {
            Event::On(n, v) => self.engine.note_on(n, v),
            Event::Off(n) => self.engine.note_off(n),
            Event::Program(p) => self.set_program(usize::from(p)),
        }
    }

    /// Applies the arpeggiator's and the sequencer's queued note-offs (a stop) at once.
    fn flush_arp(&mut self) {
        self.arp_events.clear();
        self.arp.advance(0, &mut self.arp_events);
        self.seq.advance(0, &mut self.arp_events);
        for k in 0..self.arp_events.len() {
            let ev = self.arp_events[k].1;
            self.apply_event(ev);
        }
        self.arp_events.clear();
    }

    /// The sequencer's parameters: group 0 the settings, group 1 the pattern being edited (`seq.rs`).
    pub fn seq_params(&self, group: usize) -> Vec<ParamDesc> {
        self.seq.params(group)
    }

    pub fn get_seq_param(&self, id: u16) -> Option<i32> {
        self.seq.get(id)
    }

    /// Sets a sequencer parameter. The sequencer and the arpeggiator exclude each other (as on the FM-1): starting one stops the other.
    pub fn set_seq_param(&mut self, id: u16, v: i32) -> bool {
        let ok = self.seq.set(id, v);
        if ok && id == crate::seq::P_ON && v == 1 {
            self.arp.set(crate::arp::P_ON, 0);
        }
        self.flush_arp();
        ok
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
            self.realtime(b);
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

    /// MIDI real-time bytes, which can fall anywhere in a message without disturbing it: Clock (`F8`, 24 a quarter note) steps the arpeggiator and the sequencer that have
    /// SYNC on; Start and Continue (`FA`, `FB`, alike on the FM-1) begin them from the top (and start a synced sequencer); Stop (`FC`) ends the sound and, for a synced sequencer,
    /// the sequence. Everything else (active sensing, reset) is ignored. The tick's spacing, which sets the gate, is smoothed over the last ticks; a gap of half a second
    /// (a clock that stopped) starts the estimate again.
    fn realtime(&mut self, b: u8) {
        match b {
            0xF8 => {
                if let Some(last) = self.last_tick {
                    let gap = (self.frames_total - last) as f64;
                    if gap > self.rate * 0.5 {
                        self.tick_frames = self.rate * 60.0 / 120.0 / 24.0;
                    } else if gap >= 8.0 {
                        self.tick_frames = if self.tick_frames > 0.0 { self.tick_frames * 0.8 + gap * 0.2 } else { gap };
                    }
                }
                self.last_tick = Some(self.frames_total);
                self.arp.clock_tick(self.tick_frames);
                self.seq.clock_tick(self.tick_frames);
            }
            0xFA | 0xFB => {
                self.arp.clock_start();
                self.seq.clock_start();
                if self.seq.get(crate::seq::P_SYNC) == Some(1) && !self.seq.on() {
                    self.set_seq_param(crate::seq::P_ON, 1);
                }
            }
            0xFC => {
                self.arp.clock_stop();
                if self.seq.get(crate::seq::P_SYNC) == Some(1) && self.seq.on() {
                    self.set_seq_param(crate::seq::P_ON, 0);
                }
                self.flush_arp();
            }
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
        self.frames_total += out.len() as u64;
        for chunk in out.chunks_mut(SLICE) {
            self.arp_events.clear();
            self.arp.advance(chunk.len(), &mut self.arp_events);
            self.seq.advance(chunk.len(), &mut self.arp_events);
            if self.arp_events.is_empty() {
                self.engine.render(chunk);
            } else {
                // the engine plays up to each arpeggiator event, which then lands on its exact frame
                let mut at = 0;
                for k in 0..self.arp_events.len() {
                    let (frame, ev) = self.arp_events[k];
                    if frame > at {
                        self.engine.render(&mut chunk[at..frame]);
                        at = frame;
                    }
                    self.apply_event(ev);
                }
                self.engine.render(&mut chunk[at..]);
            }
            for f in &mut self.fx {
                f.process(chunk);
            }
            self.safety.process(chunk);
        }
    }

    /// Renders `out` (mono) with the messages of `q` that fall in this buffer applied at their own frames: the audio callback's way in, with no lock shared with the
    /// thread that pushes (`evq.rs`). `now_us` is the host's monotonic microsecond clock at the call, the same clock the producer stamped with. A message stamped
    /// before the buffer (a stall) is applied at its first frame; one stamped after it stays queued for the next buffer.
    pub fn render_events(&mut self, q: &crate::evq::EventQueue, now_us: u64, out: &mut [i16]) {
        let frames = out.len();
        let (start, per_us) = self.clock.window(now_us, frames, self.rate);
        let mut at = 0usize;
        while let Some(m) = q.peek() {
            let off = (m.ts_us as f64 - start) * per_us;
            if off >= frames as f64 {
                break;
            }
            q.pop();
            let frame = (off.max(0.0) as usize).max(at).min(frames);
            if frame > at {
                self.render(&mut out[at..frame]);
                at = frame;
            }
            if m.kind == crate::evq::Kind::Midi {
                self.midi_bytes(&m.bytes[..m.len as usize]);
            }
        }
        self.render(&mut out[at..]);
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
        for d in self.arp.params() {
            let v = self.arp.get(d.id).unwrap_or(d.default);
            if v != d.default {
                p.arp.push((d.id, v));
            }
        }
        for d in self.seq.params(0) {
            let v = self.seq.get(d.id).unwrap_or(d.default);
            if v != d.default {
                p.seq.push((d.id, v));
            }
        }
        for pat in 0..crate::seq::PATTERNS {
            for st in 0..crate::seq::STEPS {
                if let Some((note, vel)) = self.seq.step(pat, st) {
                    if note != 0 {
                        p.steps.push((pat as u8, st as u8, note, vel));
                    }
                }
            }
            if let Some(v) = self.seq.voice(pat).filter(|&v| v >= 0) {
                p.seq_voices.push((pat as u8, v));
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

    #[test]
    fn an_arpeggiator_package_plays_a_pattern_and_round_trips() {
        let mut p = Package::new("A", "analog", "MIT");
        p.params.push((crate::analog::P_RELEASE, 5)); // a short release, so the gate's gaps are audible
        p.arp = vec![(crate::arp::P_ON, 1), (crate::arp::P_BPM, 240), (crate::arp::P_RATE, 2), (crate::arp::P_GATE, 60), (crate::arp::P_OCTAVES, 2)];
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        i.midi_bytes(&[0x90, 60, 100, 0x90, 64, 100]);
        let mut b = vec![0i16; 44_100];
        i.render(&mut b);
        // 240 BPM at 1/16 is 2756 frames a step: the note changes eight times a half second, and the sound comes in bursts, with silence between gated notes
        let bursts = b.windows(2).filter(|w| w[0].abs() < 4 && w[1].abs() >= 4).count();
        assert!(b.iter().any(|&x| x.abs() > 300), "the arpeggiator played");
        let quiet = b.chunks(64).filter(|c| c.iter().all(|&x| x.abs() < 3)).count();
        assert!(quiet > 20, "a 60 % gate leaves gaps ({quiet} quiet blocks, {bursts} onsets)");
        i.midi_bytes(&[0x80, 60, 0, 0x80, 64, 0]);
        let mut c = vec![0i16; 22_050];
        i.render(&mut c);
        assert!(c[15_000..].iter().all(|&x| x.abs() < 3), "released: it stops");
        // the settings go back to a package and through the text form
        let back = i.to_package("t", "0");
        let mut want = p.arp.clone();
        want.sort();
        assert_eq!(back.arp, want, "every setting here differs from its default, in id order");
        let again = Package::parse(&back.to_text()).unwrap();
        assert_eq!(again.arp, back.arp);
        // off: notes go straight to the engine; switching off mid-note releases it
        assert!(i.set_arp_param(crate::arp::P_ON, 0) && !i.arp.on());
        i.note_on(60, 100);
        let mut d = vec![0i16; 4410];
        i.render(&mut d);
        assert!(d.iter().any(|&x| x.abs() > 300));
        let mut q = Package::new("A", "analog", "MIT");
        q.arp.push((77, 1));
        assert!(Instrument::from_package(&q, 44_100.0, &none).is_err(), "an unknown arp parameter is refused");
    }

    #[test]
    fn events_land_on_their_frames_and_late_or_future_ones_are_kept_in_order() {
        use crate::evq::EventQueue;
        let mut p = Package::new("A", "analog", "MIT");
        p.params.push((crate::analog::P_ATTACK, 0));
        p.params.push((crate::analog::P_RELEASE, 1));
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        let q = EventQueue::new(64);
        // a 441-frame buffer (10 ms) at host time 100_000 us covers 90_000 to 100_000 us; a note stamped at 95_000 starts 220 frames in
        assert!(q.push(95_000, &[0x90, 69, 127]));
        let mut b = vec![0i16; 441];
        i.render_events(&q, 100_000, &mut b);
        assert!(b[..215].iter().all(|&x| x == 0), "silent before the note");
        assert!(b[240..].iter().any(|&x| x.abs() > 100), "sounding after it");
        // a note stamped for the next buffer stays queued; one stamped long ago plays at frame 0; the order of the queue is kept
        assert!(q.push(112_000, &[0x80, 69, 0]));
        assert!(q.push(112_500, &[0x90, 60, 100]));
        let mut c = vec![0i16; 441];
        i.render_events(&q, 110_000, &mut c);
        assert_eq!(q.len(), 2, "both are in the future: kept");
        assert!(c.iter().any(|&x| x.abs() > 100));
        let mut d = vec![0i16; 441];
        i.render_events(&q, 120_000, &mut d);
        assert!(q.is_empty(), "both played in the third buffer");
        assert!(i.sounding() >= 1);
        // no queue traffic: the same sound as an ordinary render
        let (mut x, mut y) = (Instrument::from_package(&p, 44_100.0, &none).unwrap(), Instrument::from_package(&p, 44_100.0, &none).unwrap());
        x.note_on(57, 100);
        y.note_on(57, 100);
        let (mut bx, mut by) = (vec![0i16; 3000], vec![0i16; 3000]);
        x.render(&mut bx);
        y.render_events(&q, 5_000_000, &mut by);
        assert_eq!(bx, by);
        // messages the instrument ignores (a system-exclusive piece, running status without a status) do nothing and cost nothing
        assert!(q.push(5_000_001, &[0x42]));
        i.render_events(&q, 5_020_000, &mut by);
        assert!(q.is_empty());
    }

    #[test]
    fn a_sequencer_package_plays_its_patterns_with_their_voices_and_excludes_the_arpeggiator() {
        let mut p = Package::new("S", "fm6", "MIT");
        p.seq = vec![(crate::seq::P_ON, 1), (crate::seq::P_BPM, 240), (crate::seq::P_STEPS, 2), (crate::seq::P_CHAIN, 2)];
        p.steps = vec![(0, 0, 60, 100), (0, 1, 64, 100), (1, 0, 67, 100)];
        p.seq_voices = vec![(1, 3)];
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let mut loud = 0;
        let mut b = vec![0i16; 1024];
        for _ in 0..90 {
            i.render(&mut b);
            seen.insert(i.program());
            loud += b.iter().filter(|&&x| x.abs() > 200).count();
        }
        assert!(seen.contains(&3) && seen.contains(&0), "pattern 2 selected voice 3 and pattern 1 left voice 0: {seen:?}");
        assert!(loud > 2000, "the sequence sounded ({loud})");
        // text and back
        let back = i.to_package("t", "0");
        let again = Package::parse(&back.to_text()).unwrap();
        assert_eq!((again.seq.clone(), again.steps.clone(), again.seq_voices.clone()), (back.seq.clone(), back.steps.clone(), back.seq_voices.clone()));
        assert!(back.steps.contains(&(0, 0, 60, 100)) && back.seq_voices == vec![(1, 3)]);
        // starting the arpeggiator stops the sequencer, and the other way round
        assert!(i.set_arp_param(crate::arp::P_ON, 1));
        assert_eq!(i.get_seq_param(crate::seq::P_ON), Some(0));
        assert!(i.set_seq_param(crate::seq::P_ON, 1));
        assert_eq!(i.get_arp_param(crate::arp::P_ON), Some(0));
        // a package with both on, an unknown id and a step off the grid are refused
        let mut both = p.clone();
        both.arp = vec![(crate::arp::P_ON, 1)];
        assert!(Instrument::from_package(&both, 44_100.0, &none).is_err());
        let mut bad = p.clone();
        bad.seq.push((77, 1));
        assert!(Instrument::from_package(&bad, 44_100.0, &none).is_err());
        let mut off = p.clone();
        off.steps.push((0, 16, 60, 100));
        assert!(Instrument::from_package(&off, 44_100.0, &none).is_err());
        // stopping releases the note it was playing
        i.set_seq_param(crate::seq::P_ON, 0);
        let mut c = vec![0i16; 4410];
        for _ in 0..40 {
            i.render(&mut c);
        }
        assert_eq!(i.sounding(), 0);
    }

    /// MIDI clock through the queue: ticks stamped at their own times put the arpeggiator's steps on them, whatever the buffer size; Start and Stop drive a synced sequencer.
    #[test]
    fn midi_clock_drives_a_synced_arpeggiator_and_sequencer_through_the_queue() {
        use crate::evq::EventQueue;
        let mut p = Package::new("C", "analog", "MIT");
        p.params.push((crate::analog::P_RELEASE, 1));
        p.arp = vec![(crate::arp::P_ON, 1), (crate::arp::P_SYNC, 1), (crate::arp::P_RATE, 2)];
        // a note first, then ticks: the queue carries the note too, stamped before the first tick
        let run = |buffer: usize| {
            let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
            let q = EventQueue::new(1024);
            let us = |frame: f64| 1_000_000 + (frame * 1e6 / 44_100.0).round() as u64;
            q.push(us(0.0), &[0x90, 60, 100]);
            for k in 0..30 {
                q.push(us(k as f64 * 918.75 + 1.0), &[0xF8]);
            }
            let total = 30 * 919;
            let mut out = vec![0i16; total];
            let mut at = 0;
            while at < total {
                let n = buffer.min(total - at);
                i.render_events(&q, us((at + n) as f64), &mut out[at..at + n]);
                at += n;
            }
            out
        };
        let a = run(512);
        let onsets = |x: &[i16]| x.windows(2).enumerate().filter(|(_, w)| w[0].abs() < 3 && w[1].abs() >= 3).map(|(k, _)| k).collect::<Vec<_>>();
        let o = onsets(&a);
        assert!(o.len() >= 4, "steps sounded: {o:?}");
        let gaps: Vec<i64> = o.windows(2).map(|w| w[1] as i64 - w[0] as i64).filter(|&g| g > 3000).take(3).collect();
        assert!(gaps.iter().all(|&g| (g - 5513).abs() < 30), "a step every 6 ticks (5512.5 frames), found {gaps:?}");
        let b97 = onsets(&run(97));
        assert!(b97.len() == o.len() && o.iter().zip(&b97).all(|(x, y)| (*x as i64 - *y as i64).abs() <= 2), "the buffer size does not move a step: {o:?} against {b97:?}");
        // a synced sequencer: Start begins it on the next tick, Stop ends it
        let mut sp = Package::new("S", "analog", "MIT");
        sp.params.push((crate::analog::P_RELEASE, 1));
        sp.seq = vec![(crate::seq::P_SYNC, 1), (crate::seq::P_RATE, 2), (crate::seq::P_STEPS, 4)];
        sp.steps = vec![(0, 0, 60, 100), (0, 1, 64, 100), (0, 2, 67, 100)];
        let mut si = Instrument::from_package(&sp, 44_100.0, &none).unwrap();
        let mut b = vec![0i16; 2205];
        si.render(&mut b);
        assert!(b.iter().all(|&x| x == 0), "stopped until Start");
        si.midi_bytes(&[0xFA, 0xF8]);
        assert_eq!(si.get_seq_param(crate::seq::P_ON), Some(1), "Start starts a synced sequencer");
        let mut c = vec![0i16; 2205];
        si.render(&mut c);
        assert!(c.iter().any(|&x| x.abs() > 100), "its first step sounded on the first tick");
        si.midi_bytes(&[0xFC]);
        assert_eq!(si.get_seq_param(crate::seq::P_ON), Some(0), "Stop stops it");
        // real-time bytes inside a message do not break it, and active sensing or reset are ignored
        let mut m = Instrument::from_package(&Package::new("M", "analog", "MIT"), 44_100.0, &none).unwrap();
        m.midi_bytes(&[0x90, 0xF8, 60, 0xFE, 100]);
        let mut d = vec![0i16; 2000];
        m.render(&mut d);
        assert!(d.iter().any(|&x| x.abs() > 100), "a clock byte between a status and its data leaves the note whole");
    }
}
