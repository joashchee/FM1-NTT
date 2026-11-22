//! Fuzzes the machine with random instruction streams: rule 5 says firmware is untrusted input, so no stream may panic, hang or grow without bound
//! (`docs/ARCHITECTURE.md`). Each case is a raw image of random bytes at the entry point, run for a bounded number of instructions with the JIT off and
//! on; a panic is caught and its message, location and image are kept. A reproduction needs `cargo run --example fuzzmachine -- --one OUT/crash-SEED.bin`.
//!   CARGO_PROFILE_RELEASE_PANIC=unwind cargo run --release --example fuzzmachine -- [CASES] [FIRST_SEED] [OUT_DIR]
//!   ... --example fuzzmachine -- --package FILE.fwsc [CASES] [FIRST_SEED] [OUT_DIR]   mutates a real package (flips, truncations, overwrites), loads it and runs what loads.
//! (the release profile aborts on a panic, as the C API needs, so the catch needs the override). Reads no firmware; writes only the failing images to OUT_DIR.
use fm1ntt::machine::Machine;
use std::panic::{catch_unwind, AssertUnwindSafe};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// One image: the 0x120 bytes before the entry, then random bytes. A third of the cases are biased toward the addresses a program might load from or store to
/// (the little-endian words 0x01Cx_xxxx, 0x0200_xxxx and the edges of RAM) so memory accesses are reached sooner.
fn image(seed: u64) -> Vec<u8> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut img = vec![0u8; 0x120];
    let n = 1024 + (r.next() % 3072) as usize;
    let mode = r.next() % 3;
    while img.len() < 0x120 + n {
        let w = r.next();
        let b = w.to_le_bytes();
        if mode == 2 && w & 0xF0 == 0 {
            // a 4-byte pattern that looks like a RAM or XIP address, or the end of RAM
            let hi = [0x01c0u16, 0x01cf, 0x01d0, 0x0200, 0x0201, 0x01ee, 0x0001, 0x0004][(w >> 8) as usize & 7];
            img.extend_from_slice(&((u32::from(hi) << 16) | (w >> 32) as u32 & 0xFFFF).to_le_bytes());
        } else {
            img.extend_from_slice(&b[..(2 + (w >> 60) as usize % 5) & !1]);
        }
    }
    img
}

fn run_case(img: &[u8], jit: bool) {
    let Ok(mut m) = Machine::from_raw(img.to_vec(), 0x0200_0120) else { return };
    m.set_jit(jit);
    let mut out = vec![0i16; 2 * 64];
    for _ in 0..6 {
        m.render(&mut out, 30_000);
        if m.halted() {
            break;
        }
    }
    m.screen(&mut vec![0u32; 57_600]);
    let mut b = [0u8; 64];
    let _ = m.read_mem(fm1ntt::machine::Region::Ram, 0, &mut b);
}

/// A real package with a few bytes changed: flips, an overwritten run, or cut short (and, now and then, padded with noise).
fn mutate(pkg: &[u8], seed: u64) -> Vec<u8> {
    let mut r = Rng(seed.wrapping_mul(0xD6E8_FEB8_6659_FD93) | 1);
    let mut v = pkg.to_vec();
    match r.next() % 4 {
        0 => {
            for _ in 0..1 + r.next() % 8 {
                let at = r.next() as usize % v.len();
                v[at] ^= 1 << (r.next() % 8);
            }
        }
        1 => {
            let at = r.next() as usize % v.len();
            for i in 0..(1 + r.next() % 64) as usize {
                if let Some(b) = v.get_mut(at + i) {
                    *b = r.next() as u8;
                }
            }
        }
        2 => v.truncate(r.next() as usize % v.len()),
        _ => {
            // the identity weave and the file list live in the first few hundred bytes: change them more often
            for _ in 0..1 + r.next() % 4 {
                let at = r.next() as usize % v.len().min(0x600);
                v[at] = r.next() as u8;
            }
        }
    }
    v
}

