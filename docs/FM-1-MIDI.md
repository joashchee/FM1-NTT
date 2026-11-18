# The FM-1's own MIDI implementation (stock firmware)

From M-VAVE's MIDI table (`docs/FM-1 MIDI EN.docx`, kept locally, not in the repository) and its manual, read 2026-11-18, restated in our own words. **It describes the stock firmware; a custom firmware may differ.** Not tried on a unit; the stock firmware boots in the core (`docs/HARDWARE.md`) but no MIDI was sent to it yet.

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

- **Single-parameter write:** `F0 43 10 pp qq vv F7`; the parameter number is `pp x 128 + qq` (0 to 155), the value is `vv`. The parameter numbering is not listed in the table we have.
- **Voice dumps:** a standard Yamaha DX7 bank (32 voices) is accepted; the unit then asks which bank (A to D) to write it to.

## What the FM-1 sends

Note On and Off with velocity, from the keyboard, the arpeggiator or the sequencer, on the interfaces below (all three work at the same time): **USB MIDI** (a standard class-compliant device), **BLE MIDI** (Windows, Mac, iOS, Android), and the **3.5 mm MIDI input** (31250 baud, a 3.5 mm to 5-pin adapter).
