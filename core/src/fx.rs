//! The shared effects of an instrument package (docs/PORTING.md): a limiter, a delay, a low-pass, a chorus, a reverb, a filter (low, band, high), a distortion and a phaser, in place on a mono block.
//! Written for FM1-NTT, fitted to no firmware. Parameters are described like an engine's (`ParamDesc`) so the same editor and MIDI map reach them.
//! No allocation after `new`.

use crate::engine::ParamDesc;

pub trait Effect {
    fn name(&self) -> &'static str;
    fn params(&self) -> Vec<ParamDesc>;
    fn get(&self, id: u16) -> Option<i32>;
    fn set(&mut self, id: u16, value: i32) -> bool;
    fn process(&mut self, buf: &mut [i16]);
}

/// The effect called `name` ("limiter", "delay", "lowpass") at `rate`, or `None`.
pub fn make(name: &str, rate: f64) -> Option<Box<dyn Effect>> {
    let rate = rate.clamp(8000.0, 192_000.0);
    match name {
        "limiter" => Some(Box::new(Limiter::new(rate))),
        "delay" => Some(Box::new(Delay::new(rate))),
        "lowpass" => Some(Box::new(Lowpass::new(rate))),
        "chorus" => Some(Box::new(Chorus::new(rate))),
        "reverb" => Some(Box::new(Reverb::new(rate))),
        "filter" => Some(Box::new(Filter::new(rate))),
        "distortion" => Some(Box::new(Distortion::new(rate))),
        "phaser" => Some(Box::new(Phaser::new(rate))),
        _ => None,
    }
}

fn pd(id: u16, name: &str, min: i32, max: i32, default: i32, unit: &'static str) -> ParamDesc {
    ParamDesc { id, name: name.to_string(), min, max, default, unit }
}

/// A peak limiter: instant attack, exponential release.
pub struct Limiter {
    rate: f64,
    ceiling: i32,
    release: i32,
    gain: f64,
}

impl Limiter {
    pub fn new(rate: f64) -> Limiter {
        Limiter { rate, ceiling: 90, release: 80, gain: 1.0 }
    }
}

