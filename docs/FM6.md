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
`core/examples/fwprobe.rs` drives a firmware's controls and writes its screen as PNG (how the setup below was found); `scripts/fm6-compare.sh` runs the grid; `fm6cmp --no-lfo --no-peg` makes the voice's own vibrato and pitch envelope neutral on the fm6 side only; `--make`, `--put`, `--strip`, `--beat` and `--op-cents` are the second round (below).

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

**What did not match** (all unexplained; hypotheses only, nothing was read to check them):

- **TINE EP at note 84**: level shape differs by 6 to 9 dB over the note (key scaling or rate scaling at the top of the keyboard).
- **BRASS SECT**: at note 60 the emulator decays at -23 to -28 dB/s and fm6 at -12.5; at note 84 fm6's pitch is 1.3 to 3.4 cents off the emulator's and the level shape differs by 28 to 39 dB.
- **SOFT PAD**: the emulator's pitch is 12 cents sharp at note 60 (3 cents at 36, 4 cents flat at 84) where fm6's is within 1; level shape differs by 11 to 26 dB and the decay is different.
  **Checked (2026-11-17), and what it ruled out.** (1) Felucca's own LFO and ENV pages are already neutral for these presets: every LFO destination (PIT, FLT, SHP, AMP) 0%, sync OFF, the ENV destinations
  0%, the mod matrix OFF, amp envelope ATK 1 ms, SUS 100%, REL 1 ms. Pushing every control on them to its minimum (LFO rate, phase and fade; ENV attack, decay and release) and rerunning changed
  nothing that matters (BRASS SECT note 60: level +6.9 dB and spread 9.7 against 9.8, decay -23.2 against -23.3 dB/s; SOFT PAD: spread 26.1 against 26.0). (2) The voices' own vibrato and pitch envelope: BRASS SECT has LFO
  pitch depth 6 (delay 50) and a pitch envelope sweep, SOFT PAD has depth 4 (delay 60), and the voices that **matched** have depth 0 and a flat pitch envelope; but making both neutral **on the fm6 side**
  (`--no-lfo --no-peg`) leaves the comparison unchanged, so they are not the cause. (3) The detune values of the mismatching voices are mild (5 to 10, centre 7), as in GLASS BELL, which matched.
  **What was left, and how it was found (2026-11-17, the editor protocol).** Felucca's editor protocol (`web/EDITOR_PROTOCOL.md` in its source archive: **only that document was read**, not the editor or the firmware)
  is SysEx over USB-MIDI, and `FM6_PUT` (command 69, target 0, the track) writes a 128-byte voice into a track and plays it at once, the format the extractor finds. The emulator has a USB host (`Soc::usb_plug`, `midi_send`,
  `midi_take`); the core now exposes it (`Machine::usb_plug`, `usb_midi_send`, `usb_midi_take`, `usb_product`: opt-in, bounded, plugged before the first `render` because Felucca's USB start gives up without a cable).
  `fm6cmp --make KIND` builds a **purpose-made voice** (one carrier on algorithm 32 unless the kind says otherwise), writes it into the firmware (the reply is `rc 0`), and plays the same bytes on fm6; `--put`
  writes a bank voice the same way (it reproduces the preset's result exactly, so the path is sound), and `--strip lfo,peg,fb,det,kvs,rs,ls` makes parts of any voice neutral on both sides.
  **Isolated, these all match fm6** (constant 3.0 to 3.1 dB lower in the emulator, the gain above): an envelope with a slow attack, a slow decay, a rise, a slow release or several stages (level within
  0.5 dB over 2.6 s); a modulator at ratio 1, 2 or 3 and output level 60 or 80 (the partials' levels within 1 dB); feedback 6; fine and coarse ratios (0.5, 1.5, 3: within 0.1 cent); velocity sensitivity;
  key scaling at notes 36 and 60. Slightly off: feedback 3 (level spread 1.8 dB, decay +1.5 dB/s) and key scaling at note 84 (+0.7 dB).
  **What does not match is the tuning.** (1) **The detune step is larger on low notes in Felucca**: two carriers 7 steps apart (detune 7 and 14) are 10.6, 9.2, 7.55 and 6.3 cents apart at C4, C5, C6 and C7 (a long
  spectrum, the two peaks equal in level); fm6's are 6.29 cents apart at every note (msfa's detune is a fixed number of cents). (2) **Operators 4, 5 and 6 are tuned off operator 1 even at the centre detune**: two
  identical carriers on algorithm 32 give one tone in fm6 and two equal tones in Felucca, operator 4 about +5.65 cents, operator 5 -5.65 and operator 6 +8.5 (constant in cents from C5 to C7; operators 2 and 3
  coincide with operator 1). SOFT PAD with its detune made neutral on both sides still differed (level +9.2 dB, spread 12.9 dB, decay -20 against -5.5 dB/s) until fm6 was given those operator offsets
  (`Voice::with_op_cents`, `fm6cmp --op-cents`): then +2.8 dB, spread 2.5 dB, decay -20.3 against -21.8 (note 60; at note 72 +3.2 dB, 2.7 dB). So **the PAD mismatch is tuning**. **But the same offsets make the real
  presets worse in general** (TINE EP note 60: spread 1.7 to 6.6 dB; BRASS SECT mostly worse; PAD only at note 60), so the offsets are not a model: they probably depend on more than the operator number (the
  algorithm, or the operator's role), or interact with the detune step of (1). **Not found**; the grid with and without the offsets is `scripts/fm6-compare.sh` and `FM6_OP_CENTS=0,0,0,5.65,-5.65,8.5 scripts/fm6-compare.sh`.
  Whether this is Felucca's own tuning (an analog-style spread, a different detune law) or what the stock FM-1 does, nothing here says.

**Not covered**: one firmware (Jangada and Skiff share the bank and were not compared), mono single notes (no chords, so no voice stealing), the pitch wheel, the modulation wheel, the LFO's effect, the 12 voice limit,
the unit itself (nothing ran on an FM-1). The 3 dB gain, the latency and the unexplained voices are the next things to look at.

## The API

`Tables::new(sample_rate)` (8000 to 192000 Hz, else an error), `unpack_patch(&[u8;128]) -> [u8;156]` (every field that indexes a table is clamped, so a bad voice plays
wrongly and never crashes: rule 5, with a 300-voice garbage test), `Voice::new` / `compute` / `key_up` (one note, one 64-sample block at a time, the original's shape),
`Lfo`, and `Synth`: 12 voices of one patch, `note_on`, `note_off`, `set_pitch_bend`, `render(&mut [i16])` mono (msfa's `SynthUnit` without its filter, sustain pedal and
MIDI parser). Output is 16-bit by msfa's mix scale (`to_i16`).

## Not done

- Nothing is wired to the app, the C API, the web build or Neumetik: it is a library module with tests.
- Compared with Felucca's FM6 in the emulator (above), not with M-VAVE's own firmware or a real unit. Its tuning differs from Felucca's in ways not yet modelled (above).
- No stereo, effects or multitimbrality; the sustain pedal, mod wheel and aftertouch are not ported (msfa's player had them in `synth_unit`, not vendored).
- Speed is unmeasured (a block is integer maths and table lookups; the R36S has not run it).
