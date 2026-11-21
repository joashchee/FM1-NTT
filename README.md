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
                              # (on the R36S the DATA card's /roms2/FM1-NTT when a valid one is in, else /roms/FM1-NTT; else ~/FM1-NTT; FM1NTT_DIR overrides; the TOOLS and MONITOR panels say which and why)
build/fm1ntt --screenshot out.bmp [--zoomed]   # one frame to a file
build/fm1ntt --report [FIRMWARE]               # the Device report (r3i6nos rule 9) -> $BASE/r36s-report.txt
build/fm1ntt --bench [--seconds N] [--fw FILE] # the Benchmark report -> $BASE/bench/ ; compare files with scripts/bench-compare.py
build/fm1ntt --rec-demo [--features a,b] [--no-video] [FIRMWARE]   # Rec demo -> $BASE/Demo/ (PNGs, an mp4 with ffmpeg, the log)
node web/server.ts            # the web shell on http://localhost:8080
r36s/build-deb.sh             # build/pkg/FM1-NTT.deb for the R36S (Docker)
```

## Panels and controls

**FILES:** A on a firmware opens its page (licence, source, SHA-256, its demo), where A loads it, X plays its 15 s demo and Y makes it; TOOLS > BACKGROUND DEMOS makes the missing demos in the background at the lowest priority (`docs/SHOWCASE.md`).

**Fn** (Home / End on a keyboard) turns the panel carousel: MACHINE, FILES, CPU, MEMORY, CONSOLE, MIDI, SOUND, MONITOR, TOOLS. **MONITOR** is a `top` for the machine the app runs on (CPU per core, memory, processes, this app's threads, battery; R1 + up/down pages, X sorts, Y freezes; `docs/MONITOR.md`); it costs nothing while another panel is showing, and TOOLS > DATA RAIN adds an opt-in stream of the data passing through. **L1 + R1** together open the **main menu** over the panel (About, Colour theme, **Keymap**, Device report, Rec demo, Benchmark, **Quit**, then Tools, which is the panel with the rest) and close it
again; **L2 + R2** open the **context menu** (what the panel can do, and Save a screenshot). **Quit is deliberate: only the main menu's Quit leaves** (no button and no Esc does). **L1 + Fn** is the first
panel and **R1 + Fn** the panel shown before this one (again: back). **R1 held is "increase"**: the d-pad's up and down become Page up and Page down, left and right Home and End; on MACHINE R1 + left and
right shift the octave and R1 + up and down transpose the key. Off MACHINE the right stick moves the selection like the d-pad (6 steps a second at most, with R1 held it pages). L1, R1, L2, R2, Fn, L3 and R3
are the system's and never a panel's; **each panel binds the ten other buttons (D-pad, A, B, X, Y, Select, Start) as it needs** (the tables below and the Keymap show them). L3 and R3 say that there is no
virtual mouse or keyboard. On a keyboard: the backquote key is the main menu and the backslash key the context menu, Esc closes a menu or a question. **Colour theme** picks Midnight, High Contrast, Light or Mono Green (kept in
`theme.txt` in the app's folder). **Tab / F1** toggles Standard and Zoomed (2x). **Start** (P on a keyboard) pauses on every panel that has no use for it (MIDI and SOUND do).

**The Keymap** (main menu): pages for the system's inputs, each panel's buttons (what each does there) and MACHINE's L1, L2, R2 and R1 sets, opening on the panel you came from, and a **test page** that
shows every press and the sticks and does nothing. **A** on a row, then the new button, rebinds it: one that another action has is a conflict, asked before the two are swapped; the system's buttons are
refused. **X** puts the row back, **Y** the page, **Start** everything (asked). Changes are kept at once in `keymap.txt` in the app's folder; Rec demo and the Device report use the defaults. This is r3i6nos's
input mapping (`CLAUDE.md` rule 13); `ROADMAP.md` section 4f records the decisions.

**The SOUND preview** (Start, or Page Up on a keyboard, on the SOUND panel): the DX7 voices found in the loaded firmware's banks (Felucca's eight, for one; DX747 keeps none in a form the extractor finds) played by the fm6
engine, mixed into the output. Up / Down pick the voice, Left / Right the octave, the keyboard's note keys (Z S X D C...) or the pad's Y B A X (C, E, G, C) play it; the machine's own keys are not touched, and leaving the panel turns it off.
When the firmware has no banks of its own (the bundled ones have none), the preview plays eight built-in voices instead, and with no firmware loaded it starts in Solo. **Select (Page Down) turns Solo on while the preview is on: the machine is not run at all (its sound and screen stop) and only the engine plays**, which is the way to play live on a unit too slow to emulate the firmware (the benchmark's `sound.fm6.voices12.load` and the Device report's `sound engine alone` line say how much of the processor the engine takes there; untested on the unit). It plays the stock engine's model of that voice (`docs/FM6.md`), not the firmware's own sound. `fm1ntt FIRMWARE --panel 6 --preview --screenshot FILE` shows it. Untested on a real R36S and by ear.

**Ports and instruments** (`docs/PORTING.md`): a firmware is not run on the R36S, it is *ported* to an instrument package (`.fm1i`, data) that native engines play and you can edit. `core/examples/port.rs` makes a level-A port from a firmware with a DX7-style bank (`port FW.fwsc --licence EXPR --model felucca --verify`), `fm1ntt --instrument FILE.fm1i` plays a package (Solo when no firmware is given), and the package keeps its source, SHA-256, licence and verification result. A port of a GPL firmware is that firmware's data: it belongs in the Firmware Pack, never in the MIT app. Untested on an R36S and by ear.

**The pad on MACHINE** (the stick axes and the black-key order are untested on a unit):

| | none held | **L1** held | **L2** held | **R2** held |
|---|---|---|---|---|
| left stick x / y | knob 1 / knob 2 | master level / presets | knob 1 / 2 | knob 1 / 2 |
| right stick x / y | knob 3 / knob 4 | select / algorithm | knob 3 / 4 | knob 3 / 4 |
| d-pad up left down right | notes 1 to 4 of the scale (C D E F) | OP1 OP2 OP3 OP4 | SCL FX HOME SAVE | GLO EDIT PLAY/STOP REC |
| Y B A X | notes 5 to 8 (G A B C) | OP5 OP6 PIT GLO | ENV LFO ARP SEQ | |
| Select / Start | OCT- / OCT+ | MONO / POLY | key down / up (the app's own key) | |
| Fn | next panel | next panel | next panel | next panel |

OP1 to OP6, PIT, GLO, MONO and POLY are **black piano keys** on the FM-1 (the manual), so L1 presses those note keys (they are functions only in EDIT mode, as on the unit: R2+d-pad left is EDIT; otherwise they sound as notes): F#3 G#3 A#3 C#4 D#4 F#4 G#4 A#4 C#5 D#5 in that order (the order is assumed from the manual's list, not measured). L2 + up presses SCL (the FM-1's scale button, matrix id 3, from `fm1-emu`'s panel table). Scales: major, minor,
dorian, mixolydian, both pentatonics, chromatic; the key moves them from C (shown as the key's name). On a keyboard: Z S X D C V ... and Q 2 W 3 E R ... play the 27 keys, F2-F12 and [ ] are the buttons, arrows
and PgUp/PgDn turn encoders on MACHINE and move around the other panels, Return is A, Backspace is B, Insert and Delete are X and Y, Esc closes a menu or a question. The next scale is in the context menu (L2 + R2).
`docs/ARCHITECTURE.md` section 4 says what each panel does.

## Hero features (the Rec demo's checklist)

1. **carousel**: the nine panels on Fn, and the main menu on L1 + R1. 2. **zoom**: Standard and Zoomed with F1 or Tab. 3. **midi**: send notes to the MIDI jack.
4. **tools**: pause, reset, flash slots. 5. **firmware**: boot a firmware and play it (a bundled one unless you give yours).

## Docs

[`docs/fm-1_reference-kit.md`](docs/fm-1_reference-kit.md) (the FM-1, its firmwares, emulators, R36S and buying notes) ·
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) · [`docs/LICENSING.md`](docs/LICENSING.md) · [`docs/FIRMWARE.md`](docs/FIRMWARE.md) · [`docs/FIRMWARE-CATALOG.md`](docs/FIRMWARE-CATALOG.md) ·
[`docs/HARDWARE.md`](docs/HARDWARE.md) · [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) · [`docs/SCREEN-MIRROR.md`](docs/SCREEN-MIRROR.md) · [`ROADMAP.md`](ROADMAP.md)

## Licence

MIT (`LICENSE`); the vendored `fm1-emu` is MIT too (`NOTICE`). Not affiliated with M-VAVE.
