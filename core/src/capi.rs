//! The C face (`fm1ntt.h`). Every function tolerates null and never unwinds (panic = "abort" in release).

use crate::{devices, machine::{Machine, Region}};
use std::ffi::{c_char, c_int, c_uint};

#[no_mangle]
pub extern "C" fn fm1ntt_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// A machine with nothing loaded, or null: `fm1ntt_load_*` replaces it. (Boxed `Option` so a host can hold one handle for its life.)
pub struct Handle(Option<Machine>);

#[no_mangle]
pub extern "C" fn fm1ntt_new() -> *mut Handle {
    Box::into_raw(Box::new(Handle(None)))
}

/// # Safety
/// `h` must be null or from `fm1ntt_new`, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_free(h: *mut Handle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

fn install(h: &mut Handle, r: crate::Result<Machine>) -> c_int {
    match r {
        Ok(m) => {
            h.0 = Some(m);
            set_error(None)
        }
        Err(e) => set_error(Some(e.to_string())),
    }
}

/// Boots a `.fwsc` firmware package. 0 on success, else -1 and `fm1ntt_error` says why.
///
/// # Safety
/// `h` is a live handle and `bytes` points at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_load_package(h: *mut Handle, bytes: *const u8, len: usize) -> c_int {
    let (Some(h), false) = (h.as_mut(), bytes.is_null()) else { return -1 };
    install(h, Machine::from_package(std::slice::from_raw_parts(bytes, len)))
}

/// Boots a raw execute-in-place image at `entry`. 0 on success, else -1.
///
/// # Safety
/// As `fm1ntt_load_package`.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_load_raw(h: *mut Handle, bytes: *const u8, len: usize, entry: u32) -> c_int {
    let (Some(h), false) = (h.as_mut(), bytes.is_null()) else { return -1 };
    install(h, Machine::from_raw(std::slice::from_raw_parts(bytes, len).to_vec(), entry))
}

/// 1 when a firmware is loaded.
///
/// # Safety
/// `h` is null or a live handle.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_loaded(h: *const Handle) -> c_int {
    h.as_ref().is_some_and(|h| h.0.is_some()) as c_int
}

pub(crate) fn with<R>(h: *mut Handle, default: R, f: impl FnOnce(&mut Machine) -> R) -> R {
    // SAFETY: the callers' contract is a live handle or null.
    match unsafe { h.as_mut() }.and_then(|h| h.0.as_mut()) {
        Some(m) => f(m),
        None => default,
    }
}

/// Renders `frames` stereo frames (2 x `frames` i16) into `out`, running at most `max_insns` instructions. Returns the real frames.
///
/// # Safety
/// `h` is live and `out` has room for `2 * frames` samples.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_render(h: *mut Handle, out: *mut i16, frames: usize, max_insns: u32) -> usize {
    if out.is_null() || frames > 65_536 {
        return 0;
    }
    let buf = std::slice::from_raw_parts_mut(out, frames * 2);
    buf.fill(0);
    with(h, 0, |m| m.render(buf, max_insns))
}

/// Writes the 240x240 screen as 0x00RRGGBB into `out` (57,600 words); black when nothing is loaded.
///
/// # Safety
/// `h` is live or null and `out` has room for 57,600 words.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_screen(h: *mut Handle, out: *mut u32) {
    if out.is_null() {
        return;
    }
    let px = std::slice::from_raw_parts_mut(out, devices::SCREEN_W * devices::SCREEN_H);
    px.fill(0);
    with(h, (), |m| m.screen(px));
}

/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_key(h: *mut Handle, id: c_int, down: c_int) {
    if let Ok(id) = u8::try_from(id) {
        with(h, (), |m| m.key(id, down != 0));
    }
}

/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_encoder(h: *mut Handle, enc: c_int, clicks: c_int) {
    if let Ok(e) = u8::try_from(enc) {
        with(h, (), |m| m.encoder(e, clicks));
    }
}

/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_pot(h: *mut Handle, value: c_int) {
    with(h, (), |m| m.pot(value.clamp(0, 1023) as u16));
}

/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_midi_in(h: *mut Handle, byte: c_int) {
    if let Ok(b) = u8::try_from(byte) {
        with(h, (), |m| m.midi_in(b));
    }
}

