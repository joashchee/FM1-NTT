/* FM1-NTT's Device report (r3i6nos CLAUDE.md rule 9, docs/DEVICE-REPORTS.md, rule 14's power section) and Benchmark report. MIT.
 * Included by fm1ntt.c after App and the helpers; uses the vendored r3i6nos_report.h for the grammar, the system and card sections
 * and the power windows.
 *
 * DEVICE REPORT  `fm1ntt --report [FIRMWARE]` or TOOLS > Device report -> $BASE/r36s-report.txt (STATUS key: value, sections, `end` last):
 *   what only the unit can answer: system, card, the core's speed against real time, the screen's paint, the sound callback, the files
 *   it writes, the power windows (idle, working), memory.
 * BENCHMARK     `fm1ntt --bench [--seconds N] [--fw FILE]` or TOOLS > Benchmark -> $BASE/bench/bench-<os>-<arch>-<stamp>.txt:
 *   fixed workloads, each repeated three times, one `BENCH id: median unit better=... min=... max=... n=...` line each, with the
 *   platform and the workload's version at the top, so reports from different machines compare with scripts/bench-compare.py.
 *   The workloads are synthetic (a loop of nops: core/src/bench.rs) and say how fast the core dispatches instructions, not how fast
 *   a real firmware runs; with --fw the same firmware is also run and timed, identified by its size and a hash. */

#include "vendor/r3i6nos_report.h"
#ifdef __APPLE__
#include <sys/sysctl.h>
#endif

#define BENCH_FORMAT 1
#define BENCH_RUNS 3

/* ---- progress for Settings > Device reports (r3i6nos's three files) --------------------------------------------------------- */
/* The same progress, in the process, for the window's progress box (a job runs on its own thread; the main thread reads this). */
static SDL_SpinLock g_prog_lock;
static int g_prog_step, g_prog_steps;
static char g_prog_doing[96];
static uint32_t g_prog_since, g_prog_step_since;
static void prog_get(int *step, int *steps, char *doing, size_t n, uint32_t *since, uint32_t *step_since) {
    SDL_AtomicLock(&g_prog_lock);
    *step = g_prog_step; *steps = g_prog_steps; *since = g_prog_since; *step_since = g_prog_step_since;
    snprintf(doing, n, "%s", g_prog_doing);
    SDL_AtomicUnlock(&g_prog_lock);
}
static void prog_reset(void) {
    SDL_AtomicLock(&g_prog_lock);
    g_prog_step = 0; g_prog_steps = 0; g_prog_doing[0] = 0; g_prog_since = g_prog_step_since = SDL_GetTicks();
    SDL_AtomicUnlock(&g_prog_lock);
}
static void report_progress(const char *doing, int step, int steps, const char *file) {
    SDL_AtomicLock(&g_prog_lock);
    g_prog_step = step; g_prog_steps = steps; g_prog_step_since = SDL_GetTicks();
    snprintf(g_prog_doing, sizeof g_prog_doing, "%s", doing);
    SDL_AtomicUnlock(&g_prog_lock);
    const char *p = getenv("R3I6NOS_REPORT_PROGRESS");
    if (!p || !*p) return;
    char body[512], tmp[400];
    snprintf(body, sizeof body, "step=%d\nsteps=%d\ndoing=%s\nfile=%s\n", step, steps, doing, file ? file : "");
    snprintf(tmp, sizeof tmp, "%s.tmp", p);
    FILE *f = fopen(tmp, "w");
    if (!f) return;
    fputs(body, f);
    fclose(f);
    rename(tmp, p);
}

/* ---- the platform, for both reports ------------------------------------------------------------------------------------------ */
static void cpu_model(char *out, size_t n) {
    out[0] = 0;
#ifdef __APPLE__
    size_t len = n;
    if (sysctlbyname("machdep.cpu.brand_string", out, &len, NULL, 0) != 0) out[0] = 0;
#endif
    if (!out[0]) {
        const char *keys[] = {"model name", "Hardware", "Model", "cpu model", "CPU part"};
        for (unsigned k = 0; k < sizeof keys / sizeof *keys && !out[0]; k++) {
            FILE *f = fopen("/proc/cpuinfo", "r");
            char line[256];
            while (f && fgets(line, sizeof line, f)) {
                if (!strncmp(line, keys[k], strlen(keys[k]))) {
                    char *c = strchr(line, ':');
                    if (c) { snprintf(out, n, "%s", c + 2); out[strcspn(out, "\n")] = 0; break; }
                }
            }
            if (f) fclose(f);
        }
    }
    if (!out[0]) snprintf(out, n, "unknown");
}
static long ram_mb(void) {
    long pages = sysconf(_SC_PHYS_PAGES), ps = sysconf(_SC_PAGESIZE);
    return pages > 0 && ps > 0 ? (long)((double)pages * (double)ps / 1048576.0) : 0;
}
/* A short id for the platform: "Darwin-arm64", "Linux-aarch64"; and "r36s" appended when the device tree says so. */
static void platform_id(char *out, size_t n) {
    struct utsname u;
    if (uname(&u) != 0) { snprintf(out, n, "unknown"); return; }
    snprintf(out, n, "%s-%s", u.sysname, u.machine);
    char m[128] = "";
    FILE *f = fopen("/proc/device-tree/model", "r");
    if (f) { if (fgets(m, sizeof m, f)) { /* the model string */ } fclose(f); }
    if (strcasestr(m, "r36s") || is_dir("/roms")) { size_t l = strlen(out); snprintf(out + l, n - l, "-r36s"); }
}

