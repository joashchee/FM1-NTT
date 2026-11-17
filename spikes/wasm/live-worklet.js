// Phase 0 spike: the audio side. Takes interleaved Float32 stereo chunks from the Worker (over a MessagePort) and plays them; reports its queue and underruns.
class Fm1 extends AudioWorkletProcessor {
  constructor() {
    super(); this.q = []; this.pos = 0; this.frames = 0; this.under = 0; this.n = 0; this.began = false; this.out = null;
    this.port.onmessage = (e) => { if (e.data.port) { this.out = e.data.port; this.out.onmessage = (m) => { if (m.data.pcm) { this.q.push(m.data.pcm); this.frames += m.data.pcm.length / 2; this.began = true; } }; } };
  }
  process(_in, outs) {
    const o = outs[0], L = o[0], R = o[1] || o[0];
    for (let i = 0; i < L.length; i++) {
      if (!this.q.length) { L[i] = R[i] = 0; if (this.began) this.under++; continue; }
      const b = this.q[0]; L[i] = b[this.pos * 2]; R[i] = b[this.pos * 2 + 1]; this.pos++; this.frames--;
      if (this.pos * 2 >= b.length) { this.q.shift(); this.pos = 0; }
    }
    if (++this.n % 4 === 0 && this.out) this.out.postMessage({ level: this.frames, under: this.under });
    return true;
  }
}
registerProcessor('fm1', Fm1);
