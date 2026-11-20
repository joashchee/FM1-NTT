// FM1-NTT's core in a page, a Worker or Node: loads the wasm build (web/build.sh) and wraps its C API (core/fm1ntt.h).
// No dependencies, no DOM: the same file runs in a Web Worker, in the page and under Node (the tests and fm1tastik's headless verify).
//
//   import { loadBuild } from './fm1ntt.js';
//   const engine = await loadBuild('/');                       // build-info.json, then the hashed .wasm, its SHA-256 checked
//   const m = engine.newMachine();
//   m.loadPackage(fwscBytes);                                  // throws Error(why) for a bad file; never a crash
//   m.setJit(true);                                            // the wasm JIT: needed for live play (the interpreter alone is 0.3x to 0.6x real time)
//   const { frames, pcm } = m.render(512);                     // 44.1 kHz interleaved stereo Int16; `frames` is how many are real audio
//   const px = m.screen();                                     // Uint32Array(240 * 240), 0x00RRGGBB
//
// The module needs one thing from its host, `env.fm1_jit_compile` (supplied here): the core turns hot blocks into small wasm modules and asks the
// host to compile them and put their `run` in the module's function table. A long-lived engine keeps every region it compiled in that table, so a
// server that verifies many firmwares should `load()` a fresh engine for each (it costs about a millisecond).
// A firmware is untrusted input: its bytes are only ever copied into the module's memory and handed to the core, which returns an error for a bad one.

export const SCREEN_W = 240;
export const SCREEN_H = 240;
export const SAMPLE_RATE = 44100;
/** The most frames one `render` call makes (the scratch buffer's size). */
export const MAX_RENDER_FRAMES = 4096;
/** A package larger than this is refused before it is copied (a real one is about 0.6 MiB). */
export const MAX_PACKAGE_BYTES = 16 << 20;
/** The most instructions one `render` may spend: it bounds a firmware that never makes audio. */
export const DEFAULT_MAX_INSNS = 6_000_000;

/** Compiles the module from bytes, a `WebAssembly.Module`, or a fetch `Response`, and returns an engine. */
export async function load(source) {
  let module;
  if (source instanceof WebAssembly.Module) module = source;
  else if (typeof Response !== 'undefined' && source instanceof Response) module = await compileResponse(source);
  else module = await WebAssembly.compile(source);

  const stats = { compiles: 0, compileMs: 0, failures: 0 };
  let x; // the instance's exports, set below; the hook runs only after instantiation
  const env = {
    fm1_jit_compile(ptr, len, hRead, hWrite, hOp) {
      const t0 = performance.now();
      try {
        const table = x.__indirect_function_table;
        const region = new WebAssembly.Module(new Uint8Array(x.memory.buffer, ptr, len).slice());
        const inst = new WebAssembly.Instance(region, { env: { memory: x.memory, h_read: table.get(hRead), h_write: table.get(hWrite), h_op: table.get(hOp) } });
        const index = table.length;
        table.grow(1);
        table.set(index, inst.exports.run);
        stats.compiles++;
        stats.compileMs += performance.now() - t0;
        return index;
      } catch {
        stats.failures++; // 0 tells the core to leave this block interpreted
        return 0;
      }
    },
  };
  x = (await WebAssembly.instantiate(module, { env })).exports;
  return new Engine(x, stats);
}

/** Fetches `<base>build-info.json`, then the hashed module it names, checks its SHA-256, and loads it. */
export async function loadBuild(base = '/') {
  const info = await (await fetchOk(base + 'build-info.json')).json();
  if (info.format !== 1 || !/^fm1ntt-[0-9a-f]{12}\.wasm$/.test(info.wasm)) throw new Error('build-info.json is not one this loader reads');
  const bytes = await (await fetchOk(base + info.wasm)).arrayBuffer();
  const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map((b) => b.toString(16).padStart(2, '0')).join('');
  if (digest !== info.sha256) throw new Error('the wasm module does not match its recorded hash: not loaded');
  const engine = await load(bytes);
  engine.info = info;
  return engine;
}

async function fetchOk(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
  return r;
}

async function compileResponse(r) {
  return typeof WebAssembly.compileStreaming === 'function' ? WebAssembly.compileStreaming(r) : WebAssembly.compile(await r.arrayBuffer());
}

