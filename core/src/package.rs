//! The instrument package (`.fm1i`): data, never code (docs/PORTING.md). A text file of `key = value` lines.
//!
//! Bounded and strict: at most 1 MiB, 4096 lines, 256 voices, 64 samples, 64 effects, 128 map entries; an unknown key, a bad number or a missing licence is an `Error`.
//! No dependency: the SHA-256 and the parser are here.

use crate::{Error, Result};

pub const MAX_BYTES: usize = 1 << 20;
pub const MAX_LINES: usize = 4096;
pub const MAX_VOICES: usize = 256;
pub const MAX_SAMPLES: usize = 64;
pub const MAX_FX: usize = 64;
pub const MAX_MAP: usize = 128;
pub const MAX_PARAMS: usize = 1024;
pub const MAX_ARP: usize = 32;
pub const MAX_STEPS: usize = 256;

fn bad<T>(why: impl Into<String>) -> Result<T> {
    Err(Error::BadImage(format!("package: {}", why.into())))
}

// ---------------------------------------------------------------------------------------------------------------
// SHA-256

pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() & 1 == 1 || !s.is_ascii() {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// licences

const PERMISSIVE: [&str; 10] = ["MIT", "BSD-2-Clause", "BSD-3-Clause", "Apache-2.0", "ISC", "Unlicense", "CC0-1.0", "Zlib", "MPL-2.0", "OFL-1.1"];
const COPYLEFT: [&str; 8] = ["GPL-2.0-only", "GPL-2.0-or-later", "GPL-3.0-only", "GPL-3.0-or-later", "LGPL-2.1-only", "LGPL-2.1-or-later", "LGPL-3.0-only", "LGPL-3.0-or-later"];

/// Where a package may be shipped (`CLAUDE.md` rule 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// `bundled-firmware/`, inside the MIT app.
    Permissive,
    /// The FM1-NTT Firmware Pack repository only: never in the MIT app's package or a Mac App Store build.
    Copyleft,
}

/// The tier of an AND-only licence expression from the allowlist (`MIT AND Apache-2.0`). Never "OR" or "WITH", never AGPL, non-commercial, closed or
/// unknown: those are an `Error`. One copyleft term makes the whole expression copyleft.
pub fn tier(expr: &str) -> Result<Tier> {
    let mut copyleft = false;
    let mut any = false;
    for term in expr.split(" AND ") {
        let t = term.trim();
        if PERMISSIVE.contains(&t) {
        } else if COPYLEFT.contains(&t) {
            copyleft = true;
        } else {
            return bad(format!("licence term {t:?} is not on the allowlist (AND-only; no OR, WITH, AGPL, closed or unknown)"));
        }
        any = true;
    }
    if !any {
        return bad("no licence");
    }
    Ok(if copyleft { Tier::Copyleft } else { Tier::Permissive })
}

