# FM1-NTT

A cross-platform sandbox, playground and emulator for **FM-1 firmware**: run the M-VAVE FM-1 pocket FM synth's community
firmwares (Felucca, SLOOP, Melodee, S1Vave and the rest) on a computer, in a browser, or on an **R36S** handheld running
[r3i6nos](https://github.com/joashchee/r3i6nos), to audition them before flashing a real FM-1. The FM-1's own
240 x 240 screen is drawn pixel for pixel at the top left, with sandbox panels around it; **Fn** zooms it to 2x.

> **Early days.** The CPU, memory map and peripherals are the vendored MIT [`fm1-emu`](https://github.com/joakimeriksson/fm1-emu) crates;
> the C and SDL2 host, the pixel layouts, the C API and the web shell are in and tested, but **nothing has run on a real R36S** (`ROADMAP.md`). Five permissively licensed firmwares
> ship with it (Haunted FM-1 v0.1: MIT, on MPL-2.0 and Apache-2.0 parts), and a **separate GPL firmware pack** (Felucca, FoMni-1, Jangada, with their source) is its own
> repository and package ([FM1-NTT Firmware Pack](https://github.com/joashchee/FM1-NTT-Firmware-Pack)); all run in the emulator. Closed and unclear ones are bring-your-own (`docs/FIRMWARE.md`, `docs/FIRMWARE-CATALOG.md`).

## Layout

| Folder | What |
|---|---|
| `core/` | Rust: the machine and a C API (`core/fm1ntt.h`) over the vendored `fm1-emu` crates (`core/vendor/`, MIT) |
| `host/` | C and SDL2 with its own pixel graphics: the desktop and R36S program |
| `web/` | The installable PWA shell (no service worker) |
| `bundled-firmware/` | The permissively licensed firmware that ships in the MIT app (Haunted FM-1 v0.1), with manifest, licences and sources |
| *(not here)* | The GPL firmware pack (Felucca, FoMni-1, Jangada) with each one's source is its **own repository, [FM1-NTT Firmware Pack](https://github.com/joashchee/FM1-NTT-Firmware-Pack)** and package: never in this repository, the MIT app or a Mac App Store build |
| `r36s/` | The R36S package (`.deb`, built in Docker) |
| `scripts/` | `check.sh`, `check-bundled.py`, `check-firmware-updates.py`, `bench-compare.py`, `build-icons.ts` |
| `docs/` | The reference kit, the architecture, licensing and firmware policy |

## Build and try

```sh
scripts/check.sh              # core tests, host build + self-test, web test, shellcheck
host/build.sh                 # build/fm1ntt (needs Rust and SDL2)
build/fm1ntt FIRMWARE.fwsc    # a package; for a raw image: --entry 0x02000120 FILE. Or put files in the FILES panel's folder
                              # (/roms/FM1-NTT/firmware on the R36S, else ~/FM1-NTT/firmware; FM1NTT_DIR overrides)
build/fm1ntt --screenshot out.bmp [--zoomed]   # one frame to a file
build/fm1ntt --report [FIRMWARE]               # the Device report (r3i6nos rule 9) -> $BASE/r36s-report.txt
build/fm1ntt --bench [--seconds N] [--fw FILE] # the Benchmark report -> $BASE/bench/ ; compare files with scripts/bench-compare.py
build/fm1ntt --rec-demo [--features a,b] [--no-video] [FIRMWARE]   # Rec demo -> $BASE/Demo/ (PNGs, an mp4 with ffmpeg, the log)
node web/server.ts            # the web shell on http://localhost:8080
r36s/build-deb.sh             # build/pkg/FM1-NTT.deb for the R36S (Docker)
```

## Panels and controls

**L1 / R1** (Home / End) turn the panel carousel: MACHINE, FILES, CPU, MEMORY, CONSOLE, MIDI, SOUND, TOOLS. **Fn** (Tab / F1) toggles Standard and
Zoomed (2x). **Start** (P) pauses. On a keyboard: Z S X D C V ... and Q 2 W 3 E R ... play the 27 keys, F2-F12 and [ ] are the buttons, arrows
and PgUp/PgDn turn encoders on MACHINE and move around the other panels, Return is A, Backspace is B, Insert and Delete are X and Y, Esc leaves.
`docs/ARCHITECTURE.md` section 4 says what each panel does.

## Hero features (the Rec demo's checklist)

1. **carousel**: the eight panels on L1 and R1. 2. **zoom**: Standard and Zoomed with Fn. 3. **midi**: send notes to the MIDI jack.
4. **tools**: pause, reset, flash slots. 5. **firmware**: boot a firmware and play it (a bundled one unless you give yours).

## Docs

[`docs/fm-1_reference-kit.md`](docs/fm-1_reference-kit.md) (the FM-1, its firmwares, emulators, R36S and buying notes) ·
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) · [`docs/LICENSING.md`](docs/LICENSING.md) · [`docs/FIRMWARE.md`](docs/FIRMWARE.md) · [`docs/FIRMWARE-CATALOG.md`](docs/FIRMWARE-CATALOG.md) ·
[`docs/HARDWARE.md`](docs/HARDWARE.md) · [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) · [`docs/SCREEN-MIRROR.md`](docs/SCREEN-MIRROR.md) · [`ROADMAP.md`](ROADMAP.md)

## Licence

MIT (`LICENSE`); the vendored `fm1-emu` is MIT too (`NOTICE`). Not affiliated with M-VAVE.
