# FM-1 Reference Kit: Emulation, R36S Integration, and Buying Guide

This reference kit compiles research on the M-VAVE FM-1 pocket FM synthesizer, its firmware ecosystem, emulation options, compatibility with an R36S handheld, and purchasing considerations.

## 1. What the FM-1 is

The M-VAVE FM-1 is a low-cost, battery-powered pocket FM synthesizer built around a 6-operator FM engine. Its 32 algorithms combine independent sine-wave operators as carriers and modulators, making it conceptually close to Yamaha DX7-style FM synthesis rather than a subtractive analog synthesizer.

Core hardware and stock capabilities:

- 6-operator FM synthesis engine with 32 algorithms.
- Up to 12 voices in stock V15 firmware; official V16 firmware raises this to 20 voices and adds 8-part multitimbrality.
- 128 factory presets.
- Imports standard FM SysEx tone banks, including DX7-family patch libraries.
- 27 silicone piano-layout keys with octave shifting across 8 octaves.
- 7-mode arpeggiator and 16-step sequencer supporting up to 16 sequences.
- Filter, reverb, delay, distortion, chorus, and phaser effects.
- USB-C MIDI plus charging, Bluetooth LE MIDI, and 3.5 mm MIDI input; all three MIDI paths can operate simultaneously.
- 3.5 mm stereo headphone/line output and a small internal speaker.
- 2,000 mAh internal rechargeable battery, with a claimed runtime of about 12 hours.
- 161.5 x 96.5 x 28.6 mm dimensions and approximately 251 g weight.

The FM-1’s USB-C port is a device port, not a host port. It must be connected to a USB host such as a computer, phone, tablet, or the R36S’s OTG port.

## 2. Why it is interesting

The FM-1 is more than a cheap preset synth. M-VAVE released FM1 Open, an official firmware and development project that includes:

- An open application layer under GPLv3.
- A browser-based Workbench.
- An application SDK.
- A WebAssembly simulator.
- Hardware C drivers.
- An 8-track groovebox framework with 8 sample slots and 16 demo effects.

Important distinction: the application layer is open, but the underlying FM synthesis engine remains a closed binary. This still enables substantial custom software development, including new user interfaces, sequencers, instruments, utilities, and games.

## 3. Custom firmware ecosystem

The FM-1 has developed a rapidly growing custom-firmware ecosystem. The main options are:

| Firmware | Purpose | Best for |
|---|---|---|
| Stock / FM1 Open | DX7-inspired 6-operator FM synth, effects, arpeggiator, sequencer, and official open development platform | Reliable FM sounds, patch exploration, official tooling |
| Felucca | Multi-engine instrument platform with simultaneous instruments, sampler, web editor, and multitrack pattern sequencer | Turning the FM-1 into a compact production device |
| SLOOP | Eight-track performance groovebox built on Felucca engines | Beat-making, live layering, drums, bass, loops |
| Melodee | Casio CZ-style phase distortion, DX-style FM, 808-style percussion, and C64 SID-style chiptune sounds | Retro digital and chiptune sounds |
| S1Vave | 8-voice virtual-analog tweak synth inspired by SH-101/S-1 architecture | Analog-style basses, leads, and sound design |
| FM-1+VA | Virtual-analog firmware option | VA synthesis on FM-1 hardware |
| X0X, Jangada, Groove OS, FoMni | Additional community firmware projects | Specialized workflows and experimentation |
| Doom ports | Games and novelty applications | Demonstrating the platform’s flexibility |

For a musician, Felucca is probably the most consequential project because it changes the FM-1 from a single-instrument synth into a multi-track production device. SLOOP is preferable for immediate beat-making and live layering, while Melodee and S1Vave are better for retro digital, phase-distortion, chiptune, or analog-style synthesis.

## 4. FM-1 emulation

### 4.1 Full emulator

Community projects can emulate the FM-1 itself, including its CPU, display, keys, knobs, audio, USB, and flash. This means many community firmware packages can run unchanged in an emulator.

Notable projects:

- `fm1-emu` by Joakim Eriksson: runs unmodified FM-1 firmware in a browser, natively, or as an AU/VST3 plugin.
- `FM1-emulator`: emulates the FM-1 CPU, screen, keys, knobs, and audio so firmware runs unchanged in a browser, on a computer, or as a DAW plugin.
- FM-1 Firmware Hub: provides access to firmware files and an online emulator.
- FM1 Studio: Android app that runs SLOOP, FM1 MOVE, Felucca, sloopDX, Ghoulbox, and zp12 with native firmware displays, touch controls, piano roll, and sound editors.
- FM1 DECK: runs unmodified Felucca, JIANT FM, SLOOP, and Hortator firmware behind a touch panel, with up to three firmwares in one rack.
- Harbor – FM-1 Synth Emulator: Android option for previewing community firmware without owning or flashing hardware.

### 4.2 Official FM1 Workbench

M-VAVE’s official Workbench is available at:

```text
https://open.m-vave.com/fm1-workbench
```

It provides a browser-based WebAssembly simulation, testing, and development environment for FM1 Open. It is excellent for editing sounds, rhythms, and FM1 Open projects, but it is not necessarily a universal full-chip emulator for every third-party firmware.

### 4.3 Emulation versus hardware

Emulation is excellent for previewing and auditioning firmware, but it does not fully replace hardware testing.

Emulation can differ from real hardware in:

- Audio latency.
- USB-MIDI timing.
- Encoder and button feel.
- Battery behaviour.
- Hardware-specific bugs.
- Flash-chip revision compatibility.
- USB audio behaviour.
- Real-world stability.

A firmware that runs well in emulation may still have flashing, storage, USB, or hardware-revision issues on a physical FM-1.

## 5. R36S compatibility

### 5.1 Can the R36S emulate FM-1 firmware?

Yes. The R36S is powerful enough to run FM-1 emulators and preview community firmware, especially through an Android build.

The R36S uses a Rockchip RK3326 SoC with four Cortex-A35 cores, a Mali-G31 MP2 GPU, and 1 GB DDR3L RAM. While modest by modern phone standards, this is sufficient for an FM-1 emulator because the emulated target is a small embedded synth SoC, a 240 x 240 display, and relatively lightweight audio DSP.

The bottleneck is not CPU power. It is software support and operating system.

### 5.2 Operating system options

| Environment | FM-1 emulation suitability | Notes |
|---|---|---|
| Stock ArkOS | Poor to awkward | Linux-based retro-gaming OS; not designed for Web MIDI, WebUSB, or Android music-app workflows |
| Android firmware | Good | Best route for browser-based emulators and Android FM-1 apps |
| Native Android emulator app | Best | Avoids browser permission and Web MIDI quirks |

### 5.3 Recommended R36S setup

Use an Android build such as Andr36oid, then install one of the following:

- FM-1 Firmware Hub browser emulator.
- FM1 Studio.
- FM1 DECK.
- Harbor – FM-1 Synth Emulator.

For evaluating firmware before flashing, start with FM1 Studio or the browser emulator. They provide a much larger interface than the FM-1’s 1.54-inch screen and make it easier to assess a firmware’s workflow.

### 5.4 R36S hardware constraints

The R36S has:

- RK3326 quad-core Cortex-A35 processor.
- 1 GB RAM.
- 3.5-inch 640 x 480 IPS display.
- Dedicated USB-C OTG port.
- 3.5 mm headphone jack.
- No built-in Wi-Fi or Bluetooth in the base hardware.

This makes it suitable for lightweight MIDI sequencing, FM-1 emulation, chiptune apps, and simple synth apps. It is not ideal for full Android DAWs, large sample libraries, or multi-track production.

## 6. Connecting FM-1 to R36S

### 6.1 MIDI connection

Connect the devices as follows:

```text
R36S OTG USB-C port -> USB-C data cable -> FM-1 USB-C port
```

Use the R36S port labelled OTG, not the port labelled DC. Only the OTG port carries USB host data.

