//! The whole FM-1, driven a quantum of audio at a time. After `fm1-web`'s machine (vendored `fm1-emu`, MIT), without its browser
//! exports and pacing: the audio device paces it, so one `render` runs the emulated machine until the DMA has made the frames.
//! Deterministic for the same image and input (no clock is read).

use crate::{devices, firmware, Error, Result};
use fm1_core::Cpu;
use fm1_soc::{machine, Action, Soc};
use std::collections::VecDeque;

const RAM_BASE: u32 = 0x01c0_0000;
const RAM_SIZE: usize = 1 << 20;
const DEFAULT_MHZ: u64 = 240;
const SECTOR: usize = 4096;
/// Instructions per slice between audio checks (fm1-web's).
const SLICE: u32 = 20_000;
/// Pacing levels: (instructions run, then stalled for that times the ratio). Chosen on an Apple M1 with the bundled firmwares, the
/// Pack's and Omni: the audio's level and pitch held within 5% and the key-to-sound time did not change (docs/PERFORMANCE.md).
pub const PACE_LEVELS: u8 = 3;
const PACE: [(u32, f64); 4] = [(0, 0.0), (2000, 3.0), (1000, 6.0), (1000, 10.0)];

/// What a boot needs, kept so the machine can be reset to power-on (and a saved flash restored into a fresh one).
struct Boot {
    image: Vec<u8>,
    entry: u32,
    /// the package's whole flash and its board (a raw image has none)
    stock: Option<(Vec<u8>, fm1_soc::Board)>,
}

/// The memory a view can read without touching a peripheral: RAM, the execute-in-place image, the external flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Ram,
    Xip,
    Flash,
}

/// One core's visible state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuView {
    pub pc: u32,
    pub sp: u32,
    pub flags: u32,
    pub regs: [u32; 16],
    pub insns: u64,
}

/// Whether this build can compile hot code (the vendored `fm1-core`'s own list of hosts). On wasm32 it compiles to WebAssembly through a hook
/// the host supplies (`env.fm1_jit_compile`, `web/public/fm1ntt.js`): a module that is running at all has that import, so it is available.
pub fn native_jit_available() -> bool {
    cfg!(any(
        target_arch = "wasm32",
        all(target_arch = "aarch64", any(target_os = "macos", target_os = "linux", target_os = "android")),
        all(target_arch = "x86_64", any(target_os = "linux", target_os = "android", target_os = "windows", target_os = "macos"))
    ))
}

/// The emulated clock in MHz: one instruction a cycle, so this is the instructions a second real time needs.
pub const CLOCK_MHZ: u64 = DEFAULT_MHZ;

pub struct Machine {
    boot: Boot,
    cpu: Cpu,
    /// the second core, once the firmware releases it
    cpu1: Option<Cpu>,
    soc: Soc,
    pending_tick: bool,
    /// interleaved L/R i16 not yet handed out
    audio: VecDeque<i16>,
    halted: bool,
    started: bool,
    /// matrix ids held down (bit n), kept for the host's display
    keys: u64,
    instructions: u64,
    identity: String,
    /// main-loop pacing, 0 (off) to `PACE_LEVELS`: see `set_pace`
    pace: u8,
}

impl Machine {
    fn build(boot: &Boot) -> (Soc, Cpu) {
        let mut soc = Soc::new(RAM_SIZE, boot.image.clone());
        soc.periph.hz = DEFAULT_MHZ * 1_000_000;
        soc.periph.skip_idle = true;
        soc.set_flash_part(fm1_soc::FlashPart::P25q80);
        let mut cpu = Cpu::new(boot.entry);
        cpu.user = true; // the SPL hands cpu0 over in user mode
        let params = RAM_BASE + 0x7fc00;
        fm1_core::Bus::write32(&mut soc, params, params + 0x40);
        cpu.regs[0] = params;
        if let Some((flash, board)) = &boot.stock {
            cpu.regs[0] = fm1_soc::stock_boot(&mut soc, flash, params, *board);
        }
        (soc, cpu)
    }

    fn boot(image: Vec<u8>, entry: u32, stock: Option<(Vec<u8>, fm1_soc::Board)>, identity: String) -> Machine {
        let boot = Boot { image, entry, stock };
        let (soc, cpu) = Machine::build(&boot);
        Machine { boot, cpu, cpu1: None, soc, pending_tick: false, audio: VecDeque::new(), halted: false, started: false, keys: 0, instructions: 0, identity, pace: 0 }
    }

