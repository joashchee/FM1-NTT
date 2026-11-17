# Performance: playing firmware live on the R36S

**Status (2026-11-17):** the first real R36S reports (`r36s-report.txt`, a benchmark file, build `2641dd3`) show the core does **not** keep up:
UNTRUE STEP runs at **0.033x** real time in the interpreter and **0.29x** with the native JIT (goal 1x). Nothing below has run on the unit
after that report: every figure for the changes is from an Apple M1 and says "Mac"; the unit's figure is **untested** until a new report comes back.

## What the measurements say

| | R36S (Cortex-A35, 1.5 GHz) | Mac (M1) | ratio |
|---|---|---|---|
| nop loop, interpreter | 1.57 Minsn/s | 33 | 21x |
| UNTRUE STEP, interpreter | 0.033x | 0.89x | 27x |
| UNTRUE STEP, native JIT | 0.29x | 4.67x | 16x |

- **The unit is simply about 15-25x slower than the M1 on this code** (an in-order little core, against a wide out-of-order one). The ratios are
  the same for three different workloads, so there is no sign of a held-down clock or of a bug in the aarch64 JIT. (The governor and the clock are
  now written into the Device report and the benchmark file, to rule it out for certain.)
- **The JIT is worth 9x on the unit** and it ran without a crash, but the app had it **off by default** (the report measured it separately). It is
  now on wherever the CPU supports it (`--no-jit` or `FM1NTT_NO_JIT=1` turns it off).
- **Every firmware keeps the emulated CPU about 98% busy.** All nine firmwares tried (the five bundled, and Omni, Felucca, Skiff and Jangada from the Pack) run
  about 235M instructions per emulated second (of 240M; the last four and UNTRUE STEP counted, the rest have the same interpreter speed on the Mac): the idle skip, which jumps over `idle`, finds nothing to jump. The main loop
  redraws the screen as fast as it can, in flat, unremarkable loops (bit-plotting for text and bitmaps), and only 12% (Omni), 15% (Felucca) and
  31% (UNTRUE STEP) of the instructions run inside interrupt handlers (the sound, the keys, the SPI).
- **The JIT emits about 16-20 host instructions per guest instruction.** Blocks are short (1-3 ops each in that drawing code, because every
  predicated `if { }` ends one) and each costs a limit check, a register load, the chain bookkeeping and a flush. A cheaper block entry,
  registers kept across chained blocks, or longer blocks (trace compilation) are the way to a faster JIT. They are in the vendored core, which is
  upstream's; nothing was changed there.

## Pacing the main loop (built, off for the exact modes)

The firmware's main loop spends most of its time waiting for the emulated SPI and LCD transfers it started, so running it slower costs the
firmware nothing it can see. **Pacing** runs a piece of the main loop (1000-4000 instructions), then moves the clock on for a multiple of it
(2x to 16x), **never past the next timed event, never inside an interrupt handler and never with an interrupt pending**. To the firmware that
is a slower CPU in its main loop only: a legal execution. The sound, the keys and the SPI come when they would. It is deterministic (the same
level gives the same machine). It is not used while core 1 runs (the stock firmware renders on it).

Levels (`Machine::set_pace`, `fm1ntt_pace`, TOOLS > PACE, `--pace`), as measured on the Mac with the JIT on and twelve notes held
(x real time, six firmwares; the first column is exact timing):

| level | piece, stall | UNTRUE STEP | Omni | Felucca | Midnight | Halflight |
|---|---|---|---|---|---|---|
| 0 | off, exact | 2.25 | 1.62 | 2.99 | 2.97 | 2.96 |
| 1 | 2000, 3x | 3.01 | 4.13 | 4.18 | 4.55 | 5.06 |
| 2 | 1000, 6x | 3.05 | 4.91 | 4.09 | 4.86 | 5.36 |
| 3 | 1000, 10x | 3.11 | 5.52 | 4.36 | 5.11 | 5.48 |

The gain stops at about 12x (nothing improves past it) and **the sound breaks above 16x to 24x**: UNTRUE STEP at 24x lost 83% of its level
and its pitch was wrong; Omni lost 26%; at 32x and up some sound is simply not made (3.6 s of audio in 4 s). So the first version's top level
(16x) was one step from a cliff, and level 3 is now 10x. **The cliff is where the firmware's main loop is starved of work it does for the
sound**, so a firmware that does more of its sound in the main loop than these six would break sooner, and nothing detects it: that is the
risk of the feature, and why it stays off for exact work (Rec demo, benchmarks, tests) and why AUTO engages only when the unit is behind.

