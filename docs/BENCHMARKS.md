# Benchmarks: reports that compare across platforms

`fm1ntt --bench` (or **TOOLS > Benchmark**) writes one text file, `$BASE/bench/bench-<os>-<arch>[-r36s]-<stamp>.txt` (and a copy as
`latest.txt`), where `$BASE` is `/roms/FM1-NTT` on the R36S, else `~/FM1-NTT` (`$FM1NTT_DIR` overrides). Run it on each platform you
care about, collect the files, and compare them:

```sh
scripts/bench-compare.py bench-Darwin-arm64-....txt bench-Linux-aarch64-r36s-....txt [more...] [--baseline 2] [--markdown]
```

The table has a row per benchmark and a column per file. After the first column's numbers, each other column shows its own number and
its **ratio to the baseline, above 1.00x always meaning better** (so instructions a second and milliseconds read the same way). The last
row is the geometric mean of the ratios. Reports whose `format` or `workload` differ are refused (`--force` compares anyway and says so);
a report without its `end` line was cut short and is noted.

## The file

```
# fm1ntt benchmark
format: 1                 the file's layout
workload: 2               the benchmark's version (core/src/bench.rs): bumped whenever the workloads change
app: FM1-NTT 0.2.0 (<commit> <date>)
made: 2026-11-16 11:27:04
platform: Darwin-arm64    <sysname>-<machine>, "-r36s" appended on an R36S
os: Darwin 24.6.0 arm64
cpu: Apple M1             model name / Hardware / sysctl; cores; max_khz when cpufreq says; ram_mb
native_jit: available     whether this CPU is one the vendored core's block compiler supports
seconds_per_run: 1.00     each timed workload runs for this long, three times
runs: 3

== results ==
BENCH core.nop.interp: 33.19 Minsn/s better=higher min=33.17 max=33.29 n=3
...
# firmware: sloop.fwsc 412816 bytes fnv64=...        (only with --fw)
BENCH fw.<fnv64>.interp.xrt: ...
SKIP fw: ...
end
```

A `BENCH` line is the **median of three runs** with the extremes beside it; `better=` says which way is good; the unit is on the line.
`end` is last, written only when the run finished, and the file is written to a temporary name and renamed. The native-JIT step writes
a `# trying the native JIT` line first: **if a report stops there, the process crashed in the compiler** (the `.tmp` is left behind),
which is itself a result.

## What is measured

| id | what | better |
|---|---|---|
| `core.nop.interp`, `.xrt` | a synthetic loop (eight `nop`s and a `goto`, hand-assembled in `core/src/bench.rs`) run flat out: millions of instructions a second, and times the emulated 240 MHz clock | higher |
| `core.nop.jit`, `.xrt` | the same with the native block compiler switched on (skipped on a CPU the core does not support) | higher |
| `core.boot`, `core.reset` | building a machine from an image, and back to power-on, in milliseconds | lower |
| `mem.read` | 256-byte reads of the flash region, as the MEMORY panel does them | higher |
| `screen.draw.standard`, `.zoomed` | the whole 640 x 480 canvas drawn, averaged over all eight panels, ms a frame | lower |
| `io.flash.write`, `io.flash.read` | a 1 MiB flash slot written the way the app writes it (temporary name, flush, rename) and read back | lower |
| `fw.<hash>.interp.xrt`, `.jit.xrt`, `.jit.pace{1,2,3}.xrt` | **with `--fw FILE`:** that firmware, **after a warm-up past its boot** (2000 emulated ms with the JIT, at most 20 s of real time; the file says what was reached in a `# fw warm-up` line), then three consecutive windows run flat out: emulated time over real time (1.0 is real time) | higher |

**What it does not say.** The nop loop is a *ceiling for dispatch speed*, not a prediction of how any firmware runs: a real firmware
mixes loads, stores and peripheral accesses (slower per instruction) and idles most of the time (the core skips idle). So the headline for
"will it run on the R36S" is the `fw.*` line. **With no `--fw`, the benchmark runs the bundled UNTRUE STEP**, so every platform times the same real firmware (the hash in the name proves it) and the line is always there; give `--fw FILE` to time another, compared across platforms by the hash in its name (the same hash
means the same file). `core.nop.jit` equal to `core.nop.interp` means the vendored core's compiler did not engage on that loop (it decides
per block); that is noted in the file and tracked in ROADMAP section 5.

**Why the warm-up (workload 2).** A firmware's first emulated seconds are cheap (boot, idle) and it gets heavier after that. Workload 1 timed 1 s windows from a cold start, so on the R36S it measured the cheap part (UNTRUE STEP, JIT, pace 3: 1.37x in the benchmark, 0.64x in the Device report, which runs 3 s). Version 1 files are refused by `bench-compare.py` against version 2 ones.

**The `# jit` lines** (with `--fw`, the best setting, pace 3, JIT counters on so never one of the timed runs): the JIT's state after the warm-up and after one window, as JSON: `blocks_cold` / `blocks_compiled` / `blocks_given_up`, `cache_bytes`, `entries` (times compiled code was entered from Rust), `jit_insns` (guest instructions run compiled) and `insns` (in all). Read the second line minus the first: `jit_insns` over `insns` is the share running compiled, and `blocks_compiled` still growing says the unit is compiling in steady state. They are notes, not `BENCH` lines, and are not compared.

## Rules for changing it

- **Bump `WORKLOAD_VERSION`** (core/src/bench.rs) whenever the workload's bytes or a benchmark's definition change, and add a line to the
  changelog. Old files are then refused by the compare script, never silently mixed.
- **Bump `format`** when the file's grammar changes, and teach `scripts/bench-compare.py` (and its tests) both.
- Keep the benchmark short (about 30 seconds on the R36S) and free of anything that needs a firmware, a network or a person.
- A new benchmark gets an id, a unit, a direction and a row in the table above in the same commit.

## Where it fits

`--report` (the Device report, r3i6nos rule 9) answers **what is wrong on this unit** (PASS/FAIL against goals, power, the card).
`--bench` answers **how fast, compared with the others**. They share the platform facts and are separate files on purpose: the report is
read by Claude after a run on the unit, the benchmark is collected from many machines and compared by a script.
