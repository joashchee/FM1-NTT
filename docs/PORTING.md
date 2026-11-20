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

**The UI half of the flow** (screens, controls, visuals and behaviour capture, with the first scaffolding and a study of VNL-1) is `docs/PORTING-UI.md`.

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
- Level B and C ports exist only as far as the engines do. `fm4` is built and fitted in part (waveforms and sustain law from OPERATOR; fixed mode, filter, LFO and the looped envelope open); the level-C reimplementations are open.
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

**Algorithm 1 again (2026-11-20, OPERATOR 0.7 from the release, SHA-256 checked; `fm4fit` driven by the panel).** The old "harmonic 2 at -3.5 dB" came from a setup that was never looked at on screen, and a re-run of the same actions shows why it cannot be trusted: with B, C and D all at 0 dB each stage is a whole cycle, so the spectrum is chaotic (a different state of the panel gave -8.0 instead, fundamental 0.0123 instead of 0.0179) and no topology or index fitted it. **Moderate levels are well-conditioned**, so the stages were measured one at a time (B, C, D each at about -12 dB, sustain 0 dB, checked on the screen): **B alone into A**: harmonics 1 to 5 of A at 0.216, 0.638, 0.237, 0.072, 0.014 against 0.216, 0.638, 0.237, 0.072, 0.014 from sin(t + b sin t), b = 2 pi x 0.251: an exact match (peak 0.0306 against 0.0309). **B and C**: fundamental 0.0560 against 0.0561 and harmonics 2 to 7 at -44.4, -13.3, -21.3, -18.2, -21.2, -25.1 against -49.7, -13.2, -21.1, -17.8, -20.5, -24.1: fm4's chain law holds for two stages. **C or D alone, or B and D (C off), leave A a pure sine**, which confirms the chain D > C > B > A as drawn. **Adding D at the same level does not follow the law**: the fundamental fell to 0.0297 where the chain predicts 0.0048, and D at 80, 98, 108 clicks (about -30, -21, -16 dB) fitted an index of 0.06, 0.26 and 0.46 rad where the law gives 0.20, 0.56 and 1.0, so D into C acts 7 to 10 dB weaker, not a power law (at 116 clicks the fit is poor whatever the index). **Not explained**: a gain on the fourth operator, the level knob's law (the levels were read from the screen, about 0.5 dB a click from -inf, 0 dB at about 140 clicks, and a fast turn of 100 goes to 0), or aliasing at a 48 kHz engine rate are all open; fm4 is unchanged and its test still holds algorithms 2 and 3 to 11. **Panel recipe** (EDIT page 1, Level is KNOB3, SELECT steps the page: 2 of 7 has KNOB3 as Sustain; ALGORITHM picks the operator A to D; key:ENV is a different page and breaks the setup; `enc:SELECT:-1` undoes it; a fast turn of 100 reaches 0 dB, single clicks give the middle).

**Per-operator self-feedback**, measured on B alone (A and the others silenced) at 13, 61 and 100 % (the panel's readout after 6, 12 and 24 clicks): the fundamental falls to 0.977, 0.64 and 0.452 of its clean level and harmonics 2 and 3 sit at -14.2/-25.0, -6.9/-11.0 and -6.5/-10.4 dB. That is the **mean of the last two outputs times about 3.1 radians at 100 %** (the DX7's form), linear in the percentage; **the feedback uses the operator's output with its level and envelope in it** (at a -24 dB sustain the effect was 0.063 of the full one, as measured). fm4 now has it on every operator (`FEEDBACK` per operator, `FEEDBACK AT 100%` global, default 310). Harmonic 2 at 100 % is 2 dB higher here (-4.5) than there (-6.5), so the top of the knob is not exactly this law. **Operators B, C and D showed the same feedback; A showed none** (100 % changed nothing), so a port of OPERATOR sets A's feedback to 0.

