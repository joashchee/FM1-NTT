//! The C face of the instrument host (`fm1ntt.h`, docs/PORTING.md): load a package (`.fm1i` text), play it from MIDI bytes or notes, edit its parameters, write it back.
//! Every function tolerates null, reads only what its length says, bounds every size, and never unwinds. A failure says why in `fm1ntt_error`.

use crate::capi::set_error;
use crate::instrument::{factory_package, Instrument};
use crate::package::{self, Package, Tier};
use std::ffi::{c_char, c_int, c_void, CStr};

const MAX_FRAMES: usize = 1 << 20;
/// The largest sample file the loader will ask the host for.
const MAX_SAMPLE_BYTES: usize = 64 << 20;

pub struct InstHandle(Instrument, Option<Package>);

/// Reads a file for the loader: called with `cap` 0 to ask the size, then with a buffer of that size; returns the size, or -1 when the file cannot be read.
pub type ReadFn = unsafe extern "C" fn(user: *mut c_void, name: *const c_char, buf: *mut u8, cap: usize) -> i64;

fn finish(r: crate::Result<Instrument>, p: Package) -> *mut InstHandle {
    match r {
        Ok(i) => {
            set_error(None);
            Box::into_raw(Box::new(InstHandle(i, Some(p))))
        }
        Err(e) => {
            set_error(Some(e.to_string()));
            std::ptr::null_mut()
        }
    }
}

/// An instrument from the package text at `text` (UTF-8, `len` bytes) at `rate` (8000 to 192000 Hz). Sample files named in it are read through `read` (may be null when the
/// package names none). Null with the reason in `fm1ntt_error` when the text is not a package, its licence is not on the allowlist, or the engine cannot be built.
///
/// # Safety
/// `text` points at `len` readable bytes; `read`, when not null, follows `ReadFn`.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_load(text: *const u8, len: usize, rate: f64, read: Option<ReadFn>, user: *mut c_void) -> *mut InstHandle {
    if text.is_null() {
        set_error(Some("no package".into()));
        return std::ptr::null_mut();
    }
    let Ok(s) = std::str::from_utf8(std::slice::from_raw_parts(text, len)) else {
        set_error(Some("package: not UTF-8".into()));
        return std::ptr::null_mut();
    };
    let pkg = match Package::parse(s) {
        Ok(p) => p,
        Err(e) => {
            set_error(Some(e.to_string()));
            return std::ptr::null_mut();
        }
    };
    let resolve = |file: &str| -> Option<Vec<u8>> {
        let read = read?;
        let name = std::ffi::CString::new(file).ok()?;
        let size = read(user, name.as_ptr(), std::ptr::null_mut(), 0);
        if size < 0 || size as usize > MAX_SAMPLE_BYTES {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let got = read(user, name.as_ptr(), buf.as_mut_ptr(), buf.len());
        (got == size).then_some(buf)
    };
    let r = Instrument::from_package(&pkg, rate, &resolve);
    finish(r, pkg)
}

/// The built-in voices as an instrument (an `fm6` package of the project's own eight), or null with the reason.
#[no_mangle]
pub extern "C" fn fm1ntt_inst_factory(rate: f64) -> *mut InstHandle {
    let p = factory_package();
    let r = Instrument::from_package(&p, rate, &|_| None);
    finish(r, p)
}

/// # Safety
/// `h` is null or from `fm1ntt_inst_load`/`fm1ntt_inst_factory`, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_free(h: *mut InstHandle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// The shipping tier of the package text: 0 permissive (may be in the MIT app), 1 copyleft (the Pack only, never the MIT app's package or a Mac App Store build), -1 not a package.
///
/// # Safety
/// `text` points at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_package_tier(text: *const u8, len: usize) -> c_int {
    if text.is_null() {
        return -1;
    }
    let Ok(s) = std::str::from_utf8(std::slice::from_raw_parts(text, len)) else { return -1 };
    match Package::parse(s).and_then(|p| p.tier()) {
        Ok(Tier::Permissive) => 0,
        Ok(Tier::Copyleft) => 1,
        Err(e) => {
            set_error(Some(e.to_string()));
            -1
        }
    }
}

