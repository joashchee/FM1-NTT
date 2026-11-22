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

## From the FM-1 reverse-engineering notes (read 2026-11-20)

Source: `AL-255/FM-1-RE`, its `README.md` and `docs/architecture.md` (WTFPL; static analysis of the **stock V13 firmware, dated 2026-07-03**). Its disassembly
and extracted loader binaries were **not** read or copied (rule 2); function addresses are left out here. Written in our own words, and **none of it was run
here or checked on a unit**: it is that project's reading of one firmware version, with the confidence it states (addresses and sizes exact for that image,
what a function is for tagged high, medium or low).

- **Where it agrees with what is above:** JieLi AC791N / WL82; RAM at 0x01C00000 (`.data` first, then `.bss`); execute-in-place flash at 0x02000000;
  a six-operator engine that is a port of Google's msfa (the Dexed core), proven by byte-exact DX7 lookup tables; 12 voices; 240 x 240 SPI LCD; 41-key matrix,
  two encoders, two ADC wheels; USB-MIDI, UART MIDI, BLE-MIDI.
- **Where it adds or differs:**
  - **CPU:** one **pi32v2** core at **240 MHz** (a 24 MHz crystal through a PLL), 16-bit little-endian instruction words. It says "CPU" in the singular; the "two cores" above comes
    from `awesome-fm-1`. Which is right for the FM-1 is open. The notes call `JL-BR22` strings library lineage, not the chip's name.
  - **Engine numbers:** **44118 Hz** sample rate (not 44100), **64-sample blocks**, integer fixed-point, 12 voices of 468 bytes each, a ping-pong buffer pumped by DMA,
    then the effects chain (reverb, phaser, chorus, filters) after the mix, then the internal DAC. Our `fm6` renders at the host rate, so a stock-firmware comparison should
    allow the 44118 against 44100 difference (0.04 %, 0.7 cent) before blaming the engine.
  - **Flash: 1 MB, linearly mapped.** XIP address is flash offset minus 0x4120 plus 0x02000000 for the app image (`app.bin` at flash 0x4120, 0x8E59C long). Layout: header and
    second-stage loader at 0, **app area from 0x4000 (encrypted with the chip key)**, a **VM region at 0x94000** (0x55000: the key-value store for config, the Bluetooth address and the wheel
    calibration), a Bluetooth pairing region at 0xE9000, **user patches at 0xEA000** (0x12000), and the chip's key and MAC data in the last 4 KB (0xFF000).
  - **Boot:** the mask ROM loads the second-stage loader from flash 0xA0, which maps the app area and jumps to the app; the app clears `.bss`, copies `.data` from flash, copies a boot-parameter struct,
    raises the PLL, starts an RTOS and the UI task. Interrupts go through one C dispatcher and a RAM vector table. **This is the sequence a core that runs the stock firmware would have to reproduce.**
  - **OS:** a FreeRTOS-derived kernel with JieLi's task and timer API, a device framework, an audio-stream pipeline and a dlmalloc heap.
  - **USB:** a composite device, **"FM-1 Midi" plus "FM-1 Audio" (UAC1, 24-bit stereo)**, strings "Jieli Technology" and a serial that is the hex chip id; the descriptor template carries VID:PID
    `4c4a:4155` and captures of a running unit show `4c4a:c755`. USB-MIDI uses a 64-byte bulk pair.
  - **MIDI:** DX7 SysEx works: a 32-voice bulk dump (4096 bytes plus checksum), single-voice dumps, and 7-byte parameter changes into an edit buffer (155 bytes, packed to 128 for storage). Notes, CC 1, 2, 4
    and 64, and 14-bit pitch bend are handled; **a vendor SysEx with the magic bytes `F0 35 59 F7`** carries configuration and update traffic. The arpeggiator and sequencer live in the firmware, not the host.
  - **Update:** a UFW-format package (here a `.fwsc`, chip name AC791N) holds the flash image and update scripts; the update is a USB-MIDI session with a loader on the device. **The notes state that
    no recovery path (rollback, interrupted-write safety) has been shown for the single-bank layout**; do not flash anything on that evidence.
  - **Bluetooth:** a full BLE stack with BLE-MIDI on GATT; the Classic profiles (A2DP, AVRCP, HFP) are SDK carryover and unused.
  - **Where the flash goes:** by function count the image is mostly Bluetooth (719 of 2062 entries), the RTOS (224), storage (215) and the UI (165); the synth engine is 20 entries and the MIDI layer 28.
