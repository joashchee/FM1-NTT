# Firmware: what ships, and bring your own

## Two tiers

The maintainer asked first for every popular firmware that is permissively licensed (only Haunted FM-1 qualified), then, once FM1-NTT was to be MIT, for
any licence that MIT allows. **MIT allows distributing GPL firmware alongside, as separate works, but it does not make that free of obligations**, so
copyleft firmware ships as a **separate pack** rather than inside the MIT app:

| | `bundled-firmware/` (this repository) | the FM1-NTT Firmware Pack (its own repository) |
|---|---|---|
| Licences (AND-joined, from an allowlist) | MIT, BSD-2/3-Clause, Apache-2.0, ISC, Unlicense, CC0-1.0, Zlib; MPL-2.0 and OFL-1.1 as components | those, plus GPL-2.0-only/or-later, GPL-3.0-only/or-later, LGPL-2.1 and 3.0 |
| Ships in | the MIT app's package, this repository's releases | **its own repository and package** (`FM1-NTT-firmware-pack.deb`), never in this repository or the app's |
| Mac App Store build | **none is planned** (the maintainer, 2026-11-18) | **never**, should one ever exist (the App Store's terms are widely reported to be incompatible with the GPL); `scripts/check-bundled.py --no-pack APP_DIR` fails if any pack file is in a build |
| Extra obligations | licence texts, `SOURCES.md`, `NOTICE`; for MPL parts, where the source is | **the complete corresponding source beside each binary** (hash-checked), a written offer, the GPL text, no added terms |
| In the FILES panel | marked **B** | marked **G** |
| Today | Haunted FM-1 v0.1 (5 firmwares) | Felucca 1.8.2, FoMni-1 0.4.1, Jangada 1.0.1 |

**Never, in either tier:** AGPL, non-commercial, "no licence", closed or commercial firmware (FM-1+VA, Groove OS, M-VAVE's stock), `GPL ... WITH` exceptions until each is reviewed,
and any firmware that **embeds third-party data without a grant** (Melodee: Casio and Sequential sound data). Everything not in a manifest is **bring-your-own**:
nothing else is committed, cached by CI, packaged or released, and r3i6nos's rule 2 (no proprietary content in a release) holds. `.gitignore` blocks `*.fwsc` and
`*.fm1save` except under `bundled-firmware/` (and the Pack repository has its own rules).

## What may be bundled

Every file, in either tier, must

- be listed in its tier's `MANIFEST.tsv` with its **SHA-256, size, source URL and the commit it was built from**, and be **byte for byte what the author published**: it is never edited, rebuilt or repacked here;
- carry a **licence expression of AND-joined identifiers from the tier's allowlist** (never `OR`, never `WITH`), and a firmware that links a part under any other licence is out, **whatever its repository's badge says**; read its own `LICENSING.md` and notices;
- sit in a folder with the **licence text of every identifier**, a `SOURCES.md` (where it came from, what is inside, and the caveats found) and a `NOTICE`;
- in the pack, **also** carry `source_archive` and `source_sha256`: the complete corresponding source, in the pack, hash-checked.

`scripts/check-bundled.py` (and its 21 tests) enforces all of this: here for the permissive tier (in `check.sh` and the package build, which also proves the package holds no pack file), and `--pack` in the Pack repository, and fails on any other firmware file in the repository.
**MPL-2.0 is not a permissive licence** (file-level copyleft): it is accepted as a component of a package whose own code is permissive, with pinned commits and URLs and the
texts. If you want a stricter reading, remove Haunted FM-1; nothing else depends on it except Rec demo's and the benchmark's default firmware.

**Why a separate repository.** The GPL's duties (source, notices, no extra terms) attach to the firmware files, and they must not reach FM1-NTT's own code: the emulator reads
a firmware as data and links none of it, so the app stays MIT, and keeping the GPL firmware in its own repository, package and manifest makes that boundary visible (this repository cannot even contain it: the checker refuses) and the
Mac App Store exclusion mechanical. GPL-3.0 section 6(d) is met by distributing the source from the same place as the binary; a written offer (section 6(b)) is
in each `SOURCES.md` as a fallback, with **the Pack repository's issues page as the contact** (the maintainer's decision, 2026-11-16: `https://github.com/joashchee/FM1-NTT-Firmware-Pack/issues`, with a "Request the corresponding source" template).

## Where the firmware is

Permissive tier: in the repository `bundled-firmware/<family>/`; in the `.deb`, `/usr/share/fm1ntt/firmware/<family>/` with the licence texts in
`/usr/share/doc/fm1ntt/bundled/<family>/`; marked **B**; `$FM1NTT_BUNDLED` names another folder. Copyleft pack (its own repository, `https://github.com/joashchee/FM1-NTT-Firmware-Pack`): `<family>/`; in its `.deb`,
`/usr/share/fm1ntt/firmware-gpl/<family>/` (source in `firmware-gpl-source/`, texts in `/usr/share/doc/fm1ntt-firmware-pack/`); marked **G**; `$FM1NTT_PACK` names another
folder. The FILES panel lists yours first, then B, then G.
**Rec demo and the benchmark use a bundled firmware (UNTRUE STEP) when you give none**, so the demo file the r3i6nos rule asks for exists and
benchmark reports from different platforms run the same real firmware (named by its hash).

## Where users get them

Any firmware that is not bundled comes from its author: the community hubs in the reference kit (section 12), the FM-1 Firmware Hub, `awesome-fm-1`, each
firmware's own page (Felucca, SLOOP, Melodee, S1Vave, ...; `docs/FIRMWARE-CATALOG.md` says which are popular and under which licence). The user
downloads a package and gives its path: `fm1ntt path/to/image`, or copies it to `/roms/FM1-NTT/firmware/` on the games card and picks it in FILES.

## What the core accepts

A `.fwsc` update package, the form community firmwares are shared in (read by the vendored `fm1_soc::stock_from_package`, which says why when it
refuses one), or a raw execute-in-place image with its entry address (`--entry 0x02000120` for Felucca's `felucca_xip.bin`). The core bounds
the size (not empty, at most 32 MiB) first. **Images are untrusted input:** a bad one is an `Error`, never a crash, a hang or an unbounded
allocation; fuzzing the loader is on the roadmap (section 5) and the vendored reader's own robustness is upstream's until then.

## Our own test images

CI has the bundled firmware, and boots every one of them (each must draw its screen and not halt); it also tests the refusals (garbage, empty, ELF, oversize), the determinism of a garbage image that halts, and the
interfaces. Running a real firmware is a **local** check the maintainer does with their own download (`local_firmware*/`, gitignored);
what it showed goes into the roadmap, never the image into the repository.

## Rec demo and the benchmark with a firmware

**Rec demo's `firmware` hero feature runs a real firmware**: the one you give (`--rec-demo FILE`), else the one loaded when you start it in TOOLS, else a
bundled one (UNTRUE STEP). It is copied into the demo's temporary folder and removed with it, never into `Demo/`. The benchmark
times a firmware with `--fw FILE` and names the lines by the file's FNV-64 hash, so two platforms' reports are about the same file only when
the hashes match. Neither writes the firmware anywhere permanent (the demo's copy is removed with its temporary folder).

## Saves

A saved flash or state (`.fm1save`-style) is the user's: written only under the app's own folder, to a temporary name and then renamed
(r3i6nos rule 7, TinyNTT's rule 5).

## Keeping the curated library current

`scripts/check-firmware-updates.py` (12 tests) reads both manifests and `firmware-watchlist.tsv`: for each bundled firmware it asks the project's public release feed
(`github.com/OWNER/REPO/releases.atom`: no login, no API quota, retried through flaky networks) for the latest release and compares it with the tag in the manifest (**NEWER**),
fetches the licence file at the newest tag and compares it with the copy kept beside the firmware (**LICENCE-CHANGED**: read it before bundling the update), and for each watch entry
compares a GitHub project's latest release or a closed firmware's home page (reduced to its licence-bearing lines) with what was last read (**WATCH-CHANGED**). It never downloads a
firmware and never edits a manifest; `--write-seen` records the watch baseline after a review. Exit 10 means something needs reading, 1 means something could not be checked. The Pack
repository runs it weekly from a workflow and opens an issue. First run (2026-11-16): Haunted FM-1 v0.1, Felucca 1.8.2, FoMni 0.4.1 and Jangada 1.0.1 all current with unchanged licence files;
SLOOP, Melodee, X0X and Quod unchanged; Shrub's page baselined.
