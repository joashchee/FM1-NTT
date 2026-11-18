//! Porting a firmware to an instrument package (docs/PORTING.md, steps 1, 2 and 6, and the native half of 5).
//!
//! `identify` unpacks and classifies; `port_fm6` makes a level-A port (a DX7-style bank into the `fm6` engine); `verify_fm6` plays the same note on the firmware in
//! the emulator and on `fm6`, and stores what it measured in the package. A firmware with no bank is **not** ported: the result says it needs a level B or C
//! port, which is by hand (the engine is fitted from observation, or reimplemented). Reads the firmware as data and links none of it.

use crate::{
    engine::{Engine, Fm6Engine, FM6_DTUN, FM6_OUTPUT, FM6_POT, FM6_TUNING},
    extract,
    package::{hex, sha256, tier, Package, Tier},
    verify, Error, Result,
};

/// What a firmware is, as far as the extractor can tell.
#[derive(Clone, Debug)]
pub struct Identity {
    pub identity: String,
    pub bytes: usize,
    pub sha256: String,
    pub banks: usize,
    pub voices: usize,
    /// engine-family words found in the app's strings (hints, not proof)
    pub hints: Vec<&'static str>,
    pub level: Level,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// A bank the `fm6` engine plays: a data port.
    A,
    /// No bank: the engine has to be fitted (B) or reimplemented (C); by hand.
    BOrC,
}

const HINTS: [&str; 10] = ["dx7", "fm6", "operator", "sampler", "drum", "arp", "sequenc", "wavetable", "granular", "analog"];

pub fn identify(bytes: &[u8]) -> Result<Identity> {
    let r = extract::report(bytes, 4)?;
    let voices = extract::voices(bytes, 4)?;
    let lower: Vec<String> = r.strings.iter().map(|(_, s)| s.to_lowercase()).collect();
    let hints = HINTS.iter().copied().filter(|h| lower.iter().any(|s| s.contains(h))).collect();
    Ok(Identity {
        identity: r.identity,
        bytes: bytes.len(),
        sha256: hex(&sha256(bytes)),
        banks: r.banks.len(),
        voices: voices.len(),
        hints,
        level: if voices.is_empty() { Level::BOrC } else { Level::A },
    })
}

/// How the native engine is set up to sound like the firmware's own: `Msfa` (no model), or the measured models of `fm6` (`docs/FM6.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Msfa,
    /// Felucca 1.8.2's FM6 engine and output stage (and the firmwares built on its platform that carry its engine: unchecked per firmware).
    Felucca { dtun: u8 },
    /// The stock FM-1 engine as DX747 runs it.
    Stock,
}

fn apply_model(e: &mut dyn Engine, model: Model) {
    match model {
        Model::Msfa => {}
        Model::Felucca { dtun } => {
            e.set(FM6_OUTPUT, 1);
            e.set(FM6_TUNING, 1);
            e.set(FM6_DTUN, dtun as i32);
        }
        Model::Stock => {
            e.set(FM6_OUTPUT, 2);
            e.set(FM6_TUNING, 2);
        }
    }
    e.set(FM6_POT, 900);
}

/// A level-A port. `licence` is the firmware's own (read from its licence files, not a repository's badge: `CLAUDE.md` rule 2), an AND-only expression on the
/// allowlist; the returned `Tier` says where the package may ship. Fails with a reason when the firmware has no bank (level B or C).
pub fn port_fm6(bytes: &[u8], name: &str, source: &str, licence: &str, model: Model) -> Result<(Package, Tier)> {
    let t = tier(licence)?;
    let id = identify(bytes)?;
    if id.level != Level::A {
        return Err(Error::BadImage(
            "no DX7-style voice bank found: this firmware's engine is not a bank for fm6, so it needs a level B port (fit an engine to what it does in the emulator) or level C (reimplement it); see docs/PORTING.md".into(),
        ));
    }
    let mut p = Package::new(name, "fm6", licence);
    p.source = source.into();
    p.source_sha256 = id.sha256;
    p.voices = extract::voices(bytes, 4)?;
    let mut e = Fm6Engine::new(44_100.0, &p.voices[0])?;
    apply_model(&mut e, model);
    for d in e.params() {
        if d.id >= FM6_POT {
            if let Some(v) = e.get(d.id) {
                if v != d.default {
                    p.params.push((d.id, v));
                }
            }
        }
    }
    Ok((p, t))
}

/// Plays `note` on the firmware (the emulator) and on the port's `fm6` (voice `voice`, the same MIDI program selecting it on the firmware if `program` is given), and
/// returns the comparison. The firmware's own program numbering is the firmware's: choose `program` and `voice` so they are the same sound.
pub fn verify_fm6(firmware: &[u8], pkg: &Package, voice: usize, channel: u8, program: Option<u8>, note: u8, velocity: u8) -> Result<verify::Comparison> {
    let v = *pkg.voices.get(voice).ok_or_else(|| Error::BadImage("verify: the package has no such voice".into()))?;
    let secs = 2.0;
    let reference = verify::emulator_note(firmware, channel, program, note, velocity, secs)?;
    let mut e = Fm6Engine::new(44_100.0, &v)?;
    for &(id, val) in &pkg.params {
        e.set(id, val);
    }
    let native = verify::render_note(&mut e, note, velocity, secs);
    let n = reference.len().min(native.len());
    Ok(verify::compare(&reference[..n], &native[..n], note, velocity))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled() -> Option<Vec<u8>> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../bundled-firmware/haunted-fm1/untrue-0.1.fwsc")).ok()
    }

    #[test]
    fn a_firmware_with_no_bank_is_not_ported_and_says_why() {
        let Some(fw) = bundled() else { return };
        let id = identify(&fw).unwrap();
        assert_eq!(id.level, Level::BOrC);
        assert_eq!(id.sha256.len(), 64);
        let e = port_fm6(&fw, "UNTRUE STEP", "untrue-0.1.fwsc", "MIT", Model::Msfa).unwrap_err();
        assert!(e.to_string().contains("level B"), "{e}");
    }

    #[test]
    fn a_bad_licence_is_refused_before_anything_else() {
        assert!(port_fm6(b"anything", "x", "x", "AGPL-3.0-only", Model::Msfa).is_err());
        assert!(port_fm6(b"anything", "x", "x", "", Model::Msfa).is_err());
        assert!(identify(b"not a package").is_err());
    }
}