    /// Back to power-on with the same firmware: the emulated flash returns to what the package carried, and the inputs are released.
    pub fn reset(&mut self) {
        let jit = self.cpu.jit_on;
        let (soc, mut cpu) = Machine::build(&self.boot);
        cpu.jit_on = jit;
        self.soc = soc;
        self.cpu = cpu;
        self.cpu1 = None;
        self.pending_tick = false;
        self.audio.clear();
        self.halted = false;
        self.started = false;
        self.keys = 0;
        self.instructions = 0;
    }

    /// The emulated external flash (1 MiB): what a firmware saved its patches and settings into.
    pub fn flash_image(&self) -> &[u8] {
        &self.soc.flash.mem
    }

    /// Writes whole bytes into the flash and, where it overlaps, the execute-in-place view (which holds the decrypted app, so it is never
    /// overwritten with raw flash): what a runtime write by the firmware does (upstream's `replace_flash`).
    fn replace_flash(&mut self, at: usize, data: &[u8]) {
        self.soc.flash.mem[at..at + data.len()].copy_from_slice(data);
        let from = at.max(fm1_soc::XIP_FLASH_OFFSET);
        let end = (at + data.len()).min(fm1_soc::XIP_FLASH_OFFSET + self.soc.xip.len());
        if from < end {
            self.soc.xip[from - fm1_soc::XIP_FLASH_OFFSET..end - fm1_soc::XIP_FLASH_OFFSET].copy_from_slice(&data[from - at..end - at]);
        }
        self.soc.flash.revision = self.soc.flash.revision.wrapping_add(1);
    }

    /// Resets, then restores a saved flash image: **only the 4 KiB sectors that differ from what the package boots with** are written (what the
    /// firmware saved at run time: its patches and settings), as upstream's restore does. Copying the whole image over the execute-in-place
    /// view would replace the decrypted app with encrypted bytes and halt the machine (found with a real firmware, 2026-11-16).
    pub fn restore_flash(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() != fm1_soc::FLASH_SIZE {
            return Err(Error::BadImage(format!("a flash image is {} bytes, this one {}", fm1_soc::FLASH_SIZE, bytes.len())));
        }
        self.reset();
        let base = self.soc.flash.mem.clone();
        for at in (0..bytes.len()).step_by(SECTOR) {
            if bytes[at..at + SECTOR] != base[at..at + SECTOR] {
                self.replace_flash(at, &bytes[at..at + SECTOR]);
            }
        }
        Ok(())
    }

    /// Boots a firmware update package (`.fwsc`), the form community firmwares are shared in.
    pub fn from_package(pkg: &[u8]) -> Result<Machine> {
        firmware::check_size(pkg)?;
        let st = fm1_soc::stock_from_package(pkg, None).map_err(Error::BadImage)?;
        Ok(Machine::boot(st.xip, st.entry, Some((st.flash, st.board)), st.identity))
    }

    /// Boots a raw execute-in-place image at `entry` (0x02000120 for Felucca's `felucca_xip.bin`).
    pub fn from_raw(image: Vec<u8>, entry: u32) -> Result<Machine> {
        firmware::check_size(&image)?;
        Ok(Machine::boot(image, entry, None, String::new()))
    }

    fn run(&mut self, n: u32) {
        let (piece, ratio) = PACE[usize::from(self.pace)];
        if piece == 0 || self.cpu1.is_some() {
            return self.run_unpaced(n);
        }
        // Pacing: the main loop runs a piece, then stalls (the clock moves, no instruction runs) for `ratio` times as long, never past
        // the next timed event and never inside an interrupt handler or with one pending. To the firmware that is a slower clock in its
        // main loop only, which is a legal execution: interrupts, the SPI and the audio come when they would. What it skips is mostly
        // busy-waiting on the emulated SPI and LCD, so the screen and the keys behave the same (docs/PERFORMANCE.md).
        let mut left = n;
        while left > 0 && !self.halted {
            let k = left.min(piece);
            self.run_unpaced(k);
            left -= k;
            let p = &mut self.soc.periph;
            if !p.in_isr && !p.any_pending() {
                p.room_cap = u64::MAX; // (the run set it to its own length; `run_dual` sets it again on the next call)
                let stall = ((f64::from(k) * ratio) as u64).min(p.tick_room().saturating_sub(2));
                p.cycles += stall;
                p.skipped += stall;
            }
        }
    }