fn run_package(pkg: &[u8], jit: bool) {
    let Ok(mut m) = Machine::from_package(pkg) else { return };
    m.set_jit(jit);
    let mut out = vec![0i16; 2 * 128];
    for _ in 0..3 {
        m.render(&mut out, 200_000);
        if m.halted() {
            break;
        }
    }
    m.screen(&mut vec![0u32; 57_600]);
    let _ = m.flash_image().len();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).is_some_and(|s| s == "--package") {
        let pkg = std::fs::read(&a[2]).expect("cannot read the package");
        let cases: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(500);
        let first: u64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
        let out = a.get(5).cloned();
        let last = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        {
            let last = last.clone();
            std::panic::set_hook(Box::new(move |info| {
                let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
                let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
                *last.lock().unwrap() = format!("{loc}: {msg}");
            }));
        }
        let (mut loaded, mut found): (u64, std::collections::BTreeMap<String, (u64, usize)>) = (0, Default::default());
        for seed in first..first + cases {
            let v = mutate(&pkg, seed);
            match catch_unwind(AssertUnwindSafe(|| Machine::from_package(&v).is_ok())) {
                Ok(ok) => loaded += u64::from(ok),
                Err(_) => {
                    let why = last.lock().unwrap().clone();
                    let e = found.entry(format!("(from_package) {why}")).or_insert((seed, 0));
                    e.1 += 1;
                    if e.1 == 1 {
                        if let Some(dir) = &out {
                            let _ = std::fs::create_dir_all(dir);
                            let _ = std::fs::write(format!("{dir}/package-crash-{seed}.fwsc"), &v);
                        }
                    }
                    continue;
                }
            }
            for jit in [false, true] {
                if catch_unwind(AssertUnwindSafe(|| run_package(&v, jit))).is_err() {
                    let why = last.lock().unwrap().clone();
                    let e = found.entry(why).or_insert((seed, 0));
                    e.1 += 1;
                    if e.1 == 1 {
                        if let Some(dir) = &out {
                            let _ = std::fs::create_dir_all(dir);
                            let _ = std::fs::write(format!("{dir}/package-crash-{seed}.fwsc"), &v);
                        }
                    }
                }
            }
        }
        println!("{cases} mutations from seed {first}: {loaded} still loaded; {} distinct panic site(s)", found.len());
        for (why, (seed, n)) in &found {
            println!("  {n:5} x  {why}   (first: seed {seed})");
        }
        return;
    }
    if a.get(1).is_some_and(|s| s == "--one") {
        let img = std::fs::read(&a[2]).expect("cannot read the image");
        for jit in [false, true] {
            run_case(&img, jit);
            println!("jit {jit}: no panic");
        }
        return;
    }
    let cases: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let first: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let out = a.get(3).cloned();
    let last = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    {
        let last = last.clone();
        std::panic::set_hook(Box::new(move |info| {
            let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
            *last.lock().unwrap() = format!("{loc}: {msg}");
        }));
    }
    let mut found: std::collections::BTreeMap<String, (u64, bool, usize)> = Default::default();
    for seed in first..first + cases {
        let img = image(seed);
        for jit in [false, true] {
            if catch_unwind(AssertUnwindSafe(|| run_case(&img, jit))).is_err() {
                let why = last.lock().unwrap().clone();
                let e = found.entry(why).or_insert((seed, jit, 0));
                e.2 += 1;
                if e.2 == 1 {
                    if let Some(dir) = &out {
                        let _ = std::fs::create_dir_all(dir);
                        let _ = std::fs::write(format!("{dir}/crash-{seed}.bin"), &img);
                    }
                }
            }
        }
    }
    println!("{cases} cases from seed {first}, JIT off and on: {} distinct panic site(s)", found.len());
    for (why, (seed, jit, n)) in &found {
        println!("  {n:5} x  {why}   (first: seed {seed}, jit {jit})");
    }
}
