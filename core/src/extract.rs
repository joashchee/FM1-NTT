//! Firmware extractor: what is inside a `.fwsc`, as data (docs/EXTRACTOR.md).
//!
//! It unpacks a package with the vendored reader, lists its directory, and scans the unpacked
//! images for **DX7 32-voice banks** (the 128-byte packed voice the stock firmware keeps in its
//! USR area) and for printable strings. It reads bytes and never runs them. A firmware is
//! untrusted input (rule 5): the size is bounded first, every access is checked, and the number
//! of results is capped.

use crate::firmware::check_size;
use crate::{Error, Result};
use fm1_soc::fwsc;

/// One packed DX7 voice is 128 bytes.
pub const VOICE_LEN: usize = 128;
/// The most banks and strings one report keeps.
pub const MAX_BANKS: usize = 64;
pub const MAX_STRINGS: usize = 4096;

/// A directory entry of the app area (app.bin, the reserved areas VM, PRCT, BTIF, USR).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

/// A run of consecutive valid packed voices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bank {
    /// Which image: `"flash"` (the raw flash as written) or `"app"` (the app area, unscrambled).
    pub image: &'static str,
    /// Byte offset in that image.
    pub offset: usize,
    /// The voices' names, 10 characters each, trailing spaces trimmed.
    pub names: Vec<String>,
    /// The algorithm of each voice, 0 to 31.
    pub algorithms: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub identity: String,
    pub flash_len: usize,
    pub app_len: usize,
    pub entries: Vec<Entry>,
    pub banks: Vec<Bank>,
    /// Printable runs of at least [`MIN_STRING`] characters in the app area, with their offsets.
    pub strings: Vec<(usize, String)>,
}

pub const MIN_STRING: usize = 6;

/// A packed voice's 102 operator bytes, pitch envelope, algorithm, name: are all fields in range?
/// (DX7 sysex format: rates, levels, break point, depths, output level and fine tune 0 to 99,
/// coarse frequency 0 to 31, algorithm 0 to 31, name printable ASCII.)
pub fn valid_voice(v: &[u8]) -> bool {
    if v.len() < VOICE_LEN {
        return false;
    }
    // the name first: it rejects almost everything at once
    if !v[118..128].iter().all(|&c| (0x20..0x7f).contains(&c)) {
        return false;
    }
    for op in 0..6 {
        let o = &v[op * 17..op * 17 + 17];
        // R1-4, L1-4, break point, left depth, right depth
        if o[..11].iter().any(|&b| b > 99) {
            return false;
        }
        // 11: curves; 12: detune and rate scale; 13: velocity sens and amp mod: bit-packed, any value
        if o[14] > 99 || o[15] >> 1 > 31 || o[16] > 99 {
            return false;
        }
    }
    // pitch envelope rates and levels, then the algorithm
    v[102..110].iter().all(|&b| b <= 99) && v[110] <= 31
}

/// One unpacked DX7 voice is 155 bytes (the sysex single-voice layout, msfa's `UnpackPatch` output): six operators of 21 bytes (operator 6 first), the pitch
/// envelope, algorithm, feedback, oscillator sync, the LFO, transpose and a 10-character name.
pub const UNPACKED_LEN: usize = 155;

/// An unpacked voice's fields in range (rates, levels, depths, output level, fine 0 to 99; curves 0 to 3; rate scale 0 to 7; amp mod 0 to 3; key velocity 0 to 7;
/// mode 0 to 1; coarse 0 to 31; detune 0 to 14; algorithm 0 to 31; feedback 0 to 7; LFO wave 0 to 5, pitch sensitivity 0 to 7, transpose 0 to 48) and a printable name.
pub fn valid_unpacked(v: &[u8]) -> bool {
    if v.len() < UNPACKED_LEN || !v[145..155].iter().all(|&c| (0x20..0x7f).contains(&c)) {
        return false;
    }
    for op in 0..6 {
        let o = &v[op * 21..op * 21 + 21];
        let r = |i: usize, max: u8| o[i] <= max;
        if !(o[..11].iter().all(|&b| b <= 99) && r(11, 3) && r(12, 3) && r(13, 7) && r(14, 3) && r(15, 7) && r(16, 99) && r(17, 1) && r(18, 31) && r(19, 99) && r(20, 14)) {
            return false;
        }
    }
    v[126..134].iter().all(|&b| b <= 99) && v[134] <= 31 && v[135] <= 7 && v[136] <= 1 && v[137..141].iter().all(|&b| b <= 99) && v[141] <= 1 && v[142] <= 5 && v[143] <= 7 && v[144] <= 48
}

