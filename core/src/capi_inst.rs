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

/// The lock-free queue of timestamped MIDI from one thread to the audio thread (`evq.rs`).
pub struct EvqHandle(crate::evq::EventQueue);

/// A queue of at least `capacity` messages (16 to 65536). **One thread pushes and one thread renders**; nothing here locks.
#[no_mangle]
pub extern "C" fn fm1ntt_evq_new(capacity: usize) -> *mut EvqHandle {
    Box::into_raw(Box::new(EvqHandle(crate::evq::EventQueue::new(capacity))))
}

/// # Safety
/// `q` is null or a queue from `fm1ntt_evq_new`, and no other thread is using it.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_evq_free(q: *mut EvqHandle) {
    if !q.is_null() {
        drop(Box::from_raw(q));
    }
}

/// Pushes one MIDI message (1 to 3 bytes: a note, controller, pitch bend or program change) stamped `ts_us` on the host's monotonic microsecond clock, from the producer
/// thread. 0 queued; -1 refused (full, empty, longer than 3 bytes, or null), counted in `fm1ntt_evq_dropped`.
///
/// # Safety
/// `bytes` points at `len` readable bytes; only one thread pushes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_evq_push(q: *const EvqHandle, ts_us: u64, bytes: *const u8, len: usize) -> c_int {
    let (Some(q), false) = (q.as_ref(), bytes.is_null()) else { return -1 };
    if len <= crate::evq::MAX_MSG && q.0.push(ts_us, std::slice::from_raw_parts(bytes, len)) {
        0
    } else {
        if len > crate::evq::MAX_MSG {
            q.0.push(ts_us, &[]); // counts the refusal
        }
        -1
    }
}

/// A key of the machine's matrix (0 to 40) goes down (`down` non-zero) or up, stamped `ts_us`. 0 queued, -1 refused or an id out of range.
///
/// # Safety
/// `q` is null or a live queue; only one thread pushes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_evq_push_key(q: *const EvqHandle, ts_us: u64, id: c_int, down: c_int) -> c_int {
    match q.as_ref() {
        Some(q) if (0..41).contains(&id) && q.0.push_key(ts_us, id as u8, down != 0) => 0,
        _ => -1,
    }
}

/// An encoder (0 to 6) turns by `clicks`. 0 queued, -1 refused or out of range.
///
/// # Safety
/// `q` is null or a live queue; only one thread pushes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_evq_push_encoder(q: *const EvqHandle, ts_us: u64, enc: c_int, clicks: c_int) -> c_int {
    match q.as_ref() {
        Some(q) if (0..7).contains(&enc) && q.0.push_encoder(ts_us, enc as u8, clicks) => 0,
        _ => -1,
    }
}

/// The pot moves to `value` (0 to 1023, clamped). 0 queued, -1 refused.
///
/// # Safety
/// `q` is null or a live queue; only one thread pushes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_evq_push_pot(q: *const EvqHandle, ts_us: u64, value: c_int) -> c_int {
    match q.as_ref() {
        Some(q) if q.0.push_pot(ts_us, value.clamp(0, 1023)) => 0,
        _ => -1,
    }
}

/// Takes every message waiting in `q` into the machine at once, without running it (the callback does this while paused, so nothing waits to land late).
///
/// # Safety
/// `m` and `q` are live; only the calling thread consumes `q`.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_apply_events(m: *mut crate::capi::Handle, q: *const EvqHandle) {
    if let Some(q) = q.as_ref() {
        crate::capi::with(m, (), |mach| mach.apply_events(&q.0));
    }
}

/// Messages waiting, and pushes refused so far.
///
/// # Safety
/// `q` is null or a live queue.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_evq_len(q: *const EvqHandle) -> usize {
    q.as_ref().map_or(0, |q| q.0.len())
}

/// # Safety
/// `q` is null or a live queue.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_evq_dropped(q: *const EvqHandle) -> u64 {
    q.as_ref().map_or(0, |q| q.0.dropped())
}

/// Like `fm1ntt_render` (stereo, `frames` frames, at most `max_insns` instructions; returns the real frames) with the MIDI bytes of `q` put into the machine's MIDI IN at their
/// own frames. `now_us` is the host clock at the call, the clock the pusher stamped with. No lock is shared with the pusher.
///
/// # Safety
/// `m` and `q` are live, `out` has room for `2 * frames` samples, and only the calling thread consumes `q`.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_render_events(m: *mut crate::capi::Handle, q: *const EvqHandle, now_us: u64, out: *mut i16, frames: usize, max_insns: u32) -> usize {
    if out.is_null() || frames > 65_536 || q.is_null() {
        return 0;
    }
    let buf = std::slice::from_raw_parts_mut(out, frames * 2);
    buf.fill(0);
    crate::capi::with(m, 0, |mach| mach.render_events(&(*q).0, now_us, buf, max_insns))
}

