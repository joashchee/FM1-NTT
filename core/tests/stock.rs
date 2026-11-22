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

const ALGORITHM: u8 = 1; // the encoder index (SELECT 0, ALGORITHM 1)

/// Transpose is 0 to -12 or 0 to +12, set by holding OCT+ or OCT- and turning ALGORITHM (up is plus, down is minus, whichever button is held) (manual; docs/HARDWARE.md). The popup shows it when the button is released:
/// more turns than 12 change nothing, +12 and -12 look different, and pressing both buttons together puts it back.
#[test]
fn transpose_is_set_with_octave_held_and_clamps_at_twelve() {
    let after = |button: u8, turns: i32| {
        let mut m = stock().unwrap();
        run(&mut m, 3000);
        m.key(button, true);
        run(&mut m, 100);
        for _ in 0..turns.abs() {
            m.encoder(ALGORITHM, turns.signum());
            run(&mut m, 60);
        }
        m.key(button, false);
        run(&mut m, 200);
        shot(&m)
    };
    if stock().is_none() {
        return;
    }
    let plus12 = after(OCT_PLUS, 12);
    let plus16 = after(OCT_PLUS, 16);
    let plus1 = after(OCT_PLUS, 1);
    assert_eq!(differs(&plus12, &plus16), 0, "more than 12 turns up moved the transpose past +12");
    assert!(differs(&plus12, &plus1) > 20, "+12 and +1 look the same");
    let minus12 = after(OCT_MINUS, -12); // the direction of the turn is the sign: either button held sets the same transpose
    assert!(differs(&plus12, &minus12) > 20, "+12 and -12 look the same");
    let minus16 = after(OCT_MINUS, -16);
    assert_eq!(differs(&minus12, &minus16), 0, "more than 12 turns down moved the transpose past -12");
}

/// Both octave buttons together put octave and transpose back to zero: after a transpose and an octave step, pressing both gives the popup of a fresh machine's reset.
#[test]
fn both_octave_buttons_reset() {
    let Some(mut m) = stock() else { return };
    run(&mut m, 3000);
    m.key(OCT_PLUS, true);
    run(&mut m, 100);
    for _ in 0..5 {
        m.encoder(ALGORITHM, 1);
        run(&mut m, 60);
    }
    m.key(OCT_PLUS, false);
    run(&mut m, 200);
    tap(&mut m, OCT_PLUS);
    let changed = shot(&m);
    m.key(OCT_MINUS, true);
    m.key(OCT_PLUS, true);
    run(&mut m, 150);
    m.key(OCT_MINUS, false);
    m.key(OCT_PLUS, false);
    run(&mut m, 200);
    let reset = shot(&m);
    let mut fresh = stock().unwrap();
    run(&mut fresh, 3000);
    fresh.key(OCT_MINUS, true);
    fresh.key(OCT_PLUS, true);
    run(&mut fresh, 150);
    fresh.key(OCT_MINUS, false);
    fresh.key(OCT_PLUS, false);
    run(&mut fresh, 200);
    assert!(differs(&changed, &reset) > 20, "the reset showed no change from a transposed, raised state");
    assert_eq!(differs(&reset, &shot(&fresh)), 0, "after the reset the machine looks like a fresh one that was reset");
}

/// Renders `ms` of emulated time and returns the left channel.
fn capture(m: &mut Machine, ms: usize) -> Vec<i16> {
    let mut all = Vec::new();
    let mut out = vec![0i16; 2 * 441];
    for _ in 0..ms / 10 {
        m.render(&mut out, 4_000_000);
        all.extend(out.iter().step_by(2));
    }
    all
}

fn send(m: &mut Machine, bytes: &[u8]) {
    for &b in bytes {
        m.midi_in(b);
    }
}

fn mean_diff(a: &[i16], b: &[i16]) -> f64 {
    let n = a.len().min(b.len()).max(1);
    a.iter().zip(b).map(|(x, y)| (f64::from(*x) - f64::from(*y)).abs()).sum::<f64>() / n as f64
}

