# Vendored: fm1-emu (Joakim Eriksson)

- **Source:** https://github.com/joakimeriksson/fm1-emu
- **Commit:** `60645a58d6cd1597ed565a3f00e4981964623232` (2026-11-15, "Merge pull request #190 from joakimeriksson/plugin-version"), version 0.5.1
- **Licence:** MIT, "Copyright (c) 2026 Joakim Eriksson" (`LICENSE`, copied unchanged)
- **What is here:** `crates/fm1-isa` (the generated pi32v2 decoder), `crates/fm1-core` (the CPU interpreter and its JITs), `crates/fm1-soc`
  (the memory map and peripherals: LCD, flash, audio, USB, the `.fwsc` package reader), and `docs/isa-notes.md`. All three crates have no
  dependencies of their own.
- **What is not:** `fm1-gdbstub`, `fm1-emu` (the CLI), `fm1-live` (the window, `minifb`/`cpal`/`midir`), `fm1-web`, `fm1-plugin`, the `isa/`
  tables and tools, and every firmware (upstream does not host them either).
- **Changed:** the workspace `Cargo.toml`'s `members` (three crates), so Cargo resolves `version.workspace`; and, **2026-11-17**, two files of
  `crates/fm1-core/src`, the AArch64 block compiler, **by this project** (not upstream's; an upstream pull request is still to be made, and until it
  is merged an update of this tree must carry these two files over or drop them):
  - `jit_a64.rs`: three encoders (`eor_mask`, `cmp_imm12`, `cmp_imm12_x`), each checked against clang's assembler.
  - `jit.rs` (AArch64 `Gen` only; the x86-64 and wasm compilers are untouched): the register ops (`MovI`, `Mov`, `AddRI`, `ShlI`, `SarI`,
    `Alu3`, `Alu3I`, `Alu2`, `Alu2I`, `A4`, `Ext`, `MinMax`) work on the host register that holds the guest one (`src`, `dst`, `rw`, `alu3`),
    where they copied through `w0` / `w1`; compares and branches (`BrI`, `BrIEq`, `BrINe`, `BrR`, `BitBr`, `IfBlk`) use immediates where they
    fit (`cmp_v`, `rhs_reg`); the loads and stores take their address from the operand register, and their RAM / XIP range check is `in_range`
    (an `eor` of the base and an immediate length, 3-4 instructions where the general form takes 6, and it falls back to the general form for any
    layout the shortcut does not hold for). One test is added (`loads_stores_and_compares_match_the_interpreter_at_the_edges_of_ram_and_xip`,
    AArch64 only): every width, signedness and a ring of addresses at and around both regions' edges and far outside, and compares at the
    immediates' encoding limits, each against the interpreter; broken on purpose three ways, it fails each time.
  - What it does: 14-18% fewer host instructions per guest instruction on six firmwares (an Apple M1; `docs/PERFORMANCE.md`), the same state
    after every instruction (the vendored tests, 72 with the new one, and a 6 s audio, screen, pc and instruction-count comparison of the JIT
    against the interpreter on six firmwares, are identical).
- **Changed, 2026-11-17 (by this project, a read-only addition for diagnostics):** `crates/fm1-core/src/jit.rs` gets `Jit::counts()` (blocks still cold, compiled,
  and given up on, and the code cache's bytes used) and `crates/fm1-core/src/lib.rs` gets `Cpu::jit_counts()` over it (zeros when there is no native JIT).
  Nothing executes differently; it is read by the benchmark (`fm1ntt_jit_report`) to say where the unit's time goes (blocks the JIT gave up on run in the interpreter).
- **Changed, 2026-11-17, second part (by this project; AArch64 `Gen` in `jit.rs` and `jit_a64.rs` only, the x86-64 and wasm compilers untouched):**
  - **The budget counts down** (`DONE`, x26, now holds the room left of the call's limit; `Gen::n`, `done_at`, `give_back`): a block takes its whole count off at its entry
    (`subs` and one branch, where it was an add, a compare and a branch) and a chain adds nothing, so the per-block bookkeeping is 2 instructions less; exits and the helpers'
    tick indexes give back what the block did not run (`LIMIT - DONE`). The stop checks (`cbz DONE`, `STRETCH_ROOM`) are the same tests on the new meaning.
  - **Float ops on the held host registers** (`FOp`: no copy through w0 / w1), **immediate forms** for and / or / xor (a bitmask immediate: `jit_a64::logical_imm`, with two tests: 24 encodings
    from clang's assembler, and every encodable 32-bit pattern round-tripped against a decoder), add / sub (12 bits), and bit-test branches (`tst` with an immediate).
  - **Cold code out of line**: the entry from Rust is one branch to the prologue, which sits after the block; each chain exit's slow half (`back`: pc, slot, to Rust) is emitted after the
    block's hot code (`Gen::cold`, `emit_cold`), so the hot path is dense in the instruction cache. Not measured on the unit.
  - What it does: 5-7% fewer host instructions per guest instruction on seven firmwares (an Apple M1), the same audio, final pc and instruction count as the interpreter after 6 s on nine
    firmwares at pace 0 and 3, and the vendored tests (74 with the two new ones) pass in debug and release.
- **Updating:** fetch the new commit into an empty folder outside the repository, diff it against this tree, read the diff, copy, and change
  the commit above, `NOTICE`, `docs/LICENSING.md` and the changelog in the same commit. Run `scripts/check.sh`.
- **Not ours to fix here:** a bug in these crates is reported or fixed upstream; a local fix is a recorded change above.
