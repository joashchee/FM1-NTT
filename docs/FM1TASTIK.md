# fm1tastik: scope and phased plan

Scoped **2026-11-16** at the maintainer's brief: *a web app, `fm1tastik.chordica.com`, modelled on Fontastik: a crowdsourced catalog of FM-1
firmware (ratings, licences, metadata, categories, tags, user-contributed links) with FM1-NTT running in the page to load and preview a
firmware, and to collect upvotes and red flags.* Its parameters differ from the R36S port's: a server, strangers' data, a browser's limits.

Nothing here is built except the Phase 0 spikes (`spikes/wasm/`, results below). **Speed in a real browser, on a phone, AudioWorklet sound, and fetching a firmware from its link are still untested** (Phase 0 results).

## Decisions (maintainer, 2026-11-16)

| Question | Decision |
|---|---|
| Where the code lives | **A separate repository, `fm1tastik`** (maintainer, 2026-11-16). It consumes FM1-NTT's core built to WebAssembly; FM1-NTT stays MIT and the Pack stays GPL. |
| Firebase | **A new Firebase project**, not Fontastik's (maintainer, 2026-11-16). Setup steps are in "Firebase setup" below, to be walked through at Phase 1. |
| Ports | **Every FM1-NTT port (R36S, macOS, Windows, Linux, web) eventually reads the same catalog** (maintainer, 2026-11-16): see "One catalog, every port". |
| Claiming | **An author can claim an entry**, proved out of band (maintainer, 2026-11-16): see "Claiming an entry". |
| Stack and rules | **Fontastik's**: Firebase (Blaze: Firestore, rules, batched Cloud Functions, App Check), a local-first admin app like FontastikA, `docs/fontastik-framework.md` as the pattern. Server-hit economy is a standing rule. |
| Preview's bytes | **Direct fetch from the link where CORS allows; otherwise the user downloads the file and drops it in.** The bytes never touch the server and never leave the browser. No proxy. |
| Identity | **Anonymous, resettable install UUID + App Check + rate limits** for votes, flags and submissions; a moderator queue. No accounts, no persistent identity, an erasure path (GDPR, CCPA). |
| Verification | **The server fetches each submitted link, hashes the file, boots it headlessly in the wasm core, reads its licence files and keeps one 240 x 240 screenshot as the thumbnail; the bytes are discarded.** Moderators can remove a thumbnail. |
| Publisher | **Scoreless Music** on the site and the privacy policy, as Fontastik (maintainer, 2026-11-16). |
| Discord | **https://discord.gg/eDdqDfpwY3** (maintainer, 2026-11-16): the community and the moderators' route; the moderators' private channel and its webhook are set up at Phase 4b. |
| Look | **Match Fontastik** (layout, components, conventions) with an FM-1 palette. Not the pixel panels, not the text-mode UI. |
| Domain | **`fm1tastik.chordica.com`** beside Fontastik (maintainer, 2026-11-16: "my mistake"; it was first written as `fm1tastik.ansiapps.com`). The DNS is at chordica.com's registrar. |

## Rules (carried from Fontastik, adapted)

1. **The catalog never hosts a firmware.** Every link goes to where the author published it. The server fetches a file only to verify it, in memory,
   with a size and time bound, and keeps the hash and the metadata (and the thumbnail), never the bytes.
2. **The licence flags are FM1-NTT's** (`CLAUDE.md` rule 2): AGPL, non-commercial, closed, unlicensed, embedded third-party data, and a licence
   that differs from the repository's badge are shown plainly on the entry; the catalog links to them, it does not bundle them. The page says what
   a licence lets you do and does not give legal advice.
3. **A firmware is untrusted input** (core rule 5): it runs in a Web Worker behind a CSP, bounded in memory and time, and never touches the page's
   storage or the network. A crash or hang is a result ("did not boot"), not an outage.
4. **Identity is a random local UUID**, resettable, never linked to a person; consent before anything is sent; erasure on request. **A claim proves control of a firmware's source, not who someone is**: the catalog stores the public repository or page, never a name, an email or a login.
5. **An installable PWA from the first commit, no service worker** (global rule). Untested on a phone until tried.
6. **Fast start:** a static splash at once; the wasm core loads after the page is usable; a determinate bar; any error shown on the splash.
7. **Cut server hits first:** batched ingestion of votes and flags (one function invocation for many), hourly background pulls, deltas not whole
   documents, a local cache before the server.
