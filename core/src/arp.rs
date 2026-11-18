//! The arpeggiator: a note sequencer a package can switch on, written fresh from the FM-1's manual (docs/HARDWARE.md: Up, Down, Inclusive, Exclusive, Random,
//! Order, Repeat; one to four octaves; 30 to 300 BPM; gate, swing, latch) and from Mighty Arp's controls (a chance per step), never from any firmware's source.
//!
//! It is pure timing and note choice: the held notes in, `(frame offset, event)` out, on the caller's sample clock, so it is deterministic (the "random" is a
//! small xorshift with a seed parameter) and costs nothing while off. The host (`instrument.rs`) feeds the events to the engine between slices of a block.
//! Parameters are described like an engine's (`ParamDesc`), so an editor and a package reach them the same way.

use crate::engine::ParamDesc;

pub const MAX_HELD: usize = 16;

pub const P_ON: u16 = 0;
pub const P_MODE: u16 = 1;
pub const P_OCTAVES: u16 = 2;
pub const P_BPM: u16 = 3;
pub const P_RATE: u16 = 4;
pub const P_GATE: u16 = 5;
pub const P_SWING: u16 = 6;
pub const P_LATCH: u16 = 7;
pub const P_CHANCE: u16 = 8;
pub const P_SEED: u16 = 9;
/// 0 the arpeggiator's own tempo, 1 follow the MIDI clock (24 ticks a quarter note): a step falls on the tick that starts it, and the tempo, for the gate, is the ticks' spacing.
pub const P_SYNC: u16 = 10;
pub const P_COUNT: usize = 11;

/// name, min, max, default, unit
const DESC: [(&str, i32, i32, i32, &str); P_COUNT] = [
    ("ARP ON", 0, 1, 0, ""),
    ("MODE (0 UP 1 DOWN 2 INCL 3 EXCL 4 RANDOM 5 ORDER 6 REPEAT)", 0, 6, 0, ""),
    ("OCTAVES", 1, 4, 1, ""),
    ("BPM", 30, 300, 120, ""),
    ("RATE (0 1/4 1 1/8 2 1/16 3 1/32 4 1/4T 5 1/8T 6 1/16T)", 0, 6, 1, ""),
    ("GATE", 5, 100, 50, "%"),
    ("SWING", 0, 50, 0, "%"),
    ("LATCH", 0, 1, 0, ""),
    ("CHANCE", 0, 100, 100, "%"),
    ("SEED", 0, 9999, 1, ""),
    ("SYNC (0 INTERNAL 1 MIDI CLOCK)", 0, 1, 0, ""),
];

/// Steps in a beat (a quarter note) for each RATE.
const STEPS_PER_BEAT: [f64; 7] = [1.0, 2.0, 4.0, 8.0, 1.5, 3.0, 6.0];
/// MIDI clock ticks in a step for each RATE (24 ticks a quarter note).
pub const TICKS_PER_STEP: [u64; 7] = [24, 12, 6, 3, 16, 8, 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    On(u8, u8),
    Off(u8),
    /// select a voice (the sequencer's patterns do; the arpeggiator never)
    Program(u8),
}

pub struct Arp {
    rate: f64,
    p: [i32; P_COUNT],
    /// held notes in the order played, with their velocities
    held: Vec<(u8, u8)>,
    /// keys physically down (a latched chord stays in `held` after they are released)
    down: Vec<u8>,
    /// the step counter (0 at the start of a run), the frames until the next step, the note now sounding and the frames until its off
    step: u64,
    to_next: f64,
    sounding: Option<u8>,
    to_off: f64,
    running: bool,
    pending: Vec<Event>,
    rng: u64,
    /// external clock: the ticks counted since the last Start, whether the clock is running (Stop pauses it), and the spacing of the ticks in frames
    tick_n: u64,
    clock_run: bool,
    tick_frames: f64,
}

impl Arp {
    pub fn new(rate: f64) -> Arp {
        let mut p = [0; P_COUNT];
        for (i, d) in DESC.iter().enumerate() {
            p[i] = d.3;
        }
        let mut a = Arp { rate: rate.clamp(8000.0, 192_000.0), p, held: Vec::new(), down: Vec::new(), step: 0, to_next: 0.0, sounding: None, to_off: f64::MAX, running: false, pending: Vec::new(), rng: 0, tick_n: 0, clock_run: true, tick_frames: 0.0 };
        a.reseed();
        a
    }