    fn run_unpaced(&mut self, n: u32) {
        let jit = self.cpu.jit_on;
        let Machine { cpu, cpu1, soc, pending_tick, .. } = self;
        if machine::run_dual(cpu1, soc, n, RAM_SIZE, jit, |soc, k| machine::run_blocks(cpu, soc, k, pending_tick)).is_err() {
            self.halted = true;
        }
    }

    /// Fills `out` (interleaved stereo i16, 44.1 kHz) by running the machine until it has made the frames or `max_insns`
    /// instructions have run. Returns the frames that are real audio; the rest of `out` is silence.
    pub fn render(&mut self, out: &mut [i16], max_insns: u32) -> usize {
        self.started = true;
        let frames = out.len() / 2;
        let mut spent = 0u32;
        while spent < max_insns && !self.halted && self.audio.len() / 2 < frames {
            self.run(SLICE);
            spent += SLICE;
            self.audio.extend(self.soc.periph.audio.drain(..));
        }
        self.instructions += u64::from(spent);
        let have = (self.audio.len() / 2).min(frames);
        for (i, o) in out.iter_mut().enumerate() {
            *o = if i / 2 < have { self.audio.pop_front().unwrap_or(0) } else { 0 };
        }
        have
    }

    /// Main-loop pacing, 0 (off, exact) to 3 (the most): the firmware's main loop is slowed to leave the host time, and nothing else is.
    /// A firmware with one loop that never idles (the nine tried here: they redraw as fast as the SPI lets them) keeps every emulated
    /// cycle busy, and a slow host cannot keep up; its interrupts (sound, keys, the SPI) are not slowed. Off by default, and
    /// deterministic: the same level gives the same machine. Not used while core 1 runs (the stock firmware renders on it).
    pub fn set_pace(&mut self, level: u8) {
        self.pace = level.min(PACE_LEVELS);
    }
    pub fn pace(&self) -> u8 {
        self.pace
    }

    /// Turns the second-by-second compile of hot code to native code on or off (off by default; speed on the R36S is untested).
    pub fn set_jit(&mut self, on: bool) {
        self.cpu.jit_on = on;
        if let Some(c1) = self.cpu1.as_mut() {
            c1.jit_on = on;
        }
    }

    /// The 240x240 screen as 0x00RRGGBB into `out` (57,600 words).
    pub fn screen(&self, out: &mut [u32]) {
        devices::rgb565_to_xrgb(&self.soc.lcd.fb, out);
    }
    pub fn lcd_revision(&self) -> u32 {
        self.soc.lcd.revision
    }

    /// A button (id 0..13) or note key (id 14..40) goes down or up. Other ids are ignored.
    pub fn key(&mut self, id: u8, down: bool) {
        if usize::from(id) < devices::MATRIX_IDS {
            self.soc.periph.apply(&Action::Key { id, down });
            if down { self.keys |= 1 << id } else { self.keys &= !(1 << id) }
        }
    }
    /// Turns encoder 0..6 by `clicks` detents (positive is clockwise).
    pub fn encoder(&mut self, enc: u8, clicks: i32) {
        if usize::from(enc) < devices::ENCODERS {
            self.soc.periph.apply(&Action::Enc { enc, clicks: clicks.clamp(-64, 64) });
        }
    }
    /// The pot, 0..1023.
    pub fn pot(&mut self, value: u16) {
        self.soc.periph.apply(&Action::Adc { ch: devices::POT_CHANNEL, value: value.min(1023) });
    }
    /// One MIDI byte into the MIDI IN jack.
    pub fn midi_in(&mut self, b: u8) {
        self.soc.periph.apply(&Action::Midi { b });
    }

    pub fn halted(&self) -> bool {
        self.halted
    }
    pub fn keys_down(&self) -> u64 {
        self.keys
    }
    pub fn instructions(&self) -> u64 {
        self.instructions
    }
    /// The package's own name for the board it is for (empty for a raw image).
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn ms(&self) -> u64 {
        self.soc.periph.ms()
    }
    pub fn started(&self) -> bool {
        self.started
    }

    pub fn jit_on(&self) -> bool {
        self.cpu.jit_on
    }

