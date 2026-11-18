# Porting: from an FM-1 firmware to an instrument that plays on the R36S

Decided with the maintainer 2026-11-18: **FM1-NTT is an R36S music instrument that FM-1 firmware can modify through a port, not a 1:1 emulation.**
The emulated CPU reaches 0.25x real time on the unit (`docs/PERFORMANCE.md`), so firmware cannot *run* there live. It can be **ported**: what
it contributes (data, and the behaviour that can be written down) is lifted into a package that native engines play. The emulator stays on the
Mac and on CI as the **extractor and the oracle**; on the R36S it is a non-live mode.

Everything below is **untested on a real R36S** unless a report says otherwise.

## The flow

```
FM-1 firmware (.fwsc / .bin)
  1. IDENTIFY   unpack, SHA-256, licence (from the firmware's own files), classify the engine
  2. EXTRACT    data: voice banks, tables, samples, presets, the control map
  3. OBSERVE    run it in the emulator (Mac/CI): probe notes, pots, MIDI; capture the audio
  4. FIT        pick a native engine and fit its parameters and tuning to the capture
  5. VERIFY     render the same notes natively, compare (level in dB, pitch in cents), store the result
  6. PACKAGE    write an instrument package (data, no CPU code) with its manifest
        |
R36S: the instrument host loads the package, plays it (pad, keys, USB MIDI), and edits it
```

`port` (`core/examples/port.rs`) runs 1, 2 and 6 and, for a firmware with a bank, a first form of 5 (`port --verify`). **Felucca 1.8.2, 2026-11-18: 8 voices ported, Copyleft tier; verified over its 5 FM6 presets x 3 notes (15 runs): level mean -0.03 dB, worst +0.35 dB, pitch worst 0.45 cent** (`port --felucca-sweep 5`; level and pitch only, not timbre). A firmware's preset is reached by its own panel steps (here the ones `scripts/fm6-compare.sh` uses), so each family needs its recipe: Felucca and Skiff are in `port::felucca_steps`, Jangada's 55 single clicks are not yet.

## Three levels of port

| Level | What | Automatic? | Example |
|---|---|---|---|
| **A, data** | a bank goes into an existing engine | yes | Felucca, Jangada, Skiff: 8 DX7 voices into `fm6` |
| **B, declarative** | the engine type, parameters and mappings are fitted from observation | partly: the fit is by hand, the package by tool | OPERATOR, Mighty Arp (4-op FM), X0X and PIGEON (drums, samples) |
| **C, behavioural** | the logic is in the firmware's code (sequencers, generative music) | no | the five Haunted FM-1 firmwares |

A level-C firmware is either reimplemented as a named native engine (written from what it is measured to do, never from GPL source) or stays
**emulator-only: not live on the R36S**. `port` says which, and why.

## The instrument package (`.fm1i`)

The extension is `.fm1i` (FM-1 instrument) and the first line is `fm1i = 1`. It was `.ntt` for a day, which is Neumetik's tracker format (renamed 2026-11-18); `.fmi` (FME, FMTracker, Garmin), `.fm1` (Lotus 1-2-3) and `.inst` (used in the sister apps) were also avoided. A Neumetik `.ntt` file is refused with a message, never mistaken for a package.

A text file, one `key = value` per line, `#` comments. **Data, never code.** No parser dependency (rule 4): the
reader is `core/src/package.rs`, bounded (1 MiB, 4096 lines, 256 voices, 64 effects, 128 map entries) and an `Error` on anything else.

```
fm1i = 1
name = Felucca factory
engine = fm6
licence = GPL-3.0-only
source = Felucca-1.8.2.fwsc
source-sha256 = <64 hex>
verify = fm6 vs emulator: level 0.04 dB, pitch 0.1 cent (note 60, velocity 100)
program = 0
voice = <256 hex: a packed DX7 voice>      # one line per voice
param 200 = 900                            # engine parameter id = value
sample 36 = kick.wav choke 42              # sampler/kit: a note, a file the host resolves
fx = delay                                 # then its `fxparam` lines
fxparam 0 = 300
map cc 1 = e130 0 99                       # CC 1 drives engine param 130 (fI.N for effect I), scaled to 0..99
root = 60                                  # the pitch a sampler plays its sample at
```

- **Provenance travels with the data**: source, its SHA-256, an **AND-only** licence expression from the allowlist (the same one
  `scripts/check-bundled.py` enforces), and the verification result. A package with no licence is refused.
- **A port inherits its firmware's licence.** A port of a GPL firmware is that firmware's data: it belongs in the **Pack** tier
  (`../FM1-NTT-Firmware-Pack`), never in the MIT app or a Mac App Store build. A port of a permissive firmware may go in `bundled-firmware/`.
  `package::tier` returns which, and `port` prints it. Packages made by hand from original voices (the built-in eight) are the project's own, MIT.