    pub fn params(&self) -> Vec<ParamDesc> {
        DESC.iter().enumerate().map(|(i, d)| ParamDesc { id: i as u16, name: d.0.to_string(), min: d.1, max: d.2, default: d.3, unit: d.4 }).collect()
    }

    pub fn get(&self, id: u16) -> Option<i32> {
        self.p.get(id as usize).copied()
    }

    /// Sets a parameter (clamped). Turning it off releases the sounding note and forgets the held notes; changing the seed restarts the random sequence.
    pub fn set(&mut self, id: u16, value: i32) -> bool {
        let Some(d) = DESC.get(id as usize) else { return false };
        let v = value.clamp(d.1, d.2);
        let before = self.p[id as usize];
        self.p[id as usize] = v;
        match id {
            P_ON if v == 0 && before != 0 => self.stop(true),
            P_LATCH if v == 0 && before != 0 => {
                // un-latching drops the chord the keys no longer hold
                let down = self.down.clone();
                self.held.retain(|(n, _)| down.contains(n));
                if self.held.is_empty() {
                    self.stop(false);
                }
            }
            P_SEED => self.reseed(),
            P_SYNC if v != before && self.running => self.to_next = if v == 1 { f64::MAX } else { 0.0 },
            _ => {}
        }
        true
    }

    pub fn on(&self) -> bool {
        self.p[P_ON as usize] == 1
    }

    pub fn held(&self) -> usize {
        self.held.len()
    }

