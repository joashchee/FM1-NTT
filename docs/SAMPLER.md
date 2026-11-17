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

## What is not measured

**Everything about how a firmware does it.** The defaults (equal temperament, linear velocity, linear interpolation, sqrt(4 / n) beyond 4, a 5 ms release) are
the plain choices, not findings. Nothing ran on a real FM-1.

## Next, in order

1. **Get a sample into a firmware in the emulator.** PIGEON starts with empty slots ("empty slots stay silent"), and a bank reaches it through its web loader's
   "Send bank" over USB MIDI; that SysEx is not in its public guides (`PIGEON_V05_GUIDE.md`, `PIGEON_CONTROLS.md`), and its source is GPL-3.0 and is
   not read here. Options, cheapest first: (a) a firmware whose factory content is samples (a drum kit: check Felucca's, Skiff's and X0X's drums by playing
   them: `fm6cmp`'s boot and note path works on any firmware); (b) watch the loader's USB MIDI traffic from the browser (a black-box capture, as for the Felucca
   editor protocol); (c) the firmware's own sample slot on the screen path (USB sampling, ENV in PIGEON's guide: "record audio from USB").
2. **Measure**: play a known sample, then fit pitch per key, velocity law, loop and release, interpolation (the sidebands of a high note show it), the
   voice count and the mixer rule, as for fm6 (`--gain`, the level difference to 0.001 dB).
3. **A bank reader** for a firmware's pool (the extractor finds DX7 voice runs; the sample pools need the same: ADPCM blocks, an index, loop points).
4. **MP3** (PIGEON's songs): a decoder is new work (Neumetik has none either).
5. **Wire it**: a C API and a SOUND-panel preview, with `fm6`.
