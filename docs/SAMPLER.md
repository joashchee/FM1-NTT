# Sampler engine (`core/src/sampler.rs`)

Started 2026-11-18 (maintainer: "start on the sampler, relicense whatever you need as MIT"). The other half of what community firmwares do besides
the voice engine (`docs/FM6.md`): PIGEON (chops, 16 drum pads, MP3 or ADPCM songs in a 500 KiB pool, 8 song voices and 4 drum voices), Ravel (8 sample
slots), Rhythmer, Felucca's sampler and X0X's drum samples all play stored samples at a pitch, with loops, in a few voices.

## What is built

MIT, written fresh: **no code is taken from a firmware or from Neumetik** (the licence offer for Neumetik's code was not needed; its `paula.rs` and
`sion/pcm.rs` play samples on the Amiga's and SiON's terms, which are not what a firmware here does, and what was needed was small).

- **IMA ADPCM** (`AdpcmState`, `decode_ima_blocks`, `encode_ima_blocks`): the public 4-bit IMA/DVI codec in WAV blocks, mono. PIGEON and the pool formats
  use ADPCM; the block layout of a firmware's own pool is not yet known, so only the WAV layout is read.
- **WAV reader** (`parse_wav`): 8 and 16-bit PCM and mono IMA ADPCM, one or two channels (averaged), the first `smpl` loop. Bounded (16 Mi frames), an
  `Error` on anything else, fuzzed by cut and by random damage (rule 5).
- **`Sampler`**: one sample on the keys. Pitch is equal temperament about a root key and corrects for the sample's own rate; `Interp` nearest, linear or
  cubic; a forward loop; up to 32 voices with the oldest stolen; a one-shot that plays out (a drum hit) or a loop that stops with a short fade on release;
  a power-constant mixer beyond `poly_limit` voices (amplitude sqrt(limit / n)); a master gain. Deterministic, mono, 16-bit.

9 tests: ADPCM round trip within its noise (under 8% of the signal), the decoder's tables never overrun, WAV formats and loop, WAV rubbish, pitch to 0.5% on
four keys and three interpolators, rate correction, one-shot against loop, the mixer rule and voice stealing, bad settings.

## First measurement: X0X's sampled drums (`core/examples/sampprobe.rs`)

A firmware with sample content that needs no loader. `sampprobe FIRMWARE CHANNEL NOTE,... [--vel V --hold S --secs S --wav FILE]` boots a firmware, plays notes and
prints peak, level, how long it sounds and a pitch hint. **X0X 1.2.2 on MIDI channel 10 (channel 9 zero-based)** sounds notes 36, 37, 38, 39, 41, 42, 45, 46, 49, 50
and 51 (kick, rim, snare, clap, tom, closed and open hat, tom, crash, tom, ride) and is silent on the others; the closed hat (42), open hat (46), crash (49) and ride (51)
are the third-party TR-909 samples its licence notes list (`docs/FIRMWARE-CATALOG.md`), the rest are probably synthesised (not checked). Skiff, Felucca and PIGEON make
no sound from a note on channel 10 at boot, and PIGEON has empty slots until a bank is loaded.

**Velocity law** (all 127 velocities, closed hat and crash, level of a hit by RMS against velocity 127): not `v / 127`. Measured: velocity 1 is -41 dB, 8 is -21 dB,
16 is -15.6 dB (linear: -42, -24, -18), 64 is -4.7 dB (-6.0), 112 is -0.4 dB and the top is flat from about 124. The levels are steps of 1/114 of full scale on the
hat and 1/160 on the crash, so the firmware scales by an integer; a straight-line fit through eleven knots (`X0X_VELOCITY`, `velocity_from_knots`, `Sampler::set_velocity`)
is within 0.7 dB of the measurement for velocity 4 to 127 on both (RMS error 0.11 dB on the hat, 0.45 dB on the crash). A power law is not near (the best, `(v/127)^0.9`, is
0.8 dB out at the hat and fails at the crash's top).

Not yet measured on X0X: choke (a closed hat cutting an open one), retrigger, the voice count and mixer rule, whether notes pitch (a drum map suggests not), the
sample rate and interpolation (a recorded hit's spectrum), and the sample data itself (recorded as untested here; it is third-party data and is not kept).

## What is not measured

**Everything about how a firmware does it.** The defaults (equal temperament, linear velocity, linear interpolation, sqrt(4 / n) beyond 4, a 5 ms release) are
the plain choices, not findings. Nothing ran on a real FM-1.

## Next, in order

1. **Get a sample into a firmware in the emulator.** (Done in part: X0X's drums above need no loading.) PIGEON starts with empty slots ("empty slots stay silent"), and a bank reaches it through its web loader's
   "Send bank" over USB MIDI; that SysEx is not in its public guides (`PIGEON_V05_GUIDE.md`, `PIGEON_CONTROLS.md`), and its source is GPL-3.0 and is
   not read here. Options, cheapest first: (a) a firmware whose factory content is samples (a drum kit: check Felucca's, Skiff's and X0X's drums by playing
   them: `fm6cmp`'s boot and note path works on any firmware); (b) watch the loader's USB MIDI traffic from the browser (a black-box capture, as for the Felucca
   editor protocol); (c) the firmware's own sample slot on the screen path (USB sampling, ENV in PIGEON's guide: "record audio from USB").
2. **Measure**: play a known sample, then fit pitch per key, velocity law, loop and release, interpolation (the sidebands of a high note show it), the
   voice count and the mixer rule, as for fm6 (`--gain`, the level difference to 0.001 dB).
3. **A bank reader** for a firmware's pool (the extractor finds DX7 voice runs; the sample pools need the same: ADPCM blocks, an index, loop points).
4. **MP3** (PIGEON's songs): a decoder is new work (Neumetik has none either).
5. **Wire it**: a C API and a SOUND-panel preview, with `fm6`.
