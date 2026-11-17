# Vendored: msfa (Google's music-synthesizer-for-android)

- **Source:** https://github.com/google/music-synthesizer-for-android (archived), `app/src/main/jni/`
- **Commit:** `f67d41d313b7dc85f6fb99e79e515cc9d208cfff` (2017-09-13)
- **Licence:** Apache-2.0 (`COPYING`, copied unchanged; SHA-256 starts `cfc7749b96f63bd3`). Every file below carries the Apache-2.0 header, Copyright Google
  Inc. 2012 or 2013. The repository has no `NOTICE` file. Checked 2026-11-17 (`ROADMAP.md` section 5a).
- **What is here** (a DX7-class 6-operator FM engine, C++): `dx7note`, `env`, `exp2`, `fm_core`, `fm_op_kernel`, `freqlut`, `lfo`, `log2`, `patch`,
  `pitchenv`, `sin` (each `.cc` and `.h`) and the headers they need: `synth.h`, `controllers.h`, `module.h`, `aligned_buf.h`.
- **What is not:** `synth_unit` (the Android glue: ring buffer, filter, `android/log.h`), the NEON assembly, the Android and Mac apps, the test programs, and
  everything else in the repository. Dexed's fork of msfa is **not** used: Dexed itself is GPL-3.0.
- **Changed (by FM1-NTT, 2026-11-17):** `sin.cc` only: an unused `#if 0` block (a table-free sine "based on ... Basile Graf", whose licence is not stated)
  was removed, and a note of the change added to the file's header (Apache-2.0 section 4). No other file was edited.
- **What it is for:** **a reference, not a build input.** This C++ is not compiled into the app. `core/tests/msfa/oracle.cc` compiles it (with the integer kernel,
  not NEON) and `scripts/msfa-oracle.sh` writes `core/tests/msfa/golden.txt`: table checksums and 96 rendered notes. `core/src/fm6.rs` is a **port to Rust**
  of its integer path (a derived work: Apache-2.0, with msfa's copyright and a pointer here in its header), and `core/tests/fm6_golden.rs` checks the port
  against that file **bit for bit**. Not ported: the NEON kernel, `log2` and the tanh table (nothing in a voice uses them), the experimental sine generators.
- **Updating:** fetch the new commit into an empty folder outside the repository, diff it against this tree, read the diff, copy, change the commit above,
  `NOTICE` and the changelog in the same commit, rerun `scripts/msfa-oracle.sh`, and see the port still passes.
