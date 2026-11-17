/* scripts/firmware-audit.c: one firmware, to one TSV line: does it boot, and what does it cost the emulator per emulated second (the R36S audit,
 * docs/R36S-FIRMWARE.md). macOS only (host instructions retired come from proc_pid_rusage). Build, from the repository root, after core/ is built:
 *   cc -O2 -Icore -o firmware-audit scripts/firmware-audit.c build/target/release/libfm1ntt.a -lm
 * Run one process per firmware (it can hang or crash on a bad file): firmware-audit FILE.fwsc. Columns: the file, the board, "ok", the warm-up, then the
 * guest and host instructions per emulated second (idle) at pace 0 and 3 (g0 h0 mac0 g3 h3 mac3), then the host instructions with six notes held (H0 H3). */// audit <firmware>: boot, warm up, then host instructions per emulated second at pace 0 and 3. One TSV line.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <libproc.h>
#include <sys/resource.h>
#include <mach/mach_time.h>
#include "fm1ntt.h"
static uint64_t instr(void) { struct rusage_info_v4 ri; proc_pid_rusage(getpid(), RUSAGE_INFO_V4, (rusage_info_t *)&ri); return ri.ri_instructions; }
static double now_s(void) { static mach_timebase_info_data_t tb; if (!tb.denom) mach_timebase_info(&tb); return (double)mach_absolute_time() * tb.numer / tb.denom / 1e9; }
int main(int argc, char **argv) {
    FILE *f = fopen(argv[1], "rb"); if (!f) { printf("%s\tnofile\n", argv[1]); return 1; }
    fseek(f, 0, SEEK_END); long n = ftell(f); fseek(f, 0, SEEK_SET);
    uint8_t *b = malloc(n); if (fread(b, 1, n, f) != (size_t)n) return 1; fclose(f);
    fm1ntt_machine *m = fm1ntt_new();
    if (fm1ntt_load_package(m, b, n) != 0) { printf("%s\tnoload\t%s\n", argv[1], fm1ntt_error()); return 0; }
    const char *board = fm1ntt_board(m);
    fm1ntt_jit(m, 1); fm1ntt_pace(m, 0);
    static int16_t pcm[2048];
    double w0 = now_s();
    while (fm1ntt_ms(m) < 2000 && now_s() - w0 < 25 && !fm1ntt_halted(m)) fm1ntt_render(m, pcm, 1024, 4000000);
    if (fm1ntt_halted(m)) { printf("%s\t%s\thalted\n", argv[1], board); return 0; }
    double warm = now_s() - w0; uint64_t warm_ms = fm1ntt_ms(m);
    double res[4][3];   // [0] idle pace 0, [1] idle pace 3, [2] held pace 0, [3] held pace 3
    for (int k = 0; k < 4; k++) {
        if (k == 2) { static const int notes[6] = {0, 4, 7, 12, 16, 19}; for (int i = 0; i < 6; i++) fm1ntt_key(m, 14 + notes[i], 1); fm1ntt_pace(m, 0); for (int i = 0; i < 2; i++) { double t = now_s(); uint64_t s0 = fm1ntt_ms(m); while (fm1ntt_ms(m) - s0 < 500 && now_s() - t < 10) fm1ntt_render(m, pcm, 1024, 4000000); } }
        fm1ntt_pace(m, k & 1 ? 3 : 0);
        Fm1Cpu c; fm1ntt_cpu(m, 0, &c);
        uint64_t i0 = instr(), g0 = c.insns, ms0 = fm1ntt_ms(m); double t0 = now_s();
        while (fm1ntt_ms(m) - ms0 < 2000 && now_s() - t0 < 25 && !fm1ntt_halted(m)) fm1ntt_render(m, pcm, 1024, 4000000);
        double wall = now_s() - t0; uint64_t i1 = instr(); fm1ntt_cpu(m, 0, &c);
        double emu = (fm1ntt_ms(m) - ms0) / 1000.0; if (emu <= 0) emu = 1e-9;
        res[k][0] = (double)(c.insns - g0) / emu;
        res[k][1] = (double)(i1 - i0) / emu;
        res[k][2] = emu / wall;
    }
    printf("%s\t%s\tok\twarm %.1fs/%llums\tg0 %.1fM\th0 %.0fM\tmac0 %.2f\tg3 %.1fM\th3 %.0fM\tmac3 %.2f\tH0 %.0fM\tH3 %.0fM\thalted %d\n", argv[1], board, warm, (unsigned long long)warm_ms,
           res[0][0] / 1e6, res[0][1] / 1e6, res[0][2], res[1][0] / 1e6, res[1][1] / 1e6, res[1][2], res[2][1] / 1e6, res[3][1] / 1e6, fm1ntt_halted(m));
    return 0;
}
