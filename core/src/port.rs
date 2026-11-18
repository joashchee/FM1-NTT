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
    let reference = verify::emulator_note(firmware, channel, program, &[], note, velocity, secs)?;
    let mut e = Fm6Engine::new(44_100.0, &v)?;
    for &(id, val) in &pkg.params {
        e.set(id, val);
    }
    let native = verify::render_note(&mut e, note, velocity, secs);
    let n = reference.len().min(native.len());
    Ok(verify::compare(&reference[..n], &native[..n], note, velocity))
}

/// How a Felucca-family firmware (Felucca 1.8.2, Skiff 2.6) is brought to its FM6 preset `preset` (0 is F1) for a verification, as `scripts/fm6-compare.sh` does it
/// (docs/FM6.md): part 2 is FM6 and starts on PAD (F5); the FX page's four knobs go to 0 so only the engine is heard; MIDI channel 2 (0-based 1) plays part 2.
pub const FELUCCA_CHANNEL: u8 = 1;

pub fn felucca_steps(preset: usize) -> Vec<verify::Step> {
    use verify::Step::*;
    // ENCODER_NAMES: SELECT ALGORITHM KNOB1 KNOB2 KNOB3 KNOB4 PRESETS; BUTTON_NAMES: FX is 2, HOME is 8
    vec![Encoder(1, 1), Encoder(6, preset as i32 - 4), Button(2), Encoder(2, -64), Encoder(3, -64), Encoder(4, -64), Encoder(5, -64), Button(8)]
}

/// The worst and mean differences of a sweep.
#[derive(Clone, Copy, Debug)]
pub struct Sweep {
    pub runs: usize,
    pub mean_level_db: f64,
    pub worst_level_db: f64,
    pub worst_pitch_cents: f64,
}

impl Sweep {
    pub fn summary(&self, what: &str, presets: usize) -> String {
        format!(
            "{what}: {presets} presets x notes 36, 60, 84 at velocity 100 ({} runs): level mean {:+.2} dB, worst {:+.2} dB; pitch worst {:.2} cent (level and pitch only)",
            self.runs, self.mean_level_db, self.worst_level_db, self.worst_pitch_cents
        )
    }
}

/// Verifies a Felucca-family port over its first `presets` voices (the preset `p` is voice `p`) at notes 36, 60 and 84: for each, the firmware in the emulator (effects off)
/// against `fm6` with the package's own parameters, DTUN 30 on the PAD preset (voice 4) as the firmware has it when the tuning model is Felucca's. Slow: a few seconds a run.
pub fn verify_felucca_family(firmware: &[u8], pkg: &Package, presets: usize) -> Result<Sweep> {
    let (mut sum, mut worst_level, mut worst_pitch, mut runs) = (0.0, 0.0f64, 0.0f64, 0);
    for p in 0..presets.min(pkg.voices.len()) {
        let mut e = Fm6Engine::new(44_100.0, &pkg.voices[p])?;
        for &(id, val) in &pkg.params {
            e.set(id, val);
        }
        if e.get(FM6_TUNING) == Some(1) {
            e.set(FM6_DTUN, if p == 4 { 30 } else { 0 });
        }
        for note in [36u8, 60, 84] {
            let reference = verify::emulator_note(firmware, FELUCCA_CHANNEL, None, &felucca_steps(p), note, 100, 2.0)?;
            e.all_off();
            let native = verify::render_note(&mut e, note, 100, 2.0);
            let n = reference.len().min(native.len());
            let c = verify::compare(&reference[..n], &native[..n], note, 100);
            sum += c.level_db;
            if c.level_db.abs() > worst_level.abs() {
                worst_level = c.level_db;
            }
            if let Some(pc) = c.pitch_cents {
                worst_pitch = worst_pitch.max(pc.abs());
            }
            runs += 1;
        }
    }
    if runs == 0 {
        return Err(Error::BadImage("verify: nothing to compare".into()));
    }
    Ok(Sweep { runs, mean_level_db: sum / runs as f64, worst_level_db: worst_level, worst_pitch_cents: worst_pitch })
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

    /// Felucca 1.8.2 is in the Pack (a separate repository beside this one); without it the test is skipped and says so.
    #[test]
    fn a_felucca_port_verifies_against_the_emulator() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../FM1-NTT-Firmware-Pack/felucca/felucca-1.8.2.fwsc");
        let Ok(fw) = std::fs::read(path) else {
            eprintln!("skipped: the Firmware Pack is not beside this repository");
            return;
        };
        let (pkg, tier) = port_fm6(&fw, "Felucca", "felucca-1.8.2.fwsc", "GPL-3.0-only AND MIT", Model::Felucca { dtun: 0 }).unwrap();
        assert_eq!(tier, Tier::Copyleft);
        assert_eq!(pkg.voices.len(), 8);
        let w = verify_felucca_family(&fw, &pkg, 1).unwrap(); // TINE EP, three notes
        assert!(w.worst_level_db.abs() < 1.0 && w.worst_pitch_cents < 2.0, "{w:?}");
        assert_eq!(felucca_steps(0).len(), 8);
    }

    #[test]
    fn a_bad_licence_is_refused_before_anything_else() {
        assert!(port_fm6(b"anything", "x", "x", "AGPL-3.0-only", Model::Msfa).is_err());
        assert!(port_fm6(b"anything", "x", "x", "", Model::Msfa).is_err());
        assert!(identify(b"not a package").is_err());
    }
}