/// Mono, `frames` samples, with the messages of `q` that fall in this buffer applied at their own frames (`now_us` is the host clock at the call, the clock the producer
/// stamped with). The audio callback calls this; no lock is shared with the pusher. The frames written (0 for more than 1 << 20 or a null).
///
/// # Safety
/// `out` has room for `frames` samples; `h` and `q` are live; only the calling thread consumes `q`.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_render_events(h: *mut InstHandle, q: *const EvqHandle, now_us: u64, out: *mut i16, frames: usize) -> usize {
    match (h.as_mut(), q.as_ref(), out.is_null()) {
        (Some(h), Some(q), false) if frames <= MAX_FRAMES => {
            h.0.render_events(&q.0, now_us, std::slice::from_raw_parts_mut(out, frames));
            frames
        }
        _ => 0,
    }
}

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

/// The arpeggiator's parameters, as `fm1ntt_inst_param_count` and `fm1ntt_inst_param` do for the engine's (`arp.rs`).
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_arp_count(h: *const InstHandle) -> usize {
    h.as_ref().map_or(0, |h| h.0.arp_params().len())
}

/// Arpeggiator parameter `index`: `info` gets { id, min, max, default }; 0 ok, -1 out of range.
///
/// # Safety
/// `info` has room for 4 values; `name` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_arp_param(h: *const InstHandle, index: usize, info: *mut i32, name: *mut c_char, cap: usize) -> c_int {
    let (Some(h), false) = (h.as_ref(), info.is_null()) else { return -1 };
    let p = h.0.arp_params();
    let Some(d) = p.get(index) else { return -1 };
    *info = d.id as i32;
    *info.add(1) = d.min;
    *info.add(2) = d.max;
    *info.add(3) = d.default;
    put_str(&d.name, name, cap);
    0
}

/// The value of arpeggiator parameter `id`, or `i32::MIN`.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_arp_get(h: *const InstHandle, id: c_int) -> i32 {
    h.as_ref().and_then(|h| h.0.get_arp_param(id.clamp(0, 65535) as u16)).unwrap_or(i32::MIN)
}

/// Sets arpeggiator parameter `id` (clamped); 0 ok, -1 no such parameter. Switching it off releases the note it holds.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_arp_set(h: *mut InstHandle, id: c_int, value: c_int) -> c_int {
    let Some(h) = h.as_mut() else { return -1 };
    if (0..=65535).contains(&id) && h.0.set_arp_param(id as u16, value) {
        0
    } else {
        -1
    }
}

/// The instrument's effects, in order: how many, and an effect's name (`out`, NUL-terminated, cut to `cap`; the full length, 0 for none).
///
/// # Safety
/// `h` is null or a live handle; `out` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_fx_count(h: *const InstHandle) -> usize {
    h.as_ref().map_or(0, |h| h.0.fx_count())
}

/// # Safety
/// `h` is null or a live handle; `out` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_fx_name(h: *const InstHandle, index: usize, out: *mut c_char, cap: usize) -> usize {
    match h.as_ref().and_then(|h| h.0.fx_name(index)) {
        Some(n) => put_str(n, out, cap),
        None => {
            put_str("", out, cap);
            0
        }
    }
}

/// Effect `fx`'s parameters, as the engine's: how many, `index`'s { id, min, max, default } and name, its value and setting it.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_fx_param_count(h: *const InstHandle, fx: usize) -> usize {
    h.as_ref().map_or(0, |h| h.0.fx_params(fx).len())
}

/// # Safety
/// `info` has room for 4 values; `name` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_fx_param(h: *const InstHandle, fx: usize, index: usize, info: *mut i32, name: *mut c_char, cap: usize) -> c_int {
    let (Some(h), false) = (h.as_ref(), info.is_null()) else { return -1 };
    let p = h.0.fx_params(fx);
    let Some(d) = p.get(index) else { return -1 };
    *info = d.id as i32;
    *info.add(1) = d.min;
    *info.add(2) = d.max;
    *info.add(3) = d.default;
    put_str(&d.name, name, cap);
    0
}

/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_fx_get(h: *const InstHandle, fx: usize, id: c_int) -> i32 {
    h.as_ref().and_then(|h| h.0.get_fx_param(fx, id.clamp(0, 65535) as u16)).unwrap_or(i32::MIN)
}