/// 1 when the emulated CPU has stopped (a fault: the log says nothing more yet).
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_halted(h: *mut Handle) -> c_int {
    with(h, 0, |m| m.halted() as c_int)
}

/// Matrix ids held down, bit n = id n (0..13 buttons, 14..40 keys).
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_keys_down(h: *mut Handle) -> u64 {
    with(h, 0, |m| m.keys_down())
}

/// Emulated milliseconds since boot.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_ms(h: *mut Handle) -> u64 {
    with(h, 0, |m| m.ms())
}

/// Changes whenever the screen has been redrawn, so a host can skip an unchanged one.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_lcd_revision(h: *mut Handle) -> u32 {
    with(h, 0, |m| m.lcd_revision())
}

/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_jit(h: *mut Handle, on: c_int) {
    with(h, (), |m| m.set_jit(on != 0));
}

/// Main-loop pacing 0 (off, exact) to 3: slows the firmware's main loop, not its interrupts, to leave a slow host time. Off by default.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_pace(h: *mut Handle, level: c_int) {
    with(h, (), |m| m.set_pace(level.clamp(0, c_int::from(crate::machine::PACE_LEVELS)) as u8));
}

/// The pacing level now.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_pace_level(h: *mut Handle) -> c_int {
    with(h, 0, |m| c_int::from(m.pace()))
}

/// Chooses the FM-1's flash chip by name (`p25q80`, `puya`, `stock`, `old`; `uc25hq80`, `uc25`, `new`; any case): the JEDEC id a firmware reads from it.
/// Returns 1 when the name was known and set, 0 for an unknown name, NULL or a NULL machine (nothing changes). Call it before the first run; it
/// is kept across a reset.
///
/// # Safety
/// `h` is live or null; `name` is a NUL-terminated string or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_flash_part(h: *mut Handle, name: *const c_char) -> c_int {
    if name.is_null() {
        return 0;
    }
    let Ok(name) = std::ffi::CStr::from_ptr(name).to_str() else { return 0 };
    let Some(part) = crate::machine::FlashPart::parse(name) else { return 0 };
    with(h, 0, |m| {
        m.set_flash_part(part);
        1
    })
}

/// 1 when `name` is one `fm1ntt_flash_part` knows, 0 when not (or NULL). Needs no machine, so a host can check an option before anything loads.
///
/// # Safety
/// `name` is a NUL-terminated string or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_flash_part_known(name: *const c_char) -> c_int {
    if name.is_null() {
        return 0;
    }
    std::ffi::CStr::from_ptr(name).to_str().ok().and_then(crate::machine::FlashPart::parse).is_some() as c_int
}

/// The flash chip's JEDEC id as 0xMMTTCC (manufacturer, type, capacity code), e.g. 0x856014; 0 for a NULL machine.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_flash_jedec(h: *mut Handle) -> c_uint {
    with(h, 0, |m| {
        let j = m.flash_jedec();
        u32::from(j[0]) << 16 | u32::from(j[1]) << 8 | u32::from(j[2])
    })
}

/// One core's state, for the CPU panel.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Fm1Cpu {
    pub pc: u32,
    pub sp: u32,
    pub flags: u32,
    pub regs: [u32; 16],
    pub insns: u64,
}

/// Core 0 or 1's state into `out`; 0 when there is no such core (core 1 until the firmware releases it) or nothing is loaded.
///
/// # Safety
/// `h` is live or null and `out` is null or points at one `Fm1Cpu`.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_cpu(h: *mut Handle, core: c_int, out: *mut Fm1Cpu) -> c_int {
    let (Some(out), Ok(core)) = (out.as_mut(), usize::try_from(core)) else { return 0 };
    with(h, 0, |m| match m.cpu_view(core) {
        Some(v) => {
            *out = Fm1Cpu { pc: v.pc, sp: v.sp, flags: v.flags, regs: v.regs, insns: v.insns };
            1
        }
        None => 0,
    })
}

fn region_of(r: c_int) -> Option<Region> {
    match r {
        0 => Some(Region::Ram),
        1 => Some(Region::Xip),
        2 => Some(Region::Flash),
        _ => None,
    }
}

