# Screen mirroring for a pixel app: the plan

Status 2026-11-16: **a plan, nothing built.** r3i6nos's screen mirror (`../r3i6nos/docs/SCREEN-MIRROR.md`) sends the cells that changed of a
64 x 24 text screen: 3 KB a frame, a few bytes a change, an idle screen sends nothing. **FM1-NTT draws pixels** (a 640 x 480 canvas: the FM-1's
240 x 240 LCD, then panels), so a screen of cells is out of scope for it, and r3i6nos's open question 4 ("pixel pages: tiles, or only the
cells around them?") is this app's whole screen. This file is the way to do it **without making the unit work harder for it**: an idle
screen must cost an idle mirror (r3i6nos rule 14), and the unit is a battery device.

## What we have that r3i6nos's pixel pages did not

- **The canvas is already in CPU memory.** `px_draw` paints into a plain `uint32_t[640*480]` and SDL uploads it; nothing is read back from
  the Mali GPU, so there is no sync to pay (the reason r3i6nos rejected VNC).
- **We know when the screen changed.** The main loop redraws only when an event arrived, the FM-1's LCD revision moved, or (with a
  firmware running) about ten times a second; an idle app with nothing loaded blocks on input.
- **The picture is mostly flat colour.** Panels are boxes, one-pixel frames and 5 x 7 text on four or five colours; the only photographic
  part is the FM-1's own LCD (240 x 240 RGB565, and a firmware usually changes a part of it).
- **The state behind the pixels is small.** `UiState` (a few hundred bytes plus the LCD and the file list) is everything `px_draw` needs.

## Two ways, and the order

| | A. Tile diff of the canvas | B. Semantic: the LCD plus the UI state |
|---|---|---|
| Sends | the 32 x 32 tiles (20 x 15 = 300) that changed since the last sent frame, each RLE- or LZ4-coded | the FM-1's LCD as changed tiles (RGB565, 240 x 240 = 8 x 8 tiles of 30 x 30) **and** `UiState` as a small delta |
| The page | paints tiles | **re-runs the drawing**: the same layout and font as `pix.h`, so text is sharp at any size (a TV) and a panel costs a few bytes |
| Idle | nothing | nothing |
| Panel change (the carousel) | most of the right panel: 100 to 150 tiles, mostly flat, a few KB coded | a dozen bytes |
| A firmware drawing | the LCD tiles that changed | the same |
| Cost to build | small: a diff and a coder, any pixel app can use it | larger: the page needs the drawing code |
| Risk | the page's picture is a scaled picture | two renderers must agree |

**Recommendation: build A first, B after A works.** A is generic, small, testable (a stream replays to a byte-identical canvas), and the
fallback r3i6nos already had in mind for pixel pages. B is what makes the mirror as light as the cells one and sharp on a TV; its two
renderers are kept in step by compiling **the same `pix.h` to WebAssembly** for the page (the core is going to WebAssembly anyway,
ROADMAP section 7), and a test draws the same `UiState` natively and in the page's module and compares every pixel.

## A, in detail

- **Where the diff is:** after `px_draw`, before `present`: compare the canvas to the last sent copy 32 x 32 tile at a time (hash a tile's
  rows, compare only on a hash change), only **when something drew** (the same condition as the redraw), only **while a viewer is
  connected**. With none, the cost is one flag test; the flag is r3i6nos's (`/run/loveos/mirror/on`, tested only after a redraw and at
  most once a second, never a timer).
- **The frame:** `F` (a keyframe: every tile, on connect and when a page asks) or `D` (a delta): a list of (tile index u16, coded length
  u16, bytes). A tile is coded as **palette + RLE** (a flat UI tile is a handful of bytes) or **LZ4** when RLE does not win (the LCD).
  RGB565 for the LCD region halves it before coding. No keyframe on a timer.
- **The link:** the one r3i6nos already offers (a USB Ethernet adapter or a Wi-Fi dongle; the OTG gadget when it exists), HTTP with
  Server-Sent Events to a web page, started by a person from a menu, off at reboot, a token in the address, no input back. **How the
  datagrams reach the daemon is a decision for the maintainer:** (1) a new datagram type in r3i6nos's `r3i6nos-mirror` (it owns the page,
  the token and the link; this app only sends tiles to `127.0.0.1:36364`), or (2) a small server in this app (self-contained, so the
  desktop and the web build use it too, and r3i6nos is not needed). (1) keeps one mirror for every app; (2) keeps this repo MIT and
  standalone. Either way the page is a canvas painted from tiles.
- **Numbers to expect (estimates, to be measured):** a full 640 x 480 RGB frame is 1.2 MB; the panel UI is roughly 95% flat colour, so a
  full keyframe codes to tens of KB and a carousel turn to a few KB; a firmware that redraws a fifth of its LCD ten times a second is
  roughly 20 to 40 KB a second after LZ4. All well inside a 100 Mbit USB adapter, and small for Wi-Fi.
- **Power (rule 14):** measure on battery, in one window each: no mirror; the link up with no page; a page open on an idle screen (must cost
  nothing more than the link); a page open during Rec demo's tour (the tile traffic). The device report's `== power ==` section gets a
  `mirror` window for it. Goals are set after the first measurement.

## B, in detail

- The stream is `UiState` deltas (by field) and LCD tiles; the page owns the drawing. `UiState` is made stable and versioned
  (`mirror_format`), and **does not grow a field without a page change**.
- The page is the web build's own viewer: the same module that will run the emulator in a browser (ROADMAP section 7) draws the panels
  from a `UiState`, so the **mirror page and the web build are one codebase**.
- Non-interactive, like r3i6nos's: the page sends nothing to the unit.

## Tests (before any of it is trusted)

1. **Replay:** a stream recorded from Rec demo's tour replays, tile by tile, to a canvas **byte-identical** to the one the app drew at
   every frame (the tour is deterministic, so this is exact).
2. **Fuzz:** every cut, every byte set to 0x00/0xFF, random damage: the decoder never reads or writes outside the canvas, never loops.
3. **Idle:** with a viewer connected and nothing changing, zero datagrams in 60 seconds, and wake-ups a second within the idle goal.
4. **Size:** the tour's bytes a frame are recorded in the log (`rec-demo.txt` gets a `mirror` line), so a change that makes it heavier shows.
5. **B only:** the native and the page's rendering of the same `UiState` agree for every panel in both modes.

## Not in scope here

Sound (the FM-1's audio is made on the unit; a WAV or Opus stream over the same link is possible later, r3i6nos open question 5), input
from the viewer, DisplayLink or any monitor with no computer (r3i6nos ROADMAP section 24), and VNC.
