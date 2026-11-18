# Performance: playing firmware live on the R36S

**Status (2026-11-17, R36S benchmark workload 2): live play is not in reach with the current core. Steady state, UNTRUE STEP, after a warm-up past
its boot: interpreter 0.036x, native JIT 0.177x, JIT with pacing 1 / 2 / 3: 0.240x / 0.233x / 0.241x (goal 1x; min and max within 5% of the median).**
Every earlier unit figure here (0.29x, the Device report's 0.64x at pace 3, the first benchmark's 1.37x) timed the firmware's cheap first seconds: a firmware's
boot and idle cost little and it gets heavier after that (on the Mac the interpreter goes 1.08x over a 1 s run, 0.78x over 3 s, 0.76x over 6 s). The benchmark
now warms up first (workload 2); **the Device report still runs 3 s from a cold start and overstates (0.64x): to change.** Pacing, which looked worth 1.4x on the
Mac and in those early figures, is worth only 1.35x over the unpaced JIT in steady state, and levels 1 to 3 are the same, so **pacing alone does not get near 1x.**

**The gap is about 4x at the best setting** (repeated 2026-11-18: JIT 0.185x, pace 3 0.246x; see the section on the counters). The projections further down (0.34x, 0.47x with pace 3) were made from the early figures and are superseded.

**Tried and dropped (2026-11-17, Mac): `lto = "fat"` with `codegen-units = 1` for the core**: no gain (nop loop 32.1 / 31.7 Minsn/s, UNTRUE STEP JIT 4.80x / 4.66x, interpreter 0.700x / 0.699x; within noise). Not adopted, and not worth measuring on the unit.