/* ---- the workloads ----------------------------------------------------------------------------------------------------------- */
typedef struct { double ips, xrt, wall_ms; uint64_t insns; } Speed;

/* The nop loop flat out for `seconds`: instructions a second and times the emulated clock. 0 when the machine will not boot. */
static int speed_nop(int jit, double seconds, Speed *out) {
    uint8_t img[0x400];
    size_t n = fm1ntt_bench_image(img, sizeof img);
    fm1ntt_machine *m = fm1ntt_new();
    if (!n || !m || fm1ntt_load_raw(m, img, n, FM1_ENTRY_FELUCCA) != 0) { fm1ntt_free(m); return 0; }
    fm1ntt_jit(m, jit);
    int16_t pcm[128];
    Fm1Cpu c;
    double t0 = r3_report_ms(), t = t0;
    fm1ntt_render(m, pcm, 64, 200000);           /* a warm-up (and the compile, with the JIT) is part of the timing: it is what a run costs */
    while (t - t0 < seconds * 1000.0) { fm1ntt_render(m, pcm, 64, 4000000); t = r3_report_ms(); }
    int ok = fm1ntt_cpu(m, 0, &c) && !fm1ntt_halted(m);
    out->wall_ms = t - t0;
    out->insns = ok ? c.insns : 0;
    out->ips = ok ? (double)c.insns / (out->wall_ms / 1000.0) : 0;
    out->xrt = out->ips / ((double)fm1ntt_clock_mhz() * 1e6);
    fm1ntt_free(m);
    return ok;
}

/* One line of a /sys file ("" when it is not there): the CPU's governor and clock, which decide how fast every number below is. */
static void sys_line(const char *path, char *out, size_t n) {
    out[0] = 0;
    FILE *f = fopen(path, "r");
    if (!f) return;
    if (fgets(out, (int)n, f)) out[strcspn(out, "\r\n")] = 0;
    fclose(f);
}
/* "governor performance, 1512 MHz now, 1512 MHz max (available: ...)" or "" when the kernel has no cpufreq */
static void cpufreq_text(char *out, size_t n) {
    char g[64], cur[32], mx[32], av[160];
    sys_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor", g, sizeof g);
    sys_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq", cur, sizeof cur);
    sys_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq", mx, sizeof mx);
    sys_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors", av, sizeof av);
    if (!g[0] && !cur[0]) { out[0] = 0; return; }
    snprintf(out, n, "governor %s, %.0f MHz now, %.0f MHz max (available: %s)", g[0] ? g : "?", atof(cur) / 1000.0, atof(mx) / 1000.0, av[0] ? av : "?");
}

/* A firmware the caller supplied, run flat out for `seconds` of real time: emulated time over real time. 0 when it halts at once. */
static int speed_firmware(const char *path, int raw, int jit, int pace, double seconds, double *xrt, double *emu_ms, int *halted) {
    char why[128];
    size_t n = 0;
    uint8_t *buf = read_file(path, &n, why, sizeof why);
    if (!buf) return 0;
    fm1ntt_machine *m = fm1ntt_new();
    int rc = raw ? fm1ntt_load_raw(m, buf, n, FM1_ENTRY_FELUCCA) : fm1ntt_load_package(m, buf, n);
    free(buf);
    if (rc != 0) { fm1ntt_free(m); return 0; }
    fm1ntt_jit(m, jit);
    fm1ntt_pace(m, pace);
    static int16_t pcm[1024 * 2];
    double t0 = r3_report_ms(), t = t0;
    uint64_t ms0 = fm1ntt_ms(m);
    while (t - t0 < seconds * 1000.0 && !fm1ntt_halted(m)) { fm1ntt_render(m, pcm, 1024, 4000000); t = r3_report_ms(); }
    *emu_ms = (double)(fm1ntt_ms(m) - ms0);
    *xrt = *emu_ms / (t - t0);
    *halted = fm1ntt_halted(m);
    fm1ntt_free(m);
    return 1;
}

/* The benchmark's firmware run: the firmware is first brought past its boot with the JIT (the first emulated seconds are cheap, then it gets heavier: a
 * 1 s run from a cold start measured the cheap part, 2026-11-17), then BENCH_RUNS consecutive windows of `seconds` are timed in the setting asked for.
 * The warm-up stops at FW_WARMUP_EMU_MS of emulated time or FW_WARMUP_WALL_S of real time, whichever is first (a machine with no JIT would take minutes);
 * `warm_ms` and `warm_s` say what was reached. 0 when the firmware did not boot. */
