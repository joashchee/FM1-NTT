# Licensing and clean-room policy

FM1-NTT is **MIT** (LICENSE). That only holds if nothing GPL goes in. The FM-1 ecosystem is mixed, so, as of 2026-11-16 (licences as
the `awesome-fm-1` list and each project's page state them; **check the repository before relying on one**):

| Project | Licence | What we may do |
|---|---|---|
| `joakimeriksson/fm1-emu` (Rust emulator and plugin) | MIT | **Vendored** (2026-11-16): `fm1-isa`, `fm1-core`, `fm1-soc` in `core/vendor/fm1-emu/` at a pinned commit (`VENDORED.md`, `NOTICE`). Its sibling `fm1-sid`, `fm1-dock` and Felucca-derived firmware are GPL-3.0: not covered by that, and not vendored |
| `google/music-synthesizer-for-android` (msfa, a DX7-class engine) | Apache-2.0 | **Vendored** (2026-11-17) in `core/vendor/msfa/` at a pinned commit as a reference, and **ported to Rust** in `core/src/fm6.rs`; compatible with MIT (it keeps its notices; `VENDORED.md`, `NOTICE`). Take it from Google's repository, **never from Felucca's tree** (there it sits beside GPL-3.0 files) and not from Dexed (GPL-3.0) |
| `simonjohansson/fm1-emulator` | GPL-3.0 | **Black box only:** run it, compare behaviour, never read or copy its source |
| `shcherbakov7/m-wave-fm1-browser-emulator` | GPL-3.0 | Black box only |
| `spinkham/felucca-try`, Felucca, other GPL firmware | GPL-3.0 | They are *firmware* run by the user, never part of this repo |
| M-VAVE's FM1 Open (application layer, SDK, drivers) | GPLv3 | Black box only; its engine is a closed binary and is never ours |
| JieLi SDKs and toolchain | vendor terms | Not redistributed; not read unless the licence allows it |
| `kagaimiq/jielie` (ISA notes) | check | Facts about the ISA may inform our code; copy no text or code without its licence |

## Rules

1. **Facts are free, expression is not.** An instruction's encoding, a register's address or a file format is a fact we may learn from
   anywhere and write down in our own words and code. Another project's source, comments and tables are not ours to copy unless it is
   MIT-compatible and we say so.
2. **A contributor who has read GPL emulator source says so and stays off that part** (the clean-room line).
3. **Every vendored file keeps its licence text and has a line in `NOTICE`:** source URL, commit, licence, what changed.
4. **Firmware ships in two tiers** (`bundled-firmware/` permissive, inside the MIT app; GPL and LGPL in **their own repository, FM1-NTT Firmware Pack**, with their source and their own package, never in this repository, the MIT app's package or a Mac App Store build): listed in a manifest with a hash, source and an AND-only allowlisted licence expression, unmodified, with its licence texts. The MIT licence covers FM1-NTT's own code only: a bundled firmware keeps its licence and its obligations. No AGPL, non-commercial, closed or unlicensed firmware, no ROM, SDK or other vendor binary, and nothing that embeds third-party data without a grant. A firmware's licence is read from its own files, not the repository's badge. The emulator links no firmware.
5. **Downloaded code is run only with the maintainer's OK each time** (an emulator to compare against, a toolchain): into its own empty
   folder, outside the repository.
6. **Trademarks:** "FM-1" and "M-VAVE" name what this works with; the project says it is not affiliated with M-VAVE.