    /// Count JIT entries and the instructions run in compiled code from now on (set it before the first run: the JIT is made with it).
    /// It slows the run a little, so a timed run never has it on.
    pub fn set_jit_profile(&mut self, on: bool) {
        self.cpu.profile = on;
    }

    /// One line of JSON about the JIT: blocks cold, compiled and given up on, code cache bytes, and (with the profile on) entries, instructions run compiled and in all.
    pub fn jit_report(&self) -> String {
        let (cold, compiled, given_up, bytes) = self.cpu.jit_counts();
        format!(
            "{{\"blocks_cold\":{cold},\"blocks_compiled\":{compiled},\"blocks_given_up\":{given_up},\"cache_bytes\":{bytes},\"entries\":{},\"jit_insns\":{},\"insns\":{}}}",
            self.cpu.jit_entries, self.cpu.jit_insns, self.cpu.insn_count
        )
    }

    /// Core 0 or 1 (None for core 1 until the firmware releases it).
    pub fn cpu_view(&self, core: usize) -> Option<CpuView> {
        let c = match core {
            0 => &self.cpu,
            1 => self.cpu1.as_ref()?,
            _ => return None,
        };
        Some(CpuView { pc: c.pc, sp: c.sp, flags: c.flags, regs: c.regs, insns: c.insn_count })
    }

    /// The region's size in bytes, and the address it has on the chip.
    pub fn region(&self, r: Region) -> (u32, usize) {
        match r {
            Region::Ram => (RAM_BASE, RAM_SIZE),
            Region::Xip => (fm1_soc::XIP_BASE, self.soc.xip.len()),
            Region::Flash => (0, self.soc.flash.mem.len()),
        }
    }

    /// Copies up to `out.len()` bytes from `offset` in the region; returns how many (0 past the end). Reads no peripheral.
    pub fn read_mem(&self, r: Region, offset: usize, out: &mut [u8]) -> usize {
        let n = match r {
            Region::Ram => {
                let have = self.soc.ram.len().min(RAM_SIZE);
                let n = have.saturating_sub(offset).min(out.len());
                for (i, o) in out[..n].iter_mut().enumerate() {
                    *o = self.soc.ram.read8((offset + i) as u32);
                }
                return n;
            }
            Region::Xip => self.soc.xip.get(offset..).map(|s| (s, s.len())),
            Region::Flash => self.soc.flash.mem.get(offset..).map(|s| (s, s.len())),
        };
        match n {
            Some((s, len)) => {
                let n = len.min(out.len());
                out[..n].copy_from_slice(&s[..n]);
                n
            }
            None => 0,
        }
    }

    /// The last `max` bytes the firmware wrote to its debug console.
    pub fn console_tail(&self, max: usize) -> &[u8] {
        let c = &self.soc.console;
        &c[c.len().saturating_sub(max)..]
    }

    /// MIDI bytes waiting behind the jack, and bytes dropped (no receiver set up, or a full queue).
    pub fn midi_stats(&self) -> (usize, u64) {
        (self.soc.periph.midi_queued(), self.soc.periph.midi_dropped())
    }

