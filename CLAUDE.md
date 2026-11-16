# FM1-NTT

A cross-platform sandbox, playground and emulator for FM-1 firmware (the M-VAVE FM-1 pocket FM synth), R36S first through r3i6nos,
then macOS, Windows, Linux and the web. **Public and MIT.** The research is `docs/fm-1_reference-kit.md`; what is known of the machine
is `docs/HARDWARE.md`; the decisions are `docs/ARCHITECTURE.md`; the licence line is `docs/LICENSING.md`.

**`CHANGELOG.md`**: add a bullet whenever a change lands.
**`ROADMAP.md`**: tick items in the same commit as the work, and say what is blocked and on what.

## Layout

- `core/`: the Rust library (`machine`, `firmware`, `devices`, `capi`) and its C header `fm1ntt.h`, over the vendored `core/vendor/fm1-emu` (MIT; `VENDORED.md`).
- `host/`: `fm1ntt.c` (SDL window, audio, input) and `pix.h` (the pixel canvas, the font, the Standard and Zoomed layouts); `host/build.sh` builds it here.
- `web/`: the PWA shell, its Node server and `pwa.test.ts`; `scripts/build-icons.ts` draws the icons.
- `r36s/`: `build-deb.sh` (Docker) and `package/`.
- `host/report.h` (Device report, Benchmark), `host/demo.h` (Rec demo), `host/vendor/r3i6nos_report.h` (MIT, from r3i6nos; `NOTICE`).
- `scripts/check.sh`: everything CI runs (and the Pack repository's own check when it is beside this one); `scripts/bench-compare.py` compares benchmark files; `scripts/check-firmware-updates.py` (with `firmware-watchlist.tsv`) asks each curated firmware's release feed for news and licence changes, and watches what was left out (Shrub, SLOOP, Melodee, X0X, Quod).
- **`../FM1-NTT-Firmware-Pack/`** (a separate repository; its folder is inside this working tree, ignored, until moved out): the GPL pack. Its `scripts/` hold copies of the two firmware checkers: change them here first, then copy.
- `docs/BENCHMARKS.md`, `docs/SCREEN-MIRROR.md` (the plan for mirroring a pixel screen).

## Rules

1. **Say what was not run.** Nothing has run on a real R36S or a real FM-1, and the CPU runs no firmware yet; a claim about either is
   "untested" until a device report or the maintainer says otherwise (r3i6nos rule 1).
2. **Firmware ships in two tiers, and only what a manifest lists** (the maintainer's instructions, 2026-11-16, and what I recommended when asked
   whether MIT allows bundling GPL: yes as separate works, with obligations; `docs/FIRMWARE.md`, `docs/FIRMWARE-CATALOG.md`).
   **`bundled-firmware/` is the permissive tier** and ships inside the MIT app and its package: identifiers from MIT, BSD, Apache-2.0, ISC, Unlicense,
   CC0-1.0, Zlib, plus MPL-2.0 and OFL-1.1 as components; never GPL, AGPL or LGPL. **The copyleft tier is its own repository, FM1-NTT Firmware Pack** (`https://github.com/joashchee/FM1-NTT-Firmware-Pack`; its folder sits beside this one, `../FM1-NTT-Firmware-Pack`): GPL-2.0/3.0 and LGPL-2.1/3.0
   firmware with its own package (`./build-deb.sh` there), each entry with its **complete corresponding source**, a
   written offer (its issues page is the contact) and the licence texts; **this repository must hold none of it** (`check-bundled.py` fails if a `firmware-pack/` or a pack manifest appears here); **it is never in the MIT app's package and never in a Mac App Store build** (`scripts/check-bundled.py --no-pack DIR`
   must pass on every app bundle before it is signed). Both tiers: each file is byte for byte the author's release, with SHA-256, source, built-from
   commit, an **AND-only** licence expression from the allowlist, the licence texts, `SOURCES.md` and `NOTICE`; `scripts/check-bundled.py` enforces it and
   fails on any other `*.fwsc` or `*.fm1save`. **Never bundle AGPL, non-commercial, closed or unlicensed firmware, or one that embeds third-party data whose
   rights are not granted** (Melodee: Casio and Sequential sound data), and **read the firmware's own licence files, not the repository's badge** (a permissive
   repository can ship a GPL firmware; a GPL one can embed reserved assets). The emulator reads a firmware as data and links none of it: keep it that way,
   or the MIT core is no longer MIT. No ROM, SDK or vendor binary other than these is in the repo, a CI artifact or a release.
