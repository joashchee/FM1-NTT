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
- **Unknown, to find out:** the sound engine's interface (closed binary on stock firmware). **The flash chip revisions are answered (2026-11-22, below): units sold since late 2026 carry a UCUN UC25HQ80 (JEDEC `B3 60 14`) in place of the Puya P25Q80H (`85 60 14`); `fm1-emu` models both and `FM1-NTT` can now choose.**

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

- **Which chip "B36014" is** (corrected 2026-11-22: this section first called it a chip revision, from the README's words "the newer chip (B36014)"): **it is the flash chip's JEDEC id**, `0xB36014`, the UCUN UC25HQ80 (read from m-bootloader's README, below). The README also names **m-bootloader 2.4-395e6cc**: `2.4` is that project's `VERSION`, and `395e6cc` is the merge commit of its "m-bootloader 2.4" release (2026-11-13, checked on GitHub).
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
- **m-bootloader** (https://github.com/jvitkauskas/m-bootloader, MIT): an open bootloader for the FM-1 and SMK-37 Pro (firmware check, recovery menu, download mode, USB MIDI install and backup). Its `VERSION` is 2.4 and `395e6cc` is its 2.4 release commit, so FM-1 Studio's "m-bootloader 2.4-395e6cc" is this project's release (checked on GitHub, 2026-11-22). Read from its README, `NOTICE.md`, `docs/STOCK-V15-V16.md` and the headings and flash sections of `docs/SPEC.md` (not the source):
  - **Two flash parts, three JEDEC ids:** the Puya P25Q80H `85 60 14` (most units), the UCUN UC25HQ80 `B3 60 14` (**FM-1s sold from late 2026**), and `EB 60 14` (the SMK-37 Pro's, a Winbond-style status model); capacity code `0x14` is 2^20 bytes, 1 MiB. Same plain NOR commands (9F JEDEC, 03/0B/3B/6B reads, 05/35 status, 06, 02 page program of at most 256 bytes, 20 sector erase of 4 KiB, D8 block of 64 KiB, 4B unique id); the stock configuration reads with the quad-output 6B.
  - **The newer flash is little tested:** the author reports one such unit that did not start after bootloader 1.1 or 1.2 (cause unknown), another running 1.3 and 2.0 (one owner's report) with firmware, menu and download mode working, and a MIDI-mode reinstall of the same firmware working. A firmware that knows only the Puya id takes the newer unit for an unknown part (per `fm1-emu`'s notes): its saves stay in RAM and its update loader refuses to install.
  - **Boot and recovery:** hold REC (FM-1) or SEQ REC (SMK-37) while switching on for the chip's USB download mode; hold PLAY or SEQ PLAY for the recovery menu, which also opens by itself when the firmware fails its checksum (the stock bootloader checks only the file table). A USB_KEY dongle forces download mode even with a broken bootloader.
  - **Stock firmware:** M-VAVE's 016 (2026-10-21) carries a JieLi AC79 SDK 1.2.13 bootloader (015: 1.1.9); going from 016 back to 015 leaves garbage presets and wrong pitch with any bootloader, because 016 overwrites the presets once it runs and official updates do not restore them (only a backup does). Our `docs/FM-1.fwsc` is **016** (its SHA-256 `d017fb65...` is the one the note gives for M-VAVE's 016, checked 2026-11-22; the core reads its identity as `FM-1_016`).
  - **Tested only by its author:** on an FM-1 and an SMK-37 Pro, on the author's say-so.

### What `pi32v2-isa` lists as open (read 2026-11-22: `docs/GAPS.md`, `hw/README.md`; the author's runs on one FM-1 and an SMK-37 Pro)

Facts for core tests, each **from the README's words, not run here**:
- **Traps:** misaligned word accesses always trap (7 instructions late, and move the aligned-down word); divide by zero and the FPU traps are enabled under EMU_CON; one `rep` runs at most 32 times; the PC-limit guard trips once until DBG_CON bit 18 (`fm1-2026-11-15-excprobe`).
- **Forms that reset the chip:** the unnamed (`??`) vector compares, the six `mem2.hh.i` multiplies, the four non-round-tripping float branch conditions (`!=`, `u==`, `uno`, `ord`), and `btbclr`, `syscall`, `idle` in the author's ROM runner (observations, not proof of a trap).
- **Revision-3 instructions** (`popcnt`, `sat8`, `vsat16`, `unpack4`, `sum` with saturation) **do not exist as such on this chip:** they behave as the older form whose encoding differs by one selector bit; `popcnt.rr` does nothing.
- **Interrupts and the second core:** TIMER5 delivery is gated at icfg bits 8 and 9; software interrupts 0 to 7 reach vectors 120 to 127; `idle` wakes through TIMER5; WFE does not block in the tested contexts and no event-routing rule is established; a locked shared counter keeps all 2000 increments from two cores while an unlocked one keeps 1455 (`docs/CPU1.md` there).
- **Open there:** `callns.r`, `gotoss.xl` and `btbclr`'s buffer effect, the `.c` pushes, what a revision-3 core does, and `copex.ppi` (different results for equal inputs).

### Flash layout and package format (m-bootloader `docs/SPEC.md` §5, §6 and §11.6, read 2026-11-22; **checked against the stock 016 in the core**)

From the specification's words (the author's, from the stock images and a unit); the layout items marked "checked" decode as stated from the flash our core builds from `docs/FM-1.fwsc` (`core/tests/stock.rs`, `the_flash_layout_is_the_one_the_bootloader_spec_describes`, which also reproduces the spec's cipher and CRC test vectors).
- **Checksums and cipher:** CRC16/XMODEM (poly `0x1021`, init 0, check `0x31C3` for `123456789`) and the IEEE CRC-32. The flash cipher "ENC" is its own inverse: a 16-bit state from the key, XORed (low byte) into each byte, one left shift per byte (xor `0x1021` on a carry), **one step per byte, not eight**; restarted per object.
- **Layout (checked):** a 32-byte header at 0 (ENC `FFFF`: CRC16 of bytes 2..31, burner field `0x0220`, filesystem size `0xFF000`, version `0x10`, alignment 16; CRC `0xDCA1` on every stock image), then 32-byte records, each decoded on its own: `0x20` `uboot.boot` (address `0xA0`), `0x40` `isd_config.ini`, `0x60` `app_dir_head` (address `0x4000`, length `FFFFFFFF`), `0x80` `key_mac` (address `0xFF000`, length `0x1000`). **016's boot container is `0x3898` bytes (015's `0x3830`) and its configuration `0x2F3` bytes (015's 699).**
- **Address map:** boot area `[0, 0x4000)`; the application from `0x4000`, encrypted per 32-byte line with `unit key XOR ((q >> 2) & 0xFFFF)` (the unit's key here is `0x980F`); entry `0x02000120` through the cached window at `0x02000000`; RAM from `0x01C00000` (the boot payload at `0x01C02000`); the retained boot/update area at `0x01C7FD80`.
- **Regions (015, FM-1):** firmware `[0x4000, 0x93000)`, VM `[0x93000, 0xE9000)`, BTIF `[0xE9000, 0xEA000)`, USR resources and sounds `[0xEA000, 0xFC000)`, the final reservation `[0xFF000, 0x100000)` (the chip's key area). **016's firmware is larger: the core's flash from the 016 package is written up to `0xA8FFE`**, so its VM starts later; the spec's table is 015's. fm1-base apps keep their own records inside the VM (`0xDA000` to `0xDE000`, `0xFC000`: TRIO's `APP_STORE_MAP`).
- **A `.fwsc` package** (checked: the core reads it): an image with the identity woven in, byte 47 of each 48-byte block carrying `(identity char + i + 1) mod 256`, followed by `0x7D` sentinels; the first 64 bytes are ENC `FFFF`, then 80-byte file records; type 0 `flash.bin` is a raw encrypted flash image from 0 (015: `0x93000` bytes, CRC `9B24`), type `0x64` `ota.bin` the staged updater (`19969` bytes, CRC `A1C9`), type `0x32` `USR`. An installer writes only `[0x4000, size)`: never the boot area or `0xFF000`.
- **Updates:** the firmware stores the staged updater in its VM (FM-1: `0xE3000`, request `0xE7F00`) and restarts into the bootloader with a request in retained RAM (80 bytes at `0x01C7FD88`: CRC16, type, result, name, private arguments, the updater's flash address); a request is tried once per power cycle.
- **m-bootloader's MIDI mode:** a class-compliant USB-MIDI device `FM-1 Update` (`1209:0002`), frames `F0 ... F7` with 8-to-7 packed bytes (`00 59`, command, 24-bit length, body, complement of the body's sum); `F0 22 24 35 7F F7` starts an install, `...7D` asks for download mode, `...7E` restarts; commands `0x11` identity (27 bytes), `0x12` (protocol, chip key, capacity, **JEDEC ID**, unique ID, version), `0x23` read raw flash, `0x24` CRC-32 of a range, `0x25` run a RAM app. **Stock firmware's own update service uses the same frame and, on some versions, its `0x21` erases a sector unchecked.** None of this is emulated here (the core models neither the bootloader nor the update route).
