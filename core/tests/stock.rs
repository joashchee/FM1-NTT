//! A regression test against the stock FM-1 firmware, which is M-VAVE's and **local only** (docs/FM-1.fwsc, gitignored: CLAUDE.md rule 2).
//! It runs when the file is present and is skipped, with a line saying so, when it is not (as the Pack's tests are). Nothing here ships:
//! the screens are the oracle for what the manual says (docs/HARDWARE.md).

use fm1ntt::devices::{SCREEN_H, SCREEN_W};
use fm1ntt::machine::Machine;

fn stock() -> Option<Machine> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/FM-1.fwsc");
    match std::fs::read(path) {
        Ok(pkg) => Some(Machine::from_package(&pkg).expect("the stock package loads")),
        Err(_) => {
            eprintln!("skipped: docs/FM-1.fwsc is not here (local only)");
            None
        }
    }
}

/// Runs about `ms` of emulated time.
fn run(m: &mut Machine, ms: usize) {
    let mut out = vec![0i16; 2 * 441];
    for _ in 0..ms / 10 {
        m.render(&mut out, 4_000_000);
    }
}

fn shot(m: &Machine) -> Vec<u32> {
    let mut px = vec![0u32; SCREEN_W * SCREEN_H];
    m.screen(&mut px);
    px
}

fn distinct(px: &[u32]) -> usize {
    px.iter().collect::<std::collections::HashSet<_>>().len()
}

fn tap(m: &mut Machine, id: u8) {
    m.key(id, true);
    run(m, 120);
    m.key(id, false);
    run(m, 300);
}

fn differs(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

const OCT_MINUS: u8 = 0;
const OCT_PLUS: u8 = 1;
const EDIT: u8 = 6;
/// Note key 0 is F3 (id 14); the black keys that select OP1..OP6, PIT, GLO in EDIT are keys 1, 3, 5, 8, 10, 13, 15, 17.
const EDIT_KEYS: [u8; 8] = [1, 3, 5, 8, 10, 13, 15, 17];

#[test]
fn boots_draws_and_does_not_halt() {
    let Some(mut m) = stock() else { return };
    run(&mut m, 3000);
    assert!(!m.halted(), "the stock firmware halted");
    assert!(distinct(&shot(&m)) > 8, "the stock firmware drew nothing");
}

/// The OCT+ popup is drawn when the button is released, and goes away by itself.
#[test]
fn octave_up_shows_a_popup_that_goes_away() {
    let Some(mut m) = stock() else { return };
    run(&mut m, 3000);
    let home = shot(&m);
    m.key(OCT_PLUS, true);
    run(&mut m, 120);
    m.key(OCT_PLUS, false);
    run(&mut m, 150);
    let popup = shot(&m);
    assert!(differs(&home, &popup) > 200, "OCT+ drew no popup");
    run(&mut m, 4000);
    let after = shot(&m);
    assert!(differs(&popup, &after) > 100, "the popup stayed");
}

/// Octave is -3 to +3: a fourth press up shows the same popup as the third did (the clamp), and -3 differs from +3.
#[test]
fn octave_is_clamped_at_three_each_way() {
    let popup_after = |id: u8, presses: usize| {
        let mut m = stock().unwrap();
        run(&mut m, 3000);
        for _ in 0..presses {
            m.key(id, true);
            run(&mut m, 120);
            m.key(id, false);
            run(&mut m, 150);
        }
        shot(&m)
    };
    if stock().is_none() {
        return;
    }
    let up3 = popup_after(OCT_PLUS, 3);
    let up5 = popup_after(OCT_PLUS, 5);
    assert_eq!(differs(&up3, &up5), 0, "more presses moved the octave past +3");
    let down3 = popup_after(OCT_MINUS, 3);
    let down5 = popup_after(OCT_MINUS, 5);
    assert_eq!(differs(&down3, &down5), 0, "more presses moved the octave past -3");
    assert!(differs(&up3, &down3) > 50, "-3 and +3 look the same");
}

#[test]
fn edit_black_keys_select_each_page_in_order() {
    let Some(mut m) = stock() else { return };
    run(&mut m, 3000);
    tap(&mut m, EDIT);
    let edit = shot(&m);
    let mut seen: Vec<Vec<u32>> = vec![];
    for k in EDIT_KEYS {
        tap(&mut m, 14 + k);
        let s = shot(&m);
        assert!(differs(&edit, &s) > 100 || !seen.is_empty(), "key {k} left EDIT unchanged");
        assert!(!seen.iter().any(|p| differs(p, &s) == 0), "key {k} shows the same page as an earlier one");
        seen.push(s);
    }
}

/// Program Change over the MIDI jack selects voices 001-128: two different programs show two different screens.
#[test]
fn program_change_selects_a_voice() {
    let Some(mut m) = stock() else { return };
    run(&mut m, 3000);
    for b in [0xC0, 5] {
        m.midi_in(b);
    }
    run(&mut m, 800);
    let a = shot(&m);
    for b in [0xC0, 32] {
        m.midi_in(b);
    }
    run(&mut m, 800);
    let b = shot(&m);
    assert!(differs(&a, &b) > 100, "program change 5 and 32 look the same");
}
