//! `pcprobe FIRMWARE.fwsc...`: does the number of a MIDI Program Change change a firmware's sound? For each firmware, on channel 1 and on channel 2: a note after Program Change 5
//! and after Program Change 11 (each 1.5 s), and Program Change 5 again as the control (the emulator is deterministic, so the same program must give the same audio). Prints the level
//! and the distance between the two programs' audio as a fraction of the level. "no program change heard" means it does not move on either channel (it may have none, or the
//! note is silent: the level says which).
use fm1ntt::verify::emulator_note;

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

fn dist(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    let d = (0..n).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>() / n.max(1) as f64;
    d.sqrt() / rms(a).max(rms(b)).max(1e-9)
}

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(fw) = std::fs::read(&path) else { continue };
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        let mut line = format!("{name:34}");
        let mut any = false;
        for ch in [0u8, 1] {
            // the two runs take the same time (a program change costs the firmware 0.2 s whatever it does), so a difference between them is the program number alone
            let (Ok(a), Ok(b), Ok(a2)) = (emulator_note(&fw, ch, Some(5), &[], 60, 100, 1.5), emulator_note(&fw, ch, Some(11), &[], 60, 100, 1.5), emulator_note(&fw, ch, Some(5), &[], 60, 100, 1.5)) else {
                line += " (does not run)";
                break;
            };
            let (d, rep) = (dist(&a, &b), dist(&a, &a2));
            any |= d > 0.05 && rms(&a) > 1e-4 && rep < 0.01;
            line += &format!(" ch{} level {:.4} pc5-vs-pc11 d={d:.2} (same program twice {rep:.2})", ch + 1, rms(&a));
        }
        println!("{line} -> {}", if any { "the program number changes the sound" } else { "no program change heard" });
    }
}
