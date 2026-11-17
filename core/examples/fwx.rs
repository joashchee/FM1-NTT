//! `fwx FILE.fwsc [--min-run N] [--dump DIR]`: list what a firmware package holds (docs/EXTRACTOR.md).
//! Reads the file, prints a report; `--dump` writes the unscrambled app image under DIR (temp name, then rename).
use fm1ntt::extract;
use std::{fs, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut file, mut min_run, mut dump) = (None, 8usize, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--min-run" => { i += 1; min_run = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(8); }
            "--dump" => { i += 1; dump = args.get(i).cloned(); }
            f => file = Some(f.to_string()),
        }
        i += 1;
    }
    let Some(file) = file else {
        eprintln!("usage: fwx FILE.fwsc [--min-run N] [--dump DIR]");
        return ExitCode::from(2);
    };
    let bytes = match fs::read(&file) { Ok(b) => b, Err(e) => { eprintln!("{file}: {e}"); return ExitCode::from(1); } };
    let r = match extract::report(&bytes, min_run) { Ok(r) => r, Err(e) => { eprintln!("{file}: {e}"); return ExitCode::from(1); } };
    println!("identity {:?}  flash {} bytes  app {} bytes", r.identity, r.flash_len, r.app_len);
    for e in &r.entries { println!("  entry {:<12} offset {:#09x} size {:#09x}", e.name, e.offset, e.size); }
    println!("{} DX7-style voice run(s) of at least {min_run}:", r.banks.len());
    for b in &r.banks {
        println!("  {} @ {:#x}: {} voices, first {:?}", b.image, b.offset, b.names.len(), &b.names[..b.names.len().min(4)]);
    }
    println!("{} strings of {}+ characters (first 30):", r.strings.len(), extract::MIN_STRING);
    for (o, s) in r.strings.iter().take(30) { println!("  {o:#09x} {s}"); }
    if let Some(dir) = dump {
        let app = match extract::app_image(&bytes) { Ok(a) => a, Err(e) => { eprintln!("{e}"); return ExitCode::from(1); } };
        let (d, tmp) = (Path::new(&dir), Path::new(&dir).join("app.img.tmp"));
        if let Err(e) = fs::create_dir_all(d).and_then(|_| fs::write(&tmp, &app)).and_then(|_| fs::rename(&tmp, d.join("app.img"))) {
            eprintln!("{dir}: {e}");
            return ExitCode::from(1);
        }
        println!("wrote {}/app.img ({} bytes)", dir, app.len());
    }
    ExitCode::SUCCESS
}