#define FW_WARMUP_EMU_MS 2000
#define FW_WARMUP_WALL_S 20.0
static int speed_firmware_windows(const char *path, int raw, int jit, int pace, double seconds, double xs[BENCH_RUNS], double *emu_ms, int *halted, double *warm_ms, double *warm_s) {
    char why[128];
    size_t n = 0;
    uint8_t *buf = read_file(path, &n, why, sizeof why);
    if (!buf) return 0;
    fm1ntt_machine *m = fm1ntt_new();
    int rc = raw ? fm1ntt_load_raw(m, buf, n, FM1_ENTRY_FELUCCA) : fm1ntt_load_package(m, buf, n);
    free(buf);
    if (rc != 0) { fm1ntt_free(m); return 0; }
    static int16_t pcm[1024 * 2];
    fm1ntt_jit(m, fm1ntt_jit_available());
    fm1ntt_pace(m, 0);
    double w0 = r3_report_ms(), t = w0;
    while (fm1ntt_ms(m) < FW_WARMUP_EMU_MS && t - w0 < FW_WARMUP_WALL_S * 1000.0 && !fm1ntt_halted(m)) { fm1ntt_render(m, pcm, 1024, 4000000); t = r3_report_ms(); }
    *warm_ms = (double)fm1ntt_ms(m);
    *warm_s = (t - w0) / 1000.0;
    fm1ntt_jit(m, jit);
    fm1ntt_pace(m, pace);
    double emu = 0;
    for (int i = 0; i < BENCH_RUNS && !fm1ntt_halted(m); i++) {
        double t0 = r3_report_ms();
        uint64_t ms0 = fm1ntt_ms(m);
        t = t0;
        while (t - t0 < seconds * 1000.0 && !fm1ntt_halted(m)) { fm1ntt_render(m, pcm, 1024, 4000000); t = r3_report_ms(); }
        emu = (double)(fm1ntt_ms(m) - ms0);
        xs[i] = emu / (t - t0);
    }
    *emu_ms = emu;
    *halted = fm1ntt_halted(m);
    fm1ntt_free(m);
    return 1;
}

static double median3(double v[BENCH_RUNS]) {
    double a = v[0], b = v[1], c = v[2];
    return a < b ? (b < c ? b : (a < c ? c : a)) : (a < c ? a : (b < c ? c : b));
}
static double min3(const double v[BENCH_RUNS]) { double m = v[0]; for (int i = 1; i < BENCH_RUNS; i++) if (v[i] < m) m = v[i]; return m; }
static double max3(const double v[BENCH_RUNS]) { double m = v[0]; for (int i = 1; i < BENCH_RUNS; i++) if (v[i] > m) m = v[i]; return m; }

/* ---- the benchmark report ---------------------------------------------------------------------------------------------------- */
typedef struct { FILE *f; char path[512], tmp[524]; int n; } Bench;

static void bench_line(Bench *b, const char *id, const double v[BENCH_RUNS], const char *unit, int higher_better) {
    char line[320];
    snprintf(line, sizeof line, "BENCH %s: %.4g %s better=%s min=%.4g max=%.4g n=%d\n", id, median3((double *)v), unit, higher_better ? "higher" : "lower", min3(v), max3(v), BENCH_RUNS);
    fputs(line, b->f);
    if (!g_quiet_logs) fputs(line, stdout);
    fflush(b->f);
    b->n++;
}
static void bench_note(Bench *b, const char *fmt, const char *a1) {
    char line[320];
    snprintf(line, sizeof line, fmt, a1 ? a1 : "");
    fputs(line, b->f);
    if (!g_quiet_logs) fputs(line, stdout);
    fflush(b->f);
    fflush(stdout);
}

static unsigned long long fnv64_file(const char *path, size_t *size) {
    char why[64];
    size_t n = 0;
    uint8_t *buf = read_file(path, &n, why, sizeof why);
    if (!buf) return 0;
    unsigned long long h = 1469598103934665603ull;
    for (size_t i = 0; i < n; i++) { h ^= buf[i]; h *= 1099511628211ull; }
    free(buf);
    *size = n;
    return h;
}

/* The whole benchmark. `seconds` is per workload per run (default 1.0 so the three runs of the speed tests take about 3 s each);
 * `fw` (may be NULL) is a firmware to time too. Writes $BASE/bench/bench-<platform>-<stamp>.txt (and latest.txt); returns 0 when written. */