/// A memory region's chip address and size (0 RAM, 1 execute-in-place image, 2 external flash); 0 when none or nothing is loaded.
///
/// # Safety
/// `h` is live or null; `base` and `size` are null or valid.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_region(h: *mut Handle, region: c_int, base: *mut u32, size: *mut usize) -> c_int {
    let Some(r) = region_of(region) else { return 0 };
    with(h, 0, |m| {
        let (b, n) = m.region(r);
        if let Some(p) = base.as_mut() {
            *p = b;
        }
        if let Some(p) = size.as_mut() {
            *p = n;
        }
        1
    })
}

/// Copies up to `len` bytes from `offset` of a region into `out`; returns how many. Reads no peripheral.
///
/// # Safety
/// `h` is live or null and `out` has room for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_read_mem(h: *mut Handle, region: c_int, offset: usize, out: *mut u8, len: usize) -> usize {
    let Some(r) = region_of(region) else { return 0 };
    if out.is_null() || len > 1 << 20 {
        return 0;
    }
    let buf = std::slice::from_raw_parts_mut(out, len);
    with(h, 0, |m| m.read_mem(r, offset, buf))
}

/// Copies the last `cap` bytes of the firmware's debug console into `out`; returns how many.
///
/// # Safety
/// `h` is live or null and `out` has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_console(h: *mut Handle, out: *mut u8, cap: usize) -> usize {
    if out.is_null() || cap > 1 << 20 {
        return 0;
    }
    with(h, 0, |m| {
        let t = m.console_tail(cap);
        std::slice::from_raw_parts_mut(out, t.len()).copy_from_slice(t);
        t.len()
    })
}

/// MIDI bytes waiting behind the emulated jack, and bytes it dropped.
///
/// # Safety
/// `h` is live or null; the pointers are null or valid.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_midi_stats(h: *mut Handle, queued: *mut u64, dropped: *mut u64) {
    let (q, d) = with(h, (0, 0), |m| {
        let (q, d) = m.midi_stats();
        (q as u64, d)
    });
    if let Some(p) = queued.as_mut() {
        *p = q;
    }
    if let Some(p) = dropped.as_mut() {
        *p = d;
    }
}

/// Back to power-on with the same firmware.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_reset(h: *mut Handle) {
    with(h, (), |m| m.reset());
}

/// The size of the emulated flash, for a buffer to hold `fm1ntt_flash_save`.
#[no_mangle]
pub extern "C" fn fm1ntt_flash_size() -> usize {
    fm1_soc::FLASH_SIZE
}

/// A counter that changes whenever the emulated flash does (0 for a null handle).
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_flash_revision(h: *mut Handle) -> u32 {
    with(h, 0, |m| m.flash_revision())
}

/// Copies the emulated flash (1 MiB) into `out` if `cap` is big enough; returns the bytes copied, 0 if not.
///
/// # Safety
/// `h` is live or null and `out` has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_flash_save(h: *mut Handle, out: *mut u8, cap: usize) -> usize {
    if out.is_null() {
        return 0;
    }
    with(h, 0, |m| {
        let f = m.flash_image();
        if cap < f.len() {
            return 0;
        }
        std::slice::from_raw_parts_mut(out, f.len()).copy_from_slice(f);
        f.len()
    })
}

/// Resets, then restores a saved flash image. 0 on success, else -1 (`fm1ntt_error`).
///
/// # Safety
/// `h` is live or null and `bytes` points at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_flash_restore(h: *mut Handle, bytes: *const u8, len: usize) -> c_int {
    if bytes.is_null() {
        return -1;
    }
    let b = std::slice::from_raw_parts(bytes, len);
    with(h, -1, |m| set_error(m.restore_flash(b).err().map(|e| e.to_string())))
}

/// 1 when hot code is compiled to native code.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_jit_on(h: *mut Handle) -> c_int {
    with(h, 0, |m| m.jit_on() as c_int)
}

/// Count the JIT's entries and the instructions run compiled (set before the first run; it slows the run, so never on for a timed one).
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_jit_profile(h: *mut Handle, on: c_int) {
    with(h, (), |m| m.set_jit_profile(on != 0));
}