unsafe fn put_str(s: &str, out: *mut c_char, cap: usize) -> usize {
    if !out.is_null() && cap > 0 {
        let n = s.len().min(cap - 1);
        std::ptr::copy_nonoverlapping(s.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    s.len()
}

/// The instrument's name, kind ("fm6", "fm4", "analog", "sampler", "kit"), licence or verification note, as text in `out` (NUL-terminated, cut to `cap`); the full length.
/// `which`: 0 name, 1 kind, 2 licence, 3 verification note (empty when unverified), 4 voice name of the current program (fm6).
///
/// # Safety
/// `out` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_text(h: *const InstHandle, which: c_int, out: *mut c_char, cap: usize) -> usize {
    let Some(h) = h.as_ref() else { return 0 };
    let i = &h.0;
    match which {
        0 => put_str(i.name(), out, cap),
        1 => put_str(i.kind(), out, cap),
        2 => put_str(i.licence(), out, cap),
        3 => put_str(i.verify_note(), out, cap),
        4 => {
            let name = i.params().iter().find(|d| d.id == 145).map(|_| (145..155).map(|k| i.get_param(k).map(|c| c as u8 as char).unwrap_or(' ')).collect::<String>()).unwrap_or_default();
            put_str(name.trim_end(), out, cap)
        }
        _ => 0,
    }
}

/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_midi(h: *mut InstHandle, bytes: *const u8, len: usize) {
    if let (Some(h), false) = (h.as_mut(), bytes.is_null()) {
        h.0.midi_bytes(std::slice::from_raw_parts(bytes, len.min(1 << 16)));
    }
}

/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_note_on(h: *mut InstHandle, note: c_int, velocity: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.note_on(note.clamp(0, 127) as u8, velocity.clamp(0, 127) as u8);
    }
}

/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_note_off(h: *mut InstHandle, note: c_int) {
    if let Some(h) = h.as_mut() {
        h.0.note_off(note.clamp(0, 127) as u8);
    }
}

/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_all_off(h: *mut InstHandle) {
    if let Some(h) = h.as_mut() {
        h.0.all_off();
    }
}

/// Notes sounding now.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_sounding(h: *const InstHandle) -> usize {
    h.as_ref().map_or(0, |h| h.0.sounding())
}

/// Interleaved stereo, `frames` frames into `out` (2 * frames samples); the frames written (0 for more than 1 << 20 or a null).
///
/// # Safety
/// `out` has room for `2 * frames` samples.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_render_stereo(h: *mut InstHandle, out: *mut i16, frames: usize) -> usize {
    match (h.as_mut(), out.is_null()) {
        (Some(h), false) if frames <= MAX_FRAMES => {
            h.0.render_stereo(std::slice::from_raw_parts_mut(out, frames * 2));
            frames
        }
        _ => 0,
    }
}

/// Mono, `frames` samples.
///
/// # Safety
/// `out` has room for `frames` samples.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_render(h: *mut InstHandle, out: *mut i16, frames: usize) -> usize {
    match (h.as_mut(), out.is_null()) {
        (Some(h), false) if frames <= MAX_FRAMES => {
            h.0.render(std::slice::from_raw_parts_mut(out, frames));
            frames
        }
        _ => 0,
    }
}

/// The voices in the bank and the current program (0 for an engine with none).
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_voice_count(h: *const InstHandle) -> usize {
    h.as_ref().map_or(0, |h| h.0.voice_count())
}

/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_program(h: *const InstHandle) -> usize {
    h.as_ref().map_or(0, |h| h.0.program())
}

/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_set_program(h: *mut InstHandle, program: usize) {
    if let Some(h) = h.as_mut() {
        h.0.set_program(program);
    }
}

/// How many parameters the engine has (an editor lists them by index 0 to count - 1).
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_param_count(h: *const InstHandle) -> usize {
    h.as_ref().map_or(0, |h| h.0.params().len())
}

/// Parameter `index`: `info` gets { id, min, max, default } and `name` its name; 0 ok, -1 out of range.
///
/// # Safety
/// `info` has room for 4 values; `name` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_param(h: *const InstHandle, index: usize, info: *mut i32, name: *mut c_char, cap: usize) -> c_int {
    let (Some(h), false) = (h.as_ref(), info.is_null()) else { return -1 };
    let p = h.0.params();
    let Some(d) = p.get(index) else { return -1 };
    *info = d.id as i32;
    *info.add(1) = d.min;
    *info.add(2) = d.max;
    *info.add(3) = d.default;
    put_str(&d.name, name, cap);
    0
}

/// The value of parameter `id`, or `i32::MIN` when there is none.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_get(h: *const InstHandle, id: c_int) -> i32 {
    h.as_ref().and_then(|h| h.0.get_param(id.clamp(0, 65535) as u16)).unwrap_or(i32::MIN)
}

/// Sets parameter `id` (clamped to its range); 0 ok, -1 no such parameter.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_set(h: *mut InstHandle, id: c_int, value: c_int) -> c_int {
    let Some(h) = h.as_mut() else { return -1 };
    if (0..=65535).contains(&id) && h.0.set_param(id as u16, value) {
        0
    } else {
        -1
    }
}

/// The instrument as package text (the edits included), written to `out` up to `cap` bytes (not NUL-terminated); the full length. `source` is the file it came from.
///
/// # Safety
/// `out` is null or has room for `cap` bytes; `source` is null or a C string.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_to_package(h: *const InstHandle, source: *const c_char, out: *mut u8, cap: usize) -> usize {
    let Some(h) = h.as_ref() else { return 0 };
    let src = if source.is_null() { String::new() } else { CStr::from_ptr(source).to_string_lossy().into_owned() };
    let sha = h.1.as_ref().map(|p| p.source_sha256.clone()).unwrap_or_default();
    let text = h.0.to_package(&src, &sha).to_text();
    if !out.is_null() {
        std::ptr::copy_nonoverlapping(text.as_ptr(), out, text.len().min(cap));
    }
    text.len()
}

