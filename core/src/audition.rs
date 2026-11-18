//! A scripted audition and a headless verdict on a firmware, for the catalog's server and for anyone who wants the same answer on every machine (ROADMAP 6b).
//! The firmware is untrusted (rule 5): the work is bounded by the script's length (at most 60 s of emulated time, 4096 messages) and by an instruction budget
//! per slice, never by a wall clock, so **the same firmware and script give the same answer, to the byte, on any computer**: the hash, the screen (as a PNG),
//! the audio's peak, level and share of non-silent samples. It reads the firmware as data and runs it; it links none of it.

use crate::devices::{SCREEN_H, SCREEN_W};
use crate::machine::Machine;
use crate::{Error, Result};

const RATE: usize = 44_100;
/// The longest script, in milliseconds of emulated time, and the most messages in one.
pub const MAX_MS: u32 = 60_000;
pub const MAX_EVENTS: usize = 4096;
/// A slice is at most this many instructions: the machine is run in slices of 4096 frames (about 93 ms of emulated time).
const SLICE_INSNS: u32 = 8_000_000;
const SLICE_FRAMES: usize = 4096;
/// Every firmware gets this long to boot before the script starts.
pub const BOOT_MS: u32 = 3000;

/// A MIDI message on the jack at a time after the boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub at_ms: u32,
    pub msg: [u8; 3],
    /// 1 for a program or channel-pressure message, else 3 (the length of `msg` that is sent)
    pub len: u8,
}

/// The same four notes for every firmware: C3, E3, G3, C4 at velocity 100 on channel 1, each held 800 ms with 400 ms of release.
pub fn standard_script() -> Vec<Event> {
    let mut v = Vec::new();
    for (k, note) in [48u8, 52, 55, 60].into_iter().enumerate() {
        let on = k as u32 * 1200;
        v.push(Event { at_ms: on, msg: [0x90, note, 100], len: 3 });
        v.push(Event { at_ms: on + 800, msg: [0x80, note, 0], len: 3 });
    }
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// it booted, drew a screen of its own and made sound
    Pass,
    /// it booted and drew but never made a sound during the script (a firmware that needs panel input first, or one that ignores the jack)
    Silent,
    /// it ran but drew (almost) nothing
    Blank,
    /// the emulated machine halted: a crash, or a firmware this core cannot run
    Halted,
}

#[derive(Clone, Debug)]
pub struct Audition {
    /// the firmware's SHA-256, lowercase hex
    pub sha256: String,
    pub size: usize,
    pub verdict: Verdict,
    /// distinct colours on the screen at the end
    pub colours: usize,
    /// the largest absolute sample (0 to 32768) and the level over the whole script in dBFS (-120 when silent)
    pub peak: i32,
    pub rms_db: f64,
    /// the share of 10 ms windows with any sound over a threshold, 0 to 1
    pub active: f64,
    pub emulated_ms: u32,
    /// the final screen as a PNG (240 x 240, written without compression)
    pub screen_png: Vec<u8>,
}

impl Audition {
    pub fn summary(&self) -> String {
        format!(
            "{:?}: {} colours, peak {}, {:.1} dBFS, {:.0}% of the time sounding, {} ms emulated, sha256 {}",
            self.verdict,
            self.colours,
            self.peak,
            self.rms_db,
            self.active * 100.0,
            self.emulated_ms,
            self.sha256
        )
    }
}

fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// Runs `fw` (a `.fwsc` package) through `script` and returns what it did. An `Error` only for a file that is not a package or a script out of bounds; a firmware
/// that crashes is a `Halted` verdict, not an error.
pub fn audition(fw: &[u8], script: &[Event]) -> Result<Audition> {
    if script.len() > MAX_EVENTS {
        return Err(Error::BadImage(format!("a script of {} messages (the most is {MAX_EVENTS})", script.len())));
    }
    if script.iter().any(|e| e.at_ms > MAX_MS || e.len == 0 || e.len > 3) {
        return Err(Error::BadImage("a script message is out of bounds".into()));
    }
    let mut events = script.to_vec();
    events.sort_by_key(|e| e.at_ms); // stable: messages at the same time keep their order
    let sha = hex(&crate::package::sha256(fw));
    let mut m = Machine::from_package(fw)?;
    m.set_jit(true);
    let end_ms = events.last().map_or(0, |e| e.at_ms) + 1000;
    let total_frames = (BOOT_MS + end_ms) as usize * RATE / 1000;
    let mut buf = vec![0i16; 2 * SLICE_FRAMES];
    let mut done = 0usize;
    let mut next = 0usize;
    let (mut peak, mut sum_sq, mut count) = (0i32, 0f64, 0u64);
    let (mut windows, mut loud, mut win_peak, mut win_n) = (0u64, 0u64, 0i32, 0usize);
    let mut guard = 0u32;
    while done < total_frames && !m.halted() && guard < 1_000_000 {
        guard += 1;
        // the messages due by the end of this slice go in at the slice's start: the slice is under 93 ms, the script's own granularity
        let now_ms = (done * 1000 / RATE) as u32;
        let slice = SLICE_FRAMES.min(total_frames - done);
        let slice_end_ms = ((done + slice) * 1000 / RATE) as u32;
        if now_ms >= BOOT_MS {
            while next < events.len() && events[next].at_ms + BOOT_MS <= slice_end_ms {
                let e = events[next];
                for &b in &e.msg[..usize::from(e.len)] {
                    m.midi_in(b);
                }
                next += 1;
            }
        }
        let got = m.render(&mut buf[..2 * slice], SLICE_INSNS);
        for f in 0..got {
            let s = ((i32::from(buf[2 * f]) + i32::from(buf[2 * f + 1])) / 2).abs();
            if now_ms >= BOOT_MS {
                peak = peak.max(s);
                sum_sq += f64::from(s) * f64::from(s);
                count += 1;
            }
            win_peak = win_peak.max(s);
            win_n += 1;
            if win_n == RATE / 100 {
                if now_ms >= BOOT_MS {
                    windows += 1;
                    loud += u64::from(win_peak > 64);
                }
                win_peak = 0;
                win_n = 0;
            }
        }
        done += got.max(1).min(slice);
    }
    let mut px = vec![0u32; SCREEN_W * SCREEN_H];
    m.screen(&mut px);
    let colours = px.iter().collect::<std::collections::HashSet<_>>().len();
    let rms = if count == 0 { 0.0 } else { (sum_sq / count as f64).sqrt() };
    let rms_db = if rms < 1.0 { -120.0 } else { 20.0 * (rms / 32768.0).log10() };
    let active = if windows == 0 { 0.0 } else { loud as f64 / windows as f64 };
    let verdict = if m.halted() {
        Verdict::Halted
    } else if colours < 4 {
        Verdict::Blank
    } else if peak < 64 {
        Verdict::Silent
    } else {
        Verdict::Pass
    };
    Ok(Audition { sha256: sha, size: fw.len(), verdict, colours, peak, rms_db, active, emulated_ms: (done * 1000 / RATE) as u32, screen_png: png(&px) })
}

