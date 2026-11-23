//! The widget detector (`screen::detect`) on real screens: each firmware that is here is booted and its home screen read. The bundled ones (`bundled-firmware/`, permissive) always
//! run; the Firmware Pack's and the stock firmware (M-VAVE's, local only) run when they are present and are skipped, with a line saying so, when not. Prints what it found, so
//! the numbers are in the log; asserts only what must hold of any screen (a draft `Ui::parse` accepts, bounded, text found where the screen has text).

use fm1ntt::{machine::Machine, screen, ui::Ui};

fn home(path: &str) -> Option<Vec<u32>> {
    let pkg = std::fs::read(path).ok()?;
    let mut m = Machine::from_package(&pkg).ok()?;
    m.set_jit(true);
    let mut out = vec![0i16; 2 * 441];
    for _ in 0..500 {
        m.render(&mut out, 4_000_000);
    }
    let mut px = vec![0u32; screen::W * screen::H];
    m.screen(&mut px);
    Some(px)
}

fn read_dir(dir: &str) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                v.extend(read_dir(&p.to_string_lossy()));
            } else if p.extension().is_some_and(|x| x == "fwsc") {
                v.push(p.to_string_lossy().to_string());
            }
        }
    }
    v.sort();
    v
}

#[test]
fn the_detector_reads_real_home_screens() {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut files = read_dir(&format!("{root}/../bundled-firmware"));
    let bundled = files.len();
    files.extend(read_dir(&format!("{root}/../../FM1-NTT-Firmware-Pack")));
    files.push(format!("{root}/../docs/FM-1.fwsc"));
    assert!(bundled >= 5, "the bundled firmware is here");
    let mut with_text = 0;
    let mut run = 0;
    for f in &files {
        let Some(px) = home(f) else {
            eprintln!("skipped: {f}");
            continue;
        };
        run += 1;
        let bg = screen::background(&px);
        let things = screen::detect(&px, bg);
        let count = |pred: &dyn Fn(&screen::Shape) -> bool| things.iter().filter(|d| pred(&d.shape)).count();
        let (t, b, m, d, g, x) = (
            count(&|s| matches!(s, screen::Shape::Text { .. })),
            count(&|s| matches!(s, screen::Shape::Bar)),
            count(&|s| matches!(s, screen::Shape::Meter)),
            count(&|s| matches!(s, screen::Shape::Disc)),
            count(&|s| matches!(s, screen::Shape::Grid { .. })),
            count(&|s| matches!(s, screen::Shape::Box)),
        );
        eprintln!("{:40} {} things: {t} text, {b} bar, {m} meter, {d} disc, {g} grid, {x} box", f.rsplit('/').next().unwrap_or(f), things.len());
        with_text += usize::from(t > 0);
        assert!(things.len() <= 256);
        let (defs, ws) = screen::draft_lines("home", &things);
        let mut draft = vec![format!("theme bg=#{bg:06x} fg=#ffffff dim=#808080 accent=#ff8040"), "page home \"HOME\"".to_string()];
        draft.extend(defs);
        draft.extend(ws);
        Ui::parse(&draft).unwrap_or_else(|e| panic!("{f}: the draft is refused: {e}"));
    }
    assert!(run >= 5);
    assert!(with_text >= 3, "most home screens carry text: {with_text} of {run}");
}