impl Effect for Limiter {
    fn name(&self) -> &'static str {
        "limiter"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "CEILING", 5, 100, 90, "%"), pd(1, "RELEASE", 1, 2000, 80, "ms")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0 => Some(self.ceiling),
            1 => Some(self.release),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        match id {
            0 => self.ceiling = v.clamp(5, 100),
            1 => self.release = v.clamp(1, 2000),
            _ => return false,
        }
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let c = self.ceiling as f64 / 100.0 * 32767.0;
        let rel = 1.0 - (-1.0 / (self.release as f64 * 0.001 * self.rate)).exp();
        for s in buf.iter_mut() {
            let y = *s as f64;
            let need = if y.abs() > c { c / y.abs() } else { 1.0 };
            self.gain = need.min(self.gain + (1.0 - self.gain) * rel);
            *s = (y * self.gain).round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

/// A feedback delay of up to one second.
pub struct Delay {
    line: Vec<f32>,
    at: usize,
    rate: f64,
    time: i32,
    feedback: i32,
    mix: i32,
}

impl Delay {
    pub fn new(rate: f64) -> Delay {
        Delay { line: vec![0.0; rate as usize + 1], at: 0, rate, time: 250, feedback: 35, mix: 25 }
    }
}

impl Effect for Delay {
    fn name(&self) -> &'static str {
        "delay"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "TIME", 1, 1000, 250, "ms"), pd(1, "FEEDBACK", 0, 90, 35, "%"), pd(2, "MIX", 0, 100, 25, "%")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0 => Some(self.time),
            1 => Some(self.feedback),
            2 => Some(self.mix),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        match id {
            0 => self.time = v.clamp(1, 1000),
            1 => self.feedback = v.clamp(0, 90),
            2 => self.mix = v.clamp(0, 100),
            _ => return false,
        }
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let n = self.line.len();
        let d = ((self.time as f64 * 0.001 * self.rate) as usize).clamp(1, n - 1);
        let (fb, mix) = (self.feedback as f32 / 100.0, self.mix as f32 / 100.0);
        for s in buf.iter_mut() {
            let x = *s as f32;
            let wet = self.line[(self.at + n - d) % n];
            self.line[self.at] = x + wet * fb;
            self.at = (self.at + 1) % n;
            *s = (x * (1.0 - mix) + wet * mix).round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

/// A one-pole low-pass.
pub struct Lowpass {
    rate: f64,
    cutoff: i32,
    y: f64,
}

impl Lowpass {
    pub fn new(rate: f64) -> Lowpass {
        Lowpass { rate, cutoff: 8000, y: 0.0 }
    }
}

impl Effect for Lowpass {
    fn name(&self) -> &'static str {
        "lowpass"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "CUTOFF", 20, 20000, 8000, "Hz")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        (id == 0).then_some(self.cutoff)
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        if id != 0 {
            return false;
        }
        self.cutoff = v.clamp(20, 20000);
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let a = 1.0 - (-2.0 * std::f64::consts::PI * (self.cutoff as f64).min(self.rate * 0.45) / self.rate).exp();
        for s in buf.iter_mut() {
            self.y += (*s as f64 - self.y) * a;
            *s = self.y.round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

/// A chorus: a short delay swept by a sine LFO, mixed with the dry signal.
pub struct Chorus {
    line: Vec<f32>,
    at: usize,
    rate: f64,
    phase: f64,
    lfo: i32,
    depth: i32,
    mix: i32,
}

impl Chorus {
    pub fn new(rate: f64) -> Chorus {
        Chorus { line: vec![0.0; (rate * 0.05) as usize + 4], at: 0, rate, phase: 0.0, lfo: 8, depth: 40, mix: 50 }
    }
}

impl Effect for Chorus {
    fn name(&self) -> &'static str {
        "chorus"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "RATE (0.1 HZ)", 1, 100, 8, ""), pd(1, "DEPTH", 0, 100, 40, ""), pd(2, "MIX", 0, 100, 50, "%")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0 => Some(self.lfo),
            1 => Some(self.depth),
            2 => Some(self.mix),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        match id {
            0 => self.lfo = v.clamp(1, 100),
            1 => self.depth = v.clamp(0, 100),
            2 => self.mix = v.clamp(0, 100),
            _ => return false,
        }
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let n = self.line.len();
        let step = self.lfo as f64 * 0.1 / self.rate;
        // the centre 12 ms, swept by up to 8 ms
        let (centre, sweep) = (0.012 * self.rate, self.depth as f64 / 100.0 * 0.008 * self.rate);
        let mix = self.mix as f32 / 100.0;
        for s in buf.iter_mut() {
            let x = *s as f32;
            self.line[self.at] = x;
            let d = centre + sweep * (2.0 * std::f64::consts::PI * self.phase).sin();
            let pos = self.at as f64 + n as f64 - d;
            let (i, f) = (pos.floor() as usize % n, (pos - pos.floor()) as f32);
            let wet = self.line[i] * (1.0 - f) + self.line[(i + 1) % n] * f;
            self.at = (self.at + 1) % n;
            self.phase = (self.phase + step).fract();
            *s = (x * (1.0 - mix * 0.5) + wet * mix * 0.5).round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

struct Comb {
    buf: Vec<f32>,
    at: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Comb {
        Comb { buf: vec![0.0; len.max(2)], at: 0, store: 0.0 }
    }

    fn tick(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.at];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.at] = x + self.store * feedback;
        self.at = (self.at + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    at: usize,
}

impl Allpass {
    fn new(len: usize) -> Allpass {
        Allpass { buf: vec![0.0; len.max(2)], at: 0 }
    }

    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.at];
        let out = b - x;
        self.buf[self.at] = x + b * 0.5;
        self.at = (self.at + 1) % self.buf.len();
        out
    }
}

/// A small Schroeder reverb: four damped combs and two all-passes (the classic arrangement), lengths scaled to the sample rate.
pub struct Reverb {
    combs: [Comb; 4],
    aps: [Allpass; 2],
    room: i32,
    damp: i32,
    mix: i32,
}

impl Reverb {
    pub fn new(rate: f64) -> Reverb {
        let k = rate / 44_100.0;
        let l = |n: f64| (n * k) as usize;
        Reverb {
            combs: [Comb::new(l(1116.0)), Comb::new(l(1188.0)), Comb::new(l(1277.0)), Comb::new(l(1356.0))],
            aps: [Allpass::new(l(556.0)), Allpass::new(l(441.0))],
            room: 60,
            damp: 40,
            mix: 25,
        }
    }
}

impl Effect for Reverb {
    fn name(&self) -> &'static str {
        "reverb"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "ROOM", 0, 99, 60, ""), pd(1, "DAMPING", 0, 99, 40, ""), pd(2, "MIX", 0, 100, 25, "%")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0 => Some(self.room),
            1 => Some(self.damp),
            2 => Some(self.mix),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        match id {
            0 => self.room = v.clamp(0, 99),
            1 => self.damp = v.clamp(0, 99),
            2 => self.mix = v.clamp(0, 100),
            _ => return false,
        }
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let feedback = 0.7 + 0.28 * self.room as f32 / 99.0;
        let damp = 0.05 + 0.85 * self.damp as f32 / 99.0;
        let mix = self.mix as f32 / 100.0;
        for s in buf.iter_mut() {
            let x = *s as f32;
            let input = x * 0.015;
            let mut wet = 0.0;
            for c in &mut self.combs {
                wet += c.tick(input, feedback, damp);
            }
            for a in &mut self.aps {
                wet = a.tick(wet);
            }
            *s = (x * (1.0 - mix) + wet * 33.0 * mix).round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

/// A state-variable filter with three modes (the stock firmware's filter slot: LPF, BPF, HPF; docs/FM-1-MIDI.md). A TPT (zero-delay) topology, stable at any cutoff.
pub struct Filter {
    rate: f64,
    kind: i32,
    cutoff: i32,
    q: i32,
    ic1: f64,
    ic2: f64,
}

impl Filter {
    pub fn new(rate: f64) -> Filter {
        Filter { rate, kind: 0, cutoff: 8000, q: 10, ic1: 0.0, ic2: 0.0 }
    }
}

impl Effect for Filter {
    fn name(&self) -> &'static str {
        "filter"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "TYPE (0 LP 1 BP 2 HP)", 0, 2, 0, ""), pd(1, "CUTOFF", 20, 20000, 8000, "Hz"), pd(2, "Q", 0, 100, 10, "")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0 => Some(self.kind),
            1 => Some(self.cutoff),
            2 => Some(self.q),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        match id {
            0 => self.kind = v.clamp(0, 2),
            1 => self.cutoff = v.clamp(20, 20000),
            2 => self.q = v.clamp(0, 100),
            _ => return false,
        }
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let g = (std::f64::consts::PI * (self.cutoff as f64).min(self.rate * 0.45) / self.rate).tan();
        // Q 0..100 maps to a damping of 1.4 (flat) down to 0.05 (a sharp peak); the loop stays stable for any value of it
        let k = 1.4 - 1.35 * self.q as f64 / 100.0;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let (a2, a3) = (g * a1, g * g * a1);
        for s in buf.iter_mut() {
            let v0 = *s as f64;
            let v3 = v0 - self.ic2;
            let v1 = a1 * self.ic1 + a2 * v3;
            let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
            self.ic1 = 2.0 * v1 - self.ic1;
            self.ic2 = 2.0 * v2 - self.ic2;
            let y = match self.kind {
                0 => v2,
                1 => v1 * k, // the band-pass at unit gain on its centre
                _ => v0 - k * v1 - v2,
            };
            *s = y.round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

/// A distortion: gain into a soft clip (tanh), a tone low-pass, an output level.
pub struct Distortion {
    rate: f64,
    gain: i32,
    tone: i32,
    level: i32,
    mix: i32,
    lp: f64,
}

impl Distortion {
    pub fn new(rate: f64) -> Distortion {
        Distortion { rate, gain: 40, tone: 60, level: 70, mix: 100, lp: 0.0 }
    }
}

impl Effect for Distortion {
    fn name(&self) -> &'static str {
        "distortion"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "GAIN", 0, 100, 40, ""), pd(1, "TONE", 0, 100, 60, ""), pd(2, "LEVEL", 0, 100, 70, ""), pd(3, "MIX", 0, 100, 100, "%")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0 => Some(self.gain),
            1 => Some(self.tone),
            2 => Some(self.level),
            3 => Some(self.mix),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        match id {
            0 => self.gain = v.clamp(0, 100),
            1 => self.tone = v.clamp(0, 100),
            2 => self.level = v.clamp(0, 100),
            3 => self.mix = v.clamp(0, 100),
            _ => return false,
        }
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let drive = 1.0 + self.gain as f64 * 0.3; // 1x to 31x
        // tone: the low-pass after the clip, 1 kHz (0) to 12 kHz (100), on a log scale
        let fc = 1000.0 * (12.0f64).powf(self.tone as f64 / 100.0);
        let a = 1.0 - (-2.0 * std::f64::consts::PI * fc.min(self.rate * 0.45) / self.rate).exp();
        let out = self.level as f64 / 100.0;
        let norm = 1.0 / drive.tanh(); // a full-scale input comes out at full scale before the level
        let mix = self.mix as f64 / 100.0;
        for s in buf.iter_mut() {
            let x = *s as f64 / 32768.0;
            let clipped = (x * drive).tanh() * norm;
            self.lp += (clipped - self.lp) * a;
            let y = (x * (1.0 - mix) + self.lp * out * mix) * 32768.0;
            *s = y.round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

/// A phaser: four first-order all-pass stages swept by a sine LFO, summed with the dry signal.
pub struct Phaser {
    rate: f64,
    phase: f64,
    freq: i32,
    depth: i32,
    mix: i32,
    z: [f64; 4],
}

impl Phaser {
    pub fn new(rate: f64) -> Phaser {
        Phaser { rate, phase: 0.0, freq: 5, depth: 60, mix: 50, z: [0.0; 4] }
    }
}

impl Effect for Phaser {
    fn name(&self) -> &'static str {
        "phaser"
    }

    fn params(&self) -> Vec<ParamDesc> {
        vec![pd(0, "RATE (0.1 HZ)", 1, 100, 5, ""), pd(1, "DEPTH", 0, 100, 60, ""), pd(2, "MIX", 0, 100, 50, "%")]
    }

    fn get(&self, id: u16) -> Option<i32> {
        match id {
            0 => Some(self.freq),
            1 => Some(self.depth),
            2 => Some(self.mix),
            _ => None,
        }
    }

    fn set(&mut self, id: u16, v: i32) -> bool {
        match id {
            0 => self.freq = v.clamp(1, 100),
            1 => self.depth = v.clamp(0, 100),
            2 => self.mix = v.clamp(0, 100),
            _ => return false,
        }
        true
    }

    fn process(&mut self, buf: &mut [i16]) {
        let step = self.freq as f64 * 0.1 / self.rate;
        let depth = self.depth as f64 / 100.0;
        let mix = self.mix as f64 / 100.0;
        for s in buf.iter_mut() {
            // the all-pass corner sweeps 300 Hz to 3 kHz (a log sweep, wider with depth)
            let lfo = 0.5 + 0.5 * (self.phase * std::f64::consts::TAU).sin();
            self.phase = (self.phase + step).fract();
            let fc = 300.0 * (10.0f64).powf(lfo * depth);
            let t = (std::f64::consts::PI * fc.min(self.rate * 0.45) / self.rate).tan();
            let c = (t - 1.0) / (t + 1.0);
            let x = *s as f64;
            let mut y = x;
            for z in &mut self.z {
                let o = c * y + *z;
                *z = y - c * o;
                y = o;
            }
            *s = (x * (1.0 - mix * 0.5) + y * mix * 0.5).round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_holds_the_ceiling() {
        let mut l = Limiter::new(44_100.0);
        l.set(0, 50);
        let mut b = vec![30_000i16; 2000];
        l.process(&mut b);
        assert!(b.iter().all(|&x| x.abs() <= 16_384), "{}", b.iter().map(|x| x.abs()).max().unwrap());
    }

    #[test]
    fn delay_echoes_at_the_time() {
        let mut d = Delay::new(44_100.0);
        d.set(0, 10); // 441 samples
        d.set(2, 100);
        d.set(1, 0);
        let mut b = vec![0i16; 2000];
        b[0] = 10_000;
        d.process(&mut b);
        assert_eq!(b[0], 0, "mix 100: the dry is gone");
        assert_eq!(b[441], 10_000);
        assert_eq!(b[442], 0);
    }

    #[test]
    fn lowpass_removes_the_top_and_params_clamp() {
        let mut f = Lowpass::new(44_100.0);
        f.set(0, 100);
        let mut b: Vec<i16> = (0..4000).map(|i| if i % 2 == 0 { 20_000 } else { -20_000 }).collect();
        f.process(&mut b);
        assert!(b[3000..].iter().all(|&x| x.abs() < 500));
        assert!(f.set(0, 99_999));
        assert_eq!(f.get(0), Some(20_000));
        assert!(!f.set(3, 1));
        assert!(make("nope", 44_100.0).is_none());
        assert_eq!(make("delay", 44_100.0).unwrap().name(), "delay");
    }
    #[test]
    fn reverb_rings_on_after_the_input_stops_and_a_bigger_room_rings_longer() {
        let tail = |room: i32| {
            let mut r = Reverb::new(44_100.0);
            r.set(0, room);
            r.set(2, 100);
            let mut b = vec![0i16; 44_100];
            for s in b.iter_mut().take(200) {
                *s = 12_000;
            }
            r.process(&mut b);
            b[20_000..].iter().map(|&x| (x as f64).powi(2)).sum::<f64>()
        };
        assert!(tail(30) > 0.0, "a reverb has a tail");
        assert!(tail(95) > tail(30) * 2.0);
        let mut r = Reverb::new(44_100.0);
        let mut z = vec![0i16; 4096];
        r.process(&mut z);
        assert!(z.iter().all(|&x| x == 0), "silence in, silence out");
        assert!(!r.set(3, 1));
    }

    #[test]
    fn chorus_keeps_the_level_and_stays_bounded() {
        let mut c = Chorus::new(44_100.0);
        let x: Vec<i16> = (0..22_050).map(|i| ((i as f64 * 0.06).sin() * 10_000.0) as i16).collect();
        let mut y = x.clone();
        c.process(&mut y);
        let rms = |v: &[i16]| (v.iter().map(|&a| (a as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        let (a, b) = (rms(&x[4000..]), rms(&y[4000..]));
        assert!(b > a * 0.6 && b < a * 1.2, "{a} vs {b}");
        assert!(y.iter().all(|&s| s.abs() <= 12_000));
        assert_eq!(make("chorus", 44_100.0).unwrap().name(), "chorus");
        assert_eq!(make("reverb", 44_100.0).unwrap().name(), "reverb");
        c.set(1, 999);
        assert_eq!(c.get(1), Some(100));
    }

    fn sine(f: f64, n: usize, amp: f64) -> Vec<i16> {
        (0..n).map(|i| ((i as f64 * f * std::f64::consts::TAU / 44_100.0).sin() * amp) as i16).collect()
    }
    fn rms(v: &[i16]) -> f64 {
        (v.iter().map(|&a| (a as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    }

    #[test]
    fn the_filter_modes_pass_and_stop_the_right_bands() {
        let through = |kind: i32, f: f64| {
            let mut e = Filter::new(44_100.0);
            e.set(0, kind);
            e.set(1, 1000);
            let mut b = sine(f, 20_000, 10_000.0);
            e.process(&mut b);
            rms(&b[10_000..]) / rms(&sine(f, 10_000, 10_000.0))
        };
        assert!(through(0, 100.0) > 0.9 && through(0, 10_000.0) < 0.05, "low-pass");
        assert!(through(2, 100.0) < 0.05 && through(2, 10_000.0) > 0.9, "high-pass");
        assert!(through(1, 1000.0) > 0.7 && through(1, 100.0) < 0.25 && through(1, 10_000.0) < 0.25, "band-pass");
    }

    #[test]
    fn the_filter_is_stable_at_every_setting_and_q_peaks_the_cutoff() {
        for kind in 0..3 {
            for q in [0, 50, 100] {
                for fc in [100, 1000, 20_000] {
                    let mut e = Filter::new(44_100.0);
                    e.set(0, kind);
                    e.set(1, fc);
                    e.set(2, q);
                    let mut b: Vec<i16> = (0..20_000).map(|i| if (i / 50) % 2 == 0 { 30_000 } else { -30_000 }).collect();
                    e.process(&mut b);
                    let mut tail = vec![0i16; 60_000];
                    e.process(&mut tail);
                    assert!(rms(&tail[50_000..]) < 50.0, "kind {kind} q {q} fc {fc} rings forever");
                }
            }
        }
        let at = |q: i32| {
            let mut e = Filter::new(44_100.0);
            e.set(1, 1000);
            e.set(2, q);
            let mut b = sine(1000.0, 20_000, 3000.0);
            e.process(&mut b);
            rms(&b[10_000..])
        };
        assert!(at(90) > at(10) * 1.5);
    }

    #[test]
    fn distortion_adds_harmonics_stays_bounded_and_is_silent_on_silence() {
        let mut d = Distortion::new(44_100.0);
        d.set(0, 100);
        let mut b = sine(300.0, 20_000, 20_000.0);
        d.process(&mut b);
        // a clipped sine is closer to a square: more energy for the same peak than the sine's
        assert!(rms(&b[5000..]) > rms(&sine(300.0, 10_000, 20_000.0)) * 0.5);
        let mut z = vec![0i16; 2048];
        Distortion::new(44_100.0).process(&mut z);
        assert!(z.iter().all(|&x| x == 0));
        let mut low = Distortion::new(44_100.0);
        low.set(0, 0);
        low.set(1, 100);
        low.set(2, 100);
        let mut q = sine(300.0, 8000, 2000.0);
        let before = q.clone();
        low.process(&mut q);
        assert!(rms(&q[4000..]) > rms(&before[4000..]) * 0.5 && rms(&q[4000..]) < rms(&before[4000..]) * 1.5, "no gain, a clean pass");
        low.set(3, 0);
        let mut dry = before.clone();
        low.process(&mut dry);
        assert_eq!(dry, before, "mix 0 is the dry signal");
    }

    #[test]
    fn phaser_notches_move_and_it_stays_bounded() {
        let mut p = Phaser::new(44_100.0);
        let x = sine(1000.0, 44_100, 10_000.0);
        let mut y = x.clone();
        p.process(&mut y);
        assert!(y.iter().all(|&s| s.abs() <= 12_000));
        let blocks: Vec<f64> = y.chunks(4410).map(rms).collect();
        let (lo, hi) = blocks.iter().fold((f64::MAX, 0.0f64), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(hi > lo * 1.15, "the sweep changes the level at 1 kHz: {lo} .. {hi}");
        p.set(2, 0);
        let mut dry = x.clone();
        p.process(&mut dry);
        assert_eq!(dry, x);
        for n in ["filter", "distortion", "phaser"] {
            assert_eq!(make(n, 44_100.0).unwrap().name(), n);
        }
        assert!(!p.set(3, 1) && !Filter::new(44_100.0).set(3, 1) && !Distortion::new(44_100.0).set(4, 1));
    }
}
