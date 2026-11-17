// SPDX-License-Identifier: MIT
// Copyright 2026 FM1-NTT. Written fresh for this repository: no code here is taken from any firmware or from Neumetik.

//! A sampler engine: the other half of what community firmwares do besides the voice engine (`fm6`).
//!
//! Chops, drum pads and song playback (PIGEON, Ravel, Rhythmer, Felucca's sampler) are samples played at a pitch, looped, mixed in
//! a few voices and scaled by a master. This module is that, as plain functions of integers and a few tables, so it is deterministic
//! and a firmware's behaviour can be measured against it in the emulator the way `fm6` was (`docs/SAMPLER.md`).
//!
//! **What is measured and what is not.** Nothing here is yet measured against a firmware: the pitch law (equal temperament about a
//! root key), the linear velocity, the sqrt(n) mixer rule and the interpolation are the plain defaults, each one a knob
//! (`Interp`, `Sampler::set_poly_limit`, `velocity_gain`) to be set to what a firmware turns out to do. Nothing ran on a real FM-1.
//!
//! Firmware data is untrusted (rule 5): every reader bounds its sizes and returns an `Error`; playback never indexes out of range.

use crate::{Error, Result};

/// The largest sample the readers accept, in frames: 16 Mi (a 6 minute mono recording at 48 kHz is 17 Mi; the FM-1's own pool is under 1 Mi).
pub const MAX_FRAMES: usize = 16 << 20;
/// Voices a `Sampler` can hold at once.
pub const MAX_VOICES: usize = 32;

// ---------------------------------------------------------------------------------------------------------------------------
// IMA ADPCM (the 4-bit codec of WAV format 0x11 and of many firmware sample pools; the step table is the public IMA/DVI one)
// ---------------------------------------------------------------------------------------------------------------------------

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209,
    230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749,
    3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385,
    24623, 27086, 29794, 32767,
];
const INDEX_ADJUST: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// The decoder's state: the last sample and the position in the step table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdpcmState {
    pub predictor: i16,
    pub index: u8,
}

impl AdpcmState {
    /// One 4-bit code to one sample.
    pub fn decode(&mut self, nibble: u8) -> i16 {
        let step = STEPS[self.index.min(88) as usize];
        let n = (nibble & 15) as i32;
        let mut diff = step >> 3;
        if n & 4 != 0 { diff += step; }
        if n & 2 != 0 { diff += step >> 1; }
        if n & 1 != 0 { diff += step >> 2; }
        let p = if n & 8 != 0 { self.predictor as i32 - diff } else { self.predictor as i32 + diff };
        self.predictor = p.clamp(-32768, 32767) as i16;
        self.index = (self.index as i32 + INDEX_ADJUST[(n & 7) as usize]).clamp(0, 88) as u8;
        self.predictor
    }

    /// One sample to its 4-bit code, updating the state as the decoder will.
    pub fn encode(&mut self, sample: i16) -> u8 {
        let step = STEPS[self.index.min(88) as usize];
        let mut diff = sample as i32 - self.predictor as i32;
        let mut code = 0u8;
        if diff < 0 { code = 8; diff = -diff; }
        let mut s = step;
        if diff >= s { code |= 4; diff -= s; }
        s >>= 1;
        if diff >= s { code |= 2; diff -= s; }
        s >>= 1;
        if diff >= s { code |= 1; }
        self.decode(code);
        code
    }
}

/// Decodes WAV-style IMA ADPCM, mono: blocks of `block_align` bytes, each a 4 byte header (predictor, index, 0) then two codes a byte, low nibble first.
pub fn decode_ima_blocks(data: &[u8], block_align: usize) -> Result<Vec<i16>> {
    if !(5..=65536).contains(&block_align) {
        return Err(Error::BadImage(format!("ADPCM block size {block_align}")));
    }
    let mut out = Vec::new();
    for block in data.chunks(block_align) {
        if block.len() < 4 { break; }
        let mut st = AdpcmState { predictor: i16::from_le_bytes([block[0], block[1]]), index: block[2].min(88) };
        out.push(st.predictor);
        for &b in &block[4..] {
            out.push(st.decode(b & 15));
            out.push(st.decode(b >> 4));
        }
        if out.len() > MAX_FRAMES {
            return Err(Error::BadImage("ADPCM data is too long".into()));
        }
    }
    Ok(out)
}