**What the figures imply (arithmetic, not a profile of the unit):** UNTRUE STEP unpaced runs about 0.177 x 235M = 42M guest instructions a second, about 36 unit cycles each: at about 12 host
instructions per guest instruction (above) that is an IPC near 0.3 on the in-order A35, so the JIT's code is stalled on something (loads and stores to the register file in memory, the RAM and XIP checks,
a mispredicted indirect branch at each block exit) as much as it is long. At pace 3 the unit executes only about 20M guest instructions a second yet goes faster in emulated time, since pacing removes the
main loop's work and what is left is the interrupt handlers' flat, many-block code. **Not known:** whether the unit is spending its time compiling (the 105+ handler regions), in block entry and exit, or in memory stalls: that needs
a profile of the unit, which does not exist (the Mac's sampler is blocked here and the unit has no `perf`).

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

### What the interrupt handlers spend their time on (kernel replacement tested, not worth it)

The floor is the interrupt handlers, so the question was whether a few hot routines in them could be replaced by native code (as the vendored
SoC already does for the hardware sample-rate converter). Sampled on the Mac, the interpreter, twelve notes held, the program counter taken only
while a handler runs:

| firmware | time in handlers | distinct 128-byte regions | top 12 regions hold | notes |
|---|---|---|---|---|
| UNTRUE STEP | 38% | 105 | 30% | flat |
| Omni | 28% | 70 | 66% | one ~1 KB routine (`0x8100`-`0x8400`) is 42% |
| Felucca | 18% | 156 | 50% | flat |
| Skiff | 23% (core 1 released) | 157 | 57% | flat |

**Almost none of that code is shared between firmwares:** looking for the hot regions' exact bytes (24-byte windows) in the other firmwares' images found
0% for Omni, 0% between UNTRUE STEP and Felucca, Skiff or Omni, and 4-15% only between Felucca and Skiff (one is derived from the other) and between UNTRUE STEP and WORN
TAPE (the same author). There is no common SDK routine to replace. So a native kernel would be **per firmware**, found by hashing its code, and only Omni has a
routine big enough to matter: replacing it would remove perhaps a quarter of what Omni still executes at pace 3, 1.3x at most. For the others the profile is flat and the
gain would be a few percent per routine. **Not built.** This also means Neumetik's chip and FM code has nothing here to stand in for: the FM-1's FM is firmware code
on the emulated CPU, not a chip. The faster JIT remains the only general lever.

## The JIT (2026-11-17): 14-18% fewer instructions, built and checked

The M1 hides the JIT's overhead (a wide core runs 5 instructions a cycle, and the real-time ratio hardly moves), the unit's in-order A35 does not,
so the measure here is **host instructions per guest instruction** (macOS reports instructions retired for the process: `proc_pid_rusage`), with the real-time ratio
beside it. Before the change it was 12.5-17.7 on six firmwares, and 0.7-3.8 of that was Rust around the JIT call (sampled with the counter's own cost taken out), so the JIT's own
code is the cost. Where it went, from the compiled blocks: operands copied through scratch registers (a three-instruction add was six), the RAM and XIP range checks
rebuilding the same constants (6 instructions, twice for a read of XIP), a compare through a built immediate, and the block-boundary bookkeeping.

Changed (`core/vendor/fm1-emu`, `VENDORED.md` has the record): the register ops and the compares work on the host registers that hold the guest ones;
a load or store takes its address from the operand; the range check is an `eor` and an immediate length. Not changed: the structure (a limit check and a flush at
every block, guest registers not kept across blocks).

| firmware | host insns per guest insn before / after | real time on the M1 before / after |
|---|---|---|
| UNTRUE STEP | 14.73 / 12.64 (-14%) | 4.69x / 5.03x |
| Midnight | 14.17 / 11.64 (-18%) | 4.63x / 4.88x |
| Halflight | 12.48 / 10.39 (-17%) | 5.50x / 6.02x |
| Omni | 17.68 / 15.25 (-14%) | 4.42x / 4.61x |
| Felucca | 13.60 / 11.37 (-16%) | 4.74x / 5.11x |
| Skiff | 14.95 / 12.60 (-16%) | 3.14x / 3.68x |

Checked: the vendored tests (72 with a new one) on the Mac and in a Debian 12 arm64 container; the new test (every width and a ring of addresses at both regions' edges and far outside, compares at
their immediates' limits) fails when the range check is made wrong three ways; and the JIT and the interpreter give **the same audio, screen, pc and instruction count after 6 s** on all six.

Tried and dropped: compiling a plain `if { }` with its then-part in line, as the if-else block already is. It works (identical output) and did nothing: the then-parts of the hot `if`s hold
jumps and other `if`s, which the compiler leaves out, and the 800-odd sites it did take were not the hot ones (14.80 against 14.67 instructions).

**What it is worth on the unit (a projection, not a measurement):** if the A35 follows the instruction count, UNTRUE STEP's 0.29x becomes about 0.34x, and about 0.47x with pace 3 (x1.4). Still short of 1x.
The rest of the JIT's cost is structural: the block's limit check and flush at each boundary (about a quarter of the guest ops are branches, 0.27 blocks entered per guest instruction) and
guest registers kept in memory between blocks. Keeping them in host registers across chained blocks, or compiling longer blocks, would take the sound of a rewrite of the compiler, for perhaps another 15%.

## The JIT, second part (2026-11-17): 5-7% fewer instructions, and where the rest is

Measured as host instructions per guest instruction on the M1 (UNTRUE STEP, pace 0): **12.62 before, 11.87 after** (Halflight 10.37 to 9.65, Midnight 11.66 to 10.81, WORN TAPE 12.06 to 11.32, Voicemail 12.15 to 11.35).
What changed (`core/vendor/fm1-emu/VENDORED.md`): a block's budget is taken off once at its entry (2 instructions less per block), the float ops and the immediates use the held registers and
immediate forms, and cold code (the prologue, the chain exits' slow halves) sits after the hot code.

**Where the time is** (a sampling profile of the Mac, CPU time, UNTRUE STEP, interpreter off): at pace 0 the compiled blocks are 66% and the rest is the SoC (peripheral reads and writes through helpers, the machine loop);
at pace 3 the blocks are 54%, `run_blocks` (the machine loop, with the JIT entry and the peripherals' advance inlined) 12.5%, and the peripherals' reads, writes, interrupt checks and the LCD another 15%.
**Why compiled code leaves**: of 2.09M exits in 3 emulated seconds (pace 0), 1.74M (83%) are `stop`, an interrupt that became deliverable, one every 108 guest instructions at pace 3 (577,000 a second, from the SPI and the sound);
11% are the limit or a computed jump with no body in the table, 5% a bus access that made a tick due, and 0.1% an unlinked chain. Each is a trip through Rust of well over a thousand host instructions. 99% of guest instructions run compiled,
and one block in 2,400 was given up on, so the interpreter is not the cost.

**Measured on the unit (2026-11-17, benchmark workload 2, UNTRUE STEP, build `70384b1` plus these changes): the JIT is 4.4% faster unpaced (0.1781x to 0.1859x), 4.0% at pace 1 and 2 (0.2390x to 0.2485x, 0.2296x to 0.2388x) and 5.5% at pace 3 (0.2349x to 0.2478x);
the interpreter is unchanged (0.0365x).** A little under the 6% fewer host instructions measured on the Mac, so **the cold-code layout (aimed at the A35's small instruction cache) added nothing visible**: instruction-cache misses are not a major cost there. The audit's calibration
held: 0.1859x at 2.79G host instructions per emulated second and 0.2478x at 1.76G give 0.52G and 0.44G, against the 0.53G and 0.44G used for the predictions (`docs/R36S-FIRMWARE.md`).

**What the unit shows** (benchmark with `# jit` lines, pace 3, one second; the build before this part, unchanged by it): 98.9% of instructions compiled; 240 blocks compiled in the second (under 5% of it); 192,000 entries; 21M guest instructions a second.
The generated code is not the problem alone and compiling is not the problem: the unit is simply about 20x slower than an M1 on all of it.

**What is left, and what each is worth** (measured on the Mac, to be confirmed on the unit): a compiled path for interrupt delivery (the Rust trip per interrupt is the largest single item, perhaps 10-15% at pace 3, and a risky change to the machine loop);
guest registers kept in host registers across chained blocks (about 2-3 of the 10 hot instructions per block); longer blocks through `if { }` (the earlier attempt gained nothing); loads and stores with their slow path out of line.
None of these is 4x, and together they are perhaps 1.3x on top of this part.

## Using the other cores (2026-11-17: looked at, two measurements built, no gain claimed)

**How the cores are used.** The whole emulated machine (both emulated cores and every peripheral) runs on **one host thread**: SDL's audio callback in the app (`audio_step`),
the job thread in a Device report or benchmark. The main thread draws and takes input (about 10% of a core at 30 fps). The unit's four Cortex-A35s share one cluster and one
clock, so two of the four are idle. Nothing set thread affinity or priority before this. (The GPU, a Mali-G31, does not help: the work is one dependent instruction stream.)

**What could use them, from this document's own figures** (arithmetic and the profile above, not measurements of the unit):

| Idea | At most | Risk |
|---|---|---|
| Pin the emulating thread to the last core, the main thread to the first | a few percent (a guess) | low; **built, off by default** (below) |
| Compile JIT blocks on a spare core | 5% (240 blocks a second is under 5% of the time) | medium: upstream's code cache, chain patching |
| Move the LCD pixel feed to a worker | under the 15% that peripherals and the LCD take together; the per-byte SPI register path stays | medium, vendored |
| An optimizing JIT tier compiled on a spare core (registers kept across blocks, longer blocks) | about 1.3x, as the list above says, and it makes the compile cost affordable | high effort |
| Run the main loop and the interrupt handlers on two threads | the only one of the size needed: the interrupt handlers are 12% (Omni), 15% (Felucca) and 31% (UNTRUE STEP) of the instructions, 29M, 35M and 73M a second against the unit's about 42M unpaced, so Omni and Felucca might keep audio at real time on one core, UNTRUE STEP not | very high: shared memory, interrupt masking would have to become a lock, and it breaks the deterministic core (CLAUDE.md rule 4); at most a separate, non-deterministic fast mode, as a spike in a scratch copy first |

**Built (host only, nothing in the core):**

- **CPU time by thread** (`host/threads.h`: `/proc/self/task/<tid>/stat`, utime + stime in 10 ms ticks; "not available" off Linux). The Device report has a
  `threads` line over all its firmware runs ("over 12.3 s: fm1ntt-job 98.2% (cpu 3), main 9.1% (cpu 0), ...; all N threads X% of 4 cores") and a `thread pin` line;
  the benchmark file has the same as `# threads` and `# thread pin` comment lines (no result lines: `bench-compare.py` is unchanged). **What to read:** the busiest thread should be the
  emulating one at about 100%; the main thread's share is what competes with it if the scheduler puts them on one core; a total well over 100% says another core is already in use.
- **Thread placement, measured:** one warmed-up machine, then six windows that alternate between the scheduler's free choice and the emulating thread pinned to the last core with
  the main thread on the first, in the order free-pinned, pinned-free, free-pinned. The Device report's `thread placement` line (best setting) and the benchmark's `# thread placement` line (JIT, pace 3) give
  both medians and ranges and say "pinned is faster in every window", "slower in every window" or "the ranges overlap: no clear difference". **It needs a job thread** (TOOLS > Device report or Benchmark): a
  command-line run is on the main thread and says so. It adds about 20 s to each (one warm-up and six windows).
- **`--pin` (or `FM1NTT_PIN=1`)** pins for real: the audio thread (and the job thread) to the last core, the window's thread to the first. **Off by default until the unit's report says it helps.**

**Checked:** the parser and sums in the self-test on any system; on Linux the self-test also reads `/proc`, pins the calling thread and puts it back, and runs the placement test end to end on a job thread with the
bundled firmware (Debian 12 arm64 in Docker, on an Apple chip: free 6.887x against pinned 6.894x, ranges overlapping: it shows the machinery works, **not** what the R36S will do). **Untested on the unit.**

**Next:** run the Device report and the benchmark on the unit and read `threads` and `thread placement`; if pinning is faster in every window, make it the default; if the main thread is already apart, the cores are not the limit and the
remaining levers are the JIT's, above.

## First benchmark with the counters and thread lines, on the unit (2026-11-18, build 53d4504)

R36S, Linux 4.4.189, four cores, governor `performance` at 1512 MHz (the maximum, so no throttling), UNTRUE STEP, workload 2, 3 windows of 1 s:

| Setting | x real time | vs 2026-11-17 |
|---|---|---|
| interpreter | 0.0362 | 0.036 |
| JIT | 0.1848 (0.184-0.187) | 0.1859 |
| JIT pace 1 / 2 / 3 | 0.2486 / 0.2367 / 0.2456 | 0.2485 / 0.2388 / 0.2478 |

Nothing moved: the figures repeat to within 1%, so they are the unit's, not noise. Still about 4x short of 1x. The nop loop is 1.6 Minsn/s either way (the JIT does not engage on it).

**What the new lines say:**

- **The JIT is not short of compiled code.** After the warm-up 379.2M of 379.7M guest instructions (99.9%) ran compiled, from 942 blocks (565 KB of cache); the following window compiled about 240 more blocks. Compile time is therefore small (the "5% at most" figure above stands) and the cost is in running the compiled code: 41.7M guest instructions a second at 1.512 GHz is about 36 host cycles per guest instruction, with the A35 at an IPC near 0.3 as estimated before. Whether that is block entry and exit (1.32M entries by the end of the warm-up, about 290 instructions a block on average) or memory stalls, these counters cannot say.
- **The cores are not the limit.** The emulating thread (`fm1ntt-job`) uses 100.0% of cpu 1; the main thread 3.7%, audio 1.5%; all 17 threads 106.2% of four cores. Placement: pinned 0.246x, free 0.249x, ranges overlap (3 windows of 255 emulated ms): **no clear difference, so `--pin` stays off.** The scheduler already keeps the main thread apart.
- **The warm-up is the same cost as before**: 2017 emulated ms took 9.1 s (10.2 s for the interpreter), 0.22x.

**Consequence.** No setting or thread placement helps; the remaining levers are the JIT's (about 1.3x together, above), the two-thread split (non-deterministic, spike only), or a lighter firmware. The question to answer before more JIT work is entry/exit against memory stalls (`perf` on the unit if it has it, otherwise change one and measure). Device report and Rec demo not run in this session.

## What would

In order of what is known:

1. **Run it and read the new report:** the benchmark now measures the interpreter, the JIT and the JIT at pace 1, 2 and 3 on the same firmware,
   the Device report adds the CPU governor and clock (before and just after the runs) and a "firmware, best setting" line, PASS at 1x.
2. **A faster JIT** (a first 14-18% is done, above; each of the rest is worth maybe 1.1-1.3x on top of the one before, measured on the Mac first, and
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

## The audio thread and the instrument's input (2026-11-18)

A recommendation on running fm1i mode across the unit's four cores (a pool of three real-time workers, a fourth core for I/O, `SCHED_FIFO`) was read and **not adopted as a whole**: the engines cost about 0.7 % of real time for 12 voices on an M1 (projected 10 to 20 % on one unit core, **not measured there**), summing voices across threads in finish order breaks the core's determinism (rule 4: the Rec demo's byte-identical check), and pinning the emulation thread showed no difference on the unit. Two of its golden rules were: **no lock shared with the audio thread**, and **MIDI driven from the audio clock**. Built as `core/src/evq.rs`: a single-producer, single-consumer ring of timestamped MIDI messages (a note, controller, pitch bend or program change), stamped on the host's monotonic microsecond clock by the pusher; `Instrument::render_events` takes the messages that fall in the buffer and applies each at its own frame (it splits the render there, as it already does for the arpeggiator). A `Clock` keeps the buffer window on the sample clock and re-syncs to the host clock only after a drift of more than a buffer, so a callback that runs a few milliseconds late does not move notes. A message stamped before the window (a stall) plays at the first frame; one stamped at or after its end stays for the next buffer; a full queue refuses and counts (it never overwrites). **Latency is about one buffer (11.6 ms at 512 frames) and steady**, instead of anywhere from nothing to a buffer of jitter plus a lock wait. C API `fm1ntt_evq_*` and `fm1ntt_inst_render_events`; the host's SOUND preview sends an instrument's notes through it (`inst_note_q`).

**The machine's MIDI went onto its own queue the same day** (`Machine::render_events`, `fm1ntt_render_events`, the host's `mevq`): the machine renders up to a message's frame, the bytes go into the MIDI IN jack, and it goes on, within one slice (20,000 instructions, about 3 frames) of the frame asked. Tested on the stock firmware (local) or the Pack's Felucca: **the queued render is sample-for-sample identical to sending the same bytes after the same frames** (0 of 18,000 samples differ) and the MIDI changes the sound (more than 1,000 samples differ from a run without it), identical before the first message; a stalled message plays at frame 0, a future one waits. The host sends through it only while the audio callback runs, a firmware is loaded and the app is not paused (paused, the Rec demo and the self-test use the locked path, which is deterministic and keeps the bytes for when it resumes).

**Still on the locked path:** the firmware machine's keys and encoders and pot (`lock(a)` around `fm1ntt_key` and the rest: a key is also a MIDI-less edge the firmware polls, so its timing within a buffer matters less, and the same queue could carry them), the fm6 preview, program changes, loading and freeing. Those are rare or belong to the emulation, which cannot be sample-accurate anyway; the queue carries only an instrument's MIDI. **Not measured:** the actual jitter or latency on a unit, with a real controller.
