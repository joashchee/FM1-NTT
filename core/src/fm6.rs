// SPDX-License-Identifier: Apache-2.0
// Copyright 2012, 2013 Google Inc. (msfa, the original C++)
// Port to Rust by FM1-NTT, 2026-11-17: see core/vendor/msfa/VENDORED.md for what it is derived from and what was changed.
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0. Unless required by
// applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.

//! A DX7-class 6-operator FM voice engine: msfa's (Google's music-synthesizer-for-android) integer path, ported to Rust.
//!
//! It plays DX7 packed voices (128 bytes: what the FM-1's stock firmware keeps, `extract::valid_voice`). It is **not
//! the FM-1's engine**: it is the DX7's as msfa models it, and how far the FM-1 differs is not yet measured
//! (ROADMAP section 5a). It is deterministic (integers and tables only, nothing random but the LFO's sample and hold,
//! which is a fixed sequence), and it never panics on a bad voice: fields are clamped where they index a table.
//!
//! `core/tests/fm6_golden.rs` checks it **bit for bit** against the vendored C++ (`core/tests/msfa/oracle.cc`).
//! Ported: the tables (sine, exp2, frequency), the DX7 envelope and pitch envelope, the LFO, the operator kernels and
//! the 32 algorithms, and a note (`Voice`). Left out: the NEON kernel (a float variant of the same maths), the tanh
//! and log2 tables (no part of the voice uses them) and the experimental sine generators (`#if 0` in the original).

use std::f64::consts::PI;

pub const LG_N: u32 = 6;
/// Samples per block: msfa computes in blocks of 64.
pub const N: usize = 1 << LG_N;
/// Voices at once in `Synth`: the FM-1's stock firmware's 12 (V16 has 20).
pub const VOICES: usize = 12;

/// Arithmetic shift right by any amount: what the C++ does where it is defined, and a fixed answer where it is not.
fn sar(v: i32, s: i32) -> i32 {
    if s >= 32 {
        v >> 31
    } else if s >= 0 {
        v >> s
    } else {
        v.wrapping_shl((-s).min(31) as u32)
    }
}

fn mul_shr(a: i32, b: i32, shift: u32) -> i32 {
    ((a as i64 * b as i64) >> shift) as i32
}

/// The look-up tables msfa builds at start-up (`Sin::init`, `Exp2::init`, `Freqlut::init`, `Lfo::init`, `PitchEnv::init`).
/// They depend on the sample rate and on the host's `libm` (`sin`, `cos`, `pow`, `exp2`): the golden test says whether
/// this host's tables match the reference's.
pub struct Tables {
    sin: [i32; 2048],
    exp2: [i32; 2048],
    freq: [i32; 1025],
    lfo_unit: u32,
    pitch_unit: i32,
}

