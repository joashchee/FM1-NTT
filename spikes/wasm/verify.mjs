// Phase 0 spike: the headless "verify" a server would run on a submitted firmware (docs/FM1TASTIK.md, Phase 4):
// hash, boot, run N emulated seconds, check it draws and does not halt, take a 240x240 screenshot, and compare the interpreter with the wasm JIT.
//   node spikes/wasm/verify.mjs <wasm> <firmware.fwsc> [emulated seconds] [out.png]
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { deflateSync, crc32 } from 'node:zlib';
import { load } from './host.mjs';
const [wasmPath, fwPath, secs = '3', pngPath] = process.argv.slice(2);
const fw = readFileSync(fwPath);
const fnv = (h, a) => { for (let i = 0; i < a.length; i++) h = Math.imul(h ^ (a[i] & 0xff) ^ 0, 16777619) >>> 0; return h; };
function png(rgb) { // 240x240 0x00RRGGBB words -> PNG
  const raw = Buffer.alloc(240 * (1 + 720)); for (let y = 0; y < 240; y++) for (let x = 0; x < 240; x++) { const v = rgb[y * 240 + x], o = y * 721 + 1 + x * 3; raw[o] = v >> 16 & 255; raw[o + 1] = v >> 8 & 255; raw[o + 2] = v & 255; }
  const ch = (t, d) => { const b = Buffer.alloc(12 + d.length); b.writeUInt32BE(d.length); b.write(t, 4); d.copy(b, 8); b.writeUInt32BE(crc32(b.subarray(4, 8 + d.length)), 8 + d.length); return b; };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(240, 0); ihdr.writeUInt32BE(240, 4); ihdr[8] = 8; ihdr[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), ch('IHDR', ihdr), ch('IDAT', deflateSync(raw)), ch('IEND', Buffer.alloc(0))]);
}
function run(jit) {
  const g = load(wasmPath), t0 = performance.now();
  const h = g.boot(fw, jit), bootMs = performance.now() - t0;
  let hp = 2166136261, hs = 2166136261, made = 0, silent = true; const t1 = performance.now();
  for (let i = 0, n = Math.ceil(secs * 44100 / 512); i < n; i++) { const b = g.buffer(h); made += b.n; hp = fnv(hp, b.pcm); if (b.pcm.some((v) => v)) silent = false; }
  const wall = performance.now() - t1, scr = g.screen(h); hs = fnv(hs, scr);
  return { jit, bootMs: +bootMs.toFixed(1), emulatedS: +(made / 44100).toFixed(2), wallS: +(wall / 1000).toFixed(2), halted: !!g.x.fm1ntt_halted(h),
    colours: new Set(scr).size, silent, pcmHash: hp.toString(16), screenHash: hs.toString(16), wasmMemMiB: +(g.x.memory.buffer.byteLength / 1048576).toFixed(1), jitFailures: g.stats.failed, scr };
}
const a = run(false), b = run(true);
console.log('sha256', createHash('sha256').update(fw).digest('hex'), fw.length, 'bytes');
for (const r of [a, b]) { const { scr, ...o } = r; console.log(JSON.stringify(o)); }
console.log(a.pcmHash === b.pcmHash && a.screenHash === b.screenHash ? 'DETERMINISTIC: interpreter and wasm JIT agree (audio and screen)' : 'MISMATCH: interpreter and wasm JIT differ');
if (pngPath) { writeFileSync(pngPath, png(b.scr)); console.log('wrote', pngPath); }