**The ratio table (2026-11-18, measured by the zero crossings of operator A alone):** the coarse knob steps **0.5, 1, 2, 3, 4, 5, 6, 7 ... up to 48**, one click each (a knob turned several clicks in one call is a fast turn and skips: 3 clicks read 11), and **the fine value adds thousandths** to it (coarse 1 with fine 26 played 1.026 times the pitch, with fine 146 1.146; the screen read "1.026" and "1.146", and B's "18.248" is coarse 18 and fine 248). fm4 has `RATIO LAW` (1 is OPERATOR's; 0, the default, a percentage of the ratio), coarse up to 48 and fine up to 999; `fitted_to_operator_the_ratio_table` holds it. **Still not fitted:** algorithm 1's depth-three chain, the waveforms, fixed-frequency mode, the filter, the LFO, key scaling, the attack curve, sustain in dB (fm4's is linear), the per-operator envelope defaults, and **Mighty Arp** (below).

### OPERATOR's waveforms, sustain and envelope page (2026-11-19)

Read off the panel (EDIT page 1, KNOB4 steps the wave; ENV page, `Times 1/3`) and measured with `fm4fit`. **Waves, in order:** Sine, Sine 4 Bit, Sine 8 Bit, Saw 3, 4, 6, 8, 16, 32, 64, Saw D, Square 3, 4, 6, 8, 16, 32, 64, Square D. **A Saw N has harmonics 1 to N at 1/k** (Saw 3: 2nd -6.1 dB, 3rd -9.7, 4th absent; Saw 6 to the 6th at -16.2) **and a Square N the first N odd harmonics at 1/k** (Square 3: 3rd -9.7, 5th -14.4, 7th absent; Square 6 to the 11th); **every wave has the same peak as the sine** (0.0484 for operator A at 0 dB), so the fundamental falls as harmonics are added (0.0335 Saw 3, 0.0518 Square 3). The D waves are the ideal ramp and square. fm4 has them as 2048-point tables (`WAVES`, ids `WAVE_BASE + op`); `fitted_to_operator_the_waveforms` holds the spectra. The 4- and 8-bit sines are quantised sines (16 and 256 levels, the screen shows the steps); their exact rounding and the D waves' aliasing at high notes are **not compared**.
**Sustain is shown in dB** (the knob read -8.5 dB after 3 clicks and -25 dB after 8, so its steps are uneven): fm4's `SUSTAIN LAW` 1 reads the 0 to 99 value as dB (99 is 0 dB, 1 dB a step); the knob's own step table is not read.
**Fixed, Phase and Key** (EDIT pages 3 to 5 of 7, SELECT steps them; measured with `fm4fit range:LO:HI`, which looks for the tone away from the note): **Fixed** plays Freq x Multi and ignores the note (100 Hz gave the same tone for notes 57 and 69; the emulator's 0.9173 pitch factor applies to it too); Freq runs 10 Hz to about 1.02 kHz in 1 Hz steps and Multi is x0.001, x0.01, x0.1, x1, x10. **Phase** is the start phase in percent of a cycle (66 % starts a sine at -0.83 and falling; Retrig, on by default, restarts it each note). **Key** is -100 to +100 % about **note 60**: the level is multiplied by `2^(percent / 100 x octaves)` (+100 %: notes 24 to 96 peaked 0.0061 0.0122 0.0243 0.0484 0.0966 0.1929 0.3847; -100 % mirrors; +73 % gave 0.0802 at note 72). **Vel** is the +50 % fm4's velocity law already has.
**The filter** (FX page, 1 of 4; measured on operator A as Saw D, harmonic levels against the unfiltered 1/k): five types (Lowpass, Highpass, Bandpass, Notch, Morph) and five circuits (Clean, OSR, MS2, SMP, PRD); only Clean was measured. **The shape is two cascaded two-pole stages for 24 dB and one for 12 dB** (fit error 0.1 to 0.3 dB over 12 harmonics); high-, band-pass and notch fit the same way. **The corner is the displayed Freq times `2^((note - 60) / 12)` (Freq<Key +100 %, its default) in the engine's own Hz**, which is 0.9173 of that in the emulator's Hz (the same engine-rate factor as the pitch): 670 Hz at note 45 fitted 260 Hz, at note 57 520 Hz. **Q is 0.5 at Res 0, 0.64 at 0.25, 0.91 at 0.50, about 1.7 at 0.75 and 6 to 7 at 0.93 or above** (the Res knob is 0.0625 a click, reads up to 1.25 and acts like 0.93 above that); fm4 uses `Q = 0.5 / (1 - 0.97 min(res, 0.93))`, good to about 20 % and **not fitted above 0.5** (the harmonics sat near the -100 dB floor). Freq reads 12.0k to 30 Hz at about 4.5 % a click. **Not fitted:** Drive, Circuit, Morph, Freq<Vel, Envelope, the LFO (its default destination is the filter), the pitch envelope, the looped envelope (Initial, Peak, Time<Vel, Mode, Loop Time, Repeat: with Peak at -13 dB the level rose slowly to the sustain, so the decay also climbs, rate unmeasured) and the Partial / Amp / Count additive editor. A first try at the LFO (Main page: Type, Rate 6.32 Hz, Amount 10 %, Range, Synced, Rate<Key, Retrig; Dest A Filter, Dest B Off, Amount A/B, Amt<Vel) turned on a heavy pitch change when its toggle was pressed, so its controls are not understood yet.

### OPERATOR's pitch envelope and the other pages (2026-11-20, looked at, not fitted)

Read off the panel (screenshots; `fm4fit` gained `dump:FILE`, the last play as raw f32, for a time-resolved look). **ENV** has tabs A, B, C, D, Filter, Pitch and LFO (ALGORITHM picks the tab); each has four pages (Times, Levels, Mode, Slopes). **Pitch** page 1: Attack 0.1 ms, Decay 399 ms, Sustain +0.0, Release 50 ms (0.1 a click, in semitones); page 2 (Levels): Initial +12, Peak +12, End 0.0, Time<Vel 0 %; page 3: Mode None, Loop Time 100 ms, Repeat 1/8T; page 4: A, D and R Slope 0.00, +1.00, +1.00. **EDIT page 5 ("Scale")** has per operator **LFO** and **Pitch Env** buttons (KNOB3 and KNOB4 turned the other way, -1, switches one off; both are on by default), and EDIT 6 and 7 the additive Partial, Amp, Count, Repeat and Normalize. **GLO** has four pages: Voices (6), Retrig, Volume, Pan; Time, Time<Key, Time Atk, Tone; Pan<Key, Pan<Rnd, **Interpol, Antialias** (both on); MIDI channel, Tune, Tempo 120. **The result: with the defaults and with Sustain pushed up (+0.6, then a fast 150 clicks) a note held for a second stayed at 201.8 Hz, the same as with the envelope off, so the pitch envelope did not move operator A's pitch in any state tried**; why is not known (the toggle's meaning, an amount elsewhere, or the envelope acting on something else). **Antialias and Interpol do not explain algorithm 1 (tested later the same day):** with B, C and D at -12 dB (116 clicks) and again at 0 dB, Antialias off gave **bit-identical** partials (so it acts on the saw and square waves, not on a sine chain), Interpol off moved them by 0.1 dB, and both off equalled Interpol off; the third stage is as weak with them as without.