/// The SHA-256 of `len` bytes at `data` as 64 lowercase hex characters plus a NUL in `out` (65 bytes): for the source line of a package.
///
/// # Safety
/// `data` points at `len` readable bytes; `out` has room for 65.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_sha256_hex(data: *const u8, len: usize, out: *mut c_char) {
    if data.is_null() || out.is_null() {
        return;
    }
    let h = package::hex(&package::sha256(std::slice::from_raw_parts(data, len)));
    put_str(&h, out, 65);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_c_face_plays_edits_and_writes_back() {
        unsafe {
            let h = fm1ntt_inst_factory(44_100.0);
            assert!(!h.is_null());
            let mut name = [0 as c_char; 32];
            assert!(fm1ntt_inst_text(h, 0, name.as_mut_ptr(), 32) > 0);
            assert_eq!(CStr::from_ptr(name.as_ptr()).to_str().unwrap(), "Built-in voices");
            assert_eq!(fm1ntt_inst_voice_count(h), 8);
            fm1ntt_inst_midi(h, [0x90u8, 60, 100].as_ptr(), 3);
            assert_eq!(fm1ntt_inst_sounding(h), 1);
            let mut out = vec![0i16; 2048];
            assert_eq!(fm1ntt_inst_render_stereo(h, out.as_mut_ptr(), 1024), 1024);
            assert!(out.iter().any(|&x| x != 0));
            assert_eq!(fm1ntt_inst_render_stereo(h, out.as_mut_ptr(), 1 << 21), 0);
            assert!(fm1ntt_inst_param_count(h) > 150);
            let (mut info, mut pn) = ([0i32; 4], [0 as c_char; 40]);
            assert_eq!(fm1ntt_inst_param(h, 134, info.as_mut_ptr(), pn.as_mut_ptr(), 40), 0);
            assert_eq!(CStr::from_ptr(pn.as_ptr()).to_str().unwrap(), "ALGORITHM");
            assert_eq!(fm1ntt_inst_param(h, 99999, info.as_mut_ptr(), pn.as_mut_ptr(), 40), -1);
            assert_eq!(fm1ntt_inst_set(h, 135, 4), 0);
            assert_eq!(fm1ntt_inst_get(h, 135), 4);
            assert_eq!(fm1ntt_inst_set(h, 30000, 4), -1);
            assert_eq!(fm1ntt_inst_get(h, 30000), i32::MIN);
            let n = fm1ntt_inst_to_package(h, c"by hand".as_ptr(), std::ptr::null_mut(), 0);
            let mut text = vec![0u8; n];
            assert_eq!(fm1ntt_inst_to_package(h, c"by hand".as_ptr(), text.as_mut_ptr(), n), n);
            assert_eq!(fm1ntt_package_tier(text.as_ptr(), n), 0);
            let g = fm1ntt_inst_load(text.as_ptr(), n, 44_100.0, None, std::ptr::null_mut());
            assert!(!g.is_null());
            assert_eq!(fm1ntt_inst_get(g, 135), 4, "the edit came back");
            fm1ntt_inst_free(g);
            fm1ntt_inst_free(h);
        }
    }

    #[test]
    fn nulls_and_bad_text_are_safe() {
        unsafe {
            fm1ntt_inst_free(std::ptr::null_mut());
            fm1ntt_inst_midi(std::ptr::null_mut(), std::ptr::null(), 0);
            assert_eq!(fm1ntt_inst_render(std::ptr::null_mut(), std::ptr::null_mut(), 8), 0);
            assert!(fm1ntt_inst_load(std::ptr::null(), 0, 44_100.0, None, std::ptr::null_mut()).is_null());
            assert!(fm1ntt_inst_load(b"junk".as_ptr(), 4, 44_100.0, None, std::ptr::null_mut()).is_null());
            assert!(fm1ntt_inst_load([0xffu8, 0xfe].as_ptr(), 2, 44_100.0, None, std::ptr::null_mut()).is_null());
            let (gpl, agpl) = ("fm1i = 1\nengine = fm6\nlicence = GPL-3.0-only\n", "fm1i = 1\nengine = fm6\nlicence = AGPL-3.0-only\n");
            assert_eq!(fm1ntt_package_tier(gpl.as_ptr(), gpl.len()), 1);
            assert_eq!(fm1ntt_package_tier(agpl.as_ptr(), agpl.len()), -1);
            let mut hex = [0 as c_char; 65];
            fm1ntt_sha256_hex(b"abc".as_ptr(), 3, hex.as_mut_ptr());
            assert!(CStr::from_ptr(hex.as_ptr()).to_str().unwrap().starts_with("ba7816bf"));
        }
    }
}