/// One line of JSON about the JIT (blocks cold / compiled / given up on, code cache bytes, entries, instructions run compiled and in all) into `out`
/// as a NUL-terminated string; returns its length, 0 when `cap` is too small or the handle is null.
///
/// # Safety
/// `h` is live or null; `out` has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_jit_report(h: *mut Handle, out: *mut u8, cap: usize) -> usize {
    if out.is_null() {
        return 0;
    }
    let text = with(h, String::new(), |m| m.jit_report());
    if text.is_empty() || text.len() + 1 > cap {
        return 0;
    }
    std::ptr::copy_nonoverlapping(text.as_ptr(), out, text.len());
    *out.add(text.len()) = 0;
    text.len()
}

/// The benchmark workload (a loop of nops and a goto: `bench.rs`) into `out`; returns its size, 0 if `cap` is too small.
///
/// # Safety
/// `out` has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_bench_image(out: *mut u8, cap: usize) -> usize {
    let img = crate::bench::nop_loop_image();
    if out.is_null() || cap < img.len() {
        return 0;
    }
    std::slice::from_raw_parts_mut(out, img.len()).copy_from_slice(&img);
    img.len()
}

/// 1 when this build can compile hot code to native code (the CPU is one the vendored core supports).
#[no_mangle]
pub extern "C" fn fm1ntt_jit_available() -> c_int {
    crate::machine::native_jit_available() as c_int
}

/// The emulated clock in MHz (instructions a second that real time needs).
#[no_mangle]
pub extern "C" fn fm1ntt_clock_mhz() -> u32 {
    crate::machine::CLOCK_MHZ as u32
}

/// The workload's version, recorded in a benchmark report.
#[no_mangle]
pub extern "C" fn fm1ntt_bench_version() -> u32 {
    crate::bench::WORKLOAD_VERSION
}

thread_local! {
    static BOARD: std::cell::RefCell<std::ffi::CString> = std::cell::RefCell::new(Default::default());
}

/// The board the package is for ("Fm1", "Smk37"), empty for a raw image; valid until the next call on this thread.
///
/// # Safety
/// `h` is live or null.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_board(h: *mut Handle) -> *const c_char {
    let name = with(h, String::new(), |m| m.board_name());
    BOARD.with(|b| {
        *b.borrow_mut() = std::ffi::CString::new(name).unwrap_or_default();
        b.borrow().as_ptr()
    })
}

thread_local! {
    static LAST_ERROR: std::cell::RefCell<std::ffi::CString> = std::cell::RefCell::new(Default::default());
}

pub(crate) fn set_error(e: Option<String>) -> c_int {
    let failed = e.is_some();
    let text = e.unwrap_or_default().replace('\0', " ");
    LAST_ERROR.with(|l| *l.borrow_mut() = std::ffi::CString::new(text).unwrap_or_default());
    if failed { -1 } else { 0 }
}

