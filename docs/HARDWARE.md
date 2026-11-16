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
- **Unknown, to find out:** the flash chip revisions that matter (the kit says newer units differ; `fm1-emu` can model the Puya P25Q80 and
  the UC25HQ80), and the sound engine's interface (closed binary on stock firmware).

Sources: https://github.com/joakimeriksson/fm1-emu · https://github.com/cicloid/awesome-fm-1 · https://kagaimiq.github.io/jielie/cpu ·
https://gitee.com/embview/fw-AC792_SDK · docs/fm-1_reference-kit.md
