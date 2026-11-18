//! The step sequencer: 16 patterns of 16 steps, each pattern with its own voice, a chain of 1 to 16 patterns, a step count of 1 to 16, tempo, gate, swing, transpose and
//! recording by playing. Written fresh from the FM-1 manual's description (docs/HARDWARE.md), never from any firmware's source; like the arpeggiator (`arp.rs`) it is a host
//! feature in front of any engine, pure timing and note choice on the caller's sample clock, deterministic, bounded and silent while off.
//!
//! **A step's note is 0 for a rest and 1 to 127 otherwise** (note 0, C-1, is not available to a step). Events come out as `arp::Event` (a pattern's voice is a `Program`).
//! The arpeggiator and the sequencer exclude each other, as on the FM-1 ("Disable SEQ to enter"): the instrument host enforces it.

use crate::arp::Event;
use crate::engine::ParamDesc;

pub const PATTERNS: usize = 16;
pub const STEPS: usize = 16;

pub const P_ON: u16 = 0;
pub const P_BPM: u16 = 1;
pub const P_RATE: u16 = 2;
pub const P_GATE: u16 = 3;
pub const P_SWING: u16 = 4;
pub const P_STEPS: u16 = 5;
pub const P_CHAIN: u16 = 6;
pub const P_PATTERN: u16 = 7;
pub const P_TRANSPOSE: u16 = 8;
pub const P_RECORD: u16 = 9;
pub const P_COUNT: usize = 10;
/// The pattern now being edited (`P_PATTERN`): its voice, then each step's note and velocity.
pub const ID_VOICE: u16 = 100;
pub const ID_STEP: u16 = 101;
/// The highest id of a step parameter.
pub const ID_LAST: u16 = ID_STEP + (STEPS as u16) * 2 - 1;

/// name, min, max, default, unit
const DESC: [(&str, i32, i32, i32, &str); P_COUNT] = [
    ("PLAYING", 0, 1, 0, ""),
    ("BPM", 30, 300, 120, ""),
    ("RATE (0 1/4 1 1/8 2 1/16 3 1/32 4 1/4T 5 1/8T 6 1/16T)", 0, 6, 2, ""),
    ("GATE", 5, 100, 60, "%"),
    ("SWING", 0, 50, 0, "%"),
    ("STEPS (1-16)", 1, 16, 16, ""),
    ("CHAIN (PATTERNS)", 1, 16, 1, ""),
    ("EDIT PATTERN", 1, 16, 1, ""),
    ("TRANSPOSE", -24, 24, 0, ""),
    ("RECORD (BY PLAYING)", 0, 1, 0, ""),
];

const STEPS_PER_BEAT: [f64; 7] = [1.0, 2.0, 4.0, 8.0, 1.5, 3.0, 6.0];

pub struct Seq {
    rate: f64,
    p: [i32; P_COUNT],
    notes: [[u8; STEPS]; PATTERNS],
    vels: [[u8; STEPS]; PATTERNS],
    /// the voice (program) a pattern selects when it starts, -1 for none
    voice: [i32; PATTERNS],
    running: bool,
    /// where in the chain and in the pattern the next step is
    chain_at: usize,
    step_at: usize,
    to_next: f64,
    sounding: Option<u8>,
    to_off: f64,
    pending: Vec<Event>,
}

impl Seq {
    pub fn new(rate: f64) -> Seq {
        let mut p = [0; P_COUNT];
        for (i, d) in DESC.iter().enumerate() {
            p[i] = d.3;
        }
        Seq { rate: rate.clamp(8000.0, 192_000.0), p, notes: [[0; STEPS]; PATTERNS], vels: [[100; STEPS]; PATTERNS], voice: [-1; PATTERNS], running: false, chain_at: 0, step_at: 0, to_next: 0.0, sounding: None, to_off: f64::MAX, pending: Vec::new() }
    }

