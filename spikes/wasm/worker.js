// Phase 0 spike: the core in a Web Worker. Messages in: {wasm: ArrayBuffer, fw: ArrayBuffer|null, seconds}; out: progress and a result.
let inst;
const env = { fm1_jit_compile(ptr, len, hr, hw, ho) {
  try { const { memory, __indirect_function_table: t } = inst.exports;
    const m = new WebAssembly.Module(new Uint8Array(memory.buffer, ptr, len).slice());
    const i = new WebAssembly.Instance(m, { env: { memory, h_read: t.get(hr), h_write: t.get(hw), h_op: t.get(ho) } });
    const idx = t.length; t.grow(1); t.set(idx, i.exports.run); return idx; } catch { return 0; } } };
function grab(x, n) { const at = x.memory.buffer.byteLength; x.memory.grow(Math.ceil(n / 65536)); return at; }
async function run(wasm, fw, seconds, jit) {
  const t0 = performance.now();
  inst = (await WebAssembly.instantiate(wasm, { env })).instance; const x = inst.exports;
  const h = x.fm1ntt_new(), p = grab(x, fw.byteLength), out = grab(x, 2048);
  new Uint8Array(x.memory.buffer, p, fw.byteLength).set(new Uint8Array(fw));
  x.fm1ntt_load_package(h, p, fw.byteLength); if (!x.fm1ntt_loaded(h)) throw new Error('the core refused this file');
  x.fm1ntt_jit(h, jit ? 1 : 0);
  const boot = performance.now() - t0; let made = 0; const t1 = performance.now(), n = Math.ceil(seconds * 44100 / 512);
  for (let i = 0; i < n; i++) { made += x.fm1ntt_render(h, out, 512, 6000000); if (i % 40 === 0) postMessage({ progress: i / n, jit }); }
  const wall = (performance.now() - t1) / 1000, audio = made / 44100;
  return { jit, bootMs: +boot.toFixed(0), audioS: +audio.toFixed(2), wallS: +wall.toFixed(2), xRealtime: +(audio / wall).toFixed(2), halted: !!x.fm1ntt_halted(h) };
}
onmessage = async ({ data: { wasm, fw, seconds } }) => {
  try { const a = await run(wasm, fw, seconds, false), b = await run(wasm, fw, seconds, true); postMessage({ done: [a, b] }); }
  catch (e) { postMessage({ error: String(e && e.message || e) }); }
};