/// Every run of at least `min_run` consecutive unpacked voices in `image` (any offset, each record `UNPACKED_LEN` bytes, or `stride` if the records are padded): the
/// offset, the stride and the names.
pub fn find_unpacked(image: &[u8], min_run: usize, stride: usize) -> Vec<(usize, usize, Vec<String>)> {
    let stride = stride.max(UNPACKED_LEN);
    let mut out = Vec::new();
    let mut at = 0;
    while at + UNPACKED_LEN <= image.len() && out.len() < MAX_BANKS {
        let mut n = 0;
        while at + n * stride + UNPACKED_LEN <= image.len() && valid_unpacked(&image[at + n * stride..]) {
            n += 1;
        }
        if n >= min_run.max(1) {
            let names = (0..n).map(|i| String::from_utf8_lossy(&image[at + i * stride + 145..at + i * stride + 155]).trim_end().to_string()).collect();
            out.push((at, stride, names));
            at += n * stride;
        } else {
            at += 1;
        }
    }
    out
}

/// Every run of at least `min_run` consecutive valid voices in `image`, at any byte offset.
/// A bank is 32 voices in a sysex, but a firmware may keep fewer: `min_run` says how many are
/// believed. At most [`MAX_BANKS`] runs are returned.
pub fn find_banks(image: &[u8], label: &'static str, min_run: usize) -> Vec<Bank> {
    let mut banks = Vec::new();
    let mut at = 0;
    while at + VOICE_LEN <= image.len() && banks.len() < MAX_BANKS {
        let mut n = 0;
        while at + (n + 1) * VOICE_LEN <= image.len() && valid_voice(&image[at + n * VOICE_LEN..]) {
            n += 1;
        }
        if n >= min_run.max(1) {
            let run = || (0..n).map(|i| &image[at + i * VOICE_LEN..at + (i + 1) * VOICE_LEN]);
            banks.push(Bank {
                image: label,
                offset: at,
                names: run().map(|v| String::from_utf8_lossy(&v[118..128]).trim_end().to_string()).collect(),
                algorithms: run().map(|v| v[110]).collect(),
            });
            at += n * VOICE_LEN;
        } else {
            at += 1;
        }
    }
    banks
}