// ---------------------------------------------------------------------------------------------------------------
// the package

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleRef {
    pub note: u8,
    pub file: String,
    pub chokes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Engine(u16),
    Fx(usize, u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapEntry {
    pub cc: u8,
    pub target: Target,
    pub min: i32,
    pub max: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FxEntry {
    pub name: String,
    pub params: Vec<(u16, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub engine: String,
    pub licence: String,
    pub source: String,
    pub source_sha256: String,
    /// what verification measured, or empty (unverified)
    pub verify: String,
    pub program: usize,
    pub root: u8,
    pub voices: Vec<[u8; 128]>,
    pub params: Vec<(u16, i32)>,
    /// the arpeggiator's settings (`arp N = V`, ids as in `arp.rs`); empty means it is off
    pub arp: Vec<(u16, i32)>,
    /// the step sequencer (`seq.rs`): its settings (`seq N = V`), the steps that are not rests (`step P S = NOTE VELOCITY`) and each pattern's voice (`seqvoice P = V`)
    pub seq: Vec<(u16, i32)>,
    pub steps: Vec<(u8, u8, u8, u8)>,
    pub seq_voices: Vec<(u8, i32)>,
    pub samples: Vec<SampleRef>,
    pub fx: Vec<FxEntry>,
    pub map: Vec<MapEntry>,
}

impl Package {
    pub fn new(name: &str, engine: &str, licence: &str) -> Package {
        Package {
            name: name.into(),
            engine: engine.into(),
            licence: licence.into(),
            source: String::new(),
            source_sha256: String::new(),
            verify: String::new(),
            program: 0,
            root: 60,
            voices: Vec::new(),
            params: Vec::new(),
            arp: Vec::new(),
            seq: Vec::new(),
            steps: Vec::new(),
            seq_voices: Vec::new(),
            samples: Vec::new(),
            fx: Vec::new(),
            map: Vec::new(),
        }
    }

    pub fn tier(&self) -> Result<Tier> {
        tier(&self.licence)
    }

    pub fn verified(&self) -> bool {
        !self.verify.is_empty()
    }

    /// The package as text. Values are one line each (a line break in a value becomes a space).
    pub fn to_text(&self) -> String {
        let one = |s: &str| s.replace(['\n', '\r'], " ");
        let mut t = String::from("fm1i = 1\n");
        t += &format!("name = {}\nengine = {}\nlicence = {}\n", one(&self.name), one(&self.engine), one(&self.licence));
        if !self.source.is_empty() {
            t += &format!("source = {}\n", one(&self.source));
        }
        if !self.source_sha256.is_empty() {
            t += &format!("source-sha256 = {}\n", one(&self.source_sha256));
        }
        if !self.verify.is_empty() {
            t += &format!("verify = {}\n", one(&self.verify));
        }
        t += &format!("program = {}\nroot = {}\n", self.program, self.root);
        for v in &self.voices {
            t += &format!("voice = {}\n", hex(v));
        }
        for (id, v) in &self.params {
            t += &format!("param {id} = {v}\n");
        }
        for (id, v) in &self.arp {
            t += &format!("arp {id} = {v}\n");
        }
        for (id, v) in &self.seq {
            t += &format!("seq {id} = {v}\n");
        }
        for &(p, st, note, vel) in &self.steps {
            t += &format!("step {p} {st} = {note} {vel}\n");
        }
        for (p, v) in &self.seq_voices {
            t += &format!("seqvoice {p} = {v}\n");
        }
        for s in &self.samples {
            t += &format!("sample {} = {}", s.note, one(&s.file));
            if !s.chokes.is_empty() {
                t += " choke";
                for c in &s.chokes {
                    t += &format!(" {c}");
                }
            }
            t.push('\n');
        }
        for f in &self.fx {
            t += &format!("fx = {}\n", f.name);
            for (id, v) in &f.params {
                t += &format!("fxparam {id} = {v}\n");
            }
        }
        for m in &self.map {
            let target = match m.target {
                Target::Engine(id) => format!("e{id}"),
                Target::Fx(i, id) => format!("f{i}.{id}"),
            };
            t += &format!("map cc {} = {} {} {}\n", m.cc, target, m.min, m.max);
        }
        t
    }

    pub fn parse(text: &str) -> Result<Package> {
        if text.len() > MAX_BYTES {
            return bad("larger than 1 MiB");
        }
        let mut p = Package::new("", "", "");
        let mut seen_header = false;
        let mut n = 0;
        for (ln, raw) in text.lines().enumerate() {
            n += 1;
            if n > MAX_LINES {
                return bad("more than 4096 lines");
            }
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let at = ln + 1;
            let Some((k, v)) = line.split_once('=') else { return bad(format!("line {at}: no '='")) };
            let (k, v) = (k.trim(), v.trim());
            let mut kw = k.split_whitespace();
            let key = kw.next().unwrap_or("");
            let num = |s: &str| -> Result<i32> { s.parse::<i32>().or_else(|_| bad(format!("line {at}: {s:?} is not a number"))) };
            if !seen_header {
                if key != "fm1i" || v != "1" {
                    return bad("the first line must be `fm1i = 1`");
                }
                seen_header = true;
                continue;
            }
            match key {
                "name" => p.name = v.into(),
                "engine" => p.engine = v.into(),
                "licence" => p.licence = v.into(),
                "source" => p.source = v.into(),
                "source-sha256" => p.source_sha256 = v.into(),
                "verify" => p.verify = v.into(),
                "program" => p.program = num(v)?.clamp(0, MAX_VOICES as i32) as usize,
                "root" => p.root = num(v)?.clamp(0, 127) as u8,
                "voice" => {
                    if p.voices.len() >= MAX_VOICES {
                        return bad("more than 256 voices");
                    }
                    let b = unhex(v).filter(|b| b.len() == 128);
                    let Some(b) = b else { return bad(format!("line {at}: a voice is 256 hex digits")) };
                    let mut a = [0u8; 128];
                    a.copy_from_slice(&b);
                    p.voices.push(a);
                }
                "param" => {
                    if p.params.len() >= MAX_PARAMS {
                        return bad("too many params");
                    }
                    let id = kw.next().map(|s| s.parse::<u16>()).and_then(|r| r.ok());
                    let Some(id) = id else { return bad(format!("line {at}: param needs an id")) };
                    p.params.push((id, num(v)?));
                }
                "arp" => {
                    if p.arp.len() >= MAX_ARP {
                        return bad("too many arp settings");
                    }
                    let id = kw.next().and_then(|s| s.parse::<u16>().ok());
                    let Some(id) = id else { return bad(format!("line {at}: arp needs an id")) };
                    p.arp.push((id, num(v)?));
                }
                "seq" => {
                    if p.seq.len() >= MAX_ARP {
                        return bad("too many sequencer settings");
                    }
                    let id = kw.next().and_then(|s| s.parse::<u16>().ok());
                    let Some(id) = id else { return bad(format!("line {at}: seq needs an id")) };
                    p.seq.push((id, num(v)?));
                }
                "step" => {
                    if p.steps.len() >= MAX_STEPS {
                        return bad("more than 256 steps");
                    }
                    let (pat, st) = (kw.next().and_then(|s| s.parse::<u8>().ok()), kw.next().and_then(|s| s.parse::<u8>().ok()));
                    let (Some(pat), Some(st)) = (pat, st) else { return bad(format!("line {at}: step needs a pattern and a step")) };
                    let mut it = v.split_whitespace();
                    let (note, vel) = (it.next().and_then(|s| s.parse::<u8>().ok()), it.next().and_then(|s| s.parse::<u8>().ok()));
                    let (Some(note), Some(vel)) = (note, vel) else { return bad(format!("line {at}: a step is `NOTE VELOCITY`")) };
                    p.steps.push((pat, st, note, vel));
                }
                "seqvoice" => {
                    if p.seq_voices.len() >= 16 {
                        return bad("more than 16 pattern voices");
                    }
                    let pat = kw.next().and_then(|s| s.parse::<u8>().ok());
                    let Some(pat) = pat else { return bad(format!("line {at}: seqvoice needs a pattern")) };
                    p.seq_voices.push((pat, num(v)?));
                }
                "sample" => {
                    if p.samples.len() >= MAX_SAMPLES {
                        return bad("more than 64 samples");
                    }
                    let note = kw.next().and_then(|s| s.parse::<u8>().ok()).filter(|&n| n < 128);
                    let Some(note) = note else { return bad(format!("line {at}: sample needs a note 0 to 127")) };
                    let mut it = v.split_whitespace();
                    let file = it.next().unwrap_or("").to_string();
                    if file.is_empty() || file.contains("..") || file.starts_with('/') || file.contains('\\') {
                        return bad(format!("line {at}: a sample file is a plain relative path"));
                    }
                    let mut chokes = Vec::new();
                    if let Some(w) = it.next() {
                        if w != "choke" {
                            return bad(format!("line {at}: expected `choke`"));
                        }
                        for c in it {
                            match c.parse::<u8>() {
                                Ok(c) if c < 128 && chokes.len() < 16 => chokes.push(c),
                                _ => return bad(format!("line {at}: bad choke list")),
                            }
                        }
                    }
                    p.samples.push(SampleRef { note, file, chokes });
                }
                "fx" => {
                    if p.fx.len() >= MAX_FX {
                        return bad("more than 64 effects");
                    }
                    p.fx.push(FxEntry { name: v.into(), params: Vec::new() });
                }
                "fxparam" => {
                    let id = kw.next().and_then(|s| s.parse::<u16>().ok());
                    let Some(id) = id else { return bad(format!("line {at}: fxparam needs an id")) };
                    let val = num(v)?;
                    match p.fx.last_mut() {
                        Some(f) if f.params.len() < 64 => f.params.push((id, val)),
                        _ => return bad(format!("line {at}: fxparam before any fx")),
                    }
                }
                "map" => {
                    if p.map.len() >= MAX_MAP {
                        return bad("more than 128 map entries");
                    }
                    if kw.next() != Some("cc") {
                        return bad(format!("line {at}: only `map cc N` is known"));
                    }
                    let cc = kw.next().and_then(|s| s.parse::<u8>().ok()).filter(|&c| c < 128);
                    let Some(cc) = cc else { return bad(format!("line {at}: a controller is 0 to 127")) };
                    let mut it = v.split_whitespace();
                    let t = it.next().unwrap_or("");
                    let target = if let Some(r) = t.strip_prefix('e') {
                        r.parse::<u16>().ok().map(Target::Engine)
                    } else if let Some(r) = t.strip_prefix('f') {
                        r.split_once('.').and_then(|(i, id)| Some(Target::Fx(i.parse().ok()?, id.parse().ok()?)))
                    } else {
                        None
                    };
                    let Some(target) = target else { return bad(format!("line {at}: a target is eN or fI.N")) };
                    let (Some(lo), Some(hi)) = (it.next(), it.next()) else { return bad(format!("line {at}: a map needs min and max")) };
                    p.map.push(MapEntry { cc, target, min: num(lo)?, max: num(hi)? });
                }
                other => return bad(format!("line {at}: unknown key {other:?}")),
            }
        }
        if !seen_header {
            return bad("empty");
        }
        if p.engine.is_empty() {
            return bad("no engine");
        }
        if p.licence.is_empty() {
            return bad("no licence (every package carries its source's licence)");
        }
        tier(&p.licence)?;
        if p.program > 0 && p.program >= p.voices.len() {
            p.program = 0;
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = vec![b'a'; 1000];
        assert_eq!(hex(&sha256(&long)), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn tiers() {
        assert_eq!(tier("MIT").unwrap(), Tier::Permissive);
        assert_eq!(tier("MIT AND Apache-2.0").unwrap(), Tier::Permissive);
        assert_eq!(tier("MIT AND GPL-3.0-only").unwrap(), Tier::Copyleft);
        assert_eq!(tier("LGPL-2.1-or-later").unwrap(), Tier::Copyleft);
        for bad in ["", "AGPL-3.0-only", "MIT OR GPL-3.0-only", "GPL-3.0-only WITH Classpath-exception-2.0", "Proprietary", "CC-BY-NC-4.0", "mit"] {
            assert!(tier(bad).is_err(), "{bad:?}");
        }
    }

    fn sample_package() -> Package {
        let mut p = Package::new("Test", "fm6", "GPL-3.0-only");
        p.source = "Felucca-1.8.2.fwsc".into();
        p.source_sha256 = hex(&sha256(b"x"));
        p.verify = "fm6 vs emulator: level 0.04 dB".into();
        p.voices.push([7; 128]);
        p.voices.push([9; 128]);
        p.program = 1;
        p.params.push((200, 900));
        p.samples.push(SampleRef { note: 42, file: "hat.wav".into(), chokes: vec![46] });
        p.fx.push(FxEntry { name: "delay".into(), params: vec![(0, 300)] });
        p.map.push(MapEntry { cc: 1, target: Target::Engine(130), min: 0, max: 99 });
        p.map.push(MapEntry { cc: 74, target: Target::Fx(0, 2), min: 5, max: 100 });
        p
    }

    #[test]
    fn round_trips() {
        let p = sample_package();
        let q = Package::parse(&p.to_text()).unwrap();
        assert_eq!(p, q);
        assert_eq!(q.tier().unwrap(), Tier::Copyleft);
        assert!(q.verified());
    }

    #[test]
    fn refuses_bad_packages() {
        let good = sample_package().to_text();
        assert!(Package::parse("").is_err());
        assert!(Package::parse("ntt = 1\nengine = fm6\nlicence = MIT\n").is_err(), "a Neumetik .ntt header is not ours");
        assert!(Package::parse("name = x\n").is_err(), "no header");
        assert!(Package::parse(&good.replace("licence = GPL-3.0-only\n", "")).is_err(), "no licence");
        assert!(Package::parse(&good.replace("GPL-3.0-only", "AGPL-3.0-only")).is_err());
        assert!(Package::parse(&format!("{good}wat = 1\n")).is_err());
        assert!(Package::parse(&format!("{good}param 1 = notanumber\n")).is_err());
        assert!(Package::parse(&format!("{good}voice = abcd\n")).is_err());
        assert!(Package::parse(&format!("{good}sample 40 = ../../etc/passwd\n")).is_err());
        assert!(Package::parse(&format!("{good}sample 40 = /abs.wav\n")).is_err());
        assert!(Package::parse(&format!("{good}map cc 300 = e1 0 1\n")).is_err());
        assert!(Package::parse("fm1i = 1\nengine = fm6\nlicence = MIT\nfxparam 0 = 1\n").is_err());
        let many = "voice = ".to_string() + &"00".repeat(128) + "\n";
        assert!(Package::parse(&format!("fm1i = 1\nengine = fm6\nlicence = MIT\n{}", many.repeat(257))).is_err());
        assert!(Package::parse(&"x".repeat(MAX_BYTES + 1)).is_err());
        assert!(Package::parse(&format!("{good}{}", "# c\n".repeat(5000))).is_err(), "too many lines");
    }

    #[test]
    fn comments_blank_lines_and_program_clamp() {
        let t = "# hi\nfm1i = 1\n\nengine = analog\nlicence = MIT\nprogram = 9\n";
        let p = Package::parse(t).unwrap();
        assert_eq!(p.engine, "analog");
        assert_eq!(p.program, 0, "past the bank: back to 0");
        assert!(!p.verified());
    }

    #[test]
    fn sequencer_lines_parse_round_trip_and_refuse_nonsense() {
        let t = "fm1i = 1\nname = S\nengine = analog\nlicence = MIT\nseq 0 = 1\nseq 1 = 90\nstep 0 3 = 60 100\nstep 15 15 = 127 1\nseqvoice 2 = 5\n";
        let p = Package::parse(t).unwrap();
        assert_eq!((p.seq.clone(), p.steps.clone(), p.seq_voices.clone()), (vec![(0, 1), (1, 90)], vec![(0, 3, 60, 100), (15, 15, 127, 1)], vec![(2, 5)]));
        assert_eq!(Package::parse(&p.to_text()).unwrap(), p);
        for bad in ["step 0 = 60 100", "step 0 3 = 60", "step 0 3 = 300 100", "step x 3 = 60 100", "seqvoice = 3", "seq = 3"] {
            assert!(Package::parse(&format!("fm1i = 1\nname = S\nengine = analog\nlicence = MIT\n{bad}\n")).is_err(), "{bad}");
        }
        let many: String = (0..300).map(|i| format!("step {} {} = 60 100\n", i % 16, i % 16)).collect();
        assert!(Package::parse(&format!("fm1i = 1\nname = S\nengine = analog\nlicence = MIT\n{many}")).is_err(), "more than 256 steps");
    }
}