/// Program Change 0 to 127 is voices 001 to 128 (MIDI table): every value draws a screen unlike the one before it, going up and coming back down, and the same value
/// twice draws the same screen (the emulator is deterministic, so a name that did not change is a program that did not take).
#[test]
fn program_change_reaches_all_128_voices() {
    let Some(mut m) = stock() else { return };
    run(&mut m, 3000);
    // the machine starts on voice 001, so program 0 shows no change from boot: begin from 1 and fill 0 in afterwards
    send(&mut m, &[0xC0, 1]);
    run(&mut m, 250);
    let mut last = shot(&m);
    let mut seen = vec![Vec::new(), last.clone()];
    for p in 2..128u8 {
        send(&mut m, &[0xC0, p]);
        run(&mut m, 250);
        let s = shot(&m);
        assert!(differs(&last, &s) > 20, "program {p} looks like program {}", p - 1);
        seen.push(s.clone());
        last = s;
    }
    send(&mut m, &[0xC0, 0]);
    run(&mut m, 250);
    assert!(differs(&last, &shot(&m)) > 20, "program 0 looks like program 127");
    for p in [127u8, 64, 1] {
        send(&mut m, &[0xC0, p]);
        run(&mut m, 250);
        assert_eq!(differs(&seen[p as usize], &shot(&m)), 0, "program {p} drew a different screen the second time");
    }
}

/// CC 1 is the filter type (MIDI table: on the effect channel, default 2, not the mod wheel). **Found in the emulator: with the note channel at its default, ALL (omni),
/// CC 1 does the same on channel 1 as on channel 2, to the sample**: the table's warning about the mod wheel holds only when a note channel is set apart from the
/// effect channel (not tried: that setting is on a GLO page). This pins what was seen.
#[test]
fn cc1_is_the_filter_type_on_every_channel_with_omni_notes() {
    if stock().is_none() {
        return;
    }
    let played = |msg: &[u8]| {
        let mut m = stock().unwrap();
        run(&mut m, 3000);
        send(&mut m, msg);
        run(&mut m, 100);
        send(&mut m, &[0x90, 60, 100]);
        let held = capture(&mut m, 800);
        send(&mut m, &[0x80, 60, 0]);
        let mut all = held;
        all.extend(capture(&mut m, 600));
        all
    };
    let none = played(&[]);
    let fx = played(&[0xB1, 1, 2]);
    let note_ch = played(&[0xB0, 1, 2]);
    assert!(none.iter().any(|s| s.abs() > 100), "the voice made no sound");
    assert!(mean_diff(&none, &fx) > 5.0, "CC 1 on channel 2 did not change the sound");
    assert_eq!(fx, note_ch, "with omni notes CC 1 on channels 1 and 2 sounded different");
}

/// Continue (FB) is the same as Start (FA) (MIDI table: no resume). Neither is a no-op against a run with no real-time byte... that is not asserted (nothing is
/// playing to start); what is: FA and FB leave the machine in exactly the same state, and Stop after either does too.
#[test]
fn continue_acts_as_start() {
    if stock().is_none() {
        return;
    }
    let run_with = |rt: &[u8]| {
        let mut m = stock().unwrap();
        run(&mut m, 3000);
        send(&mut m, rt);
        let a = capture(&mut m, 500);
        send(&mut m, &[0xFC]);
        let b = capture(&mut m, 300);
        (a, b, shot(&m))
    };
    let start = run_with(&[0xF8, 0xFA, 0xF8, 0xF8]);
    let cont = run_with(&[0xF8, 0xFB, 0xF8, 0xF8]);
    assert_eq!(start, cont, "Continue did not act as Start");
}

/// A DX7 32-voice bulk dump (F0 43 00 09 20 00, 4096 bytes, checksum, F7) is accepted and the unit asks which bank to write it to: the screen changes
/// from the home screen and the machine stays alive. **The jack must be paced** (16 bytes, then 10 ms, a little over 31250 baud): the whole dump sent at once drew
/// nothing, because the jack's buffer overflows (the real wire is that slow too).
#[test]
fn a_dx7_bank_dump_asks_for_a_bank() {
    let Some(mut m) = stock() else { return };
    run(&mut m, 3000);
    let home = shot(&m);
    let mut data = vec![0u8; 4096];
    for (i, b) in data.iter_mut().enumerate() {
        *b = (i * 7 % 100) as u8;
    }
    let sum = (!data.iter().fold(0u32, |a, b| a + u32::from(*b)) as u8).wrapping_add(1) & 0x7F;
    let mut msg = vec![0xF0, 0x43, 0x00, 0x09, 0x20, 0x00];
    msg.extend(&data);
    msg.push(sum);
    msg.push(0xF7);
    for chunk in msg.chunks(16) {
        send(&mut m, chunk);
        run(&mut m, 10);
    }
    run(&mut m, 2000);
    assert!(!m.halted());
    let asked = shot(&m);

    assert!(differs(&home, &asked) > 200, "a bank dump drew no question");
}