3. **MIT only; GPL emulators are black boxes** (`docs/LICENSING.md`). Never read or copy GPL-3.0 source (`fm1-emulator`, the browser
   emulator, FM1 Open, Felucca). `fm1-emu` (MIT) is vendored in `core/vendor/fm1-emu` at a pinned commit: never edit a file there without
   recording it in `VENDORED.md` first; update it by diffing a fresh copy (fetched into an empty folder outside the repo) and reading the diff.
4. **The core's only dependencies are the vendored crates, it is deterministic, and the C API never unwinds** (`panic = "abort"`; every function tolerates null). No new dependency without asking.
5. **Firmware is untrusted input:** an `Error`, never a crash, a hang or unbounded memory. Bound every size, check every access.
6. **The window is pixel graphics, not r3i6nos's text theme** (the maintainer, 2026-11-16): the FM-1's 240 x 240 screen flush top left,
   pixel for pixel; **Tab or F1** toggles Standard (1x, panels right and below) and Zoomed (2x, one panel at the right).
   **Fn (raw button 16) turns the panel carousel** (MACHINE FILES CPU MEMORY CONSOLE MIDI SOUND TOOLS), **L1+R1 open the menu** (the maintainer, 2026-11-16);
   Start pauses off MACHINE; the FM-1's own controls (notes, sticks, L1/L2/R2 modifiers: README) are the pad only on MACHINE. Only the app's folder is written (`firmware/ slots/ recordings/ screenshots/`), temp name then rename.
   Text is the project's own 5 x 7 font; every text pair passes 4.5:1; nothing scrolls (a long list is whole pages). Select and Start never
   quit when the app has a Quit item (the maintainer's rule, 2026-11-10); until Files has one, B and Esc leave.
7. **The r3i6nos port rules that are not about the theme** (`../r3i6nos/CLAUDE.md` 9 to 14, rule 8 excepted), all in pixels and in this repo:
   a **Device report** (`fm1ntt --report [FIRMWARE]`, TOOLS > Device report -> `/roms/FM1-NTT/r36s-report.txt`, r3i6nos's grammar and shared
   writer vendored at `host/vendor/r3i6nos_report.h`, with `# NEEDS:` setup lines, progress files and the `== power ==` section), a **Rec demo**
   (`--rec-demo`, TOOLS > Rec demo -> `/roms/FM1-NTT/Demo/`: a checklist of hero features, the lock and status files in `/run/loveos/demo/`,
   Select+Start held to cancel, deterministic PNGs, WAV and mp4 opening on the splash with the version line, `rec-demo.txt`), a local build
   script (`r36s/build-deb.sh`, still to be in r3i6nos's `scripts/ports.conf`), the splash, and the power section. **Hot-plug toast** (`host/toast.h`) is built; not done: Rec demo polling it and stopping on a failure. **Keep the hero list in `README.md` and `HEROES` in `host/demo.h` in step**: a new hero feature gets a step in the tour that works
   alone (it starts from `rec_place`), a row in the checklist and a frame count in `DEMO_FRAMES_FEATURE` (the self-test fails when the table
   is out of date), and the two recordings must stay byte-identical. The `firmware` hero feature is shown
   with a real firmware (the one given, else the one loaded, else the bundled UNTRUE STEP); with none at all it is SKIPped and the log says so.
   **The Benchmark report** (`fm1ntt --bench`, TOOLS > Benchmark, `docs/BENCHMARKS.md`) is the cross-platform comparison: bump
   `WORKLOAD_VERSION` (core/src/bench.rs) whenever a workload changes and `format` when the grammar does, and keep `scripts/bench-compare.py`
   and its tests in step.
8. **Fast start:** the splash shows at once, before any heavy work; slow work runs off the UI thread; any load error shows on the splash.
9. **The web build is an installable PWA from the first commit** (`web/`): manifest, icons by `scripts/build-icons.ts`, no service worker,
   `manifest-src 'self'`, and a test that fetches the manifest and every icon. Say untested what was not tried on a phone (Add to Home Screen on iOS and Android).
10. **A free macOS app ships twice:** the Mac App Store and a signed, notarized direct download (**the App Store build has no copyleft firmware: rule 2**), published as ANSI Apps, bundle id `com.ansiapps.fm1ntt`, signed and notarized by Scoreless Music (the only Apple account),
    built on the maintainer's Mac, never on CI (the global rules). Never change a shipped bundle id.
11. **The app never mentions its development docs** in anything a user sees (UI text, errors, empty states): say what is needed in the UI.
12. **CI is Linux and GCC:** run changed C and scripts in Debian 12 under Docker (`r36s/build-deb.sh`) before saying done on the R36S side.

## Check before committing

`scripts/check.sh` (core tests and clippy, host build and self-test, web test, shellcheck), then `r36s/build-deb.sh` after packaging changes.
Report tools that were not installed rather than calling the run clean.
