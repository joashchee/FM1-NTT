# Which FM-1 firmwares exist, which are popular, and which can be bundled

Surveyed **2026-11-16** from the community lists and press, then each candidate's own repository and licence files. Re-check a licence before relying on
a row: the ecosystem is a month or two old and moves daily.

## Sources

- **awesome-fm-1** (https://github.com/cicloid/awesome-fm-1): 222 custom firmware entries (186 open source, 4 closed, 32 "not a synth"), with licences.
- **FM-1 Firmware Hub** (https://github.com/fabiomorena/fm1-custom-fw-hub): a catalogue grouped by family (Felucca, SLOOP, FoMni, independent engines) with
  licences and, for the stock firmware, M-VAVE's own download. It hosts no community `.fwsc`.
- Reviews and guides: "M-VAVE FM-1 Firmware: 7 Custom Firmwares Ranked (2026)" (dreyandersson.com), Piano & Synth Magazine's custom firmware collection,
  Sonicstate (2026-09-29), Synthanatomy (Felucca 1.0), Matrixsynth (Felucca, GHOULBOX, FM-1 Dock), Synthtopia, and the Elektronauts "M-vave FM-1" thread.
- **Reddit could not be searched** (the search tool is blocked from reddit.com), so nothing here comes from a Reddit thread. The Elektronauts thread and the
  press are the community voice that was reachable.

## What people call stable or popular

No source ranks by stability on evidence; stars and reviews are what there is. The ranked guide's order, with the repositories' star counts as of the survey:

| Firmware | Author | Version | Licence | Stars | Said of it |
|---|---|---|---|---|---|
| **SLOOP** | isod89 | 2.8.5 | GPL-3.0 | 810 | free live-jamming groovebox; recognises the newer flash chip; the most-starred |
| **Felucca** | Hügelton Instruments (Leo Kuroshita) | 1.8.1 (1.8.2 beta) | GPL-3.0-only | 650 | the base most others fork; sound design and USB audio; its first-version boot bug fixed |
| **Melodee** | keremimo | 1.6.1 | GPL-3.0 | 212 | a Felucca fork with five-plus engines and a 4-track 64-step sequencer; recommended for stability; saves a backup before it installs |
| **FM-1+VA** | Baud Girl | 102 | free, **closed source** | n/a | patches M-VAVE's stock V15 instead of replacing it: "the lowest-risk free option" |
| X0X | Charles Vestal | 1.2.0 | GPL-3.0 | | acid, 909, 808 and breaks |
| Jangada | zednaked | 1.0.1 | GPL-3.0 | | drones and dark superwaves |
| Groove OS | Peter Gombos | 2.0 (1.4.1) | **commercial**, $29 | n/a | sampling groovebox |
| FoMni | Charles Vestal | 0.4 | GPL-3.0 | | Omnichord-style strumming |
| PIGEON | The Shrimp Blimp | 0.5.0-beta.1 | GPL-3.0-only | | "the final beta"; hardware checks done by its maintainer |

The advice every guide repeats: back up first, keep stock V15 (or V16) as the way back, and update to the official firmware before switching between custom ones.

## What is bundled, and what is not

Two tiers (`docs/FIRMWARE.md`, "Two tiers"). **Permissive tier, inside the MIT app: Haunted FM-1 v0.1** (five firmwares; MIT on MPL-2.0 platform layers).
**Copyleft pack, its own repository (https://github.com/joashchee/FM1-NTT-Firmware-Pack) and package, with source: Felucca 1.8.2, FoMni-1 0.4.1 and Jangada 1.0.1** (GPL-3.0-only for the whole, with the MIT, MPL-2.0, Apache-2.0, CC0-1.0 and OFL-1.1 parts
each one's `SOURCES.md` lists). Each was chosen **after reading its own licence files at the release tag**, and each was booted in the emulator.

Left out, each for a specific reason found in its own files:

| Firmware | Why it is not bundled |
|---|---|
| **SLOOP 2.8.5** (the most-starred) | GPL-3.0, but its source tree carries `assets/icons.png`, the Hügelton icon atlas, and its own `LICENSING.md` (a copy from Felucca 1.0 days) says the atlas is "all rights reserved, licence terms will be published later"; its `icons.c` is built from it. Felucca has since dropped the atlas for the MIT Fukiai font, but SLOOP 2.8.5 has not, so the terms the §7 permission depends on do not exist. **Ask isod89 to rebuild without it**, then it can go in |
| **Melodee 1.6.1** | GPL-3.0, but its own licensing says it embeds the **Casio CZ-1's 64 factory sounds** (Casio, 1986) and the **Sequential Prophet-5/10 factory programs v1.03** (Sequential, 2025) "as sound data ... the rights stay with" them. Third-party data with no grant: out, however good the firmware |
| **X0X 1.2.2** | GPL-3.0, but its licensing lists **TR-909 hi-hat, ride and crash samples** taken from a third-party project (ER-99 via 9W9), whose provenance in Roland's hardware is not addressed, and a break generator ported from a repository with **no licence file**, "used with the author's permission". A decision for the maintainer (roadmap 4d) |
| PIGEON, ORBIT, JIANT FM, Skiff, Hortator, ChoralRoot, GHOULBOX, Caravel and ~150 more GPL firmwares | not yet checked one by one: each needs the same read of its own licence files, a published binary and its source. They are the next candidates (roadmap 4d), starting with the most-starred |
| **GPL-3.0 with an "FM1 SDK linking exception"** (FM1 Keyboard Academy, FM-PLAITS, Shed, DX747...) | the `WITH` form is refused by the checker until each exception is reviewed |
| **AGPL-3.0** (ACID FM1) | AGPL is not in either allowlist: its network-use clause adds risk for no gain here |
| **Closed or commercial** | FM-1+VA, Groove OS, M-VAVE's stock V15/V16 (a proprietary engine), **Shrub** (below) |
| **Shrub 1.00 beta** (Steve Duda, Xfer Records: a stripped-down Serum for the FM-1: two wavetable oscillators, sub, noise, Serum-derived filters, 4 notes of paraphony, a Serum 1 preset importer; free, with a browser editor and a VST3 coming) | Checked 2026-11-16 at its home page, https://shrub.audio: **"SHRUB is copyright 2026 Xfer Inc. All rights reserved."** It is free to download and install over USB, but **no licence, no source and no redistribution grant is stated**, so the default is no redistribution: **it cannot be bundled or placed in either tier.** It is not a Felucca fork (the author says so). It runs fine as bring-your-own: download it from shrub.audio and give its path. **Ask steve@xferrecords.com for written permission to redistribute** (a per-file grant is enough for the permissive tier's rules only if it is a licence we can state: otherwise it stays link-only); watched by `scripts/check-firmware-updates.py` for a licence appearing |
| **No licence stated** | SLOOP ALG, fm1-polyseq, DriftSeq, TheReFmB1rth, REDACTED, More Chords |
| **Non-GPL copyleft that is weaker still: MPL-2.0 firmwares** (FIELD, LOOM-8, Erik37, MODELAR, J6, DelRio, Saadet, VNL-1, FIZZ, Tonic, LUOGU, FRASIL, AEther, MIDIMON, TRIO, CHOPPAH, NOCT FM) | now allowed in the pack's tier (MPL-2.0 is on its allowlist) but each needs its own check and a published binary: candidates |
| **Quod** (MIT repo) | its `fm1/` firmware is GPL-3.0-only (its README says so): it would belong in the pack, with its source, if it is wanted |
| **Permissive, but no published binary** | Lunar Modulator (MIT), Album (MIT), fm1-nes (Apache-2.0, needs your NES ROM) |
| **Repository gone** | FM-1 Neverhood (MIT, listed; was 404) |
| **Not a firmware** | fm1-firmware-patcher (Unlicense, patches the proprietary stock V15), FM-1 Macropad Patch (a binary patch of the stock firmware), installers and recovery tools |

A firmware's repository badge is not its licence: **Quod** (an MIT repo with a GPL firmware), **Melodee** (GPL code with proprietary sound data inside), **SLOOP** (a licence file that
predates its own assets) and **Haunted FM-1** (listed MIT in one catalogue and GPL-3.0 in another) are all examples in this survey.

## How a firmware gets bundled

`docs/FIRMWARE.md`, "What may be bundled": every file is in its tier's `MANIFEST.tsv` with its SHA-256, its licence expression (AND-joined, from the tier's allowlist), its source
and the commit it was built from (the pack's entries also with their complete corresponding source and its hash); the licence texts, a `SOURCES.md` and a `NOTICE` sit beside it;
`scripts/check-bundled.py` (in `check.sh` and in both package builds) fails otherwise. To add one: read its `LICENSING.md` and notices at the release tag, scan its source's SPDX
identifiers, download the release into an empty folder, boot it, then write the folder.

## Candidates to look at next

- More of the GPL firmwares, the most-starred first, each with the same read of its own licence files.
- X0X, if the maintainer accepts its provenance caveats; SLOOP, once it drops the reserved icon atlas; Melodee, if it drops the Casio and Sequential data.
- The MPL-2.0 firmwares above.
- **Lunar Modulator** and **Album**, if someone builds a release from source with JieLi's toolchain (the build is reproducible only with their SDK) and publishes it; or if FM1-NTT builds them (a toolchain in the Docker image).
- New releases of Haunted FM-1 (this survey took v0.1).
- A hand-written original firmware for FM1-NTT itself (the ISA tables are in the vendored crates): MIT, no third parties; a project of its own.
