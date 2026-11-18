# What is known about the machine

Researched 2026-11-16 from public pages; **nothing here was run, and none of it is verified against a unit.** It is the research
`ROADMAP.md` section 1 starts from. Sources are named so each claim can be checked.

- **SoC:** JieLi AC791N (WL82), two 32-bit **pi32v2** cores (`awesome-fm-1`; `fm1-emu`'s README). A JieLi AC79 datasheet (AC7911B, v1.1,
  2021) describes a 32-bit RISC CPU but not its instruction set.
- **Instruction set:** JieLi publishes no manual. `fm1-emu` recovered it from firmwares (its `isa/` and `docs/isa-notes.md`) and checked it
  with programs built by JieLi's toolchain; `kagaimiq/jielie` documents the CPU as a community effort. Official SDK docs for the neighbour
  AC792N are at doc.zh-jieli.com; JieLi's toolchain is clang-based (unconfirmed for pi32v2).
- **Peripherals `fm1-emu` models:** timers, interrupts, flash (Puya by default; `uc25hq80` selectable), a 240 x 240 LCD over SPI, a key
  matrix and encoders, knobs, audio output, a MIDI UART, a USB controller, and hardware DSP, EQ and SRC units with a flash cache.
- **Package format:** `.fwsc` is what the community firmwares ship as (`fm1-emu` loads it with `--fwsc`); its layout is not documented
  here yet. `.syx` DX7 banks load into M-VAVE's own firmware.
- **Memory map:** in the vendored `fm1-soc` (`core/vendor/fm1-emu/crates/fm1-soc`): RAM at 0x01C00000, the execute-in-place flash at 0x02000000,
  Felucca's entry at 0x02000120. Not verified against a unit by this project.
- **The front panel** (from `fm1-emu`'s `fm1-live/src/panel.rs`, read as the MIT source it is): a matrix of 41 ids, 14 buttons (0 OCT-, 1 OCT+,
  2 FX, 3 SCL, 4 ENV, 5 LFO, 6 EDIT, 7 GLO, 8 HOME, 9 SAVE, 10 ARP, 11 SEQ, 12 PLAY, 13 REC) and 27 note keys F3 to G5 (ids 14-40);
  7 encoders (SELECT, ALGORITHM, KNOB1-4, PRESETS) and one pot (ADC channel 4); the LCD is 240 x 240 RGB565.