### OPERATOR's LFO (2026-11-20, measured, built in `fm4`)

The LFO page (`key:LFO`, then `SELECT -1` for page 1 of 5): **Main** (LFO on/off button, Type, Rate, Amount; Range, Synced, Rate<Key, Retrig; four Osc A to D buttons), **Rate** (the same), **Dest A** and **Dest A+** (Filter button, Amount A, Amt<Vel; Dest B, Amount B), **Dest B**. It is **off by default** (the LFO button is dark). **Turned on with the defaults it vibrato-modulates the pitch of every operator** (the Osc A to D buttons are the targets; the Filter button was not tried). Measured on operator A alone by the zero crossings of each cycle (`fm4fit`'s `dump:` and a script):
- **Depth:** **12 cents a percent**: Amount 10 % swung the pitch +-120 cents, 69 % +-827 (10 clicks of the knob gave 69 %); 100 % is an octave either way (+-1200 measured). Linear.
- **Rate:** 6.32 Hz by default, measured 5.81 Hz, which is 0.9173 of it (the same engine-rate factor as the pitch); 10 clicks gave 30.0 Hz (27.5 measured), about 17 % a click.
- **Types:** Sine, Square, Triangle, Saw Up, Saw Down, S&H, Noise, in that order. By sound: **sine** starts at zero rising, **square** starts high and switches at half a cycle, **triangle** starts at the top falling, **saw up** starts low and rises then drops, **saw down** starts high. S&H holds a random value (the screen shows a step each cycle); Noise looks like a new value every few milliseconds. **S&H and Noise are not fitted** (fm4 draws a value each cycle and sixteen times a cycle).
- fm4 has `LFO ON`, `TYPE`, `RATE` (hundredths of Hz), `AMOUNT` (percent of an octave), `RETRIG` (a phase of its own per note, else one free-running LFO) and `OPERATORS` (a bit mask of pitch targets). `fitted_to_operator_the_lfo` holds depth, rate and the shapes' starts.
- **The filter as a target (2026-11-20):** the Filter button on the Dest A+ page (and the LFO button on the FX page, the same switch: both light together; the operators' Osc A to D buttons and the per-operator LFO buttons on EDIT page 5 are likewise one state). Measured with operator A as Saw D, the 24 dB low-pass at 846 Hz, Freq<Key +100, Res 0.27, only the filter a target: the cutoff, fitted per 40 ms window from the first ten harmonics against two cascaded two-pole stages (fit error 0.1 dB), swung **537 to 800 Hz about 655 at Amount 10 %, plus and minus 0.285 octave: 2.85 octaves at 100 %, a sine geometrically about its own value** (a 76 % run reached 2.9 kHz, 4.4 times the centre). **Amount A** (-100 to 100 %, the Dest A+ page, KNOB2) scales it, and the pitch depth too: 34 % gave 0.20 octave against 0.57, 0.35 of it. fm4 has `LFO AMOUNT A` and bit 4 of `LFO TARGETS` (cutoff recomputed every eighth sample); `fitted_to_operator_the_lfo_on_the_filter` fits the swing from the sound (0.55 octave about 709 Hz, against 0.57 about 712) and Amount A. **Dest B (2026-11-20):** one target from a menu, with its own Amount B (KNOB2, -100 to 100 %). **The menu, in order:** Off, Osc Vol A, B, C, D, X-Fade A/C, X-Fade B/D, Feedback, Fixed Freq, FM Drive, Filter Freq, Filter Q, Filter Res, Morph, Flt Drive, Filter Env, Shp Drive, LFO Rate, LFO Amt, Pitch Env, Volume, Pan, Tone, Time (24 entries). **Osc Vol A measured** (a lone sine carrier, no pitch targets): **gain = 1 + LFO amount x Amount B x the LFO value**: amount 10 % gave a peak of 0.0436 to 0.0533 (0.9 to 1.1 of a clean 0.0484), 69 % 0.0156 to 0.0818 (0.32 to 1.69), 100 % 0 to 0.0968 (0 to 2): bipolar and linear. **Volume** gives the same at 10 % for a lone carrier (indistinguishable there), but at 100 % it went silent after the first 30 ms and stayed so, so its law above small depths is not understood. **Pan** at 100 % moved the mono mix between 0.5 and 1 of its peak. **Filter Freq is not the Dest A law**: with the same saw and filter, 10 % moved the fitted cutoff only +0.03 octave and 100 % up to 3.3 octaves (one-sided, the fit poor in places), so it is neither linear nor bipolar and is not fitted. **FM Drive (measured):** B into A at -12 dB (index 1.58 rad), the index fitted per 20 ms window against a phase-modulation table: **0.52 to 2.64 at 69 %, 0.30 to 2.84 at 100 %, 1.44 to 1.70 at 10 %: the index is multiplied by the same 1 + amount x value as Osc Vol** (windows of 40 ms were too coarse and halved the swing). **Feedback (measured, chaotic):** a lone B in algorithm 11 (B in algorithm 8 shows no feedback at all, whatever its knob: the feedback is only heard there... not understood; A silent, B at 0 dB) with base feedback 0 sounded feedback only while the LFO was positive (clean sine through the other half), up to about the 100 % tone at the peak; with a base of about 61 % and 100 % amount it ranged from none to more than the base and was clean in the negative half: **feedback = base + amount x Amount B x value, as a fraction of full feedback, never below 0** (per-window fits are noisy because feedback is chaotic, so the size above 100 % is not pinned). fm4 has `LFO DEST B` (the menu's numbers) and `LFO AMOUNT B`, **acting on Osc Vol A to D, Feedback (B to D), FM Drive and Volume** (`fitted_to_operator_lfo_dest_b_on_osc_volume`, `..._on_fm_drive_and_feedback`); the other targets are accepted and do nothing. **Not done:** the other Dest B targets (Filter Q, Res, LFO Rate and the rest), Amt<Vel,  Amt<Vel, Rate<Key, Range (Low), the Synced rates, retrig versus free measured on the unit (fm4's retrig is assumed on, as the button shows it lit), and the per-operator LFO toggles on EDIT page 5.

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
- **Not done:** mapping the arpeggiator's controls to the pad and the SOUND panel (the host does not call the new C functions yet); fitting its timing and modes against the stock firmware or Mighty Arp (their rate and gate laws, mode order, the octave order of Random and the first step's timing are **not measured**; the manual's wording was used).

## The instrument editor and the picker (2026-11-18)

**Picker:** FILES lists `*.fm1i` packages with the firmware (your `instruments/` folder and the firmware folders, `bundled-firmware/` and the Pack when they hold any); A on one loads it, shows SOUND with the preview on, and plays it alone when no firmware is loaded (the same as `--instrument`). A bad package keeps the old instrument and says why.

**Global settings and banks (format 2, 2026-11-19).** A package may carry `global N = V` lines (at most 16; ids from `core/src/globals.rs`) and `bank N = NAME` lines (up to 8 banks of 32 voices, a name up to 32 ASCII characters); a package with neither is still written as `fm1i = 1`, and a reader accepts 1 or 2. The globals are what the FM-1 keeps outside a voice, written from the manual:

| id | setting | range | default | does |
|---|---|---|---|---|
| 0 | MIDI CHANNEL | 0-16 | 0 (omni) | 0 plays every channel; 1-16 only that one |
| 1 | EFFECT CHANNEL | 0-16 | 2 | with a MIDI channel set, controllers on this channel reach the package's map and nothing else does (no notes, program change or bend); the main channel wins when both are the same; inert while omni |
| 2 | BEND RANGE | 0-24 semitones | 2 | the wheel's range; 0 turns it off. analog and fm4 take any range; **fm6's synth fixes the wheel at 3 semitones (as msfa does) so it scales down and stops at 3**; an fm6 package that never sets it keeps its 3 |
| 3 | FIXED VELOCITY | 0-127 | 0 (played) | every note at that velocity |
| 4 | MONO | 0-1 | 0 | one note at a time, the last played; letting it go strikes the held note before it again (a stack of 16); turning it on or off lets go of every note; the arpeggiator and the sequencer play as before |

**Banks:** the FM-1's 128 presets are four banks of 32; Program Change stays absolute (0-127 are voices 001-128, observed on the stock firmware), and **Bank Select (CC 0) goes to the same slot in another bank** unless the package maps controller 0. `bank_count`, `bank_name` (a default "BANK n"), `set_bank`. **Effect chain order:** `Instrument::move_fx(from, to)` reorders the chain and the controller map follows each effect (the order is the order in the file). C calls: `fm1ntt_inst_global_*`, `fm1ntt_inst_fx_move`, `fm1ntt_inst_bank_*`. The editor has a GLOBAL group (after the pattern). **Not done: glide** (no engine can slide its pitch yet), a way to reorder effects in the editor, a bank picker in the app, the stock firmware's own CC numbers for these (the effect CCs are unconfirmed, ROADMAP 5c), flash persistence of globals as the FM-1 does it. Tested: 8 core tests (channel routing, fixed velocity, mono return, bend range measured by frequency, chain reorder with the map, banks and Bank Select, round trip, bounds), one for the format, one through the C face, one in the host editor.

**Editor** (a mode of the SOUND panel, not a new panel, so the carousel, the Rec demo's tour and the benchmark's panel workload are unchanged): with an instrument loaded, **B** (keyboard Backspace) or the pad's **Select** (which also works while the preview is playing the pad) opens it. It is generic over what the core describes (`ParamDesc`): the groups are the **engine's** parameters (fm6's 155 patch bytes and model, fm4's, analog's, ...), the **arpeggiator's** and **each effect's**, a group a page at a time (24 rows, never scrolling). **Up and Down** pick, **Left and Right** change by 1, **X and Y** by a tenth of the range (at least 10), **L2 and R2** change group, **A** saves, **B**, Select or Esc leave. The value is clamped by the core, applied under the audio lock, and heard at once (the keyboard's keys still play the preview; the pad's face buttons edit while it is open). **A saves to `instruments/<name>-edit.fm1i`** (a temporary name, then a rename, written through `fm1ntt_inst_to_package`: edits included, the verification note says "edited since"), **never over the file it was loaded from**, and the FILES list is rescanned so the copy is there. New C calls: `fm1ntt_inst_fx_count/name/param_count/param/get/set`. Tested in the host self-test (open, a page's rows, +-1 and the big step, clamping, wrapping, the groups, the save, loading the copy back with its edits, the picker, Select, the pad not playing while it is open, and both layouts drawn with the foot clear); looked at as screenshots on this Mac in Standard and Zoomed. **Not seen on the R36S panel, no pad tried there.**

**Not done:** a reset-to-default key (the core has the default; no button is free), naming a copy, deleting one, editing the voice bank's names, a bar or graph for a parameter, and the "port available" line and Load button on a firmware's page (they need the package's firmware hash matched to a firmware, which the picker alone does not give).

## The step sequencer (2026-11-18)

`core/src/seq.rs`, beside the arpeggiator and by the same rules: a host feature in front of any engine, written from the FM-1 manual's description (docs/HARDWARE.md: **16 patterns of 16 steps, each with its own voice, recording by playing, chain 1 to 16 patterns, step count 1 to 16, gate, swing, transpose -24 to +24; ARP and SEQ exclude each other**), never from a firmware's source; pure timing and note choice on the caller's sample clock, deterministic, bounded, silent and free while off.

- **Steps:** a note 1 to 127 and a velocity 1 to 127; **note 0 is a rest** (so C-1 cannot be a step). A pattern's **voice** is a program number applied when the pattern starts (-1 leaves the voice alone), through the same `set_program` as a MIDI program change.
- **Settings** (ids 0 to 9): PLAYING, BPM, RATE (the arpeggiator's seven), GATE, SWING, STEPS, CHAIN (patterns played in order, then round again), EDIT PATTERN, TRANSPOSE, RECORD. The steps of the pattern named by EDIT PATTERN are ids 100 (voice) and 101 to 132 (note and velocity of each step), which is how the editor and the C face reach all 16 patterns without 544 parameters at once.
- **Recording by playing:** with RECORD and PLAYING on, a note played goes into the step it is nearest to, in the pattern now playing (the sounding step in its first half, the next one after that); it is also played live over the sequence.
- **Exclusion:** starting the sequencer stops the arpeggiator and starting the arpeggiator stops the sequencer (`Instrument`); a package with both on is refused.
- **Package:** `seq N = V` (settings), `step P S = NOTE VELOCITY` (every step that is not a rest, at most 256) and `seqvoice P = V`; unknown ids and steps off the 16 x 16 grid are refused at load; only what differs from the default is written back. **C API** `fm1ntt_inst_seq_count/param/get/set` (group 0 the settings, 1 the edited pattern).
- **Host:** the instrument editor (above) has two more groups, **SEQUENCER** and **PATTERN n** (its voice and 16 steps in two pages), between the arpeggiator and the effects; notes played on the pad or keyboard record when armed.
- **Tests:** order, rests, loops, exact step frames, block-size independence, the gate, transpose and its clamp, swing keeping a pair's length, a three-pattern chain with voices, stop releasing the note, recording landing on the nearest step, id clamping, shrinking the chain and step count while running; through the instrument (voices change, sound, text round trip, exclusion, refusals), the package parser and the C face; the host self-test edits steps.
- **Not done:** a step page of the pad as a grid (the editor lists rows), copying and clearing patterns, saving to flash on the unit's rules ("patterns are saved to flash by themselves": the editor saves to `instruments/` on A), **and any fit to the stock firmware**: its rate, gate and swing scales (the manual's gate 20 to 100 % and swing 50 to 75 %), its recording quantisation and what a chain does at a voice change are not measured; the manual's wording was used.

## MIDI clock sync (2026-11-18)

The arpeggiator and the sequencer each have a **SYNC** setting (0 their own tempo, 1 follow the MIDI clock). The instrument host reads the real-time bytes, which can fall anywhere in a message and never disturb it:

- **Clock `F8`** (24 ticks to a quarter note): with SYNC on, **every Nth tick starts a step** (N is 24, 12, 6, 3, 16, 8 or 4 for the rates 1/4, 1/8, 1/16, 1/32 and the triplets), counted from the last Start, so a step falls on its tick, and an arpeggiator or sequencer started between ticks waits for the next one on the grid. **The tempo is the spacing of the ticks** (smoothed over the last few, restarted after a gap of half a second): it sets the gate; the BPM setting is not used. Swing delays an odd step by a fraction of the step after its tick. With SYNC off the ticks change nothing.
- **Start `FA` and Continue `FB`** (the FM-1 treats them alike, `docs/FM-1-MIDI.md`): the grid and the pattern begin again; a synced sequencer **starts playing** (its PLAYING turns on, which stops the arpeggiator).
- **Stop `FC`:** the sounding note ends and no step falls until the next Start; a synced sequencer stops; held arpeggiator notes are kept.
- Active sensing, reset and the rest are ignored.

**Frame accuracy:** a tick through the lock-free queue (`evq.rs`) carries its own timestamp and the render splits there, so a step lands on its tick's frame whatever the buffer size (tested: a step every 5513 frames at 6 ticks of 918.75, the same within 2 frames at buffers of 512 and 97). A tick given directly with `midi` takes effect at the next buffer, and the spacing it measures is then quantised to the buffers (smoothing covers it; use the queue for a real clock). **The host has no MIDI input yet** (ROADMAP 2): nothing feeds the clock in the app today, so this is reachable from the C API (`fm1ntt_inst_midi`, the queue) and from tests only.

**Not done / not measured:** the clock's source (a port, USB, Web MIDI, BLE), sending a clock out, song position (`F2`), the stock firmware's behaviour (which tick a step falls on after Start, how it follows a tempo change, whether Stop and Start keep the position): the manual's wording was used.

**Stock effect controllers (2026-11-19).** The global `STOCK EFFECT CCS` (id 5) makes controllers 0 to 23 on the effect channel drive the six effects by name, as M-VAVE's table lists (`docs/FM-1-MIDI.md`); off by default, so a package's own map keeps working. Switches are bypasses held in the host, not saved. Scalings are ours and fitted to nothing.