    fn reseed(&mut self) {
        self.rng = 0x9E37_79B9_7F4A_7C15 ^ (self.p[P_SEED as usize] as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
        if self.rng == 0 {
            self.rng = 1;
        }
    }

    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn stop(&mut self, forget: bool) {
        if let Some(n) = self.sounding.take() {
            self.pending.push(Event::Off(n));
        }
        self.to_off = f64::MAX;
        self.running = false;
        self.step = 0;
        if forget {
            self.held.clear();
            self.down.clear();
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let note = note.min(127);
        if !self.down.contains(&note) && self.down.len() < MAX_HELD {
            self.down.push(note);
        }
        // a latched chord is replaced by the first note pressed after every key was released
        if self.p[P_LATCH as usize] == 1 && self.down.len() == 1 {
            self.held.clear();
        }
        self.held.retain(|&(n, _)| n != note);
        if self.held.len() < MAX_HELD {
            self.held.push((note, velocity.clamp(1, 127)));
        }
        if !self.running {
            self.running = true;
            self.step = 0;
            self.to_next = if self.p[P_SYNC as usize] == 1 { f64::MAX } else { 0.0 }; // the first step is at once, or on the next tick that starts a step
        }
    }

    /// One MIDI clock tick (a `0xF8`), `tick_frames` frames after the last: with SYNC on, every `TICKS_PER_STEP`th tick starts a step (an odd step a swing's delay after it).
    pub fn clock_tick(&mut self, tick_frames: f64) {
        if self.p[P_SYNC as usize] != 1 {
            return;
        }
        self.tick_frames = tick_frames.max(1.0);
        let tps = TICKS_PER_STEP[self.p[P_RATE as usize] as usize];
        let n = self.tick_n;
        self.tick_n += 1;
        if self.on() && self.running && self.clock_run && n.checked_rem(tps) == Some(0) {
            let swing = self.p[P_SWING as usize] as f64 / 100.0;
            self.to_next = if self.step % 2 == 1 { swing * tps as f64 * self.tick_frames } else { 0.0 };
        }
    }

    /// MIDI Start or Continue (`0xFA`, `0xFB`; the FM-1 treats them alike): the grid and the pattern begin again, and the clock runs.
    pub fn clock_start(&mut self) {
        self.tick_n = 0;
        self.clock_run = true;
        self.step = 0;
        if !self.held.is_empty() && !self.running {
            self.running = true;
            self.to_next = if self.p[P_SYNC as usize] == 1 { f64::MAX } else { 0.0 };
        }
    }

    /// MIDI Stop (`0xFC`): the sounding note ends and no step falls until the next Start; the held notes are kept.
    pub fn clock_stop(&mut self) {
        if self.p[P_SYNC as usize] != 1 {
            return;
        }
        self.clock_run = false;
        if let Some(n) = self.sounding.take() {
            self.pending.push(Event::Off(n));
        }
        self.to_off = f64::MAX;
        self.to_next = f64::MAX;
    }

    pub fn note_off(&mut self, note: u8) {
        self.down.retain(|&n| n != note);
        if self.p[P_LATCH as usize] == 1 {
            return;
        }
        self.held.retain(|&(n, _)| n != note);
        if self.held.is_empty() {
            self.stop(false);
        }
    }

    pub fn all_off(&mut self) {
        self.stop(true);
    }

    /// The notes of one cycle for the mode, octaves included.
    fn cycle(&self) -> Vec<(u8, u8)> {
        let mode = self.p[P_MODE as usize];
        let octs = self.p[P_OCTAVES as usize] as u8;
        let mut base = self.held.clone();
        if mode != 5 && mode != 6 {
            base.sort_by_key(|&(n, _)| n);
        }
        let mut up = Vec::new();
        for o in 0..octs {
            for &(n, v) in &base {
                if let Some(m) = n.checked_add(12 * o).filter(|&m| m < 128) {
                    up.push((m, v));
                }
            }
        }
        match mode {
            1 => up.into_iter().rev().collect(),
            2 => {
                let mut c = up.clone();
                c.extend(up.iter().rev());
                c
            }
            3 => {
                let mut c = up.clone();
                if up.len() > 2 {
                    c.extend(up[1..up.len() - 1].iter().rev());
                }
                c
            }
            _ => up,
        }
    }

    /// The note of step `k`, or `None` for a skipped one (chance).
    fn pick(&mut self) -> Option<(u8, u8)> {
        let mode = self.p[P_MODE as usize];
        let cycle = self.cycle();
        if cycle.is_empty() {
            return None;
        }
        let chance = self.p[P_CHANCE as usize] as u64;
        let r = self.rand();
        let skip = chance < 100 && r % 100 >= chance;
        let note = if mode == 4 { cycle[(self.rand() % cycle.len() as u64) as usize] } else { cycle[(self.step % cycle.len() as u64) as usize] };
        // mode 6 (repeat) plays each note twice before the next
        let note = if mode == 6 { cycle[((self.step / 2) % cycle.len() as u64) as usize] } else { note };
        (!skip).then_some(note)
    }

    /// Advances `frames` of audio and appends the events that fall in them, as `(frame, event)` in order.
    pub fn advance(&mut self, frames: usize, out: &mut Vec<(usize, Event)>) {
        for e in self.pending.drain(..) {
            out.push((0, e));
        }
        if !self.on() || !self.running {
            return;
        }
        let ext = self.p[P_SYNC as usize] == 1;
        let beat = 60.0 / self.p[P_BPM as usize] as f64 * self.rate;
        let rate_i = self.p[P_RATE as usize] as usize;
        let step_len = if ext && self.tick_frames > 0.0 { TICKS_PER_STEP[rate_i] as f64 * self.tick_frames } else { beat / STEPS_PER_BEAT[rate_i] };
        let gate = self.p[P_GATE as usize] as f64 / 100.0;
        let swing = self.p[P_SWING as usize] as f64 / 100.0;
        let mut t = 0.0f64; // frames into this call
        let end = frames as f64;
        loop {
            let next_on = self.to_next;
            let next_off = if self.sounding.is_some() { self.to_off } else { f64::MAX };
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
            } else {
                if let Some(n) = self.sounding.take() {
                    out.push((at, Event::Off(n))); // a gate of 100 % ends exactly as the next step begins
                }
                if let Some((n, v)) = self.pick() {
                    out.push((at, Event::On(n, v)));
                    self.sounding = Some(n);
                    self.to_off = (step_len * gate).max(1.0);
                }
                self.step += 1;
                // swing delays every odd step by a fraction of the step, taking it from the step before so the pair keeps its length
                if ext {
                    self.to_next = f64::MAX; // the next tick that starts a step says when
                } else {
                    self.to_next += if self.step % 2 == 1 { step_len * (1.0 + swing) } else { step_len * (1.0 - swing) };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays `frames` at 44.1 kHz in blocks of `block` and returns the events with their absolute frame.
    fn run(a: &mut Arp, frames: usize, block: usize) -> Vec<(usize, Event)> {
        let mut all = Vec::new();
        let mut at = 0;
        while at < frames {
            let n = block.min(frames - at);
            let mut ev = Vec::new();
            a.advance(n, &mut ev);
            all.extend(ev.into_iter().map(|(f, e)| (at + f, e)));
            at += n;
        }
        all
    }

    fn ons(ev: &[(usize, Event)]) -> Vec<u8> {
        ev.iter().filter_map(|(_, e)| if let Event::On(n, _) = e { Some(*n) } else { None }).collect()
    }

    fn chord(a: &mut Arp, notes: &[u8]) {
        a.set(P_ON, 1);
        for &n in notes {
            a.note_on(n, 100);
        }
    }

    #[test]
    fn off_by_default_and_costs_nothing() {
        let mut a = Arp::new(44_100.0);
        a.note_on(60, 100);
        assert!(run(&mut a, 44_100, 512).is_empty());
        assert!(!a.on());
    }

    #[test]
    fn up_down_and_the_octaves() {
        let notes = [64, 60, 67];
        let seq = |mode: i32, octs: i32| {
            let mut a = Arp::new(44_100.0);
            a.set(P_MODE, mode);
            a.set(P_OCTAVES, octs);
            a.set(P_BPM, 120);
            a.set(P_RATE, 2); // 1/16 at 120 BPM: 5512.5 frames
            chord(&mut a, &notes);
            ons(&run(&mut a, 5513 * 9, 512)).into_iter().take(9).collect::<Vec<u8>>()
        };
        assert_eq!(seq(0, 1), vec![60, 64, 67, 60, 64, 67, 60, 64, 67]);
        assert_eq!(seq(1, 1), vec![67, 64, 60, 67, 64, 60, 67, 64, 60]);
        assert_eq!(seq(0, 2)[..7], [60, 64, 67, 72, 76, 79, 60]);
        assert_eq!(seq(2, 1)[..8], [60, 64, 67, 67, 64, 60, 60, 64], "inclusive repeats the ends");
        assert_eq!(seq(3, 1)[..8], [60, 64, 67, 64, 60, 64, 67, 64], "exclusive does not");
        assert_eq!(seq(5, 1)[..4], [64, 60, 67, 64], "order is the order played");
        assert_eq!(seq(6, 1)[..6], [64, 64, 60, 60, 67, 67], "repeat plays each twice");
    }

    #[test]
    fn the_step_gate_and_tempo_are_exact() {
        let mut a = Arp::new(44_100.0);
        a.set(P_BPM, 120);
        a.set(P_RATE, 1); // 1/8: 11025 frames
        a.set(P_GATE, 50);
        chord(&mut a, &[60]);
        let ev = run(&mut a, 11_025 * 3 + 10, 700);
        let on: Vec<usize> = ev.iter().filter(|(_, e)| matches!(e, Event::On(..))).map(|(f, _)| *f).collect();
        let off: Vec<usize> = ev.iter().filter(|(_, e)| matches!(e, Event::Off(..))).map(|(f, _)| *f).collect();
        assert_eq!(on, vec![0, 11_025, 22_050, 33_075]);
        assert_eq!(off[0], 5_512, "50 % gate");
        // the block size does not change a single event
        let mut b = Arp::new(44_100.0);
        b.set(P_BPM, 120);
        b.set(P_RATE, 1);
        chord(&mut b, &[60]);
        assert_eq!(ev, run(&mut b, 11_025 * 3 + 10, 64));
    }

    #[test]
    fn a_full_gate_ends_as_the_next_step_begins_and_swing_moves_the_odd_steps() {
        let mut a = Arp::new(44_100.0);
        a.set(P_GATE, 100);
        chord(&mut a, &[60]);
        let ev = run(&mut a, 11_025 * 3, 512);
        let offs: Vec<usize> = ev.iter().filter(|(_, e)| matches!(e, Event::Off(..))).map(|(f, _)| *f).collect();
        assert!(offs.contains(&11_025), "{offs:?}");
        let mut s = Arp::new(44_100.0);
        s.set(P_SWING, 25);
        chord(&mut s, &[60]);
        let on: Vec<usize> = run(&mut s, 11_025 * 4, 512).iter().filter(|(_, e)| matches!(e, Event::On(..))).map(|(f, _)| *f).collect();
        assert_eq!(on[0], 0);
        assert!((on[1] as i64 - 13_781).abs() <= 1, "step 2 is late by a quarter: {on:?}");
        assert!((on[2] as i64 - 22_050).abs() <= 1, "the pair keeps its length: {on:?}");
    }

    #[test]
    fn releasing_stops_and_latch_holds_until_a_new_chord() {
        let mut a = Arp::new(44_100.0);
        chord(&mut a, &[60, 64]);
        let _ = run(&mut a, 3000, 512);
        a.note_off(60);
        a.note_off(64);
        let ev = run(&mut a, 44_100, 512);
        assert!(ons(&ev).is_empty() && ev.iter().any(|(f, e)| *f == 0 && matches!(e, Event::Off(_))), "released: the sounding note ends and nothing follows");
        let mut l = Arp::new(44_100.0);
        l.set(P_LATCH, 1);
        chord(&mut l, &[60, 64]);
        l.note_off(60);
        l.note_off(64);
        assert!(ons(&run(&mut l, 44_100, 512)).len() >= 4, "latched: it goes on");
        l.note_on(55, 100); // the first key of a new chord replaces the old one
        let after = ons(&run(&mut l, 44_100, 512));
        assert!(after.iter().all(|&n| n == 55), "{after:?}");
        l.set(P_LATCH, 0);
        assert!(l.held() == 1, "un-latching keeps only the keys still down");
        l.note_off(55);
        assert!(ons(&run(&mut l, 20_000, 512)).is_empty());
    }

    #[test]
    fn chance_and_random_are_deterministic_and_seeded() {
        let play = |seed: i32, chance: i32, mode: i32| {
            let mut a = Arp::new(44_100.0);
            a.set(P_SEED, seed);
            a.set(P_CHANCE, chance);
            a.set(P_MODE, mode);
            a.set(P_RATE, 3);
            chord(&mut a, &[48, 55, 60, 64]);
            ons(&run(&mut a, 44_100 * 4, 333))
        };
        assert_eq!(play(7, 100, 4), play(7, 100, 4), "the same seed, the same notes");
        assert_ne!(play(7, 100, 4), play(8, 100, 4));
        let all = play(3, 100, 0).len();
        let some = play(3, 50, 0).len();
        assert!(some > all / 4 && some < all * 3 / 4, "{some} of {all} at 50 %");
        assert!(play(3, 0, 0).is_empty(), "chance 0 plays nothing (Mighty Arp does the same)");
        let r = play(5, 100, 4);
        for n in [48, 55, 60, 64] {
            assert!(r.contains(&n), "random reaches {n}");
        }
    }

    #[test]
    fn it_is_bounded_and_params_clamp() {
        let mut a = Arp::new(44_100.0);
        a.set(P_ON, 1);
        for n in 0..200u8 {
            a.note_on(n, 0);
        }
        assert!(a.held() <= MAX_HELD);
        a.set(P_OCTAVES, 4);
        a.set(P_MODE, 2);
        let ev = run(&mut a, 44_100, 1024);
        assert!(ev.iter().all(|(_, e)| match e {
            Event::On(n, v) => *n < 128 && *v >= 1,
            Event::Off(n) => *n < 128,
            Event::Program(_) => false,
        }));
        assert!(a.set(P_BPM, 9999) && a.get(P_BPM) == Some(300));
        assert!(!a.set(99, 1) && a.get(99).is_none());
        a.set(P_ON, 0);
        assert_eq!(a.held(), 0, "off forgets the notes");
        assert!(a.params().len() == P_COUNT);
    }

    /// Drives the arpeggiator from a MIDI clock: tick `i` at frame `round(i * gap)`, the audio advanced to it first. Returns the events at their absolute frames.
    fn clocked(a: &mut Arp, from_tick: usize, ticks: usize, gap: f64) -> Vec<(usize, Event)> {
        let mut all = Vec::new();
        let mut at = (from_tick as f64 * gap).round() as usize;
        for i in from_tick..from_tick + ticks {
            let target = (i as f64 * gap).round() as usize;
            let mut ev = Vec::new();
            a.advance(target - at, &mut ev);
            all.extend(ev.into_iter().map(|(f, e)| (at + f, e)));
            at = target;
            a.clock_tick(gap);
        }
        let mut ev = Vec::new();
        a.advance(2000, &mut ev);
        all.extend(ev.into_iter().map(|(f, e)| (at + f, e)));
        all
    }

    const TICK: f64 = 918.75; // 120 BPM: 24 ticks to a 22050-frame quarter note

    #[test]
    fn clock_sync_puts_each_step_on_a_tick_and_waits_for_the_clock() {
        let mut a = Arp::new(44_100.0);
        a.set(P_SYNC, 1);
        a.set(P_RATE, 2); // 1/16: 6 ticks
        a.set(P_BPM, 40); // the internal tempo is not used
        chord(&mut a, &[64, 60, 67]);
        assert!(run(&mut a, 30_000, 512).is_empty(), "no clock, no step: it waits");
        a.clock_start();
        let ev = clocked(&mut a, 0, 25, TICK);
        let on: Vec<(usize, u8)> = ev.iter().filter_map(|(f, e)| if let Event::On(n, _) = e { Some((*f, *n)) } else { None }).collect();
        assert_eq!(on[..4], [(0, 60), (5513, 64), (11_025, 67), (16_538, 60)].map(|(f, n)| (f as usize, n)), "{on:?}");
        let off = ev.iter().find(|(_, e)| matches!(e, Event::Off(_))).unwrap().0;
        assert_eq!(off, 2756, "the gate (50 %) of a step measured from the ticks"); // the step is 6 ticks of 918.75
    }

    #[test]
    fn clock_sync_follows_the_tempo_of_the_ticks_and_swing_delays_odd_steps() {
        let mut a = Arp::new(44_100.0);
        a.set(P_SYNC, 1);
        a.set(P_RATE, 2);
        a.set(P_SWING, 25);
        chord(&mut a, &[60]);
        a.clock_start();
        let slow = clocked(&mut a, 0, 13, TICK);
        let on: Vec<usize> = slow.iter().filter(|(_, e)| matches!(e, Event::On(..))).map(|(f, _)| *f).collect();
        assert_eq!(on[0], 0);
        assert!((on[1] as i64 - 6_891).abs() <= 1, "the second step is a quarter of a step late: {on:?}");
        assert!((on[2] as i64 - 11_025).abs() <= 1, "the pair keeps its length: {on:?}");
        // twice as fast: the same steps come in half the frames
        let mut b = Arp::new(44_100.0);
        b.set(P_SYNC, 1);
        b.set(P_RATE, 2);
        chord(&mut b, &[60]);
        b.clock_start();
        let fast = clocked(&mut b, 0, 13, TICK / 2.0);
        let f: Vec<usize> = fast.iter().filter(|(_, e)| matches!(e, Event::On(..))).map(|(f, _)| *f).collect();
        assert_eq!(f[..3], [0, 2756, 5513]);
    }

    #[test]
    fn clock_stop_ends_the_note_and_start_begins_again_and_internal_mode_ignores_ticks() {
        let mut a = Arp::new(44_100.0);
        a.set(P_SYNC, 1);
        a.set(P_RATE, 2);
        chord(&mut a, &[60, 64]);
        a.clock_start();
        let _ = clocked(&mut a, 0, 1, TICK); // step one has just begun and its note is still sounding
        a.clock_stop();
        let mut ev = Vec::new();
        a.advance(100, &mut ev);
        assert!(ev.contains(&(0, Event::Off(60))) || ev.contains(&(0, Event::Off(64))), "stop releases the sounding note: {ev:?}");
        a.clock_tick(TICK);
        a.clock_tick(TICK);
        let mut quiet = Vec::new();
        a.advance(20_000, &mut quiet);
        assert!(quiet.is_empty(), "stopped: ticks do not start steps");
        a.clock_start();
        let again = clocked(&mut a, 0, 8, TICK);
        assert_eq!(ons(&again)[..2], [60, 64], "it begins again from the first note");
        // with SYNC off the ticks change nothing
        let mut i = Arp::new(44_100.0);
        chord(&mut i, &[60]);
        let mut base = Vec::new();
        i.advance(12_000, &mut base);
        let mut j = Arp::new(44_100.0);
        chord(&mut j, &[60]);
        for _ in 0..10 {
            j.clock_tick(TICK);
        }
        let mut with = Vec::new();
        j.advance(12_000, &mut with);
        assert_eq!(base, with);
        // switching sync on while running waits for a tick; off, it runs on its own again
        let mut k = Arp::new(44_100.0);
        chord(&mut k, &[60]);
        let _ = run(&mut k, 1000, 512);
        k.set(P_SYNC, 1);
        assert!(ons(&run(&mut k, 30_000, 512)).is_empty(), "no new step until a tick");
        k.set(P_SYNC, 0);
        assert!(!ons(&run(&mut k, 30_000, 512)).is_empty());
    }
}
