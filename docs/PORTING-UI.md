# Porting the UI: from a firmware's screen and controls to an instrument that looks and behaves like it

Written 2026-11-19 from a study of **VNL-1 0.9.2** (Sam Oltz, GPL-3.0-only; a generative lo-fi groovebox). The study is **black box**: the firmware ran in
the emulator and its screens and controls were observed; **its source was not read** (rule 3's spirit: a GPL firmware's code stays out of this MIT
project; what a screen looks like and what a control does are observations). `docs/PORTING.md` has the sound side; this file is the UI and behaviour side,
and the plan for the pieces the porter still needs. Everything is **untested on a real R36S**.

## 1. What a port of the UI is

The R36S cannot run the firmware live (`docs/PERFORMANCE.md`), so an instrument package plays native engines. A port that stops at sound leaves the person with
the generic editor: parameters in rows. A firmware like VNL-1 is **its screens**: the record on the home screen, the mixer, the band's piano roll. The goal is
that a `.fm1i` can carry **a model of the firmware's UI**: pages of widgets bound to the engine's parameters and to state the engine publishes, plus the
control map (which button opens which page, which knob turns what), drawn by the host in the same 240 x 240 pixels.

It is **data, never code** (the package rule): no scripts, no firmware pixels. The model says *where* a fader is and *what it controls*; the host owns the font
(the project's own 5 x 7, scaled), the shapes and the animation. A port of a GPL firmware's UI is that firmware's data: it inherits the licence and belongs in
the **Pack** tier, never in the MIT app (`docs/PORTING.md`). The colours, positions and wording observed are facts about a screen; the artwork (icons, the
firmware's own font) is not copied, it is redrawn.

## 2. The study: what VNL-1's UI is made of

Observed with `fwprobe` (by hand) and `uiport` (the sweep below: **9 distinct screens from 80 probes, 114 s on the Mac**).

| Screen | How it is reached | What is on it |
|---|---|---|
| **Home (the record)** | boot, `HOME` from another page | a vinyl disc that turns with the beat; the song's title ("SLOW BLOOM"), key and mode ("Db MIXOLYDIAN"), a stopped/playing line, the section ("INTRO 2/64") with a progress line, tempo (84 BPM) and the mood ("SUNNY") |
| **Home detail** | `HOME` | the song's chord, six band parts each with an icon, a colour, the chosen instrument and its style ("RHODES / TRESILLO"), and key, tempo, motion and mood with small bars |
| **Song** | `SCL` | page title and "1/3", a chord at the top right, rows K1 KEY, K2 SCALE, K3 TEMPO (with a bar), K4 MOOD, each with `< >` arrows, a hint line of key to action |
| **Mixer** | `FX` | six vertical faders (DRUMS BASS ACCOMP LEAD PAD TXTR) in the six part colours, each with an icon and a level, K1-K4 mapped to four of them, "SELECT PAD/TXTR" |
| **Parts** | `ARP` | a piano-roll grid per part: rows for the parts with their colours, a bar position, key letters down the side |
| **Settings** | `GLO` | a list (THEME, BASS SATURATION, HOME SCREEN, MOVEMENT, SCREEN READER, KEY LIGHTS, BRIGHTNESS, ADVANCED RULES) with the row highlighted; theme "GARNET" (it has 15) |

Patterns that recur, and so become the widget set (section 3): a **status line** (section and tempo, top), a **title and page counter**, **K1-K4 rows** that
are the four knobs' current values, **faders** and **bars**, a **grid**, the **disc** (an animated element driven by the beat), a **hint line** at the bottom
that always lists the keys' jobs ("EDIT BACK  HOME MAIN  SEQ NEW SONG"), and **per-part colour and icon** used everywhere a part appears. One palette on a
dark ground, a theme that swaps the palette, and a settings page whose rows are the global switches.

Behaviour (not UI, but the UI shows it): VNL-1 **generates** a song (key, scale, tempo, chords, layout and band) and plays it; the "next song" key makes a new
one; a mood and a "motion" setting steer it. The notes are not in a bank; they come out of the firmware's logic. That is a **level C** port
(`docs/PORTING.md`), and the honest reading is below (section 6).

What the sweep found that a person did not choose by hand: the buttons that go to a new screen from the home screen (`FX`, `SCL`, `GLO`, `HOME`, `ARP`: the
hint line's own labels agree), the screens reached from `SCL` by `PLAY` and by the SELECT encoder (the song page has sub-pages), and that **SELECT and the
knobs change a value on a page** without changing the layout (the sweep sees only a changed pixel count there).

## 3. The UI model in the package (`core/src/ui.rs`, format 3)

`ui = ...` lines in a `.fm1i` (`fm1i = 3`), each one directive; the grammar is in the module's header. **Built and tested (2026-11-19)**: a bounded parser (an
`Error` on any bad line, rule 5), a round trip, and a layout that turns a page into a **display list** (rectangles, discs, text) that any renderer paints.

| Directive | Meaning |
|---|---|
| `theme`, `color` | the four base colours (`bg fg dim accent`) and named colours (the part colours) |
| `page NAME "TITLE"` | a screen |
| `text`, `value`, `fader`, `meter`, `grid`, `disc`, `bar`, `hint` | widgets, placed in the 240 x 240 screen |
| `key NAME page=P`, `enc NAME page=P bind=B step=N` | the control map: a button opens a page; an encoder turns a binding |
| bindings | `param:N` (engine), `global:N`, `fx:I.N`, `state:NAME` from a closed list (`STATE_NAMES`: title, key, scale, bpm, mood, beat, bar, progress, playing, level0, level1, pattern0) |

Limits (bounded): 16 pages, 256 widgets, 64 controls, 64 colours, 1024 lines of 240 characters; positions inside the screen; every colour and page a line
names must exist. **Not in the model yet:** icons (a glyph set to design), the proportional font (the model uses the 5 x 7 font, scaled), per-widget
animation beyond a bound state, sub-pages and a cursor row (the settings list), themes as swappable sets, text that is a list of choices.

**In the instrument and the host (2026-11-22).** `Instrument` keeps the model (it is saved back with an edited package, which it was not before), refuses a package whose
binding names a parameter, global or effect parameter it does not have, and offers `ui_draw` (the page shown, bound to live values), `ui_control(i, clicks)` (a button
opens its page; an encoder opens its page *and* steps its binding by `step` a click, clamped to the parameter's range) and `ui_state(n)`. **Published state:** `title` the
instrument's name (text); `key` the last note's pitch class unless set; `scale` and `mood` what was set, else 0 (these three are set by hand, the rest refuse);
`bpm` the sequencer's tempo when it runs, else the arpeggiator's; `beat` 0 to 3, `bar` and `progress` (0 to 99 through the bar) from the frames rendered at that tempo
(a free-running count, not locked to a note); `playing` 1 while anything sounds; `level0` and `level1` the output peak 0 to 99 (the instrument is mono, so both, falling a
few points a buffer); `pattern0` the sequencer's step 0 to 15. A test checks each name. The C face is `fm1ntt_inst_ui_*` (`core/fm1ntt.h`); the host paints the display list
into the FM-1's screen box (`px_paint_ui`, `host/pix.h`): **rectangles and discs are the core's raster pixel for pixel (a host self-test and a C-face test compare them);
text is the host's 5 x 7 font where the raster has solid cells.** A package with a screen opens the SOUND editor on its last group, SCREEN: one row for each control (Left and
Right press a button or turn an encoder), and the box beside it shows the page, live. **Not seen on the R36S panel; no real firmware's model exists yet (the sweep's draft
has no widgets).** The pad's own buttons are not the FM-1's, so the rows stand for them; mapping the FM-1's button names onto the pad is open.

**Widget detection and the UI check (2026-11-22).** `screen::detect(frame, bg)` reads a captured frame: single-colour blocks give **discs** (a roughly square block three quarters filled, the ring and the dot of a disc are two), **bars** and **meters** (solid, three times longer than thick), **grids** (eight or more equal cells on a lattice; cells 5 : 7 are not, since `ui::rasterize` draws text as such cells) and **boxes**; what ink is left is read as marks of any colour (so an anti-aliased glyph holds together) and the glyph-sized ones are joined into **text lines** (a count of glyphs and a size). `draft_lines` writes them as `ui = ` lines with `?` for the words, and `uiport` puts them in its draft (cut at the model's 256 widgets, said in its last line). `screen::verify_ui(model, page, frame)` is the check: the overlap of the model's ink and the screen's, and the share of the model's ink that sits on ink, stored as a `verify-ui` line in the package. **What it cannot do:** the words (text recovery needs the firmware's font), a value row's label and number as one widget, a fader's handle, which control a widget belongs to, and anything that is not still on the screen at capture.

## 4. The porter, extended

The six steps of `docs/PORTING.md` gain a UI half. A piece marked **built** exists and is tested here; **scaffold** is built but thin; the rest is the plan.

```
  3a OBSERVE-SCREEN    boot, find every screen, its palette, blocks and text bands                      uiport + screen.rs   built (scaffold)
  3b OBSERVE-CONTROLS  press each button, turn each encoder both ways, from each screen, to a depth     uiport               built (scaffold)
  3c OBSERVE-BEHAVIOUR capture what the firmware plays and sends (notes, MIDI out) under many seeds     --                   planned
  4a FIT-UI            from the sweep's draft to a UI model: widgets, bindings, colours                  ui-draft.txt         draft by tool, edit by hand
  4b FIT-ENGINES       per part: pick or build the engine, fit it to captured audio                      fm4 / fm6 / sampler  per engine, by hand
  5a VERIFY-UI         draw the model, compare with the captured screen                                  screen::coverage_agreement  built (scaffold)
  5b VERIFY-BEHAVIOUR  statistics of the generated music against the firmware's                          --                   planned
  6  PACKAGE           ui lines in the package, provenance, tier                                         package.rs (format 3) built
```

**What was built, and what it does** (all in `core/`, all tested, `cargo test` 183 passed, nothing run on a unit):

- `src/ui.rs`: the model (`Ui`, `Widget`, `Control`, `Bind`), `Ui::parse` (bounded), `Ui::to_lines`, `Ui::draw` (a page to a display list from values the host
  supplies), `rasterize` (a display list to pixels, text as solid cells).
- `src/package.rs`: a `ui` field, `fm1i = 3` when it is not empty, validated at load (a bad UI is a bad package).
- `src/screen.rs`: `hash`, `palette`, `background`, `diff` (the changed box), `blobs` (connected ink, a stack-based flood fill), `bands` (text rows),
  `coverage_agreement` (a drawn layout against a captured frame: overlap and precision), `png` (to look at).
- `examples/uiport.rs`: the sweep. `uiport FW.fwsc --out DIR [--settle S] [--depth N] [--max N]` writes `pages/NN.png`, `controls.tsv`, `pages.tsv` and
  `ui-draft.txt` (a theme from the home palette, a page per screen, a `key` line per control that opened a screen, checked with `Ui::parse`).

**What the first run found (VNL-1, depth 2, 80 probes):** 9 screens, the five page buttons, `ui-draft.txt` accepted by the parser. **Its limits, said plainly:**
a screen's identity is its layout (text bands and main colours), so two screens with the same layout and colours are one, and an animation is tolerated by
design; one click each way finds the pages but not a value's range; every probe reboots the firmware and replays its path (the machine cannot be cloned), so a
sweep is minutes; the draft's page names are `p00`, not "SONG" (a person, or a later OCR against the firmware's own font, names them); no widgets are detected
yet, only the theme and the control map.

## 5. The roadmap

In order; each phase ends at a gate that is a test or a number, never "looks right". (Mirrored in `ROADMAP.md`.)

1. **Study and scaffold** (done 2026-11-19): this study, `ui.rs`, `screen.rs`, `uiport`, package format 3.
2. **Host renderer.** `host/uirender.h` paints a display list with `pix.h` (rectangles, discs, the 5 x 7 font at a scale); a C API (`fm1ntt_ui_*`) loads a
   package's UI, returns a page's display list for the values the host gives; the host's SOUND panel shows the package's UI when it has one and the generic
   editor when not. *Gate:* the same display list drawn by the host and by `rasterize` agree on every rectangle (a self-test), and the screenshots in both
   layouts were looked at.
3. **Control map.** The host routes buttons and encoders through `Ui::control`: a button opens a page, an encoder steps a binding by its `step` (a fast turn
   steps more). *Gate:* a self-test drives a package's controls and checks the parameter values and the current page.
4. **State publishing.** Engines and the host publish the closed `STATE_NAMES` (the beat, the bar, the song's title, levels per part, a pattern's bits); the
   model's widgets read them. *Gate:* a test per name.
5. **Widget detection** (`screen.rs` next): classify blobs and bands into widgets: a horizontal line of text, a value with a bar under it, a vertical bar of
   a part colour (a fader or a meter), a regular grid of cells (a piano roll), a disc (a circle fit to a blob). The draft then carries widgets, not just pages.
   *Gate:* run on the stored VNL-1, OPERATOR and Mighty Arp screens; each detected widget is checked against a hand-made model of the same screen.
6. **Text recovery.** Names for pages and labels need text: compare a screen's glyph blobs against the firmware's own font (found by running it on a known
   string, or by clustering equal-size blobs and having a person name each shape once). *Gate:* a labelled page reads back the strings a person sees.
7. **Value ranges and the knob map.** A sweep that turns each knob in each direction until the screen stops changing, reading the number off the screen
   (once text recovery works) to give `min`, `max`, `step`, `unit` and the law. This is what was done by hand for OPERATOR's filter (`docs/PORTING.md`).
8. **Verify the UI.** `screen::coverage_agreement` of the model's drawn page against the captured screen, with a threshold per firmware (text is drawn
   in another font, so 1 is not the goal). Stored with the package (`verify-ui = ...`) as the sound verification is. *Gate:* a number per page, shown.
9. **Behaviour capture** (3c). A host that records what a firmware plays: MIDI out (if it sends any; **not checked for VNL-1**), the audio, the screen's
   section counter and the song title, across seeds (new-song presses) and settings (key, scale, mood). This is the data a generative engine is fitted to.
10. **Engines and the generator** (section 6).
11. **Package the first full UI port** on a firmware that needs no generator first (OPERATOR: its pages are the model's widgets; the filter and waves are
    already fitted), to prove the chain before VNL-1.

## 6. VNL-1 as a level C port: what it would take, honestly

VNL-1's sound has two halves, and they are different kinds of work.

**The voices (fit by hand, per `docs/PORTING.md`):** ten engines by its page: a **sampler** (the repository's sampler engine exists; the sample bank is
a separate CC0 download, so a port carries *references* to it, never the audio, `docs/SAMPLER.md`), **FM piano and FM bass** (`fm4` or `fm6` fits, by
measurement as for OPERATOR), a **tonewheel organ** (a new additive `organ` engine, the `fm4` waveform tables are the start), **drum synths** (a `drum`
engine; the firmware says some are GPL-3.0-only engines of other authors, which only strengthens the Pack-tier rule), and **ten lo-fi effects** (`fx.rs` has
limiter, delay, low-pass, chorus, reverb, filter, distortion and phaser; vinyl crackle, wow and flutter, tape saturation, bit reduction and the rest are
new). Every one is a measurement job like the OPERATOR fits: drive it in the emulator, capture, fit, test.

**The composer (cannot be copied, can be measured):** the song generator is logic in the firmware. Its source is not read, so a native `gen` engine is
**written from what it is observed to do**: the keys, scales and tempos it picks, the chord vocabulary and how often each progression appears, the density
and swing per mood, how the six parts divide the bar, how a section changes. Section 3c (behaviour capture) is the data for it; 5b is its check, and the check
is **statistical** (the same distributions over many seeds), **not note for note**: the firmware's random numbers cannot be reproduced without its code, and a
port that promised the same song for the same seed would be promising what it cannot know. The package would carry the generator's *settings*; the engine
stays in Rust, by name (rule: no scripting).

**Order that makes sense:** finish the UI chain on a firmware with no generator (phase 11), then the VNL-1 voices one at a time (each is useful alone: an
organ, a drum kit), then the generator last. VNL-1 stays in the Pack tier throughout (GPL-3.0-only as a whole), and a port of it carries the same.

## 7. Rules this relies on

Rule 1 (say what was not run: nothing here ran on an R36S; the sweep ran on the Mac); rule 2 and `docs/PORTING.md` (a port inherits the licence; GPL goes to
the Pack); rule 3 (no GPL source read: the study is by observation); rule 4 (no new dependency: the PNG writer, the flood fill and the parser are ours);
rule 5 (a UI line is untrusted: bounded, an `Error`); rule 6 (the window is pixels and the project's own font: the UI model uses the 5 x 7 font scaled, not the
firmware's); rule 11 (nothing a person sees names these docs).
