//! `port FIRMWARE.fwsc --licence EXPR [--name N] [--source FILE] [--model msfa|felucca[:DTUN]|stock] [--verify [--program P] [--voice I] [--channel C] [--note N] [--vel V]] [--out FILE.ntt]`
//! `port --check PACKAGE.ntt`
//!
//! The porting tool (docs/PORTING.md): identify a firmware, make a level-A port (its DX7-style bank into the `fm6` engine) and optionally verify it against the
//! emulator. `--licence` is the firmware's own, read from its licence files; the tier it prints says where the package may ship. A firmware with no bank is
//! not ported and the reason is printed. `--check` loads a package the way the app does and lists it.
use fm1ntt::{
    instrument::Instrument,
    package::{Package, Tier},
    port,
};
use std::{fs, path::Path, process::ExitCode};

fn write_atomic(path: &str, text: &str) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, Path::new(path))
}

fn tier_line(t: Tier) -> &'static str {
    match t {
        Tier::Permissive => "permissive: may go in bundled-firmware/ (the MIT app)",
        Tier::Copyleft => "COPYLEFT: the FM1-NTT Firmware Pack only; never the MIT app's package or a Mac App Store build",
    }
}

fn check(path: &str) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    let pkg = match Package::parse(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    let dir = Path::new(path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let resolve = |f: &str| fs::read(dir.join(f)).ok();
    let inst = match Instrument::from_package(&pkg, 44_100.0, &resolve) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    println!("{}: {} ({}), {} voices, {} effects, {} parameters", path, inst.name(), inst.kind(), inst.voice_count(), inst.fx_count(), inst.params().len());
    println!("licence {} -> {}", inst.licence(), pkg.tier().map(tier_line).unwrap_or("?"));
    println!("verification: {}", if inst.verify_note().is_empty() { "none (unverified)" } else { inst.verify_note() });
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut file, mut licence, mut name, mut source, mut out, mut model) = (None, None, None, None, None, "msfa".to_string());
    let (mut verify, mut program, mut voice, mut channel, mut note, mut vel) = (false, None, 0usize, 0u8, 60u8, 100u8);
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--check" => return check(&next(&mut i).unwrap_or_default()),
            "--licence" => licence = next(&mut i),
            "--name" => name = next(&mut i),
            "--source" => source = next(&mut i),
            "--out" => out = next(&mut i),
            "--model" => model = next(&mut i).unwrap_or_default(),
            "--verify" => verify = true,
            "--program" => program = next(&mut i).and_then(|s| s.parse().ok()),
            "--voice" => voice = next(&mut i).and_then(|s| s.parse().ok()).unwrap_or(0),
            "--channel" => channel = next(&mut i).and_then(|s| s.parse().ok()).unwrap_or(0),
            "--note" => note = next(&mut i).and_then(|s| s.parse().ok()).unwrap_or(60),
            "--vel" => vel = next(&mut i).and_then(|s| s.parse().ok()).unwrap_or(100),
            f => file = Some(f.to_string()),
        }
        i += 1;
    }
    let (Some(file), Some(licence)) = (file, licence) else {
        eprintln!("usage: port FIRMWARE.fwsc --licence EXPR [--name N] [--source FILE] [--model msfa|felucca[:DTUN]|stock] [--verify ...] [--out FILE.ntt]\n       port --check PACKAGE.ntt");
        return ExitCode::from(2);
    };
    let bytes = match fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::from(1);
        }
    };
    let id = match port::identify(&bytes) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::from(1);
        }
    };
    println!("{file}: {:?}, {} bytes, sha256 {}", id.identity, id.bytes, id.sha256);
    println!("  {} voice run(s), {} voices; engine hints in its strings: {:?}", id.banks, id.voices, id.hints);
    println!("  port level: {}", if id.level == port::Level::A { "A (a bank for fm6)" } else { "B or C (no bank: by hand)" });
    let m = match model.split_once(':') {
        None if model == "msfa" => port::Model::Msfa,
        None if model == "stock" => port::Model::Stock,
        Some(("felucca", d)) => port::Model::Felucca { dtun: d.parse().unwrap_or(0) },
        None if model == "felucca" => port::Model::Felucca { dtun: 0 },
        _ => {
            eprintln!("--model: msfa, felucca[:DTUN] or stock");
            return ExitCode::from(2);
        }
    };
    let base = Path::new(&file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let name = name.unwrap_or_else(|| id.identity.clone());
    let (mut pkg, tier) = match port::port_fm6(&bytes, &name, &source.unwrap_or_else(|| base.clone()), &licence, m) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("not ported: {e}");
            return ExitCode::from(1);
        }
    };
    println!("  licence {licence} -> {}", tier_line(tier));
    if verify {
        match port::verify_fm6(&bytes, &pkg, voice, channel, program, note, vel) {
            Ok(c) => {
                pkg.verify = c.summary(&format!("fm6 ({model}) vs the firmware in the emulator"));
                println!("  verify: {}", pkg.verify);
                println!("  {}", if c.within(1.0, 5.0) { "within 1 dB and 5 cents" } else { "NOT within 1 dB and 5 cents: check --program/--voice pair the same sound, or the model" });
            }
            Err(e) => {
                eprintln!("verify failed: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        println!("  unverified (run with --verify)");
    }
    let out = out.unwrap_or_else(|| format!("{}.ntt", base.trim_end_matches(".fwsc")));
    match write_atomic(&out, &pkg.to_text()) {
        Ok(()) => println!("wrote {out}"),
        Err(e) => {
            eprintln!("{out}: {e}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}