/// The flash the package leaves in the machine has the layout m-bootloader's specification gives for M-VAVE's stock images (its section 5.1 and 5.2: the cipher and the
/// CRC16 with their test vectors, a 32-byte header and 32-byte top-level records, each decoded on its own). It holds for 015 and 016 alike, so it checks the
/// layout and not a version: the header's CRC 0xDCA1 and burner field 0x0220, the filesystem size 0xFF000, and the four records in their places with good CRCs.
#[test]
fn the_flash_layout_is_the_one_the_bootloader_spec_describes() {
    let Some(m) = stock() else { return };
    // ENC: a 16-bit state from the key, XORed into each byte, one left shift (with 0x1021 on a carry) per byte; restarted for each object
    fn enc(data: &[u8]) -> Vec<u8> {
        let mut st = 0xFFFFu16;
        data.iter()
            .map(|&b| {
                let o = b ^ (st & 0xFF) as u8;
                st = (st << 1) ^ if st & 0x8000 != 0 { 0x1021 } else { 0 };
                o
            })
            .collect()
    }
    fn crc16(data: &[u8]) -> u16 {
        data.iter().fold(0u16, |mut c, &b| {
            c ^= u16::from(b) << 8;
            for _ in 0..8 {
                c = (c << 1) ^ if c & 0x8000 != 0 { 0x1021 } else { 0 };
            }
            c
        })
    }
    // the specification's own test vectors, so a wrong helper cannot pass the layout by agreeing with itself
    assert_eq!(crc16(b"123456789"), 0x31C3);
    let vec32: Vec<u8> = enc(&[0u8; 32]);
    assert_eq!(&vec32[..8], &[0xff, 0xdf, 0x9f, 0x1f, 0x1f, 0x3e, 0x7c, 0xf8]);
    assert_eq!(&vec32[24..], &[0x9c, 0x19, 0x13, 0x26, 0x4c, 0x98, 0x30, 0x60]);

    let f = m.flash_image();
    assert!(m.identity().starts_with("FM-1_"), "the package's identity: {:?}", m.identity());
    let h = enc(&f[..32]);
    let u16at = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32at = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    assert_eq!(u16at(&h, 0), crc16(&h[2..32]), "header CRC");
    assert_eq!(u16at(&h, 0), 0xDCA1, "the stock images' header CRC");
    assert_eq!((u16at(&h, 2), u32at(&h, 8), h[12], h[13]), (0x0220, 0xFF000, 0x10, 16), "burner field, filesystem size, version, alignment");
    let rec = |at: usize| enc(&f[at..at + 32]);
    let name = |r: &[u8]| String::from_utf8_lossy(r[16..32].split(|&b| b == 0).next().unwrap()).into_owned();
    let mut seen = Vec::new();
    for at in [0x20usize, 0x40, 0x60, 0x80] {
        let r = rec(at);
        assert_eq!(u16at(&r, 0), crc16(&r[2..32]), "record at {at:#x}: CRC");
        seen.push((name(&r), u32at(&r, 4), u32at(&r, 8), r[12]));
    }
    let boot_len = seen[0].2;
    assert_eq!(seen[0], ("uboot.boot".to_string(), 0xA0, boot_len, 0x00));
    assert!((0x1000..=0x3FA0).contains(&boot_len), "the boot container fits the boot area: {boot_len:#x}");
    assert_eq!((seen[1].0.as_str(), seen[1].1, seen[1].3), ("isd_config.ini", 0xA0 + boot_len, 0x02), "the configuration follows the boot container");
    assert_eq!(seen[2], ("app_dir_head".to_string(), 0x4000, 0xFFFF_FFFF, 0x81));
    assert_eq!(seen[3], ("key_mac".to_string(), 0xFF000, 0x1000, 0x12));
}
