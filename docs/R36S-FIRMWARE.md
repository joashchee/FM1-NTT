# Which firmwares can the R36S run? The audit of 2026-11-17

**Short answer: almost none, live.** Of the 195 firmware files measured (the 186 `.fwsc` that the community's repositories publish as releases, and the nine that
FM1-NTT bundles or packs), **two are predicted to keep up with real time on the R36S** (the two languages of one firmware, Compás), **six more are borderline**
(0.6x to 0.9x: they would stutter), 70 are predicted at 0.3x to 0.6x and 117 below 0.3x. Everything runs in the emulator; the R36S is simply about 20x slower than
the Mac it was measured on. **Nothing here was run on a unit.** The predictions are an estimate from one calibration point (below), good to perhaps +-30%.

The full table (195 rows, with each file's SHA-256, release tag, licence as listed, and the numbers) is `r36s/firmware-audit.tsv`.

## The curated list for the R36S

Ordered by predicted speed on the unit, the better of exact timing and main-loop pacing 3 (`docs/PERFORMANCE.md`; pace 3 is not judged by ear). "Exact" is the
emulation with nothing skipped and is the figure to trust; "pace 3" skips the firmware's main-loop waiting and may change its sound.

**Tier A: predicted to keep up (1.0x or more).**

| Firmware | File | Licence (as listed) | Exact | Pace 3 | Repository |
|---|---|---|---|---|---|
| Compás 1.2 (a rhythm machine: cumbia, with a pattern screen) | `compas-1.2-editor-en.fwsc`, `-es` | GPL-3.0-only | 1.03x | 1.11x | TexasRADIO/compas-firmware |

**Tier B: borderline (0.6x to 0.9x): usable for looking at and for recording, stuttering live.**

| Firmware | File | Licence (as listed) | Exact | Pace 3 | Repository |
|---|---|---|---|---|---|
| Sidou 1.20 (a Commodore 64 SID, six voices) | `sidou-1.20.0.fwsc` | GPL-3.0 | 0.80x | 0.59x | dsablic/sidou |
| MIDIMON 0.5 (a MIDI monitor) | `midimon-0.5.fwsc` | MPL-2.0 | 0.70x | 0.71x | ignis32/midimonitor_fm1 |
| B1 Drawbar Organ 5.6 | `SikkenWorks-B1-V5.6.fwsc` | GPL-3.0 | 0.34x | 0.63x | SikkenWorks/B1-Drawbar-Organ |
| Bytemania 1.3.2 | `bytemania-1.3.2.fwsc` | GPL-3.0 | 0.24x | 0.63x | llozd/bytemania |
| RAMBEAVER 745 | `RAMBEAVER-FM-1_745.fwsc` | GPL-3.0 | 0.15x | 0.63x | adamcharleswilsonmusic-code/RAMBEAVER |
| Mocky 0.9.2 | `mocky-0.9.2.fwsc` | GPL-3.0 | 0.16x | 0.61x | charlesvestal/fm1-mocky |

Compás and Sidou were booted and their screens looked at (`--screenshot`): both run and draw. Compás is the only one above 1.0x on both settings, so it is the one worth
putting on a unit first; its licence (GPL-3.0-only) would put it in the Firmware Pack, with its source, if you want it shipped (`docs/FIRMWARE.md`).

**What the firmwares FM1-NTT already ships predict** (none gets above 0.5x): Omni 0.46x (pace 3), Halflight 0.37x, Felucca 0.31x, Midnight 0.30x, UNTRUE STEP 0.25x
(measured on the unit: 0.235x), Voicemail 0.22x, WORN TAPE 0.21x, Skiff 0.19x, Jangada 0.15x.

## How it was measured (and what that does and does not say)

1. **The firmwares**: the open-source, closed-source and "not a synth" lists of awesome-fm-1 (254 entries, 247 GitHub repositories) were read; each repository's
   **latest release** was asked for its `.fwsc` assets, and 186 files from 147 repositories were downloaded into a scratch folder outside this repository
   (nothing third-party is in the repository; the licence column is what the list says, not a licence read: `docs/FIRMWARE-CATALOG.md` is the rule for bundling).
2. **The measurement** (`scripts/firmware-audit.c`, macOS only, one process per file): boot the firmware, run it 2 emulated seconds with the JIT (a warm-up past its
   boot: `docs/BENCHMARKS.md`, workload 2), then count the **host instructions the whole emulator retires per emulated second** (the Mac's `proc_pid_rusage`),
   idle and with **six notes held**, at exact timing and at pace 3. Instructions, not time: the M1 hides the cost that the R36S's in-order core does not.
3. **The prediction**: on the unit UNTRUE STEP ran at 0.178x (exact) and 0.235x (pace 3), at about 2.97G and 1.9G host instructions per emulated second (the JIT
   of that build, on the Mac), so the estimate for any firmware is 0.53G / (its exact figure) and 0.44G / (its pace-3 figure), the larger of the two. The six-notes-held figure is used (a synth costs
   more sounding than idle: for SikkenWorks's drawbar organ, idle said 1.21x and held said 0.63x).

**Not known, and why the numbers are an estimate:** one calibration firmware; the Cortex-A35 may spend its time differently on a peripheral-heavy firmware than on
UNTRUE STEP; the unit was measured on the JIT before the 2026-11-17 second part (5-7% fewer instructions), so the estimates are a little optimistic about the old
build and about right for the new one; **no firmware was played on a unit, and pace 3's effect on the sound was not checked by ear for any of them.**

## What was not measured

- **96 of the 247 repositories have no release with a `.fwsc`** (their binaries are in the repository, on an installer site, or not published): not downloaded.
- **Closed-source and commercial firmwares** (FM-1+VA, Groove OS, M-VAVE's stock V15 and V16): not downloaded.
- **The SMK-37 builds** (32 of the 195 files) were measured like the rest; they are for another instrument (the board shows `SMK37` in the TSV) and are not on the list above.
- **Only each repository's latest release** and its first three `.fwsc` assets.
- **No key beyond six held notes**; a firmware that plays its own sequencer unprompted costs more than idle, and one that waits for a button costs less.

## To repeat it

Build the core (`host/build.sh`), build the tool (the command in `scripts/firmware-audit.c`'s header), download releases into a folder of your own, run
`firmware-audit FILE` on each (4 at a time is fine) and divide as in step 3. A fresh benchmark and Device report from the unit (`docs/BENCHMARKS.md`) with Compás
on it would replace the calibration with a measurement: that is the next step.