    /// The scalar settings (group 0) or the edited pattern's voice and steps (group 1).
    pub fn params(&self, group: usize) -> Vec<ParamDesc> {
        let pd = |id: u16, name: String, min: i32, max: i32, default: i32| ParamDesc { id, name, min, max, default, unit: "" };
        if group == 0 {
            return DESC.iter().enumerate().map(|(i, d)| ParamDesc { id: i as u16, name: d.0.to_string(), min: d.1, max: d.2, default: d.3, unit: d.4 }).collect();
        }
        let mut v = vec![pd(ID_VOICE, "VOICE (-1 KEEPS THE ONE SET)".into(), -1, 255, -1)];
        for s in 0..STEPS {
            v.push(pd(ID_STEP + 2 * s as u16, format!("STEP {} NOTE (0 REST)", s + 1), 0, 127, 0));
            v.push(pd(ID_STEP + 2 * s as u16 + 1, format!("STEP {} VELOCITY", s + 1), 1, 127, 100));
        }
        v
    }

    fn pat(&self) -> usize {
        (self.p[P_PATTERN as usize] as usize - 1).min(PATTERNS - 1)
    }

    pub fn get(&self, id: u16) -> Option<i32> {
        if (id as usize) < P_COUNT {
            return Some(self.p[id as usize]);
        }
        let pat = self.pat();
        match id {
            ID_VOICE => Some(self.voice[pat]),
            ID_STEP..=ID_LAST => {
                let k = (id - ID_STEP) as usize;
                Some(if k % 2 == 0 { i32::from(self.notes[pat][k / 2]) } else { i32::from(self.vels[pat][k / 2]) })
            }
            _ => None,
        }
    }

    pub fn set(&mut self, id: u16, value: i32) -> bool {
        if (id as usize) < P_COUNT {
            let d = DESC[id as usize];
            let v = value.clamp(d.1, d.2);
            let before = self.p[id as usize];
            self.p[id as usize] = v;
            match id {
                P_ON if v == 1 && before == 0 => self.start(),
                P_ON if v == 0 && before == 1 => self.stop(),
                P_CHAIN if self.chain_at >= v as usize => self.chain_at = 0,
                P_STEPS if self.step_at >= v as usize => self.step_at = 0,
                _ => {}
            }
            return true;
        }
        let pat = self.pat();
        match id {
            ID_VOICE => self.voice[pat] = value.clamp(-1, 255),
            ID_STEP..=ID_LAST => {
                let k = (id - ID_STEP) as usize;
                if k % 2 == 0 {
                    self.notes[pat][k / 2] = value.clamp(0, 127) as u8;
                } else {
                    self.vels[pat][k / 2] = value.clamp(1, 127) as u8;
                }
            }
            _ => return false,
        }
        true
    }

    /// A step by pattern (0 to 15) and step (0 to 15): note (0 a rest) and velocity.
    pub fn step(&self, pattern: usize, step: usize) -> Option<(u8, u8)> {
        (pattern < PATTERNS && step < STEPS).then(|| (self.notes[pattern][step], self.vels[pattern][step]))
    }

    pub fn set_step(&mut self, pattern: usize, step: usize, note: u8, velocity: u8) -> bool {
        if pattern >= PATTERNS || step >= STEPS {
            return false;
        }
        self.notes[pattern][step] = note.min(127);
        self.vels[pattern][step] = velocity.clamp(1, 127);
        true
    }

    pub fn voice(&self, pattern: usize) -> Option<i32> {
        self.voice.get(pattern).copied()
    }

    pub fn set_voice(&mut self, pattern: usize, program: i32) -> bool {
        match self.voice.get_mut(pattern) {
            Some(v) => {
                *v = program.clamp(-1, 255);
                true
            }
            None => false,
        }
    }

    pub fn on(&self) -> bool {
        self.p[P_ON as usize] == 1
    }

    pub fn recording(&self) -> bool {
        self.on() && self.p[P_RECORD as usize] == 1
    }

    fn start(&mut self) {
        self.running = true;
        self.chain_at = 0;
        self.step_at = 0;
        self.to_next = 0.0;
    }

    fn stop(&mut self) {
        if let Some(n) = self.sounding.take() {
            self.pending.push(Event::Off(n));
        }
        self.to_off = f64::MAX;
        self.running = false;
    }

