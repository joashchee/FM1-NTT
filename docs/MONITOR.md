# The MONITOR panel

A `top` for the machine the app runs on, between SOUND and TOOLS in the carousel (maintainer, 2026-11-18). It is the **host's** view; CPU and MEMORY stay
the emulated FM-1's. Code: `host/monitor.h` (no SDL: parsers, maths, thresholds, the sampler, the rain's ring) and `px_panel_monitor` in `host/pix.h`.
**Everything below is untested on an R36S**: it has run on this Mac (macOS reads) and in Debian 12 under Docker (the Linux reads and the self-test); the
figures for the unit are still to come (ROADMAP 5e, last item).

## What it shows

| Box | Linux (R36S and desktop) | macOS | Elsewhere |
|---|---|---|---|
| CPU: per-core bar and % (up to 8 rows, `+N MORE CORES`), total, load average, temperature, governor, clock per core, a 24-sample history strip per core | `/proc/stat` (two samples), `/proc/loadavg`, `thermal_zone0..7` (the hottest), `cpufreq` (core 0's governor, every core's clock) | `host_processor_info`, `getloadavg`; no temperature, governor or clock (n/a) | n/a |
| MEMORY: RAM used / total, cached, free, swap, this app's RSS, the emulated machine's buffers, free space on the card | `/proc/meminfo`, `/proc/self/statm`, `statvfs` of the app's folder | `vm_statistics64`, `hw.memsize`, `vm.swapusage`, `task_info`, `statvfs` | n/a |
| PROCESSES (pages): the busiest by CPU, memory or PID, with a bar | `/proc/<pid>/stat`, the first 192 parsed | `proc_listpids` and `proc_pidinfo` (only what the system lets the app see) | n/a |
| THIS APP'S THREADS | `thr_snapshot` (`host/threads.h`) | `task_threads` and `thread_info` | n/a |
| THIS APP: emulated speed against real time, JIT compiled blocks (`!` when some were given up on), audio buffers a second and **late buffers**, frame time and its worst in the last second, the job and its progress, the firmware | the app's own counters | the same | the same |
| DEVICE: battery level, state and draw in mW, wake-ups a second, pads seen, the MIDI jack's counts, the network link, the last hot-plug events | `power_supply` (`type` Battery, `capacity`, `status`, `current_now` x `voltage_now`), `getrusage`, `/sys/class/net/*/operstate`, `/sys/bus/usb/devices` (products, hubs left out), `host/toast.h` | `getrusage`, `getifaddrs`, IOKit battery level and state (no draw, no USB list) | n/a |

A value nobody read is a dash and a box with no source on the platform says n/a; the layout does not move. The first second after the panel opens is a
baseline (`SAMPLING...`): a percent needs two samples, and a counter that went backwards or stood still is a dash, never a number.

## Keys (the panel's own)

L2 / R2 (Page Up / Down): the page back and forward, wrapping. X: sort the processes (CPU, memory, PID). Y: freeze or resume (frozen reads nothing). The foot
says `LINUX PAGE 2/4` (or `MACOS`, or `NO HOST SOURCE`) and `FROZEN`. Standard: page 1 is the overview (CPU, MEMORY and THIS APP side by side, DEVICE), the
rest list processes with the threads under them. Zoomed: page 1 CPU and MEMORY, page 2 THIS APP, the threads and DEVICE, the rest the processes.
Nothing scrolls. A page key or a sort key asks for a sample at once, at most once in 250 ms.

## Colour is never alone

Green, amber and red come from the project's palette (`C_OK`, `C_WARN`, `C_ERR`; every text pair passes 4.5:1, a bar's fill 3:1 against its ground, both
checked in the self-test). The number is always printed, an amber value ends in `*` and a red one in `!`, and a bar's length is the value. The thresholds
(`MON_TH` in `monitor.h`, inclusive edges; the first guesses, to be changed from what a unit says):