export class Engine {
  constructor(x, stats) {
    this.x = x;
    /** The JIT's counters: regions compiled, time spent compiling, compiles that failed (the core then interprets that block). */
    this.stats = stats;
    this.info = null;
    this.version = this.cstr(x.fm1ntt_version());
    this.clockMHz = x.fm1ntt_clock_mhz();
  }
  /** A NUL-terminated string the module returned (valid until its next call). */
  cstr(ptr) {
    if (!ptr) return '';
    const mem = new Uint8Array(this.x.memory.buffer);
    let end = ptr;
    while (mem[end] !== 0 && end - ptr < 4096) end++;
    return new TextDecoder().decode(mem.subarray(ptr, end));
  }
  /** The core's last error text. */
  lastError() {
    return this.cstr(this.x.fm1ntt_error());
  }
  newMachine() {
    return new Machine(this);
  }
  /**
   * The standard audition of a `.fwsc` package (`core/src/audition.rs`): four notes, no wall clock, so the same firmware gives the same answer on any computer.
   * Returns `{ verdict: 'pass' | 'silent' | 'blank' | 'halted', colours, peak, rmsDb, active, emulatedMs, size, sha256, png }` (`png` a Uint8Array of the final screen),
   * or throws an Error with the core's reason when the file is not a package. It runs the whole script before it returns: call it in a Worker.
   */
  audition(bytes) {
    const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    if (b.length === 0 || b.length > MAX_PACKAGE_BYTES) throw new Error(`a firmware package is 1 to ${MAX_PACKAGE_BYTES} bytes, this is ${b.length}`);
    const x = this.x, OUT = 112, PNG_CAP = 240 * 240 * 3 + 240 + 4096; // the C struct (wasm32: sha256 at 40) and a stored-deflate PNG of the screen
    const fw = x.fm1ntt_wasm_alloc(b.length), out = x.fm1ntt_wasm_alloc(OUT), png = x.fm1ntt_wasm_alloc(PNG_CAP);
    try {
      if (!fw || !out || !png) throw new Error('the module refused an allocation');
      new Uint8Array(x.memory.buffer, fw, b.length).set(b);
      if (x.fm1ntt_audition(fw, b.length, out, png, PNG_CAP) !== 0) throw new Error(this.lastError() || 'the core refused this file');
      const dv = new DataView(x.memory.buffer, out, OUT);
      const pngLen = dv.getUint32(20, true);
      return {
        verdict: ['pass', 'silent', 'blank', 'halted'][dv.getInt32(0, true)] ?? 'halted',
        colours: dv.getUint32(4, true), peak: dv.getInt32(8, true), emulatedMs: dv.getUint32(12, true), size: dv.getUint32(16, true),
        rmsDb: dv.getFloat64(24, true), active: dv.getFloat64(32, true),
        sha256: this.cstr(out + 40),
        png: pngLen <= PNG_CAP ? new Uint8Array(x.memory.buffer, png, pngLen).slice() : new Uint8Array(0),
      };
    } finally {
      if (fw) x.fm1ntt_wasm_free(fw, b.length);
      if (out) x.fm1ntt_wasm_free(out, OUT);
      if (png) x.fm1ntt_wasm_free(png, PNG_CAP);
    }
  }
}

export class Machine {
  constructor(engine) {
    this.e = engine;
    this.x = engine.x;
    this.h = this.x.fm1ntt_new();
    if (!this.h) throw new Error('the core could not make a machine');
    this.pcmPtr = this.alloc(MAX_RENDER_FRAMES * 4);
    this.scrPtr = this.alloc(SCREEN_W * SCREEN_H * 4);
  }
  alloc(len) {
    const p = this.x.fm1ntt_wasm_alloc(len);
    if (!p) throw new Error(`the module refused an allocation of ${len} bytes`);
    return p;
  }
  /** Boots a `.fwsc` package. Throws an Error with the core's reason when it is not one the core reads; the machine is then unloaded. */
  loadPackage(bytes) {
    const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    if (b.length === 0 || b.length > MAX_PACKAGE_BYTES) throw new Error(`a firmware package is 1 to ${MAX_PACKAGE_BYTES} bytes, this is ${b.length}`);
    const p = this.alloc(b.length);
    try {
      new Uint8Array(this.x.memory.buffer, p, b.length).set(b);
      if (this.x.fm1ntt_load_package(this.h, p, b.length) !== 0 || !this.x.fm1ntt_loaded(this.h)) throw new Error(this.e.lastError() || 'the core refused this file');
    } finally {
      this.x.fm1ntt_wasm_free(p, b.length);
    }
  }
  get loaded() {
    return !!this.x.fm1ntt_loaded(this.h);
  }
  /** Turns the wasm JIT on or off (off by default). */
  setJit(on) {
    this.x.fm1ntt_jit(this.h, on ? 1 : 0);
  }
  /**
   * Runs the machine until it has made `frames` of audio (1 to 4096) or spent `maxInsns`. Returns `{ frames, pcm }`: `frames` is how many are real audio
   * (fewer means the emulation was too slow or halted), `pcm` the interleaved stereo samples, silence after the real ones. Pass `out` (an Int16Array of
   * at least 2 * frames) to reuse a buffer.
   */
  render(frames = 512, maxInsns = DEFAULT_MAX_INSNS, out) {
    if (!Number.isInteger(frames) || frames < 1 || frames > MAX_RENDER_FRAMES) throw new RangeError(`frames is 1 to ${MAX_RENDER_FRAMES}`);
    const got = this.x.fm1ntt_render(this.h, this.pcmPtr, frames, maxInsns);
    const pcm = out ?? new Int16Array(frames * 2);
    pcm.set(new Int16Array(this.x.memory.buffer, this.pcmPtr, frames * 2));
    return { frames: got, pcm };
  }
  /** The 240 x 240 screen as 0x00RRGGBB words (a copy). Mask each channel with `& 255` before putting it in a canvas's clamped array. */
  screen() {
    this.x.fm1ntt_screen(this.h, this.scrPtr);
    return new Uint32Array(this.x.memory.buffer, this.scrPtr, SCREEN_W * SCREEN_H).slice();
  }
  /** A button (0..13) or note key (14..40) goes down or up. */
  key(id, down) {
    this.x.fm1ntt_key(this.h, id, down ? 1 : 0);
  }
  encoder(index, clicks) {
    this.x.fm1ntt_encoder(this.h, index, clicks);
  }
  pot(value) {
    this.x.fm1ntt_pot(this.h, value);
  }
  midiIn(byte) {
    this.x.fm1ntt_midi_in(this.h, byte);
  }
  get halted() {
    return !!this.x.fm1ntt_halted(this.h);
  }
  /** Emulated milliseconds since boot. */
  get ms() {
    return Number(this.x.fm1ntt_ms(this.h));
  }
  /** Frees the machine and its scratch buffers. The object must not be used afterwards. */
  free() {
    if (!this.h) return;
    this.x.fm1ntt_free(this.h);
    this.x.fm1ntt_wasm_free(this.pcmPtr, MAX_RENDER_FRAMES * 4);
    this.x.fm1ntt_wasm_free(this.scrPtr, SCREEN_W * SCREEN_H * 4);
    this.h = 0;
  }
}