static int bench_run(App *a, double seconds, const char *fw, int fw_raw, char *result, size_t rn) {
    const int steps = 12;
    char dir[300], plat[64], st[32], name[160];
    sub_path(dir, sizeof dir, "bench", NULL);
    if (mkdir_p(dir) != 0) { snprintf(result, rn, "Cannot make the bench folder"); return -1; }
    platform_id(plat, sizeof plat);
    stamp(st, sizeof st);
    snprintf(name, sizeof name, "bench-%s-%s.txt", plat, st);
    Bench b = {0};
    snprintf(b.path, sizeof b.path, "%s/%s", dir, name);
    snprintf(b.tmp, sizeof b.tmp, "%s.tmp", b.path);
    b.f = fopen(b.tmp, "w");
    if (!b.f) { snprintf(result, rn, "Cannot write %s", name); return -1; }
    char cpu[160], when[32], line[512];
    cpu_model(cpu, sizeof cpu);
    time_t now = time(NULL);
    strftime(when, sizeof when, "%Y-%m-%d %H:%M:%S", localtime(&now));
    struct utsname u;
    uname(&u);
    char maxmhz[32] = "";
    r3_report_field("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq", "", maxmhz, sizeof maxmhz);
    bench_note(&b, "# fm1ntt benchmark\nformat: %s\n", "1");
    snprintf(line, sizeof line, "workload: %u\napp: FM1-NTT %s (%s)\nmade: %s\nplatform: %s\nos: %s %s %s\ncpu: %s\ncores: %ld\nmax_khz: %s\nram_mb: %ld\nnative_jit: %s\nseconds_per_run: %.2f\nruns: %d\n",
             (unsigned)fm1ntt_bench_version(), fm1ntt_version(), FM1NTT_BUILD, when, plat, u.sysname, u.release, u.machine, cpu, sysconf(_SC_NPROCESSORS_ONLN), maxmhz[0] ? maxmhz : "-",
             ram_mb(), fm1ntt_jit_available() ? "available" : "no", seconds, BENCH_RUNS);
    fputs(line, b.f); if (!g_quiet_logs) fputs(line, stdout);
    bench_note(&b, "\n== results ==\n%s", "");
    int paused = a ? a->paused : 0;
    if (a) a->paused = 1;                                /* the app's own emulation would steal the core */
    double v[BENCH_RUNS];
    Speed sp;
    report_progress("core: the nop loop, interpreter", 1, steps, b.path);
    for (int i = 0; i < BENCH_RUNS; i++) { v[i] = speed_nop(0, seconds, &sp) ? sp.ips / 1e6 : 0; }
    bench_line(&b, "core.nop.interp", v, "Minsn/s", 1);
    double xv[BENCH_RUNS];
    for (int i = 0; i < BENCH_RUNS; i++) xv[i] = v[i] * 1e6 / ((double)fm1ntt_clock_mhz() * 1e6);
    bench_line(&b, "core.nop.interp.xrt", xv, "x-real-time", 1);
    if (fm1ntt_jit_available()) {
        report_progress("core: the nop loop, native JIT", 2, steps, b.path);
        bench_note(&b, "# trying the native JIT (the flag is set; the vendored core decides per block, and a result equal to the interpreter means it did not engage on this loop): if this is the last line, the process crashed in it\n%s", "");
        for (int i = 0; i < BENCH_RUNS; i++) { v[i] = speed_nop(1, seconds, &sp) ? sp.ips / 1e6 : 0; }
        bench_line(&b, "core.nop.jit", v, "Minsn/s", 1);
        for (int i = 0; i < BENCH_RUNS; i++) xv[i] = v[i] * 1e6 / ((double)fm1ntt_clock_mhz() * 1e6);
        bench_line(&b, "core.nop.jit.xrt", xv, "x-real-time", 1);
    } else bench_note(&b, "SKIP core.nop.jit: this CPU is not one the native JIT supports%s\n", "");

    /* boot and reset: a machine from a raw image, and back to power-on */
    report_progress("core: boot and reset", 3, steps, b.path);
    uint8_t img[0x400];
    size_t in = fm1ntt_bench_image(img, sizeof img);
    for (int i = 0; i < BENCH_RUNS; i++) {
        double t0 = r3_report_ms();
        for (int k = 0; k < 20; k++) { fm1ntt_machine *m = fm1ntt_new(); fm1ntt_load_raw(m, img, in, FM1_ENTRY_FELUCCA); fm1ntt_free(m); }
        v[i] = (r3_report_ms() - t0) / 20;
    }
    bench_line(&b, "core.boot", v, "ms", 0);
    fm1ntt_machine *m = fm1ntt_new();
    fm1ntt_load_raw(m, img, in, FM1_ENTRY_FELUCCA);
    for (int i = 0; i < BENCH_RUNS; i++) {
        double t0 = r3_report_ms();
        for (int k = 0; k < 20; k++) fm1ntt_reset(m);
        v[i] = (r3_report_ms() - t0) / 20;
    }
    bench_line(&b, "core.reset", v, "ms", 0);

    /* memory views: 256-byte reads of the flash, the panel's own access */
    report_progress("memory reads", 4, steps, b.path);
    uint8_t page[256];
    for (int i = 0; i < BENCH_RUNS; i++) {
        double t0 = r3_report_ms(), bytes = 0;
        while (r3_report_ms() - t0 < seconds * 250) for (int k = 0; k < 2000; k++) bytes += (double)fm1ntt_read_mem(m, FM1_REGION_FLASH, (size_t)(k * 256) % (1 << 20), page, sizeof page);
        v[i] = bytes / 1048576.0 / ((r3_report_ms() - t0) / 1000.0);
    }
    bench_line(&b, "mem.read", v, "MiB/s", 1);

    /* the screen: every panel drawn into the canvas, both layouts */
    report_progress("screen: drawing every panel", 5, steps, b.path);
    {
        static UiState u;
        static uint32_t canvas[PX_W * PX_H];
        memset(&u, 0, sizeof u);
        u.fb_valid = 1; u.run_state = "RUN"; u.fw_name = "BENCH.BIN"; u.fw_board = "Fm1"; u.msg = "BENCH";
        for (int i = 0; i < FM_W * FM_H; i++) u.fb[i] = (uint32_t)(i * 2654435761u) & 0xFFFFFF;
        u.ncpu = 2; u.nfiles = 1; snprintf(u.files[0], PX_NAME, "  [ QUIT ]"); u.mem_n = PX_MEM_BYTES; u.ntools = 8; u.console_n = 0;
        for (int mode = 0; mode < 2; mode++) {
            for (int i = 0; i < BENCH_RUNS; i++) {
                double t0 = r3_report_ms();
                int frames = 0;
                while (r3_report_ms() - t0 < seconds * 250 || frames < PANEL_COUNT) { u.panel = frames % PANEL_COUNT; px_draw(canvas, mode, &u); frames++; }
                v[i] = (r3_report_ms() - t0) / frames;
            }
            bench_line(&b, mode ? "screen.draw.zoomed" : "screen.draw.standard", v, "ms/frame", 0);
        }
    }

    /* files: the flash slot, written the way the app writes it, and read back */
    report_progress("files: a flash slot", 6, steps, b.path);
    {
        size_t n = fm1ntt_flash_size();
        uint8_t *buf = malloc(n);
        char p[400];
        snprintf(p, sizeof p, "%s/bench-slot.tmp", dir);
        double w[BENCH_RUNS], r[BENCH_RUNS];
        for (size_t i = 0; buf && i < n; i++) buf[i] = (uint8_t)(i * 31 + (i >> 8));
        for (int i = 0; buf && i < BENCH_RUNS; i++) {
            double t0 = r3_report_ms();
            int ok = write_atomic(p, buf, n) == 0;
            w[i] = ok ? r3_report_ms() - t0 : 0;
            size_t got = 0; char why[64];
            t0 = r3_report_ms();
            uint8_t *rd = read_file(p, &got, why, sizeof why);
            r[i] = rd && got == n ? r3_report_ms() - t0 : 0;
            free(rd);
        }
        remove(p);
        free(buf);
        bench_line(&b, "io.flash.write", w, "ms", 0);
        bench_line(&b, "io.flash.read", r, "ms", 0);
    }
    fm1ntt_free(m);

    /* a firmware the caller supplied, if any */
    if (fw && *fw) {
        size_t sz = 0;
        unsigned long long h = fnv64_file(fw, &sz);
        const char *base = strrchr(fw, '/');
        char id[96];
        snprintf(id, sizeof id, "# firmware: %s %zu bytes fnv64=%016llx\n", base ? base + 1 : fw, sz, h);
        bench_note(&b, "%s", id);
        char cf[300];
        cpufreq_text(cf, sizeof cf);
        if (cf[0]) bench_note(&b, "# cpu0 before the firmware runs: %s\n", cf);
        /* (jit, pace): the interpreter, the native JIT, and the JIT with the main loop paced at 1, 2 and 3 (what the app's PACE does) */
        static const int var[5][2] = {{0, 0}, {1, 0}, {1, 1}, {1, 2}, {1, 3}};
        for (int v = 0; v < 5; v++) {
            int jit = var[v][0], pace = var[v][1];
            if (jit && !fm1ntt_jit_available()) break;
            report_progress(!jit ? "the firmware, interpreter" : pace ? "the firmware, native JIT, paced" : "the firmware, native JIT", 7 + v, steps, b.path);
            double xs[BENCH_RUNS], emu = 0, warm_ms = 0, warm_s = 0;
            int halted = 0, ok = speed_firmware_windows(fw, fw_raw, jit, pace, seconds, xs, &emu, &halted, &warm_ms, &warm_s);
            if (ok) { char wl[160]; snprintf(wl, sizeof wl, "# fw warm-up: %.0f emulated ms in %.1f s, then %d windows of %.2f s\n", warm_ms, warm_s, BENCH_RUNS, seconds); bench_note(&b, "%s", wl); }
            char key[160];
            if (pace) snprintf(key, sizeof key, "fw.%016llx.jit.pace%d.xrt", h, pace);
            else snprintf(key, sizeof key, "fw.%016llx.%s.xrt", h, jit ? "jit" : "interp");
            if (ok && !halted) bench_line(&b, key, xs, "x-real-time", 1);
            else bench_note(&b, ok ? "SKIP fw: the firmware halted before the run ended%s\n" : "SKIP fw: it did not boot (see the app's log)%s\n", "");
        }
    } else bench_note(&b, "SKIP fw: no firmware given (--fw FILE): only the synthetic workloads were run%s\n", "");
    report_progress("done", steps, steps, b.path);
    fputs("end\n", b.f);
    if (!g_quiet_logs) fputs("end\n", stdout);
    int bad = fflush(b.f) != 0;
    bad |= fsync(fileno(b.f)) != 0;
    bad |= fclose(b.f) != 0;
    if (a) a->paused = paused;
    if (bad || rename(b.tmp, b.path) != 0) { remove(b.tmp); snprintf(result, rn, "Writing %s failed", name); return -1; }
    char latest[400];
    snprintf(latest, sizeof latest, "%s/latest.txt", dir);
    size_t ln = 0; char why[64];
    uint8_t *copy = read_file(b.path, &ln, why, sizeof why);
    if (copy) { write_atomic(latest, copy, ln); free(copy); }
    snprintf(result, rn, "Benchmark saved: bench/%s (%d results)", name, b.n);
    return 0;
}

