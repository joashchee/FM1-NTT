# Architecture

Decisions as of 2026-11-16. **The ones marked open need the maintainer.**

## 1. What it is

A sandbox for FM-1 firmware: load an image, run it on an emulated FM-1, hear it, play its keys and knobs, feed it MIDI. R36S first
(through r3i6nos), then macOS, Windows, Linux and the web. It previews community firmware before it is flashed; it does not replace the
unit (docs/fm-1_reference-kit.md, section 4.3).

**Update 2026-11-18:** the product is also an *instrument*: firmware is ported to data packages that native engines play on the R36S, and the emulator is the extractor and oracle (`docs/PORTING.md`).

## 2. The machine (what is being emulated)

From the reference kit and the vendored emulator (docs/HARDWARE.md): a **JieLi AC791N** with two **pi32v2** cores; a 240 x 240 LCD; 14
buttons, 27 note keys, 7 encoders and a pot; SPI flash; audio out at 44.1 kHz; MIDI over UART, USB and BLE; a USB device port.

## 3. Shape

- **`core/`: a Rust static library behind a C API** (`fm1ntt.h`). The CPU, memory map and peripherals are the **vendored `fm1-emu` crates**
  (`core/vendor/fm1-emu`, MIT, decided 2026-11-16; `VENDORED.md`, `NOTICE`): `fm1-isa` (the decoder), `fm1-core` (interpreter, plus a native
  block compiler for AArch64 and x86-64, off by default here), `fm1-soc` (the map, LCD, flash, audio, USB, the `.fwsc` reader). They have no
  dependencies of their own. `core/src/` wraps them as a deterministic `Machine`: boot a package or a raw image, render a quantum of audio
  (the audio device paces the emulation), read the screen, take keys, encoders, the pot and MIDI bytes. Never panics on bad input.
- **`host/`: a C and SDL2 program with its own pixel graphics** (`pix.h`: a 640 x 480 canvas, an original 5 x 7 font). **It does not use
  r3i6nos's text-mode theme** (decided 2026-11-16), so it needs no r3i6nos checkout to build.
- **`web/`: the installable PWA shell**, then the core built to WebAssembly (`wasm32-unknown-unknown`) behind a Web Worker and an
  AudioWorklet, so the page never blocks. No service worker unless offline use becomes a stated requirement.
- **`r36s/`: the `.deb`** for r3i6nos (`build-deb.sh`, Docker only), listed in r3i6nos's `scripts/ports.conf` once it runs on a unit.

## 4. The window: Standard and Zoomed

The FM-1's 240 x 240 screen is drawn **pixel for pixel, flush top left**; the rest of the 640 x 480 window is the sandbox's panels. **Tab or F1**
(a keyboard; the pad's Fn now turns the carousel, the maintainer's mapping of 2026-11-16) toggles the two modes:

| | Standard | Zoomed |
|---|---|---|
| FM-1 screen | 1x, (0,0)-(239,239) | 2x, each pixel a 2 x 2 block, (0,0)-(479,479) |
| Panels | right, x 240-639 the full height (machine, buttons, keys, encoders and pot, sound, the host monitor, log); below the screen, x 0-239, y 240-479 (controls) | right, x 480-639 (state, sound, keys, buttons, log) |

`host/pix.h` holds the layout (`px_screen_rect`, `px_panel_rects`) and the drawing; the self-test checks, **on every panel in both modes**,
that the screen lands exactly (1x and 2x), that the panels and the screen tile the window with no overlap, that every printable character has a
glyph and that every text colour passes 4.5:1. The canvas is presented with SDL's integer scaling and no smoothing, so on a 640 x 480 R36S it
is 1:1.

### The panel carousel (as TinyNTT's)

The top 18 rows of the right panel are the carousel: the panel before, the current one in the middle, the one after; **Fn** (raw button 16) turns it forward, Home and End both ways
on a keyboard; it goes round. **L1 + R1** together open the menu (TOOLS) and close it again, back to the panel it came from. Standard also has, below the screen, what the buttons do on this panel. Eight panels:

| Panel | What it is for | Controls (pad) |
|---|---|---|
| MACHINE | the FM-1 itself: state, the 14 buttons, the 27 keys, 7 encoders and the pot, levels, the log; the pad is the FM-1's controls here | A PLAY, X HOME, Y EDIT, D-pad OCT and SELECT, L2 R2 PRESETS |
| FILES | your firmware folder, then the permissive **bundled firmware** marked B, then the **GPL pack** (its own repository) marked G (`.fwsc`, `.bin`), what is loaded, and **Quit** as the last row | Up Down, L2 R2 page, A boot (a `.bin` as a raw image), X rescan |
| CPU | both cores: pc, sp, flags, the 16 registers, instructions retired; speed (M insns/s, times real time); JIT | Up Down or Left Right: core 0 or 1 |
| MEMORY | a hex and ASCII view of RAM, the XIP image or the flash, by whole pages, reading no peripheral | Up Down row, L2 R2 page, Left Right region, X Y 64 KiB |
| CONSOLE | what the firmware writes to its debug box | none |
| MIDI | send notes to the emulated MIDI IN jack (channel, note, velocity), the queue and drop counters, the bytes sent | A note on then off, D-pad note and octave, X Y velocity, L2 R2 channel, B all notes off |
| SOUND | a scope, level and peak meters, the **recorder** (a WAV, written from a ring off the audio thread) | A record, X screenshot of the FM-1's screen, Y clear peaks |
| TOOLS | pause, **reset**, panic, native JIT, four **flash slots** (save, restore), screenshot, **Device report**, **Rec demo**, **Benchmark** | Up Down, Left Right slot, A run, B no |

Start pauses and resumes from any panel (the emulation stops; the sound goes quiet). A question (reset, restore, quit while recording) is
answered with A or B and nothing else moves meanwhile. Nothing scrolls: a long list is whole pages. Files are written only under the app's
folder (`$FM1NTT_DIR`, else r3i6nos's choice (`$R3I6NOS_APP_DATA`, else `r3i6nos-appdata path FM1-NTT`: the data card's `/roms2/FM1-NTT` when a valid one is in, else `/roms/FM1-NTT`), else our own check of `/roms2` by the same rules when there is no engine, else `~/FM1-NTT`): `firmware/`, `slots/`, `recordings/`, `screenshots/`, each to a
temporary name and then renamed. **The three reports** (TOOLS, and as flags for SSH): the **Device report** is r3i6nos's (what is wrong on this unit: PASS/FAIL against goals,
the card, the sound callback, the power windows), the **Rec demo** records a deterministic tour with screenshots, a WAV and an mp4, and the
**Benchmark** (`docs/BENCHMARKS.md`) is the file to collect from every platform and compare with `scripts/bench-compare.py`. A report or a
benchmark runs on a thread and pauses the emulation while it measures; Rec demo takes over the window with its own progress screen.

**Open:** MIDI out from the emulated jack is not exposed by the vendored core, so the MIDI panel shows only
what was sent in.

## 5. The CPU question: decided

Vendor `fm1-emu`'s core (the maintainer, 2026-11-16). The cost to watch: its size on a 1 GB R36S and whether the interpreter runs in real
time on the Cortex-A35 (the native AArch64 block compiler is there and is the R36S's own architecture; it is off by default until measured,
and untested on the unit). A bug in the vendored crates is reported upstream; a local change is recorded in `VENDORED.md` first.

## 6. Platforms and shipping

| Platform | Route |
|---|---|
| R36S (r3i6nos) | `FM1-NTT.deb`, built by `r36s/build-deb.sh`; the device report, Rec demo, splash, hot-plug toast and power section every r3i6nos port has (ROADMAP section 4), drawn in pixels |
| macOS | free app: a signed, notarized direct download only (**no Mac App Store build**, the maintainer, 2026-11-18); published as ANSI Apps, bundle id `com.ansiapps.fm1ntt`, signed and notarized by Scoreless Music, the only Apple account (global rule); `scripts/build-release.sh` on the maintainer's Mac, as Coupler's |
| Windows, Linux | the SDL2 host built on CI |
| Web | the PWA shell, then the WebAssembly build |
| Plugin (AU, VST3, CLAP) | not planned: `fm1-emu` has one; this project is the sandbox and the R36S |

## 7. Audio and MIDI

The emulated firmware makes the sound (a closed binary on stock firmware: docs/fm-1_reference-kit.md, section 2). The core hands the
host 16-bit stereo at 44.1 kHz; SDL's audio callback asks for 512 frames at a time and the emulation runs until they exist, so the audio
device is the clock. Input is taken under SDL's audio lock between buffers. MIDI in is one byte at a time into the emulated MIDI IN jack;
the pad, USB MIDI and Web MIDI feed it (ROADMAP section 2). The R36S can also send MIDI *to a real FM-1* over its OTG port (reference kit,
section 6): a separate, later mode.

## 8. Screen mirroring

r3i6nos's mirror sends text cells, which a pixel app has none of; the plan for an efficient pixel mirror (tile diffs first, then the LCD plus
the UI state redrawn by the page) is `docs/SCREEN-MIRROR.md`. Not built.

## 9. The look

The R36S and desktop host draw their own pixels (section 4). **Open:** the web build's look (the global text-mode rule, 48 x 25 with a
widescreen dashboard, or these pixel panels).