/// The standard script on `fw`.
pub fn audition_standard(fw: &[u8]) -> Result<Audition> {
    audition(fw, &standard_script())
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------
// a PNG with no compression (stored deflate blocks), 240 x 240, 8-bit RGB

fn crc32(data: &[u8], mut c: u32) -> u32 {
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    c
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(kind);
    out.extend(data);
    let c = crc32(data, crc32(kind, 0xFFFF_FFFF)) ^ 0xFFFF_FFFF;
    out.extend(c.to_be_bytes());
}

/// `px` as 0x00RRGGBB, `SCREEN_W` x `SCREEN_H`.
pub fn png(px: &[u32]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((SCREEN_W * 3 + 1) * SCREEN_H);
    for y in 0..SCREEN_H {
        raw.push(0); // filter: none
        for x in 0..SCREEN_W {
            let p = px.get(y * SCREEN_W + x).copied().unwrap_or(0);
            raw.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend((c.len() as u16).to_le_bytes());
        z.extend((!(c.len() as u16)).to_le_bytes());
        z.extend(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend((SCREEN_W as u32).to_be_bytes());
    ihdr.extend((SCREEN_H as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads our own PNG back: checks the signature, every chunk's CRC, the stored blocks and the Adler sum, and returns the pixels.
    fn unpng(p: &[u8]) -> Vec<u32> {
        assert_eq!(&p[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut at = 8;
        let mut idat = Vec::new();
        let mut kinds = Vec::new();
        while at < p.len() {
            let n = u32::from_be_bytes(p[at..at + 4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = p[at + 4..at + 8].try_into().unwrap();
            let data = &p[at + 8..at + 8 + n];
            let crc = u32::from_be_bytes(p[at + 8 + n..at + 12 + n].try_into().unwrap());
            assert_eq!(crc, crc32(data, crc32(&kind, 0xFFFF_FFFF)) ^ 0xFFFF_FFFF, "crc of {:?}", kind);
            if &kind == b"IHDR" {
                assert_eq!(u32::from_be_bytes(data[..4].try_into().unwrap()) as usize, SCREEN_W);
                assert_eq!(u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize, SCREEN_H);
            }
            if &kind == b"IDAT" {
                idat.extend(data);
            }
            kinds.push(kind);
            at += 12 + n;
        }
        assert_eq!(kinds, [*b"IHDR", *b"IDAT", *b"IEND"]);
        assert_eq!(&idat[..2], &[0x78, 0x01]);
        let mut raw: Vec<u8> = Vec::new();
        let mut i = 2;
        loop {
            let last = idat[i] & 1 == 1;
            let len = u16::from_le_bytes([idat[i + 1], idat[i + 2]]) as usize;
            assert_eq!(u16::from_le_bytes([idat[i + 3], idat[i + 4]]), !(len as u16));
            raw.extend(&idat[i + 5..i + 5 + len]);
            i += 5 + len;
            if last {
                break;
            }
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &v in &raw {
            a = (a + u32::from(v)) % 65_521;
            b = (b + a) % 65_521;
        }
        assert_eq!(&idat[i..], &((b << 16) | a).to_be_bytes());
        let mut px = Vec::new();
        for row in raw.chunks(SCREEN_W * 3 + 1) {
            assert_eq!(row[0], 0);
            for c in row[1..].chunks(3) {
                px.push(u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]));
            }
        }
        px
    }

    #[test]
    fn the_png_reads_back_exactly() {
        let px: Vec<u32> = (0..SCREEN_W * SCREEN_H).map(|i| ((i * 2654435761) >> 8) as u32 & 0xFFFFFF).collect();
        assert_eq!(unpng(&png(&px)), px);
        assert_eq!(unpng(&png(&[])).len(), SCREEN_W * SCREEN_H, "a short buffer is black, not a panic");
    }

    #[test]
    fn crc_and_known_value() {
        assert_eq!(crc32(b"123456789", 0xFFFF_FFFF) ^ 0xFFFF_FFFF, 0xCBF4_3926);
    }

    #[test]
    fn scripts_are_bounded_and_the_standard_one_is_four_notes() {
        let s = standard_script();
        assert_eq!(s.len(), 8);
        assert!(s.iter().filter(|e| e.msg[0] == 0x90).count() == 4);
        let junk = vec![0u8; 100];
        assert!(audition(&junk, &[Event { at_ms: MAX_MS + 1, msg: [0x90, 60, 1], len: 3 }]).is_err());
        assert!(audition(&junk, &vec![s[0]; MAX_EVENTS + 1]).is_err());
        assert!(audition(&junk, &[Event { at_ms: 0, msg: [0; 3], len: 0 }]).is_err());
        assert!(audition_standard(&junk).is_err(), "not a package is an error, not a verdict");
    }

    /// The bundled firmwares (MIT; permissive tier): each is auditioned, the answer is the same twice to the byte, and none halts. Their sounds are generative, so the
    /// verdict is not pinned beyond "ran and drew".
    #[test]
    fn a_bundled_firmware_is_auditioned_deterministically() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../bundled-firmware/haunted-fm1");
        let Some(path) = std::fs::read_dir(&dir).ok().and_then(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "fwsc")).min()) else {
            eprintln!("skipped: no bundled firmware found");
            return;
        };
        let fw = std::fs::read(&path).unwrap();
        let a = audition_standard(&fw).unwrap();
        let b = audition_standard(&fw).unwrap();
        assert_eq!((a.verdict, a.colours, a.peak, a.screen_png.clone(), a.sha256.clone()), (b.verdict, b.colours, b.peak, b.screen_png.clone(), b.sha256.clone()), "not deterministic");
        assert_eq!(a.rms_db.to_bits(), b.rms_db.to_bits());
        assert_ne!(a.verdict, Verdict::Halted, "{}", a.summary());
        assert!(a.colours > 4, "{}", a.summary());
        assert_eq!(a.sha256.len(), 64);
        assert!((8390..=8400).contains(&a.emulated_ms), "the script is 3 s of boot, 4.4 s of notes and 1 s of tail: {}", a.summary());
    }
}
