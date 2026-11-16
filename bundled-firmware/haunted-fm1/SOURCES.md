# Haunted FM-1 v0.1: where these files come from, and what is in them

Five generative firmware packages for the M-VAVE FM-1 by **David Ryan**, bundled with FM1-NTT on 2026-11-16 so there is something to run
without hunting for a download. They are **his work, unmodified**: each `.fwsc` is byte for byte the file on the project's release page, and the
SHA-256 of every one is in `../MANIFEST.tsv`, which `scripts/check-bundled.py` checks.

| | |
|---|---|
| Project | https://github.com/davidpryan/haunted-fm1 |
| Release | `v0.1`, released 21 Oct 2026: https://github.com/davidpryan/haunted-fm1/releases/tag/v0.1 |
| Built from | commit `b7485a1f91bbc59c15ee3dd94af59509877a8637` |
| Files | `halflight-0.1.fwsc` (HALF-LIGHT), `midnight-0.1.fwsc` (MIDNIGHT ROUTE), `untrue-0.1.fwsc` (UNTRUE STEP), `voicemail-0.1.fwsc` (VOICEMAIL), `worntape-0.1.fwsc` (WORN TAPE); manuals in `docs/` |
| Fetched | 2026-11-16 from the release's download URLs, into an empty folder; the five files were then booted in FM1-NTT's emulator (each draws its own screen; WORN TAPE makes sound) |

## What is inside a package, and under which licence

A `.fwsc` is a build of the author's code **on top of two MPL-2.0 platform layers and a few Apache-2.0 vendor files**. The upstream project says so
in its README ("The firmwares and the shared libraries are MIT. fm1-base and felucca-bsp are MPL-2.0; the JieLi SDK files the package needs are
Apache-2.0") and `docs/FM1-BASE-LICENSING.md` (fm1-base's own account, at the pinned commit) says where each part came from.

| Part | Licence | Text here | Source for it |
|---|---|---|---|
| The firmwares (`firmware/*`) and the shared DSP and UI libraries (`lib/hdsp.h`, `lib/hui.h`), Copyright (c) 2026 David Ryan | **MIT** | `LICENSE` | https://github.com/davidpryan/haunted-fm1 at `b7485a1f91bbc59c15ee3dd94af59509877a8637` |
| fm1-base (the platform, drivers, UI toolkit), Copyright Charles Vestal, with code of Leo Kuroshita relicensed by him | **MPL-2.0**, file by file | `LICENSE-MPL-2.0.txt` | https://github.com/charlesvestal/fm1-base at the pinned commit `e35f7e2c6904ff1a3eed10035474c8cf089c0b88` |
| felucca-bsp (the chip layer: clock, interrupts, flash, USB), Copyright (C) 2026 Leo Kuroshita, Hügelton Instruments | **MPL-2.0**, file by file; two files are Apache-2.0 from JieLi | `LICENSE-MPL-2.0.txt` | https://github.com/hugelton/felucca-bsp , the submodule inside fm1-base at that commit |
| JieLi AC79 SDK files the package needs (`uboot.boot`, `cfg_tool.bin`, `eq_cfg_hw.bin`) and the radio start-up tables, Copyright (c) Zhuhai Jieli Technology Co., Ltd. | **Apache-2.0** | `LICENSE-Apache-2.0.txt` | JieLi's AC79 SDK (Apache-2.0); not redistributed apart from inside the packages |
| Barlow Semi Condensed, Copyright 2017 The Barlow Project Authors | **SIL OFL 1.1** (rendered into the screen's bitmaps) | `LICENSE-OFL-Barlow.txt` | https://github.com/jpt/barlow |
| Terminus, Copyright (C) 2020 Dimitar Toshkov Zhekov | **SIL OFL 1.1** (rendered into the screen's bitmaps) | `LICENSE-OFL-Terminus.txt` | https://terminus-font.sourceforge.net/ |
| Jon Dattorro's plate reverb | an algorithm ("Effect Design, Part 1", JAES 1997), reimplemented by the author | none | none |

No `ble`, Wi-Fi or own-Bluetooth-stack code is built in (`ble = false` in each firmware's `fm1.toml`), so none of fm1-base's BSD-2-Clause or
Android-derived Bluetooth parts are in these packages (checked in `untrue-step`, `midnight-route`, `voicemail`, `half-light` and `worn-tape`).
**No GPL code is in them**: that is the reason they, and not the better-known community firmwares, are bundled
(`docs/FIRMWARE-CATALOG.md`).

## What the MPL-2.0 parts ask of whoever redistributes the packages

MPL-2.0 is **weak, file-level copyleft**, not a permissive licence: the packages' MIT parts stay MIT, but the files that came from fm1-base and
felucca-bsp stay MPL-2.0. Section 3.2 lets the packages be passed on in their binary form on condition that the **source of those MPL files is available
and the recipient is told how to get it**, and that the licence text and notices travel with them. This directory does both: the pinned commits and
URLs above are the source, and the licence texts and notices are in this folder and in the `.deb`'s `/usr/share/doc/fm1ntt/bundled/`. Nobody here has modified
any of these files. If the upstream repositories ever disappear, the source would have to be mirrored here: a roadmap item.

## Trademarks

M-VAVE and FM-1 are trademarks of their owners; neither Haunted FM-1 nor FM1-NTT is affiliated with M-VAVE.
