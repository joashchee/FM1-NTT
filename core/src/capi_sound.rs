//! The C face of the two sound engines (`fm1ntt.h`): `fm6` (a DX7 voice played from the keys) and the sampler (a WAV on the keys, or a drum kit). Every function tolerates
//! null, reads only what its length says, bounds every size, and never unwinds. A handle is made by `_new` and freed by `_free`; failures say why in `fm1ntt_error`.

use crate::capi::set_error;
use crate::fm6::{self, Output, Synth, Tuning};
use crate::sampler::{parse_wav, Kit, Sampler};
use std::ffi::c_int;

/// The most frames one render call may ask for.
const MAX_FRAMES: usize = 1 << 20;

pub struct Fm6Handle(Synth);

/// A plain test voice (a 99-level sine on operator 1, algorithm 32, full-rate envelope) for a handle made without one.
fn init_voice() -> [u8; 128] {
    let mut p = [0u8; 156];
    for op in 0..6 {
        let o = op * 21;
        p[o..o + 4].copy_from_slice(&[99, 99, 99, 99]);
        p[o + 4..o + 7].copy_from_slice(&[99, 99, 99]);
        p[o + 18] = 1;
        p[o + 16] = if op == 5 { 99 } else { 0 };
    }
    p[126..130].copy_from_slice(&[99, 99, 99, 99]);
    p[130..134].copy_from_slice(&[50, 50, 50, 50]);
    p[134] = 31;
    p[137] = 35;
    p[142] = 0;
    for (i, c) in b"INIT VOICE".iter().enumerate() {
        p[145 + i] = *c;
    }
    p[155] = 0x3f;
    fm6::pack_patch(&p)
}