    /// Stops and forgets nothing but the position (the patterns are the person's).
    pub fn all_off(&mut self) {
        self.p[P_ON as usize] = 0;
        self.stop();
    }

    fn step_len(&self) -> f64 {
        60.0 / self.p[P_BPM as usize] as f64 * self.rate / STEPS_PER_BEAT[self.p[P_RATE as usize] as usize]
    }

    /// A note played while recording goes into the step it is nearest to: the step now sounding in its first half, the next one after that (a note a little early
    /// is the next step's). Into the pattern now playing. Returns the step written, or `None` when not recording.
    pub fn record_note(&mut self, note: u8, velocity: u8) -> Option<usize> {
        if !self.recording() || !self.running || note == 0 {
            return None;
        }
        let len = self.step_len();
        let steps = self.p[P_STEPS as usize] as usize;
        // `step_at` is the next step to play; the one sounding is the one before it
        let now = (self.step_at + steps - 1) % steps;
        let into = if self.to_next < len / 2.0 { self.step_at } else { now };
        let pat = self.chain_pattern();
        self.notes[pat][into] = note.min(127);
        self.vels[pat][into] = velocity.clamp(1, 127);
        Some(into)
    }

    fn chain_pattern(&self) -> usize {
        // the pattern whose step `step_at - 1` just played is the chain's current one, until the chain moves on at the end of its last step
        self.chain_at.min(PATTERNS - 1)
    }