/// Printable ASCII runs of at least `min` characters, with their offsets, at most [`MAX_STRINGS`].
pub fn find_strings(image: &[u8], min: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &b) in image.iter().chain(std::iter::once(&0)).enumerate() {
        let printable = (0x20..0x7f).contains(&b);
        match (printable, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                let run = &image[s..i];
                // text, not code that happens to be printable: mostly letters, digits and spaces
                let texty = run.iter().filter(|b| b.is_ascii_alphanumeric() || **b == b' ' || **b == b'_').count();
                if run.len() >= min && texty * 10 >= run.len() * 8 && out.len() < MAX_STRINGS {
                    out.push((s, String::from_utf8_lossy(run).into_owned()));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// What is in a firmware package. A refusal says why; nothing here panics on bad bytes.
pub fn report(bytes: &[u8], min_run: usize) -> Result<Report> {
    check_size(bytes)?;
    let u = fwsc::unpack(bytes).map_err(Error::BadImage)?;
    let mut banks = find_banks(&u.raw, "flash", min_run);
    let room = MAX_BANKS.saturating_sub(banks.len());
    banks.extend(find_banks(&u.xip, "app", min_run).into_iter().take(room));
    Ok(Report {
        identity: u.identity.clone(),
        flash_len: u.raw.len(),
        app_len: u.xip.len(),
        entries: u.dir.iter().map(|h| Entry { name: h.name.clone(), offset: h.offset, size: h.size }).collect(),
        banks,
        strings: find_strings(&u.xip[..u.size.min(u.xip.len())], MIN_STRING),
    })
}

/// The app image of a package, unscrambled (what the CPU sees through XIP), for a dump.
pub fn app_image(bytes: &[u8]) -> Result<Vec<u8>> {
    check_size(bytes)?;
    Ok(fwsc::unpack(bytes).map_err(Error::BadImage)?.xip)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid packed voice named `name` with algorithm `alg`.
    fn voice(name: &str, alg: u8) -> Vec<u8> {
        let mut v = vec![0u8; VOICE_LEN];
        for op in 0..6 {
            v[op * 17 + 14] = 80; // output level
            v[op * 17 + 15] = 2; // coarse 1
        }
        v[110] = alg;
        let mut n = [b' '; 10];
        n[..name.len()].copy_from_slice(name.as_bytes());
        v[118..128].copy_from_slice(&n);
        v
    }

    #[test]
    fn finds_a_bank_at_an_odd_offset() {
        let mut img = vec![0xffu8; 301];
        for i in 0..32u8 {
            img.extend(voice(&format!("VOICE {i}"), i % 32));
        }
        img.extend(vec![0xff; 50]);
        let banks = find_banks(&img, "app", 8);
        assert_eq!(banks.len(), 1);
        assert_eq!(banks[0].offset, 301);
        assert_eq!(banks[0].names.len(), 32);
        assert_eq!(banks[0].names[5], "VOICE 5");
        assert_eq!(banks[0].algorithms[31], 31);
    }

    #[test]
    fn rejects_out_of_range_fields() {
        let mut v = voice("BAD", 3);
        assert!(valid_voice(&v));
        v[110] = 32; // algorithm
        assert!(!valid_voice(&v));
        let mut v = voice("BAD", 3);
        v[4] = 100; // a level over 99
        assert!(!valid_voice(&v));
        let mut v = voice("BAD", 3);
        v[118] = 0x07; // unprintable name
        assert!(!valid_voice(&v));
        assert!(!valid_voice(&[0; 10]));
    }

    #[test]
    fn finds_unpacked_voices() {
        let mut v = vec![0u8; UNPACKED_LEN];
        for op in 0..6 {
            v[op * 21 + 16] = 90;
            v[op * 21 + 18] = 1;
            v[op * 21 + 20] = 7;
        }
        v[144] = 24;
        v[145..155].copy_from_slice(b"TEST VOICE");
        assert!(valid_unpacked(&v));
        let mut img = vec![0xffu8; 100];
        for i in 0..4 {
            let mut w = v.clone();
            w[154] = b'0' + i;
            img.extend(w);
        }
        img.extend(vec![0xff; 40]);
        let found = find_unpacked(&img, 3, 0);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].0, found[0].2.len()), (100, 4));
        assert_eq!(found[0].2[3], "TEST VOIC3");
        v[134] = 32;
        assert!(!valid_unpacked(&v), "an algorithm of 33");
        assert!(find_unpacked(&[0xab; 4096], 1, 0).is_empty());
    }

    #[test]
    fn short_runs_and_garbage_are_ignored() {
        let mut img = Vec::new();
        for i in 0..3u8 {
            img.extend(voice("X", i));
        }
        assert!(find_banks(&img, "app", 8).is_empty());
        assert!(find_banks(&[], "app", 1).is_empty());
        assert!(find_banks(&[0xab; 4096], "app", 1).is_empty());
    }

    #[test]
    fn strings_are_found_and_capped() {
        let s = find_strings(b"\0\0hello world\0ab\0second one", 6);
        assert_eq!(s, vec![(2, "hello world".to_string()), (17, "second one".to_string())]);
        let many: Vec<u8> = (0..10_000).flat_map(|_| b"abcdefg\0".to_vec()).collect();
        assert_eq!(find_strings(&many, 6).len(), MAX_STRINGS);
    }

    #[test]
    fn refuses_what_is_not_a_package() {
        assert!(report(&[], 8).is_err());
        assert!(report(b"not a firmware package", 8).is_err());
        assert!(report(&vec![0x55; 4096], 8).is_err());
    }
}
