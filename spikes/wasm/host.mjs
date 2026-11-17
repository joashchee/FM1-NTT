// Shared by the Node spikes: loads the wasm build, supplies the JIT compile hook the core expects, wraps the C API.
import { readFileSync } from 'node:fs';
export function load(wasmPath) {
  let inst; const stats = { compiles: 0, compileMs: 0, failed: 0 };
  const env = {
    fm1_jit_compile(ptr, len, hr, hw, ho) {
      const t0 = performance.now();
      try {
        const { memory, __indirect_function_table: tbl } = inst.exports;
        const m = new WebAssembly.Module(new Uint8Array(memory.buffer, ptr, len).slice());
        const i = new WebAssembly.Instance(m, { env: { memory, h_read: tbl.get(hr), h_write: tbl.get(hw), h_op: tbl.get(ho) } });
        const idx = tbl.length; tbl.grow(1); tbl.set(idx, i.exports.run);
        stats.compiles++; stats.compileMs += performance.now() - t0; return idx;
      } catch { stats.failed++; return 0; }
    },
  };
  inst = new WebAssembly.Instance(new WebAssembly.Module(readFileSync(wasmPath)), { env });
  const x = inst.exports;
  const scratch = (n) => { const at = x.memory.buffer.byteLength; x.memory.grow(Math.ceil(n / 65536)); return at; };
  const out = scratch(512 * 4), scr = scratch(57600 * 4);
  return {
    x, stats,
    boot(bytes, jit) {
      const h = x.fm1ntt_new(), p = scratch(bytes.length);
      new Uint8Array(x.memory.buffer, p, bytes.length).set(bytes);
      x.fm1ntt_load_package(h, p, bytes.length);
      if (!x.fm1ntt_loaded(h)) throw new Error('not loaded');
      x.fm1ntt_jit(h, jit ? 1 : 0); return h;
    },
    // one 512-frame buffer; returns the PCM as a fresh Int16Array (frames actually made are `n`)
    buffer(h) { const n = x.fm1ntt_render(h, out, 512, 6_000_000); return { n, pcm: new Int16Array(x.memory.buffer, out, 1024).slice() }; },
    screen(h) { x.fm1ntt_screen(h, scr); return new Uint32Array(x.memory.buffer, scr, 57600).slice(); },
  };
}