/// Sets effect `fx`'s parameter `id` (clamped); 0 ok, -1 no such effect or parameter.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_inst_fx_set(h: *mut InstHandle, fx: usize, id: c_int, value: c_int) -> c_int {
    let Some(h) = h.as_mut() else { return -1 };
    if (0..=65535).contains(&id) && h.0.set_fx_param(fx, id as u16, value) {
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
            // the event queue: a note stamped inside the buffer sounds from its frame, a refused message is counted, nulls are tolerated
            let q = fm1ntt_evq_new(8);
            let note = [0x90u8, 69, 127];
            assert_eq!(fm1ntt_evq_push(q, 95_000, note.as_ptr(), 3), 0);
            assert_eq!(fm1ntt_evq_push(q, 0, note.as_ptr(), 4), -1);
            assert_eq!((fm1ntt_evq_len(q), fm1ntt_evq_dropped(q)), (1, 1));
            let mut mono = vec![0i16; 441];
            assert_eq!(fm1ntt_inst_render_events(h, q, 100_000, mono.as_mut_ptr(), 441), 441);
            assert!(mono[300..].iter().any(|&x| x != 0), "the queued note sounds (frame placement is tested in instrument.rs)");
            assert_eq!((fm1ntt_evq_len(q), fm1ntt_inst_render_events(std::ptr::null_mut(), q, 0, mono.as_mut_ptr(), 4), fm1ntt_evq_push(std::ptr::null(), 0, note.as_ptr(), 3)), (0, 0, -1));
            // the machine's entry point tolerates nulls and an empty machine (nothing loaded renders silence)
            let mut st = vec![1i16; 2 * 64];
            assert_eq!(fm1ntt_render_events(std::ptr::null_mut(), q, 0, st.as_mut_ptr(), 64, 1000), 0);
            assert_eq!(fm1ntt_render_events(std::ptr::null_mut(), std::ptr::null(), 0, st.as_mut_ptr(), 64, 1000), 0);
            let mach = crate::capi::fm1ntt_new();
            assert_eq!(fm1ntt_render_events(mach, q, 0, st.as_mut_ptr(), 64, 1000), 0);
            assert!(st.iter().all(|&x| x == 0));
            crate::capi::fm1ntt_free(mach);
            fm1ntt_evq_free(q);
            fm1ntt_evq_free(std::ptr::null_mut());
            assert_eq!(fm1ntt_inst_arp_count(h), 10);
            assert_eq!(fm1ntt_inst_arp_param(h, 3, info.as_mut_ptr(), pn.as_mut_ptr(), 40), 0);
            assert_eq!((CStr::from_ptr(pn.as_ptr()).to_str().unwrap(), info[1], info[2]), ("BPM", 30, 300));
            assert_eq!(fm1ntt_inst_arp_set(h, 3, 9999), 0);
            assert_eq!(fm1ntt_inst_arp_get(h, 3), 300);
            assert_eq!((fm1ntt_inst_arp_set(h, 99, 1), fm1ntt_inst_arp_get(h, 99)), (-1, i32::MIN));
            assert_eq!(fm1ntt_inst_arp_set(h, 3, 120), 0);
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

    #[test]
    fn the_c_face_reaches_the_effects() {
        let text = "fm1i = 1\nname = FxTest\nengine = analog\nlicence = MIT\nfx = delay\nfxparam 2 = 40\nfx = chorus\n";
        unsafe {
            let h = fm1ntt_inst_load(text.as_ptr(), text.len(), 44_100.0, None, std::ptr::null_mut());
            assert!(!h.is_null());
            assert_eq!(fm1ntt_inst_fx_count(h), 2);
            let mut nm = [0 as c_char; 24];
            assert_eq!(fm1ntt_inst_fx_name(h, 0, nm.as_mut_ptr(), 24), 5);
            assert_eq!(CStr::from_ptr(nm.as_ptr()).to_str().unwrap(), "delay");
            assert_eq!(fm1ntt_inst_fx_name(h, 9, nm.as_mut_ptr(), 24), 0);
            assert_eq!(fm1ntt_inst_fx_param_count(h, 0), 3);
            let mut info = [0i32; 4];
            assert_eq!(fm1ntt_inst_fx_param(h, 0, 2, info.as_mut_ptr(), nm.as_mut_ptr(), 24), 0);
            assert_eq!((info[0], info[1], info[2], CStr::from_ptr(nm.as_ptr()).to_str().unwrap()), (2, 0, 100, "MIX"));
            assert_eq!(fm1ntt_inst_fx_get(h, 0, 2), 40);
            assert_eq!((fm1ntt_inst_fx_set(h, 0, 2, 999), fm1ntt_inst_fx_get(h, 0, 2)), (0, 100));
            assert_eq!((fm1ntt_inst_fx_set(h, 5, 2, 1), fm1ntt_inst_fx_set(h, 0, 77, 1), fm1ntt_inst_fx_get(h, 7, 0), fm1ntt_inst_fx_param_count(h, 9)), (-1, -1, i32::MIN, 0));
            assert_eq!(fm1ntt_inst_fx_param(h, 0, 9, info.as_mut_ptr(), nm.as_mut_ptr(), 24), -1);
            fm1ntt_inst_free(h);
        }
    }
}
