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
**Copyleft pack, its own repository (https://github.com/joashchee/FM1-NTT-Firmware-Pack) and package, with source: Felucca 1.8.2, FoMni-1 0.4.1, Jangada 1.0.1 and Skiff 2.6** (GPL-3.0-only for the whole, with the MIT, MPL-2.0, Apache-2.0, CC0-1.0 and OFL-1.1 parts
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

## Licence reads, 2026-11-16 (continued)

Each read from the firmware's own licence files at its release tag (not the repository badge), its `.fwsc` hashed against the author's `SHA256SUMS`. **Nothing here is bundled yet**: this is what the next pack entries need.

| Firmware | Result | Open points |
|---|---|---|
| **FIZZ 0.13.0** (Quixotic7) | **MPL-2.0** firmware (was GPL-3.0-only until 2026-10-23; the Felucca-derived code was rewritten clean-room, `research/PROVENANCE.md`). Both `.fwsc` match the author's sums (`fizz-0.13.0` `f695bf22...`, `fizz-usb-0.13.0` `11148699...`). Its GPL parts are the web `site/` and test oracles, not the firmware | Third parties in the firmware: plutovg (MIT), **FreeType's rasterizer under the FreeType License (FTL)** (not on either allowlist: a decision), stb (MIT OR Unlicense: the checker is AND-only), Inter Tight (OFL-1.1). **Decided 2026-11-16 (maintainer: "leave freetype out of allowlist"): the FreeType License stays off both allowlists, so FIZZ is not bundled** in either tier (its firmware carries FTL-licensed FreeType code inside plutovg); it stays link-only, and the checker still refuses an FTL entry |
| **Tessera 3.5.3** (Rastapopolix, a Melodee derivative) | **GPL-3.0-only** as a whole; both `.fwsc` match the author's sums. **It does not carry what excluded Melodee**: its ORACLE sounds are "Tessera's own (no Sequential factory programs)" (`oracle_sounds.h`) and the CZ-1 and Prophet stores are "retired with their engines" (`native_presets.c`) | MIT, Apache-2.0, GPL-3.0-or-later (Grids map, Dexed tables), MPL-2.0 (`announce.c`), OFL; Scala tunings from the Huygens-Fokker archive (data, no licence statement found: ask); the 3 JieLi SDK files in each package (Apache-2.0, as in the pack's others). A source scan of SPDX identifiers is still to do |
| **Skiff 2.6** (illuminus5, a Felucca 1.1.5 fork) | **GPL-3.0-only** as a whole, no `THIRD_PARTY_NOTICES.md` (the earlier retry failure was a network drop on a file that does not exist) | **Two drum loops (`assets/breaks/`) generated with the YuE2 music model**, whose weights are CC BY-NC 4.0 with an extra permission for individual creators: a rights question, not a licence the pack can state. CC0 samples (VSCO-2 CE, VCSL) are fine. **Decided 2026-11-16 (maintainer: "skiff to the pack"): in the pack** (FM-1 package only), with the loops recorded as a caveat in `skiff/SOURCES.md`: the pack relies on the author's stated permission and GPL grant, unconfirmed with the model's authors; it comes out if either is withdrawn |

The three `.fwsc` hash reads that first looked like MISMATCHes were **truncated downloads**, not re-uploads: the same file hashed differently on each read. `fm1tastik/scripts/research.py` now reads to the end and refuses a body shorter than its `Content-Length` (retrying), with tests.


## MPL-2.0 firmwares, 2026-11-18

The maintainer asked for all MPL-2.0 firmwares to be included. Each was read from its own notice files at the release tag (not the repository badge); `fm1tastik/scripts/research.py` gave the release, the `.fwsc` and its hash. **Built into the Pack on 2026-11-18: FRASIL, MIDIMON, MODELAR, J6 and TRIO** (the "Pass" rows; not yet committed there). Their tier is the Pack (`MPL-2.0` is on its allowlist), with the platform's submodule sources (fm1-base, felucca-bsp) alongside.

| Firmware | Release | Result |
|---|---|---|
| **FRASIL 3.3.1** | `.fwsc` published | **Pass.** MPL-2.0 file by file, fonts OFL-1.1, JieLi SDK files not in the repository; the name and logo are the author's, redistributed unchanged |
| **MIDIMON 0.5** | `.fwsc` published | **Pass, with a caveat.** MPL-2.0; its USB MIDI host is MPL-2.0 only by two authors' own quoted words (recorded in its `LICENSING.md`); Terminus is OFL-1.1 |
| **MODELAR 0.3** | `.fwsc` published | **Pass on a first read** (README only: MPL-2.0 on fm1-base and felucca-bsp; no notice file at the tag, so read its source's SPDX lines before bundling) |
| **J6 1.4.0** | `.fwsc` published | **Pass on a first read** (the same: a synthesiser, no samples, README only) |
| **TRIO 0.1.1** | `.fwsc` published | **Pass, with a caveat.** MPL-2.0; its network weights were trained on FiloBass (CC BY 4.0) and JazzSAMBA (an MIT-style licence), neither included; keep the attribution; JieLi's Bluetooth libraries (Apache-2.0) are in the build |
| **Erik37 1.3** | `.fwsc` published | **Out:** the piano bank is the Salamander Grand Piano V3, **CC BY 3.0**, a derivative inside every package; CC BY is not on the allowlist |
| **DelRio 1.0** | `.fwsc` published | **Out:** the kit is freesound recordings under **CC BY 3.0 and CC BY 4.0** (with CC0 VCSL parts) |
| **Saadet 1.0.5** | `.fwsc` published | **Out:** its licensing says factory sound data (**Roland, Sequential, Casio**) is the vendors', no licence granted (a Melodee fork) |
| **LUOGU 1.0.2** | `.fwsc` published | **Out:** five arias' scores under **CC BY-NC-ND 4.0** (non-commercial), and two words cut from a comedy recording that "belong to their owners" |
| **CHOPPAH 1.4.1** | `.fwsc` published | **Out:** eight breaks "from the records" (The Winstons' "Amen, Brother" and others): commercial recordings |
| **FIZZ 0.13.0** | `.fwsc` published | **Out:** FreeType's FTL (the earlier decision, 2026-11-16: "leave freetype out of allowlist") |
| **LOOM-8 0.16.2** | `.fwsc` published | **Not MPL as a whole:** its own `LICENSING.md` says the firmware is **GPL-3.0-only** as a whole (the acid engine and the 909 models), with MPL-2.0 files inside; a Pack entry on a full GPL read, with 9W9's permission as a caveat |
| **VNL-1 0.9.2** | `.fwsc` published | **Not MPL:** **GPL-3.0-only** as a whole since the `mpl-final` tag (FOMAJAM and Optimist drum engines); a Pack entry on a full GPL read |
| **FIELD, Tonic, AEther, NOCT FM** | none in the release feed | **Cannot be bundled** until the author publishes a `.fwsc` (the app bundles the author's release byte for byte) |
