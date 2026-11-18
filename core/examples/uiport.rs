//! `uiport FIRMWARE.fwsc --out DIR [--settle S] [--depth N] [--max N]`: the porter's OBSERVE-SCREEN and OBSERVE-CONTROLS steps (docs/PORTING-UI.md).
//!
//! Boots the firmware in the emulator, then presses every button and turns every encoder one click each way from the home screen, and again from each new
//! screen found, down to `--depth` (default 2) and `--max` probes (default 120). Every probe boots afresh and replays its path (the machine cannot be cloned), so
//! it is slow but exact. Writes to DIR: `pages/NN.png` (every distinct screen, 3x), `controls.tsv` (what each control changed: pixels and the box), `pages.tsv`
//! (background, palette, blobs and text bands of each screen) and `ui-draft.txt`, a first `ui = ...` draft (theme from the home screen's palette, one page per
//! screen, one `key`/`enc` line per control that opened a new screen) that is checked with `Ui::parse` before it is written. The firmware is read as data.
//!
//! A screen's identity is its **layout** (the text bands and the main colours), not its pixels, so an animated element (a spinning disc, a meter) does not make
//! every probe a new screen. Two screens with the same layout and colours are one: a limit, said in the docs.
use fm1ntt::{devices, machine::Machine, screen, ui::Ui};
use std::{collections::HashMap, fs, path::PathBuf, process::ExitCode};

#[derive(Clone)]
enum Act {
    Key(u8),
    Enc(u8, i32),
}

fn name(a: &Act) -> String {
    match a {
        Act::Key(k) => devices::BUTTON_NAMES[*k as usize].to_string(),
        Act::Enc(e, c) => format!("{}{:+}", devices::ENCODER_NAMES[*e as usize], c),
    }
}

fn run(m: &mut Machine, secs: f64) {
    let target = m.ms() + (secs * 1000.0) as u64;
    let mut out = vec![0i16; 2048];
    let mut guard = 0;
    while m.ms() < target && !m.halted() && guard < 1_000_000 {
        guard += 1;
        m.render(&mut out, 2_000_000);
    }
}

fn apply(m: &mut Machine, a: &Act, settle: f64) {
    match a {
        Act::Key(k) => {
            m.key(*k, true);
            run(m, 0.15);
            m.key(*k, false);
        }
        Act::Enc(e, c) => m.encoder(*e, *c),
    }
    run(m, settle);
}

/// boots, replays `path`, returns the machine and its frame
fn at(bytes: &[u8], path: &[Act], settle: f64) -> Option<(Machine, Vec<u32>)> {
    let mut m = Machine::from_package(bytes).ok()?;
    m.set_jit(true);
    run(&mut m, 3.5);
    for a in path {
        apply(&mut m, a, settle);
    }
    let mut f = vec![0u32; screen::W * screen::H];
    m.screen(&mut f);
    Some((m, f))
}