8. **Check for leaked keys and open write paths before every commit** (Fontastik's rule).

## What an entry holds

- **Identity:** a firmware (name, author, home page, source repository) with **versions**; a version is its **SHA-256** (the same file is one version
  wherever it is linked), its link(s), the date seen, its size.
- **Licence:** an AND-expression of SPDX identifiers, the licence texts' links, and flags (rule 2). Read from the repository's own licence files at the
  release tag, not from its badge; changes are caught by `scripts/check-firmware-updates.py`'s logic, run as a scheduled function.
- **Facts:** the FM-1 hardware and stock-firmware versions it needs, whether it needs bring-your-own assets, sound engines, the flash chip it
  supports (P25Q80 or UC25HQ80), whether it boots in FM1-NTT and how fast, the thumbnail, link health.
- **Browse:** categories (synth, sequencer and groovebox, sampler, drum machine, utility, game and toy), free tags, search, sort by rating, newest and
  most flagged; each entry a page.
- **Community:** an upvote per version per install; **red flags** of fixed kinds (wrong licence, broken link, won't boot, brick risk, malware,
  embeds third-party data, other) with an optional note; a rating shown only past a minimum number of votes; entries past a flag threshold go to
  the moderator queue and carry a banner until cleared.
- **Contributions:** submit a link; the verifier fills the hash, licences, boot result and thumbnail; a moderator approves; a duplicate hash joins the
  existing firmware.

## Preview

The page runs FM1-NTT's core as WebAssembly in a Worker with an AudioWorklet (`ARCHITECTURE.md` already plans this). It shows the FM-1's screen at 1x
and 2x, a 27-key on-screen keyboard, the keyboard, Web MIDI, and the encoders and buttons. Preview's own actions: **Boot**, **Audition**, **Vote**, **Flag**.

- **Audition** plays a fixed script of notes (the vendored core has `fm1_soc::script`) so every firmware is heard the same way. **Because the
  native interpreter ran WORN TAPE at 0.90x real time on an M1, the wasm interpreter runs at about 0.55x, and a phone is slower still, live play
  may fall under real time without the core's wasm JIT** (which Phase 0 found: 2.8x to 3.8x on an M1 under Node; see Phase 0 results). So Audition **bounces**: it renders N seconds offline in the Worker, then plays the buffer, with a progress bar. Live play is offered
  where a speed check passes. Phase 0 measures this; it is the biggest unknown.
- **Getting the file:** **Phase 0 showed a browser cannot fetch a GitHub release link** (CORS), so the main path is: the author's page one tap away, then a file picker and drop zone ("download it from its author, then choose it here"), the file checked against the catalog's hash; a `preview_url` the author publishes with CORS gives one-click Preview. The file
  stays in the tab. Votes and flags need no firmware at all.
- **A verified boot is not an endorsement:** the page says the emulator is not the hardware, and nothing here has run on a real FM-1.

## Claiming an entry

A claim lets the person who controls a firmware's source edit its entry. **It must not need an account** (rule 4), so the proof is control of the
source, checked out of band, never a password or a social login.

**Recommended routes, strongest first** (the verifier tries the one the entry's source allows):

1. **A file in the author's repository (GitHub).** The claimant starts a claim in the page and gets a one-time code. They commit a
   **`fm1tastik.json`** (format: [`FM1TASTIK-JSON.md`](FM1TASTIK-JSON.md)) at the repository's root (or the release notes carry the code), and a function reads it from the public raw URL. Control of the
   repository is the proof: no OAuth, no GitHub login, nothing stored about a person. The same file is then the author's own **metadata**
   (name, tags, categories, licence, releases with hashes, links, public contact), so the catalog can re-read it on a schedule instead of being typed into.
2. **A DNS TXT record or a file on the project's own site** (for firmware not on GitHub: itch.io pages, personal sites), with the same code.
3. **Discord, by a moderator.** For closed or paid firmware with no repository, or where 1 and 2 are impossible. The claim opens a private
   moderator thread (a webhook into a private channel, with the claim's id and the entry); the author writes from the project's own published
   contact (the Discord or the page named on its release), and a moderator confirms and approves. Manual, slow, and the fallback only.
4. **Not recommended: GitHub or Discord OAuth.** It would store a persistent identity and break rule 4.

**What a claim holder can do:** edit the description, tags and links; add a version; reply to a red flag (the reply shows on the entry); deprecate a
version; ask to remove a thumbnail or the entry; give the claim page a private notify address or webhook. **What they cannot:** delete votes or flags, change a **licence flag the
verifier derived from the files** (they show the dispute; a moderator rules), or hide their own red flags.

**The claim key** is a random secret kept in the claimant's browser (exportable, rotatable); it is the only credential. To rotate or recover, prove
control of the source again (route 1 or 2). A repository that later changes hands moves the claim with a fresh proof; a failed re-proof suspends edits,
not the entry.

**Out-of-band notices, so nobody has to watch the site** (recommended, in this order):

- **Moderators:** a **Discord webhook** into a private channel for: a new claim, a submission waiting, a flag over its threshold, a takedown request, a
  link or licence change found by the checker. Discord is a *notification*, not a source of truth: every item links to the admin app, which holds the state.
- **Authors:** **opt-in only**, by the public `contact` in their `fm1tastik.json` (an issues page, an email or a Discord invite) or, for a claimed entry, a
  **webhook or an email they give the claim page privately** (stored as a secret, **never read from or written to a public file**: a webhook in a public repository can be
  spammed by anyone). **Never open an issue or a PR on a stranger's repository**: it is unsolicited, can be read as
  spam, and some maintainers are already wary of this ecosystem's bots.
- **Public:** a **GitHub issues page** on the `fm1tastik` repository for "my licence is wrong" and takedowns, with a template, so there is a public,
  dated record; a link to it on every entry (it is also the contact a licence correction needs).
- **Abuse limits:** one open claim per entry, a daily cap per IP and per App Check token, claims on an entry that already has a holder go to a moderator.

## One catalog, every port

Every FM1-NTT port should be able to browse the catalog (ratings, licences, flags, hashes, links, categories) and **download from the author's own link**; none
of them ever gets a firmware from our server.

- **A snapshot, not an API, for reading:** the server publishes a versioned **catalog snapshot** (and small deltas since a revision) as static files on
  Firebase Hosting, rebuilt hourly by a function. Reading it costs no Firestore reads and works from a CDN. A port caches it under the app's folder
  (temp name, then rename) and shows the cached copy offline; the R36S has no network on most units, so the cached or **sideloaded** snapshot (a file on the
  card) must work.
- **A format the core can read without a dependency** (core rule 4: no new dependency without asking): a line-based, versioned, size-bounded text format in
  the manner of `MANIFEST.tsv`, parsed by a small module in the core that treats it as untrusted input (every size bounded, an `Error` never a crash). The web
  app may use JSON for the same data; one is generated from the other.
- **Writes (votes and flags) from a port** go through one small batched endpoint with the same install UUID and App Check where the platform has it
  (the web and the desktop apps); where there is none (the R36S), a vote is queued locally and sent when a network exists, or not at all: say which.
- **The ports:** the FILES panel gains a **CATALOG** view (browse, filter by licence, see flags, copy or open the link; a **Get** that downloads to `firmware/`
  from the author's link and checks the hash); the web build is the full app. **Untested on any port until built.**
- **A firmware from the catalog never changes a tier:** the pack and permissive tiers still ship only what their manifests list; a catalog download is the
  user's own file, like any file they put in `firmware/`.

## Firebase setup

To be done **at the start of Phase 1**, with the maintainer, step by step (the console is the maintainer's; no credential ever enters the repository, an environment or CI).
Written here so nothing is forgotten; each step is confirmed before the next.

1. **Create the project** in the Firebase console: name `fm1tastik`; Google Analytics **off** (rule 4 and the privacy rules). Note the project id.
2. **Upgrade to Blaze** (Cloud Functions 2nd gen need it; Fontastik's rule 12) and **set a budget alert** at a small amount (for example 5, 20 and 50 USD, with
   email) so a runaway function or a scraping bot is seen at once.
3. **Create Firestore** in **production mode**. **Pick the region carefully: it cannot be changed** (a multi-region such as `nam5` or `eur3`, or the region
   nearest most FM-1 owners; I would choose with the maintainer).
4. **Enable Hosting** and **add the custom domain `fm1tastik.chordica.com`**: Hosting, "Add custom domain", then the DNS records the console shows (a TXT to
   verify and an A or CNAME to point) at chordica.com's registrar; wait for the certificate (minutes to hours).
5. **Enable App Check** for the web app with **reCAPTCHA Enterprise** (or a Cloudflare Turnstile custom provider), start in **monitor** mode, then **enforce** on
   Firestore and Functions once the traffic looks right. The R36S and desktop ports cannot attest: they use the snapshot and a rate-limited, batched endpoint.
6. **Authentication only for the admin app** (the moderators): enable one provider (email link or Google), and give the maintainer an **admin custom claim**
   (set once with the Admin SDK from the maintainer's Mac); the public site never uses Auth. (Check FontastikA's own approach in `../fontastik` first and copy it.)
7. **Install the Firebase CLI** (`npm i -g firebase-tools`), run `firebase login` in a **real Terminal** (not `!`), then in the repository `firebase init`
   (Firestore, Functions in TypeScript, Hosting) and `firebase use <project id>`. **`.firebaserc` may be committed; service-account JSON, `.env.local` and keys may not**
   (check `.gitignore` and run the pre-commit security read).
8. **Deploy** the rules first (`firebase deploy --only firestore:rules`), then Hosting, then Functions; **test the rules with the emulator** before any public deploy.
9. **Add the Discord webhook** for the moderators' private channel as a **Functions secret** (`firebase functions:secrets:set DISCORD_WEBHOOK`), never in the code.
10. **Backups:** schedule Firestore exports to a Cloud Storage bucket (cheap, regional); say who can restore.

## Phases

Each phase ends with something that works and is checked; a phase's items tick in `ROADMAP.md` of the repository that holds the work.

**Phase 0: spikes** (in this repository; decides the rest). Run 2026-11-16 on an Apple M1 under **Node 26 (V8, the engine Chrome uses)**; `spikes/wasm/`.
- [x] **Core to `wasm32-unknown-unknown`:** builds first time with no code change (a spike crate re-exports the C API as a `cdylib`; `panic = "abort"`, no new dependency). **2.6 MB raw, 755 KB gzipped** (no `wasm-opt`; later tried and not worth it, see the web build below). Instantiates in about 1 ms, boots a firmware in 1 to 3 ms, **about 50 MiB** of wasm memory per machine.
- [x] **Found: the vendored core already has a WebAssembly JIT** (`fm1-core`'s `jit_wasm.rs`, on by `target_arch = "wasm32"`): hot blocks become small wasm modules the *host* compiles. The host must supply one import, `env.fm1_jit_compile`, and the build must export the function table (`--export-table --growable-table`, in `spikes/wasm/.cargo/config.toml`); `spikes/wasm/host.mjs` and `worker.js` are a 10-line implementation. (I first wrote that wasm has no JIT: wrong.)
- [x] **Speed under Node, all five bundled firmwares** (`bench.mjs`, 8 emulated seconds, 1.0x = real time): **interpreter 0.53x to 0.58x; wasm JIT 2.83x to 3.76x** (WORN TAPE 3.17x against the native JIT's 5.12x on this Mac: about 60%). 1,300 to 1,450 regions compile in about 40 ms in total. So **live play is plausible on a recent laptop**; a phone or a low-end laptop is still unmeasured, and the bounce stays as the fallback.
- [x] **Determinism:** the wasm interpreter and the wasm JIT give byte-identical audio and screen over 3 emulated seconds (`verify.mjs`, UNTRUE STEP). Not yet compared with the native core's frames.
- [x] **Headless verify** (`verify.mjs`: hash, boot, run N emulated seconds, halted or not, distinct colours, silent or not, a 240 x 240 PNG): works, a good thumbnail comes out (UNTRUE STEP's 2-Step screen, 75 colours). **About 1 s of CPU for 3 emulated seconds with the JIT** (5 s without), 50 MiB. A server worker can run it under a hard time cap (kill on 10 s) and a memory cap (reject over a fixed size before booting). Not run on a Functions or Cloud Run image yet.
- [x] **Real browser, first result (maintainer, 2026-11-16): Chrome 157 on a Mac (Intel Mac OS X user agent, 8 cores, 8 GB), the Worker page, 6 emulated seconds of the file chosen:** interpreter **0.57x**, wasm JIT **2.87x**, boot 5 to 6 ms, not halted. **It matches Node's figures (0.53x to 0.58x, 2.8x to 3.8x)**, so the Node numbers are a fair guide for Chrome and the Worker plus the JIT hook work in a real browser. Which firmware and which Mac were not stated.
- [x] **Safari 18.6 on a Mac (maintainer, 2026-11-16), the same page:** interpreter **0.32x**, wasm JIT **2.66x**, boot 13 to 27 ms, not halted. **The JIT works in Safari** (the region-size caps for it hold) and is within 8% of Chrome's 2.87x; the interpreter alone is much slower there (0.32x against 0.57x), so **the JIT is not optional in Safari**: without it nothing is live. Which firmware was not stated.
- [x] **Mobiles deferred** (maintainer, 2026-11-16: "skip mobiles for now"): no iOS or Android result; the live-play decision for phones waits. Until then the page is **desktop-first** and a phone gets the bounce and the drop-the-file path.
- [x] **The live sound page is written** (`spikes/wasm/live.html`, `live-worker.js`, `live-worklet.js`: a Worker renders while an audio queue is low, an AudioWorklet plays it, the screen is drawn on a canvas, the 27 keys are on the keyboard). The Worker's logic ran in Node against a fake audio port (UNTRUE STEP with the JIT: **5 ms to render an 11.6 ms buffer**, 4 short buffers in the first second while the JIT warms up, then none); the smoke test **found and I fixed a bug** (the JIT hook read the wrong object, so every compile failed). **Run in Chrome 157 on a Mac (maintainer, 2026-11-16): 8,486 buffers (about 98 s of live play), `short` 4 (the JIT's first second), `under` 0, 4.6 ms to render an 11.6 ms buffer (about 40% of one core), base latency 5.4 ms and output latency 16 ms.** The report is clean; whether the sound was right to the ear, and key feel, were not stated. [x] **Safari (maintainer, 2026-11-16): sound heard from Felucca, keys feel responsive.** [x] The screen first showed as cyan with white text: **my canvas code, not the core** (`Uint8ClampedArray` clamps, so `v >> 8` saturated the green and blue channels; fixed with `& 255`; the headless render of Felucca is correct). [x] **Safari 18.6 counters (maintainer):** 2,846 buffers (about 33 s), `short` 4 (the JIT's first second), `under` 0, 6.4 ms to render an 11.6 ms buffer (about 55% of one core, against Chrome's 40%), base latency 2.9 ms and output 5.1 ms. [ ] Reload to see the fixed screen. **Phase 0 on desktop: go.**
- [ ] **Still untested: Firefox, a low-end laptop, and (deferred) phones.**
- [x] **CORS survey, run by me 2026-11-16** (`spikes/wasm/cors-probe.sh`, `curl` with an `Origin` header, redirects followed by hand; the header decides, though curl is not a browser, and a browser needs it on **every hop**). **A browser cannot fetch a firmware from a GitHub release link.** Tested on the real release assets of Felucca 1.8.2, FoMni 0.4.1, Jangada 1.0.1 and Haunted FM-1 v0.1:

  | Host | Result |
  |---|---|
  | `github.com/<o>/<r>/releases/download/...` (what authors link) | **blocked**: the first hop and the redirect target, `release-assets.githubusercontent.com`, send no `Access-Control-Allow-Origin` |
  | `api.github.com/repos/<o>/<r>/releases/assets/<id>` (Accept: octet-stream) | **blocked**: the first hop allows `*`, but it redirects to `release-assets...`, which does not |
  | `api.github.com` (JSON: releases, tags, licence, contents) | **works** (`*`): the catalog's own page can read metadata, hashes and licence files from it |
  | `raw.githubusercontent.com/<o>/<r>/<ref>/<path>` | **works** (`*`), but only for a file **committed** to the repository, not a release asset |
  | `cdn.jsdelivr.net/gh/<o>/<r>@<tag>/<path>` | **works** (`*`), again only for committed files |
  | Codeberg raw, GitLab raw, itch.io, Dropbox, Google Drive, shrub.audio | **blocked** (no header) |
  | GitHub Pages | **varies by site** (one allowed `*`, one did not) |

  Consequence: **most firmware, which is published as a release asset, cannot be fetched by the web app.** The drop-the-file path is not a fallback, it is the main path, and it must be good (a file picker that works on phones, a drop zone, the author's page one tap away, the hash checked against the catalog's so a wrong file is refused). The desktop and R36S apps have no CORS, so their **Get** works for every link. For a firmware whose author wants one-click Preview, `fm1tastik.json` gets an optional `preview_url` (the same bytes, served with CORS, for example a committed file on `raw.githubusercontent.com`; the hash must match): see `FM1TASTIK-JSON.md`. **No proxy**, as decided. The server's own verification fetch is unaffected (no CORS on a server).
- [x] **Cost estimate** (from memory of Firebase's pricing and Fontastik's `CLAUDE.md` quotas; **check the current price list at setup**): reading is the hourly snapshot on Hosting, so **Firestore reads are near zero**; votes and flags are batched into counters, so writes stay far under the free daily quota at a few thousand voters a day; a verification is about 1 CPU-second and a download of a few MB, so **1,000 submissions a month is cents** on a Cloud Function or Cloud Run job; the wasm page weighs about 0.75 MB a first visit (cached after), so the free monthly Hosting transfer covers on the order of ten thousand visits before it costs anything. **The Blaze budget alerts (setup step 2) are the real guard** against a scraper or a loop.
- Exit: **go.** The two things that would change the plan are the browser results (speed on phones, CORS), which need the maintainer and a few devices. Nothing found so far argues for dropping live play or the catalog.

**What this repository owed the web build, and what is done** (2026-11-16, maintainer: "build the real web/ build script"): [x] **`web/build.sh`** (the crate `web/wasm/`, `cdylib`, `panic = "abort"`, symbols stripped, its own workspace and lock file, no new dependency): builds the core for wasm32 with the function table exported and growable, **checks the module's contract** (exactly one import, `env.fm1_jit_compile`; the exports the loader needs), names the file by its SHA-256 (`web/public/fm1ntt-<12 hex>.wasm`, cached for good) and writes `build-info.json` (hash, size, core version, commit, date). Output is gitignored. **2.65 MB, 747 KB gzipped**; `wasm-opt` (binaryen 133) was tried 2026-11-17 and is not used: `-O3` and `-Os` cut the raw module 6% but grew the gzipped one 1.8%, left the JIT's speed unchanged and made the interpreter 8% slower (one firmware, Node, M1; Safari and a phone untried). [x] **`web/public/fm1ntt.js`**, the loader and a `Machine` wrapper (works in a page, a Worker and Node; supplies the JIT hook; `loadBuild` checks the hash; bounded sizes; a bad file is an `Error` with the core's reason). [x] **`fm1ntt_wasm_alloc` and `fm1ntt_wasm_free`** (bounded to 64 MiB) so the page never grows the module's memory behind the allocator. [x] **`native_jit_available` is true on wasm32.** [x] **The server** serves the module as `application/wasm` (immutable), `build-info.json` and the loader, and the **CSP** allows `'wasm-unsafe-eval'` and `worker-src 'self'` (not `'unsafe-eval'`). [x] **`web/wasm.test.ts`** (in `check.sh` and CI, which now adds the wasm32 target): the module's imports and exports, the recorded hash, the served headers and `loadBuild`, every bundled firmware booting, drawing and making sound with the JIT engaging, **wasm interpreter and wasm JIT byte-identical**, a bad file refused with a reason, sizes bounded. `COMPILE_BUDGET` is not refilled (the spike's compiles took about 40 ms in total, so it is not needed yet). [ ] **Still owed:** the scripted audition, the headless verify as a library function, a worklet and Worker runtime in `web/public/` (the spike's `live-*.js` are the prototype), flash save and restore in the loader, (`wasm-opt` is tried and dropped, above).

**Phase 1: the catalog without community** (the new repository; **started 2026-11-16**: scaffold in `fm1tastik/` inside this working tree, git-ignored, because the sandbox cannot create a sibling folder: move it out with `cd .. && mv fm1ntt/fm1tastik . && cd fm1tastik && git init -b main`)
- [ ] **Firebase setup, with the maintainer** (section above), then the first deploy to `fm1tastik.chordica.com`.
- [ ] Scaffold from Fontastik's pattern: web app (PWA, manifest, icons, no service worker), `firestore.rules`, the admin app, CI, `CLAUDE.md`, `ROADMAP.md`, `CHANGELOG.md`, the privacy page.
- [ ] Data model: firmware, version (by hash), licence, tags, category, link health; rules that make the client read-only.
- [ ] Seed from the curated library: the Haunted FM-1 (permissive) and the Pack's three, then the survey's candidates from `FIRMWARE-CATALOG.md`, each with its own licence read.
- [ ] Browse, search, filter by licence, category and tag; the entry page; licence flags shown plainly.
- [ ] Deploy to `fm1tastik.chordica.com`; link-health and licence-change function from `check-firmware-updates.py`.

**Phase 2: Preview**
- [ ] The wasm core in the page: Boot from a link or a dropped file; screen, keyboard, Web MIDI, encoders and buttons; the audition bounce; a speed check that offers live play.
- [ ] Worker isolation, CSP, memory and time bounds; a crash or hang becomes "did not boot".
- [ ] Fast start: splash, determinate progress, error on the splash.
- [ ] A test that boots every bundled firmware in the browser build and compares frames with the native core (determinism).

**Phase 3: community**
- [ ] Anonymous UUID, App Check, rate limits; upvotes per version; red flags with kinds and notes; **batched ingestion** into counters.
- [ ] Rating display rules (minimum votes), flag thresholds, banners, the moderator queue in the admin app.
- [ ] Privacy: consent text, erasure path, GDPR and CCPA read.

**Phase 4: contributions and verification**
- [ ] Submit a link (and its repository): the verifier fetches, hashes, boots headlessly, reads licences, makes the thumbnail, discards the bytes.
- [ ] Moderator approval, duplicate-hash merge, takedown (entry or thumbnail) on request; abuse limits on submissions.
- [ ] Re-verify on a schedule and when the link or licence changes.

**Phase 4b: claiming** (the routes under "Claiming an entry")
- [ ] The `fm1tastik.json` format (`FM1TASTIK-JSON.md`: schema, validator and conformance cases), its verifier, the claim code and key, and the DNS and file routes.
- [ ] Claim-holder edits (rules that let only the key's holder write the allowed fields), replies to flags, deprecation, takedown requests.
- [ ] The Discord webhook for moderators, the Discord moderator route for claims with no source to prove, the public issues page and its template, abuse limits.
- [ ] A read of what the claim form stores: the repository URL and a random key, nothing about a person.

**Phase 5: every port reads the catalog**
- [ ] The snapshot and delta generator (hourly function), the line-based format, its parser in the core with fuzzing, and a sideloadable snapshot file.
- [ ] The CATALOG view in the SDL host (desktop and R36S), and **Get** from the author's link with a hash check; the web app uses the same data.
- [ ] Batched votes and flags from the desktop apps; the R36S's queue-or-not decision; the Device report and Rec demo each gain a catalog line (rules 7).
- [ ] The `.deb`, macOS and Windows and Linux builds carry no catalog snapshot that was not generated by the server (and no firmware: `check-bundled.py` still holds).

**Phase 6: polish and launch**
- [ ] Accessibility (4.5:1 text, keyboard, screen reader), mobile layout, Add to Home Screen on iOS and Android (**say untested until tried**).
- [ ] A legal read before launch: the catalog's licence statements, thumbnails of firmware screens, takedown policy, the server's temporary fetch.
- [ ] Announce to the community; a way to report a wrong licence (the red flag and a contact).

## Open questions

- [x] **The Firebase region and id** (maintainer, 2026-11-16): **`nam5`** (as Fontastik), project **`chordica-fm1tastik`**.
- [ ] **The Discord server** exists (https://discord.gg/eDdqDfpwY3). Still to decide: **who the moderators are**, and a **private moderators' channel** with its webhook (made at Phase 4b). Until then claims by route 3 wait.
- [ ] **Who moderates** besides the maintainer, and the takedown and dispute policy's wording (a legal read in Phase 6).
- [x] **The `fm1tastik.json` proposal is written** (`FM1TASTIK-JSON.md`, draft 0.1). [ ] The schema, validator and conformance cases (Phase 4b), then offer it to awesome-fm-1 and the FM-1 Firmware Hub.

Closed 2026-11-16: publisher text is Scoreless Music; separate repository; new Firebase project; chordica.com domain; every port reads the catalog; claiming with out-of-band proof.
