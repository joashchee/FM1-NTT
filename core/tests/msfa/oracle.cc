// The reference for core/src/fm6.rs: the vendored msfa (core/vendor/msfa, Apache-2.0) compiled as it is,
// run on voices this file generates, written to golden.txt (scripts/msfa-oracle.sh). The Rust port must
// give the same numbers bit for bit (core/tests/fm6_golden.rs). Not part of any build of the app.
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <math.h>
#include "../../vendor/msfa/synth.h"
#include "../../vendor/msfa/controllers.h"
#include "../../vendor/msfa/sin.h"
#include "../../vendor/msfa/exp2.h"
#include "../../vendor/msfa/freqlut.h"
#include "../../vendor/msfa/lfo.h"
#include "../../vendor/msfa/patch.h"
#include "../../vendor/msfa/dx7note.h"
// the tables, which the msfa files define
extern int32_t lut[1025];  // freqlut.cc: N_SAMPLES + 1 (the others are declared in sin.h and exp2.h)

static uint64_t rng = 0x9e3779b97f4a7c15ULL;
static unsigned rnd(unsigned n) {  // xorshift64*, deterministic
  rng ^= rng >> 12; rng ^= rng << 25; rng ^= rng >> 27;
  return (unsigned)(((rng * 0x2545F4914F6CDD1DULL) >> 33) % n);
}
static uint64_t fnv(uint64_t h, const void *p, size_t n) {
  const unsigned char *b = (const unsigned char *)p;
  for (size_t i = 0; i < n; i++) { h ^= b[i]; h *= 0x100000001b3ULL; }
  return h;
}
static const uint64_t FNV0 = 0xcbf29ce484222325ULL;

// An original packed voice (128 bytes, the DX7 sysex layout), valid in every field.
static void make_voice(unsigned char v[128], int alg) {
  memset(v, 0, 128);
  for (int op = 0; op < 6; op++) {
    unsigned char *o = v + op * 17;
    for (int i = 0; i < 4; i++) o[i] = 20 + rnd(80);          // rates
    o[4] = 99; o[5] = 60 + rnd(40); o[6] = 40 + rnd(60); o[7] = rnd(3) ? 0 : rnd(40);  // levels
    o[8] = rnd(100); o[9] = rnd(100); o[10] = rnd(100);       // break point, depths
    o[11] = rnd(4) | (rnd(4) << 2);                           // curves
    o[12] = rnd(8) | (rnd(8) << 3);                           // detune, rate scale
    o[13] = rnd(4) | (rnd(8) << 2);                           // velocity sens, amp mod
    o[14] = (op >= 3 || rnd(2)) ? 70 + rnd(30) : 30 + rnd(60);// output level
    unsigned mode = rnd(8) == 0, coarse = mode ? rnd(4) : rnd(32);
    o[15] = mode | (coarse << 1);
    o[16] = rnd(100);                                         // fine
  }
  for (int i = 0; i < 4; i++) v[102 + i] = 30 + rnd(70);      // pitch rates
  v[106] = 50; v[107] = rnd(3) ? 50 : rnd(100); v[108] = 50; v[109] = rnd(3) ? 50 : rnd(100);
  v[110] = alg;
  v[111] = rnd(8) | (rnd(2) << 3);                            // feedback, osc sync
  v[112] = rnd(100); v[113] = rnd(100); v[114] = rnd(100); v[115] = rnd(100);  // lfo
  v[116] = rnd(2) | (rnd(6) << 1) | (rnd(8) << 4);            // lfo sync, wave (0-5), pitch mod sens
  v[117] = 24;
  const char *name = "ORACLE";
  for (int i = 0; i < 10; i++) v[118 + i] = i < 6 ? name[i] : ' ';
  v[123] = '0' + (alg / 10); v[124] = '0' + (alg % 10);
}

int main() {
  const double SR = 44100.0;
  Freqlut::init(SR); Exp2::init(); Sin::init(); Lfo::init(SR); PitchEnv::init(SR);
  printf("# FM1-NTT msfa oracle golden file: written by scripts/msfa-oracle.sh from core/vendor/msfa; do not edit\n");
  printf("rate 44100\n");
  uint64_t h = FNV0; h = fnv(h, sintab, sizeof sintab); printf("table sin %016llx\n", (unsigned long long)h);
  h = fnv(FNV0, exp2tab, sizeof exp2tab); printf("table exp2 %016llx\n", (unsigned long long)h);
  h = fnv(FNV0, lut, sizeof lut); printf("table freq %016llx\n", (unsigned long long)h);
  struct { int note, vel, bend; } sc[3] = { {60, 100, 0x2000}, {36, 30, 0x2000}, {84, 127, 0x3000} };
  for (int vi = 0; vi < 32; vi++) {
    unsigned char bulk[128];
    make_voice(bulk, vi);
    printf("voice %d ", vi);
    for (int i = 0; i < 128; i++) printf("%02x", bulk[i]);
    printf("\n");
    char patch[156];
    UnpackPatch((const char *)bulk, patch);
    for (int s = 0; s < 3; s++) {
      Dx7Note *note = new Dx7Note();   // value-initialised: zero, as the port's default is
      Lfo *lfo = new Lfo();
      Controllers ctrls; memset(&ctrls, 0, sizeof ctrls); ctrls.values_[kControllerPitch] = sc[s].bend;
      lfo->reset(patch + 137);
      lfo->keydown();
      note->init(patch, sc[s].note, sc[s].vel);
      uint64_t hh = FNV0; long long sumabs = 0; int32_t first[4] = {0, 0, 0, 0};
      for (int blk = 0; blk < 60; blk++) {
        if (blk == 30) note->keyup();
        int32_t buf[N]; memset(buf, 0, sizeof buf);
        int32_t lv = lfo->getsample(), ld = lfo->getdelay();
        note->compute(buf, lv, ld, &ctrls);
        hh = fnv(hh, buf, sizeof buf);
        for (int j = 0; j < N; j++) sumabs += buf[j] < 0 ? -(long long)buf[j] : buf[j];
        if (blk == 5) memcpy(first, buf + 32, sizeof first);
      }
      printf("play %d %d %d %d %016llx %lld %d %d %d %d\n", vi, sc[s].note, sc[s].vel, sc[s].bend,
             (unsigned long long)hh, sumabs, first[0], first[1], first[2], first[3]);
      delete note; delete lfo;
    }
  }
  return 0;
}