    /// Advances `frames` of audio and appends the events that fall in them as `(frame, event)` in order.
    pub fn advance(&mut self, frames: usize, out: &mut Vec<(usize, Event)>) {
        for e in self.pending.drain(..) {
            out.push((0, e));
        }
        if !self.on() || !self.running {
            return;
        }
        let step_len = self.step_len();
        let gate = self.p[P_GATE as usize] as f64 / 100.0;
        let swing = self.p[P_SWING as usize] as f64 / 100.0;
        let steps = self.p[P_STEPS as usize] as usize;
        let chain = self.p[P_CHAIN as usize] as usize;
        let transpose = self.p[P_TRANSPOSE as usize];
        let end = frames as f64;
        let mut t = 0.0f64;
        loop {
            let next_off = if self.sounding.is_some() { self.to_off } else { f64::MAX };
            let next_on = self.to_next;
            let dt = next_on.min(next_off);
            if t + dt >= end {
                self.to_next -= end - t;
                self.to_off -= end - t;
                break;
            }
            t += dt;
            self.to_next -= dt;
            self.to_off -= dt;
            let at = (t as usize).min(frames.saturating_sub(1));
            if next_off <= next_on {
                if let Some(n) = self.sounding.take() {
                    out.push((at, Event::Off(n)));
                }
                self.to_off = f64::MAX;
                continue;
            }
            // a step begins
            if let Some(n) = self.sounding.take() {
                out.push((at, Event::Off(n)));
            }
            let pat = self.chain_at.min(PATTERNS - 1);
            if self.step_at == 0 && self.voice[pat] >= 0 {
                out.push((at, Event::Program(self.voice[pat] as u8)));
            }
            let note = self.notes[pat][self.step_at];
            if note != 0 {
                let played = (i32::from(note) + transpose).clamp(1, 127) as u8;
                out.push((at, Event::On(played, self.vels[pat][self.step_at])));
                self.sounding = Some(played);
                self.to_off = (step_len * gate).max(1.0);
            }
            let k = self.step_at;
            self.step_at += 1;
            if self.step_at >= steps {
                self.step_at = 0;
                self.chain_at = (self.chain_at + 1) % chain;
            }
            self.to_next += if k % 2 == 0 { step_len * (1.0 + swing) } else { step_len * (1.0 - swing) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Seq, frames: usize, block: usize) -> Vec<(usize, Event)> {
        let mut all = Vec::new();
        let mut at = 0;
        while at < frames {
            let n = block.min(frames - at);
            let mut ev = Vec::new();
            s.advance(n, &mut ev);
            all.extend(ev.into_iter().map(|(f, e)| (at + f, e)));
            at += n;
        }
        all
    }

    fn ons(ev: &[(usize, Event)]) -> Vec<u8> {
        ev.iter().filter_map(|(_, e)| if let Event::On(n, _) = e { Some(*n) } else { None }).collect()
    }

    /// A 4-step pattern C E G rest at 120 BPM, 1/16 steps (5512.5 frames).
    fn four(s: &mut Seq) {
        s.set(P_STEPS, 4);
        s.set_step(0, 0, 60, 100);
        s.set_step(0, 1, 64, 90);
        s.set_step(0, 2, 67, 80);
        s.set(P_BPM, 120);
        s.set(P_RATE, 2);
    }

    #[test]
    fn off_by_default_and_silent() {
        let mut s = Seq::new(44_100.0);
        assert!(run(&mut s, 44_100, 512).is_empty());
        assert!(!s.on() && s.get(P_PATTERN as u16) == Some(1));
    }

    #[test]
    fn plays_its_steps_in_order_rests_and_loops() {
        let mut s = Seq::new(44_100.0);
        four(&mut s);
        s.set(P_ON, 1);
        let ev = run(&mut s, 5513 * 9, 512);
        assert_eq!(ons(&ev), vec![60, 64, 67, 60, 64, 67, 60, 64, 67][..ons(&ev).len()], "a rest plays nothing and the pattern loops");
        let first: Vec<usize> = ev.iter().filter(|(_, e)| matches!(e, Event::On(..))).map(|(f, _)| *f).collect();
        assert_eq!(first[..3], [0, 5512, 11_025], "steps fall on the sixteenth grid");
        let velocities: Vec<u8> = ev.iter().filter_map(|(_, e)| if let Event::On(_, v) = e { Some(*v) } else { None }).take(3).collect();
        assert_eq!(velocities, vec![100, 90, 80]);
    }

    #[test]
    fn the_block_size_changes_nothing_and_gate_ends_notes() {
        let go = |block: usize| {
            let mut s = Seq::new(44_100.0);
            four(&mut s);
            s.set(P_GATE, 50);
            s.set(P_ON, 1);
            run(&mut s, 44_100, block)
        };
        let a = go(700);
        assert_eq!(a, go(64));
        let offs: Vec<usize> = a.iter().filter(|(_, e)| matches!(e, Event::Off(..))).map(|(f, _)| *f).collect();
        assert_eq!(offs[0], 2_756, "a 50 % gate of a 5512.5 frame step");
    }

    #[test]
    fn transpose_gate_swing_and_the_step_count() {
        let mut s = Seq::new(44_100.0);
        four(&mut s);
        s.set(P_TRANSPOSE, 12);
        s.set(P_STEPS, 2);
        s.set(P_ON, 1);
        let ev = run(&mut s, 5513 * 4, 512);
        assert_eq!(ons(&ev)[..4], [72, 76, 72, 76], "two steps, an octave up");
        let mut lo = Seq::new(44_100.0);
        four(&mut lo);
        lo.set(P_TRANSPOSE, -24);
        lo.set_step(0, 0, 20, 100);
        lo.set(P_ON, 1);
        assert_eq!(ons(&run(&mut lo, 100, 100)), vec![1], "transposed past the bottom is clamped to note 1, not wrapped");
        let mut w = Seq::new(44_100.0);
        four(&mut w);
        w.set(P_SWING, 25);
        w.set(P_ON, 1);
        let on: Vec<usize> = run(&mut w, 5513 * 4, 512).iter().filter(|(_, e)| matches!(e, Event::On(..))).map(|(f, _)| *f).collect();
        assert_eq!(on[0], 0);
        assert!((on[1] as i64 - 6_891).abs() <= 1, "the second step is late by a quarter: {on:?}");
        assert!((on[2] as i64 - 11_025).abs() <= 1, "the pair keeps its length: {on:?}");
    }

    #[test]
    fn a_chain_plays_patterns_in_order_and_each_selects_its_voice() {
        let mut s = Seq::new(44_100.0);
        s.set(P_STEPS, 2);
        s.set(P_CHAIN, 3);
        s.set_step(0, 0, 60, 100);
        s.set_step(1, 0, 62, 100);
        s.set_step(2, 0, 64, 100);
        s.set_voice(1, 7);
        s.set_voice(2, 12);
        s.set(P_ON, 1);
        let ev = run(&mut s, 5513 * 14, 512);
        let seq: Vec<Event> = ev.iter().map(|(_, e)| *e).filter(|e| !matches!(e, Event::Off(_))).collect();
        assert_eq!(seq[..6], [Event::On(60, 100), Event::Program(7), Event::On(62, 100), Event::Program(12), Event::On(64, 100), Event::On(60, 100)], "pattern 1, 2 (voice 7), 3 (voice 12), then round again");
        assert!(!ev.iter().any(|(_, e)| *e == Event::Program(0)), "a pattern with no voice set changes nothing");
    }

    #[test]
    fn stopping_releases_the_note_and_recording_by_playing_lands_on_the_nearest_step() {
        let mut s = Seq::new(44_100.0);
        four(&mut s);
        s.set(P_ON, 1);
        let _ = run(&mut s, 1000, 512);
        s.set(P_ON, 0);
        let ev = run(&mut s, 512, 512);
        assert_eq!(ev, vec![(0, Event::Off(60))], "stopping ends the sounding note and plays nothing more");
        assert!(run(&mut s, 44_100, 512).is_empty());
        // recording: needs running and the record flag
        let mut r = Seq::new(44_100.0);
        four(&mut r);
        r.set(P_ON, 1);
        assert_eq!(r.record_note(70, 100), None, "not armed");
        r.set(P_RECORD, 1);
        let _ = run(&mut r, 1000, 512); // inside step 1 (the first half): its own step
        assert_eq!(r.record_note(70, 99), Some(0));
        assert_eq!(r.step(0, 0), Some((70, 99)));
        let _ = run(&mut r, 3500, 512); // 4500 frames in: the second half of step 1, so the note is for step 2
        assert_eq!(r.record_note(71, 98), Some(1));
        assert_eq!(r.step(0, 1), Some((71, 98)));
        let again = run(&mut r, 5513 * 4, 512);
        assert!(ons(&again).contains(&71) && ons(&again).contains(&70), "what was recorded plays");
    }

    #[test]
    fn ids_clamp_edit_the_chosen_pattern_and_are_bounded() {
        let mut s = Seq::new(44_100.0);
        assert!(s.set(P_PATTERN, 3) && s.set(ID_STEP + 4, 62) && s.set(ID_STEP + 5, 999) && s.set(ID_VOICE, 9));
        assert_eq!((s.step(2, 2), s.voice(2), s.get(ID_STEP + 5)), (Some((62, 127)), Some(9), Some(127)));
        assert_eq!(s.step(0, 2), Some((0, 100)), "pattern 1 is untouched");
        assert!(s.set(P_BPM, 9999) && s.get(P_BPM) == Some(300));
        assert!(s.set(P_TRANSPOSE, -999) && s.get(P_TRANSPOSE) == Some(-24));
        assert!(!s.set(55, 1) && s.get(55).is_none() && !s.set(ID_LAST + 1, 1));
        assert!(s.step(16, 0).is_none() && !s.set_step(0, 16, 1, 1) && !s.set_voice(16, 1));
        assert_eq!(s.params(0).len(), P_COUNT);
        assert_eq!(s.params(1).len(), 1 + 2 * STEPS);
        assert!(s.params(0).iter().chain(s.params(1).iter()).all(|d| d.min <= d.default && d.default <= d.max));
        // shrinking the chain or the step count while running keeps the position valid
        let mut r = Seq::new(44_100.0);
        r.set(P_STEPS, 16);
        r.set(P_CHAIN, 16);
        r.set(P_ON, 1);
        let _ = run(&mut r, 44_100, 512);
        r.set(P_STEPS, 1);
        r.set(P_CHAIN, 1);
        let _ = run(&mut r, 44_100, 512);
    }
}
