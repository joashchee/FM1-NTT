# The firmware showcase (ROADMAP 5d)

A short demo of one firmware, made without a window, kept beside the firmware, shown on its page in FILES and played inside the app. Code: `host/showcase.h`
(no SDL) and the "showcase in the app" block of `host/fm1ntt.c`. **Untested on an R36S**: it has run on this Mac and in Debian 12 arm64 under Docker. The
times for the unit are projections from the 2026-11-18 benchmark (JIT about 0.18-0.25x real time, the interpreter 0.036x), not measurements.

## The headless run: `fm1ntt --showcase [--jit] [--entry 0xADDR] [--out BASE] FIRMWARE`

Its own machine on a stepped clock (30 ticks a second, 1470 frames of audio each), no window, no SDL, nothing read from a clock for content: the same firmware and
tour give the same bytes (the self-test runs it twice and compares every file). It writes `BASE/demos/<sha256 of the firmware>/` (BASE is the app's folder):

| File | What |
|---|---|
| `NNNN.png` | the FM-1's own 240 x 240 screen, only when it changed, at most 10 a second, at most 400; 8-bit paletted when the screen has 256 colours or fewer, RGB otherwise (our own uncompressed PNG, which the player reads back; any viewer opens it) |
| `audio.wav` | the sound, 16-bit stereo, 44100 Hz |
| `frames.txt` | `NNNN <ms>` per picture and `end <ms>` |
| `state.txt` | `state` (recording, ready, failed), `reason`, `tour`, `frames`, `ms`, `attempts`, `jit`; written first as recording and last as ready or failed |
| `progress.txt` | `pct=N` while it runs |

The tour (`SHOW_TOUR 2`, generic): 3 s of boot, three single notes, a chord, one turn of the SELECT encoder, a note, a rest: 15 s. UNTRUE STEP gives 140 pictures and
11 MB; the same tour in uncompressed RGB was 34 MB, which is why the palette. A per-family script (Felucca, Jangada) is not written. Bump `SHOW_TOUR` when the tour
changes: every demo made with an older one is made again.

The mp4 is not made here (decided 2026-11-18: the unit may have no ffmpeg, and playing frames needs no encode); the manual Rec demo still makes one.
**Deviation from the brief:** the brief asked for a headless run byte-identical to the foreground Rec demo of the same firmware. The showcase is its own tour,
not the Rec demo's, so the guard is two headless runs being identical to each other (and the Rec demo's own two runs staying identical, as before).

**The audio must not have gaps (fixed 2026-11-18, tour 2).** `fm1ntt_render` runs the machine until it has made the frames asked for or an instruction budget runs out,
and pads what is missing with silence. At 8 million instructions a tick the budget ran out in 46% of the ticks (heavy ones: a note-on burst), so tour 1's sound had a gap
of silence in nearly every other tick: the stutter and distortion heard on playback. A tick may now spend 64 million (`SHOW_BUDGET`; the core stops there so a hostile
firmware still ends), no tick came near it on the five bundled firmwares, `state.txt` records `shorts=N` (ticks that were padded) and the self-test fails on a gap of
40 or more silent samples after the boot. Demos made with tour 1 are made again. The Rec demo had the same cap: its tour had 538 such gaps and has 4 (the pauses of the
music) since its budget was raised the same way; the Rec demo's self-test run still uses a tiny budget on purpose (to be quick), so it does not check gaps.
**A steady rushing sound is not a fault:** HALF-LIGHT and MIDNIGHT ROUTE have a "bed" (rain, crackle, hiss, wind, room; their own documents), audible before any note.

