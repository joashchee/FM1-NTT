# FM6: a 6-operator engine (msfa, ported)

`core/src/fm6.rs` (maintainer, 2026-11-17: "do C": build the engine in FM1-NTT first, under MIT, with msfa vendored as Apache-2.0). It is **msfa's integer
path ported to Rust**: a DX7-class engine (6 sine operators, 32 algorithms, feedback, DX7 envelopes, pitch envelope, LFO, key and velocity scaling) that plays a
DX7 packed voice, the 128 bytes the extractor finds (`docs/EXTRACTOR.md`). **It is the DX7's engine as msfa models it, not the FM-1's**: how far the FM-1
differs (its 32 algorithms are likely the DX7's, but its envelope and level maths, sample rate and bit depth are not measured) is the next step
(`ROADMAP.md` section 5a).

## Where it came from

`core/vendor/msfa/` is Google's C++ at a pinned commit, kept as the reference and not built into the app (`VENDORED.md`: source, commit, licence, the one change).
`core/src/fm6.rs` is the port (Apache-2.0, a derived work, its header says so). The Felucca firmware ships its own copy of the same engine beside GPL-3.0 files; **this port
is from Google's repository and none of Felucca's source was read** (only its file list and licence headers, rule 3).

## Checked against the original, bit for bit

`scripts/msfa-oracle.sh` compiles the vendored C++ as it is (integer kernel, not NEON) and writes `core/tests/msfa/golden.txt`: checksums of the sine, exp2 and
frequency tables, and 96 notes (32 original voices, one per algorithm, at three note, velocity and pitch-bend settings) of 60 blocks each, with a release at block 30:
a hash, the sum of |samples| and four samples. `core/tests/fm6_golden.rs` runs the port on the same voices and compares everything. **Broken on purpose
two ways** (one algorithm's table entry; the envelope's rate scale) **it fails**. The golden file is checked in, so CI needs no C++ compiler; rerun the script after
changing the vendored files or the oracle.

The tables use `sin`, `cos`, `pow`, `exp2` from the host's libm; **they matched on the Mac (macOS libm) and in Debian 12 on aarch64 (glibc)**. Not run on x86-64.

## Compared with the emulator's audio (2026-11-17)

`core/examples/fm6cmp.rs` plays a note on a firmware in the emulator and the same voice, taken from the firmware's own bank, on fm6, and compares them;
`core/examples/fwprobe.rs` drives a firmware's controls and writes its screen as PNG (how the setup below was found); `scripts/fm6-compare.sh` runs the grid; `fm6cmp --no-lfo --no-peg` makes the voice's own vibrato and pitch envelope neutral on the fm6 side only; `--make`, `--put`, `--strip`, `--partials`, `--peaks`, `--beat`, `--tuning` and `--dtun` are the second round (below).

**What was compared, and what it is not.** The emulator ran **Felucca 1.8.2** (GPL, from the Pack, read as data: no source was read), not M-VAVE's stock firmware (closed, not here).
Felucca's FM6 engine is itself msfa ported to integer C, so this checks **fm6 against msfa as Felucca ships it**, not against the FM-1's factory engine. Setup, found with the screen: the ALGORITHM
encoder turns to part 2 (FM6), PRESETS picks TINE EP, BELL, FM BASS, BRASS or PAD (bank voices 0 to 4: TINE EP, GLASS BELL, ROUND BASS, BRASS SECT, SOFT PAD), MIDI channel 2 plays part 2, and
**chorus, delay and reverb on the FX page are turned to 0**: with them on, the emulator showed chorus sidebands (about 20 to 40 Hz either side of the note, 45 dB down) and a reverb tail 20 dB
or more above fm6's release, and none of that is the engine.

**What matched** (TINE EP, 261.6 Hz, effects off: level 3 dB lower in the emulator, constant to 0.2 dB over 1.4 s in the best cases):

| | GLASS BELL, ROUND BASS (every note and velocity tried) | TINE EP (notes 36 and 60) |
|---|---|---|
| pitch, emulator against fm6 | within 0.1 cent | within 0.3 cent |
| level, fm6 minus emulator | +2.8 to +2.9 dB, spread 0.2 to 0.4 dB over 1.2 s | +3.2 to +3.6 dB, spread 0.4 to 2.0 dB |
| decay while held | within 0.4 dB/s | within 2 dB/s |
| release, 0.3 s after the key goes up | 0.0 to 2.9 dB | 2.7 to 3.0 dB (the same constant) |
| second harmonic against the fundamental | ROUND BASS 0 dB; GLASS BELL's partials are not harmonics of the note, so its harmonic figures (up to 30 dB) mean nothing | 0 dB |
| velocity 40 against 100 | the same level difference | the same level difference |

So the pitch, the operator frequencies and algorithms, the envelopes (the amp envelope, its release slope to the 16-bit floor) and the velocity scaling are the same engine; **the one systematic difference is the
level: Felucca's output is about 3 dB (2.8 to 3.6) below fm6's** (a gain stage; not looked for). The emulator's note starts 7 to 9 ms after the MIDI byte (the firmware's own latency), and its noise floor is higher
(harmonics under about -90 dBFS differ by tens of dB and mean nothing).