    /// The board the package is for ("Fm1", "Smk37"); empty for a raw image.
    pub fn board_name(&self) -> String {
        self.boot.stock.as_ref().map(|(_, b)| format!("{b:?}")).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_package_is_refused_with_a_reason_not_a_panic() {
        assert!(Machine::from_package(&[]).is_err());
        assert!(Machine::from_package(&[0xAA; 64]).is_err());
        assert!(Machine::from_package(b"\x7fELF....").is_err());
    }

    #[test]
    fn a_raw_image_of_garbage_halts_instead_of_crashing() {
        let mut m = Machine::from_raw(vec![0xFF; 4096], 0x0200_0120).unwrap();
        let mut out = vec![1i16; 64];
        let real = m.render(&mut out, 100_000);
        assert!(real <= 32);
        assert!(out[real * 2..].iter().all(|&s| s == 0));
    }

    #[test]
    fn input_ids_are_bounded() {
        let mut m = Machine::from_raw(vec![0; 64], 0x0200_0120).unwrap();
        m.key(255, true);
        m.key(14, true);
        m.encoder(99, 5);
        m.encoder(0, 1_000_000);
        m.pot(u16::MAX);
        assert_eq!(m.keys_down(), 1 << 14);
        m.key(14, false);
        assert_eq!(m.keys_down(), 0);
    }

    #[test]
    fn memory_views_stay_inside_their_regions() {
        let m = Machine::from_raw((0..=255u8).cycle().take(8192).collect(), 0x0200_0120).unwrap();
        let mut b = [0u8; 16];
        assert_eq!(m.read_mem(Region::Xip, 0, &mut b), 16);
        assert_eq!(&b[..4], &[0, 1, 2, 3]);
        let (base, size) = m.region(Region::Ram);
        assert_eq!(base, 0x01c0_0000);
        assert_eq!(m.read_mem(Region::Ram, size - 4, &mut b), 4);
        assert_eq!(m.read_mem(Region::Ram, size + 10, &mut b), 0);
        assert_eq!(m.read_mem(Region::Flash, usize::MAX, &mut b), 0);
        assert!(m.cpu_view(0).is_some() && m.cpu_view(1).is_none() && m.cpu_view(7).is_none());
    }

    #[test]
    fn reset_and_flash_restore() {
        let mut m = Machine::from_raw(vec![0x11; 4096], 0x0200_0120).unwrap();
        m.key(14, true);
        let mut out = vec![0i16; 64];
        m.render(&mut out, 40_000);
        m.reset();
        assert_eq!(m.keys_down(), 0);
        assert_eq!(m.cpu_view(0).unwrap().insns, 0);
        let mut flash = m.flash_image().to_vec();
        flash[5] = 0xAB;
        assert!(m.restore_flash(&flash[1..]).is_err());
        assert!(m.restore_flash(&flash).is_ok());
        assert_eq!(m.flash_image()[5], 0xAB);
    }

    /// A real firmware (the bundled UNTRUE STEP) must still run after its flash is saved, the machine reset and the flash restored.
    #[test]
    fn a_real_firmware_runs_after_a_flash_restore() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bundled-firmware/haunted-fm1/untrue-0.1.fwsc");
        let Ok(pkg) = std::fs::read(path) else { return }; // a source tree without the bundle: nothing to test
        let mut m = Machine::from_package(&pkg).unwrap();
        let mut out = vec![0i16; 1024];
        for _ in 0..30 {
            m.render(&mut out, 2_000_000);
        }
        assert!(!m.halted());
        let mut saved = m.flash_image().to_vec();
        saved[fm1_soc::FLASH_SIZE - 4096] ^= 0x55; // a sector the firmware did not write: a change to carry over
        m.restore_flash(&saved).unwrap();
        assert_eq!(m.flash_image()[fm1_soc::FLASH_SIZE - 4096], saved[fm1_soc::FLASH_SIZE - 4096]);
        let before = m.cpu_view(0).unwrap().insns;
        for _ in 0..30 {
            m.render(&mut out, 2_000_000);
        }
        assert!(!m.halted(), "the machine halted after the restore");
        assert!(m.cpu_view(0).unwrap().insns > before + 1_000_000);
        let mut px = vec![0u32; devices::SCREEN_W * devices::SCREEN_H];
        m.screen(&mut px);
        assert!(px.iter().collect::<std::collections::HashSet<_>>().len() > 8, "the screen did not draw after the restore");
    }

    #[test]
    fn the_same_image_gives_the_same_frames() {
        let run = || {
            let mut m = Machine::from_raw(vec![0x5A; 8192], 0x0200_0120).unwrap();
            let mut out = vec![0i16; 512];
            m.render(&mut out, 200_000);
            let mut px = vec![0u32; devices::SCREEN_W * devices::SCREEN_H];
            m.screen(&mut px);
            (out, px, m.halted())
        };
        assert_eq!(run(), run());
    }

    /// A deterministic xorshift, so a failure repeats (no dependency: the core has none to spare).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Whatever the bytes, loading a package is an `Ok` or an `Err`, never a panic, and a booted machine renders without one.
    fn load_and_run(bytes: &[u8]) {
        let r = std::panic::catch_unwind(|| {
            if let Ok(mut m) = Machine::from_package(bytes) {
                let mut out = vec![0i16; 256];
                m.render(&mut out, 20_000);
                m.key(20, true);
                m.render(&mut out, 20_000);
            }
        });
        assert!(r.is_ok(), "the loader or the machine panicked ({} bytes)", bytes.len());
    }