- **A package that fails verification still loads**: it is marked unverified and the measured error is shown, never hidden.
- **No scripting.** Engines are Rust, chosen by name; packages are data. If logic is needed later, a small bounded expression VM of our own for
  modulation and mapping only (an open decision, ROADMAP 5c).

## The engine interface (`core/src/engine.rs`)

Every engine implements `Engine`: `kind`, `params` (a descriptor per parameter: id, name, range, default, unit), `get`/`set` (clamped), `note_on`, `note_off`,
`all_off`, `pitch_bend`, `render` (mono `i16`) and `sounding`. The descriptors are what make a package **editable**: the editor screen, the MIDI map
and the modulation matrix are generic over them, so a new engine gets an editor for free. The audio path allocates nothing after `new`.

| Engine | State |
|---|---|
| `fm6` | DX7-style 6-operator (msfa port), fitted to DX747, Felucca; params 0 to 154 are the patch bytes, 200 up the model |
| `sampler`, `kit` | IMA ADPCM / WAV, drum map, choke, limiter; fitted to X0X |
| `analog` | subtractive: polyBLEP saw/pulse/triangle/sine, a state-variable filter, ADSR, 8 voices; **written fresh, not fitted to any firmware** |
| `fm4` | 4-operator FM for the OPERATOR / Mighty Arp family; **not built, not fitted** |

The shared effects (`core/src/fx.rs`): limiter, delay, low-pass, chorus, reverb.

In the app, `fm1ntt --instrument FILE.fm1i` loads a package into the SOUND preview (Solo when no firmware is given); the C API is `fm1ntt_inst_*`.

## The instrument host (`core/src/instrument.rs`)

Loads a package, builds the engine, applies its parameters and effects, parses MIDI bytes (note on/off, CC through the map, pitch bend, program change
across the bank, all notes off), and renders mono or stereo with the limiter last. Deterministic. The C API (`fm1ntt_inst_*`) is what the SDL host calls.

## Verification (`core/src/verify.rs`)

The oracle half needs the emulator, so the comparison tool is `fm6cmp` (`docs/FM6.md`); `verify` holds the part both sides share: render a note,
measure its level (dB RMS), its pitch (a peak search, cents) and its decay, and compare two renders. A package's `verify` line is what that returned.

## What the stock firmware adds to the flow (2026-11-18, from `docs/HARDWARE.md` and `docs/FM-1-MIDI.md`; nothing run)

Stock is the one firmware with a manual, a MIDI table and a known engine (DX747, matched by `fm6` within 0.03 dB). Its port is **local-only**: a vendor binary (rule 2), so any test using it skips when it is absent.

- **EXTRACT has two routes.** The file (`fwx`) finds no packed bank in the stock `.fwsc`; the manual puts voices in flash (4 banks x 32). The second route reads them from the running machine: the emulated flash after boot, or a bank dump over MIDI (precedent: Felucca's `FM6_PUT`). Whether stock's flash can be read this way is unchecked.
- **OBSERVE can be driven by MIDI, for presets.** On stock in the emulator, Program Change reached voices 001, 006, 032, 033 and 128, so a preset needs no panel recipe like `port::felucca_steps` (other families untested). **The effect CCs did not do the same:** only filter type and the reverb and delay mixes changed the sound, while the panel's effects work, so effects are fitted from the panel. **The SysEx parameter write does work on stock** (the DX7's numbers, jack and USB alike): a port can write test voices into the firmware and compare, as the Felucca work did with `FM6_PUT`, with no panel clicks (`docs/FM-1-MIDI.md`).
- **FIT has an effects target.** Six slots in a reorderable chain: filter (LP/BP/HP, log cutoff, Q), reverb (room/hall/plate), delay, distortion, chorus, phaser. `fx.rs` lacks distortion, phaser, the BP/HP modes and reordering.
- **A fourth level, between B and C: ARP and SEQ.** Their behaviour is documented well enough to write from the manual and verify against stock, so they are a declarative host feature, not emulator-only level C.
- **The package is too thin for a complete port.** It must also carry four banks, global settings (MIDI channel, effect channel, bend range, velocity, glide), the effect-chain order, mono/poly and ARP/SEQ patterns.

## Limits (said plainly)

- Nothing here has been heard on, or run on, a real R36S or a real FM-1.
- Level B and C ports exist only as far as the engines do. `fm4` and the level-C reimplementations are open.
- Extracted data from a GPL firmware is that firmware's: no bank leaves this machine except into the Pack (`docs/LICENSING.md`).
