// The web build's contract (web/build.sh, web/public/fm1ntt.js): the module has the one import and the exports the loader expects, it is served as
// WebAssembly with a CSP that lets it compile, its recorded hash is true, every bundled firmware boots and draws and (with the JIT) makes sound, the
// JIT really engages, the wasm interpreter and wasm JIT give byte-identical audio and screen, and a bad file is an error, not a crash.
// Needs the build: run web/build.sh first (scripts/check.sh does). Run with `node --test web/`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import type { AddressInfo } from 'node:net';
import { makeServer } from './server.ts';
import { load, loadBuild, MAX_RENDER_FRAMES } from './public/fm1ntt.js';

const PUB = new URL('./public/', import.meta.url);
const built = existsSync(new URL('build-info.json', PUB));
const opts = { skip: built ? false : 'not built: run web/build.sh (needs the wasm32 target)' };
const info = built ? JSON.parse(readFileSync(new URL('build-info.json', PUB), 'utf8')) : null;
const wasm = built ? readFileSync(new URL(info.wasm, PUB)) : null;

function firmwares(): string[] {
  const root = new URL('../bundled-firmware/', import.meta.url).pathname;
  return (readdirSync(root, { recursive: true }) as string[]).filter((f) => f.endsWith('.fwsc')).sort().map((f) => root + f);
}
const fnv = (h: number, a: ArrayLike<number>) => { for (let i = 0; i < a.length; i++) h = Math.imul(h ^ (a[i] & 0xff), 16777619) >>> 0; return h; };

test('the module has the one import and the exports the loader needs', opts, () => {
  const m = new WebAssembly.Module(wasm!);
  assert.deepEqual(WebAssembly.Module.imports(m).map((i) => `${i.module}.${i.name}:${i.kind}`), ['env.fm1_jit_compile:function']);
  const ex = new Set(WebAssembly.Module.exports(m).map((e) => `${e.name}:${e.kind}`));
  for (const w of ['memory:memory', '__indirect_function_table:table', 'fm1ntt_wasm_alloc:function', 'fm1ntt_wasm_free:function', 'fm1ntt_new:function', 'fm1ntt_render:function', 'fm1ntt_jit:function'])
    assert.ok(ex.has(w), `missing export ${w}`);
});

test('build-info.json is true: the file, its hash and its size', opts, () => {
  assert.equal(info.format, 1);
  assert.match(info.wasm, /^fm1ntt-[0-9a-f]{12}\.wasm$/);
  assert.equal(createHash('sha256').update(wasm!).digest('hex'), info.sha256);
  assert.equal(info.wasm, `fm1ntt-${info.sha256.slice(0, 12)}.wasm`);
  assert.equal(wasm!.length, info.bytes);
});

test('the server serves it as WebAssembly, cached for good, and the CSP lets it compile; loadBuild checks the hash', opts, async () => {
  const server = makeServer();
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  try {
    const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
    const r = await fetch(base + info.wasm);
    assert.equal(r.status, 200);
    assert.equal(r.headers.get('content-type'), 'application/wasm');
    assert.equal(r.headers.get('cache-control'), 'public, max-age=31536000, immutable');
    const csp = r.headers.get('content-security-policy') ?? '';
    for (const need of ["'wasm-unsafe-eval'", "worker-src 'self'", "manifest-src 'self'"]) assert.ok(csp.includes(need), `CSP lacks ${need}`);
    assert.ok(!csp.includes("'unsafe-eval'") || csp.includes("'wasm-unsafe-eval'"), 'plain unsafe-eval must not be allowed');
    assert.equal((await fetch(base + 'build-info.json')).headers.get('cache-control'), 'no-cache');
    assert.equal((await fetch(base + 'fm1ntt.js')).headers.get('content-type'), 'text/javascript; charset=utf-8');
    assert.equal((await fetch(base + 'fm1ntt-000000000000.wasm')).status, 404);
    const engine = await loadBuild(base);
    assert.equal(engine.info.sha256, info.sha256);
    assert.match(engine.version, /^\d+\.\d+\.\d+/);
  } finally {
    server.close();
  }
});

test('every bundled firmware boots, draws, makes sound with the JIT, and the JIT engages', opts, async () => {
  const files = firmwares();
  assert.ok(files.length >= 5, 'the bundled firmware is missing');
  let withSound = 0;
  for (const f of files) {
    const engine = await load(wasm!); // a fresh engine each time: the table keeps every compiled region
    const m = engine.newMachine();
    m.loadPackage(readFileSync(f));
    m.setJit(true);
    let sound = false, real = 0;
    for (let i = 0; i < 90; i++) { const { frames, pcm } = m.render(512); real += frames; if (pcm.some((v) => v !== 0)) sound = true; }
    assert.ok(!m.halted, `${f} halted`);
    assert.ok(real > 90 * 512 * 0.9, `${f} made only ${real} frames`);
    assert.ok(new Set(m.screen()).size > 8, `${f} drew almost nothing`);
    assert.equal(engine.stats.failures, 0, `${f}: JIT compiles failed`);
    assert.ok(engine.stats.compiles > 0, `${f}: the JIT never engaged`);
    if (sound) withSound++;
    m.free();
  }
  assert.ok(withSound >= 1, 'no bundled firmware made any sound in 1 second');
});

test('the wasm interpreter and the wasm JIT give byte-identical audio and screen', opts, async () => {
  const f = firmwares().find((x) => x.includes('untrue'))!;
  const run = async (jit: boolean) => {
    const engine = await load(wasm!);
    const m = engine.newMachine();
    m.loadPackage(readFileSync(f));
    m.setJit(jit);
    let h = 2166136261;
    for (let i = 0; i < 90; i++) h = fnv(h, m.render(512).pcm);
    const out = { audio: h, screen: fnv(2166136261, m.screen()), compiles: engine.stats.compiles };
    m.free();
    return out;
  };
  const a = await run(false), b = await run(true);
  assert.equal(a.compiles, 0, 'the interpreter run compiled something');
  assert.ok(b.compiles > 0);
  assert.equal(a.audio, b.audio, 'audio differs');
  assert.equal(a.screen, b.screen, 'screen differs');
});

test('a bad file is an error with a reason, never a crash; sizes are bounded', opts, async () => {
  const engine = await load(wasm!);
  const m = engine.newMachine();
  for (const bad of [new Uint8Array(0), new Uint8Array(100), new Uint8Array(4096).fill(0xff), new Uint8Array(1 << 20)]) assert.throws(() => m.loadPackage(bad), /./);
  assert.ok(!m.loaded);
  assert.throws(() => m.loadPackage(new Uint8Array((16 << 20) + 1)), /1 to/);
  assert.throws(() => m.render(MAX_RENDER_FRAMES + 1), RangeError);
  assert.equal(engine.x.fm1ntt_wasm_alloc(65 << 20), 0, 'the allocator must refuse over 64 MiB');
  assert.equal(m.screen().every((v) => v === 0), true, 'an unloaded machine shows black');
  // the machine still works after the refusals
  m.loadPackage(readFileSync(firmwares()[0]));
  assert.ok(m.loaded);
  m.free();
});