- **EDIT mode, from the FM-1 manual (p. 7; the maintainer pasted it 2026-11-17; not verified on a unit):** six operators OP1-OP6, each a sine
  oscillator that is a carrier or a modulator; the ALGORITHM encoder sets how they connect. EDIT turns the bottom keyboard into function
  selectors (the selected one's LED stays lit); pressing EDIT again returns to the main display.
  - **OP1-OP6:** a short press opens that operator's parameter page (the last page shown is kept per operator); a long press mutes it
    (restored when the voice changes). **PIT:** the pitch envelope. **MONO / POLY:** one note, last-note priority / up to 12 voices.
  - **GLO:** algorithm, feedback, oscillator sync, transpose, voice template, rename and copy. *Init voice:* KNOB 1 clockwise resets every
    parameter. *Rename:* KNOB 2 clockwise opens the panel (OP1-OP5 pick or delete a character, KNOB 3 chooses it, SELECT confirms; KNOB 2
    counterclockwise cancels). *Copy voice:* KNOB 3 clockwise opens the voice panel (counterclockwise leaves), KNOB 4 picks the source,
    SELECT confirms overwriting the target.
  - **Pages:** the SELECT knob turns through a section's pages (at most 6 per operator); KNOB 1-4 set the parameters.
- **Combo function keys (manual 4.2, pasted 2026-11-17):** some keys carry a secondary
  label, **active only in Edit mode**; outside it the same key does its primary job (the ten black keys are ordinary notes). In Edit mode
  all 8 OP/PIT/GLO keys blink (selectable) and the chosen one is solid; MONO/POLY show the mode (active solid, inactive blinking).
  The table: OP1-OP6 select that operator for editing (LED solid when selected); PIT the pitch envelope; GLO the "Globe" (global) parameters,
  listed here as algorithm, feedback, oscillator sync and transpose (section 3's EDIT text adds voice template, rename and copy); MONO one
  note at a time; POLY up to 12.
  The LED states are the firmware's: the core does not expose LEDs, so the host cannot draw them yet.
- **Unknown, to find out:** the flash chip revisions that matter (the kit says newer units differ; `fm1-emu` can model the Puya P25Q80 and
  the UC25HQ80), and the sound engine's interface (closed binary on stock firmware).

Sources: https://github.com/joakimeriksson/fm1-emu · https://github.com/cicloid/awesome-fm-1 · https://kagaimiq.github.io/jielie/cpu ·
https://gitee.com/embview/fw-AC792_SDK · docs/fm-1_reference-kit.md

## From M-VAVE's own documents (read 2026-11-18)

The maintainer put three official files in `docs/`, **kept on this machine only and ignored by git** (they are M-VAVE's, not ours to redistribute; the stock `FM-1.fwsc` is a vendor binary, rule 2): the **FM-1 manual** (`FM-1-manual.pdf`, English then Chinese, the specifications page dated 2026-08-10), the **MIDI table** (`FM-1 MIDI EN.docx`), and the **stock firmware** (`FM-1.fwsc`, SHA-256 `d017fb65e70aca97ee29d944470c3515a74581df5c0576bf70de917c7b2b61d5`, 718,196 bytes; its version is not stated in the file we read). They describe the **stock** firmware. The MIDI side is in `docs/FM-1-MIDI.md`.

**The stock firmware boots in FM1-NTT's core** (`fwprobe`, 2026-11-18, on the Mac): not halted after 3 emulated seconds, and its screens match the manual: the home screen is the voice number and name on a grey ground with a yellow oscilloscope line and the battery icon (`001 PIANO 3`), FX shows a 3 x 2 grid (Filter, Reverb, Delay, Distort, Chorus, Phaser) with a red dot under the selected slot, and OCT+ opens an `Octave` popup (it read `1`, where the manual's example reads `+1`). Not run on a unit.

**Facts the manual adds (stock firmware):**
- **Hardware:** 1.54 inch TFT, 27 silicone keys, one 300-degree volume knob and seven 360-degree knobs, built-in speaker (mutes when headphones are in; use a TRS plug or the speaker may not mute), 3.5 mm audio out, 3.5 mm MIDI in (a 3.5 mm to MIDI adapter, 31250 baud), USB-C, a power switch, 161.5 x 96.5 x 28.6 mm, 251 g, about 12 hours on the battery, BLE MIDI in the 2403 to 2478 MHz band at up to 9 dBm. USB MIDI, BLE MIDI and the 3.5 mm MIDI input **all work at once**.
- **Notes:** MIDI note = key position + 53 + octave x 12 + transpose, so the default range (octave 0, transpose 0) is F3 to G5, which is the 27 keys at notes 53 to 79. **Octave is -3 to +3** (OCT- and OCT+ together reset octave and transpose; the LEDs show the sign and, by blink rate, the size); **transpose is 0 to -12 or 0 to +12 semitones**, set by holding OCT- or OCT+ and turning ALGORITHM, saved with the global settings. Polyphony is **12 voices**; MONO is last-note priority, with glide (Full Time or Fingered, 0 to 100 meaning 0 to 1 s).
- **Voices:** 128 presets in four banks A to D (32 each; a DX7 bank dump asks which bank to write); each voice has six operators with six pages each (tune and level; two envelope pages; sensitivity and mode; two keyboard-scaling pages), a pitch envelope, and GLO (algorithm 1-32, feedback 0-7, oscillator sync, transpose 0-48, init, rename, copy). **Reset Patches** (GLO page 2) puts a bank back to **the last imported bank**, not to the factory voices.
- **Global (GLO, two pages):** MIDI channel (ALL or 1-16), effect channel (1-16, default 2), pitch bend up and down (1-24 each); then reset, global velocity (1-127) and glide.
- **Effects:** six slots in a chain you can reorder (SEL picks one up and a turn moves it): filter (low-pass, band-pass, high-pass; cutoff 100 Hz to 20 kHz in 100 log steps; Q 0-100), reverb (room, hall, plate), delay, distortion, chorus, phaser; each on or off (KNOB 4).
- **ARP:** Up, Down, Inclusive, Exclusive, Random, Order, Repeat; octave 1-4, rate, tempo 30-300 BPM, gate 20-100%, swing 50-75%, latch, sync to external MIDI clock. **SEQ:** 16 patterns of 16 steps, each with its own voice (the PRESETS knob does nothing while SEQ is on), recording by playing, chain 1-16 patterns, step count 1-16, gate, swing, sync, transpose -24 to +24; patterns are saved to flash by themselves. **ARP and SEQ exclude each other** ("Disable SEQ to enter" / "Disable ARP to enter").
- **Other:** HOME held toggles Bluetooth (LED blinking: on, not connected; solid: connected; off: Bluetooth off); SAVE held writes the voice, effects, sequences and globals to flash; a full-screen LOW BATTERY warning stays until charging.
- **EDIT-mode key order, settled in the emulator (2026-11-18):** with the stock firmware in EDIT, pressing the black keys F#3, G#3, A#3, C#4 and D#4 (note keys 1, 3, 5, 8, 10) selects **OP1 to OP5**, F#4 (13) **OP6**, G#4 (15) **PIT**, A#4 (17) **GLO**; C#5 and D#5 (20, 22) are MONO and POLY (a mode change with no screen change, so by position, not seen). That is the order the README assumed; **still not tried on a real unit.**

**Corrections to what was written before:** the reference kit says octave shifting spans 8 octaves; the manual gives -3 to +3 (seven octaves) plus a separate transpose of +/-12. The older "closed binary on stock firmware" line stands for the sound engine, but the stock firmware *runs* in the core.