/// The encoder for the same blocks (a test and tooling aid): `samples_per_block` is `(block_align - 4) * 2 + 1`.
pub fn encode_ima_blocks(samples: &[i16], block_align: usize) -> Vec<u8> {
    let per = (block_align.max(5) - 4) * 2 + 1;
    let mut out = Vec::new();
    for chunk in samples.chunks(per) {
        let mut st = AdpcmState { predictor: chunk[0], index: 0 };
        out.extend_from_slice(&chunk[0].to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        let mut byte = 0u8;
        for (i, &s) in chunk[1..].iter().enumerate() {
            let c = st.encode(s);
            if i % 2 == 0 { byte = c } else { out.push(byte | (c << 4)) }
        }
        if chunk.len() % 2 == 0 { out.push(byte) }
        while out.len() % block_align != 0 { out.push(0) }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------------
// A sample, and the WAV reader
// ---------------------------------------------------------------------------------------------------------------------------

/// A mono sample at its own rate, with an optional forward loop `[start, end)` in frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub rate: u32,
    pub data: Vec<i16>,
    pub loop_range: Option<(usize, usize)>,
}

impl Sample {
    pub fn new(rate: u32, data: Vec<i16>) -> Sample {
        Sample { rate: rate.clamp(1000, 384_000), data, loop_range: None }
    }

    /// Sets the loop; a range that is empty or outside the data is dropped (no loop).
    pub fn with_loop(mut self, start: usize, end: usize) -> Sample {
        self.loop_range = if start < end && end <= self.data.len() { Some((start, end)) } else { None };
        self
    }
}

fn le16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]))
}
fn le32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?]))
}

/// Reads a WAV file: 8 or 16-bit PCM, or IMA ADPCM, one or two channels (two are averaged to mono), and a `smpl` chunk's first loop.
pub fn parse_wav(bytes: &[u8]) -> Result<Sample> {
    let bad = |m: &str| Error::BadImage(format!("WAV: {m}"));
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad("not a RIFF WAVE file"));
    }
    let (mut fmt, mut data, mut smpl) = (None, None, None);
    let mut o = 12usize;
    while o + 8 <= bytes.len() {
        let id = &bytes[o..o + 4];
        let size = le32(bytes, o + 4).ok_or_else(|| bad("cut chunk"))? as usize;
        let body = o + 8;
        let end = body.saturating_add(size).min(bytes.len());
        match id {
            b"fmt " => fmt = Some(&bytes[body..end]),
            b"data" => data = Some(&bytes[body..end]),
            b"smpl" => smpl = Some(&bytes[body..end]),
            _ => {}
        }
        o = body.saturating_add(size).saturating_add(size & 1);
    }
    let fmt = fmt.ok_or_else(|| bad("no fmt chunk"))?;
    let data = data.ok_or_else(|| bad("no data chunk"))?;
    let tag = le16(fmt, 0).ok_or_else(|| bad("short fmt"))?;
    let channels = le16(fmt, 2).ok_or_else(|| bad("short fmt"))? as usize;
    let rate = le32(fmt, 4).ok_or_else(|| bad("short fmt"))?;
    let align = le16(fmt, 12).ok_or_else(|| bad("short fmt"))? as usize;
    let bits = le16(fmt, 14).ok_or_else(|| bad("short fmt"))?;
    if !(1..=2).contains(&channels) {
        return Err(bad("only one or two channels"));
    }
    if !(1000..=384_000).contains(&rate) {
        return Err(bad("sample rate out of range"));
    }
    let frames: Vec<i16> = match (tag, bits) {
        (1, 16) => data.chunks_exact(2 * channels).map(|c| if channels == 1 { i16::from_le_bytes([c[0], c[1]]) } else { ((i16::from_le_bytes([c[0], c[1]]) as i32 + i16::from_le_bytes([c[2], c[3]]) as i32) / 2) as i16 }).collect(),
        (1, 8) => data.chunks_exact(channels).map(|c| (((c.iter().map(|&b| b as i32).sum::<i32>() / channels as i32) - 128) << 8) as i16).collect(),
        (0x11, 4) if channels == 1 => decode_ima_blocks(data, align)?,
        _ => return Err(bad("format not read (8 or 16-bit PCM, or mono IMA ADPCM)")),
    };
    if frames.len() > MAX_FRAMES {
        return Err(bad("too long"));
    }
    let mut s = Sample::new(rate, frames);
    // smpl: 36 bytes of header, then loops of 24 bytes (id, type, start, end, fraction, count); the end is inclusive
    if let Some(sm) = smpl {
        if le32(sm, 28).unwrap_or(0) > 0 {
            if let (Some(a), Some(b)) = (le32(sm, 44), le32(sm, 48)) {
                s = s.with_loop(a as usize, (b as usize).saturating_add(1));
            }
        }
    }
    Ok(s)
}

