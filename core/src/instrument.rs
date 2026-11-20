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
    globals::{Globals, Route, G_BEND, G_FX_CHANNEL, G_GLIDE, G_LEGATO, G_MONO, G_STOCK_CC},
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
    /// an effect switched out of the chain by the stock effect controllers (live state, not saved in a package)
    fx_off: Vec<bool>,
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
    globals: Globals,
    bank_names: Vec<String>,
    /// notes held in mono mode, in the order played (the last one sounds)
    mono_stack: Vec<(u8, u8)>,
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
            fx_off: Vec::new(),
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
            globals: Globals::new(),
            bank_names: vec![String::new(); crate::package::MAX_BANKS],
            mono_stack: Vec::with_capacity(16),
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
        for &(id, v) in &pkg.globals {
            if !inst.set_global(id, v) {
                return fail(format!("there is no global setting {id}"));
            }
        }
        for (b, n) in &pkg.banks {
            if let Some(slot) = inst.bank_names.get_mut(usize::from(*b)) {
                *slot = n.clone();
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
            inst.fx_off.push(false);
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

    /// Moves effect `from` to place `to` in the chain; the package's controller map follows the effects it points at. False when either is out of range.
    pub fn move_fx(&mut self, from: usize, to: usize) -> bool {
        let n = self.fx.len();
        if from >= n || to >= n {
            return false;
        }
        if from == to {
            return true;
        }
        let e = self.fx.remove(from);
        self.fx.insert(to, e);
        let name = self.fx_names.remove(from);
        self.fx_names.insert(to, name);
        let off = self.fx_off.remove(from);
        self.fx_off.insert(to, off);
        let place = |i: usize| -> usize {
            if i == from {
                to
            } else if from < to && i > from && i <= to {
                i - 1
            } else if to < from && i >= to && i < from {
                i + 1
            } else {
                i
            }
        };
        for m in &mut self.map {
            if let Target::Fx(i, id) = m.target {
                m.target = Target::Fx(place(i), id);
            }
        }
        true
    }

    /// The global settings (`globals.rs`), described like an engine's.
    pub fn global_params(&self) -> Vec<ParamDesc> {
        self.globals.params()
    }

    pub fn get_global(&self, id: u16) -> Option<i32> {
        self.globals.get(id)
    }

    /// Sets a global setting. The bend range reaches the engine at once; mono on or off lets go of every note.
    pub fn set_global(&mut self, id: u16, v: i32) -> bool {
        if !self.globals.set(id, v) {
            return false;
        }
        match id {
            G_BEND => self.engine.bend_range(self.globals.get(G_BEND).unwrap_or(2)),
            G_MONO => self.all_off(),
            G_GLIDE => self.engine.glide(self.globals.get(G_GLIDE).unwrap_or(0)),
            _ => {}
        }
        true
    }

    /// Banks of 32 voices: how many the voices fill, a bank's name (a default when the package names none), the bank the program is in.
    pub fn bank_count(&self) -> usize {
        self.voices.len().div_ceil(crate::package::BANK_SIZE)
    }

    pub fn bank_name(&self, b: usize) -> String {
        match self.bank_names.get(b) {
            Some(n) if !n.is_empty() => n.clone(),
            _ => format!("BANK {}", b + 1),
        }
    }

    pub fn set_bank_name(&mut self, b: usize, name: &str) -> bool {
        let ok = name.len() <= 32 && name.is_ascii() && !name.contains(['\n', '\r']);
        match self.bank_names.get_mut(b) {
            Some(slot) if ok => {
                *slot = name.into();
                true
            }
            _ => false,
        }
    }

    pub fn bank(&self) -> usize {
        self.program / crate::package::BANK_SIZE
    }

    /// Goes to bank `b`, to the same slot (or the last voice there is); false when the bank does not exist.
    pub fn set_bank(&mut self, b: usize) -> bool {
        if b >= self.bank_count() {
            return false;
        }
        let at = (b * crate::package::BANK_SIZE + self.program % crate::package::BANK_SIZE).min(self.voices.len() - 1);
        self.set_program(at);
        true
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
        let velocity = self.globals.velocity(velocity);
        if self.globals.get(G_MONO) == Some(1) && !self.arp.on() && !self.seq.on() {
            let top = self.mono_stack.last().map(|&(n, _)| n);
            let legato = self.globals.get(G_LEGATO) == Some(1) && top.is_some_and(|t| t != note && self.engine.legato(t, note));
            if !legato {
                if let Some(top) = top {
                    self.engine.note_off(top);
                }
            }
            self.mono_stack.retain(|&(n, _)| n != note);
            if self.mono_stack.len() >= 16 {
                self.mono_stack.remove(0);
            }
            self.mono_stack.push((note, velocity));
            if !legato {
                self.engine.note_on(note, velocity);
            }
        } else if self.arp.on() {
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
        } else if self.globals.get(G_MONO) == Some(1) && !self.seq.on() {
            // the sounding note let go goes back to the one before it, struck again; a note under it is just forgotten
            let was_top = self.mono_stack.last().map(|&(n, _)| n) == Some(note);
            self.mono_stack.retain(|&(n, _)| n != note);
            if was_top {
                match self.mono_stack.last() {
                    Some(&(n, _)) if self.globals.get(G_LEGATO) == Some(1) && self.engine.legato(note, n) => {}
                    Some(&(n, v)) => {
                        self.engine.note_off(note);
                        self.engine.note_on(n, v);
                    }
                    None => self.engine.note_off(note),
                }
            }
        } else {
            self.engine.note_off(note);
        }
    }

    pub fn all_off(&mut self) {
        self.mono_stack.clear();
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

    /// A stock FM-1 effect controller (`stockmap.rs`, controllers 0 to 23) on the first effect of that name in the chain; false when there is none or `cc` is not one.
    fn stock_cc(&mut self, cc: u8, value: u8) -> bool {
        let Some((name, act)) = crate::stockmap::act(cc, value) else { return false };
        let Some(i) = self.fx_names.iter().position(|n| n == name) else { return false };
        match act {
            crate::stockmap::Act::Switch(on) => self.fx_off[i] = !on,
            crate::stockmap::Act::Param(id, v) => {
                self.fx[i].set(id, v);
            }
        }
        true
    }

    /// Whether the effect at `i` is in the chain (a stock switch can take it out; a package always starts with every effect in).
    pub fn fx_on(&self, i: usize) -> bool {
        self.fx_off.get(i).is_some_and(|o| !o)
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
        // the stock effect controllers: on the effect channel they are the six effects, even when the notes are omni (so the channel also plays, as on the FM-1)
        if self.status & 0xF0 == 0xB0
            && self.globals.get(G_STOCK_CC) == Some(1)
            && self.globals.get(G_FX_CHANNEL).is_some_and(|c| c != 0 && c == i32::from(self.status & 0x0F) + 1)
            && self.stock_cc(d0, d1)
        {
            return;
        }
        match self.globals.route(self.status & 0x0F) {
            Route::None => return,
            // the effect channel: controllers through the map, nothing that plays or selects
            Route::Controllers => {
                if self.status & 0xF0 == 0xB0 && d0 != 120 && d0 != 123 {
                    self.cc(d0, d1);
                }
                return;
            }
            Route::All => {}
        }
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
                // Bank Select picks a bank of 32 voices, keeping the slot, unless the package maps controller 0 to something
                0 if !self.map.iter().any(|m| m.cc == 0) => {
                    self.set_bank(usize::from(d1));
                }
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
            for (f, off) in self.fx.iter_mut().zip(&self.fx_off) {
                if !*off {
                    f.process(chunk);
                }
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
        for d in self.globals.params() {
            let v = self.globals.get(d.id).unwrap_or(d.default);
            if v != d.default {
                p.globals.push((d.id, v));
            }
        }
        for (b, n) in self.bank_names.iter().enumerate() {
            if !n.is_empty() {
                p.banks.push((b as u8, n.clone()));
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

/// The built-in voices as the package file the app ships (`bundled-firmware/instruments/factory-voices.fm1i`): the project's own original voices, MIT, with
/// no firmware behind them. `core/examples/factory_file.rs` prints it; a test keeps the shipped file equal to it.
pub fn factory_file_text() -> String {
    let mut p = factory_package();
    p.source = "FM1-NTT core/src/factory.rs (original voices written for this project)".into();
    p.to_text()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{FxEntry, MapEntry};

    #[test]
    fn the_shipped_factory_file_is_what_the_code_writes() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bundled-firmware/instruments/factory-voices.fm1i");
        let on_disk = std::fs::read_to_string(path).expect("bundled-firmware/instruments/factory-voices.fm1i: run `cargo run --example factory_file > ...`");
        assert_eq!(on_disk, factory_file_text());
        let pkg = Package::parse(&on_disk).unwrap();
        assert_eq!(pkg.voices.len(), 8);
        assert!(pkg.tier().is_ok());
    }

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

#[cfg(test)]
mod tests_globals {
    use super::*;
    use crate::globals::{G_CHANNEL, G_FX_CHANNEL, G_GLIDE, G_LEGATO, G_STOCK_CC, G_VELOCITY};

    fn none(_: &str) -> Option<Vec<u8>> {
        None
    }
    fn analog() -> Instrument {
        let mut p = Package::new("T", "analog", "MIT");
        p.params = vec![(crate::analog::P_RELEASE, 1), (crate::analog::P_ATTACK, 1), (crate::analog::P_WAVE, 3)]; // a sine: one rising crossing a cycle
        Instrument::from_package(&p, 44_100.0, &none).unwrap()
    }
    fn run(i: &mut Instrument, n: usize) -> Vec<i16> {
        let mut o = vec![0i16; n];
        i.render(&mut o);
        o
    }
    fn peak(x: &[i16]) -> i32 {
        x.iter().map(|&s| i32::from(s).abs()).max().unwrap_or(0)
    }
    /// zero crossings a second of a mono signal, by counting rising ones
    fn freq(x: &[i16], rate: f64) -> f64 {
        let n = x.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
        n as f64 * rate / x.len() as f64
    }

    #[test]
    fn channels_route_notes_and_controllers() {
        let mut i = analog();
        i.set_global(G_CHANNEL, 1);
        i.midi_bytes(&[0x91, 60, 100]); // channel 2 is the effect channel: no note
        i.midi_bytes(&[0x92, 60, 100]); // channel 3 is nothing
        run(&mut i, 512);
        assert_eq!(i.sounding(), 0);
        i.midi_bytes(&[0x90, 60, 100]);
        run(&mut i, 512);
        assert_eq!(i.sounding(), 1);

        let mut p = i.to_package("", "");
        p.map = vec![crate::package::MapEntry { cc: 74, target: Target::Engine(crate::analog::P_CUTOFF), min: 0, max: 100 }];
        let mut j = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        j.midi_bytes(&[0xB2, 74, 127]); // another channel: ignored
        assert_ne!(j.get_param(crate::analog::P_CUTOFF), Some(100));
        j.midi_bytes(&[0xB1, 74, 127]); // the effect channel: the map answers
        assert_eq!(j.get_param(crate::analog::P_CUTOFF), Some(100));
        j.set_global(G_FX_CHANNEL, 0);
        j.midi_bytes(&[0xB1, 74, 0]);
        assert_eq!(j.get_param(crate::analog::P_CUTOFF), Some(100), "no effect channel: nothing on it");
        j.midi_bytes(&[0xB0, 74, 0]);
        assert_eq!(j.get_param(crate::analog::P_CUTOFF), Some(0));
    }

    #[test]
    fn fixed_velocity_is_louder_than_a_soft_touch() {
        let (mut a, mut b) = (analog(), analog());
        b.set_global(G_VELOCITY, 127);
        a.midi_bytes(&[0x90, 60, 10]);
        b.midi_bytes(&[0x90, 60, 10]);
        assert!(peak(&run(&mut b, 4096)) > peak(&run(&mut a, 4096)) * 2);
    }

    #[test]
    fn mono_plays_one_note_and_returns_to_the_one_before() {
        let mut poly = analog();
        poly.midi_bytes(&[0x90, 60, 100, 64, 100]);
        run(&mut poly, 2048);
        assert_eq!(poly.sounding(), 2);

        let mut m = analog();
        m.set_global(crate::globals::G_MONO, 1);
        m.midi_bytes(&[0x90, 60, 100, 64, 100]);
        run(&mut m, 2048);
        assert_eq!(m.sounding(), 1, "the 60 let go when the 64 came");
        m.midi_bytes(&[0x80, 64, 0]);
        let back = run(&mut m, 8192);
        assert_eq!(m.sounding(), 1, "the 60 is struck again");
        let hz = freq(&back[2048..], 44_100.0);
        assert!((hz - 261.6).abs() < 12.0, "the held C4 sounds again, got {hz} Hz");
        m.midi_bytes(&[0x80, 60, 0]);
        run(&mut m, 2048);
        assert_eq!(m.sounding(), 0);
        // letting go of a note under the top one changes nothing
        m.midi_bytes(&[0x90, 60, 100, 64, 100, 0x80, 60, 0]);
        run(&mut m, 2048);
        assert_eq!(m.sounding(), 1);
        m.midi_bytes(&[0x80, 64, 0]);
        run(&mut m, 2048);
        assert_eq!(m.sounding(), 0);
    }

    #[test]
    fn bend_range_sets_the_wheel() {
        let mut a = analog();
        a.midi_bytes(&[0x90, 57, 100, 0xE0, 127, 127]); // A3 (220 Hz), the wheel up
        let base = freq(&run(&mut a, 44_100)[2000..], 44_100.0);
        let mut b = analog();
        b.set_global(G_BEND, 12);
        b.midi_bytes(&[0x90, 57, 100, 0xE0, 127, 127]);
        let wide = freq(&run(&mut b, 44_100)[2000..], 44_100.0);
        let ratio_a = base / 220.0;
        let ratio_b = wide / 220.0;
        assert!((ratio_a - 2f64.powf(2.0 / 12.0)).abs() < 0.03, "default: two semitones, got {ratio_a}");
        assert!((ratio_b - 2.0).abs() < 0.04, "twelve semitones is an octave, got {ratio_b}");
        let mut c = analog();
        c.set_global(G_BEND, 0);
        c.midi_bytes(&[0x90, 57, 100, 0xE0, 127, 127]);
        let none_bent = freq(&run(&mut c, 44_100)[2000..], 44_100.0) / 220.0;
        assert!((none_bent - 1.0).abs() < 0.02, "range 0 turns the wheel off, got {none_bent}");
    }

    #[test]
    fn fm6_takes_a_bend_range() {
        let mut i = Instrument::from_package(&factory_package(), 44_100.0, &none).unwrap();
        assert!(i.set_global(G_BEND, 24) && i.set_global(G_BEND, 2));
        assert_eq!(i.get_global(G_BEND), Some(2));
    }

    #[test]
    fn effect_chain_reorders_and_the_map_follows() {
        let mut p = Package::new("T", "analog", "MIT");
        p.fx = ["delay", "chorus", "reverb"].iter().map(|n| crate::package::FxEntry { name: (*n).into(), params: Vec::new() }).collect();
        p.map = vec![
            crate::package::MapEntry { cc: 1, target: Target::Fx(0, 0), min: 0, max: 10 },
            crate::package::MapEntry { cc: 2, target: Target::Fx(1, 0), min: 0, max: 10 },
            crate::package::MapEntry { cc: 3, target: Target::Fx(2, 0), min: 0, max: 10 },
        ];
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        assert!(i.move_fx(0, 2));
        assert_eq!((i.fx_name(0), i.fx_name(1), i.fx_name(2)), (Some("chorus"), Some("reverb"), Some("delay")));
        let t: Vec<Target> = i.to_package("", "").map.iter().map(|m| m.target).collect();
        assert_eq!(t, vec![Target::Fx(2, 0), Target::Fx(0, 0), Target::Fx(1, 0)], "each controller still reaches the effect it did");
        assert!(i.move_fx(2, 0));
        assert_eq!(i.fx_name(0), Some("delay"));
        assert!(!i.move_fx(0, 3) && !i.move_fx(5, 0));
        let back = Instrument::from_package(&i.to_package("", ""), 44_100.0, &none).unwrap();
        assert_eq!((back.fx_name(0), back.fx_name(1), back.fx_name(2)), (Some("delay"), Some("chorus"), Some("reverb")));
    }

    #[test]
    fn banks_and_bank_select() {
        let mut p = factory_package();
        let base = p.voices[0];
        p.voices = vec![base; 70];
        for (k, v) in p.voices.iter_mut().enumerate() {
            v[0] = (k % 100) as u8; // distinguishable
        }
        p.banks = vec![(1, "PADS".into())];
        let mut i = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        assert_eq!((i.bank_count(), i.bank(), i.bank_name(0), i.bank_name(1)), (3, 0, "BANK 1".to_string(), "PADS".to_string()));
        i.set_program(5);
        assert!(i.set_bank(1));
        assert_eq!((i.program(), i.bank()), (37, 1), "the same slot in the next bank");
        i.midi_bytes(&[0xB0, 0, 2]); // Bank Select
        assert_eq!(i.program(), 69, "bank 3 has six voices: the last there is");
        i.midi_bytes(&[0xB0, 0, 9]);
        assert_eq!(i.program(), 69, "no such bank: nothing happens");
        assert!(!i.set_bank(3));
        i.midi_bytes(&[0xC0, 3]); // Program Change is absolute
        assert_eq!(i.program(), 3);
        assert!(i.set_bank_name(0, "LEADS") && !i.set_bank_name(8, "X") && !i.set_bank_name(0, &"x".repeat(33)));
    }

    #[test]
    fn globals_and_banks_survive_a_package() {
        let mut i = analog();
        i.set_global(G_CHANNEL, 5);
        i.set_global(crate::globals::G_MONO, 1);
        i.set_global(G_BEND, 7);
        let text = i.to_package("", "").to_text();
        assert!(text.starts_with("fm1i = 2\n") && text.contains("global 0 = 5"));
        let back = Instrument::from_package(&Package::parse(&text).unwrap(), 44_100.0, &none).unwrap();
        assert_eq!((back.get_global(G_CHANNEL), back.get_global(crate::globals::G_MONO), back.get_global(G_BEND)), (Some(5), Some(1), Some(7)));
        let mut p = Package::new("T", "analog", "MIT");
        p.globals = vec![(99, 1)];
        assert!(Instrument::from_package(&p, 44_100.0, &none).is_err());
    }

    fn stock_chain() -> Instrument {
        let mut p = Package::new("T", "analog", "MIT");
        p.fx = ["delay", "filter", "reverb", "distortion", "chorus", "phaser"].iter().map(|n| crate::package::FxEntry { name: (*n).into(), params: Vec::new() }).collect();
        // a package controller that the stock mode must not shadow off the effect channel
        p.map = vec![crate::package::MapEntry { cc: 1, target: Target::Engine(crate::analog::P_CUTOFF), min: 0, max: 100 }];
        Instrument::from_package(&p, 44_100.0, &none).unwrap()
    }

    #[test]
    fn stock_effect_controllers_on_the_effect_channel() {
        let mut i = stock_chain();
        let f = 1; // the filter's place
        // off by default: the effect channel's CC 2 reaches nothing mapped
        i.midi_bytes(&[0xB1, 2, 100]);
        assert_eq!(i.get_fx_param(f, 1), Some(8000), "stock mode is off");
        i.set_global(G_STOCK_CC, 1);
        i.midi_bytes(&[0xB1, 2, 100, 0xB1, 3, 5, 0xB1, 1, 2]); // cutoff, Q, type: running status
        assert_eq!((i.get_fx_param(f, 1), i.get_fx_param(f, 2), i.get_fx_param(f, 0)), (Some(20_000), Some(50), Some(2)));
        // by name, so the chain's order does not matter
        assert!(i.move_fx(1, 5));
        assert_eq!(i.fx_name(5), Some("filter"));
        i.midi_bytes(&[0xB1, 2, 0]);
        assert_eq!(i.get_fx_param(5, 1), Some(100));
        // the reverb's mix and the delay's time (a faster rate, a shorter time)
        i.midi_bytes(&[0xB1, 7, 40, 0xB1, 10, 100]);
        let (r, d) = ((0..6).find(|&k| i.fx_name(k) == Some("reverb")).unwrap(), (0..6).find(|&k| i.fx_name(k) == Some("delay")).unwrap());
        assert_eq!((i.get_fx_param(r, 2), i.get_fx_param(d, 0)), (Some(40), Some(50)));
    }

    #[test]
    fn stock_switches_take_an_effect_out_and_back_and_follow_a_move() {
        let mut i = stock_chain();
        i.set_global(G_STOCK_CC, 1);
        assert!((0..6).all(|k| i.fx_on(k)) && !i.fx_on(9));
        i.midi_bytes(&[0xB1, 12, 0]); // distortion off
        let k = (0..6).find(|&k| i.fx_name(k) == Some("distortion")).unwrap();
        assert!(!i.fx_on(k));
        assert!(i.move_fx(k, 0));
        assert!(!i.fx_on(0) && i.fx_on(k), "the switch moved with the effect");
        i.midi_bytes(&[0xB1, 12, 1]);
        assert!(i.fx_on(0));
        // it is audible: a distortion in the chain changes the signal, out it does not
        let mut a = stock_chain();
        let mut b = stock_chain();
        for m in [&mut a, &mut b] {
            m.set_global(G_STOCK_CC, 1);
            m.set_fx_param(3, 0, 100);
            m.set_fx_param(3, 3, 100);
            m.note_on(60, 100);
        }
        b.midi_bytes(&[0xB1, 12, 0]);
        let (x, y) = (run(&mut a, 2048), run(&mut b, 2048));
        assert_ne!(x, y, "a switched-out distortion is not applied");
    }

    #[test]
    fn stock_controllers_leave_the_note_channel_alone() {
        let mut i = stock_chain();
        i.set_global(G_STOCK_CC, 1);
        // CC 1 on the note channel is the mod wheel (the package's map), not the filter type
        i.midi_bytes(&[0xB0, 1, 127]);
        assert_eq!(i.get_fx_param(1, 0), Some(0), "the filter type did not move");
        // with the notes on channel 1 the effect channel 2 still reaches the effects, and a note on it does not play
        i.set_global(G_CHANNEL, 1);
        i.midi_bytes(&[0xB1, 1, 1]);
        assert_eq!(i.get_fx_param(1, 0), Some(1));
        i.midi_bytes(&[0x91, 60, 100]);
        assert_eq!(i.sounding(), 0);
        // omni notes: channel 2 plays too, and its controllers are still the effects
        i.set_global(G_CHANNEL, 0);
        i.midi_bytes(&[0x91, 60, 100, 0xB1, 1, 2]);
        assert_eq!((i.sounding(), i.get_fx_param(1, 0)), (1, Some(2)));
        // a controller the stock table does not have falls through to the package's map on the effect channel
        i.set_global(G_CHANNEL, 1);
        i.midi_bytes(&[0xB1, 40, 1]);
        // an effect the chain lacks: nothing happens and nothing breaks
        let mut p = Package::new("T", "analog", "MIT");
        p.fx = vec![crate::package::FxEntry { name: "delay".into(), params: Vec::new() }];
        let mut j = Instrument::from_package(&p, 44_100.0, &none).unwrap();
        j.set_global(G_STOCK_CC, 1);
        j.midi_bytes(&[0xB1, 2, 50, 0xB1, 0, 0, 0xB1, 11, 70]);
        assert_eq!(j.get_fx_param(0, 2), Some(70));
    }

    #[test]
    fn glide_is_a_setting_that_reaches_the_engine_and_the_package() {
        let mut i = analog();
        assert_eq!(i.get_global(G_GLIDE), Some(0));
        assert!(i.set_global(G_GLIDE, 250) && i.get_global(G_GLIDE) == Some(250));
        assert!(i.set_global(G_GLIDE, 99_999) && i.get_global(G_GLIDE) == Some(2000), "clamped");
        i.set_global(G_GLIDE, 400);
        let back = Instrument::from_package(&Package::parse(&i.to_package("", "").to_text()).unwrap(), 44_100.0, &none).unwrap();
        assert_eq!(back.get_global(G_GLIDE), Some(400));
        // it reaches the engine: with glide the second note does not start at its own pitch
        let count = |glide: i32| {
            let mut m = analog();
            m.set_global(G_GLIDE, glide);
            m.note_on(45, 100);
            run(&mut m, 2000);
            m.note_off(45);
            run(&mut m, 2000);
            m.note_on(81, 100);
            let x = run(&mut m, 4096);
            x.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count()
        };
        assert!(count(600) + 15 < count(0), "{} against {}: a slide upward has fewer cycles", count(600), count(0));
    }

    #[test]
    fn the_stock_setting_survives_a_package() {
        let mut i = stock_chain();
        i.set_global(G_STOCK_CC, 1);
        let back = Instrument::from_package(&i.to_package("", ""), 44_100.0, &none).unwrap();
        assert_eq!(back.get_global(G_STOCK_CC), Some(1));
    }

    #[test]
    fn legato_keeps_the_voice_and_its_envelope() {
        let mut m = analog();
        m.set_global(crate::globals::G_MONO, 1);
        m.set_global(G_LEGATO, 1);
        m.midi_bytes(&[0x90, 60, 100]);
        run(&mut m, 44_100); // the first note settles into its sustain
        m.midi_bytes(&[0x90, 67, 100]);
        let moved = run(&mut m, 8192);
        assert_eq!(m.sounding(), 1, "one voice, not a new strike");
        let hz = freq(&moved[4096..], 44_100.0);
        assert!((hz - 392.0).abs() < 14.0, "the G4 sounds, got {hz} Hz");
        // letting go of the top note returns to the held one, still one voice
        m.midi_bytes(&[0x80, 67, 0]);
        let back = run(&mut m, 8192);
        assert_eq!(m.sounding(), 1);
        assert!((freq(&back[4096..], 44_100.0) - 261.6).abs() < 12.0);
        m.midi_bytes(&[0x80, 60, 0]);
        run(&mut m, 44_100);
        assert_eq!(m.sounding(), 0);
        // without legato the same moves restrike: the attack shows again
        let peak_first = |legato: i32| {
            let mut m = analog();
            m.set_global(crate::globals::G_MONO, 1);
            m.set_global(G_LEGATO, legato);
            m.midi_bytes(&[0x90, 60, 100]);
            run(&mut m, 44_100);
            m.midi_bytes(&[0x90, 67, 100]);
            let r = run(&mut m, 64);
            r.iter().map(|&v| i32::from(v).abs()).max().unwrap_or(0)
        };
        assert_ne!(peak_first(0), peak_first(1), "the two paths differ");
    }
}