**What did not match at first, and why** (first grid, msfa's tuning: TINE EP at note 84, BRASS SECT and SOFT PAD): **tuning**, and only tuning. Found by writing test voices into the firmware (below), modelled
as `fm6::Tuning::Felucca { dtun }`, and then the same grid matches everywhere (see "The tuning model").

**What was ruled out on the way** (2026-11-17): Felucca's own LFO and ENV pages (already neutral for these presets, and pushed to their minimum with no change); the voices' own vibrato and pitch envelope (made
neutral on both sides, no change); the operator envelopes (a slow attack, slow decay, rise, slow release and several stages written as test voices all match fm6 within 0.5 dB); FM modulation index, feedback
6, the fine and coarse ratios, velocity sensitivity and key scaling (all match one at a time).

**How the test voices were written (the editor protocol).** Felucca's editor protocol (`web/EDITOR_PROTOCOL.md` in its source archive: **only that document was read**, not the editor or the firmware)
is SysEx over USB-MIDI, and `FM6_PUT` (command 69, target 0, the track) writes a 128-byte voice into a track and plays it at once, the format the extractor finds. The emulator has a USB host (`Soc::usb_plug`,
`midi_send`, `midi_take`); the core now exposes it (`Machine::usb_plug`, `usb_midi_send`, `usb_midi_take`, `usb_product`: opt-in, bounded, plugged before the first `render` because Felucca's USB start gives up
without a cable). `fm6cmp --make KIND` builds a **purpose-made voice**, writes it into the firmware (the reply is `rc 0`) and plays the same bytes on fm6; `--put` writes a bank voice the same way (it reproduces the
preset's result exactly, so the path is sound); `--strip lfo,peg,fb,det,kvs,rs,ls` makes parts of a voice neutral on both sides; `--partials`, `--peaks` and `--beat` measure frequencies from a 1.5 s spectrum
(about 0.02 Hz). Writing a voice into a track keeps that part's preset macros, which is what made the first measurements misleading (below).

## The tuning model

**Felucca tunes its operators differently from msfa in two ways, one of them a macro.** Measured by writing six-carrier voices (operator k at ratio k, so each partial is one operator) at detune 0 to 14 and notes 36 to
108, the frequencies read from a long spectrum, against fm6 on the same bytes:

1. **The detune step depends on the note, and it does not depend on anything else** (the same at DTUN 0, and the same for every operator, modulators included: pairs with a detuned modulator match to 0.13 cent).
   Cents a step: 1.98 at note 36, 1.55 at 48, 1.20 at 60, 0.905 at 72, 0.68 at 84, 0.50 at 96, 0.37 at 108; msfa's is 0.9017 at every note (it equals Felucca's at about note 72). ln(step) is a
   quadratic in the note to within 0.008 cent a step (`felucca_detune_step_cents`).
2. **The DTUN macro spreads the carriers.** The FM6 PATCH page (EDIT, twice) has DTUN, 0 to 127 (shown as a percent). It tunes each **carrier** off by a unit times 0, +1, -1, +2, -2, +3 by the carrier's
   place among the algorithm's carriers in operator order (algorithm 32: operators 1 to 6; algorithm 5: operators 1, 3 and 5; **modulators none**, their sidebands are exact multiples). The unit is 0.0945 cent
   a DTUN step (0.0942 to 0.0946 measured at 12, 24 and 48) and stops growing at 64 steps (6.05 cents); at DTUN 0 it is exactly zero. **The default PAD preset has DTUN 30 (shown 24%); TINE EP, BELL, FM BASS and
   BRASS have 0.** The first measurements were made on part 2 as it boots, on the PAD preset, so they had DTUN 30 in them: that was the "per-operator offset" of the earlier notes, wrongly taken for Felucca's own tuning
   and for something depending on the operator number.

`fm6::Tuning::Felucca { dtun }` is this model (default `Tuning::Msfa`, bit for bit the original: the golden test still passes). **Held out** (detune values and notes not in the fit: 45, 55, 63, 69, 75, 87, 100 at
detune 0 to 14): 84 partials within 0.15 cent of the emulator, most within 0.05. **On the real presets** (`FM6_TUNING=felucca scripts/fm6-compare.sh`, DTUN 30 for PAD, 0 for the others), 30 runs (five presets, notes
36, 60, 84, velocity 40 and 100): the mean level spread goes **from 7.6 dB with msfa's tuning to 0.8 dB** (worst 3.3 dB), the strongest partial agrees within 0.1 cent everywhere (it was 1200 cents out, an octave,
for PAD and BRASS), and the level difference is a constant 2.8 to 3.2 dB, the output gain. The model is **fitted to measurements, not read from the firmware**, for ratio mode only (fixed-frequency mode is
unmeasured), at 44.1 kHz, on one firmware; whether the FM-1's stock firmware has the same law is not known.

