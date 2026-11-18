# FM1-NTT

A cross-platform sandbox, playground and emulator for **FM-1 firmware**: run the M-VAVE FM-1 pocket FM synth's community
firmwares (Felucca, SLOOP, Melodee, S1Vave and the rest) on a computer, in a browser, or on an **R36S** handheld running
[r3i6nos](https://github.com/joashchee/r3i6nos), to audition them before flashing a real FM-1. The FM-1's own
240 x 240 screen is drawn pixel for pixel at the top left, with sandbox panels around it; **F1 or Tab** zooms it to 2x.

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

**Fn** (Home / End on a keyboard) turns the panel carousel: MACHINE, FILES, CPU, MEMORY, CONSOLE, MIDI, SOUND, MONITOR, TOOLS. **MONITOR** is a `top` for the machine the app runs on (CPU per core, memory, processes, this app's threads, battery; L2 R2 pages, X sorts, Y freezes; `docs/MONITOR.md`); it costs nothing while another panel is showing, and TOOLS > DATA RAIN adds an opt-in stream of the data passing through. **L1 + R1** together open the menu
(TOOLS) and close it again. **Tab / F1** toggles Standard and Zoomed (2x). **Start** (P) pauses on every panel but MACHINE.

**The SOUND preview** (L2, or Page Up on a keyboard, on the SOUND panel): the DX7 voices found in the loaded firmware's banks (Felucca's eight, for one; DX747 keeps none in a form the extractor finds) played by the fm6
engine, mixed into the output. Up / Down pick the voice, Left / Right the octave, the keyboard's note keys (Z S X D C...) or the pad's Y B A X (C, E, G, C) play it; the machine's own keys are not touched, and leaving the panel turns it off.
When the firmware has no banks of its own (the bundled ones have none), the preview plays eight built-in voices instead, and with no firmware loaded it starts in Solo. **R2 (Page Down) turns Solo on while the preview is on: the machine is not run at all (its sound and screen stop) and only the engine plays**, which is the way to play live on a unit too slow to emulate the firmware (the benchmark's `sound.fm6.voices12.load` and the Device report's `sound engine alone` line say how much of the processor the engine takes there; untested on the unit). It plays the stock engine's model of that voice (`docs/FM6.md`), not the firmware's own sound. `fm1ntt FIRMWARE --panel 6 --preview --screenshot FILE` shows it. Untested on a real R36S and by ear.

**Ports and instruments** (`docs/PORTING.md`): a firmware is not run on the R36S, it is *ported* to an instrument package (`.fm1i`, data) that native engines play and you can edit. `core/examples/port.rs` makes a level-A port from a firmware with a DX7-style bank (`port FW.fwsc --licence EXPR --model felucca --verify`), `fm1ntt --instrument FILE.fm1i` plays a package (Solo when no firmware is given), and the package keeps its source, SHA-256, licence and verification result. A port of a GPL firmware is that firmware's data: it belongs in the Firmware Pack, never in the MIT app. Untested on an R36S and by ear.

**The pad on MACHINE** (the stick axes and the black-key order are untested on a unit):

| | none held | **L1** held | **L2** held | **R2** held |
|---|---|---|---|---|
| left stick x / y | knob 1 / knob 2 | master level / presets | knob 1 / 2 | knob 1 / 2 |
| right stick x / y | knob 3 / knob 4 | select / algorithm | knob 3 / 4 | knob 3 / 4 |
| d-pad up left down right | notes 1 to 4 of the scale (C D E F) | OP1 OP2 OP3 OP4 | SEL FX HOME SAVE | GLO EDIT PLAY/STOP REC |
| Y B A X | notes 5 to 8 (G A B C) | OP5 OP6 PIT GLO | ENV LFO ARP SEQ | |
| Select / Start | OCT- / OCT+ | MONO / POLY | key down / up (the app's own key) | |
| Fn | next panel | next scale | next panel | next panel |

OP1 to OP6, PIT, GLO, MONO and POLY are **black piano keys** on the FM-1 (the manual), so L1 presses those note keys (they are functions only in EDIT mode, as on the unit: R2+d-pad left is EDIT; otherwise they sound as notes): F#3 G#3 A#3 C#4 D#4 F#4 G#4 A#4 C#5 D#5 in that order (the order is assumed from the manual's list, not measured). SEL has no matrix button, so it says so and presses nothing. Scales: major, minor,
dorian, mixolydian, both pentatonics, chromatic; the key moves them from C (shown as the key's name). On a keyboard: Z S X D C V ... and Q 2 W 3 E R ... play the 27 keys, F2-F12 and [ ] are the buttons, arrows
and PgUp/PgDn turn encoders on MACHINE and move around the other panels, Return is A, Backspace is B, Insert and Delete are X and Y, Esc leaves.
`docs/ARCHITECTURE.md` section 4 says what each panel does.

## Hero features (the Rec demo's checklist)

1. **carousel**: the nine panels on Fn, and the menu on L1 + R1. 2. **zoom**: Standard and Zoomed with F1 or Tab. 3. **midi**: send notes to the MIDI jack.
4. **tools**: pause, reset, flash slots. 5. **firmware**: boot a firmware and play it (a bundled one unless you give yours).

## Docs

[`docs/fm-1_reference-kit.md`](docs/fm-1_reference-kit.md) (the FM-1, its firmwares, emulators, R36S and buying notes) ·
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) · [`docs/LICENSING.md`](docs/LICENSING.md) · [`docs/FIRMWARE.md`](docs/FIRMWARE.md) · [`docs/FIRMWARE-CATALOG.md`](docs/FIRMWARE-CATALOG.md) ·
[`docs/HARDWARE.md`](docs/HARDWARE.md) · [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) · [`docs/SCREEN-MIRROR.md`](docs/SCREEN-MIRROR.md) · [`ROADMAP.md`](ROADMAP.md)

## Licence

MIT (`LICENSE`); the vendored `fm1-emu` is MIT too (`NOTICE`). Not affiliated with M-VAVE.