// ---------------------------------------------------------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------------------------------------------------------

/// How a voice reads between two sample points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Interp {
    /// The nearest point (what a plain integer-stepping player does).
    Nearest,
    #[default]
    Linear,
    /// Four points, Catmull-Rom.
    Cubic,
}

/// Velocity 1 to 127 to a gain: linear, `v / 127` (the plain default; a firmware's curve is to be measured).
pub fn velocity_gain(velocity: u8) -> f64 {
    velocity.min(127) as f64 / 127.0
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    live: bool,
    releasing: bool,
    note: u8,
    gain: f64,
    pos: f64,
    step: f64,
    age: u64,
    /// 1 at the start of a release, falling to 0 over `release_len` output frames
    fade: f64,
}

const IDLE: Voice = Voice { live: false, releasing: false, note: 0, gain: 0.0, pos: 0.0, step: 0.0, age: 0, fade: 1.0 };

/// One sample played from the keys: the pitch of a key relative to `root`, a forward loop, a polyphony limit, a power-constant mixer beyond it.
#[derive(Clone, Debug)]
pub struct Sampler {
    sample: Sample,
    out_rate: f64,
    root: u8,
    interp: Interp,
    voices: [Voice; MAX_VOICES],
    max_voices: usize,
    poly_limit: usize,
    release_frames: f64,
    master: f64,
    clock: u64,
    /// notes held as a one-shot play to the end; a looped sample plays until released
    one_shot_ignores_release: bool,
}

impl Sampler {
    /// A sampler for `sample` at the output rate `out_rate` (8000 to 192000 Hz), with `root` (a MIDI note) sounding the sample at its own pitch.
    pub fn new(sample: Sample, out_rate: f64, root: u8) -> Result<Sampler> {
        if !(8000.0..=192_000.0).contains(&out_rate) {
            return Err(Error::BadImage("sampler: output rate out of range".into()));
        }
        Ok(Sampler {
            sample,
            out_rate,
            root: root.min(127),
            interp: Interp::default(),
            voices: [IDLE; MAX_VOICES],
            max_voices: 8,
            poly_limit: 4,
            release_frames: out_rate * 0.005,
            master: 1.0,
            clock: 0,
            one_shot_ignores_release: true,
        })
    }

    pub fn set_interp(&mut self, i: Interp) {
        self.interp = i;
    }
    /// How many notes sound at once (1 to `MAX_VOICES`); a further note takes the oldest voice.
    pub fn set_voices(&mut self, n: usize) {
        self.max_voices = n.clamp(1, MAX_VOICES);
    }
    /// Beyond this many sounding voices the mixer holds the total power constant (amplitude sqrt(limit / n)); `usize::MAX` for no rule.
    pub fn set_poly_limit(&mut self, n: usize) {
        self.poly_limit = n.max(1);
    }
    pub fn set_master(&mut self, gain: f64) {
        self.master = gain.clamp(0.0, 16.0);
    }
    /// The release fade, in seconds (0 to 1): a click-free cut.
    pub fn set_release(&mut self, seconds: f64) {
        self.release_frames = (seconds.clamp(0.0, 1.0) * self.out_rate).max(1.0);
    }
    /// With true (the default) a sample with no loop plays to its end whatever the key does, as a drum hit does; with false a release stops it.
    pub fn set_one_shot_ignores_release(&mut self, on: bool) {
        self.one_shot_ignores_release = on;
    }

