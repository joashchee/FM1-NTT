//! A workload for benchmarks that needs no firmware: a loop of eight `nop`s closed by a `goto`, hand-assembled here (original, ours).
//! It runs the interpreter (or the native block compiler) flat out and never faults, so instructions per second and times real time
//! are comparable between platforms. It says how fast the *core* dispatches instructions, **not** how fast a real firmware runs
//! (a firmware's mix of loads, stores, multiplies and peripheral accesses is slower per instruction). Bump `WORKLOAD_VERSION` when the
//! bytes change, so reports of different versions are never compared.
//!
//! The encodings come from the vendored ISA table (`nop` is `0x0000`; the `goto -#i` form with mask 0xe00e and match 0x8006 is the loop
//! branch, 0x8076 found by running every candidate and keeping those that stay in the loop).

/// Version of the workload below; a benchmark report records it.
/// 2: the firmware lines run after a warm-up past the firmware's boot (version 1 timed its cheap first seconds).
pub const WORKLOAD_VERSION: u32 = 2;
/// Where a raw image is entered (Felucca's entry; the same as `fm1ntt_load_raw`'s FM1_ENTRY_FELUCCA).
pub const ENTRY: u32 = 0x0200_0120;

/// The image: the vector area zeroed, then the loop at the entry.
pub fn nop_loop_image() -> Vec<u8> {
    let mut img = vec![0u8; 0x400];
    // eight nops (0x0000 each) are already zeros; the goto closes the loop
    img[0x120 + 16] = 0x76;
    img[0x120 + 17] = 0x80;
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Machine;

    #[test]
    fn the_loop_never_faults_and_stays_in_the_image() {
        let mut m = Machine::from_raw(nop_loop_image(), ENTRY).unwrap();
        let mut out = vec![0i16; 64];
        for _ in 0..5 {
            m.render(&mut out, 100_000);
            let v = m.cpu_view(0).unwrap();
            assert!(!m.halted());
            assert!(v.pc >= ENTRY && v.pc < ENTRY + 20, "pc {:#x}", v.pc);
        }
        assert!(m.cpu_view(0).unwrap().insns >= 500_000);
    }

    #[test]
    fn the_same_loop_gives_the_same_state() {
        let run = || {
            let mut m = Machine::from_raw(nop_loop_image(), ENTRY).unwrap();
            let mut out = vec![0i16; 64];
            m.render(&mut out, 123_456);
            m.cpu_view(0).unwrap()
        };
        assert_eq!(run(), run());
    }
}