    /// Pacing is a legal slower main loop: deterministic, and the sound is the sound (level within 10%), with far fewer
    /// instructions run for the same emulated time. Measured on an M1 with three firmwares (docs/PERFORMANCE.md); this checks the bundled one.
    #[test]
    fn pacing_is_deterministic_keeps_the_sound_and_runs_fewer_instructions() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bundled-firmware/haunted-fm1/untrue-0.1.fwsc");
        let Ok(pkg) = std::fs::read(path) else { return };
        let play = |level: u8| {
            let mut m = Machine::from_package(&pkg).unwrap();
            m.set_pace(level);
            let mut out = vec![0i16; 1024];
            while m.ms() < 3000 {
                m.render(&mut out, 2_000_000);
            }
            m.key(devices::KEY_ID_BASE + 7, true);
            let (i0, t0) = (m.cpu_view(0).unwrap().insns, m.ms());
            let mut pcm: Vec<i16> = Vec::new();
            while m.ms() < t0 + 1500 {
                let n = m.render(&mut out, 2_000_000);
                pcm.extend_from_slice(&out[..n * 2]);
            }
            assert!(!m.halted());
            let insns = m.cpu_view(0).unwrap().insns - i0;
            let rms = (pcm.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / pcm.len().max(1) as f64).sqrt();
            (pcm, insns, rms)
        };
        let (a, ia, ra) = play(3);
        let (b, _, _) = play(3);
        assert!(a == b, "the same pace level must give the same sound");
        let (_, i0, r0) = play(0);
        assert!(ia * 3 < i0 * 2, "pace 3 ran {ia} instructions against {i0}");
        assert!(r0 > 100.0 && (ra - r0).abs() < r0 * 0.1, "level {ra} against {r0}");
    }

    #[test]
    fn a_damaged_package_is_an_error_or_a_boot_never_a_panic() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../bundled-firmware/haunted-fm1");
        let Ok(rd) = std::fs::read_dir(&dir) else { return };   // a checkout without the bundled folder: nothing to damage
        let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "fwsc")).collect();
        files.sort();
        let Some(path) = files.first() else { return };
        let good = std::fs::read(path).unwrap();
        // every cut length near the start (the header), then a stride across the rest
        for n in (0..2048.min(good.len())).chain((2048..good.len()).step_by(good.len() / 40 + 1)) {
            load_and_run(&good[..n]);
        }
        // a byte set to 0x00 / 0xFF / 0x7F / 0x80: every offset in the header, a stride across the rest
        let dense = 512.min(good.len());
        let offsets: Vec<usize> = (0..dense).chain((dense..good.len()).step_by(good.len() / 60 + 1)).collect();
        for &at in &offsets {
            for v in [0x00u8, 0xFF, 0x7F, 0x80] {
                let mut b = good.clone();
                b[at] = v;
                load_and_run(&b);
            }
        }
        // random damage: a few bytes, a few hundred times
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..200 {
            let mut b = good.clone();
            for _ in 0..1 + rng.next() % 8 {
                let at = (rng.next() % b.len() as u64) as usize;
                b[at] = rng.next() as u8;
            }
            load_and_run(&b);
        }
    }

    #[test]
    fn raw_garbage_and_wrong_sized_saves_are_refused_or_run_safely() {
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        for len in [1usize, 3, 4, 64, 4095, 4096, 65536] {
            let img: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let r = std::panic::catch_unwind(|| {
                let mut m = Machine::from_raw(img, 0x0200_0120).unwrap();
                let mut out = vec![0i16; 128];
                m.render(&mut out, 20_000);
                for bad in [0usize, 1, 4096, fm1_soc::FLASH_SIZE - 1, fm1_soc::FLASH_SIZE + 1] {
                    assert!(m.restore_flash(&vec![0xA5; bad]).is_err());
                }
            });
            assert!(r.is_ok());
        }
        for entry in [0u32, 1, 0x0200_0000, 0xFFFF_FFFF, 0x0200_0121] {
            let r = std::panic::catch_unwind(|| {
                let mut m = Machine::from_raw(vec![0xFF; 4096], entry).unwrap();
                let mut out = vec![0i16; 128];
                m.render(&mut out, 20_000);
            });
            assert!(r.is_ok(), "entry {entry:#x}");
        }
    }
}