The FM-1 presents itself as a standard, driver-free USB MIDI device. Android supports USB host/OTG MIDI through its MidiManager APIs, so the R36S can send MIDI notes, CCs, program changes, pitch bend, and MIDI clock/start/stop to the FM-1.

### 6.2 Audio monitoring

For reliable sound, monitor the FM-1 through its own 3.5 mm stereo output or internal speaker. This avoids relying on USB audio routing through the R36S.

The FM-1’s USB connection can also expose audio in some firmware versions as a UAC1 stereo 16-bit interface, but USB audio support is newer and less universally dependable than USB MIDI.

### 6.3 Power

The FM-1 has its own 2,000 mAh battery, so it does not need bus power from the R36S. A normal USB-C data cable should be sufficient.

A powered USB hub becomes relevant only if additional bus-powered peripherals are attached.

### 6.4 Cable requirements

Use a USB-C data cable. A charge-only cable will power or charge the FM-1 but will not establish MIDI communication.

## 7. Recommended R36S workflow

Treat the R36S as a MIDI sequencer, controller, and lightweight app host. Let the FM-1 remain the sound engine and audio output.

Recommended workflow:

1. Install Android firmware on the R36S.
2. Install FM1 Studio, FM-1 Firmware Hub browser emulator, or another compatible emulator.
3. Connect the FM-1 directly to the R36S OTG port using a data-capable USB-C cable.
4. Select the FM-1 as the MIDI destination in the chosen app.
5. Monitor audio through the FM-1’s headphone output or internal speaker.
6. Preview community firmware before flashing it to hardware.
7. Save patches, projects, and settings before experimenting with firmware.
8. Flash only firmware that has been verified as compatible with the physical FM-1’s hardware revision.

## 8. Buying recommendations

### 8.1 Most trusted sources

| Source | Trust level | Recommendation |
|---|---|---|
| M-VAVE official Amazon storefront | Best | Preferred if available in your country |
| AliExpress official M-VAVE store | Good | Usually lowest price and broad colour choice |
| Amazon third-party seller | Acceptable | Check seller feedback and fulfilment method |
| Temu or eBay | Lower | Use only with strong buyer protection |
| Traditional music retailers | Varies | Limited availability as of late 2026 |

For Singapore, check the official M-VAVE Amazon storefront and AliExpress official store first. Compare the landed price including GST, shipping, and return costs.

### 8.2 Forlo Store assessment

Forlo Store appears legitimate but is a high-volume general AliExpress reseller, not an official M-VAVE store.

Evidence:

- FM-1 listing shows 4.9/5 rating from 705 reviews.
- More than 900 sales attributed to the FM-1 listing.
- More than 10,000 total seller sales.
- Seller feedback is approximately 95.9 to 96.2 percent positive.
- Reviews specifically mention FM-1 sound, speaker, MIDI, DX7 patch loading, and custom firmware.
- The store sells a broad, unrelated catalogue, including camera accessories, headphones, e-readers, guitar pedals, and phone accessories.

Verdict: Forlo Store is legitimate-looking and acceptable for an FM-1 purchase, but it is a third-party reseller rather than the manufacturer. Buy from it if it is meaningfully cheaper than an official M-VAVE channel, but do not pay a premium for it over an official storefront.

### 8.3 Model differences

There is essentially one FM-1 hardware product, but three distinctions matter:

| Difference | Meaning |
|---|---|
| Colour | Cosmetic only; specifications remain the same |
| Older versus newer flash chip | Newer units use a different flash chip; older custom firmware may not support it correctly |
| Firmware V15 versus V16 | V16 adds 8-part multitimbrality and raises polyphony from 12 to 20 voices |
| FM-1 versus SMK-37 family | Different products; SMK-37 Pro/Elite and MKE-P37 are full-size 37-key MIDI keyboards with related FM engine tooling |

### 8.4 Which unit to buy

Buy the newest available FM-1, ideally from an official M-VAVE storefront. Update to official firmware V16 before installing third-party firmware.

Before checkout, verify:

- The listing explicitly says M-VAVE FM-1, not merely FM1, Fannya FM-1, or an SMK keyboard.
- It includes 27 keys, 6 operators, 32 algorithms, 128 presets, USB-C, Bluetooth MIDI, and 3.5 mm MIDI input.
- The seller states shipping origin, return window, and warranty process.
- If buying used, request a photo of the unit powered on and its firmware version.
- Avoid a used unit running unknown custom firmware unless the seller can restore official V16.

## 9. Flashing precautions

Do not assume every FM-1 is equally safe to flash with community firmware.

Precautions:

- Confirm the installed firmware version.
- Fully charge the FM-1.
- Use a Chromium-based browser for WebUSB/Web MIDI flashing.
- Avoid interrupting power or USB during the write.
- Back up the current state where possible.
- Return to official stock firmware V15 before moving from one custom firmware to another.
- Confirm that the custom firmware explicitly supports the unit’s hardware revision.
- Be aware that flashing erases saved projects and settings.
- Use M-VAVE’s force-flash tool only as a recovery option, not as a routine installation method.

## 10. Alternatives to FM-1

| Synth | Why it suits an R36S | Trade-offs | Approximate price |
|---|---|---|---|
| Korg Volca FM2 | Established 6-op FM module, USB MIDI, battery power, sync, DX7 patch compatibility | 6 voices, ribbon-style touch keyboard, AA batteries, no speaker | About EUR 150 / GBP 120-150 |
| Roland S-1 | Strong portable virtual-analog sequencer/synth, USB MIDI, rechargeable battery | No keys, 4 voices, higher price | About USD 270 |
| Behringer Pro VS Mini | Cheap, compact, battery-powered vector/hybrid synth with presets and sequencer | 4 voices, small keys, simpler workflow | About USD 119 |
| Sonicware Liven XFM | Portable FM-oriented groovebox with battery power, mini keys | 4-op XFM rather than classic 6-op FM; higher price | About GBP 230-260 |
| M-VAVE SMK-37 Pro | 37 velocity-sensitive keys, battery power, USB MIDI, DX7-style engine | Less convenient patch loading; not the FM-1’s open firmware platform | About USD 70-100 |
| Artiphon Orba | Tiny MIDI controller and looper with built-in sound; Bluetooth MIDI works with Android | Not a serious polyphonic synth engine | Varies by model and region |

## 11. Final recommendation

For a single purchase, the M-VAVE FM-1 remains the best value: it combines a real FM engine, USB MIDI, rechargeable battery, speaker, DX7 patch support, and custom-firmware potential.

For a second synth:

- Choose Volca FM2 for a dependable classic FM companion.
- Choose Roland S-1 for a stronger groovebox and sequencer.
- Choose Behringer Pro VS Mini for contrasting analog/vector sounds cheaply.
- Choose M-VAVE SMK-37 Pro if playable keys matter more than hackability.

For the R36S, use it as a portable FM-1 firmware preview device and MIDI sequencer. Do not rely on it as the final compatibility test. Run the emulator on Android, audition community builds, save preferred patches or projects, then flash only firmware verified on the real FM-1.

## 12. Key links

- M-VAVE FM-1 product page: https://www.m-vave.com/product?id=fm-1
- M-VAVE downloads: https://www.m-vave.com/download
- FM1 Open forum announcement: https://forum.m-vave.com/topic/4
- Official FM1 Workbench: https://open.m-vave.com/fm1-workbench
- FM-1 Firmware Hub: https://github.com/fabiomorena/fm1-custom-fw-hub
- FM-1 emulator project: https://github.com/topics/m-vave
- FM1 Studio and Android emulator resources: https://github.com/cicloid/awesome-fm-1
- Forlo Store FM-1 listing: https://www.aliexpress.com/item/1005012650197161.html

## 13. Source notes

Findings are based on M-VAVE product and download pages, FM1 Open documentation, FM-1 community firmware repositories, Android MIDI documentation, R36S hardware references, AliExpress listing metadata, and independent FM-1 reviews.

The most important caveat is that the FM-1’s open ecosystem evolves quickly. Before flashing, always verify the latest compatibility notes for the exact firmware and the exact hardware revision of the physical FM-1.