| What | Amber at | Red at |
|---|---|---|
| a core's load, % | 70 | 90 |
| temperature, C | 65 | 80 |
| RAM used, % | 75 | 90 |
| battery, % (low is bad) | 30 | 15 |
| card free, % (low is bad) | 15 | 5 |
| frame time, ms (the goal is 10) | 11 | 25 |
| emulated speed, % of real time (low is bad) | under 97 | under 50 |

## The cost rule

**While the panel is not showing, nothing runs:** `mon_tick` returns before any read (no file, no folder, no timer), and `g_mon_reads`, which counts
every open, is checked not to move over 2000 calls. While it shows: one sample a second from the main loop's own pass (the loop's sliced sleep, never a
busy wait), the redraw once a second (`panel_ms` 1000), a sample that changes nothing asks for no redraw, and the process list is read only on the pages
that list it. Frozen (Y) reads nothing. Leaving the panel forgets the baseline so a long gap is not averaged into the first reading. Every read is bounded:
fixed buffers, at most 192 processes parsed, 10 threads, 16 cores kept, 64 processes sorted. The panel shows its own cost (`PANEL 0.05% 450US`: the last
sample's time and its share of a core at one sample a second). Drawing reads nothing from the host (checked).

## The data rain (TOOLS > DATA RAIN, off by default, not saved between runs)

When on, the space the page leaves free on the MONITOR panel streams real data as it passes through the app, as hex rows led by a letter: **A** the audio
output (high bytes of the scope), **M** the MIDI bytes sent, **P** the LCD's pixels as RGB bytes, **R** a page of the emulated RAM (read, not written: the
core does not expose a write log). After 30 s with no input the whole panel is the rain (`ANY KEY ENDS THIS`; any key or button ends it on the next
frame). It reads what is already there (one 40 x 32 ring), steps **at most 3 times a second** (nothing changes over the same area faster: no flashing),
stops while a job runs or the battery is at 15% or less, and the picture is a function of the ring (the same bytes draw the same pixels, checked).
`--reduced-motion` (or `FM1NTT_REDUCED_MOTION=1`) turns it into a still page, filled once. It is **never in a Rec demo** (the demo's own app has it off and its
MONITOR frame is an empty view, the same every time) **or a screenshot** (a screenshot is the FM-1's screen only). The window is a pixel canvas with no
accessibility tree, so there is nothing for assistive technology to read or skip.

## Tests (`fm1ntt --selftest`, `mon_selftest`)

Parsers against fixtures: truncated, huge (summed numbers capped so nothing wraps), garbage, a core number outside the table, a name with `)` in it; the
percent maths over counters that go backwards, stand still or do not add up; a fixture `/proc` and `/sys` tree (`FM1NTT_MON_PROC`, `FM1NTT_MON_SYS`) sampled
through `mon_tick`: values, deltas, sorts, the cap, a vanished tree; the cost rule (no reads when not showing or frozen, fewer reads on a page without the
list); every threshold edge; the colour pairs; the rain's cadence (no second holds more than 3 steps), its determinism and its bound; every page of both
layouts at low, mid and high load with 1, 4 and 12 cores, checked to stay above the foot, to be the same every time and to read nothing; an empty view; the
keys; the TOOLS row; the hot-plug history. `FM1NTT_MON_SHOTS=DIR build/fm1ntt --selftest` writes those pictures as BMP files to look at.

## Not done

Figures from the unit (the panel's own cost, whether a one-second sample sees a spike, the rain's cost, power and heat). The share of instructions run
compiled (it needs the JIT's profiling mode, which slows the run). A real MIDI port (the jack's sent and dropped counts are shown). macOS temperature,
clock and battery draw. The web build: it does not run the host's panels yet (ROADMAP 6, `docs/SCREEN-MIRROR.md` B), so the browser's own readings
(`hardwareConcurrency`, `performance.memory`) have no panel to go in. The rain shows the emulated RAM it reads, not the bus writes, and does not show the
job's file as it is read.
