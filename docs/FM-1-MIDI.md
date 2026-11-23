# The FM-1's own MIDI implementation (stock firmware)

From M-VAVE's MIDI table (`docs/FM-1 MIDI EN.docx`, kept locally, not in the repository) and its manual, read 2026-11-18, restated in our own words. **It describes the stock firmware; a custom firmware may differ.** Not tried on a unit; the stock firmware boots in the core (`docs/HARDWARE.md`) and **what was sent to it in the emulator is in "What was tried" at the end**.

**Channels:** the **note channel** defaults to ALL (omni); the **effect channel** defaults to **2**; real-time messages carry no channel. Both are set in GLO page 1.
**Warning in the table:** CC 1 sent on the effect channel is the filter type, not the mod wheel. Send mod wheel on another channel. Pitch bend works on any channel.

## Note channel

| Message | Bytes | Data | What it does |
|---|---|---|---|
| Note On | 9n | note, velocity 1-127 | plays (velocity 0 is a Note Off) |
| Note Off | 8n | note | releases |
| Program Change | Cn | 0-127 | voice 001-128 |
| Pitch Bend | En | 14 bit | within the bend range set in GLO (1-24 semitones each way) |
| Channel aftertouch | Dn | 0-127 | channel aftertouch |
| Mod wheel | Bn | CC 1 | modulation |
| Sustain | Bn | CC 64 | sustain (releases when off) |

## Effect channel: CC 0 to 23

| CC | Effect | Parameter (range) | CC | Effect | Parameter (range) |
|---|---|---|---|---|---|
| 0 | Filter | switch (0 off, 1+ on) | 12 | Distortion | switch |
| 1 | Filter | type 0-2 (LPF, BPF, HPF) | 13 | Distortion | gain 0-100 |
| 2 | Filter | cutoff 0-107 | 14 | Distortion | tone 0-100 |
| 3 | Filter | Q 0-10 | 15 | Distortion | level 0-100 |
| 4 | Reverb | switch | 16 | Chorus | switch |
| 5 | Reverb | type 0-2 (room, hall, plate) | 17 | Chorus | frequency 0-100 |
| 6 | Reverb | decay 0-100 | 18 | Chorus | depth 0-100 |
| 7 | Reverb | mix 0-100 | 19 | Chorus | mix 0-100 |
| 8 | Delay | switch | 20 | Phaser | switch |
| 9 | Delay | decay 0-100 | 21 | Phaser | frequency 0-100 |
| 10 | Delay | rate 0-100 | 22 | Phaser | depth 0-100 |
| 11 | Delay | mix 0-100 | 23 | Phaser | mix 0-100 |

The manual's own ranges differ a little (it shows the filter's Q as 0-100 and the cutoff as 100 Hz to 20 kHz in 100 log steps): **the table's CC ranges are the wire values**, and how they scale onto the screen's values is for the emulator to show.

## System real-time

| Byte | Message | Response |
|---|---|---|
| F8 | Clock | follow the external tempo (ARP and SEQ sync) |
| FA | Start | start from step 0 |
| FB | Continue | **the same as Start** (no resume) |
| FC | Stop | stop |

## System exclusive

- **Single-parameter write:** `F0 43 10 pp qq vv F7`; the parameter number is `pp x 128 + qq` (0 to 155), the value is `vv`. The table does not list the numbering, **but it is the DX7's** (see "What was tried").
- **Voice dumps:** a standard Yamaha DX7 bank (32 voices) is accepted; the unit then asks which bank (A to D) to write it to.

## What the FM-1 sends

Note On and Off with velocity, from the keyboard, the arpeggiator or the sequencer, on the interfaces below (all three work at the same time): **USB MIDI** (a standard class-compliant device), **BLE MIDI** (Windows, Mac, iOS, Android), and the **3.5 mm MIDI input** (31250 baud, a 3.5 mm to 5-pin adapter).

## What was tried in the emulator (2026-11-18, stock firmware, a Mac; not a unit)

Sent through the emulated 3.5 mm MIDI jack (`fwprobe midi:`), read off the screen and, for the effects, off the rendered audio compared sample by sample with a matched run (the emulator is deterministic: two identical runs differ by 0.0000).

- **Program Change works, across all four banks.** `C0 05` showed `006 HonkyTonk1`, `C0 1F` `032 B-3 II`, `C0 20` `033 ROCK ORGAN`, `C0 7F` `128 155Sec.efx`, `C0 00` back to `001 PIANO 3`: value N is voice N+1, as the table says. So a preset can be reached by MIDI with no panel clicks, and the 128 names can be read by walking it.
- **The effect CCs mostly did not change the sound.** On channel 2 (`B1`), one note held 1 s and released for 1 s, FX001:
  - **Changed the sound:** CC 1 (filter type, 0 to 2: diff 0.19 to 0.20 of the signal), CC 7 (reverb mix) and CC 11 (delay mix) (both about 0.80 for 100 to 20).
  - **Changed nothing audible:** the six switches (CC 0, 4, 8, 12, 16, 20, tried with 0, 1 and 127), the filter cutoff and Q (CC 2, 3), the other reverb, delay, distortion, chorus and phaser parameters, with all parameters set strongly. The same CCs on channel 1 changed nothing.
  - **The effects do work in the emulator:** turning the filter on from the panel (FX, then KNOB 4) changed the sound by 1.27 of the signal, and CC 0, 2 and 3 on top of that still changed nothing.
  - **Not known why:** the firmware may ignore those CCs, honour them only in a state this probe did not reach (the screen did not show the values change either), or scale them differently from the table; the probe is one note and a short time. **So the table's effect CCs are not confirmed**, and a port must not rely on them. Not tried: a firmware other than stock.