**More cores.** The time of one demo is the emulation, one host thread: the machine's two cores are stepped in turn by one thread, and each step needs the one before.
Writing the pictures was 0.1 s of 21.6 s with the interpreter (and 0.09 s of 3.1 s with the JIT, which is 7 times faster here), so handing it to another core gains nothing.
Several firmwares could be made at once, one worker each; that finishes a backlog sooner but not one demo, and costs battery and heat: not built. The mp4 encode is the one step
that already uses every core (x264's own threads, when ffmpeg exists).

## Change detection, failure, cancel (the sidecar is `state.txt`)

A firmware with no state needs a demo. **Ready and failed are not made again** (a failure is never retried in a loop: rule 5) unless the tour is a newer one. A run
that was killed leaves `recording` and is tried once more (`attempts`), then counts as failed. A cancelled run (SIGTERM) removes its folder and leaves no state, so it
is needed again. A hard wall-time budget (900 s, `FM1NTT_SHOWCASE_BUDGET_S`) ends a run as `failed: time budget`, and a frame cap and the core's per-buffer instruction
budget bound the rest. A firmware that does not load, halts, or leaves the screen blank is `failed: <reason>`. Exit codes: 0 ready, 1 failed, 2 could not start,
3 cancelled. `scripts/check-firmware-updates.py` stays the source for "an updated release exists"; a changed file has a new hash and so a new folder.

## The background queue (TOOLS > BACKGROUND DEMOS, off by default)

The worker is a **separate process** (`fm1ntt --showcase`), not a thread: it has its own machine, cannot touch the app's memory, can be stopped and cancelled, and does
not share the process-wide app folder variable a thread would. It runs at nice 19 (and SCHED_IDLE on Linux), one at a time, started by the queue: one file looked at
every 2 s (hashed once), the first without a demo starts it, a pass over the folder and then a minute's rest. **The policy:** it is stopped (SIGSTOP) and goes on
(SIGCONT) by whether it may run: not while you play a running firmware (input in the last 15 s), not while a report, a benchmark, a Rec demo or a recording runs, and not
on a battery at 20% or less and discharging (looked at every 30 s). It is cancelled when the option is turned off or Y is pressed on its page, and asked to stop (2 s,
then killed, its folder removed) when the app closes. A worker that dies without a word is written as `failed: the worker stopped (signal N)` and not retried. FILES shows
a **progress box** (the firmware, a bar, the seconds, `PAUSED` while you play). The JIT is used when the build has it (the unit is too slow without it).

## The firmware's page (A on a firmware in FILES)

A opens the page first; on it A loads, X plays the demo, Y makes it now or stops it, B (or Esc) goes back. It shows the file name, the tier (bundled and permissive,
the GPL pack, or your own file), the licence and source from the manifest of its tier (for a file you brought: not known), the SHA-256, and the demo: none yet, being made
(a bar), ready (pictures and seconds), or failed (the reason). Whole page, nothing scrolls, and no text names a file of the app's own (rule 11).

## The player (X on a page with a ready demo)

The whole window becomes the player, whatever layout the app was in: the demo's picture at **2x (480 x 480) flush top left**, and in the 160 px column at the right
(`px_panel_player`): **DEMO** (the firmware, its tier, its licence), **NOW** (what the tour is doing: BOOTING, NOTE 1, A CHORD, SELECT KNOB TURNED, THE TAIL; a bar,
m:ss of 0:15, the picture number), **KEYS HELD** (the keyboard with the keys the tour holds lit), **SOUND** (the scope and the L and R levels of the sound being played),
**WHAT THIS IS** (one sentence) and `ANY KEY STOPS`. The tour is a table (`SHOW_NOTES`) that both the recorder's script and the player read, so the keys and steps shown are
what was played (the self-test steps the script on a machine and compares the held keys at every tick). The sound goes through the app's audio device; the machine waits while
it plays. Any key or button ends it and is used up (no stray note), as does the end of the sound. Our own PNGs are read back by a decoder that takes only what the writer
makes (stored blocks, RGB or paletted, filter 0): anything else is refused, never read past its end (the self-test cuts and damages files).

## Not done

- **Measure on the unit:** the worker's cost to the UI and the audio, battery and heat, and the real time of a tour (Device report `power` and `threads` lines with a worker
  running). `--pin` stays off.
- **A per-family tour** (Felucca, Jangada) beyond the generic one.
- **"Port available" and a Load instrument button:** blocked on the package picker (ROADMAP 5c).
- **The background port job:** blocked on a second verified family (ROADMAP 5d).
- **Not shipped:** demos are written under the app's own `demos/` folder from a file the person holds and are never put in the app, a package or `bundled-firmware/`;
  `scripts/check-bundled.py` has no check for a `demos/` folder yet.
- **Tried only headless:** the windowed app has not been run with a display here; the page, the progress box and the player are checked by the self-test through `act`,
  `fill_ui`, `audio_step` and `dispatch`, and by pictures drawn from the same code.

## Compare mode (ROADMAP section 3)

Two firmwares on the same notes: every demo plays the same tour (`SHOW_TOUR`), so the pair's demos are already the same notes in two firmwares. **Y on a firmware in FILES marks it**
(`1`, then `2`; Y on mark 1 clears); the second mark opens the pair. A demo that is not ready starts the worker (when it is free) and says so; Y on the pair again opens it when it is.
Both demos must be of the current tour (450 ticks of 1470 frames), else it asks for them to be made again.

The window is the player's (the picture at 2x, flush top left) with the compare column. One clock (`pl.pos`), one sound at a time: **left** hears A, **right** hears B at the same
position (the picture follows the side heard), **up** and **down** jump to the start of the step before or after (the steps are the runs of `show_step` labels), **A** restarts, **Start**
pauses, **B** closes; every other key is used up while it is open. The column shows, for the current step, each sound's peak and the **difference** (the RMS of the two sounds' difference
over the louder one's RMS, 0 to 100%), and for the whole tour **where the two first differ** in milliseconds, or IDENTICAL (sample for sample). Code: the "compare mode" block of
`host/fm1ntt.c` (`cmp_*`) and `px_panel_compare` in `host/pix.h`. Not built: your own notes (a per-pair demo from `fm1_soc::script`), the picture of both at once.
Untested by eye and ear, and on a unit.
