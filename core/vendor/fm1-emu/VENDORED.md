# Vendored: fm1-emu (Joakim Eriksson)

- **Source:** https://github.com/joakimeriksson/fm1-emu
- **Commit:** `60645a58d6cd1597ed565a3f00e4981964623232` (2026-11-15, "Merge pull request #190 from joakimeriksson/plugin-version"), version 0.5.1
- **Licence:** MIT, "Copyright (c) 2026 Joakim Eriksson" (`LICENSE`, copied unchanged)
- **What is here:** `crates/fm1-isa` (the generated pi32v2 decoder), `crates/fm1-core` (the CPU interpreter and its JITs), `crates/fm1-soc`
  (the memory map and peripherals: LCD, flash, audio, USB, the `.fwsc` package reader), and `docs/isa-notes.md`. All three crates have no
  dependencies of their own.
- **What is not:** `fm1-gdbstub`, `fm1-emu` (the CLI), `fm1-live` (the window, `minifb`/`cpal`/`midir`), `fm1-web`, `fm1-plugin`, the `isa/`
  tables and tools, and every firmware (upstream does not host them either).
- **Changed:** only the workspace `Cargo.toml`'s `members` (three crates), so Cargo resolves `version.workspace`; no source file is edited.
  A change to a vendored file is recorded here before it is made.
- **Updating:** fetch the new commit into an empty folder outside the repository, diff it against this tree, read the diff, copy, and change
  the commit above, `NOTICE`, `docs/LICENSING.md` and the changelog in the same commit. Run `scripts/check.sh`.
- **Not ours to fix here:** a bug in these crates is reported or fixed upstream; a local fix is a recorded change above.