/// A DX7-voice synth at `sample_rate` (8000 to 192000), playing the 128-byte packed voice at `voice` (null for a plain sine), or null with the reason in `fm1ntt_error`.
///
/// # Safety
/// `voice` is null or points at 128 readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_new(sample_rate: f64, voice: *const u8) -> *mut Fm6Handle {
    let v: [u8; 128] = if voice.is_null() { init_voice() } else { std::slice::from_raw_parts(voice, 128).try_into().unwrap_or([0; 128]) };
    match Synth::new(sample_rate, &v) {
        Ok(s) => {
            set_error(None);
            Box::into_raw(Box::new(Fm6Handle(s)))
        }
        Err(e) => {
            set_error(Some(e.to_string()));
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `h` is null or from `fm1ntt_fm6_new`, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_free(h: *mut Fm6Handle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// Plays the voice in a 163-byte DX7 single-voice sysex dump (what `fm6::vced_sysex` writes, what the DX7 and DX747 take). 0 on success, -1 when it is not one.
///
/// # Safety
/// `h` is live and `bytes` points at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_set_voice_sysex(h: *mut Fm6Handle, bytes: *const u8, len: usize) -> c_int {
    let (Some(h), false) = (h.as_mut(), bytes.is_null()) else { return -1 };
    match fm6::parse_vced_sysex(std::slice::from_raw_parts(bytes, len)) {
        Some(p) => {
            h.0.set_voice(&fm6::pack_patch(&p));
            0
        }
        None => set_error(Some("not a DX7 single-voice dump".into())),
    }
}

/// The output stage: 0 msfa's own level, 1 Felucca's with the master pot (0 to 1023), 2 the stock firmware's. Tuning: 0 msfa's, 1 Felucca's (with `dtun`, 0 to 99), 2 the stock engine's.
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_set_model(h: *mut Fm6Handle, output: c_int, pot: c_int, tuning: c_int, dtun: c_int) {
    if let Some(h) = h.as_mut() {
        let pot = pot.clamp(0, 1023) as u16;
        h.0.set_output(match output {
            1 => Output::Felucca { pot },
            2 => Output::Stock { pot },
            _ => Output::Msfa,
        });
        h.0.set_tuning(match tuning {
            1 => Tuning::Felucca { dtun: dtun.clamp(0, 99) as u8 },
            2 => Tuning::Stock,
            _ => Tuning::Msfa,
        });
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_note_on(h: *mut Fm6Handle, note: c_int, velocity: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.note_on(note.clamp(0, 127) as u8, velocity.clamp(0, 127) as u8);
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_note_off(h: *mut Fm6Handle, note: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.note_off(note.clamp(0, 127) as u8);
    }
}

/// The pitch wheel, 0 to 16383 (8192 is centre).
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_pitch_bend(h: *mut Fm6Handle, value: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.set_pitch_bend(value);
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_all_off(h: *mut Fm6Handle) {
    if let Some(h) = h.as_mut() {
        h.0.all_off();
    }
}

/// Renders `frames` mono samples. Returns the frames written (0 for a null handle or buffer, or more than 1 << 20 frames).
///
/// # Safety
/// `h` is live and `out` has room for `frames` samples.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_render(h: *mut Fm6Handle, out: *mut i16, frames: usize) -> usize {
    let (Some(h), false) = (h.as_mut(), out.is_null()) else { return 0 };
    if frames > MAX_FRAMES {
        return 0;
    }
    h.0.render(std::slice::from_raw_parts_mut(out, frames));
    frames
}

/// Voices that still sound.
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_fm6_sounding(h: *const Fm6Handle) -> usize {
    h.as_ref().map_or(0, |h| h.0.live_voices())
}

// ---------------------------------------------------------------------------------------------------------------
// The sampler: one sample on the keys

pub struct SamplerHandle(Sampler);

/// A sampler playing the WAV file at `wav` (8 or 16-bit PCM or IMA ADPCM, a `smpl` loop honoured) at `out_rate`, sounding the sample's own pitch at MIDI note `root`; null with the
/// reason in `fm1ntt_error` when the file is not one.
///
/// # Safety
/// `wav` points at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_sampler_new(wav: *const u8, len: usize, out_rate: f64, root: c_int) -> *mut SamplerHandle {
    if wav.is_null() {
        set_error(Some("no data".into()));
        return std::ptr::null_mut();
    }
    match parse_wav(std::slice::from_raw_parts(wav, len)).and_then(|s| Sampler::new(s, out_rate, root.clamp(0, 127) as u8)) {
        Ok(s) => {
            set_error(None);
            Box::into_raw(Box::new(SamplerHandle(s)))
        }
        Err(e) => {
            set_error(Some(e.to_string()));
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `h` is null or from `fm1ntt_sampler_new`, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_sampler_free(h: *mut SamplerHandle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_sampler_note_on(h: *mut SamplerHandle, note: c_int, velocity: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.note_on(note.clamp(0, 127) as u8, velocity.clamp(0, 127) as u8);
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_sampler_note_off(h: *mut SamplerHandle, note: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.note_off(note.clamp(0, 127) as u8);
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_sampler_all_off(h: *mut SamplerHandle) {
    if let Some(h) = h.as_mut() {
        h.0.all_off();
    }
}

/// Renders `frames` mono samples; the frames written (0 for null or more than 1 << 20).
///
/// # Safety
/// `h` is live and `out` has room for `frames` samples.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_sampler_render(h: *mut SamplerHandle, out: *mut i16, frames: usize) -> usize {
    let (Some(h), false) = (h.as_mut(), out.is_null()) else { return 0 };
    if frames > MAX_FRAMES {
        return 0;
    }
    h.0.render(std::slice::from_raw_parts_mut(out, frames));
    frames
}

// ---------------------------------------------------------------------------------------------------------------
// The drum kit

pub struct KitHandle(Kit);

/// An empty drum kit at `out_rate`, or null. `ceiling` is the limiter's peak as a fraction of full scale (0 for none; X0X's is 0.4).
///
/// # Safety
/// Always safe to call.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_kit_new(out_rate: f64, ceiling: f64) -> *mut KitHandle {
    match Kit::new(out_rate) {
        Ok(mut k) => {
            if ceiling > 0.0 {
                k.set_ceiling(Some(ceiling), 0.05);
            }
            set_error(None);
            Box::into_raw(Box::new(KitHandle(k)))
        }
        Err(e) => {
            set_error(Some(e.to_string()));
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `h` is null or from `fm1ntt_kit_new`, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_kit_free(h: *mut KitHandle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// Puts the WAV file at `wav` on `note`; striking it cuts the `n_chokes` notes at `chokes` (null for none). 0 on success, else -1 with the reason in `fm1ntt_error`.
///
/// # Safety
/// `h` is live, `wav` points at `len` readable bytes and `chokes` at `n_chokes` bytes (or is null).
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_kit_add_wav(h: *mut KitHandle, note: c_int, wav: *const u8, len: usize, chokes: *const u8, n_chokes: usize) -> c_int {
    let (Some(h), false) = (h.as_mut(), wav.is_null()) else { return -1 };
    let ch: &[u8] = if chokes.is_null() { &[] } else { std::slice::from_raw_parts(chokes, n_chokes.min(128)) };
    match parse_wav(std::slice::from_raw_parts(wav, len)).and_then(|s| h.0.add(note.clamp(0, 127) as u8, s, ch)) {
        Ok(()) => set_error(None),
        Err(e) => set_error(Some(e.to_string())),
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_kit_note_on(h: *mut KitHandle, note: c_int, velocity: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.note_on(note.clamp(0, 127) as u8, velocity.clamp(0, 127) as u8);
    }
}

/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_kit_all_off(h: *mut KitHandle) {
    if let Some(h) = h.as_mut() {
        h.0.all_off();
    }
}

/// Renders `frames` mono samples; the frames written (0 for null or more than 1 << 20).
///
/// # Safety
/// `h` is live and `out` has room for `frames` samples.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_kit_render(h: *mut KitHandle, out: *mut i16, frames: usize) -> usize {
    let (Some(h), false) = (h.as_mut(), out.is_null()) else { return 0 };
    if frames > MAX_FRAMES {
        return 0;
    }
    h.0.render(std::slice::from_raw_parts_mut(out, frames));
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, data: &[i16]) -> Vec<u8> {
        let mut b = Vec::new();
        let n = data.len() * 2;
        b.extend(b"RIFF");
        b.extend((36 + n as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((n as u32).to_le_bytes());
        for s in data {
            b.extend(s.to_le_bytes());
        }
        b
    }

    fn rms(x: &[i16]) -> f64 {
        (x.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    #[test]
    fn fm6_through_the_c_face() {
        unsafe {
            assert!(fm1ntt_fm6_new(100.0, std::ptr::null()).is_null());
            let h = fm1ntt_fm6_new(44100.0, std::ptr::null());
            assert!(!h.is_null());
            let mut out = vec![0i16; 4410];
            assert_eq!(fm1ntt_fm6_render(h, out.as_mut_ptr(), out.len()), out.len());
            assert!(out.iter().all(|&x| x == 0), "silent until a note");
            fm1ntt_fm6_note_on(h, 69, 100);
            assert_eq!(fm1ntt_fm6_sounding(h), 1);
            fm1ntt_fm6_render(h, out.as_mut_ptr(), out.len());
            assert!(rms(&out) > 100.0, "the plain voice sounds: {}", rms(&out));
            fm1ntt_fm6_set_model(h, 2, 900, 2, 0);
            fm1ntt_fm6_pitch_bend(h, 99999);
            fm1ntt_fm6_note_off(h, 69);
            fm1ntt_fm6_all_off(h);
            assert_eq!(fm1ntt_fm6_render(h, out.as_mut_ptr(), 1 << 21), 0);
            assert_eq!(fm1ntt_fm6_render(h, std::ptr::null_mut(), 8), 0);
            let sx = fm6::vced_sysex(&fm6::unpack_patch(&init_voice()), 0);
            assert_eq!(fm1ntt_fm6_set_voice_sysex(h, sx.as_ptr(), sx.len()), 0);
            assert_eq!(fm1ntt_fm6_set_voice_sysex(h, sx.as_ptr(), 10), -1);
            assert_eq!(fm1ntt_fm6_set_voice_sysex(std::ptr::null_mut(), sx.as_ptr(), sx.len()), -1);
            fm1ntt_fm6_free(h);
            fm1ntt_fm6_free(std::ptr::null_mut());
            fm1ntt_fm6_note_on(std::ptr::null_mut(), 60, 100);
            assert_eq!(fm1ntt_fm6_sounding(std::ptr::null()), 0);
        }
    }

    #[test]
    fn the_sampler_and_the_kit_through_the_c_face() {
        let tone: Vec<i16> = (0..8000).map(|i| (8000.0 * (i as f64 * 0.1).sin()) as i16).collect();
        let w = wav(22050, &tone);
        unsafe {
            assert!(fm1ntt_sampler_new(std::ptr::null(), 0, 44100.0, 60).is_null());
            assert!(fm1ntt_sampler_new([1u8, 2, 3].as_ptr(), 3, 44100.0, 60).is_null());
            let s = fm1ntt_sampler_new(w.as_ptr(), w.len(), 44100.0, 60);
            assert!(!s.is_null());
            let mut out = vec![0i16; 2205];
            fm1ntt_sampler_note_on(s, 60, 127);
            assert_eq!(fm1ntt_sampler_render(s, out.as_mut_ptr(), out.len()), out.len());
            assert!(rms(&out) > 100.0);
            fm1ntt_sampler_note_off(s, 60);
            fm1ntt_sampler_all_off(s);
            assert_eq!(fm1ntt_sampler_render(s, out.as_mut_ptr(), 1 << 21), 0);
            fm1ntt_sampler_free(s);
            fm1ntt_sampler_free(std::ptr::null_mut());

            assert!(fm1ntt_kit_new(10.0, 0.4).is_null());
            let k = fm1ntt_kit_new(44100.0, 0.4);
            assert!(!k.is_null());
            let open = [46u8];
            assert_eq!(fm1ntt_kit_add_wav(k, 46, w.as_ptr(), w.len(), std::ptr::null(), 0), 0);
            assert_eq!(fm1ntt_kit_add_wav(k, 42, w.as_ptr(), w.len(), open.as_ptr(), 1), 0);
            assert_eq!(fm1ntt_kit_add_wav(k, 36, [0u8; 4].as_ptr(), 4, std::ptr::null(), 0), -1);
            fm1ntt_kit_note_on(k, 46, 127);
            fm1ntt_kit_render(k, out.as_mut_ptr(), out.len());
            assert!(rms(&out) > 100.0);
            assert!(out.iter().all(|&x| (x as f64).abs() <= 0.4 * 32768.0 + 2.0));
            fm1ntt_kit_all_off(k);
            fm1ntt_kit_free(k);
            fm1ntt_kit_free(std::ptr::null_mut());
            assert_eq!(fm1ntt_kit_add_wav(std::ptr::null_mut(), 1, w.as_ptr(), w.len(), std::ptr::null(), 0), -1);
        }
    }
}
