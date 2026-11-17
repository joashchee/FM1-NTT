//! `fwprobe FIRMWARE.fwsc ACTION...`: boots a firmware and drives its controls, writing the FM-1's screen as PNG, to see what a control does.
//! Actions: `wait:S` run S emulated seconds; `enc:NAME:CLICKS` turn an encoder (SELECT ALGORITHM KNOB1-4 PRESETS); `key:NAME` press and release a
//! button (OCT- OCT+ FX SCL ENV LFO EDIT GLO HOME SAVE ARP SEQ PLAY REC), `hold:NAME:S` hold one S seconds, `note:N` a note key 0..26, `info` state, `snap` keep the screen and `sheet:FILE:COLS` write the kept screens as one PNG; `shot:FILE.png`; `midi:HEX,HEX,...`.
use fm1ntt::{devices, machine::Machine};
use std::{fs, process::ExitCode};

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 { c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 }; }
    }
    !c
}
fn png(path: &str, px: &[u32], w: usize, h: usize, scale: usize) -> std::io::Result<()> {
    let mut raw = Vec::new();
    for y in 0..h * scale {
        raw.push(0);
        for x in 0..w * scale { let p = px[(y / scale) * w + x / scale]; raw.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]); }
    }
    let mut z = vec![0x78, 0x01];                       // zlib, stored blocks
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(if i + 1 == chunks.len() { 1 } else { 0 });
        z.extend((c.len() as u16).to_le_bytes());
        z.extend((!(c.len() as u16)).to_le_bytes());
        z.extend(*c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw { a = (a + v as u32) % 65521; b = (b + a) % 65521; }
    z.extend(((b << 16) | a).to_be_bytes());
    let chunk = |tag: &[u8; 4], data: &[u8]| { let mut c = (data.len() as u32).to_be_bytes().to_vec(); c.extend(tag); c.extend(data); let crc = crc32(&c[4..]); c.extend(crc.to_be_bytes()); c };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = ((w * scale) as u32).to_be_bytes().to_vec();
    ihdr.extend(((h * scale) as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &z));
    out.extend(chunk(b"IEND", &[]));
    fs::write(path, out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else { eprintln!("usage: fwprobe FIRMWARE.fwsc ACTION..."); return ExitCode::from(2) };
    let bytes = match fs::read(file) { Ok(b) => b, Err(e) => { eprintln!("{file}: {e}"); return ExitCode::from(1) } };
    let mut m = match Machine::from_package(&bytes) { Ok(m) => m, Err(e) => { eprintln!("{e}"); return ExitCode::from(1) } };
    m.set_jit(true);
    let mut buf = vec![0i16; 2 * 4096];
    let mut run = |m: &mut Machine, secs: f64| {
        let target = m.ms() + (secs * 1000.0) as u64;
        let mut guard = 0;
        while m.ms() < target && !m.halted() && guard < 1_000_000 { guard += 1; m.render(&mut buf[..2 * 1024], 2_000_000); }
    };
    run(&mut m, 3.0);
    let mut snaps: Vec<Vec<u32>> = Vec::new();
    for a in &args[1..] {
        let f: Vec<&str> = a.split(':').collect();
        match f[0] {
            "wait" => run(&mut m, f[1].parse().unwrap_or(0.5)),
            "enc" => {
                let Some(e) = devices::ENCODER_NAMES.iter().position(|n| *n == f[1]) else { eprintln!("no encoder {}", f[1]); return ExitCode::from(2) };
                m.encoder(e as u8, f.get(2).and_then(|c| c.parse().ok()).unwrap_or(1));
                run(&mut m, 0.4);
            }
            "key" => {
                let Some(k) = devices::BUTTON_NAMES.iter().position(|n| *n == f[1]) else { eprintln!("no button {}", f[1]); return ExitCode::from(2) };
                m.key(k as u8, true); run(&mut m, 0.15); m.key(k as u8, false); run(&mut m, 0.4);
            }
            "hold" => {
                let Some(k) = devices::BUTTON_NAMES.iter().position(|n| *n == f[1]) else { eprintln!("no button {}", f[1]); return ExitCode::from(2) };
                m.key(k as u8, true); run(&mut m, f.get(2).and_then(|c| c.parse().ok()).unwrap_or(0.8)); m.key(k as u8, false); run(&mut m, 0.5);
            }
            "note" => {
                let k = devices::KEY_ID_BASE + f[1].parse::<u8>().unwrap_or(0);
                m.key(k, true); run(&mut m, 0.15); m.key(k, false); run(&mut m, 0.4);
            }
            "midi" => { for h in f[1].split(',') { m.midi_in(u8::from_str_radix(h, 16).unwrap_or(0)); } run(&mut m, 0.3); }
            "info" => { let mut px = vec![0u32; 240 * 240]; m.screen(&mut px); println!("halted {} ms {} lcd revision {} lit pixels {} insns {}", m.halted(), m.ms(), m.lcd_revision(), px.iter().filter(|&&p| p != 0).count(), m.instructions()); }
            "snap" => { let mut px = vec![0u32; 240 * 240]; m.screen(&mut px); snaps.push(px); }
            "sheet" => {
                let cols = f.get(2).and_then(|c| c.parse().ok()).unwrap_or(3usize).max(1);
                let rows = snaps.len().div_ceil(cols);
                let (w, h) = (cols * 240, rows * 240);
                let mut all = vec![0u32; w * h];
                for (i, sn) in snaps.iter().enumerate() { for y in 0..240 { for x in 0..240 { all[((i / cols) * 240 + y) * w + (i % cols) * 240 + x] = sn[y * 240 + x]; } } }
                if let Err(e) = png(f[1], &all, w, h, 1) { eprintln!("{}: {e}", f[1]); return ExitCode::from(1) }
                println!("wrote {} ({} screens)", f[1], snaps.len());
            }
            "shot" => {
                let mut px = vec![0u32; 240 * 240];
                m.screen(&mut px);
                if let Err(e) = png(f[1], &px, 240, 240, 2) { eprintln!("{}: {e}", f[1]); return ExitCode::from(1) }
                println!("wrote {}", f[1]);
            }
            x => { eprintln!("unknown action {x}"); return ExitCode::from(2) }
        }
    }
    ExitCode::SUCCESS
}
