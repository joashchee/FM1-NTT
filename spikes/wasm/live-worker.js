// Phase 0 spike: the emulation thread. Renders 512-frame buffers while the audio queue is below its target, sends them to the worklet,
// sends the screen about 15 times a second, takes keys. Counts renders that came back short (the emulation was too slow).
let x, h, out, scr, port, queued = 0, under = 0, short = 0, buffers = 0, renderMs = 0, lastScreen = 0, jitFail = 0, started = 0, made = 0;
const TARGET = 4096; // frames kept queued (about 93 ms)
const env = { fm1_jit_compile(ptr, len, hr, hw, ho) {
  try { const { memory, __indirect_function_table: t } = x;
    const m = new WebAssembly.Module(new Uint8Array(memory.buffer, ptr, len).slice());
    const i = new WebAssembly.Instance(m, { env: { memory, h_read: t.get(hr), h_write: t.get(hw), h_op: t.get(ho) } });
    const idx = t.length; t.grow(1); t.set(idx, i.exports.run); return idx; } catch { jitFail++; return 0; } } };
const grab = (n) => { const at = x.memory.buffer.byteLength; x.memory.grow(Math.ceil(n / 65536)); return at; };
function loop() {
  for (let n = 0; n < 6 && queued < TARGET; n++) {
    const t = performance.now(); const got = x.fm1ntt_render(h, out, 512, 6000000); renderMs += performance.now() - t;
    if (got < 512) short++; made += got; buffers++;
    const i16 = new Int16Array(x.memory.buffer, out, 1024), f = new Float32Array(1024);
    for (let i = 0; i < 1024; i++) f[i] = i16[i] / 32768;
    port.postMessage({ pcm: f }, [f.buffer]); queued += 512;
  }
  const now = performance.now();
  if (now - lastScreen > 66) { lastScreen = now; x.fm1ntt_screen(h, scr); postMessage({ screen: new Uint32Array(x.memory.buffer, scr, 57600).slice(),
    stats: { buffers, short, under, queued, avgRenderMs: +(renderMs / Math.max(1, buffers)).toFixed(2), live: +(made / 44100 / ((now - started) / 1000)).toFixed(2), jitFail } }); }
  setTimeout(loop, 2);
}
onmessage = async ({ data: d }) => {
  try {
    if (d.init) {
      x = (await WebAssembly.instantiate(d.wasm, { env })).instance.exports; h = x.fm1ntt_new();
      const p = grab(d.fw.byteLength); out = grab(2048); scr = grab(57600 * 4);
      new Uint8Array(x.memory.buffer, p, d.fw.byteLength).set(new Uint8Array(d.fw));
      x.fm1ntt_load_package(h, p, d.fw.byteLength); if (!x.fm1ntt_loaded(h)) throw new Error('the core refused this file');
      x.fm1ntt_jit(h, d.jit ? 1 : 0); port = d.port; port.onmessage = (m) => { queued = m.data.level; under = m.data.under; };
      started = performance.now(); postMessage({ ready: true }); loop();
    } else if (d.key !== undefined && x) x.fm1ntt_key(h, d.key, d.down ? 1 : 0);
  } catch (e) { postMessage({ error: String((e && e.message) || e) }); }
};
