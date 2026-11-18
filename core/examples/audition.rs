//! `audition FIRMWARE.fwsc [--png OUT.png]`: boots a firmware, plays the standard four-note script on the MIDI jack and prints a verdict (docs/FM1TASTIK.md,
//! `core/src/audition.rs`). The same file gives the same output, to the byte, on any computer. Exit code 0 for Pass, 1 for Silent or Blank, 2 for Halted or an error.

use fm1ntt::audition::{audition_standard, Verdict};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: audition FIRMWARE.fwsc [--png OUT.png]");
        std::process::exit(2);
    };
    let fw = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(2);
        }
    };
    match audition_standard(&fw) {
        Ok(a) => {
            println!("{}", a.summary());
            if let Some(i) = args.iter().position(|a| a == "--png") {
                if let Some(out) = args.get(i + 1) {
                    if let Err(e) = std::fs::write(out, &a.screen_png) {
                        eprintln!("{out}: {e}");
                        std::process::exit(2);
                    }
                }
            }
            std::process::exit(match a.verdict {
                Verdict::Pass => 0,
                Verdict::Silent | Verdict::Blank => 1,
                Verdict::Halted => 2,
            });
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