impl Tables {
    pub fn new(sample_rate: f64) -> Result<Tables, &'static str> {
        if !(8000.0..=192_000.0).contains(&sample_rate) {
            return Err("sample rate outside 8000 to 192000");
        }
        let mut t = Tables { sin: [0; 2048], exp2: [0; 2048], freq: [0; 1025], lfo_unit: 0, pitch_unit: 0 };
        // sine: a rotation recurrence in Q30, as msfa does
        let dphase = 2.0 * PI / 1024.0;
        let c = (dphase.cos() * (1u64 << 30) as f64 + 0.5).floor() as i32;
        let s = (dphase.sin() * (1u64 << 30) as f64 + 0.5).floor() as i32;
        let (mut u, mut v) = (1i32 << 30, 0i32);
        const R: i64 = 1 << 29;
        for i in 0..512 {
            t.sin[(i << 1) + 1] = (v + 32) >> 6;
            t.sin[((i + 512) << 1) + 1] = -((v + 32) >> 6);
            let tt = ((u as i64 * s as i64 + v as i64 * c as i64 + R) >> 30) as i32;
            u = ((u as i64 * c as i64 - v as i64 * s as i64 + R) >> 30) as i32;
            v = tt;
        }
        for i in 0..1023 {
            t.sin[i << 1] = t.sin[(i << 1) + 3] - t.sin[(i << 1) + 1];
        }
        t.sin[2046] = -t.sin[2047];
        // exp2
        let inc = (1.0f64 / 1024.0).exp2();
        let mut y = (1u64 << 30) as f64;
        for i in 0..1024 {
            t.exp2[(i << 1) + 1] = (y + 0.5).floor() as i32;
            y *= inc;
        }
        for i in 0..1023 {
            t.exp2[i << 1] = t.exp2[(i << 1) + 3] - t.exp2[(i << 1) + 1];
        }
        t.exp2[2046] = 0x8000_0000u32.wrapping_sub(t.exp2[2047] as u32) as i32;
        // frequency: Q24 octaves to a phase step
        let mut y = (1u64 << (24 + 20)) as f64 / sample_rate;
        let inc = 2f64.powf(1.0 / 1024.0);
        for i in 0..1025 {
            t.freq[i] = (y + 0.5).floor() as i32;
            y *= inc;
        }
        t.lfo_unit = ((64.0 * 25_190_424.0) / sample_rate + 0.5) as i32 as u32;
        t.pitch_unit = ((64.0 * (1u64 << 24) as f64) / (21.3 * sample_rate) + 0.5) as i32;
        Ok(t)
    }

    pub fn sin_table(&self) -> &[i32] {
        &self.sin
    }
    pub fn exp2_table(&self) -> &[i32] {
        &self.exp2
    }
    pub fn freq_table(&self) -> &[i32] {
        &self.freq
    }

    /// Sine of a Q24 phase (one cycle is 1 << 24), Q24 out.
    fn sin(&self, phase: i32) -> i32 {
        let lowbits = phase & ((1 << 14) - 1);
        let ix = ((phase >> 13) & (1023 << 1)) as usize;
        let (dy, y0) = (self.sin[ix], self.sin[ix + 1]);
        y0.wrapping_add(mul_shr(dy, lowbits, 14))
    }

    /// 2 to the Q24 power x, Q24 out.
    fn exp2(&self, x: i32) -> i32 {
        let lowbits = x & ((1 << 14) - 1);
        let ix = ((x >> 13) & (1023 << 1)) as usize;
        let (dy, y0) = (self.exp2[ix], self.exp2[ix + 1]);
        let y = y0.wrapping_add(mul_shr(dy, lowbits, 14));
        sar(y, 6 - (x >> 24))
    }

    /// A frequency in Q24 octaves to a phase step per sample.
    fn freq(&self, logfreq: i32) -> i32 {
        let ix = ((logfreq & 0xff_ffff) >> 14) as usize;
        let (y0, y1) = (self.freq[ix], self.freq[ix + 1]);
        let lowbits = logfreq & ((1 << 14) - 1);
        let y = y0.wrapping_add(mul_shr(y1.wrapping_sub(y0), lowbits, 14));
        sar(y, 20 - (logfreq >> 24))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Envelopes

const LEVELLUT: [i32; 20] = [0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46];

/// An operator level 0 to 99 as the DX7's output-level scale.
fn scale_out_level(l: i32) -> i32 {
    let l = l.clamp(0, 99);
    if l >= 20 {
        28 + l
    } else {
        LEVELLUT[l as usize]
    }
}

#[derive(Clone, Default)]
struct Env {
    rates: [i32; 4],
    levels: [i32; 4],
    outlevel: i32,
    rate_scaling: i32,
    level: i32,
    targetlevel: i32,
    rising: bool,
    ix: usize,
    inc: i32,
    down: bool,
}

impl Env {
    fn init(&mut self, rates: [i32; 4], levels: [i32; 4], outlevel: i32, rate_scaling: i32) {
        self.rates = rates;
        self.levels = levels;
        self.outlevel = outlevel;
        self.rate_scaling = rate_scaling;
        self.level = 0;
        self.down = true;
        self.advance(0);
    }

    fn getsample(&mut self) -> i32 {
        if self.ix < 3 || (self.ix < 4 && !self.down) {
            if self.rising {
                const JUMP: i32 = 1716;
                if self.level < (JUMP << 16) {
                    self.level = JUMP << 16;
                }
                self.level = self.level.wrapping_add((((17 << 24) - self.level) >> 24).wrapping_mul(self.inc));
                if self.level >= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            } else {
                self.level = self.level.wrapping_sub(self.inc);
                if self.level <= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            }
        }
        self.level
    }

    fn keydown(&mut self, d: bool) {
        if self.down != d {
            self.down = d;
            self.advance(if d { 0 } else { 3 });
        }
    }

    fn advance(&mut self, newix: usize) {
        self.ix = newix;
        if newix < 4 {
            let actual = scale_out_level(self.levels[newix]) >> 1;
            let actual = ((actual << 6) + self.outlevel - 4256).max(16);
            self.targetlevel = actual << 16;
            self.rising = self.targetlevel > self.level;
            let qrate = (((self.rates[newix] * 41) >> 6) + self.rate_scaling).min(63);
            self.inc = (4 + (qrate & 3)) << (2 + LG_N as i32 + (qrate >> 2));
        }
    }
}

const RATETAB: [u8; 100] = [
    1, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 16, 16, 17, 18, 18, 19,
    20, 21, 22, 23, 24, 25, 26, 27, 28, 30, 31, 33, 34, 36, 37, 38, 39, 41, 42, 44, 46, 47, 49, 51, 53, 54, 56, 58, 60,
    62, 64, 66, 68, 70, 72, 74, 76, 79, 82, 85, 88, 91, 94, 98, 102, 106, 110, 115, 120, 125, 130, 135, 141, 147, 153,
    159, 165, 171, 178, 185, 193, 202, 211, 232, 243, 254, 255,
];
const PITCHTAB: [i8; 100] = [
    -128, -116, -104, -95, -85, -76, -68, -61, -56, -52, -49, -46, -43, -41, -39, -37, -35, -33, -32, -31, -30, -29,
    -28, -27, -26, -25, -24, -23, -22, -21, -20, -19, -18, -17, -16, -15, -14, -13, -12, -11, -10, -9, -8, -7, -6, -5,
    -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 38, 40, 43, 46, 49, 53, 58, 65, 73, 82, 92, 103, 115, 127,
];

#[derive(Clone, Default)]
struct PitchEnv {
    rates: [i32; 4],
    levels: [i32; 4],
    level: i32,
    targetlevel: i32,
    rising: bool,
    ix: usize,
    inc: i32,
    down: bool,
    unit: i32,
}

impl PitchEnv {
    fn set(&mut self, t: &Tables, rates: [i32; 4], levels: [i32; 4]) {
        self.unit = t.pitch_unit;
        self.rates = rates;
        self.levels = levels;
        self.level = (PITCHTAB[levels[3].clamp(0, 99) as usize] as i32) << 19;
        self.down = true;
        self.advance(0);
    }

    fn getsample(&mut self) -> i32 {
        if self.ix < 3 || (self.ix < 4 && !self.down) {
            if self.rising {
                self.level = self.level.wrapping_add(self.inc);
                if self.level >= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            } else {
                self.level = self.level.wrapping_sub(self.inc);
                if self.level <= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            }
        }
        self.level
    }

    fn keydown(&mut self, d: bool) {
        if self.down != d {
            self.down = d;
            self.advance(if d { 0 } else { 3 });
        }
    }

    fn advance(&mut self, newix: usize) {
        self.ix = newix;
        if newix < 4 {
            self.targetlevel = (PITCHTAB[self.levels[newix].clamp(0, 99) as usize] as i32) << 19;
            self.rising = self.targetlevel > self.level;
            self.inc = (RATETAB[self.rates[newix].clamp(0, 99) as usize] as i32).wrapping_mul(self.unit);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The LFO

/// The DX7's low frequency oscillator and its delay.
#[derive(Clone, Default)]
pub struct Lfo {
    phase: u32,
    delta: u32,
    waveform: u8,
    randstate: u8,
    sync: bool,
    delaystate: u32,
    delayinc: u32,
    delayinc2: u32,
}

impl Lfo {
    pub fn new() -> Lfo {
        Lfo::default()
    }

    /// `params`: speed, delay, pitch mod depth (unused here), amp mod depth (unused), sync, waveform: the unpacked patch's bytes 137 to 142.
    pub fn reset(&mut self, t: &Tables, params: [u8; 6]) {
        let rate = params[0].min(99) as i32;
        let mut sr = if rate == 0 { 1 } else { (165 * rate) >> 6 };
        sr *= if sr < 160 { 11 } else { 11 + ((sr - 160) >> 4) };
        self.delta = t.lfo_unit.wrapping_mul(sr as u32);
        let a = 99 - params[1].min(99) as i32;
        if a == 99 {
            self.delayinc = !0;
            self.delayinc2 = !0;
        } else {
            let mut a = (16 + (a & 15)) << (1 + (a >> 4));
            self.delayinc = t.lfo_unit.wrapping_mul(a as u32);
            a &= 0xff80;
            a = a.max(0x80);
            self.delayinc2 = t.lfo_unit.wrapping_mul(a as u32);
        }
        self.waveform = params[5];
        self.sync = params[4] != 0;
    }

    /// 0 to 1 in Q24, once per block.
    pub fn getsample(&mut self, t: &Tables) -> i32 {
        self.phase = self.phase.wrapping_add(self.delta);
        let p = self.phase;
        match self.waveform {
            0 => {
                let mut x = (p >> 7) as i32;
                x ^= (p >> 31).wrapping_neg() as i32;
                x & ((1 << 24) - 1)
            }
            1 => ((!p ^ (1 << 31)) >> 8) as i32,
            2 => ((p ^ (1 << 31)) >> 8) as i32,
            3 => (((!p) >> 7) & (1 << 24)) as i32,
            4 => (1 << 23) + (t.sin((p >> 8) as i32) >> 1),
            5 => {
                if p < self.delta {
                    self.randstate = ((self.randstate as u32 * 179 + 17) & 0xff) as u8;
                }
                let x = (self.randstate ^ 0x80) as i32;
                (x + 1) << 16
            }
            _ => 1 << 23,
        }
    }

    /// The delay ramp, 0 to 1 in Q24, once per block.
    pub fn getdelay(&mut self) -> i32 {
        let delta = if self.delaystate < (1 << 31) { self.delayinc } else { self.delayinc2 };
        let d = self.delaystate.wrapping_add(delta);
        if d < self.delayinc {
            return 1 << 24;
        }
        self.delaystate = d;
        if d < (1 << 31) {
            0
        } else {
            ((d >> 7) & ((1 << 24) - 1)) as i32
        }
    }

    pub fn keydown(&mut self) {
        if self.sync {
            self.phase = (1 << 31) - 1;
        }
        self.delaystate = 0;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Operator kernels and algorithms

#[derive(Clone, Copy, Default)]
struct OpParams {
    gain: [i32; 2],
    freq: i32,
    phase: i32,
}

fn dgain_of(gain1: i32, gain2: i32) -> i32 {
    gain2.wrapping_sub(gain1).wrapping_add((N >> 1) as i32) >> LG_N
}

fn write(out: &mut i32, v: i32, add: bool) {
    *out = if add { out.wrapping_add(v) } else { v };
}

#[allow(clippy::too_many_arguments)]
fn op_compute(t: &Tables, out: &mut [i32; N], input: &[i32; N], phase0: i32, freq: i32, gain1: i32, gain2: i32, add: bool) {
    let dgain = dgain_of(gain1, gain2);
    let (mut gain, mut phase) = (gain1, phase0);
    for i in 0..N {
        gain = gain.wrapping_add(dgain);
        let y = t.sin(phase.wrapping_add(input[i]));
        write(&mut out[i], mul_shr(y, gain, 24), add);
        phase = phase.wrapping_add(freq);
    }
}

fn op_pure(t: &Tables, out: &mut [i32; N], phase0: i32, freq: i32, gain1: i32, gain2: i32, add: bool) {
    let dgain = dgain_of(gain1, gain2);
    let (mut gain, mut phase) = (gain1, phase0);
    for o in out.iter_mut() {
        gain = gain.wrapping_add(dgain);
        let y = t.sin(phase);
        write(o, mul_shr(y, gain, 24), add);
        phase = phase.wrapping_add(freq);
    }
}

#[allow(clippy::too_many_arguments)]
fn op_fb(t: &Tables, out: &mut [i32; N], phase0: i32, freq: i32, gain1: i32, gain2: i32, fb_buf: &mut [i32; 2], fb_shift: i32, add: bool) {
    let dgain = dgain_of(gain1, gain2);
    let (mut gain, mut phase) = (gain1, phase0);
    let (mut y0, mut y) = (fb_buf[0], fb_buf[1]);
    for o in out.iter_mut() {
        gain = gain.wrapping_add(dgain);
        let scaled_fb = y0.wrapping_add(y) >> (fb_shift + 1);
        y0 = y;
        y = mul_shr(t.sin(phase.wrapping_add(scaled_fb)), gain, 24);
        write(o, y, add);
        phase = phase.wrapping_add(freq);
    }
    fb_buf[0] = y0;
    fb_buf[1] = y;
}

const OUT_BUS_ADD: u8 = 1 << 2;

/// Per operator: bits 0-1 out bus, bit 2 add, bits 4-5 in bus, bit 6 feedback in, bit 7 feedback out (msfa's table).
const ALGORITHMS: [[u8; 6]; 32] = [
    [0xc1, 0x11, 0x11, 0x14, 0x01, 0x14],
    [0x01, 0x11, 0x11, 0x14, 0xc1, 0x14],
    [0xc1, 0x11, 0x14, 0x01, 0x11, 0x14],
    [0x41, 0x11, 0x94, 0x01, 0x11, 0x14],
    [0xc1, 0x14, 0x01, 0x14, 0x01, 0x14],
    [0x41, 0x94, 0x01, 0x14, 0x01, 0x14],
    [0xc1, 0x11, 0x05, 0x14, 0x01, 0x14],
    [0x01, 0x11, 0xc5, 0x14, 0x01, 0x14],
    [0x01, 0x11, 0x05, 0x14, 0xc1, 0x14],
    [0x01, 0x05, 0x14, 0xc1, 0x11, 0x14],
    [0xc1, 0x05, 0x14, 0x01, 0x11, 0x14],
    [0x01, 0x05, 0x05, 0x14, 0xc1, 0x14],
    [0xc1, 0x05, 0x05, 0x14, 0x01, 0x14],
    [0xc1, 0x05, 0x11, 0x14, 0x01, 0x14],
    [0x01, 0x05, 0x11, 0x14, 0xc1, 0x14],
    [0xc1, 0x11, 0x02, 0x25, 0x05, 0x14],
    [0x01, 0x11, 0x02, 0x25, 0xc5, 0x14],
    [0x01, 0x11, 0x11, 0xc5, 0x05, 0x14],
    [0xc1, 0x14, 0x14, 0x01, 0x11, 0x14],
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x14],
    [0x01, 0x14, 0x14, 0xc1, 0x14, 0x14],
    [0xc1, 0x14, 0x14, 0x14, 0x01, 0x14],
    [0xc1, 0x14, 0x14, 0x01, 0x14, 0x04],
    [0xc1, 0x14, 0x14, 0x14, 0x04, 0x04],
    [0xc1, 0x14, 0x14, 0x04, 0x04, 0x04],
    [0xc1, 0x05, 0x14, 0x01, 0x14, 0x04],
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x04],
    [0x04, 0xc1, 0x11, 0x14, 0x01, 0x14],
    [0xc1, 0x14, 0x01, 0x14, 0x04, 0x04],
    [0x04, 0xc1, 0x11, 0x14, 0x04, 0x04],
    [0xc1, 0x14, 0x04, 0x04, 0x04, 0x04],
    [0xc4, 0x04, 0x04, 0x04, 0x04, 0x04],
];

/// Carriers (operators that reach the output) of an algorithm, 0 to 31.
pub fn carriers(algorithm: usize) -> usize {
    ALGORITHMS[algorithm & 31].iter().filter(|&&f| f & 7 == OUT_BUS_ADD).count()
}

/// Adds the six operators' output to `output`.
fn fm_compute(t: &Tables, output: &mut [i32; N], params: &mut [OpParams; 6], algorithm: usize, fb_buf: &mut [i32; 2], fb_shift: i32) {
    const LEVEL_THRESH: i32 = 1120;
    let alg = &ALGORITHMS[algorithm & 31];
    let mut bus = [*output, [0; N], [0; N], [0; N]];
    let mut has = [true, false, false, false];
    for op in 0..6 {
        let flags = alg[op];
        let mut add = flags & OUT_BUS_ADD != 0;
        let inbus = ((flags >> 4) & 3) as usize;
        let outbus = (flags & 3) as usize;
        let p = params[op];
        if p.gain[0] >= LEVEL_THRESH || p.gain[1] >= LEVEL_THRESH {
            if !has[outbus] {
                add = false;
            }
            if inbus == 0 || !has[inbus] {
                if flags & 0xc0 == 0xc0 && fb_shift < 16 {
                    op_fb(t, &mut bus[outbus], p.phase, p.freq, p.gain[0], p.gain[1], fb_buf, fb_shift, add);
                } else {
                    op_pure(t, &mut bus[outbus], p.phase, p.freq, p.gain[0], p.gain[1], add);
                }
            } else {
                let input = bus[inbus];
                op_compute(t, &mut bus[outbus], &input, p.phase, p.freq, p.gain[0], p.gain[1], add);
            }
            has[outbus] = true;
        } else if !add {
            has[outbus] = false;
        }
        params[op].phase = p.phase.wrapping_add(p.freq.wrapping_shl(LG_N));
    }
    *output = bus[0];
}

// ---------------------------------------------------------------------------------------------------------------
// Patch and note

/// Unpacks a DX7 packed voice (128 bytes) to msfa's 156-byte layout, with every field that indexes a table clamped
/// (rates, levels, depths and levels to 99, the algorithm to 31): a bad voice plays wrongly, never crashes.
pub fn unpack_patch(bulk: &[u8; 128]) -> [u8; 156] {
    let mut p = [0u8; 156];
    for op in 0..6 {
        let (b, o) = (op * 17, op * 21);
        p[o..o + 11].copy_from_slice(&bulk[b..b + 11]);
        let curves = bulk[b + 11];
        p[o + 11] = curves & 3;
        p[o + 12] = (curves >> 2) & 3;
        let detune_rs = bulk[b + 12];
        p[o + 13] = detune_rs & 7;
        p[o + 20] = (detune_rs >> 3) & 15;
        let kvs_ams = bulk[b + 13];
        p[o + 14] = kvs_ams & 3;
        p[o + 15] = kvs_ams >> 2;
        p[o + 16] = bulk[b + 14];
        let fcoarse_mode = bulk[b + 15];
        p[o + 17] = fcoarse_mode & 1;
        p[o + 18] = fcoarse_mode >> 1;
        p[o + 19] = bulk[b + 16];
        for i in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 16, 19] {
            p[o + i] = p[o + i].min(99);
        }
    }
    p[126..135].copy_from_slice(&bulk[102..111]);
    for b in &mut p[126..134] {
        *b = (*b).min(99);
    }
    p[134] = p[134].min(31);
    let oks_fb = bulk[111];
    p[135] = oks_fb & 7;
    p[136] = (oks_fb >> 3) & 1;
    p[137..141].copy_from_slice(&bulk[112..116]);
    for b in &mut p[137..141] {
        *b = (*b).min(99);
    }
    let lpms_lfw_lks = bulk[116];
    p[141] = lpms_lfw_lks & 1;
    p[142] = (lpms_lfw_lks >> 1) & 7;
    p[143] = (lpms_lfw_lks >> 4) & 7;
    p[144..155].copy_from_slice(&bulk[117..128]);
    p[155] = 0x3f;
    p
}

/// The LFO's six bytes in an unpacked patch.
pub fn lfo_params(patch: &[u8; 156]) -> [u8; 6] {
    [patch[137], patch[138], patch[139], patch[140], patch[141], patch[142]]
}

fn midinote_to_logfreq(midinote: i32) -> i32 {
    const BASE: i32 = 50_857_777; // (1 << 24) * (log(440) / log(2) - 69/12)
    const STEP: i32 = (1 << 24) / 12;
    BASE + STEP * midinote
}

const COARSEMUL: [i32; 32] = [
    -16777216, 0, 16777216, 26591258, 33554432, 38955489, 43368474, 47099600, 50331648, 53182516, 55732705, 58039632,
    60145690, 62083076, 63876816, 65546747, 67108864, 68576247, 69959732, 71268397, 72509921, 73690858, 74816848,
    75892776, 76922906, 77910978, 78860292, 79773775, 80654032, 81503396, 82323963, 83117622,
];

/// How an operator's frequency is tuned. `Msfa` is msfa's: a ratio from the coarse and fine values, and detune a fixed 0.9 cent a step. `Felucca` is what the
/// FM6 engine of Felucca 1.8.2 was measured to do in the emulator (docs/FM6.md): a model fitted to those measurements and not read from its source, in ratio
/// mode only. Two things: (1) **the detune step depends on the note** (always), and (2) **the DTUN macro** (`dtun`, 0 to 127, the PATCH page's DTUN; 0 is none) spreads
/// the carriers: each carrier is tuned off by `0.0945 * min(dtun, 64)` cents times 0, +1, -1, +2, -2, +3 by its place among the carriers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tuning {
    #[default]
    Msfa,
    Felucca { dtun: u8 },
}

/// The pattern the DTUN spread gives the carriers, by the carrier's place among the algorithm's carriers taken in operator order (measured to within 0.012 cent;
/// on algorithm 32 the carriers are operators 1 to 6, on algorithm 5 operators 1, 3 and 5). A modulator has none (its sidebands sit at exact multiples of its frequency).
const FELUCCA_CARRIER_UNITS: [i32; 6] = [0, 1, -1, 2, -2, 3];
/// One DTUN step is this many cents of the spread's unit (0.0942 to 0.0946 measured at 12, 24 and 48 steps), and the spread stops growing at 64 steps (6.05 cents).
const FELUCCA_DTUN_CENTS: f64 = 0.0945;
const FELUCCA_DTUN_MAX: u8 = 64;
/// ln of Felucca's detune step in cents, as a quadratic in x = (note - 72) / 12, fitted to the steps measured at notes 36 to 108 every 6 (maximum error
/// 0.008 cent a step, which is 0.11 cent at the extreme detune): 1.98 cents a step at note 36, 0.905 at 72, 0.37 at 108. Notes outside 36..108 use the nearest measured note's step.
fn felucca_detune_step_cents(midinote: i32) -> f64 {
    let x = (midinote.clamp(36, 108) - 72) as f64 / 12.0;
    (-0.09936 - 0.28199 * x - 0.00654 * x * x).exp()
}

fn cents_to_logfreq(cents: f64) -> i32 {
    (cents * (1u32 << 24) as f64 / 1200.0).round() as i32
}

/// `carrier` is the operator's place among the algorithm's carriers (0 for the first), or `None` for a modulator.
fn osc_freq(midinote: i32, mode: i32, coarse: i32, fine: i32, detune: i32, carrier: Option<usize>, tuning: Tuning) -> i32 {
    if mode == 0 {
        let mut logfreq = midinote_to_logfreq(midinote);
        logfreq += COARSEMUL[(coarse & 31) as usize];
        if fine != 0 {
            // (1 << 24) / log(2)
            logfreq += (24204406.323123 * (1.0 + 0.01 * fine as f64).ln() + 0.5).floor() as i32;
        }
        match tuning {
            // measured at 7.213 Hz per count at 9600 Hz (msfa's note)
            Tuning::Msfa => logfreq + 12606 * (detune - 7),
            Tuning::Felucca { dtun } => {
                let steps = (detune.clamp(0, 14) - 7) as f64;
                let unit = FELUCCA_DTUN_CENTS * dtun.min(FELUCCA_DTUN_MAX) as f64;
                logfreq + cents_to_logfreq(steps * felucca_detune_step_cents(midinote) + carrier.map_or(0, |c| FELUCCA_CARRIER_UNITS[c.min(5)]) as f64 * unit)
            }
        }
    } else {
        // ((1 << 24) * log(10) / log(2) * .01) << 3
        let mut logfreq = (4458616 * ((coarse & 3) * 100 + fine)) >> 3;
        if detune > 7 {
            logfreq += 13457 * (detune - 7);
        }
        logfreq
    }
}

const VELOCITY_DATA: [u8; 64] = [
    0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163, 166, 170, 173, 174, 178, 181, 184, 186,
    189, 190, 194, 196, 198, 200, 202, 205, 206, 209, 211, 214, 216, 218, 220, 222, 224, 225, 227, 229, 230, 232, 233,
    235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248, 249, 250, 251, 252, 253, 254,
];

/// The velocity's change of an operator's level, in microsteps.
fn scale_velocity(velocity: i32, sensitivity: i32) -> i32 {
    let vel = velocity.clamp(0, 127);
    let vel_value = VELOCITY_DATA[(vel >> 1) as usize] as i32 - 239;
    ((sensitivity * vel_value + 7) >> 3) << 4
}

fn scale_rate(midinote: i32, sensitivity: i32) -> i32 {
    let x = (midinote / 3 - 7).clamp(0, 31);
    (sensitivity * x) >> 3
}

const EXP_SCALE_DATA: [u8; 33] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66, 80, 94, 110, 126, 142, 158, 174, 190, 206,
    222, 238, 250,
];

fn scale_curve(group: i32, depth: i32, curve: i32) -> i32 {
    let scale = if curve == 0 || curve == 3 {
        (group * depth * 329) >> 12
    } else {
        let raw_exp = EXP_SCALE_DATA[group.clamp(0, 32) as usize] as i32;
        (raw_exp * depth * 329) >> 15
    };
    if curve < 2 {
        -scale
    } else {
        scale
    }
}

fn scale_level(midinote: i32, break_pt: i32, left_depth: i32, right_depth: i32, left_curve: i32, right_curve: i32) -> i32 {
    let offset = midinote - break_pt - 17;
    if offset >= 0 {
        scale_curve(offset / 3, right_depth, right_curve)
    } else {
        scale_curve((-offset) / 3, left_depth, left_curve)
    }
}

const PITCHMODSENS: [u8; 8] = [0, 10, 20, 33, 55, 92, 153, 255];

/// One sounding note: six envelopes, the pitch envelope and the operator state.
#[derive(Clone)]
pub struct Voice {
    env: [Env; 6],
    params: [OpParams; 6],
    pitchenv: PitchEnv,
    basepitch: [i32; 6],
    fb_buf: [i32; 2],
    fb_shift: i32,
    algorithm: usize,
    pitchmoddepth: i32,
    pitchmodsens: i32,
}

/// The MIDI pitch wheel's centre.
pub const PITCH_CENTRE: i32 = 0x2000;

impl Voice {
    /// A note of an unpacked patch (`unpack_patch`), for a MIDI note 0 to 127 and velocity 0 to 127.
    pub fn new(t: &Tables, patch: &[u8; 156], midinote: i32, velocity: i32) -> Voice {
        Voice::with_tuning(t, patch, midinote, velocity, Tuning::Msfa)
    }

    /// `new`, tuned as `tuning` says.
    pub fn with_tuning(t: &Tables, patch: &[u8; 156], midinote: i32, velocity: i32, tuning: Tuning) -> Voice {
        let midinote = midinote.clamp(0, 127);
        let g = |i: usize| patch[i] as i32;
        let mut v = Voice {
            env: Default::default(),
            params: [OpParams::default(); 6],
            pitchenv: PitchEnv::default(),
            basepitch: [0; 6],
            fb_buf: [0; 2],
            fb_shift: 0,
            algorithm: 0,
            pitchmoddepth: 0,
            pitchmodsens: 0,
        };
        // each carrier's place among the carriers, taken in operator order (the record holds operator 6 first): Felucca's tuning uses it
        let alg = (g(134) & 31) as usize;
        let mut carrier_place = [None; 6];
        let mut seen = 0;
        for opnum in 1..=6usize {
            if ALGORITHMS[alg][6 - opnum] & 7 == OUT_BUS_ADD {
                carrier_place[6 - opnum] = Some(seen);
                seen += 1;
            }
        }
        for (op, place) in carrier_place.iter().enumerate() {
            let off = op * 21;
            let rates = [g(off), g(off + 1), g(off + 2), g(off + 3)];
            let levels = [g(off + 4), g(off + 5), g(off + 6), g(off + 7)];
            let mut outlevel = scale_out_level(g(off + 16));
            outlevel += scale_level(midinote, g(off + 8), g(off + 9), g(off + 10), g(off + 11), g(off + 12));
            outlevel = outlevel.min(127) << 5;
            outlevel += scale_velocity(velocity, g(off + 15));
            outlevel = outlevel.max(0);
            let rate_scaling = scale_rate(midinote, g(off + 13));
            v.env[op].init(rates, levels, outlevel, rate_scaling);
            v.basepitch[op] = osc_freq(midinote, g(off + 17), g(off + 18), g(off + 19), g(off + 20), *place, tuning);
        }
        let rates = [g(126), g(127), g(128), g(129)];
        let levels = [g(130), g(131), g(132), g(133)];
        v.pitchenv.set(t, rates, levels);
        v.algorithm = (g(134) & 31) as usize;
        let feedback = g(135);
        v.fb_shift = if feedback != 0 { 8 - feedback } else { 16 };
        v.pitchmoddepth = (g(139) * 165) >> 6;
        v.pitchmodsens = PITCHMODSENS[(g(143) & 7) as usize] as i32;
        v
    }

    /// Adds one block of this note to `buf`. `lfo_val` and `lfo_delay` come from the shared `Lfo` once per block;
    /// `pitch_bend` is the MIDI wheel, 0 to 16383 (`PITCH_CENTRE` is none): a 3 semitone range, as msfa fixes it.
    pub fn compute(&mut self, t: &Tables, buf: &mut [i32; N], lfo_val: i32, lfo_delay: i32, pitch_bend: i32) {
        let mut pitchmod = self.pitchenv.getsample();
        let pmd = self.pitchmoddepth.wrapping_mul(lfo_delay) as u32;
        let senslfo = self.pitchmodsens.wrapping_mul(lfo_val - (1 << 23));
        pitchmod = pitchmod.wrapping_add(((pmd as i64 * senslfo as i64) >> 39) as i32);
        pitchmod = pitchmod.wrapping_add((pitch_bend - PITCH_CENTRE) << 9);
        for op in 0..6 {
            self.params[op].gain[0] = self.params[op].gain[1];
            let level = self.env[op].getsample();
            self.params[op].gain[1] = t.exp2(level.wrapping_sub(14 * (1 << 24)));
            self.params[op].freq = t.freq(self.basepitch[op].wrapping_add(pitchmod));
        }
        fm_compute(t, buf, &mut self.params, self.algorithm, &mut self.fb_buf, self.fb_shift);
    }

    pub fn key_up(&mut self) {
        for e in &mut self.env {
            e.keydown(false);
        }
        self.pitchenv.keydown(false);
    }
}

/// A block's sample (msfa's mix scale) as 16 bits.
pub fn to_i16(x: i32) -> i16 {
    let val = x >> 4;
    if val < -(1 << 24) {
        i16::MIN
    } else if val >= (1 << 24) {
        i16::MAX
    } else {
        (val >> 9) as i16
    }
}

// ---------------------------------------------------------------------------------------------------------------
// A small polyphonic player

struct Slot {
    voice: Voice,
    midi_note: u8,
    keydown: bool,
    live: bool,
    quiet: u32,
}

/// Up to `VOICES` notes of one patch, mixed to 16 bits mono, block by block (msfa's `SynthUnit` without its filter,
/// sustain pedal and MIDI parser). A note is free once it has been released and has stayed quiet for a while.
pub struct Synth {
    t: Tables,
    lfo: Lfo,
    patch: [u8; 156],
    slots: Vec<Slot>,
    current: usize,
    pitch_bend: i32,
    tuning: Tuning,
    spare: Vec<i16>,
}

impl Synth {
    pub fn new(sample_rate: f64, voice: &[u8; 128]) -> Result<Synth, &'static str> {
        let t = Tables::new(sample_rate)?;
        let patch = unpack_patch(voice);
        let mut lfo = Lfo::new();
        lfo.reset(&t, lfo_params(&patch));
        let blank = Voice::new(&t, &patch, 60, 100);
        let slots = (0..VOICES).map(|_| Slot { voice: blank.clone(), midi_note: 0, keydown: false, live: false, quiet: 0 }).collect();
        Ok(Synth { t, lfo, patch, slots, current: 0, pitch_bend: PITCH_CENTRE, tuning: Tuning::Msfa, spare: Vec::new() })
    }

    pub fn set_voice(&mut self, voice: &[u8; 128]) {
        self.patch = unpack_patch(voice);
        self.lfo.reset(&self.t, lfo_params(&self.patch));
    }

    /// How the notes played from now on are tuned (`Tuning`).
    pub fn set_tuning(&mut self, tuning: Tuning) {
        self.tuning = tuning;
    }

    pub fn set_pitch_bend(&mut self, bend: i32) {
        self.pitch_bend = bend.clamp(0, 0x3fff);
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(note);
        }
        for k in 0..VOICES {
            let i = (self.current + k) % VOICES;
            if !self.slots[i].keydown {
                self.current = (i + 1) % VOICES;
                self.lfo.keydown();
                self.slots[i] = Slot {
                    voice: Voice::with_tuning(&self.t, &self.patch, note as i32, velocity as i32, self.tuning),
                    midi_note: note,
                    keydown: true,
                    live: true,
                    quiet: 0,
                };
                return;
            }
        }
    }

    pub fn note_off(&mut self, note: u8) {
        for s in &mut self.slots {
            if s.live && s.keydown && s.midi_note == note {
                s.voice.key_up();
                s.keydown = false;
            }
        }
    }

    pub fn all_off(&mut self) {
        for s in &mut self.slots {
            if s.keydown {
                s.voice.key_up();
                s.keydown = false;
            }
        }
    }

    /// One block of `N` samples.
    pub fn render_block(&mut self) -> [i16; N] {
        let mut mix = [0i32; N];
        let lfo_val = self.lfo.getsample(&self.t);
        let lfo_delay = self.lfo.getdelay();
        for s in self.slots.iter_mut().filter(|s| s.live) {
            let mut b = [0i32; N];
            s.voice.compute(&self.t, &mut b, lfo_val, lfo_delay, self.pitch_bend);
            for (m, x) in mix.iter_mut().zip(b) {
                *m = m.wrapping_add(x);
            }
            if !s.keydown {
                // released: free after about 0.4 s below the 16-bit noise floor
                s.quiet = if b.iter().all(|&x| to_i16(x).unsigned_abs() < 2) { s.quiet + 1 } else { 0 };
                if s.quiet > 280 {
                    s.live = false;
                }
            }
        }
        let mut out = [0i16; N];
        for (o, m) in out.iter_mut().zip(mix) {
            *o = to_i16(m);
        }
        out
    }

    /// Fills `out` (any length) with mono samples.
    pub fn render(&mut self, out: &mut [i16]) {
        let mut at = 0;
        let take = self.spare.len().min(out.len());
        out[..take].copy_from_slice(&self.spare[..take]);
        self.spare.drain(..take);
        at += take;
        while at < out.len() {
            let blk = self.render_block();
            let n = (out.len() - at).min(N);
            out[at..at + n].copy_from_slice(&blk[..n]);
            self.spare.extend_from_slice(&blk[n..]);
            at += n;
        }
    }

    pub fn live_voices(&self) -> usize {
        self.slots.iter().filter(|s| s.live).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(alg: u8) -> [u8; 128] {
        let mut v = [0u8; 128];
        for op in 0..6 {
            let o = &mut v[op * 17..op * 17 + 17];
            o[..4].copy_from_slice(&[90, 60, 50, 60]);
            o[4..8].copy_from_slice(&[99, 80, 60, 0]);
            o[14] = 90;
            o[15] = 2; // coarse 1
        }
        v[106..110].copy_from_slice(&[50, 50, 50, 50]);
        v[102..106].copy_from_slice(&[99, 99, 99, 99]);
        v[110] = alg;
        v[111] = 3;
        v[117] = 24;
        v
    }

    #[test]
    fn a_note_sounds_and_dies() {
        let mut s = Synth::new(44100.0, &voice(4)).unwrap();
        s.note_on(60, 100);
        let mut out = vec![0i16; 44100 / 2];
        s.render(&mut out);
        assert!(out.iter().any(|&x| x.abs() > 100), "no sound");
        s.note_off(60);
        let mut tail = vec![0i16; 44100 * 8];
        s.render(&mut tail);
        assert_eq!(s.live_voices(), 0, "the note never freed its slot");
    }

    #[test]
    fn is_deterministic() {
        let run = || {
            let mut s = Synth::new(44100.0, &voice(31)).unwrap();
            s.note_on(48, 90);
            s.note_on(67, 60);
            let mut out = vec![0i16; 5000];
            s.render(&mut out);
            out
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn render_in_odd_pieces_equals_one_piece() {
        let mut a = Synth::new(44100.0, &voice(7)).unwrap();
        let mut b = Synth::new(44100.0, &voice(7)).unwrap();
        a.note_on(60, 100);
        b.note_on(60, 100);
        let mut whole = vec![0i16; 1000];
        a.render(&mut whole);
        let mut parts = vec![0i16; 1000];
        let mut at = 0;
        for n in [1usize, 63, 64, 65, 7, 300, 500] {
            let n = n.min(1000 - at);
            b.render(&mut parts[at..at + n]);
            at += n;
        }
        assert_eq!(whole[..at], parts[..at]);
    }

    #[test]
    fn bad_voices_never_panic() {
        // every byte pattern in turn and random garbage: an engine fed untrusted data (rule 5)
        let mut x = 0x1234_5678_9abc_def0u64;
        for round in 0..300 {
            let mut v = [0u8; 128];
            for b in v.iter_mut() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *b = if round < 3 { [0x00, 0xff, 0x80][round] } else { (x >> 24) as u8 };
            }
            let mut s = Synth::new(44100.0, &v).unwrap();
            s.set_pitch_bend(((x >> 8) & 0xffff) as i32);
            s.note_on((x >> 3) as u8, (x >> 11) as u8);
            let mut out = [0i16; 700];
            s.render(&mut out);
            s.note_off(0);
            s.all_off();
            s.render(&mut out);
        }
    }

    #[test]
    fn felucca_tuning_is_the_measured_model() {
        // the detune step: 1.98 cents at note 36, 0.905 at 72, 0.37 at 108 (the measured values), clamped outside
        assert!((felucca_detune_step_cents(36) - 1.9812).abs() < 0.01);
        assert!((felucca_detune_step_cents(72) - 0.9054).abs() < 0.01);
        assert!((felucca_detune_step_cents(108) - 0.3677).abs() < 0.01);
        assert_eq!(felucca_detune_step_cents(0), felucca_detune_step_cents(36));
        assert_eq!(felucca_detune_step_cents(127), felucca_detune_step_cents(108));
        let cents_of = |x: i32| x as f64 * 1200.0 / (1u32 << 24) as f64;
        let base = osc_freq(60, 0, 1, 0, 7, None, Tuning::Msfa);
        let cents = |carrier: Option<usize>, note: i32, detune: i32, dtun: u8| cents_of(osc_freq(note, 0, 1, 0, detune, carrier, Tuning::Felucca { dtun }) - osc_freq(note, 0, 1, 0, 7, None, Tuning::Msfa));
        // DTUN 0: no spread, whatever the note; the spread grows by 0.0945 cent a step to 64 and stops there, and a modulator has none
        for note in [36, 60, 96] {
            for place in 0..6 {
                assert!(cents(Some(place), note, 7, 0).abs() < 0.001, "no spread at DTUN 0");
            }
            for (place, units) in FELUCCA_CARRIER_UNITS.iter().enumerate() {
                assert!((cents(Some(place), note, 7, 30) - *units as f64 * 0.0945 * 30.0).abs() < 0.001, "carrier {place} note {note}");
                assert!((cents(Some(place), note, 7, 64) - *units as f64 * 6.048).abs() < 0.001);
                assert_eq!(cents(Some(place), note, 7, 64), cents(Some(place), note, 7, 127), "the spread stops at 64");
            }
            assert!(cents(None, note, 7, 100).abs() < 0.001, "a modulator has no spread");
        }
        // the carriers' places: algorithm 32 has six, algorithm 5 has operators 1, 3 and 5 (the record holds operator 6 first)
        let t = Tables::new(44100.0).unwrap();
        let basepitch = |alg: u8, dtun: u8| { let mut patch = unpack_patch(&voice(alg)); for op in 0..6 { patch[op * 21 + 20] = 7; } Voice::with_tuning(&t, &patch, 60, 100, Tuning::Felucca { dtun }).basepitch };   // the centre detune
        let (a32, a5) = (basepitch(31, 30), basepitch(4, 30));
        let m = |n: usize, v: [i32; 6]| cents_of(v[6 - n] - base);
        for (n, units) in [(1, 0), (2, 1), (3, -1), (4, 2), (5, -2), (6, 3)] { assert!((m(n, a32) - units as f64 * 2.835).abs() < 0.01, "alg 32 operator {n}"); }
        for (n, units) in [(1, 0), (3, 1), (5, -1)] { assert!((m(n, a5) - units as f64 * 2.835).abs() < 0.01, "alg 5 carrier {n}"); }
        for n in [2, 4, 6] { assert!(m(n, a5).abs() < 0.01, "alg 5 modulator {n}"); }
        // a detune step is larger on a low note, and detune 14 against 7 moves seven steps
        assert!(cents(Some(0), 36, 14, 0) > cents(Some(0), 96, 14, 0));
        assert!((cents(Some(0), 72, 14, 0) - 7.0 * 0.9054).abs() < 0.02);
        // msfa's own tuning is untouched, and fixed-frequency mode is the same under both
        assert_eq!(osc_freq(60, 0, 1, 0, 10, Some(3), Tuning::Msfa), osc_freq(60, 0, 1, 0, 7, None, Tuning::Msfa) + 3 * 12606);
        assert_eq!(osc_freq(60, 1, 1, 50, 10, Some(4), Tuning::Msfa), osc_freq(60, 1, 1, 50, 10, Some(4), Tuning::Felucca { dtun: 40 }));
        // it never panics, detune and DTUN out of range included, and plays
        for d in [-5, 0, 14, 15, 99] {
            for dt in [0u8, 64, 255] {
                let _ = osc_freq(60, 0, 1, 0, d, Some(5), Tuning::Felucca { dtun: dt });
            }
        }
        let mut s = Synth::new(44100.0, &voice(31)).unwrap();
        s.set_tuning(Tuning::Felucca { dtun: 30 });
        s.note_on(60, 100);
        let mut out = [0i16; 300];
        s.render(&mut out);
        assert!(out.iter().any(|&x| x != 0));
    }

    #[test]
    fn refuses_a_bad_sample_rate() {
        assert!(Tables::new(0.0).is_err());
        assert!(Tables::new(f64::NAN).is_err());
        assert!(Tables::new(1e9).is_err());
        assert!(Tables::new(44100.0).is_ok());
    }

    #[test]
    fn carriers_of_the_algorithms() {
        // the DX7's: algorithm 1 has two carriers, 5 three, 32 six
        assert_eq!(carriers(0), 2);
        assert_eq!(carriers(4), 3);
        assert_eq!(carriers(31), 6);
    }
}