/* ---- the device report ------------------------------------------------------------------------------------------------------- */
static int report_sound_check(App *a, R3Report *r) {
    if (!a || !a->dev) { r3_report_line(r, R3_SKIP, "sound callback", "no sound device was opened in this run (a headless run, or none on this machine)"); return 0; }
    int c0 = SDL_AtomicGet(&a->callbacks);
    double t0 = r3_report_ms();
    SDL_Delay(1000);
    int n = SDL_AtomicGet(&a->callbacks) - c0;
    double secs = (r3_report_ms() - t0) / 1000.0;
    int want = (int)(FM1_SAMPLE_RATE * secs / AUDIO_FRAMES);
    r3_report_line(r, n >= want / 2 && n > 0 ? R3_PASS : R3_FAIL, "sound callback", "ran %d times in %.1f s (about %d expected): %s", n, secs, want,
                   n > 0 ? "the sound system is asking for sound" : "the sound system never asked for sound");
    return n;
}

/* One report. `fw` (may be NULL) is a firmware for the working window; the app's own loaded one is used when it has one. */
static int device_report(App *a, const char *how, const char *fw, int fw_raw, char *result, size_t rn) {
    const int steps = 10;
    char base[300];
    base_dir(base, sizeof base);
    R3Report r;
    memset(&r, 0, sizeof r);
    r.quiet = g_quiet_logs;
    if (r3_report_open(&r, base, "FM1-NTT", fm1ntt_version(), "r36s-fm1ntt", how) != 0) { snprintf(result, rn, "Cannot write the report in %s: %s", base, strerror(errno)); return -1; }
    r3_report_raw(&r, "# NEEDS: for the draw in mW: run on battery (unplug the charger)\n# NEEDS: for the working window: a firmware loaded, or given as --report FILE\n");
    report_progress("system", 1, steps, r.path);
    r3_report_system(&r);
    { char cf[300]; cpufreq_text(cf, sizeof cf); r3_report_line(&r, R3_INFO, "cpu0 clock", "%s", cf[0] ? cf : "the kernel shows no cpufreq"); }
    {   /* where the firmware the app lists comes from: a unit that shows none of one kind says why here */
        char bd[300], pd[300], cwd[300] = "?";
        bundled_dir(bd, sizeof bd);
        pack_dir(pd, sizeof pd);
        if (!getcwd(cwd, sizeof cwd)) snprintf(cwd, sizeof cwd, "?");
        char *bp = SDL_GetBasePath();
        FileEnt *fe = calloc(PX_MAX_FILES, sizeof *fe);
        int nb = fe && bd[0] ? add_dir(fe, 0, bd, 1) : 0, np = fe && pd[0] ? add_dir(fe, 0, pd, 2) : 0;
        free(fe);
        r3_report_line(&r, R3_INFO, "program folder", "%s (working folder %s)", bp ? bp : "?", cwd);
        if (nb) r3_report_line(&r, R3_PASS, "bundled firmware", "%d firmware files in %s", nb, bd);
        else r3_report_line(&r, R3_FAIL, "bundled firmware", "none found: looked at ../share/fm1ntt/firmware, ../bundled-firmware and bundled-firmware beside the program, in /usr/share/fm1ntt/firmware and the working folder, and $FM1NTT_BUNDLED%s", getenv("FM1NTT_BUNDLED") ? " (set)" : " (not set)");
        if (np) r3_report_line(&r, R3_INFO, "firmware pack", "%d firmware files in %s", np, pd);
        else r3_report_line(&r, R3_INFO, "firmware pack", "not found (optional)");
        if (bp) SDL_free(bp);
    }
    report_progress("card", 2, steps, r.path);
    r3_report_card(&r, base);
    int paused = a ? a->paused : 0;
    if (a) a->paused = 1;                                  /* the speed tests have the processor to themselves */

    r3_report_section(&r, "core");
    r3_report_line(&r, R3_INFO, "version", "FM1-NTT %s, build %s", fm1ntt_version(), FM1NTT_BUILD);
    r3_report_line(&r, R3_INFO, "workload", "benchmark workload %u; the emulated clock is %u MHz (one instruction a cycle)", (unsigned)fm1ntt_bench_version(), (unsigned)fm1ntt_clock_mhz());
    r3_report_line(&r, R3_INFO, "native jit", "%s", fm1ntt_jit_available() ? "this CPU is supported by the vendored core's block compiler" : "not supported on this CPU (the interpreter only)");
    report_progress("core: boot, reset", 3, steps, r.path);
    {
        uint8_t img[0x400];
        size_t n = fm1ntt_bench_image(img, sizeof img);
        double t0 = r3_report_ms();
        fm1ntt_machine *m = fm1ntt_new();
        int rc = fm1ntt_load_raw(m, img, n, FM1_ENTRY_FELUCCA);
        double boot = r3_report_ms() - t0;
        r3_report_line(&r, rc == 0 && boot < 500 ? R3_PASS : R3_FAIL, "boot a machine", "%.2f ms (goal 500 ms)", boot);
        t0 = r3_report_ms();
        fm1ntt_reset(m);
        double rs = r3_report_ms() - t0;
        r3_report_line(&r, rs < 500 ? R3_PASS : R3_FAIL, "reset", "%.2f ms (goal 500 ms)", rs);
        fm1ntt_free(m);
    }
    r3_report_section(&r, "speed");
    report_progress("speed: the nop loop", 4, steps, r.path);
    {
        double secs = r3_report_power_seconds(3) > 3 ? 3 : r3_report_power_seconds(3);
        Speed sp;
        /* the nop loop is a ceiling for dispatch speed, not a goal: a real firmware idles most of the time (the core skips idle) and is also slower
         * per instruction, so a gate here would pass or fail for the wrong reason. The goal is the firmware's own run, below. */
        if (speed_nop(0, secs, &sp)) r3_report_line(&r, R3_INFO, "nop loop, interpreter", "%.1f Minsn/s = %.2fx real time at the emulated %u MHz (a synthetic ceiling, no goal: compare with other platforms' benchmark reports)", sp.ips / 1e6, sp.xrt, (unsigned)fm1ntt_clock_mhz());
        else r3_report_line(&r, R3_FAIL, "nop loop, interpreter", "the workload did not run");
        if (fm1ntt_jit_available()) {
            r3_report_line(&r, R3_INFO, "native jit", "trying it: if the report ends here, the process crashed in the compiler (the .tmp file is left)");
            fflush(r.f);
            if (speed_nop(1, secs, &sp)) r3_report_line(&r, R3_INFO, "nop loop, native jit", "%.1f Minsn/s = %.2fx real time (the flag is on; the vendored core decides per block, and equal to the interpreter means it did not engage on this loop)", sp.ips / 1e6, sp.xrt);
            else r3_report_line(&r, R3_FAIL, "nop loop, native jit", "the workload did not run");
        } else r3_report_line(&r, R3_SKIP, "nop loop, native jit", "this CPU is not one the native JIT supports");
        if (fw && *fw) {
            double xrt = 0, emu = 0;
            int halted = 0;
            const char *fb = strrchr(fw, '/');
            double best = 0;
            int best_pace = 0, best_jit = 0;
            char cf[300];
            /* (jit, pace): the interpreter, the JIT, and the JIT with the main loop paced 1..3 (the app's PACE). Only the first two are gates; the goal
             * is that SOME setting keeps up, which the "firmware, best setting" line below judges. */
            static const int var[5][2] = {{0, 0}, {1, 0}, {1, 1}, {1, 2}, {1, 3}};
            for (int v = 0; v < 5; v++) {
                int jit = var[v][0], pace = var[v][1];
                char what[48];
                snprintf(what, sizeof what, !jit ? "firmware, interpreter" : pace ? "firmware, native jit, pace %d" : "firmware, native jit", pace);
                if (jit && !fm1ntt_jit_available()) { r3_report_line(&r, R3_SKIP, what, "this CPU is not one the native JIT supports"); break; }
                if (jit && !pace) { r3_report_line(&r, R3_INFO, "native jit", "trying it on %s: if the report ends here, the process crashed in the compiler", fb ? fb + 1 : fw); fflush(r.f); }
                int got = speed_firmware(fw, fw_raw, jit, pace, secs, &xrt, &emu, &halted);
                if (jit) { cpufreq_text(cf, sizeof cf); if (cf[0]) r3_report_line(&r, R3_INFO, "cpu0 clock", "just after %s: %s", what, cf); }
                if (!got) { r3_report_line(&r, R3_FAIL, what, "%s did not boot", fb ? fb + 1 : fw); continue; }
                if (!halted && xrt > best) { best = xrt; best_pace = pace; best_jit = jit; }
                if (pace) r3_report_line(&r, halted ? R3_FAIL : R3_INFO, what, "%s: %.2fx real time over %.0f emulated ms%s (the main loop paced: sound level, pitch and key-to-sound time held on a Mac; not judged by ear or on the unit)", fb ? fb + 1 : fw, xrt, emu, halted ? "; it HALTED" : "");
                else r3_report_line(&r, halted ? R3_FAIL : (xrt >= 1.0 ? R3_PASS : R3_FAIL), what, "%s: %.2fx real time over %.0f emulated ms%s (goal 1x: the unit must keep up with the firmware's audio)", fb ? fb + 1 : fw, xrt, emu, halted ? "; it HALTED" : "");
            }
            r3_report_line(&r, best >= 1.0 ? R3_PASS : R3_FAIL, "firmware, best setting", "%s: %.2fx real time with %s%s (goal 1x: live play)", fb ? fb + 1 : fw, best, best_jit ? "the native jit" : "the interpreter", best_pace ? (best_pace == 1 ? ", pace 1" : best_pace == 2 ? ", pace 2" : ", pace 3") : "");
        } else r3_report_line(&r, R3_SKIP, "firmware", "no firmware: give one (--report FILE) for its speed against real time");
    }
    r3_report_section(&r, "screen");
    report_progress("screen: paint", 5, steps, r.path);
    {
        static UiState u;
        static uint32_t canvas[PX_W * PX_H];
        memset(&u, 0, sizeof u);
        u.fb_valid = 1; u.run_state = "RUN"; u.ncpu = 1; u.nfiles = 1; u.ntools = 8;
        for (int mode = 0; mode < 2; mode++) {
            double t0 = r3_report_ms();
            for (int i = 0; i < PANEL_COUNT * 3; i++) { u.panel = i % PANEL_COUNT; px_draw(canvas, mode, &u); }
            double per = (r3_report_ms() - t0) / (PANEL_COUNT * 3);
            r3_report_line(&r, per < 10 ? R3_PASS : R3_FAIL, mode ? "paint, zoomed" : "paint, standard", "%.3f ms for a whole 640 x 480 screen (goal 10 ms)", per);
        }
    }
    r3_report_section(&r, "sound");
    report_progress("sound callback", 6, steps, r.path);
    if (a) a->paused = paused;                             /* the callback has to be running to be counted */
    report_sound_check(a, &r);
    if (a) a->paused = 1;
    r3_report_section(&r, "files");
    report_progress("files", 7, steps, r.path);
    {
        size_t n = fm1ntt_flash_size();
        uint8_t *buf = malloc(n);
        char p[400];
        snprintf(p, sizeof p, "%s/report-slot.tmp", base);
        if (buf) {
            for (size_t i = 0; i < n; i++) buf[i] = (uint8_t)(i * 7);
            double t0 = r3_report_ms();
            int ok = write_atomic(p, buf, n) == 0;
            double w = r3_report_ms() - t0;
            r3_report_line(&r, ok && w < 1000 ? R3_PASS : R3_FAIL, "save a flash slot", "1 MiB written, synced and renamed into place in %.0f ms (goal 1000 ms)", w);
            remove(p);
        }
        free(buf);
    }
    if (a) a->paused = paused;

    report_progress("power", 8, steps, r.path);
    r3_report_power_section(&r);
    R3PowerResult idle, work;
    memset(&work, 0, sizeof work);
    r3_report_power_idle(&r, r3_report_power_seconds(30), NULL, NULL, &idle);        /* the app open and waiting: its own main loop blocks on input */
    if (a && fm1ntt_loaded(a->m) && !a->paused) {
        R3Power p;
        r3_power_begin(&p);
        double secs = r3_report_power_seconds(20), t0 = r3_report_ms();
        while (r3_report_ms() - t0 < secs * 1000) { SDL_Delay(100); r3_power_sample(&p); }
        r3_report_power_end(&r, &p, "working", 95.0, 400.0, &work);                 /* first guesses: one core's worth, set before any measurement */
    } else r3_report_line(&r, R3_SKIP, "working", "no firmware is running (or the emulation is paused): load one and run the report again to measure it");
    r3_report_power_advice(&r, &idle, &work);
    r3_report_section(&r, "memory");
    report_progress("memory", 9, steps, r.path);
    r3_report_memory(&r);
    report_progress("done", steps, steps, r.path);
    int rc = r3_report_close(&r);
    snprintf(result, rn, rc == 0 ? "Report written: %s (PASS %d FAIL %d)" : "Writing the report failed (%s)", rc == 0 ? r.path : r.tmp, r.pass, r.fail);
    return rc;
}