Checked on the Mac: the RMS level within 5% for five of the six firmwares at every level, and the key-to-sound time unchanged
(Omni 18-23 ms, Felucca 6 ms, UNTRUE STEP 12-28 ms). **Voicemail's level swings** (3693 exact, 4715, 1796, 2737 at levels 1-3): its sound
changes over time, so a four-second RMS does not compare; it was not judged. **Not checked:** by ear, on the unit, or in the first second
after boot, where a key pressed at 1.5 s of emulated time was lost (the firmware boots slower in emulated time).
`AUTO` (the app's default) keeps the exact mode while the unit keeps up, goes one level up a second while it is behind (below 0.97x) and
one down above 3x, so a fast machine never runs paced.

**What it is worth on the unit (a projection, not a measurement):** the Mac's gain applied to the unit's own 0.29x gives about 0.40x for UNTRUE STEP
(3.11 / 2.25 x 0.29) and about 0.3x for Omni (5.52 / 1.62 x ~0.1). Better than 0.29x, still short of 1x: **pacing alone does not make the unit play live.**

### The floor, and why a frame cap would not help

At level 3 the firmware still runs 48M (Omni), 59M (Felucca) and 85M (UNTRUE STEP) instructions per emulated second, against 235M exact. The
interrupt handlers alone are 12%, 15% and 31% of the exact count (29M, 36M, 74M): **UNTRUE STEP is already at its floor**, Omni could gain
at most 1.6x more. A frame-based cap (the firmware's screen at 15 fps) would only move the main loop's share, which pacing already removes,
and would add latency to the firmware's own screens, so it was **not built**. The LCD's update count barely moved under pacing (the firmware is bound by the
emulated SPI, not by compute), which is why slowing its loop is invisible to it.

### What the host draws

The host's redraw is not on the emulation's thread, and costs ~3.5 ms a frame on the unit (about 10% of one of four cores at 30 fps). It
did hold the audio device's lock while it converted the whole screen every frame. Now the screen is converted only when the LCD changed, the firmware's
screen is redrawn at most 15 times a second, the panels 5 times (SOUND 10, for its meters) and at once for anything the person does. That is a power
and a lock-time saving; it does **not** move the emulation's speed. Not measured on the unit.

## What would

In order of what is known:

1. **Run it and read the new report:** the benchmark now measures the interpreter, the JIT and the JIT at pace 1, 2 and 3 on the same firmware,
   the Device report adds the CPU governor and clock (before and just after the runs) and a "firmware, best setting" line, PASS at 1x.
2. **A faster JIT** (the biggest unknown: each of these is worth maybe 1.2-1.5x on top of the one before, measured on the Mac first, and
   checked against the interpreter, which the vendored tests already do op by op): keep the guest registers in host registers across chained
   blocks instead of flushing and reloading at every block exit; cut the block entry check; compile longer blocks through `if { }` and short jumps.
   This is upstream's code (`fm1-emu`, `crates/fm1-core/src/jit.rs`): a local change is recorded in `core/vendor/fm1-emu/VENDORED.md` first, and
   an upstream issue or pull request is the better route.
3. **Skip the wait loops properly:** the pacer is a blunt form of what an exact idle skip would do. Teaching `probe_loop` the SPI-busy poll loops would
   keep the exact timing and the saving, but that is the vendored core as well.
4. **Pick a lighter firmware:** the bundled ones are all heavy redrawers; nothing is known of how the stock firmware compares.
5. **Not an option:** running the main loop in another thread (it shares memory and the clock with the interrupts), and dropping the screen
   (the work is the firmware's own).

## Method (so it can be repeated)

A scratch copy of `core/` outside the repository, instrumented: per-window speed (steady, no warm-up effect: 4.4-4.7x for 8 s), a PC histogram
(`Machine`'s `cpu_view` every 500-900 instructions, interpreter), a disassembly of the hot loops with the vendored decoder, the JIT's compiled
blocks dumped and disassembled with `llvm-objdump` (a 1-op block: about 10 host instructions of its own for one guest instruction), helper
call counts (2.3% of instructions use a helper; their time is small), instructions inside interrupt handlers, and the pacing experiment above.
The sampling profiler (`sample`) is blocked by the sandbox, so none of this is a CPU profile of the unit.
