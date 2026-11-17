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

## The API

`Tables::new(sample_rate)` (8000 to 192000 Hz, else an error), `unpack_patch(&[u8;128]) -> [u8;156]` (every field that indexes a table is clamped, so a bad voice plays
wrongly and never crashes: rule 5, with a 300-voice garbage test), `Voice::new` / `compute` / `key_up` (one note, one 64-sample block at a time, the original's shape),
`Lfo`, and `Synth`: 12 voices of one patch, `note_on`, `note_off`, `set_pitch_bend`, `render(&mut [i16])` mono (msfa's `SynthUnit` without its filter, sustain pedal and
MIDI parser). Output is 16-bit by msfa's mix scale (`to_i16`).

## Not done

- Nothing is wired to the app, the C API, the web build or Neumetik: it is a library module with tests.
- Not compared with the emulator's audio, so nothing says yet how the FM-1 differs from msfa.
- No stereo, effects or multitimbrality; the sustain pedal, mod wheel and aftertouch are not ported (msfa's player had them in `synth_unit`, not vendored).
- Speed is unmeasured (a block is integer maths and table lookups; the R36S has not run it).
