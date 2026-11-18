# M-VAVE FM-1 Custom Firmware Development Guide

The M-VAVE FM-1 has sparked a massive open-source firmware ecosystem—with community repositories like *The Firmware Hub* hosting over 250 distinct custom firmwares. This rapid proliferation comes down to a unique intersection of **unlocked hardware, official vendor support, WebAssembly browser emulation, and AI-assisted development**.

---

## 1. Available Development Tools & Toolchains

Under the hood, the M-VAVE FM-1 runs on a **JieLi AC79 / WL82 chipset** (using the `pi32v2` / `r3` CPU architecture). Depending on how low-level you want to go, there are three primary starting points:

### Official M-VAVE Open SDK & Workbench
* **Repository:** `m-vave/fm1_open`
* **Overview:** Following early community reverse-engineering, M-VAVE released an open-source application layer (GPLv3) paired with a pre-compiled binary DSP engine SDK.
* **FM-1 Workbench (`open.m-vave.com/fm1-workbench`):** A browser-based simulator and previewer featuring SysEx/sample management and a built-in USB WebMIDI/SysEx flasher.

### Community Base Firmwares & Bare-Metal Templates
Rather than writing hardware drivers for the TFT screen, 27-key matrix, rotary encoders, and audio interrupts from scratch, most developers fork existing open-source firmwares:
* **Felucca (`hugelton/Felucca`):** Created by Leo Kuroshita / Hügelton Instruments, Felucca is explicitly structured as a modular base ("the custom firmware the others build on"), cleanly separating the hardware board support package (`felucca-bsp`) from the DSP sound engines (`Felucca-Engines`).
* **Sloop:** Another popular community groovebox/synth codebase frequently forked for custom rhythm and synthesis engines.
* **Bare-Metal & Reverse-Engineering Repos (`Keitark/fm1-nes`, `FM-1-RE`):** Minimal USB-only bring-up templates, guarded flash wrappers, and hardware reverse-engineering documentation for developers writing completely custom engines or non-synth applications (such as NES or Doom ports).

### Compilers & Emulators
* **JieLi PI32v2 Toolchain (`clang 4.0.1 r3`):** Required to compile C/C++ code into `.fwsc` firmware binaries for the physical hardware. Typically run on Linux or Windows via WSL.
* **WASI SDK (WebAssembly):** Both `fm1_open` and community bases like *Felucca* compile the exact same C codebase into WebAssembly (`WASM`). This allows developers to test UI layouts, button mappings, and DSP audio in a local browser emulator (`?mock=1`) without needing to flash the physical device on every build.

### Flashing & Recovery Utilities
* **WebMIDI / WebSerial Installers:** Browser-based flashers (Chrome/Edge) integrated into *The Firmware Hub* and individual GitHub Pages allow one-click flashing over USB.
* **Hardware Unbrickers (`FM-1 Transporter` / `MvaveFM1Unbricker`):** Recovery tools used to force the JieLi chip into download/UBOOT mode if a bad flash prevents normal USB enumeration.

---

## 2. Why Are Firmwares Being Created So Quickly?

The sheer volume of firmwares—ranging from multi-engine grooveboxes and Casio CZ/DX7 emulators to generative ambient machines and bio-synthetic sequencers—is driven by five key factors:

1. **AI-Assisted "Vibe Coding" on Modular Bases**
   * Because projects like *Felucca* and `fm1_open` handle the low-level hardware abstraction (display rendering, key scanning, audio buffers, and MIDI routing), creating a new synth comes down to writing pure C DSP code and UI state machines.
   * Developers heavily use AI coding agents (Claude, GitHub Copilot, Gemini) inside VS Code/WSL to rapidly generate synthesis algorithms, sequencer logic, and menu screens. Several community repos even include system prompts tailored for AI agents to build FM-1 apps.
2. **Instant Browser Compilation & Testing (WASM)**
   * Embedded hardware development is usually bottlenecked by slow compile-flash-reboot cycles. Compiling the C codebase to WebAssembly via WASI SDK lets developers iterate on DSP and UI changes in seconds inside a desktop browser, and lets users test-drive firmwares online before flashing.
3. **Zero-Friction Distribution (*The Firmware Hub*)**
   * *The Firmware Hub* (`thefirmwarehub.com`) acts as a centralized app store and WebMIDI flasher. Creators simply push a `.fwsc` release to GitHub, and anyone with a USB cable and Chrome/Edge can flash it in roughly 60 seconds and swap back just as easily.
4. **Inexpensive, Unencrypted Hardware**
   * At roughly $50–$70 USD, the FM-1 is affordable enough that musicians and developers aren't afraid to experiment. M-VAVE left the bootloader open without cryptographic signature locks and embraced the modding community with an official SDK.
5. **High Derivative Remixing**
   * Once solid GPL-3.0 bases like *Felucca* and *Sloop* were published, dozens of developers forked them to create dedicated acid boxes, drum machines, drone synths, and genre-specific grooveboxes.

---

## 3. Quick-Start Checklist for Developers

1. **Choose a Starting Template:**
   * Clone `m-vave/fm1_open` if you want to build custom UI/sequencers on top of M-VAVE's pre-built DSP engine.
   * Clone `hugelton/Felucca` if you want a 100% open-source C audio and UI stack.
2. **Set Up the Build Environment:**
   * Install the **JieLi PI32v2 (`clang 4.0.1 r3`)** toolchain for `.fwsc` hardware builds.
   * Install **WASI SDK** for instant browser simulation.
3. **Back Up Your Device Before Flashing:**
   * Ensure your FM-1 is updated to stock **V15** (and check flash chip compatibility, as newer hardware revisions use the `UC25HQ80` flash chip).
   * Export and back up your factory patches before flashing custom builds.