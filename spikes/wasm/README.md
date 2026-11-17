# Phase 0 spike: the core as WebAssembly

Part of fm1tastik's plan (`docs/FM1TASTIK.md`, Phase 0). **Not shipped; not part of `scripts/check.sh`.** MIT, like the rest.

- **The wasm build is no longer here:** it is `web/build.sh` (crate `web/wasm/`, loader `web/public/fm1ntt.js`, tests `web/wasm.test.ts`). These files are the Phase 0 experiments that decided it; they still work against the built module.
- `host.mjs`: loads the wasm in Node and supplies the one import the core needs (superseded by `web/public/fm1ntt.js`).
- `bench.mjs`: speed against real time, interpreter and JIT. `verify.mjs`: the headless check a server would run (hash, boot, run, screenshot PNG, interpreter vs JIT determinism).
- `index.html`, `worker.js`, `serve.mjs`: the **browser page** (a speed test in a Web Worker, and a CORS probe for firmware links). `chrome-run.mjs`: drives headless Chrome for it (did not start in the build sandbox).

```sh
# from the repository root: build the module, then point W at it
web/build.sh
W=$(ls "$PWD"/web/public/fm1ntt-*.wasm)

node spikes/wasm/bench.mjs   $W bundled-firmware/haunted-fm1/untrue-0.1.fwsc 8
node spikes/wasm/verify.mjs  $W bundled-firmware/haunted-fm1/untrue-0.1.fwsc 3 /tmp/shot.png

# the browser page: serve it, then open it on every device you can (a phone on the same Wi-Fi: http://<this Mac's address>:8099)
node spikes/wasm/serve.mjs $W bundled-firmware/haunted-fm1 8099
#   open http://localhost:8099/ , choose a .fwsc (or http://localhost:8099/?fw=/fw/untrue-0.1.fwsc&secs=6 to run at once)
#   send back: the printed user agent, cores, and the two lines (interpreter, jit). xRealtime above 1.0 is fast enough for live play.
#   section 2: paste real firmware download URLs, press Probe: OK means a browser can fetch it, FAIL means CORS blocks it.
```

## The live sound page

`live.html` runs the core in a Worker and plays it through an AudioWorklet (the Worker renders 512-frame buffers while the queue is under about 93 ms and sends them to the worklet over a MessageChannel; the screen is drawn on a canvas; the 27 keys are on the keyboard). Open `http://localhost:8099/live.html`, choose a firmware, press Start, play, and leave it a minute. The report shows `short` (buffers the emulation could not finish) and `under` (silent samples the speaker got): **both should stay 0** (a handful of `short` in the first second is the JIT warming up). Its Worker logic was smoke-tested in Node against a fake audio port (UNTRUE STEP: 5 ms to render an 11.6 ms buffer, no underruns); **the page itself has not been run in a browser.**

## CORS probe (from a terminal; no browser needed)

```sh
spikes/wasm/cors-probe.sh https://github.com/hugelton/Felucca/releases/download/v1.8.2/felucca-1.8.2-app.bin \
                          https://raw.githubusercontent.com/<owner>/<repo>/<ref>/<path>
```
Each hop prints its status and `Access-Control-Allow-Origin`; a browser needs the header on **every** hop. Results (2026-11-16) are in `docs/FM1TASTIK.md`, Phase 0.