    pub fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.live).count()
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(note);
        }
        let note = note.min(127);
        let ratio = 2f64.powf((note as f64 - self.root as f64) / 12.0) * self.sample.rate as f64 / self.out_rate;
        let mut slot = self.voices[..self.max_voices].iter().position(|v| !v.live);
        if slot.is_none() {
            slot = (0..self.max_voices).min_by_key(|&i| self.voices[i].age);
        }
        if let Some(i) = slot {
            self.clock += 1;
            self.voices[i] = Voice { live: !self.sample.data.is_empty(), releasing: false, note, gain: velocity_gain(velocity), pos: 0.0, step: ratio, age: self.clock, fade: 1.0 };
        }
    }

    pub fn note_off(&mut self, note: u8) {
        let looped = self.sample.loop_range.is_some();
        for v in self.voices.iter_mut().filter(|v| v.live && !v.releasing && v.note == note.min(127)) {
            if looped || !self.one_shot_ignores_release {
                v.releasing = true;
            }
        }
    }

    pub fn all_off(&mut self) {
        self.voices = [IDLE; MAX_VOICES];
    }

    fn read(&self, pos: f64) -> f64 {
        let d = &self.sample.data;
        let i = pos.floor() as isize;
        let f = pos - i as f64;
        let at = |k: isize| -> f64 { d[k.clamp(0, d.len() as isize - 1) as usize] as f64 };
        match self.interp {
            Interp::Nearest => at(pos.round() as isize),
            Interp::Linear => at(i) * (1.0 - f) + at(i + 1) * f,
            Interp::Cubic => {
                let (a, b, c, e) = (at(i - 1), at(i), at(i + 1), at(i + 2));
                b + 0.5 * f * (c - a + f * (2.0 * a - 5.0 * b + 4.0 * c - e + f * (3.0 * (b - c) + e - a)))
            }
        }
    }

    /// Adds mono output to `out` (replacing it).
    pub fn render(&mut self, out: &mut [i16]) {
        let n = self.sample.data.len();
        let poly = if self.sounding() > self.poly_limit { (self.poly_limit as f64 / self.sounding() as f64).sqrt() } else { 1.0 };
        let fade_step = 1.0 / self.release_frames;
        for o in out.iter_mut() {
            let mut acc = 0.0;
            for i in 0..MAX_VOICES {
                if !self.voices[i].live {
                    continue;
                }
                let v = self.voices[i];
                acc += self.read(v.pos) * v.gain * v.fade;
                let v = &mut self.voices[i];
                v.pos += v.step;
                if let Some((s, e)) = self.sample.loop_range {
                    if v.pos >= e as f64 {
                        v.pos = s as f64 + (v.pos - e as f64) % (e - s) as f64;
                    }
                } else if v.pos >= n as f64 {
                    v.live = false;
                }
                if v.releasing {
                    v.fade -= fade_step;
                    if v.fade <= 0.0 {
                        v.live = false;
                    }
                }
            }
            *o = (acc * poly * self.master).round().clamp(-32768.0, 32767.0) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, frames: usize, amp: f64) -> Vec<i16> {
        (0..frames).map(|i| (amp * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin()) as i16).collect()
    }

    /// Frequency by upward zero crossings.
    fn hz_of(x: &[i16], rate: f64) -> f64 {
        let mut first = None;
        let mut last = 0;
        let mut count = 0;
        for i in 1..x.len() {
            if x[i - 1] < 0 && x[i] >= 0 {
                if first.is_none() { first = Some(i) } else { count += 1 }
                last = i;
            }
        }
        count as f64 * rate / (last - first.unwrap()) as f64
    }

    fn rms(x: &[i16]) -> f64 {
        (x.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    #[test]
    fn adpcm_round_trips_within_its_noise() {
        let s = sine(22050, 440.0, 4000, 12000.0);
        let enc = encode_ima_blocks(&s, 256);
        let dec = decode_ima_blocks(&enc, 256).unwrap();
        // the last block is padded to its full length (a WAV says the true length in its fact chunk)
        assert!(dec.len() >= s.len() && dec.len() < s.len() + 512);
        let err: Vec<i16> = s.iter().zip(&dec).map(|(a, b)| a.wrapping_sub(*b)).collect();
        assert!(rms(&err) < rms(&s) * 0.08, "error {} of {}", rms(&err), rms(&s));
        assert!(decode_ima_blocks(&enc, 2).is_err());
    }

    #[test]
    fn adpcm_state_never_leaves_its_tables() {
        let mut st = AdpcmState { predictor: 32767, index: 200 };
        for i in 0..10_000u32 {
            st.decode((i.wrapping_mul(2654435761) >> 28) as u8);
            assert!(st.index <= 88);
        }
    }

    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, align: u16, data: &[u8], extra: &[u8]) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&tag.to_le_bytes());
        f.extend_from_slice(&channels.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * align as u32).to_le_bytes());
        f.extend_from_slice(&align.to_le_bytes());
        f.extend_from_slice(&bits.to_le_bytes());
        f.extend_from_slice(extra);
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(data);
        f
    }

    #[test]
    fn wav_reads_pcm_stereo_adpcm_and_a_loop() {
        let s = sine(32000, 500.0, 800, 8000.0);
        let pcm: Vec<u8> = s.iter().flat_map(|v| v.to_le_bytes()).collect();
        let w = parse_wav(&wav(1, 1, 32000, 16, 2, &pcm, &[])).unwrap();
        assert_eq!((w.rate, w.data.len(), w.loop_range), (32000, 800, None));
        let st: Vec<u8> = s.iter().flat_map(|v| [v.to_le_bytes(), v.to_le_bytes()]).flatten().collect();
        assert_eq!(parse_wav(&wav(1, 2, 32000, 16, 4, &st, &[])).unwrap().data, s);
        let ad = encode_ima_blocks(&s, 256);
        let adw = parse_wav(&wav(0x11, 1, 32000, 4, 256, &ad, &[])).unwrap();
        assert!(adw.data.len() >= 800 && adw.rate == 32000);
        // an smpl chunk with one loop 100..=199
        let mut smpl = Vec::new();
        smpl.extend_from_slice(b"smpl");
        smpl.extend_from_slice(&60u32.to_le_bytes());
        let mut body = vec![0u8; 60];
        body[28..32].copy_from_slice(&1u32.to_le_bytes());
        body[44..48].copy_from_slice(&100u32.to_le_bytes());
        body[48..52].copy_from_slice(&199u32.to_le_bytes());
        smpl.extend_from_slice(&body);
        let mut with = wav(1, 1, 32000, 16, 2, &pcm, &[]);
        with.extend_from_slice(&smpl);
        assert_eq!(parse_wav(&with).unwrap().loop_range, Some((100, 200)));
    }

    #[test]
    fn wav_refuses_rubbish_and_never_panics() {
        assert!(parse_wav(b"").is_err());
        assert!(parse_wav(b"RIFF....WAVE").is_err());
        let good = wav(1, 1, 32000, 16, 2, &[0; 64], &[]);
        for cut in 0..good.len() {
            let _ = parse_wav(&good[..cut]);
        }
        let mut x = 0x9e37_79b9u32;
        for _ in 0..300 {
            let mut b = good.clone();
            for _ in 0..4 {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                let i = (x >> 8) as usize % b.len();
                b[i] = (x >> 24) as u8;
            }
            let _ = parse_wav(&b);
        }
        assert!(parse_wav(&wav(1, 1, 5, 16, 2, &[0; 8], &[])).is_err(), "rate out of range");
        assert!(parse_wav(&wav(3, 1, 32000, 32, 4, &[0; 8], &[])).is_err(), "float is not read");
    }

    #[test]
    fn pitch_follows_the_key() {
        let rate = 44100.0;
        let smp = Sample::new(44100, sine(44100, 441.0, 44100, 10000.0)).with_loop(0, 44100);
        for (note, want) in [(60u8, 441.0), (72, 882.0), (48, 220.5), (67, 441.0 * 2f64.powf(7.0 / 12.0))] {
            for interp in [Interp::Nearest, Interp::Linear, Interp::Cubic] {
                let mut s = Sampler::new(smp.clone(), rate, 60).unwrap();
                s.set_interp(interp);
                s.note_on(note, 127);
                let mut out = vec![0i16; 44100];
                s.render(&mut out);
                let got = hz_of(&out, rate);
                assert!((got - want).abs() < want * 0.005, "note {note} {interp:?}: {got} for {want}");
            }
        }
    }

    #[test]
    fn a_sample_at_another_rate_keeps_its_pitch() {
        let smp = Sample::new(22050, sine(22050, 441.0, 22050, 10000.0)).with_loop(0, 22050);
        let mut s = Sampler::new(smp, 48000.0, 60).unwrap();
        s.note_on(60, 127);
        let mut out = vec![0i16; 48000];
        s.render(&mut out);
        assert!((hz_of(&out, 48000.0) - 441.0).abs() < 2.0);
    }

    #[test]
    fn a_one_shot_ends_and_a_loop_runs_until_released() {
        let mut s = Sampler::new(Sample::new(44100, sine(44100, 441.0, 4410, 10000.0)), 44100.0, 60).unwrap();
        s.note_on(60, 100);
        s.note_off(60); // a drum hit ignores the release
        let mut out = vec![0i16; 8820];
        s.render(&mut out);
        assert!(rms(&out[..4000]) > 1000.0 && out[4500..].iter().all(|&v| v == 0));
        assert_eq!(s.sounding(), 0);
        let mut l = Sampler::new(Sample::new(44100, sine(44100, 441.0, 4410, 10000.0)).with_loop(0, 4410), 44100.0, 60).unwrap();
        l.note_on(60, 100);
        let mut out = vec![0i16; 44100];
        l.render(&mut out);
        assert!(rms(&out[40000..]) > 1000.0, "the loop sounds a second on");
        l.note_off(60);
        l.render(&mut out);
        assert!(out[1000..].iter().all(|&v| v == 0) && l.sounding() == 0);
    }

    #[test]
    fn the_mixer_holds_power_beyond_the_limit_and_steals_the_oldest() {
        let smp = Sample::new(44100, sine(44100, 441.0, 44100, 3000.0)).with_loop(0, 44100);
        let level = |n: usize| {
            let mut s = Sampler::new(smp.clone(), 44100.0, 60).unwrap();
            s.set_voices(16);
            for _ in 0..n {
                s.note_on(60, 127); // the same note n times: n times the amplitude, in phase
            }
            let mut out = vec![0i16; 4410];
            s.render(&mut out);
            rms(&out) / n as f64
        };
        let one = level(1);
        assert!((level(4) - one).abs() < one * 0.01);
        // 8 in phase: amplitude 8 * sqrt(4 / 8) = 5.66 times one, so per note 0.707
        assert!((level(8) / one - 0.5f64.sqrt()).abs() < 0.01);
        let mut s = Sampler::new(smp, 44100.0, 60).unwrap();
        s.set_voices(2);
        s.note_on(60, 100);
        s.note_on(64, 100);
        s.note_on(67, 100);
        assert_eq!(s.sounding(), 2);
        assert!(s.voices.iter().any(|v| v.live && v.note == 67) && !s.voices.iter().any(|v| v.live && v.note == 60));
    }

    #[test]
    fn bad_settings_are_refused_or_clamped() {
        assert!(Sampler::new(Sample::new(44100, vec![0; 4]), 100.0, 60).is_err());
        let s = Sample::new(44100, vec![1; 10]).with_loop(5, 5);
        assert_eq!(s.loop_range, None);
        assert_eq!(Sample::new(44100, vec![1; 10]).with_loop(0, 99).loop_range, None);
        let mut e = Sampler::new(Sample::new(44100, vec![]), 44100.0, 60).unwrap();
        e.note_on(60, 127);
        let mut out = [1i16; 8];
        e.render(&mut out);
        assert_eq!(out, [0; 8]);
        let mut x = Sampler::new(Sample::new(44100, vec![100; 3]), 44100.0, 200).unwrap();
        for n in 0..=255u8 {
            x.note_on(n, n);
        }
        x.render(&mut out);
    }
}
