//! `stockbank FIRMWARE.fwsc [SECONDS]`: boots a firmware, runs it, and looks for DX7-style voice banks in the emulated FLASH it left behind (the stock firmware keeps its
//! voices in flash, 4 banks of 32; `fwx` finds none in the package). Reads the firmware as data (rule 3 is about GPL source, and the stock firmware is M-VAVE's, local only).
use fm1ntt::{extract, machine::Machine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: stockbank FIRMWARE.fwsc [SECONDS]");
        std::process::exit(2);
    };
    let secs: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let pkg = std::fs::read(path).expect("read the firmware");
    let mut m = Machine::from_package(&pkg).expect("load");
    let before = m.flash_image().to_vec();
    let mut out = vec![0i16; 2 * 441];
    for _ in 0..secs * 100 {
        m.render(&mut out, 4_000_000);
    }
    let after = m.flash_image().to_vec();
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    println!("flash {} bytes, {} changed by the run (revision {})", after.len(), changed, m.flash_revision());
    let mut ram = vec![0u8; m.region(fm1ntt::machine::Region::Ram).1];
    let n = m.read_mem(fm1ntt::machine::Region::Ram, 0, &mut ram);
    ram.truncate(n);
    let mut xip = vec![0u8; m.region(fm1ntt::machine::Region::Xip).1];
    let n = m.read_mem(fm1ntt::machine::Region::Xip, 0, &mut xip);
    xip.truncate(n);
    if let Some(dump) = args.get(2) {
        std::fs::write(dump, &xip).unwrap();
    }
    for (label, img) in [("flash before", &before), ("flash after", &after), ("ram after", &ram), ("xip", &xip)] {
        for min in [4usize, 1] {
            let banks = extract::find_banks(img, "flash", min);
            let un = extract::find_unpacked(img, min, extract::UNPACKED_LEN);
            println!("{label}: min run {min}: {} packed run(s), {} unpacked run(s)", banks.len(), un.len());
            for b in banks.iter().take(6) {
                println!("  packed at {:#x}: {} voices, names {:?}, algorithms {:?}", b.offset, b.names.len(), b.names, b.algorithms);
            }
            for (at, st, names) in un.iter().take(6) {
                println!("  unpacked at {at:#x} stride {st}: {} voices, first {:?}", names.len(), names.iter().take(4).collect::<Vec<_>>());
            }
        }
    }
}


#[allow(dead_code)]
fn unused() {}