/// The last failure's text, valid until the next call on this thread; empty when none.
#[no_mangle]
pub extern "C" fn fm1ntt_error() -> *const c_char {
    LAST_ERROR.with(|l| l.borrow().as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn c_api_round_trip_and_null_safety() {
        unsafe {
            let h = fm1ntt_new();
            assert_eq!(fm1ntt_loaded(h), 0);
            assert_eq!(fm1ntt_load_package(h, [1u8, 2, 3].as_ptr(), 3), -1);
            assert!(!CStr::from_ptr(fm1ntt_error()).to_str().unwrap().is_empty());
            assert_eq!(fm1ntt_load_package(h, std::ptr::null(), 0), -1);
            let mut px = vec![1u32; devices::SCREEN_W * devices::SCREEN_H];
            fm1ntt_screen(h, px.as_mut_ptr());
            assert_eq!(px[0], 0);
            assert_eq!(fm1ntt_load_raw(h, [0u8; 64].as_ptr(), 64, 0x0200_0120), 0);
            assert_eq!(fm1ntt_loaded(h), 1);
            fm1ntt_key(h, -1, 1);
            fm1ntt_key(h, 14, 1);
            assert_eq!(fm1ntt_keys_down(h), 1 << 14);
            let mut a = vec![7i16; 128];
            assert!(fm1ntt_render(h, a.as_mut_ptr(), 64, 20_000) <= 64);
            assert_eq!(fm1ntt_render(h, std::ptr::null_mut(), 64, 1), 0);
            assert_eq!(fm1ntt_render(h, a.as_mut_ptr(), 1 << 20, 1), 0);
            let mut cpu = Fm1Cpu::default();
            assert_eq!(fm1ntt_cpu(h, 0, &mut cpu), 1);
            assert_eq!(fm1ntt_cpu(h, 1, &mut cpu), 0);
            assert_eq!(fm1ntt_cpu(h, -1, &mut cpu), 0);
            let (mut base, mut size) = (0u32, 0usize);
            assert_eq!(fm1ntt_region(h, 0, &mut base, &mut size), 1);
            assert_eq!(base, 0x01c0_0000);
            assert_eq!(fm1ntt_region(h, 9, &mut base, &mut size), 0);
            let mut mem = [0u8; 8];
            assert_eq!(fm1ntt_read_mem(h, 1, 0, mem.as_mut_ptr(), 8), 8);
            assert_eq!(fm1ntt_read_mem(h, 9, 0, mem.as_mut_ptr(), 8), 0);
            let mut flash = vec![0u8; fm1ntt_flash_size()];
            assert_eq!(fm1ntt_flash_save(h, flash.as_mut_ptr(), 10), 0);
            assert_eq!(fm1ntt_flash_save(h, flash.as_mut_ptr(), flash.len()), flash.len());
            assert_eq!(fm1ntt_flash_restore(h, flash.as_ptr(), flash.len()), 0);
            assert_eq!(fm1ntt_flash_restore(h, flash.as_ptr(), 5), -1);
            let _ = fm1ntt_flash_revision(h);
            assert_eq!(fm1ntt_flash_revision(std::ptr::null_mut()), 0);
            fm1ntt_reset(h);
            assert_eq!(fm1ntt_keys_down(h), 0);
            assert_eq!(fm1ntt_jit_on(h), 0);
            fm1ntt_pace(h, 99);
            assert_eq!(fm1ntt_pace_level(h), 3);
            fm1ntt_pace(h, -4);
            assert_eq!(fm1ntt_pace_level(h), 0);
            fm1ntt_pace(std::ptr::null_mut(), 2);
            assert_eq!(fm1ntt_pace_level(std::ptr::null_mut()), 0);
            // the flash chip: the stock Puya by default, the newer UC25HQ80 by name, an unknown or null name changes nothing, a reset keeps it
            assert_eq!((fm1ntt_flash_part_known(c"puya".as_ptr()), fm1ntt_flash_part_known(c"x".as_ptr()), fm1ntt_flash_part_known(std::ptr::null())), (1, 0, 0));
            assert_eq!(fm1ntt_flash_jedec(h), 0x85_60_14);
            assert_eq!(fm1ntt_flash_part(h, c"UC25HQ80".as_ptr()), 1);
            assert_eq!(fm1ntt_flash_jedec(h), 0xb3_60_14);
            assert_eq!(fm1ntt_flash_part(h, c"winbond".as_ptr()), 0);
            assert_eq!(fm1ntt_flash_part(h, std::ptr::null()), 0);
            assert_eq!(fm1ntt_flash_part(std::ptr::null_mut(), c"new".as_ptr()), 0);
            fm1ntt_reset(h);
            assert_eq!(fm1ntt_flash_jedec(h), 0xb3_60_14);
            assert_eq!(fm1ntt_flash_part(h, c"stock".as_ptr()), 1);
            assert_eq!(fm1ntt_flash_jedec(h), 0x85_60_14);
            assert_eq!(fm1ntt_flash_jedec(std::ptr::null_mut()), 0);
            assert!(CStr::from_ptr(fm1ntt_board(h)).to_bytes().is_empty());
            let (mut q, mut d) = (9u64, 9u64);
            fm1ntt_midi_stats(h, &mut q, &mut d);
            assert_eq!((q, d), (0, 0));
            let mut con = [0u8; 4];
            assert_eq!(fm1ntt_console(h, con.as_mut_ptr(), 4), 0);
            fm1ntt_free(h);
            fm1ntt_free(std::ptr::null_mut());
            assert_eq!(fm1ntt_halted(std::ptr::null_mut()), 0);
        }
    }
}

/// What `fm1ntt_audition` reports (see `core/src/audition.rs`). `verdict`: 0 Pass, 1 Silent, 2 Blank, 3 Halted. `sha256` is 64 lowercase hex digits and a NUL.
#[repr(C)]
pub struct Fm1nttAudition {
    pub verdict: c_int,
    pub colours: u32,
    pub peak: i32,
    pub emulated_ms: u32,
    pub size: u32,
    /// the final screen's PNG: its length (set even when it did not fit `png_cap`)
    pub png_len: u32,
    pub rms_db: f64,
    pub active: f64,
    pub sha256: [c_char; 65],
}

/// Runs the standard audition (a fixed script of four notes, no wall clock: the same firmware gives the same result on any computer) on a `.fwsc` package.
/// 0 on success, else -1 and `fm1ntt_error` says why (not a package, or a null argument). The screen PNG is copied to `png` when `png_cap` holds it.
///
/// # Safety
/// `data` must point to `len` readable bytes, `out` to one `Fm1nttAudition`, and `png` (when not null) to `png_cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_audition(data: *const u8, len: usize, out: *mut Fm1nttAudition, png: *mut u8, png_cap: usize) -> c_int {
    if data.is_null() || out.is_null() || len > 64 << 20 {
        return set_error(Some("no firmware to audition".into()));
    }
    match crate::audition::audition_standard(std::slice::from_raw_parts(data, len)) {
        Ok(a) => {
            let o = &mut *out;
            o.verdict = match a.verdict {
                crate::audition::Verdict::Pass => 0,
                crate::audition::Verdict::Silent => 1,
                crate::audition::Verdict::Blank => 2,
                crate::audition::Verdict::Halted => 3,
            };
            o.colours = a.colours as u32;
            o.peak = a.peak;
            o.emulated_ms = a.emulated_ms;
            o.size = a.size as u32;
            o.png_len = a.screen_png.len() as u32;
            o.rms_db = a.rms_db;
            o.active = a.active;
            o.sha256 = [0; 65];
            for (d, s) in o.sha256.iter_mut().zip(a.sha256.bytes().take(64)) {
                *d = s as c_char;
            }
            if !png.is_null() && png_cap >= a.screen_png.len() {
                std::ptr::copy_nonoverlapping(a.screen_png.as_ptr(), png, a.screen_png.len());
            }
            set_error(None)
        }
        Err(e) => set_error(Some(e.to_string())),
    }
}