- **The SysEx parameter write works, with the DX7's parameter numbers** (`F0 43 10 pp qq vv F7`, number `pp x 128 + qq`; tried over both the 3.5 mm jack and USB MIDI, which gave the same results to three decimals). With OP1's Out Level at 86 on `001 PIANO 3`, writing parameter 121 = 50 made EDIT's OP1 page read `Out Level 50`, and the sound changed. That is the DX7 layout: operators 6 down to 1 at 21 bytes each (OP1 is 105 to 125, its output level the 17th, so 121), then the pitch envelope and the globals. Audible, compared with a run that sent an invalid parameter (the timing control, 0.003 apart): OP1 level 50 by 0.37 of the signal, **algorithm** (134) = 31 by 1.90, **feedback** (135) = 7 by 0.04, **transpose** (144) = 36 (+12) by 0.85 and 12 (-12) by 1.28 (24 is centre). **No audible change:** the name's first character (145, nor did the home screen change) and oscillator sync (136, which may be silent for this voice). Not tried: parameters 0 to 120 and 125 to 133, 137 to 143 and 146 to 155 one by one, SAVE and a reboot (a write **survives a Program Change away and back**, 2026-11-22 `sysexpc`: the first twelve harmonics 0.2 dB from the written sound, 47 dB from the plain one; whether it reaches flash is unknown), and any firmware but stock.
- **No SysEx parameter reaches the effects (2026-11-18).** With the filter on from the panel, every parameter number from 156 to 1023 was written with the value 100 (and 156 to 600 with 1 and with 0), the sound compared with a timing control after each; **none changed it**. The same harness did see parameters 121, 134 and 135 change it. So the effects are not at numbers 156 and up in this message, and the effect CCs (above) did little: **on stock, the effect chain is reachable from the panel only, as far as tried** (by the effect CCs only filter type and the reverb and delay mixes). **Repeated over USB MIDI with the cable plugged before boot, the same ranges and values: the same result, nothing changed** (the same harness saw parameters 121, 134 and 135 change the sound). **Other device bytes (the third byte, `F0 43 xx`), over the jack:** with parameter 121 written under every value 0 to 127, **only `10` changes the sound** (the firmware takes the DX7's channel-1 parameter change and ignores the rest); then, with the filter on, value 100 under headers `11` to `1F` for parameters 0 to 47, `20` to `7F` for 0 to 11 and `00` to `0F` for 0 to 23, **nothing changed the sound**. Not tried: values between 2 and 99 other than 100, numbers above 1023 (and 156 and up under the other header bytes), a different manufacturer byte, a SysEx form of another shape, NRPN.


## What FM1-NTT does with the table (2026-11-19)

The instrument host can answer the effect channel like the stock firmware's table says (`core/src/stockmap.rs`; the global **STOCK EFFECT CCS**, off by default). Controllers 0 to 23 on the effect channel go to the first effect of that name in the package's chain; a switch bypasses the effect (not saved in a package); a missing effect is ignored. The note channel is untouched, so CC 1 there is still the mod wheel. **How a value lands on our effects is our own** (they are written fresh and fitted to nothing): cutoff is 100 Hz to 20 kHz in 100 log steps, Q 0-10 is our 0-100, a reverb's type is its damping and its decay its room, a delay's decay is its feedback and its rate a shorter time, chorus and phaser frequencies are 0.1 Hz units with 0 the slowest. This follows the table, not a measurement: the stock firmware itself reacted to only three of these controllers in the emulator (above).

## More tried in the emulator (2026-11-19, `core/tests/stock.rs`)

- **All 128 Program Changes** each draw a screen unlike the one before and come back identical (values 0 to 127 are voices 001 to 128).
- **A DX7 bulk dump** (`F0 43 00 09 20 00`, 4096 bytes, checksum, `F7`) over the jack changes the screen (the bank question); it must be paced like a real 31250 baud wire: 16 bytes then 10 ms worked, the whole dump at once did nothing.
- **FB (Continue) and FA (Start)** leave the machine in exactly the same state (identical sound and screen after a Stop); that they start anything was not shown.
- **CC 1 is the filter type on channel 1 as well as channel 2, sample for sample, with the note channel at its default ALL.** The table's warning that CC 1 on the effect channel is not the mod wheel is right, but it does not mean channel 1 keeps a mod wheel: with omni notes both channels act as effect controllers. A set note channel (GLO page 1) was not tried.