/* ---- running one in the background, from the TOOLS panel --------------------------------------------------------------------- */
static int job_thread(void *p) {
    App *a = p;
    char how[64];
    int rc;
    if (a->job_kind == JOB_REPORT) {
        snprintf(how, sizeof how, "TOOLS > Device report");
        char dflt[300];
        default_firmware(dflt, sizeof dflt);
        const char *fw = a->fw_path[0] ? a->fw_path : (dflt[0] ? dflt : NULL);
        rc = device_report(a, how, fw, a->fw_path[0] ? a->fw_raw : (fw && has_ext(fw, ".bin")), a->job_result, sizeof a->job_result);
    } else {
        char dflt[300];
        default_firmware(dflt, sizeof dflt);
        const char *fw = a->fw_path[0] ? a->fw_path : (dflt[0] ? dflt : NULL);
        rc = bench_run(a, a->job_seconds, fw, a->fw_path[0] ? a->fw_raw : (fw && has_ext(fw, ".bin")), a->job_result, sizeof a->job_result);
    }
    a->job_style = rc == 0 ? 1 : 3;
    SDL_AtomicSet(&a->job_done, 1);
    return rc;
}
static void job_start(App *a, int kind) {
    if (a->job) { say(a, 2, "Wait for the running job to finish", NULL); return; }
    a->job_kind = kind;
    a->job_seconds = 1.0;
    a->job_result[0] = 0;
    SDL_AtomicSet(&a->job_done, 0);
    prog_reset();
    report_progress("starting", 0, kind == JOB_REPORT ? 10 : 12, NULL);
    a->job = SDL_CreateThread(job_thread, "fm1ntt-job", a);
    if (!a->job) { job_thread(a); SDL_AtomicSet(&a->job_done, 0); a->job_kind = JOB_NONE; say(a, a->job_style, "%s", a->job_result); return; }
    say(a, 0, kind == JOB_REPORT ? "Device report running (about a minute): the emulation is paused while it measures" : "Benchmark running (about a minute and a half)", NULL);
}
/* Called by the main loop each pass: reaps a finished job and says how it went. */
static int job_poll(App *a) {                  /* 1 when a job just finished: the caller must redraw (the box goes away) */
    if (a->job && SDL_AtomicGet(&a->job_done)) {
        SDL_WaitThread(a->job, NULL);
        a->job = NULL;
        a->job_kind = JOB_NONE;
        say(a, a->job_style, "%s", a->job_result);
        return 1;
    }
    return 0;
}
