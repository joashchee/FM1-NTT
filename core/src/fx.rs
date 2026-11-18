//! The shared effects of an instrument package (docs/PORTING.md): a limiter, a delay, a low-pass, a chorus and a reverb, in place on a mono block.
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
}
