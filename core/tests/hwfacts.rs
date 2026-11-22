//! What the vendored core does with a few things the FM-1 hardware is reported to do (`docs/HARDWARE.md`, "What `pi32v2-isa` lists as open": the author's runs on one
//! FM-1, `hw/fm1-2026-11-15-excprobe`; **not run on a unit here**). One small program, assembled with `pi32v2-isa`'s assembler (MIT-0) from the source below, writes its
//! results into RAM and loops. The cases the core agrees with are plain tests; the three where it differs are `#[ignore]`d tests that assert the *hardware's* value, so
//! `cargo test --test hwfacts -- --ignored` lists the differences and each starts passing the day the core is fixed (a change to `core/vendor/` is recorded in
//! `VENDORED.md` first). Nothing is fixed here: the roadmap's rule for these is "list, do not fix".
//!
//! ```text
//!   r4 = 0x01C30000                      ; the results
//!   r0 = 100 ; r1 = 0
//!   r2 = r0 / r1 (u) ; [r4+0] = r2       ; integer divides by zero
//!   r2 = r0 / r1 (s) ; [r4+4] = r2
//!   r0 = 0x3F800000 ; r2 = r0 / r1 (f) ; [r4+8] = r2     ; 1.0 / 0.0
//!   r0 = 0 ; r2 = r0 / r1 (f) ; [r4+12] = r2             ; 0.0 / 0.0
//!   r5 = 0 ; r9 = 40 ; rep r9 { r5 += 1 } ; [r4+16] = r5 ; a rep of 40
//!   r6 = 0xAABBCCDD ; [r4+32] = r6 ; r1 = r4 + 33 ; r7 = [r1+0] ; [r4+20] = r7   ; a misaligned word load
//!   r8 = 0x600D ; [r4+24] = r8           ; reached
//!   1: goto 1b
//! ```

use fm1ntt::machine::{Machine, Region};

const CODE: &str = "c4ff0000c30158244120f4e10021c260f4e10121c26160e07e053fe50321c26240203fe50321c263452049e0280009035d81c564c6ffddccbbaac66801e121401760c76548e00d60d0ec4981f79f";

/// The results the program left in RAM, as seven words (0: u32 divide, 1: s32 divide, 2: float 1/0, 3: float 0/0, 4: rep count, 5: misaligned load, 6: marker).
fn run() -> [u32; 7] {
    let code: Vec<u8> = (0..CODE.len()).step_by(2).map(|i| u8::from_str_radix(&CODE[i..i + 2], 16).unwrap()).collect();
    let mut img = vec![0u8; 0x120];
    img.extend(&code);
    img.resize(4096, 0);
    let mut m = Machine::from_raw(img, 0x0200_0120).unwrap();
    let mut out = vec![0i16; 2 * 64];
    for _ in 0..20 {
        m.render(&mut out, 20_000);
    }
    assert!(!m.halted(), "the program halted the machine");
    let mut b = [0u8; 28];
    assert_eq!(m.read_mem(Region::Ram, 0x30000, &mut b), 28);
    let w = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    [w(0), w(4), w(8), w(12), w(16), w(20), w(24)]
}

#[test]
fn integer_divide_by_zero_gives_zero_and_float_one_over_zero_gives_infinity_as_the_hardware_does() {
    let r = run();
    assert_eq!(r[6], 0x600D, "the program ran to its end");
    assert_eq!((r[0], r[1]), (0, 0), "no trap with EMU_CON as found, integer results 0");
    assert_eq!(r[2], 0x7F80_0000, "1.0 / 0.0");
}

#[test]
#[ignore = "known difference: the core gives the usual quiet NaN 0x7fc00000"]
fn float_zero_over_zero_is_0x7f800001_on_the_hardware() {
    assert_eq!(run()[3], 0x7F80_0001);
}

#[test]
#[ignore = "known difference: the core runs all 40 passes of a rep"]
fn one_rep_runs_at_most_32_times_on_the_hardware() {
    assert_eq!(run()[4], 32);
}

#[test]
#[ignore = "known difference: the core reads the bytes at the misaligned address (0x00aabbcc) and does not trap"]
fn a_misaligned_word_load_moves_the_aligned_down_word_and_traps_on_the_hardware() {
    let r = run();
    assert_eq!(r[5], 0xAABB_CCDD, "the aligned-down word");
    assert_ne!(r[6], 0x600D, "the trap, 7 instructions late, stops the program before its end marker (the handler is not modelled here: this only shows the program did not run on)");
}