**Not covered**: other firmwares than Felucca, Jangada and Skiff (SLOOP, Melodee and the rest of the Felucca family are not in the pack or not run), mono single notes (no chords, so no voice stealing), the pitch wheel, the modulation wheel, the LFO's effect, the 12 voice limit,
the unit itself (nothing ran on an FM-1). The output gain (a constant 2.8 to 3.2 dB below fm6's: not looked for) and the firmware's 7 to 9 ms latency are the other differences.

## Jangada and Skiff (2026-11-17)

Both are Felucca forks (Jangada by zednaked, Skiff 2.6 by illuminus5 from Felucca 1.1.5, as the Pack's `SOURCES.md` say), run the same way (`FM6_FIRMWARE=jangada|skiff scripts/fm6-compare.sh`, optionally `FM6_TUNING=felucca`),
and answer the same editor protocol (`FM6_PUT` replies `rc 0`; Jangada's USB product string is "Jangada", Skiff's "Felucca").

**Reaching the FM6 engine differs.** Skiff: part 2 is FM6 at boot (its list screen says FM6, patch F5), PRESETS steps F1 to F8 (it names them FM BRASS, FM BASS...). Jangada: part 2 boots on its **DIGITAL** engine (the four-operator
one Felucca replaced with FM6 in 1.0: IDX, MDEC, FB, REL), and **FM6 is far down the preset list**: 55 single clicks of PRESETS reach TINE EP (F1), after ANALOG, DIGITAL, PHASE, LOFI, SAMPLE, VOICE, TRIO, WHEEL and
GRAIN; its FM6 presets are TINE EP, BELL, FM BASS, BRASS, PAD, MARIMBA, ORGAN, PLUCK and its own CARVAO. A big turn of an encoder is not taken one click for one (55 in a single step landed on a GRAIN preset), so
`fm6cmp --encn NAME:N` turns it one click at a time. Both have the same FX page (chorus, delay and reverb on knobs 2 to 4; Skiff also has a second page, a send delay, not needed here: the tail after the
key goes up is under -90 dB), and MIDI channel 2 plays part 2.

**The engine is the same engine, and so is the tuning.** At DTUN 0 the detune step measured 1.978, 1.550, 1.197, 0.905, 0.677, 0.5005 and 0.367 cents at notes 36, 48, 60, 72, 84, 96 and 108 in **all three** firmwares, equal to
four decimals; the DTUN spread is the same (only the F5 PAD preset has it, 30, unit 2.83 cents, the pattern 0, +1, -1, +2, -2, +3 on the carriers); the output is the same 3.0 dB below fm6; and `Tuning::Felucca` needed
no change. Over the five presets all three have (TINE EP, BELL, FM BASS, BRASS, PAD; notes 36, 60, 84; velocity 40 and 100: 30 runs each):

| | mean level spread, msfa tuning | with `Tuning::Felucca` | worst | level fm6 minus emulator | strongest partial |
|---|---|---|---|---|---|
| Felucca 1.8.2 | 7.6 dB | **0.65 dB** | 2.5 dB | +2.8 to +3.2 dB | within 0.2 cent |
| Jangada 1.0.1 | 7.6 dB | **1.0 dB** | 4.3 dB (SOFT PAD, note 84) | +2.6 to +3.3 dB | within 0.1 cent |
| Skiff 2.6 | 7.5 dB | **0.65 dB** | 2.5 dB | +2.8 to +3.2 dB | within 0.7 cent (GLASS BELL, note 36) |

**What they add, F6 to F8** (both firmwares list them; Felucca's list has five): NYLON PICK matches (level +2.7 to +2.8 dB, spread 0.3 to 1.3 dB). **WOOD BARS** matches over its audible note (the curves are within about 2 dB
for 0.4 s): its "spread" of 70 dB is the decayed tail, where one engine is at -112 dB and the other at digital silence, so the metric means nothing for a voice that dies in half a second. **DRAWBARS first looked
1 to 3 dB too quiet in the emulator, and that was the effects, not the engine**: its preset (F7, "FM ORGAN") has **DIST 8%** on its FX page (SOFT, tone 0), and my effects-off recipe zeroed chorus, delay and reverb
but not distortion. The same plain test voice written over F7 was 3.3 dB quieter at note 36 than over any other preset (+6.3 dB against +3.0), the loss shrinking with the note (+4.2 at 60, +4.1 at 84) and not
depending on the voice's output level (a soft distortion with a tone control acts as a filter at low drive); with distortion at 0 it is **+3.0 dB at every note**, for the plain voice and for DRAWBARS. So every preset
now matches at the same 3.0 dB. (The earlier six-carrier and single-operator tests all ran on presets with DIST 0, which is why none showed it.) **Lesson: effects off means distortion, chorus, delay and reverb**
(`FX` page, knobs 1 to 4; `scripts/fm6-compare.sh` does all four), and other presets' FX pages should be looked at when a level is off.

**Differences between the three, none in the engine:** the note-on latency (first sample over 0.002: Felucca 7 to 9 ms, Skiff 7 ms, **Jangada 16 ms**), and what the panel offers. Nothing else was found. Still the emulator, one note at a time,
and not the stock firmware or a unit.

## The API

`Tables::new(sample_rate)` (8000 to 192000 Hz, else an error), `unpack_patch(&[u8;128]) -> [u8;156]` (every field that indexes a table is clamped, so a bad voice plays
wrongly and never crashes: rule 5, with a 300-voice garbage test), `Voice::new` / `compute` / `key_up` (one note, one 64-sample block at a time, the original's shape),
`Lfo`, `Tuning` (msfa's, or Felucca's measured: above), and `Synth`: 12 voices of one patch, `set_tuning`, `note_on`, `note_off`, `set_pitch_bend`, `render(&mut [i16])` mono (msfa's `SynthUnit` without its filter, sustain pedal and
MIDI parser). Output is 16-bit by msfa's mix scale (`to_i16`).

## Not done

- Nothing is wired to the app, the C API, the web build or Neumetik: it is a library module with tests.
- Compared with Felucca's FM6 in the emulator (above), not with M-VAVE's own firmware or a real unit. Its tuning differs from Felucca's in ways not yet modelled (above).
- No stereo, effects or multitimbrality; the sustain pedal, mod wheel and aftertouch are not ported (msfa's player had them in `synth_unit`, not vendored).
- Speed is unmeasured (a block is integer maths and table lookups; the R36S has not run it).
