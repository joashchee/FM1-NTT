//! `drive SECONDS SEEDS FIRMWARE.fwsc...`: runs each firmware with random input (buttons, note keys, encoders, the pot, MIDI notes, controllers 0-23,
//! program changes, pitch bend and single-parameter SysEx), after a 4 s boot, and says whether the machine halted (an instruction the core cannot decode or run) and where.
//! A path nobody drove cannot halt it, so this widens what "none was reached" covers (ROADMAP: which of the undecodable forms is executed).
use fm1ntt::machine::Machine;
use std::{fs, process::ExitCode};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn below(&mut self, n: u64) -> u64 { self.next() % n }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 { eprintln!("usage: drive SECONDS SEEDS FIRMWARE.fwsc..."); return ExitCode::from(2); }
    let secs: u64 = args[0].parse().unwrap_or(6);
    let seeds: u64 = args[1].parse().unwrap_or(5);
    let mut bad = 0;
    for path in &args[2..] {
        let pkg = match fs::read(path) { Ok(p) => p, Err(e) => { println!("{path}: {e}"); continue; } };
        let mut halted_at: Vec<String> = Vec::new();
        let mut events = 0u64;
        for seed in 1..=seeds {
            let Ok(mut m) = Machine::from_package(&pkg) else { println!("{path}: does not load"); break };
            let mut buf = vec![0i16; 2 * 441];
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (seed * 0x1000_0000_01B3));
            let boot = 4000u64;
            while m.ms() < boot && !m.halted() { m.render(&mut buf, 2_000_000); }
            let end = boot + secs * 1000;
            let mut held: Vec<u8> = Vec::new();
            while m.ms() < end && !m.halted() {
                match rng.below(12) {
                    0 | 1 => { let id = rng.below(41) as u8; m.key(id, true); held.push(id); }
                    2 => { if let Some(id) = held.pop() { m.key(id, false); } }
                    3 => m.encoder(rng.below(7) as u8, rng.below(9) as i32 - 4),
                    4 => m.pot(rng.below(4096) as u16),
                    5 => { for b in [0x90, 36 + rng.below(60) as u8, 1 + rng.below(126) as u8] { m.midi_in(b); } }
                    6 => { for b in [0xb0, rng.below(24) as u8, rng.below(128) as u8] { m.midi_in(b); } }
                    7 => { for b in [0xc0, rng.below(128) as u8] { m.midi_in(b); } }
                    8 => { for b in [0xe0, rng.below(128) as u8, rng.below(128) as u8] { m.midi_in(b); } }
                    9 => { for b in [0xb0, 123, 0] { m.midi_in(b); } }
                    10 => { for b in [0xf0, 0x7d, rng.below(156) as u8, rng.below(128) as u8, 0xf7] { m.midi_in(b); } }
                    _ => {}
                }
                events += 1;
                m.render(&mut buf, 2_000_000);
            }
            if m.halted() {
                let pc = m.cpu_view(0).map(|c| c.pc).unwrap_or(0);
                halted_at.push(format!("seed {seed}: HALTED at {:.1} s, pc {pc:#x}", m.ms() as f64 / 1000.0));
            }
        }
        let name = std::path::Path::new(path).file_name().and_then(|s| s.to_str()).unwrap_or(path);
        if halted_at.is_empty() { println!("{name}: {seeds} seeds x {secs} s, {events} inputs: none halted"); }
        else { bad += 1; println!("{name}: {}", halted_at.join("; ")); }
    }
    if bad == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
