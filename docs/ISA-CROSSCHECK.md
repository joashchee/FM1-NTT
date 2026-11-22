# Cross-check: `fm1-isa`'s decoder against `pi32v2-isa`'s table (2026-11-22)

**Why:** the core runs no firmware of its own making yet, and `fm1-isa` (vendored, MIT; its table was recovered from firmwares) is the only instruction-set source the core has.
`pi32v2-isa` (Julius Vitkauskas; code MIT-0, table CC0-1.0; https://github.com/jvitkauskas/pi32v2-isa) is an independent table, checked against JieLi's own objdump and, for 1085 of 1171 forms,
on a real AC791N (its README). Agreement is evidence for both; a difference is a lead, not a verdict. **Nothing here ran on a unit.**

**How (`scripts/isa-crosscheck/run.sh`, tried 2026-11-22 end to end, 3 min 53 s on the Mac):** one list of 376,832 six-byte windows goes through both decoders.
Every 16-bit first word 0x0000-0xFFFF appears; those below 0xE000 (16-bit forms only, per `pi32v2-isa`) with two windows each, those from 0xE000 (the 32- and 48-bit forms) with 32 each (two fixed and thirty
random second and third words, a fixed seed). `pi32v2-isa` runs in a `python:3.12-slim` container with no network (it needs Python 3.11); `fm1-isa` runs as a standalone crate (`scripts/isa-crosscheck`,
its own workspace, not a dependency of the core). Only `pi32v2-isa`'s table and its README were read; **no GPL project's source was.** "Decodes" for `pi32v2-isa` means the first table entry (in table order) whose
fixed bits match, which is not the same as JieLi's objdump showing it; its `-mcpu=r3` flag is used to split off forms that need `instrev3` or another CPU.

## Result

| | windows |
| --- | ---: |
| decoded by both | 225,141 |
| decoded by neither | 121,022 |
| only `pi32v2-isa` | 26,052 (and 435 more that need `instrev3` or another CPU) |
| only `fm1-isa` | 4,182 |

- **Lengths agree on every window both decode: 0 mismatches in 225,141.** This is the strong result: the two tables split the 16-, 32- and 48-bit forms the same way.
- **The operation agrees on 223,343 of them (99.2%)**, comparing the text with registers, immediates and register lists blanked (my own normaliser: a coarse check, not an exact one).
- **The 1,798 that differ, in 48 groups, are all one of:**
  - **`iff` float compares (about 1,500).** The same bytes get a different condition: `pi32v2-isa` has ten (`==`, `u!=`, `<`, `u<`, `<=`, `u<=`, `>=`, `u>=`, `>`, `u>`) where `fm1-isa`'s *text* has five and drops bit 7 of the first byte (it prints `iff (R == R)` for both `0x00e8..` and `0x80e8..`).
    **The executor is not affected:** `decode_cmp_rr_branch` (`core/vendor/fm1-emu/crates/fm1-core/src/lib.rs`, around line 4168) reads that bit as the "alt" (unordered negation) flag and has all ten conditions (`Cond::FEq, FUne, FLt, ...`), and its pairing agrees with `pi32v2-isa`'s names (FUge's alt is `<`, FGe's is `u<`, and so on). **So this is a disassembly-label difference in the vendored table, not a core bug.**
    I did not run the executor on these windows.
  - **`d[R] (pair)` loads and stores (about 100):** `fm1-isa` has one coarse text for all the 64-bit pair forms; `pi32v2-isa` names each addressing mode. Same length.
  - **The 48-bit `.l` forms whose condition `fm1-isa` prints as `??`** (about 70): `pi32v2-isa` names them (`ifs (r15 > r11)`). Its GAPS file says the `??` forms reset the chip.
  - **Two stragglers:** one `ld.w.sp.l` and one `sum.nacc.s` text.
- **Only `pi32v2-isa` (26,487 windows, 626 of its table's ids):** mostly the DSP, saturating, parallel and two-memory-operand families (`madd`, `msub`, `addsub`, `mul4`, `mul2`, `mul`, `sum`, `unpack4`, `iff` blocks, `ld`/`st`
  variants, `xchg`, `popcnt`...). The core's stock firmware boots without them, so most may be unused; **not checked** which ones the firmwares we care about use (`pi32v2-isa`'s README says one factory firmware uses 562 forms and another 598).
- **Only `fm1-isa` (4,182 windows):** 16-bit `R = #i` on 521 first words (0x0045 to 0x15ff), the `iff ... goto/{` forms on a few dozen first words each, and 16 `d[R] (pair)` first words (0xec50 to 0xec5f). The last agrees with `pi32v2-isa`'s own gap list ("words no objdump decodes that
  the chip executes: the twelve `d[...]` pair loads and stores run with second-word bit 12 set"). The rest is **not investigated**: either `fm1-isa` is looser (it accepts words JieLi's objdump rejects) or `pi32v2-isa` has holes.

## What it does not show

- Second and third words are sampled (30 per first word from 0xE000), not swept; a form that needs a rare second word can be missed on either side.
- It compares what each decoder *calls* an instruction, not what the core *does* with it; no semantics were compared.
- `pi32v2-isa` is one person's work, and young (its repositories are days old); its table is confirmed against JieLi's toolchain, which is the only oracle it has for most forms.
- Only the AC791N's `-mcpu=r3` (the FM-1) was considered.

## Using the toolchain

The same author's LLVM fork (pi32v2-llvm, Apache-2.0 with LLVM exceptions) built a float function whose 12 instructions `fm1-isa` decoded alike (`docs/HARDWARE.md`). Downloaded binaries stay out of the repo and run only in a container.

## Which of the `pi32v2-isa`-only forms do the firmwares contain? (2026-11-22)

**A static sweep, not an execution trace** (`scripts/isa-crosscheck/src/bin/sweep.rs`, `sweep_side.py`; the vendored core only traces by printing every instruction). For each of 15 firmwares (the stock 016, the 5 bundled, the Pack's 9) the code image
is walked linearly from `0x120` with `fm1-isa`; a failure counts as "apparent code" only if it follows at least 32 good decodes in a row (about two random words in three decode, so data rarely does that). Those windows go to `pi32v2-isa`'s table.

- **4,254 failures in apparent code across the 15 images; `pi32v2-isa` decodes 156 of them, as 48 forms, all shown by JieLi's objdump at `-mcpu=r3`.** The other 4,098 are neither decoder's instructions: data, or words neither table knows.
- **The stock 016 has 67 of the 156**, among them `madd.f.rrr` 35 times (the floating-point multiply-add; `pi32v2-isa` notes that 016 uses it far more than JieLi's compiler ever emits it), `copex.ppi.p`, `mov.r_sr`, `ld.bs.sp.l`, `rev8.rr`. The others carry 1 to 24 each (FRASIL 24, Skiff 15, TRIO 10, Felucca 10); `copex.ppi.p` is in 10 firmwares, `div.u.ppr` (a 64-bit divide) in 2.
- **`fm1-isa` has no `madd` form at all**, and a decode failure halts the machine (`CoreError::Decode`; `Machine::run_unpaced` sets `halted`). **None was reached, though:** the stock firmware was driven through all 41 buttons and keys, all 7 encoders, notes on two channels and every effect CC 0 to 23 at three values, and never halted; the 14 others ran 6 s each without halting. So the 156 are either code nothing exercised, or data that looks like code.
- **What it does not show:** which of them are real code, which are executed on a path nobody drove (the effects in particular, which are mostly silent in the emulator: `docs/HARDWARE.md`), and what their semantics are. It is an upper bound, not a count of used instructions.