#[cfg(test)]
mod audition_tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn the_c_face_auditions_and_refuses_a_non_package() {
        unsafe {
            let mut o: Fm1nttAudition = std::mem::zeroed();
            assert_eq!(fm1ntt_audition(std::ptr::null(), 0, &mut o, std::ptr::null_mut(), 0), -1);
            let junk = [1u8, 2, 3];
            assert_eq!(fm1ntt_audition(junk.as_ptr(), 3, &mut o, std::ptr::null_mut(), 0), -1);
            assert!(!CStr::from_ptr(fm1ntt_error()).to_bytes().is_empty());
            // a bundled firmware, when present, passes and its PNG is returned
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../bundled-firmware/haunted-fm1");
            if let Some(f) = std::fs::read_dir(dir).ok().and_then(|d| d.flatten().find(|e| e.path().extension().is_some_and(|x| x == "fwsc"))) {
                let fw = std::fs::read(f.path()).unwrap();
                assert_eq!(fm1ntt_audition(fw.as_ptr(), fw.len(), &mut o, std::ptr::null_mut(), 0), 0);
                assert!(o.png_len > 1000);
                let mut png = vec![0u8; o.png_len as usize];
                assert_eq!(fm1ntt_audition(fw.as_ptr(), fw.len(), &mut o, png.as_mut_ptr(), png.len()), 0);
                assert_eq!(&png[1..4], b"PNG");
                assert_eq!(CStr::from_ptr(o.sha256.as_ptr()).to_bytes().len(), 64);
            }
        }
    }
}
