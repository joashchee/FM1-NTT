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
| `fm4` | 4-operator FM for the OPERATOR / Mighty Arp family (`core/src/fm4.rs`, 2026-11-18): OPERATOR's eleven algorithms, self-feedback on every operator, per operator a ratio, fine ratio, detune, level, velocity sensitivity, key scaling, feedback and ADSR (parameter ids: 0 to 3 global, then eleven an operator from 10); 8 voices; **built; a first fit to OPERATOR 0.7 (below)** |

The shared effects (`core/src/fx.rs`): limiter, delay, low-pass, chorus, reverb, **filter** (low, band, high), **distortion** and **phaser** (2026-11-18: written fresh, fitted to nothing, tested for pass bands, stability, bounds and silence).

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
- **OBSERVE can be driven by MIDI, for presets.** On stock in the emulator, Program Change reached voices 001, 006, 032, 033 and 128, so a preset needs no panel recipe like `port::felucca_steps` (other families untested). **The effect CCs did not do the same:** only filter type and the reverb and delay mixes changed the sound, while the panel's effects work, so effects are fitted from the panel; **a scan of SysEx parameter numbers 156 to 1023 found no effect parameter either**. **The SysEx parameter write does work on stock** (the DX7's numbers, jack and USB alike): a port can write test voices into the firmware and compare, as the Felucca work did with `FM6_PUT`, with no panel clicks (`docs/FM-1-MIDI.md`).
- **FIT has an effects target.** Six slots in a reorderable chain: filter (LP/BP/HP, log cutoff, Q), reverb (room/hall/plate), delay, distortion, chorus, phaser. `fx.rs` now has distortion, phaser and the BP/HP modes (written, **not yet fitted** against stock's panel reads); it lacks reordering.
- **A fourth level, between B and C: ARP and SEQ.** Their behaviour is documented well enough to write from the manual and verify against stock, so they are a declarative host feature, not emulator-only level C.
- **The package is too thin for a complete port.** It must also carry four banks, global settings (MIDI channel, effect channel, bend range, velocity, glide), the effect-chain order, mono/poly and ARP/SEQ patterns.

## Limits (said plainly)

- Nothing here has been heard on, or run on, a real R36S or a real FM-1.
- Level B and C ports exist only as far as the engines do. `fm4` is built but unfitted; the level-C reimplementations are open.
- Extracted data from a GPL firmware is that firmware's: no bank leaves this machine except into the Pack (`docs/LICENSING.md`).

## `fm4` against OPERATOR 0.7 (2026-11-18)

OPERATOR (`npi3pak/fm1-operator`, release v0.7, `operator-0.7.fwsc`, SHA-256 `0ad697a2...c48d7`, matches the release's `SHA256SUMS`; GPL-3.0, so it is **bring-your-own, never copied into this repository**; its source was not read, rule 3) was driven in the emulator by MIDI over the jack and by its panel controls, and its audio measured with `core/examples/fm4fit.rs` (levels, the fundamental and 12 partials by a Fourier sum, an envelope trace). The firmware is an Ableton-Operator-style synth: four operators A to D (our OP1 to OP4), coarse ratio, fine, level in dB, waveform, ADSR with a dB sustain, per-operator feedback, a filter, an LFO and 6 voices; its panel draws the algorithm (algorithm 2 is C and D into B into A).

**Panel controls found** (home page): SELECT steps through D coarse/fine, D level, C coarse/fine, C level, B coarse/fine, B level, A coarse/fine, A level; KNOB1 then turns coarse (or level), KNOB2 fine. Level is steep and uneven (about 0.5 dB a click near -70 dB, 20 clicks from -inf to 0 dB), and a big step in one call is taken as a fast turn, so a level is set by clicking and **read off the screen**: after 5, 8, 12, 16 and 20 clicks, B read -54, -42, -26, -9.5 and 0.0 dB.

**Measured** (note 57, velocity 127 unless said):
- **A alone at 0 dB: 0.0484 peak (-26.3 dBFS)**, a clean sine. fm4 matches with `LEVEL` 16.
- **Velocity is linear in amplitude**: 0.0244 at velocity 1 to 0.0484 at 127, half at the bottom (the firmware's default sensitivity). fm4's law was quadratic; now linear, `VELOCITY SENS` 0 to 100 %, default 50.
- **Release**: 399 ms is the time to fall about 64 dB, 16 dB every 100 ms; the same law is used for decay (not measured separately). fm4's decay and release were 40 dB over the time; now 64 dB.
- **Modulation, algorithm 1 (B into A, ratio 1:1):** with B's level at -54, -42, -26 and -9.5 dB, harmonic 2 sat 13.6 to 14.1 dB below the level, so **the index is linear in the modulator's amplitude**. **Corrected later the same day:** those runs had B's own sustain at -24 dB (each operator has its own default envelope: B decays 399 ms to -24 dB), so the 0.42 rad first read off them was the index at -24 dB. With B's sustain raised to 0 dB the spectrum (harmonics 2 to 10 against a phase-modulation fit) gives **6.24 radians, a whole cycle (2 pi), at 0 dB**. fm4's `MOD INDEX` is in hundredths of a radian (generic default 400); **628 is OPERATOR**.
- **Pitch: every note is 146.8 cents flat** (ratio 0.9173 to the nominal, the same on notes 45, 57 and 69). 44100/48000 is 146.8 cents, and the emulator's DAC clock is fixed at about 44.1 kHz (`fm1-soc` `periph.rs`, from Felucca's timer), so **OPERATOR looks like a 48 kHz engine played back at 44.1 kHz here**. Whether a real FM-1 gives the right pitch is not known; the fit above is measured with the offset divided out, and the offset is **a finding about the emulator, open** (does the firmware program a different DAC rate that the core ignores?).

`fitted_to_operator_*` tests in `core/src/fm4.rs` hold these numbers (a patch built the same way: `LEVEL` 16, `MOD INDEX` 628, velocity 50, decay 998, release 399).

### The algorithms and the feedback (2026-11-18)

**The eleven algorithms** were read off the panel's diagrams (ALGORITHM turns through them on the home page; in EDIT the same encoder picks the operator). A is OP1 and D is OP4; `>` is "modulates":
1: D > C > B > A. 2: (C + D) > B > A. 3: C > B > A and D > A. 4: D > C > A and B > A. 5: D > B > A and D > C > A. 6: B + C + D > A. 7: B > A and D > C (A and C heard). 8: D > C > B, A alone (A and B heard). 9: D > A, B, C (all three heard). 10: D > C, with A and B alone (A, B, C heard). 11: all four alone.
Each was then played in the emulator with all four operators at 0 dB and full sustain (set by the panel: SELECT picks the level, KNOB1 turns it; EDIT, page 2, KNOB3 the sustain) and compared, harmonic by harmonic, with fm4 set the same way (`fm4fit fm4:ALG`). **Algorithms 2 and 3 to 11 agree within about 1 to 2.5 dB on the fundamental's level and harmonics 2 to 4, and the carriers sum linearly** (four carriers: 0.1933 against 4 x 0.0484). 3 and 4, and 7 and 9, are the same shape with letters swapped, so equal operators cannot tell them apart; their drawings do. **Algorithm 1, the deepest chain, differs (harmonic 2 at -3.5 dB there, -10.8 here), unexplained**: three stages of a whole cycle each make a spectrum that moves a lot with a small error in any level, envelope or phase, and algorithm 8 (a chain of three) and 5 agree. The test holds 2 and 3 to 11.

**Per-operator self-feedback**, measured on B alone (A and the others silenced) at 13, 61 and 100 % (the panel's readout after 6, 12 and 24 clicks): the fundamental falls to 0.977, 0.64 and 0.452 of its clean level and harmonics 2 and 3 sit at -14.2/-25.0, -6.9/-11.0 and -6.5/-10.4 dB. That is the **mean of the last two outputs times about 3.1 radians at 100 %** (the DX7's form), linear in the percentage; **the feedback uses the operator's output with its level and envelope in it** (at a -24 dB sustain the effect was 0.063 of the full one, as measured). fm4 now has it on every operator (`FEEDBACK` per operator, `FEEDBACK AT 100%` global, default 310). Harmonic 2 at 100 % is 2 dB higher here (-4.5) than there (-6.5), so the top of the knob is not exactly this law. **Operators B, C and D showed the same feedback; A showed none** (100 % changed nothing), so a port of OPERATOR sets A's feedback to 0.

**The ratio table (2026-11-18, measured by the zero crossings of operator A alone):** the coarse knob steps **0.5, 1, 2, 3, 4, 5, 6, 7 ... up to 48**, one click each (a knob turned several clicks in one call is a fast turn and skips: 3 clicks read 11), and **the fine value adds thousandths** to it (coarse 1 with fine 26 played 1.026 times the pitch, with fine 146 1.146; the screen read "1.026" and "1.146", and B's "18.248" is coarse 18 and fine 248). fm4 has `RATIO LAW` (1 is OPERATOR's; 0, the default, a percentage of the ratio), coarse up to 48 and fine up to 999; `fitted_to_operator_the_ratio_table` holds it. **Still not fitted:** algorithm 1's depth-three chain, the waveforms, fixed-frequency mode, the filter, the LFO, key scaling, the attack curve, sustain in dB (fm4's is linear), the per-operator envelope defaults, and **Mighty Arp** (below).

## `fm4` against Mighty Arp 1.5 (2026-11-18)

Mighty Arp (`NickCulbertson/FM1-Mighty-Arp`, release v1.5, `mightyarp-1.5.fwsc`, SHA-256 `7f15b86d...b54f`, matches the release's `SHA256SUMS`; **MPL-2.0**, a candidate for the Pack's MPL tier but not added; its source was not read) is a 4-operator FM and subtractive hybrid around a chance arpeggiator. Driven in the emulator with fwprobe/fm4fit. Controls: EDIT then EDIT again steps ALGO/RATIO/FEEDBACK/MOD WAVE, then OP 1 to 4 (each RATIO, LEVEL, WAVE, DECAY on KNOB1 to 4; a modulator or carrier tag); ARP has RATE, OCTAVES, MODE, GATE; HOME has CHANCE, RATE, SPACE, DRIFT; FX pages DELAY, REVERB, CHORUS. PLAY toggles ARP and SEQ. **A single held note is not a clean tone**: the arpeggiator fires steps by chance and retriggers every step. The runs used chance 97 %, one octave, gate 98 %, delay and reverb mix 0, space 0, drift 0, and `fm4fit`'s new `n:8192` and `win:0.05` (analyse a 0.19 s window from 50 ms after the first sound, before the next step) and `narrow` (the fundamental is where the note says).

**Findings**
- **It is in tune** (note 57 at 220 Hz): so OPERATOR's 146.8 cents is OPERATOR's own, not the emulator's clock, which **settles the open question from the OPERATOR fit**: the core's 44.1 kHz DAC is right for firmware that programs it, and OPERATOR looks like a 48 kHz engine that the core plays too slowly (or the real unit resamples it; unknown).
- **Eight algorithms, a subset of OPERATOR's** with the same numbering of operators (the diagram boxes are numbered 1 to 4 here): 1 = 4>3>2>1, 2 = (3+4)>2>1, 3 = 3>2>1 and 4>1, 4 = 4>3>1 and 2>1, 5 = 2>1 and 4>3 (the home patch, TINES EP), 6 = 4>1, 2, 3, 7 = 4>3 with 1 and 2 alone, 8 = all four alone. These are OPERATOR's algorithms 1 to 4, 7, 9, 10 and 11, so **fm4's table already has them** (fm4 index = Mighty number 1 to 4, then 6, 8, 9, 10). Read from the small diagrams; only 5 was checked by sound.
- **Level is linear in amplitude** (OPERATOR's is a dB law): one carrier at 100 % peaks at 0.0708 of full scale, 50 % at half. fm4 has `LEVEL LAW` (1 is linear, level 0 to 100).
- **Modulation index about 5.4 radians at 100 %**, linear in the level to within a little compression: OP2 into OP1 at 5, 10, 15, 25 % gave harmonic 2 at -15.9, -9.4, -5.1, +2.1 dB and harmonic 3 at -40.6, -28.4, -20.5, -8.7 dB; fm4 with `MOD INDEX` 540 is within 1.3 dB on harmonic 2 and 2.1 dB on harmonic 3 (the best single index for each level ran from 520 to 620). OPERATOR's was a whole cycle, 628.

**Not fitted:** the ratio table (RATIO is X1, X2, ... X14, a table, not read), the waves W1 to W? (W2 is "peaked sine"; a wavetable), feedback, the decay law, the carrier's own envelope, the arpeggiator itself (a feature for the host, ROADMAP 5c), the filter and effects, the presets. The Mighty Arp test is `fitted_to_mighty_arp_the_linear_level_and_the_index` in `core/src/fm4.rs`.

## The arpeggiator (2026-11-18)

`core/src/arp.rs`, written fresh from the FM-1 manual's list (Up, Down, Inclusive, Exclusive, Random, Order, Repeat; one to four octaves; 30 to 300 BPM; gate; swing; latch) and from Mighty Arp's controls (a chance per step), never from any firmware's source. It is **a host feature, not an engine**: it sits in front of whatever engine an instrument package names, so a ported voice gets an arpeggiator for free, and it costs nothing while off.

- **How it works:** held notes in, `(frame, event)` out, on the caller's sample clock. Deterministic (the random modes use a seeded xorshift, `SEED` restarts them), bounded (16 held notes, notes clamped to 0 to 127), and exact: the events do not depend on the block size (tested at 64 and 700 frames). The instrument host splits each render slice at the events, so a note lands on its frame.
- **Parameters** (ids in `arp.rs`, `ParamDesc` like an engine's): ON, MODE, OCTAVES, BPM, RATE (1/4, 1/8, 1/16, 1/32 and the triplets of the first three), GATE, SWING, LATCH, CHANCE, SEED. In a package: `arp N = V` lines (at most 32; an unknown id is refused at load; only the non-default settings are written back). C API: `fm1ntt_inst_arp_count/param/get/set`.
- **Gate and swing:** a note ends at gate percent of the step; at 100 % it ends exactly as the next begins. Swing lengthens every odd step and shortens the next by the same fraction, so a pair keeps its length.
- **Latch:** released keys stay in the chord; the first key pressed after all were released replaces it; un-latching drops the keys no longer down.
- **Chance:** each step plays with that probability and the step still counts (chance 0 plays nothing, as Mighty Arp's does).
- **Not done:** clock sync to MIDI clock and the tempo from it; a step sequencer (SEQ); mapping the arpeggiator's controls to the pad and the SOUND panel (the host does not call the new C functions yet); fitting its timing and modes against the stock firmware or Mighty Arp (their rate and gate laws, mode order, the octave order of Random and the first step's timing are **not measured**; the manual's wording was used).

## The instrument editor and the picker (2026-11-18)

**Picker:** FILES lists `*.fm1i` packages with the firmware (your `instruments/` folder and the firmware folders, `bundled-firmware/` and the Pack when they hold any); A on one loads it, shows SOUND with the preview on, and plays it alone when no firmware is loaded (the same as `--instrument`). A bad package keeps the old instrument and says why.

**Editor** (a mode of the SOUND panel, not a new panel, so the carousel, the Rec demo's tour and the benchmark's panel workload are unchanged): with an instrument loaded, **B** (keyboard Backspace) or the pad's **Select** (which also works while the preview is playing the pad) opens it. It is generic over what the core describes (`ParamDesc`): the groups are the **engine's** parameters (fm6's 155 patch bytes and model, fm4's, analog's, ...), the **arpeggiator's** and **each effect's**, a group a page at a time (24 rows, never scrolling). **Up and Down** pick, **Left and Right** change by 1, **X and Y** by a tenth of the range (at least 10), **L2 and R2** change group, **A** saves, **B**, Select or Esc leave. The value is clamped by the core, applied under the audio lock, and heard at once (the keyboard's keys still play the preview; the pad's face buttons edit while it is open). **A saves to `instruments/<name>-edit.fm1i`** (a temporary name, then a rename, written through `fm1ntt_inst_to_package`: edits included, the verification note says "edited since"), **never over the file it was loaded from**, and the FILES list is rescanned so the copy is there. New C calls: `fm1ntt_inst_fx_count/name/param_count/param/get/set`. Tested in the host self-test (open, a page's rows, +-1 and the big step, clamping, wrapping, the groups, the save, loading the copy back with its edits, the picker, Select, the pad not playing while it is open, and both layouts drawn with the foot clear); looked at as screenshots on this Mac in Standard and Zoomed. **Not seen on the R36S panel, no pad tried there.**

**Not done:** a reset-to-default key (the core has the default; no button is free), naming a copy, deleting one, editing the voice bank's names, a bar or graph for a parameter, and the "port available" line and Load button on a firmware's page (they need the package's firmware hash matched to a firmware, which the picker alone does not give).