- **For this project:**
  - **The 44118 Hz engine rate** is a candidate for the "0.9173 engine factor" measured on OPERATOR (`docs/PORTING.md`): that is about 44100 / 48000 (146.8 cents flat), nowhere near 44118, so the stock engine's rate does **not** explain it; OPERATOR running its engine at a different rate is the likelier cause.
  - **The patch store** (user patches at 0xEA000, 128-byte packed voices) is what a `.fm1save` should eventually model if it is to match the unit; today it does not.
  - **The vendor SysEx magic** `F0 35 59 F7` is worth a line in `docs/FM-1-MIDI.md` once seen on a capture (not confirmed here).
  - **Open:** the one-core or two-core question, the flash chip revisions, and the effects chain's algorithms (named, not described).

Source: https://github.com/AL-255/FM-1-RE (`docs/architecture.md`, WTFPL).

## From FM-1 Studio's README and the maintainer (2026-11-22; README only, no source read)

FM-1 Studio (https://github.com/TimSamshuijzen/fm1studiocom, fm1studio.com) is **GPL-3.0**, so it is a black box (`CLAUDE.md` rule 3): only its public README and the
maintainer's note were used, none of `src/`, `firmware/` or `examples/`. **Nothing here was run, and none of it is verified against a unit by this project.**

- **Chip revisions:** the README says it is tested on one FM-1 with **the newer chip, B36014**, and **m-bootloader 2.4-395e6cc**. This is the first named chip revision, and the first named
  bootloader version, in our notes; it bears on the open "flash chip revisions" question above (the kit says newer units differ). It does not say what differs.
- **The processor** is named there as JieLi **pi32v2**, as above.
- **A third-party compiler for it:** **pi32v2-llvm** (Julius Vitkauskas, https://github.com/jvitkauskas/pi32v2-llvm), LLVM with a pi32v2 target, described by its author as **experimental**;
  FM-1 Studio builds clang 23.1.5 and ld.lld from commit `60b0340f47083cf088903030a9b55d8a65525dde` (release `pi32v2-23.1.5-5`) to make `.fwsc` packages. Its licence is Apache-2.0 with LLVM
  exceptions (**read from the project's own `LICENSE.TXT`, 2026-11-22**). This answers "JieLi's toolchain is clang-based (unconfirmed for pi32v2)" above in part: JieLi's own is clang 4.0.1 for
  pi32v2, a binary from JieLi's server with no source (README's words).
- **The programming model it names:** programs are **C, `float` only, with the FM-1's C library**, written against an `fm1.h` API; FM-1 Studio **simulates that API** and does not emulate the CPUs
  ("code that works in this simulator does not guarantee it will work on a real FM-1").
- **Install path:** a `.fwsc` package is written to the unit **over USB MIDI by fm1-base's installer** (Chrome or Edge, Web MIDI); fm1-base is a separate project, not read here.
- **A firmware gallery:** it loads source of open-source FM-1 firmware from GitHub. Candidates only: each would need its own licence file read (`CLAUDE.md` rule 2).

Source: https://github.com/TimSamshuijzen/fm1studiocom (README).

### The author's other projects (read 2026-11-22; the READMEs, docs and licences, not their source; **nothing was run except the compiler**)

- **pi32v2-llvm, tried:** release `pi32v2-23.1.6-2` compiled a float C function for `-mcpu=r3` (CPUs offered: `ac791n`, `generic`, `r3`), and the vendored `fm1-isa` decoded all 12 resulting instructions (16, 32 and 48 bits) with the same lengths and forms as its objdump. A single small function, not a sweep.
- **pi32v2-isa** (https://github.com/jvitkauskas/pi32v2-isa; code MIT-0, docs and table CC0-1.0): an open instruction table with an assembler, linker and objdump. Its README says every word JieLi's objdump decodes is in it, **1085 of 1171 forms are confirmed on a real AC791N**, and it builds m-bootloader, fm1-base, Felucca and SLOOP with the LLVM toolchain and runs them on a unit (`-mcpu=r3`, FPU `fprev1`). Its sources are public notes, black-box observation of JieLi's free but closed toolchain, and runs on the chip; it says no JieLi code is in it. Gaps it lists: `callns.r`, `gotoss.xl` and `btbclr` effects, and that the `??`-condition forms reset the chip. `docs/CPU1.md` there covers the second core, which bears on the one-core or two-core question above (not read).
- **m-bootloader** (https://github.com/jvitkauskas/m-bootloader, MIT): an open bootloader for the FM-1 and SMK-37 Pro (firmware check, recovery menu, download mode, USB MIDI install and backup). Whether FM-1 Studio's "m-bootloader 2.4-395e6cc" is this project's release is **not checked**.