/// a layout signature: the text bands and the three main colours (not the counts)
fn identity(f: &[u32]) -> u64 {
    let bg = screen::background(f);
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |v: u64| h = (h ^ v).wrapping_mul(0x100_0000_01b3);
    for (a, b) in screen::bands(f, bg, 2) {
        mix(a as u64);
        mix(b as u64);
    }
    for (c, _) in screen::palette(f, 3) {
        mix(u64::from(c));
    }
    h
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else { eprintln!("usage: uiport FIRMWARE.fwsc --out DIR [--settle S] [--depth N] [--max N]"); return ExitCode::from(2) };
    let opt = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let Some(out) = opt("--out").map(PathBuf::from) else { eprintln!("--out DIR is needed"); return ExitCode::from(2) };
    let settle: f64 = opt("--settle").and_then(|v| v.parse().ok()).unwrap_or(0.6);
    let depth: usize = opt("--depth").and_then(|v| v.parse().ok()).unwrap_or(2).min(4);
    let max: usize = opt("--max").and_then(|v| v.parse().ok()).unwrap_or(120);
    let Ok(bytes) = fs::read(file) else { eprintln!("{file}: cannot read"); return ExitCode::from(1) };
    if fs::create_dir_all(out.join("pages")).is_err() {
        eprintln!("cannot write {}", out.display());
        return ExitCode::from(1);
    }
    let Some((_, home)) = at(&bytes, &[], settle) else { eprintln!("not a firmware package"); return ExitCode::from(1) };

    let mut pages: Vec<(u64, Vec<Act>)> = vec![(identity(&home), vec![])];
    let mut tsv = String::from("path\taction\tpixels\tbox\tpage\tnew\n");
    let mut ptsv = String::from("page\tpath\tbackground\tpalette\tblobs\tbands\n");
    let mut transitions: Vec<(usize, Act, usize)> = Vec::new();
    let save = |i: usize, path: &[Act], f: &[u32], ptsv: &mut String| {
        let _ = fs::write(out.join("pages").join(format!("{i:02}.png")), screen::png(f, 3));
        let bg = screen::background(f);
        let pal: Vec<String> = screen::palette(f, 6).iter().map(|(c, n)| format!("#{c:06x}:{n}")).collect();
        *ptsv += &format!(
            "{i:02}\t{}\t#{bg:06x}\t{}\t{}\t{}\n",
            path.iter().map(name).collect::<Vec<_>>().join(" "),
            pal.join(" "),
            screen::blobs(f, bg, 12, 200).len(),
            screen::bands(f, bg, 2).len()
        );
    };
    save(0, &[], &home, &mut ptsv);
    let mut probes = 0;
    let mut frontier = vec![0usize];
    for level in 0..depth {
        let mut next = Vec::new();
        for &pi in &frontier {
            let path = pages[pi].1.clone();
            let Some((_, base)) = at(&bytes, &path, settle) else { continue };
            let mut acts: Vec<Act> = (0..devices::BUTTONS as u8).map(Act::Key).collect();
            for e in 0..devices::ENCODERS as u8 {
                acts.push(Act::Enc(e, 1));
                acts.push(Act::Enc(e, -1));
            }
            for a in acts {
                if probes >= max {
                    break;
                }
                probes += 1;
                let mut p = path.clone();
                p.push(a.clone());
                let Some((_, f)) = at(&bytes, &p, settle) else { continue };
                let (px, bx) = screen::diff(&base, &f).map_or((0, String::from("-")), |(r, n)| (n, format!("{},{} {}x{}", r.x, r.y, r.w, r.h)));
                let id = identity(&f);
                let (page, new) = match pages.iter().position(|q| q.0 == id) {
                    Some(i) => (i, false),
                    None => {
                        pages.push((id, p.clone()));
                        save(pages.len() - 1, &p, &f, &mut ptsv);
                        next.push(pages.len() - 1);
                        (pages.len() - 1, true)
                    }
                };
                if page != pi {
                    transitions.push((pi, a.clone(), page));
                }
                tsv += &format!("{}\t{}\t{px}\t{bx}\t{page:02}\t{}\n", path.iter().map(name).collect::<Vec<_>>().join(" "), name(&a), u8::from(new));
            }
        }
        frontier = next;
        println!("level {}: {} screens, {probes} probes", level + 1, pages.len());
    }
    let _ = fs::write(out.join("controls.tsv"), tsv);
    let _ = fs::write(out.join("pages.tsv"), ptsv);

    // a first draft of the UI directives: the theme from the home screen, a page for each screen, the controls that opened one from the home screen
    let bg = screen::background(&home);
    let pal = screen::palette(&home, 8);
    let fg = pal.iter().map(|p| p.0).filter(|&c| c != bg).max_by_key(|&c| (c >> 16 & 255) + (c >> 8 & 255) + (c & 255)).unwrap_or(0xffffff);
    let sat = |c: u32| {
        let (r, g, b) = ((c >> 16 & 255) as i32, (c >> 8 & 255) as i32, (c & 255) as i32);
        r.max(g).max(b) - r.min(g).min(b)
    };
    let accent = pal.iter().map(|p| p.0).filter(|&c| c != bg).max_by_key(|&c| sat(c)).unwrap_or(fg);
    let lum = |c: u32| (c >> 16 & 255) as i32 * 3 + (c >> 8 & 255) as i32 * 6 + (c & 255) as i32;
    // dim: the first other colour that is readable on the background (a clear luminance step), else the foreground
    let dim = pal.iter().map(|p| p.0).find(|&c| c != bg && c != fg && c != accent && (lum(c) - lum(bg)).abs() > 600).unwrap_or(fg);
    let mut lines = vec![format!("theme bg=#{bg:06x} fg=#{fg:06x} dim=#{dim:06x} accent=#{accent:06x}")];
    for i in 0..pages.len() {
        lines.push(format!("page p{i:02} \"PAGE {i:02}\""));
    }
    let mut seen_names: HashMap<String, usize> = HashMap::new();
    for (from, a, to) in &transitions {
        let n = name(a);
        if *from == 0 && !seen_names.contains_key(&n) && !n.ends_with("+1") && !n.ends_with("-1") {
            seen_names.insert(n.clone(), *to);
            lines.push(format!("key {n} page=p{to:02}"));
        }
    }
    let verdict = match Ui::parse(&lines) {
        Ok(_) => "checked: Ui::parse accepts it".to_string(),
        Err(e) => format!("NOT accepted by Ui::parse: {e}"),
    };
    let _ = fs::write(out.join("ui-draft.txt"), lines.iter().map(|l| format!("ui = {l}\n")).collect::<String>() + &format!("# {verdict}\n"));
    println!("{} screens, {probes} probes, {verdict}; wrote {}", pages.len(), out.display());
    ExitCode::SUCCESS
}
