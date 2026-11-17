// Phase 0 spike (docs/FM1TASTIK.md): runs a firmware in the wasm build under Node (V8, as Chrome) and reports speed against real time.
//   node spikes/wasm/bench.mjs <wasm> <firmware.fwsc> [seconds]
import { readFileSync } from 'node:fs';
const [wasmPath, fwPath, secs = '10'] = process.argv.slice(2);
let inst, compiles = 0, compileMs = 0, failed = 0;
const env = {
  // the core hands over a wasm module for a hot region; install its `run` in the emulator's table, return the index (0 = failed)
  fm1_jit_compile(ptr, len, hr, hw, ho) {
    const t0 = performance.now();
    try {
      const { memory, __indirect_function_table: tbl } = inst.exports;
      const bytes = new Uint8Array(memory.buffer, ptr, len).slice();
      const m = new WebAssembly.Module(bytes);
      const i = new WebAssembly.Instance(m, { env: { memory, h_read: tbl.get(hr), h_write: tbl.get(hw), h_op: tbl.get(ho) } });
      const idx = tbl.length; tbl.grow(1); tbl.set(idx, i.exports.run);
      compiles++; compileMs += performance.now() - t0;
      return idx;
    } catch (e) { failed++; if (failed < 3) console.error('jit compile failed:', e.message); return 0; }
  },
};
const mod = new WebAssembly.Module(readFileSync(wasmPath));
const t0 = performance.now();
inst = new WebAssembly.Instance(mod, { env });
const x = inst.exports;
const fw = readFileSync(fwPath);
function run(jit, seconds) {
  const h = x.fm1ntt_new();
  // copy firmware into wasm memory via a leaked allocation: use fm1ntt's own scratch = grow memory
  const pages = Math.ceil(fw.length / 65536) + 1;
  const base = x.memory.buffer.byteLength; x.memory.grow(pages);
  new Uint8Array(x.memory.buffer, base, fw.length).set(fw);
  const rc = x.fm1ntt_load_package(h, base, fw.length);
  if (rc !== 0 && !x.fm1ntt_loaded(h)) throw new Error('load failed rc=' + rc);
  x.fm1ntt_jit(h, jit ? 1 : 0);
  const frames = 512, outp = x.memory.buffer.byteLength; x.memory.grow(1);
  const need = Math.floor(seconds * 44100 / frames);
  let real = 0; const t = performance.now();
  for (let i = 0; i < need; i++) real += x.fm1ntt_render(h, outp, frames, 6_000_000);
  const wall = (performance.now() - t) / 1000;
  const audio = real / 44100;
  const halted = x.fm1ntt_halted(h);
  x.fm1ntt_free(h);
  return { jit, audio_s: +audio.toFixed(2), wall_s: +wall.toFixed(2), x_realtime: +(audio / wall).toFixed(2), halted };
}
console.log('node', process.version, 'wasm', (readFileSync(wasmPath).length / 1024).toFixed(0), 'KiB, instantiate', (performance.now() - t0).toFixed(1), 'ms');
console.log(run(false, +secs));
console.log(run(true, +secs), 'regions compiled', compiles, 'in', compileMs.toFixed(0), 'ms; failures', failed);
