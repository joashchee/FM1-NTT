# Firmware extractor

`core/src/extract.rs` and `core/examples/fwx.rs` (maintainer, 2026-11-17: "start on the extractor"). The first step of getting a firmware's
data out for a simulation (for example a Neumetik 6-operator engine) instead of running its code on the emulated CPU.

```
cargo run --release --example fwx -- FILE.fwsc [--min-run N] [--dump DIR]
```

It unpacks the package with the vendored `fm1_soc::fwsc::unpack`, prints the identity and the app area's directory (app.bin, VM, PRCT, BTIF, USR),
scans the flash and the unscrambled app area for **DX7 packed-voice runs**, and lists text strings. `--dump` writes the unscrambled app image
(`app.img`, temp name then rename). It reads bytes and runs nothing; the size is bounded (`firmware::check_size`), results are capped, and a
package it cannot read is an `Error` that says why.

## The voice scan

A packed DX7 voice is 128 bytes: six operators of 17 bytes (four rates, four levels, break point, depths, curves, detune and rate scale, sensitivity,
output level, frequency, fine), the pitch envelope, the algorithm (0 to 31), then a 10-character name. `valid_voice` checks every range and that the
name is printable ASCII; `find_banks` reports each run of at least `--min-run` consecutive valid voices, **at any byte offset** (a table in a binary
is not aligned to 128). A run of 4 or more with real names is not chance.

## What it found (2026-11-17, a Mac; nothing was run on a unit)

| Firmware | Voice runs |
|---|---|
| The five bundled Haunted FM-1 packages (generative, not a DX7 engine) | none |
| Felucca 1.8.2, Jangada 1.0.1, Skiff 2.6 (the pack) | one run of **8 voices** in the app area each, the same eight names: TINE EP, GLASS BELL, ROUND BASS, BRASS SECT, ... |
| Omni 0.4.1 (the pack) | none |

So **the data in a firmware is its own small factory bank, not the sound engine**: the engine is code. The stock firmware's 32-voice bank lives in the USR
area, which a package leaves alone (`fwsc.rs`), so it is not in a package at all. The extractor does not yet say how a voice maps to the engine's
behaviour, and nothing has compared a simulated voice with the emulator's audio.

## Not done

- A decoder from a packed voice to named parameters (and to a Neumetik patch), and a DX7 sysex writer.
- Other tables: algorithm tables, wavetables and envelopes that are not in the DX7 layout; the scan only knows that one.
- Comparing a voice played through a simulation with the same voice played by the emulator (the test that says how much of a firmware is data).
- Fuzzing: the vendored reader's own robustness is upstream's (`docs/FIRMWARE.md`).

## Where a 6-operator engine can come from (checked 2026-11-17)

Not from Felucca's tree. In its 1.8.2 source archive (file list and licence headers only were read, rule 3) the FM6 synthesis is one Apache-2.0 file,
`firmware/src/fm6_core.c` (msfa, Dexed's core, ported to integer C), beside GPL-3.0-only files (`eng_fm6.c`, `fm6_vbank.c`, the UI); the MIT
`firmware/engines/` holds the other engines (ANALOG, LOFI, TRIO, WHEEL, NOISE, PHASE), no FM6. The route is **msfa from upstream** (Apache-2.0,
compatible with MIT FM1-NTT and proprietary Neumetik), matched to the FM-1 by comparing audio with the emulator's. `ROADMAP.md` section 5a has the steps.

## Licences

The extractor is MIT and reads a firmware as data. What it extracts is that firmware's: a bank from a GPL firmware is not copied into this repository or
into Neumetik without `docs/LICENSING.md` being settled for it. The table above names what was found, and no extracted data is committed.
