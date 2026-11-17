# `fm1tastik.json`: a proposal for describing FM-1 firmware in its own repository

**Status:** draft 0.1, 2026-11-16. Written for [fm1tastik](FM1TASTIK.md) (fm1tastik.chordica.com, by Scoreless Music), and offered to anyone else who builds a
catalog, a launcher or a flasher for the M-VAVE FM-1. **Nothing implements it yet**; the validator and the schema are Phase 4b of the fm1tastik plan.
This document is CC0-1.0: copy it, change it, propose a better one.

## Why

Every FM-1 catalog (awesome-fm-1's list, the FM-1 Firmware Hub, a launcher, a store page) re-types the same facts about a firmware by hand: its name, its
author, its licence, where the file is, what it needs. They go stale, disagree, and get the licence wrong. The people who know the facts are the authors, and
they already keep a repository. **One small file in the repository, written once, read by every catalog.** It also gives a catalog a way to confirm that a
person who says "this is my firmware" controls its source, without an account or a login.

## What it is not

- **Not a licence and not a grant.** It *describes* a licence; the repository's own licence files are what binds. A catalog must show a mismatch
  between this file and the licence files, not trust either blindly.
- **Not a place for secrets.** The file is public. **Never put a webhook URL, a token, a password or a private address in it.**
- **Not required.** A firmware without one can still be listed by hand; the file only makes the entry richer and claimable.
- **Not a hosting arrangement.** It points to where the author publishes the firmware; a catalog does not need to, and fm1tastik does not, host the file.

## Where it lives

1. **GitHub (and any Git host):** `fm1tastik.json` at the **root of the default branch**. A catalog may also read it **at a release's tag**, which wins for that release.
2. **Not in a repository:** `https://<the project's site>/.well-known/fm1tastik.json`.
3. **A claim without a file** (a site you cannot add a file to): a DNS TXT record `_fm1tastik.<your domain>` with `claim=<code>` (see "Claiming").

A repository that ships several firmwares (as Haunted FM-1 does) lists them all in the one file.

## The file

UTF-8 JSON, **at most 64 KiB**, served over **https**. Unknown keys are **ignored** (so the format can grow); keys starting `x-` are reserved for private
extensions. Strings are plain text (no HTML, no Markdown rendering is promised). Every limit below is a limit a reader may enforce and a writer must meet.

```json
{
  "format": "fm1tastik/1",
  "claim": { "code": "k7Q2-M9XV-4HNE" },
  "listed": true,
  "contact": { "issues": "https://github.com/example/zither/issues", "email": "maker@example.org", "discord": "https://discord.gg/xxxxxxx" },
  "firmware": [
    {
      "id": "zither",
      "name": "ZITHER",
      "summary": "A plucked-string synth with a four-track sequencer.",
      "description": "Karplus-Strong strings, eight voices, 64-step patterns. Needs stock V16 or newer on the unit before flashing.",
      "author": { "name": "A. Maker", "url": "https://example.org" },
      "homepage": "https://example.org/zither",
      "source": "https://github.com/example/zither",
      "categories": ["synth", "sequencer"],
      "tags": ["plucked", "karplus-strong", "sequencer", "poly"],
      "hardware": { "fm1": "any", "stock_firmware_min": "V16", "flash": ["P25Q80", "UC25HQ80"] },
      "licence": {
        "spdx": "GPL-3.0-only AND MIT AND OFL-1.1",
        "files": ["LICENSE", "LICENSES/MIT.txt", "LICENSES/OFL-1.1.txt"],
        "third_party": [ { "what": "factory preset names", "rights": "granted", "note": "written by the author" } ],
        "notes": "The font is OFL-1.1; everything else is the author's."
      },
      "needs": { "assets": false, "notes": "" },
      "releases": [
        {
          "version": "1.2.0",
          "date": "2026-11-10",
          "url": "https://github.com/example/zither/releases/download/v1.2.0/ZITHER.fwsc",
          "page": "https://github.com/example/zither/releases/tag/v1.2.0",
          "preview_url": "https://raw.githubusercontent.com/example/zither/v1.2.0/dist/ZITHER.fwsc",
          "sha256": "0c3e9a...64 hex digits...",
          "size": 482112,
          "format": "fwsc",
          "notes": "Fixes the boot loop on UC25HQ80 units.",
          "deprecated": false
        }
      ],
      "links": { "docs": "https://example.org/zither/manual", "support": "https://example.org/zither/help" },
      "catalog": { "thumbnail": true, "preview": true }
    }
  ]
}
```

### Top level

| Key | Type | Required | Meaning |
|---|---|---|---|
| `format` | string | **yes** | `fm1tastik/<major>`. A reader refuses a major it does not know and ignores minor additions. |
| `claim` | object | no | `{ "code": "<one-time code>" }`: present **only while proving a claim**; remove it afterwards (see "Claiming"). A reader never stores the code after use. |
| `listed` | boolean | no, default `true` | **`false` asks every catalog not to list these firmwares.** A catalog **must** honour it, and remove an existing entry on its next read. |
| `contact` | object | no | Public ways to reach the author: `issues` (URL), `email`, `discord` (an invite URL). All public: **never a webhook or a token.** |
| `firmware` | array | **yes**, 1 to 32 | One object per firmware (below). |

### A `firmware` object

| Key | Type | Required | Limit | Meaning |
|---|---|---|---|---|
| `id` | string | **yes** | `[a-z0-9-]`, 1 to 48 | Stable within this file; never reused for another firmware. |
| `name` | string | **yes** | 1 to 64 | The firmware's own name. |
| `summary` | string | **yes** | 1 to 140 | One line for a list. |
| `description` | string | no | up to 2000 | What it does, what it needs. |
| `author` | object | no | `name` up to 64, `url` | A byline. Optional on purpose: it need not be a real name. |
| `homepage`, `source` | URL | no | https | The project page and the source repository. |
| `categories` | array | no | 1 to 3 of a fixed list: `synth`, `sequencer`, `groovebox`, `sampler`, `drum`, `utility`, `midi`, `toy`, `game`, `other` | Where it shows up in browsing. |
| `tags` | array | no | up to 10, each `[a-z0-9-]` up to 24 | Free tags. |
| `hardware` | object | no |  | `fm1`: `"any"` or a hardware revision; `stock_firmware_min`: the stock version it must be flashed over (a string); `flash`: chip names it supports. Unknown values are shown as text. |
| `licence` | object | **yes** |  | See below. |
| `needs` | object | no |  | `assets` (boolean): needs files the user must supply (samples, a ROM); `notes`. |
| `releases` | array | no, up to 64 | newest first | See below. |
| `links` | object | no |  | `docs`, `support`, `donate`, `video`: https URLs. |
| `catalog` | object | no |  | `thumbnail` (boolean, default `true`): `false` asks a catalog not to keep a screenshot of this firmware; `preview` (default `true`): `false` asks a catalog not to run it in a browser. **A catalog must honour both.** |

### `licence`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `spdx` | string | **yes** | An SPDX expression. **Use `AND`** for the parts that all apply, as the whole firmware's licence (not `OR`, unless a recipient really may choose). Non-SPDX terms: `LicenseRef-<name>`, with the text in `files`. |
| `files` | array of paths | **yes**, 1 to 16 | Paths **in the repository** to the licence texts a reader can fetch and compare. |
| `third_party` | array | no | Third-party data or code embedded in the firmware: `{ "what", "rights": "granted" \| "reserved" \| "unknown", "note" }`. **Say `unknown` if you do not know**: it is not an accusation, and a catalog shows it as a flag. |
| `notes` | string | no | up to 500. |

**A reader compares `spdx` with the files and the repository's badge** and shows any difference (a permissive badge on a GPL firmware, or the reverse). The
author's words are shown, a derived difference is shown beside them, and neither silently wins.

### `releases`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `version` | string | **yes** | The author's version string. |
| `date` | date | no | `YYYY-MM-DD`. |
| `url` | URL | **yes** | **A direct, https download of the file the author publishes**, the same bytes a user would flash. Not a mirror, not a re-pack. |
| `page` | URL | no | The page that describes the release. |
| `preview_url` | URL | no | **The same bytes as `url`, at an https address that sends `Access-Control-Allow-Origin`**, so a web page can fetch it to run a preview. A release asset on GitHub cannot be fetched by a browser (no CORS header, measured 2026-11-16); a file committed to the repository can, through `raw.githubusercontent.com` or jsDelivr (`cdn.jsdelivr.net/gh/<owner>/<repo>@<tag>/<path>`). A reader **must check its hash against `sha256`**; a mismatch is ignored and flagged. Optional: without it, a browser user downloads `url` and chooses the file. |
| `sha256` | 64 hex digits | **yes** | The hash of the bytes at `url`. **A reader that fetches the file must check it** and treat a mismatch as a broken or tampered release, not as a new one. |
| `size` | integer | no | Bytes; a reader may refuse a file far over this or over its own cap. |
| `format` | string | no | `fwsc` (the FM-1's `.fwsc` package, the default), or another named container. |
| `notes` | string | no | up to 500. |
| `deprecated` | boolean | no | `true` if the author asks people not to use this release (a bad flash, a security bug). A catalog shows it prominently. |

## Claiming

A catalog that lets authors claim an entry (fm1tastik does: [`FM1TASTIK.md`](FM1TASTIK.md), "Claiming an entry") uses this file as the proof, **because
whoever can commit to the repository's default branch controls the source**:

1. The claimant asks the catalog for a claim on an entry; the catalog gives a **one-time code** (random, 12 or more characters, valid for 7 days, for that entry only).
2. The claimant adds `"claim": { "code": "…" }` to `fm1tastik.json`, commits it to the default branch (or the release tag) and tells the catalog.
3. The catalog fetches the file from the source it already lists for that entry, checks the code, the `source` URL (it must be the entry's own), and **grants the claim**.
4. The claimant **removes `claim`** (a catalog should say so). A leftover code is harmless once used; it must not be accepted twice.

Not on a Git host: put the same file at `/.well-known/fm1tastik.json`, or add a DNS TXT record `_fm1tastik.<domain>` with the text `claim=<code>`.

**A claim is not an identity.** The catalog learns that someone controls this repository or domain, nothing about who. Nothing in this format is an account.

**Secrets never travel in this file.** A catalog that wants to notify an author through a webhook asks for it **privately, in its own claim page**, and stores it as
a secret; it is never read from, or written to, a public repository.

## How a reader must behave

- **Fetch only `https`.** Allow a few redirects, stay on public addresses (refuse localhost, private and link-local ranges: a fetch of a user-supplied URL is
  the classic server-side request forgery), apply a **size cap (64 KiB for this file)** and a **time limit**, and send an honest `User-Agent` naming the catalog.
- **Treat everything as untrusted text.** Escape on output; cap every field; ignore what does not fit; never run, render or follow instructions from it.
- **Be polite:** cache with `ETag` or `If-Modified-Since`, and re-read hourly or less; never open an issue or a pull request on a repository to ask for the file.
- **Honour opt-outs:** `listed: false`, `catalog.thumbnail: false`, `catalog.preview: false`, `deprecated: true`.
- **Show provenance:** whether a value came from the author's file, was derived from the files, or was typed by a moderator.

## Versioning

`fm1tastik/1` allows new optional keys and new values for open lists (`categories` may grow: a reader shows an unknown one as `other`). A change that
removes or reinterprets a key is `fm1tastik/2`; readers refuse a major they do not know and say why. The schema (JSON Schema, draft 2020-12) and a command-line
validator (standard library only) will be published with fm1tastik's repository, with conformance cases: a good file, a file at every limit, and files that
must be refused (over 64 KiB, a bad hash length, a webhook-looking URL in `contact`, an unknown major).

## Who might use it

fm1tastik is the first reader. The aim is a file an author writes once for **awesome-fm-1, the FM-1 Firmware Hub, flashers and launchers, and the FM1-NTT apps**.
Proposed next steps, once there is a validator and a first catalog reading real files:

1. Publish the format and the validator in fm1tastik's repository under CC0 and MIT.
2. Open a discussion with the maintainers of **awesome-fm-1** and the **FM-1 Firmware Hub**, and in the community Discord
   (https://discord.gg/eDdqDfpwY3), asking what they would change.
3. Offer a pull request that adds a `fm1tastik.json` to a few well-known firmwares **only if their authors ask** (never unsolicited).

## Open questions

- Whether `categories` should be a closed list in v1 or free (kept closed here so browsing is predictable).
- Whether `hardware` needs a better model of the FM-1's revisions and flash parts than text (the hardware is documented in `HARDWARE.md`; the facts are still partial).
- Whether a signature (a signed release hash) belongs in v1 (not here; a repository's own signed tags already say who made a release).
- Whether `third_party.rights` needs more values than three.
- How to describe a firmware that **patches** the stock firmware rather than replacing it (FM-1+VA, the macropad patch): probably `format: "patch"` with `base`, to be settled with real examples.
