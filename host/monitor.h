/* FM1-NTT host: the MONITOR panel's numbers, a `top` for the machine the app runs on (not the emulated FM-1). MIT. docs/MONITOR.md.
 *
 * No SDL and no drawing here: the parsers are plain functions of strings (the self-test feeds them fixtures, truncated, huge and garbage ones), the percent
 * maths is a function of two counters, and the sampler reads /proc and /sys on Linux (FM1NTT_MON_PROC and FM1NTT_MON_SYS name other roots, for tests), the
 * Mach and libproc calls on macOS, and nothing elsewhere (every box says n/a). pix.h draws a MonView; fm1ntt.c fills the app's own part of it.
 *
 * THE COST RULE (the maintainer, 2026-11-18): while the panel is not showing, mon_tick returns at once and reads nothing, no file, no syscall for a
 * number, no timer (g_mon_reads counts every open, and the self-test checks it does not move). While it is showing: one sample a second, on the
 * main thread's own loop, never a busy wait. The process list is read only on the pages that show it. Every read is bounded: fixed buffers, a cap on
 * the processes parsed (MON_PROCS) and on the threads and cores kept, a missing or odd file is "n/a", never a failure (rule 5).
 * Nothing here has run on an R36S: every figure is "untested on the unit" until a report says otherwise. */
#ifndef FM1NTT_MONITOR_H
#define FM1NTT_MONITOR_H

#include <ctype.h>
#include <dirent.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/resource.h>
#include <sys/statvfs.h>
#include <time.h>
#include <unistd.h>
#include "threads.h"
#ifdef __APPLE__
#include <ifaddrs.h>
#include <libproc.h>
#include <mach/mach.h>
#include <mach/mach_time.h>
#include <net/if.h>
#include <pthread.h>
#include <CoreFoundation/CoreFoundation.h>
#include <IOKit/ps/IOPowerSources.h>
#include <IOKit/ps/IOPSKeys.h>
#include <sys/sysctl.h>
#endif

enum { MON_CORES = 16, MON_SHOW_CORES = 8, MON_HIST = 24, MON_PROCS = 192, MON_TOP = 64, MON_NAME = 16, MON_THR = 10, MON_NA = -1 };
enum { MON_PLAT_NONE = 0, MON_PLAT_LINUX = 1, MON_PLAT_MAC = 2 };
enum { MON_SORT_CPU, MON_SORT_MEM, MON_SORT_PID, MON_SORTS };
enum { LV_OK, LV_WARN, LV_CRIT };
enum { MON_ROWS_STD = 22, MON_ROWS_NARROW = 34 };      /* process rows on a page, Standard and Zoomed */

/* ---- the colour-coding thresholds, in one table: warn and crit are inclusive; low_is_bad reads a value that is bad when it is LOW ---------------- */
typedef struct { const char *what; int warn, crit, low_is_bad; } MonThresh;
enum { TH_CPU, TH_TEMP, TH_MEM, TH_BATT, TH_CARD, TH_FRAME, TH_SPEED, TH_COUNT };
static const MonThresh MON_TH[TH_COUNT] = {
    {"CPU %", 70, 90, 0},         /* a core's load */
    {"TEMP C", 65, 80, 0},        /* the RK3326 throttles near 85 */
    {"MEM USED %", 75, 90, 0},
    {"BATTERY %", 30, 15, 1},
    {"CARD FREE %", 15, 5, 1},
    {"FRAME MS", 11, 25, 0},      /* the goal is a frame in 10 ms: 11 and over is amber */
    {"SPEED %", 96, 49, 1},       /* emulated time over real time, in percent: under 0.97x is amber (the pace's own line), under 0.5x red */
};
static inline int mon_level(int which, int v) {
    if (v < 0 || which < 0 || which >= TH_COUNT) return LV_OK;      /* unknown: no verdict (drawn as a dash) */
    const MonThresh *t = &MON_TH[which];
    if (t->low_is_bad) return v <= t->crit ? LV_CRIT : v <= t->warn ? LV_WARN : LV_OK;
    return v >= t->crit ? LV_CRIT : v >= t->warn ? LV_WARN : LV_OK;
}
/* Never colour alone: a glyph and the number say the same. */
static inline char mon_glyph(int level) { return level == LV_CRIT ? '!' : level == LV_WARN ? '*' : ' '; }

/* ---- the data ------------------------------------------------------------------------------------------------------------------------------ */
typedef struct { unsigned long long total, idle; int ok; } MonCpuRaw;
typedef struct { int has; unsigned long long total_kb, used_kb, cached_kb, free_kb, swap_total_kb, swap_used_kb; } MonMem;
typedef struct { int pid; char name[MON_NAME]; unsigned long long ticks; long rss_kb; int cpu_x10; } MonProc;     /* cpu_x10: tenths of a percent of one core */
typedef struct { char name[24]; int cpu_x10, core; } MonThread;
typedef struct { int has, pct, mw; char status[16]; } MonBat;                                                       /* pct, mw: -1 when not reported */

/* What the app itself knows (the host fills it: fm1ntt.c). */
typedef struct {
    int loaded, jit_pref, jit_avail;                                  /* a firmware is loaded; the JIT setting (TOOLS) and whether this build can run it */
    int jit_blocks, jit_given_up;                                      /* blocks compiled, blocks given up on (-1: no machine) */
    char root[100], root_why[40];                                      /* the app's folder and which rule chose it */
    char pad[2][28];                                                   /* the first two pads' names, from SDL */
    int speed_pct, jit, cb_per_s, late_cb, frame_ms, frame_max_ms;      /* speed in percent of real time (-1: no firmware running) */
    char job[40];                                                      /* "" when none */
    int job_pct;                                                       /* -1 unknown */
    char fw[28];
    int njoy, emu_kb;                                                  /* pads seen; the emulated machine's buffers */
    char midi[28];
    char hot[3][44];                                                   /* the hot-plug history, newest first */
    int reduced_motion;
} MonApp;

typedef struct {
    int platform, sampled;                                             /* sampled: at least one delta exists */
    int ncores, core_pct[MON_CORES], core_mhz[MON_CORES], cpu_pct;
    uint8_t hist[MON_CORES][MON_HIST];
    int hist_n;
    char gov[16];
    int temp_c;
    int has_load; double load[3];
    MonMem mem;
    long self_rss_kb;
    int has_card, card_free_pct; unsigned long long card_free_mb;
    int nproc_total, proc_capped, nprocs, procs_read;                  /* procs_read: the list was read at least once */
    MonProc procs[MON_TOP];
    int nthr; MonThread thr[MON_THR];
    MonBat bat;
    int wake_ps, app_cpu_x10;                                          /* context switches a second; this app's own CPU, tenths of a percent of a core */
    int nusb; char usb[3][28];                                         /* USB products seen (Linux /sys/bus/usb: the first three that are not hubs) */
    char net[24];                                                      /* "WLAN0 UP", "NONE", or "" when unknown here */
    int sample_us, share_x100;                                         /* the cost of the last sample, and its share of a core in hundredths of a percent */
    int sort, self_pid;
    MonApp app;
} MonView;

/* ---- reads, counted ----------------------------------------------------------------------------------------------------------------------- */
static unsigned long g_mon_reads;                                      /* every file or folder opened here: the cost rule's witness */
static const char *mon_root(const char *env, const char *def) { const char *v = getenv(env); return v && *v ? v : def; }
static int mon_path(char *out, size_t cap, const char *root, const char *rel) { int n = snprintf(out, cap, "%s/%s", root, rel); return n < 0 || (size_t)n >= cap ? -1 : 0; }
/* The first cap-1 bytes of a file, NUL ended; its length, or -1 when it cannot be read. */
static int mon_slurp(const char *path, char *buf, size_t cap) {
    g_mon_reads++;
    FILE *f = fopen(path, "r");
    if (!f) { if (cap) buf[0] = 0; return -1; }
    size_t n = fread(buf, 1, cap - 1, f);
    fclose(f);
    buf[n] = 0;
    return (int)n;
}
static double mon_now_ms(void) { struct timespec ts; clock_gettime(CLOCK_MONOTONIC, &ts); return (double)ts.tv_sec * 1000.0 + (double)ts.tv_nsec / 1e6; }

/* ---- the parsers: strings in, numbers out, no read beyond the NUL ---------------------------------------------------------------------------- */
#define MON_CAP_BIG (1ULL << 40)
static int mon_nums(const char *p, unsigned long long *v, int n) {      /* up to n unsigned numbers; how many, each capped so a sum cannot wrap */
    int k = 0;
    while (k < n) {
        while (*p == ' ' || *p == '\t') p++;
        if (!isdigit((unsigned char)*p)) break;
        char *end;
        unsigned long long x = strtoull(p, &end, 10);
        v[k++] = x > (1ULL << 60) ? (1ULL << 60) : x;
        p = end;
    }
    return k;
}

/* /proc/stat: the "cpu " line into *all and each "cpuN " line into cores[N] (N below max). The number of cores seen (highest N + 1). */
static int mon_parse_stat(const char *text, MonCpuRaw *all, MonCpuRaw *cores, int max) {
    int n = 0;
    memset(all, 0, sizeof *all);
    for (int i = 0; i < max; i++) memset(&cores[i], 0, sizeof cores[i]);
    for (const char *line = text; line && *line; ) {
        const char *next = strchr(line, '\n');
        if (!strncmp(line, "cpu", 3)) {
            const char *p = line + 3;
            int idx = -1;
            if (isdigit((unsigned char)*p)) { long id = strtol(p, (char **)&p, 10); idx = id >= 0 && id < max ? (int)id : -2; }
            else if (*p != ' ') { line = next ? next + 1 : NULL; continue; }
            unsigned long long v[8] = {0};
            int k = mon_nums(p, v, 8);
            if (k >= 4 && idx != -2) {
                MonCpuRaw r = {0, v[3] + (k > 4 ? v[4] : 0), 1};
                for (int i = 0; i < k; i++) r.total += v[i];
                if (idx == -1) *all = r; else { cores[idx] = r; if (idx + 1 > n) n = idx + 1; }
            }
        }
        line = next ? next + 1 : NULL;
    }
    return n;
}

/* The percent of one core in use between two counter pairs, 0..100; -1 when the counters went backwards, did not move, or do not add up. */
static int mon_pct(const MonCpuRaw *prev, const MonCpuRaw *cur) {
    if (!prev->ok || !cur->ok || cur->total <= prev->total || cur->idle < prev->idle) return MON_NA;
    double dt = (double)(cur->total - prev->total), di = (double)(cur->idle - prev->idle);
    if (di > dt) return MON_NA;
    double p = (dt - di) * 100.0 / dt;
    return p < 0 ? 0 : p > 100 ? 100 : (int)(p + 0.5);
}

static unsigned long long mon_kb_of(const char *p) { unsigned long long v[1]; return mon_nums(p, v, 1) ? (v[0] > MON_CAP_BIG ? MON_CAP_BIG : v[0]) : 0; }
/* /proc/meminfo. has is 1 when MemTotal was there; used is total less what is available. */
static void mon_parse_meminfo(const char *text, MonMem *m) {
    memset(m, 0, sizeof *m);
    unsigned long long avail = 0, buffers = 0;
    int have_avail = 0;
    for (const char *line = text; line && *line; ) {
        const char *next = strchr(line, '\n');
        if (!strncmp(line, "MemTotal:", 9)) m->total_kb = mon_kb_of(line + 9);
        else if (!strncmp(line, "MemFree:", 8)) m->free_kb = mon_kb_of(line + 8);
        else if (!strncmp(line, "MemAvailable:", 13)) { avail = mon_kb_of(line + 13); have_avail = 1; }
        else if (!strncmp(line, "Buffers:", 8)) buffers = mon_kb_of(line + 8);
        else if (!strncmp(line, "Cached:", 7)) m->cached_kb = mon_kb_of(line + 7);
        else if (!strncmp(line, "SwapTotal:", 10)) m->swap_total_kb = mon_kb_of(line + 10);
        else if (!strncmp(line, "SwapFree:", 9)) m->swap_used_kb = mon_kb_of(line + 9);     /* the free figure for now: turned into used below */
        line = next ? next + 1 : NULL;
    }
    unsigned long long swap_free = m->swap_used_kb;
    m->swap_used_kb = m->swap_total_kb > swap_free ? m->swap_total_kb - swap_free : 0;
    if (!have_avail) avail = m->free_kb + buffers + m->cached_kb;
    if (avail > m->total_kb) avail = m->total_kb;
    m->used_kb = m->total_kb - avail;
    m->has = m->total_kb > 0;
}

/* /proc/loadavg: "0.52 0.40 0.31 ...". 1 when the three numbers are sane. */
static int mon_parse_loadavg(const char *text, double *out) {
    char *e;
    const char *p = text;
    for (int i = 0; i < 3; i++) {
        while (*p == ' ') p++;
        double v = strtod(p, &e);
        if (e == p || !(v >= 0) || v > 1e5) return 0;
        out[i] = v;
        p = e;
    }
    return 1;
}

/* /proc/<pid>/stat: pid, the name between the first "(" and the LAST ")", utime + stime in ticks, resident pages. The fields are counted after the ")". 1 when parsed. */
static int mon_parse_pstat(const char *s, MonProc *p, long page_kb) {
    const char *lp = strchr(s, '('), *rp = strrchr(s, ')');
    if (!lp || !rp || rp < lp) return 0;
    memset(p, 0, sizeof *p);
    p->pid = atoi(s);
    size_t n = (size_t)(rp - lp - 1);
    if (n >= sizeof p->name) n = sizeof p->name - 1;
    for (size_t i = 0; i < n; i++) p->name[i] = lp[1 + i] >= 32 && lp[1 + i] < 127 ? lp[1 + i] : '?';
    unsigned long long ut = 0, st = 0, rss = 0;
    int have = 0;
    const char *q = rp + 1;
    for (int field = 3; field <= 24; field++) {               /* 3 state, 14 utime, 15 stime, 24 rss in pages */
        while (*q == ' ') q++;
        if (!*q) break;
        char *end;
        long long v = strtoll(q, &end, 10);
        if (field == 14) { ut = v < 0 ? 0 : (unsigned long long)v; have |= 1; }
        else if (field == 15) { st = v < 0 ? 0 : (unsigned long long)v; have |= 2; }
        else if (field == 24) { rss = v < 0 ? 0 : (unsigned long long)v; have |= 4; }
        while (*end && *end != ' ') end++;
        q = end;
    }
    if ((have & 3) != 3) return 0;
    p->ticks = (ut > (1ULL << 50) ? (1ULL << 50) : ut) + (st > (1ULL << 50) ? (1ULL << 50) : st);
    p->rss_kb = (have & 4) && rss < (1ULL << 40) ? (long)(rss * (unsigned long long)page_kb) : 0;
    return 1;
}

/* A temperature in thousandths of a degree ("54000"): whole degrees, or MON_NA when it is not a number or is not believable (-50..200). */
static int mon_parse_milli(const char *text) {
    char *e;
    long long v = strtoll(text, &e, 10);
    if (e == text) return MON_NA;
    v /= 1000;
    return v < -50 || v > 200 ? MON_NA : (int)v;
}

/* The battery from the text of its files. Anything not a number is -1; the draw is |current| x voltage (microamps and microvolts) in mW. */
static void mon_battery_from(const char *capacity, const char *status, const char *cur, const char *volt, MonBat *b) {
    memset(b, 0, sizeof *b);
    b->pct = b->mw = MON_NA;
    char *e;
    long long c = strtoll(capacity, &e, 10);
    if (e != capacity && c >= 0 && c <= 100) b->pct = (int)c;
    size_t n = 0;
    for (; status[n] && status[n] != '\n' && n < sizeof b->status - 1; n++) b->status[n] = status[n] >= 32 && status[n] < 127 ? status[n] : '?';
    b->status[n] = 0;
    long long i = strtoll(cur, &e, 10);
    int ok_i = e != cur;
    long long v = strtoll(volt, &e, 10);
    if (ok_i && e != volt && v > 0 && v < 100000000LL) {
        double mw = (double)(i < 0 ? -i : i) * (double)v / 1e9;
        b->mw = mw > 99999.0 ? 99999 : (int)(mw + 0.5);
    }
    b->has = 1;
}

/* ---- Linux readers (any system with the roots, so the tests run on a Mac) -------------------------------------------------------------------- */
static int mon_proc_wanted(void) {
#ifdef __linux__
    return 1;
#else
    return getenv("FM1NTT_MON_PROC") != NULL;
#endif
}

static int mon_read_battery(MonBat *b) {
    char base[300], p[420], dir[420] = "", cap[48], st[48], cur[48], vol[48], t[48];
    if (mon_path(base, sizeof base, mon_root("FM1NTT_MON_SYS", "/sys"), "class/power_supply") != 0) return 0;
    g_mon_reads++;
    DIR *d = opendir(base);
    if (!d) return 0;
    struct dirent *e;
    for (int seen = 0; (e = readdir(d)) && seen < 16; ) {
        if (e->d_name[0] == '.') continue;
        seen++;
        snprintf(p, sizeof p, "%s/%s/type", base, e->d_name);
        if (mon_slurp(p, t, sizeof t) > 0 && !strncasecmp(t, "battery", 7)) { snprintf(dir, sizeof dir, "%s/%s", base, e->d_name); break; }
    }
    closedir(d);
    if (!dir[0]) return 0;
    cap[0] = st[0] = cur[0] = vol[0] = 0;
    snprintf(p, sizeof p, "%s/capacity", dir); mon_slurp(p, cap, sizeof cap);
    snprintf(p, sizeof p, "%s/status", dir); mon_slurp(p, st, sizeof st);
    snprintf(p, sizeof p, "%s/current_now", dir); mon_slurp(p, cur, sizeof cur);
    snprintf(p, sizeof p, "%s/voltage_now", dir); mon_slurp(p, vol, sizeof vol);
    mon_battery_from(cap, st, cur, vol, b);
    return 1;
}

/* The hottest of the first eight thermal zones, whole degrees, or MON_NA. */
static int mon_read_temp(void) {
    int best = MON_NA;
    char base[300], p[360], t[32];
    if (mon_path(base, sizeof base, mon_root("FM1NTT_MON_SYS", "/sys"), "class/thermal") != 0) return MON_NA;
    for (int z = 0; z < 8; z++) {
        snprintf(p, sizeof p, "%s/thermal_zone%d/temp", base, z);
        if (mon_slurp(p, t, sizeof t) < 0) continue;
        int c = mon_parse_milli(t);
        if (c > best) best = c;
    }
    return best;
}

static int mon_read_net(char *out, size_t cap) {
    char base[300], p[420], t[32];
    if (mon_path(base, sizeof base, mon_root("FM1NTT_MON_SYS", "/sys"), "class/net") != 0) return 0;
    g_mon_reads++;
    DIR *d = opendir(base);
    if (!d) return 0;
    struct dirent *e;
    snprintf(out, cap, "NONE");
    for (int seen = 0; (e = readdir(d)) && seen < 16; ) {
        if (e->d_name[0] == '.' || !strcmp(e->d_name, "lo")) continue;
        seen++;
        snprintf(p, sizeof p, "%s/%s/operstate", base, e->d_name);
        if (mon_slurp(p, t, sizeof t) > 0 && !strncmp(t, "up", 2)) {
            char nm[12];
            size_t k = 0;
            for (; e->d_name[k] && k < sizeof nm - 1; k++) nm[k] = (char)toupper((unsigned char)e->d_name[k]);
            nm[k] = 0;
            snprintf(out, cap, "%s UP", nm);
            break;
        }
    }
    closedir(d);
    return 1;
}

/* ---- the sampler ---------------------------------------------------------------------------------------------------------------------------- */
typedef struct {
    int on;                                   /* the panel is showing (set by the host each pass) */
    int paused;                               /* the person froze the view (Y) */
    int force;                                /* a page or sort key asks for a sample now (but not twice in 250 ms) */
    int plat;
    double last_ms;                           /* when the last sample was taken; <0: none since the panel opened */
    int have_prev;
    MonCpuRaw prev[MON_CORES], cur[MON_CORES], all_prev;
    ThrSnap thr_last;
    int pnp[MON_PROCS]; unsigned long long pnt[MON_PROCS]; int pn_prev;       /* the last process list's ticks, to take a delta */
    double pn_ms;                                                            /* when it was read: the list may be read less often than the rest */
    unsigned long long ctx_prev; double ctx_ms; double self_cpu_prev, self_cpu_ms;
    char card_path[300];
    MonView v;
} Mon;

static void mon_init(Mon *m, const char *card_path) {
    memset(m, 0, sizeof *m);
    m->last_ms = -1;
    m->v.temp_c = MON_NA; m->v.wake_ps = MON_NA; m->v.app_cpu_x10 = MON_NA; m->v.cpu_pct = MON_NA;
    m->v.platform = MON_PLAT_NONE;
    if (card_path) snprintf(m->card_path, sizeof m->card_path, "%s", card_path);
}
/* The panel went away or came back: forget the old counters, so a long gap is not averaged into the first reading. */
static void mon_show(Mon *m, int showing) {
    if (showing == m->on) return;
    m->on = showing;
    if (!showing) { m->force = 0; m->last_ms = -1; m->have_prev = 0; m->pn_prev = 0; m->thr_last.n = 0; m->self_cpu_ms = m->ctx_ms = 0; m->v.sampled = 0; m->v.procs_read = 0; }
}

static int mon_before(const MonProc *a, const MonProc *b, int sort) {      /* a sorts before b */
    if (sort == MON_SORT_MEM) return a->rss_kb != b->rss_kb ? a->rss_kb > b->rss_kb : a->pid < b->pid;
    if (sort == MON_SORT_PID) return a->pid < b->pid;
    return a->cpu_x10 != b->cpu_x10 ? a->cpu_x10 > b->cpu_x10 : a->rss_kb != b->rss_kb ? a->rss_kb > b->rss_kb : a->pid < b->pid;
}
/* Keeps the first MON_TOP of `all` (n of them) in sort order, in out. An insertion into a bounded array: no qsort, no allocation. */
static int mon_top(const MonProc *all, int n, MonProc *out, int sort) {
    int k = 0;
    for (int i = 0; i < n; i++) {
        int at = k;
        while (at > 0 && mon_before(&all[i], &out[at - 1], sort)) at--;
        if (at >= MON_TOP) continue;
        int end = k < MON_TOP ? k : MON_TOP - 1;
        memmove(out + at + 1, out + at, (size_t)(end - at) * sizeof out[0]);
        out[at] = all[i];
        if (k < MON_TOP) k++;
    }
    return k;
}

static long mon_page_kb(void) { long p = sysconf(_SC_PAGESIZE); return p > 0 ? p / 1024 : 4; }

#ifdef __APPLE__
static int mon_mac_cpu(MonCpuRaw *cores, int max) {
    natural_t n = 0;
    processor_info_array_t info = NULL;
    mach_msg_type_number_t cnt = 0;
    if (host_processor_info(mach_host_self(), PROCESSOR_CPU_LOAD_INFO, &n, &info, &cnt) != KERN_SUCCESS || !info) return 0;
    int got = (int)n < max ? (int)n : max;
    for (int i = 0; i < got; i++) {
        unsigned long long u = (unsigned)info[i * CPU_STATE_MAX + CPU_STATE_USER], s = (unsigned)info[i * CPU_STATE_MAX + CPU_STATE_SYSTEM],
                           id = (unsigned)info[i * CPU_STATE_MAX + CPU_STATE_IDLE], ni = (unsigned)info[i * CPU_STATE_MAX + CPU_STATE_NICE];
        cores[i] = (MonCpuRaw){u + s + id + ni, id, 1};
    }
    vm_deallocate(mach_task_self(), (vm_address_t)info, cnt * sizeof(integer_t));
    return got;
}
static void mon_mac_mem(MonMem *m) {
    memset(m, 0, sizeof *m);
    uint64_t total = 0;
    size_t sz = sizeof total;
    if (sysctlbyname("hw.memsize", &total, &sz, NULL, 0) != 0 || !total) return;
    vm_statistics64_data_t vm;
    mach_msg_type_number_t cnt = HOST_VM_INFO64_COUNT;
    vm_size_t ps = 0;
    host_page_size(mach_host_self(), &ps);
    if (host_statistics64(mach_host_self(), HOST_VM_INFO64, (host_info64_t)&vm, &cnt) != KERN_SUCCESS || !ps) return;
    m->total_kb = total / 1024;
    m->free_kb = (unsigned long long)vm.free_count * ps / 1024;
    m->cached_kb = (unsigned long long)vm.external_page_count * ps / 1024;
    unsigned long long avail = (unsigned long long)(vm.free_count + vm.inactive_count) * ps / 1024;
    m->used_kb = m->total_kb > avail ? m->total_kb - avail : 0;
    struct xsw_usage sw;
    size_t sl = sizeof sw;
    if (sysctlbyname("vm.swapusage", &sw, &sl, NULL, 0) == 0) { m->swap_total_kb = sw.xsu_total / 1024; m->swap_used_kb = sw.xsu_used / 1024; }
    m->has = 1;
}
static long mon_mac_self_rss(void) {
    mach_task_basic_info_data_t ti;
    mach_msg_type_number_t cnt = MACH_TASK_BASIC_INFO_COUNT;
    if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t)&ti, &cnt) != KERN_SUCCESS) return 0;
    return (long)(ti.resident_size / 1024);
}
/* Every process libproc will tell about (a sandbox or a permission may hide some: those are not listed). ticks are hundredths of a second of CPU time. */
static int mon_mac_procs(MonProc *out, int max, int *total) {
    static pid_t pids[2048];
    int bytes = proc_listpids(PROC_ALL_PIDS, 0, pids, (int)sizeof pids), n = bytes > 0 ? bytes / (int)sizeof(pid_t) : 0, k = 0;
    mach_timebase_info_data_t tb;
    mach_timebase_info(&tb);
    *total = 0;
    for (int i = 0; i < n; i++) {
        if (pids[i] <= 0) continue;
        (*total)++;
        if (k >= max) continue;
        struct proc_taskinfo ti;
        if (proc_pidinfo(pids[i], PROC_PIDTASKINFO, 0, &ti, sizeof ti) != (int)sizeof ti) continue;
        MonProc *p = &out[k];
        memset(p, 0, sizeof *p);
        p->pid = pids[i];
        char nm[64] = "";
        proc_name(pids[i], nm, sizeof nm);
        snprintf(p->name, sizeof p->name, "%s", nm[0] ? nm : "?");
        unsigned long long mach = ti.pti_total_user + ti.pti_total_system, ns = tb.denom ? mach / tb.denom * tb.numer : mach;
        p->ticks = ns / 10000000ULL;
        p->rss_kb = (long)(ti.pti_resident_size / 1024);
        k++;
    }
    return k;
}
static int mon_mac_threads(MonThread *out, int max) {
    thread_act_array_t th;
    mach_msg_type_number_t n = 0;
    if (task_threads(mach_task_self(), &th, &n) != KERN_SUCCESS) return 0;
    int k = 0;
    for (mach_msg_type_number_t i = 0; i < n; i++) {
        thread_basic_info_data_t bi;
        mach_msg_type_number_t c = THREAD_BASIC_INFO_COUNT;
        if (k < max && thread_info(th[i], THREAD_BASIC_INFO, (thread_info_t)&bi, &c) == KERN_SUCCESS) {
            MonThread *t = &out[k];
            memset(t, 0, sizeof *t);
            t->cpu_x10 = bi.cpu_usage * 1000 / TH_USAGE_SCALE;
            t->core = -1;
            pthread_t pt = pthread_from_mach_thread_np(th[i]);
            char nm[64] = "";
            if (pt) pthread_getname_np(pt, nm, sizeof nm);
            snprintf(t->name, sizeof t->name, "%s", nm[0] ? nm : (i == 0 ? "main" : "thread"));
            k++;
        }
        mach_port_deallocate(mach_task_self(), th[i]);
    }
    vm_deallocate(mach_task_self(), (vm_address_t)th, n * sizeof(thread_t));
    return k;
}
static int mon_mac_net(char *out, size_t cap) {
    struct ifaddrs *ifa = NULL;
    if (getifaddrs(&ifa) != 0) return 0;
    snprintf(out, cap, "NONE");
    for (struct ifaddrs *i = ifa; i; i = i->ifa_next)
        if (i->ifa_addr && i->ifa_addr->sa_family == AF_INET && (i->ifa_flags & IFF_UP) && (i->ifa_flags & IFF_RUNNING) && !(i->ifa_flags & IFF_LOOPBACK)) {
            char nm[12];
            size_t k = 0;
            for (; i->ifa_name[k] && k < sizeof nm - 1; k++) nm[k] = (char)toupper((unsigned char)i->ifa_name[k]);
            nm[k] = 0;
            snprintf(out, cap, "%s UP", nm);
            break;
        }
    freeifaddrs(ifa);
    return 1;
}
#endif

/* "blocks_compiled":N and "blocks_given_up":M from fm1ntt_jit_report's one line of JSON; 1 when both were found. */
static int mon_parse_jit(const char *json, int *compiled, int *given_up) {
    const char *a = strstr(json, "\"blocks_compiled\":"), *b = strstr(json, "\"blocks_given_up\":");
    if (!a || !b) return 0;
    long x = strtol(a + 18, NULL, 10), y = strtol(b + 18, NULL, 10);
    *compiled = x < 0 ? 0 : x > 99999999 ? 99999999 : (int)x;
    *given_up = y < 0 ? 0 : y > 99999999 ? 99999999 : (int)y;
    return 1;
}

/* USB products from bus/usb/devices (each folder's product file): the first `max` whose name is not a hub. How many were stored. */
static int mon_read_usb(char (*out)[28], int max) {
    char base[300], p[420], t[64];
    if (mon_path(base, sizeof base, mon_root("FM1NTT_MON_SYS", "/sys"), "bus/usb/devices") != 0) return 0;
    g_mon_reads++;
    DIR *d = opendir(base);
    if (!d) return 0;
    struct dirent *e;
    int n = 0;
    for (int seen = 0; n < max && (e = readdir(d)) && seen < 64; ) {
        if (e->d_name[0] == '.') continue;
        seen++;
        snprintf(p, sizeof p, "%s/%.100s/product", base, e->d_name);
        if (mon_slurp(p, t, sizeof t) <= 0) continue;
        size_t k = 0;
        for (; t[k] && t[k] != '\n' && k < 27; k++) out[n][k] = t[k] >= 32 && t[k] < 127 ? t[k] : '?';
        out[n][k] = 0;
        if (!k || strstr(out[n], "Hub") || strstr(out[n], "hub") || strstr(out[n], "HCI") || strstr(out[n], "Host Controller")) continue;
        n++;
    }
    closedir(d);
    return n;
}

#ifdef __APPLE__
/* The first power source that is a battery: level and state (no draw: IOKit's public keys do not carry it). */
static int mon_mac_battery(MonBat *b) {
    memset(b, 0, sizeof *b);
    b->pct = b->mw = MON_NA;
    CFTypeRef info = IOPSCopyPowerSourcesInfo();
    if (!info) return 0;
    CFArrayRef list = IOPSCopyPowerSourcesList(info);
    int found = 0;
    for (CFIndex i = 0; list && i < CFArrayGetCount(list) && !found; i++) {
        CFDictionaryRef ps = IOPSGetPowerSourceDescription(info, CFArrayGetValueAtIndex(list, i));
        if (!ps) continue;
        CFStringRef type = CFDictionaryGetValue(ps, CFSTR(kIOPSTypeKey));
        if (!type || CFStringCompare(type, CFSTR(kIOPSInternalBatteryType), 0) != kCFCompareEqualTo) continue;
        int cur = 0, max = 0;
        CFNumberRef c = CFDictionaryGetValue(ps, CFSTR(kIOPSCurrentCapacityKey)), m = CFDictionaryGetValue(ps, CFSTR(kIOPSMaxCapacityKey));
        if (c) CFNumberGetValue(c, kCFNumberIntType, &cur);
        if (m) CFNumberGetValue(m, kCFNumberIntType, &max);
        if (max > 0 && cur >= 0 && cur <= max) b->pct = cur * 100 / max;
        CFBooleanRef ch = CFDictionaryGetValue(ps, CFSTR(kIOPSIsChargingKey));
        CFStringRef st = CFDictionaryGetValue(ps, CFSTR(kIOPSPowerSourceStateKey));
        snprintf(b->status, sizeof b->status, "%s", ch && CFBooleanGetValue(ch) ? "Charging" : st && CFStringCompare(st, CFSTR(kIOPSBatteryPowerValue), 0) == kCFCompareEqualTo ? "Discharging" : "AC power");
        b->has = found = 1;
    }
    if (list) CFRelease(list);
    CFRelease(info);
    return found;
}
#endif

static double mon_self_cpu_s(void) {
    struct rusage ru;
    if (getrusage(RUSAGE_SELF, &ru) != 0) return -1;
    return (double)ru.ru_utime.tv_sec + (double)ru.ru_utime.tv_usec / 1e6 + (double)ru.ru_stime.tv_sec + (double)ru.ru_stime.tv_usec / 1e6;
}
static long long mon_self_ctx(void) {
    struct rusage ru;
    if (getrusage(RUSAGE_SELF, &ru) != 0) return -1;
    return (long long)ru.ru_nvcsw + (long long)ru.ru_nivcsw;
}

/* One pass over the host. `want_procs`: the page showing lists processes (it is the costly read, so it is only done then). */
static void mon_sample(Mon *m, double now_ms, int want_procs) {
    MonView *v = &m->v;
    double t0 = mon_now_ms();
    int proc_root = mon_proc_wanted();
    v->platform = proc_root ? MON_PLAT_LINUX : MON_PLAT_NONE;
    v->self_pid = (int)getpid();
    int ncores = 0;
    MonCpuRaw all = {0, 0, 0};
    memset(m->cur, 0, sizeof m->cur);
    char buf[16384];
    char pr[300], p[360];
    snprintf(pr, sizeof pr, "%s", mon_root("FM1NTT_MON_PROC", "/proc"));
    long page_kb = mon_page_kb();
    if (proc_root) {
        v->has_load = 0; v->self_rss_kb = 0;                         /* a file that has gone is n/a, not the last value it had */
        if (mon_path(p, sizeof p, pr, "stat") == 0 && mon_slurp(p, buf, sizeof buf) > 0) ncores = mon_parse_stat(buf, &all, m->cur, MON_CORES);
        if (mon_path(p, sizeof p, pr, "loadavg") == 0 && mon_slurp(p, buf, 256) > 0) v->has_load = mon_parse_loadavg(buf, v->load);
        if (mon_path(p, sizeof p, pr, "meminfo") == 0 && mon_slurp(p, buf, sizeof buf) > 0) mon_parse_meminfo(buf, &v->mem);
        else v->mem.has = 0;
        if (mon_path(p, sizeof p, pr, "self/statm") == 0 && mon_slurp(p, buf, 128) > 0) { unsigned long long a = 0, r = 0; char *e; a = strtoull(buf, &e, 10); r = strtoull(e, NULL, 10); (void)a; v->self_rss_kb = r < MON_CAP_BIG ? (long)(r * (unsigned long long)page_kb) : 0; }
        /* the clocks and the temperature: cpufreq per core, the governor of core 0, the hottest thermal zone */
        char sys[300];
        snprintf(sys, sizeof sys, "%s", mon_root("FM1NTT_MON_SYS", "/sys"));
        for (int i = 0; i < ncores; i++) {
            char rel[96], t[32];
            snprintf(rel, sizeof rel, "devices/system/cpu/cpu%d/cpufreq/scaling_cur_freq", i);
            v->core_mhz[i] = MON_NA;
            if (mon_path(p, sizeof p, sys, rel) == 0 && mon_slurp(p, t, sizeof t) > 0) { long long khz = strtoll(t, NULL, 10); if (khz > 0 && khz < 100000000LL) v->core_mhz[i] = (int)(khz / 1000); }
        }
        v->gov[0] = 0;
        if (mon_path(p, sizeof p, sys, "devices/system/cpu/cpu0/cpufreq/scaling_governor") == 0 && mon_slurp(p, buf, 32) > 0) {
            size_t k = 0;
            for (; buf[k] && buf[k] != '\n' && k < sizeof v->gov - 1; k++) v->gov[k] = (char)toupper((unsigned char)(buf[k] >= 32 && buf[k] < 127 ? buf[k] : '?'));
            v->gov[k] = 0;
        }
        v->temp_c = mon_read_temp();
        if (!mon_read_battery(&v->bat)) memset(&v->bat, 0, sizeof v->bat);
        if (!mon_read_net(v->net, sizeof v->net)) v->net[0] = 0;
        v->nusb = mon_read_usb(v->usb, 3);
        v->nthr = 0;
        ThrSnap ts;
        if (thr_snapshot(&ts, now_ms) > 0) {
            ThrSnap *last_p = &m->thr_last;
#define last (*last_p)
            long hz = sysconf(_SC_CLK_TCK);
            for (int i = 0; i < ts.n && v->nthr < MON_THR; i++) {
                unsigned long long before = 0;
                for (int j = 0; j < last.n; j++) if (last.t[j].tid == ts.t[i].tid) { before = last.t[j].ticks; break; }
                double secs = (ts.wall_ms - last.wall_ms) / 1000.0;
                MonThread t;
                memset(&t, 0, sizeof t);
                snprintf(t.name, sizeof t.name, "%s", ts.t[i].tid == (int)getpid() ? "main" : ts.t[i].name);
                t.core = ts.t[i].cpu;
                t.cpu_x10 = last.n && secs > 0 && hz > 0 && ts.t[i].ticks >= before ? (int)(1000.0 * ((double)(ts.t[i].ticks - before) / (double)hz) / secs + 0.5) : MON_NA;
                int at = v->nthr;
                while (at > 0 && t.cpu_x10 > v->thr[at - 1].cpu_x10) { v->thr[at] = v->thr[at - 1]; at--; }
                v->thr[at] = t;
                v->nthr++;
            }
            last = ts;
#undef last
        }
    }
#ifdef __APPLE__
    else {
        v->platform = MON_PLAT_MAC;
        ncores = mon_mac_cpu(m->cur, MON_CORES);
        for (int i = 0; i < ncores; i++) all.total += m->cur[i].total, all.idle += m->cur[i].idle;
        all.ok = ncores > 0;
        mon_mac_mem(&v->mem);
        v->has_load = getloadavg(v->load, 3) == 3;
        v->self_rss_kb = mon_mac_self_rss();
        v->nthr = mon_mac_threads(v->thr, MON_THR);
        for (int i = 1; i < v->nthr; i++) { MonThread t = v->thr[i]; int j = i; while (j > 0 && t.cpu_x10 > v->thr[j - 1].cpu_x10) { v->thr[j] = v->thr[j - 1]; j--; } v->thr[j] = t; }
        if (!mon_mac_net(v->net, sizeof v->net)) v->net[0] = 0;
        v->temp_c = MON_NA;
        v->gov[0] = 0;
        for (int i = 0; i < MON_CORES; i++) v->core_mhz[i] = MON_NA;
        if (!mon_mac_battery(&v->bat)) memset(&v->bat, 0, sizeof v->bat);
        v->nusb = 0;
    }
#endif
    if (!proc_root) {
#ifndef __APPLE__
        v->nthr = 0; v->mem.has = 0; v->has_load = getloadavg(v->load, 3) == 3; v->net[0] = 0;
#endif
    }
    /* the per-core percentages from two samples */
    v->ncores = ncores;
    if (m->have_prev) {
        for (int i = 0; i < ncores && i < MON_CORES; i++) {
            v->core_pct[i] = mon_pct(&m->prev[i], &m->cur[i]);
            if (v->hist_n < MON_HIST && i == 0) v->hist_n++;
            memmove(v->hist[i], v->hist[i] + 1, MON_HIST - 1);
            v->hist[i][MON_HIST - 1] = (uint8_t)(v->core_pct[i] < 0 ? 0 : v->core_pct[i]);
        }
        MonCpuRaw s0 = m->all_prev, s1 = all;
        if (!proc_root) { s0 = (MonCpuRaw){0, 0, 1}; s1 = (MonCpuRaw){0, 0, 1}; for (int i = 0; i < ncores; i++) { s0.total += m->prev[i].total; s0.idle += m->prev[i].idle; s1.total += m->cur[i].total; s1.idle += m->cur[i].idle; } }
        v->cpu_pct = mon_pct(&s0, &s1);
        v->sampled = 1;
    } else for (int i = 0; i < MON_CORES; i++) v->core_pct[i] = MON_NA;
    m->all_prev = all;
    memcpy(m->prev, m->cur, sizeof m->prev);
    m->have_prev = ncores > 0;
    /* this app's own CPU and its context switches: portable (getrusage) */
    double cpu = mon_self_cpu_s();
    v->app_cpu_x10 = m->self_cpu_ms > 0 && cpu >= m->self_cpu_prev && now_ms > m->self_cpu_ms ? (int)(1000.0 * (cpu - m->self_cpu_prev) / ((now_ms - m->self_cpu_ms) / 1000.0) + 0.5) : MON_NA;
    m->self_cpu_prev = cpu; m->self_cpu_ms = now_ms;
    long long ctx = mon_self_ctx();
    v->wake_ps = m->ctx_ms > 0 && ctx >= (long long)m->ctx_prev && now_ms > m->ctx_ms ? (int)((double)(ctx - (long long)m->ctx_prev) / ((now_ms - m->ctx_ms) / 1000.0) + 0.5) : MON_NA;
    m->ctx_prev = (unsigned long long)(ctx < 0 ? 0 : ctx); m->ctx_ms = now_ms;
    /* the card: free space where the app writes */
    struct statvfs sv;
    v->has_card = 0;
    if (m->card_path[0] && statvfs(m->card_path, &sv) == 0 && sv.f_blocks > 0) {
        double blocks = (double)sv.f_bavail, total = (double)sv.f_blocks, bs = (double)sv.f_frsize;
        v->card_free_pct = (int)(blocks * 100.0 / total + 0.5);
        v->card_free_mb = (unsigned long long)(blocks * bs / 1048576.0);
        v->has_card = 1;
    }
    /* the process list: only when a page shows it, and from a baseline of its own (the first read has no delta, shown as a dash) */
    if (want_procs) {
        static MonProc all_p[MON_PROCS];
        int n = 0, total = 0;
        if (proc_root) {
            DIR *d = NULL;
            if (mon_path(p, sizeof p, pr, "") == 0) { g_mon_reads++; d = opendir(pr); }
            struct dirent *e;
            while (d && (e = readdir(d))) {
                if (e->d_name[0] < '1' || e->d_name[0] > '9') continue;
                total++;
                if (n >= MON_PROCS) continue;
                char rel[300], t[1024];
                snprintf(rel, sizeof rel, "%.20s/stat", e->d_name);
                if (mon_path(p, sizeof p, pr, rel) != 0 || mon_slurp(p, t, sizeof t) <= 0) continue;
                if (mon_parse_pstat(t, &all_p[n], page_kb)) n++;
            }
            if (d) closedir(d);
        }
#ifdef __APPLE__
        else n = mon_mac_procs(all_p, MON_PROCS, &total);
#endif
        double secs = (now_ms - m->pn_ms) / 1000.0;
        long hz = proc_root ? sysconf(_SC_CLK_TCK) : 100;
        for (int i = 0; i < n; i++) {
            all_p[i].cpu_x10 = MON_NA;
            if (m->pn_prev && secs > 0 && hz > 0)
                for (int j = 0; j < m->pn_prev; j++)
                    if (m->pnp[j] == all_p[i].pid) { if (all_p[i].ticks >= m->pnt[j]) all_p[i].cpu_x10 = (int)(1000.0 * ((double)(all_p[i].ticks - m->pnt[j]) / (double)hz) / secs + 0.5); break; }
        }
        m->pn_prev = n;
        for (int i = 0; i < n; i++) { m->pnp[i] = all_p[i].pid; m->pnt[i] = all_p[i].ticks; }
        m->pn_ms = now_ms;
        for (int i = 0; i < n; i++) if (all_p[i].cpu_x10 < 0) all_p[i].cpu_x10 = 0;          /* sorted as idle until there is a delta; drawn as a dash */
        v->nprocs = mon_top(all_p, n, v->procs, v->sort);
        v->nproc_total = total;
        v->proc_capped = n >= MON_PROCS && total > n;                    /* the cap cut the list (a garbage or vanished entry is not a cap) */
        v->procs_read = 1;
    }
    double t1 = mon_now_ms();
    v->sample_us = (int)((t1 - t0) * 1000.0);
    m->last_ms = now_ms;
}

/* Once a pass of the main loop. Reads nothing and returns 0 unless the panel is showing, not paused, and a second has passed. Returns 1 when what is on
 * screen changed (the redraw is then asked for; an unchanged sample asks for nothing). */
static int mon_tick(Mon *m, int showing, double now_ms, int want_procs) {
    mon_show(m, showing);
    if (!showing || m->paused) return 0;
    int forced = m->force && (m->last_ms < 0 || now_ms - m->last_ms >= 250.0);
    if (!forced && m->last_ms >= 0 && now_ms - m->last_ms < 1000.0) return 0;
    m->force = 0;
    MonView before = m->v;
    mon_sample(m, now_ms, want_procs);
    m->v.share_x100 = (int)((double)m->v.sample_us / 1e6 * 10000.0 + 0.5);          /* a sample a second: its microseconds are the share, in hundredths of a percent */
    return memcmp(&before, &m->v, sizeof before) != 0;
}

/* The number of pages the panel has: Standard has one overview page and the process pages; Zoomed has two box pages first. */
static int mon_pages(int narrow, int nprocs) {
    int rows = narrow ? MON_ROWS_NARROW : MON_ROWS_STD, pp = nprocs > 0 ? (nprocs + rows - 1) / rows : 1;
    return (narrow ? 2 : 1) + pp;
}
/* Does this page list processes? (the costly read). */
static int mon_page_has_procs(int narrow, int page) { return page >= (narrow ? 2 : 1); }

/* ---- the data rain: real data on its way through, as hex columns -------------------------------------------------------------------------------
 * A ring of 256 bytes fed from what the app already has (the audio output, the MIDI bytes, the LCD's pixels, a page of the emulated RAM); the picture is a
 * function of the ring and a step counter, so the same bytes and the same step draw the same pixels. It steps at most 3 times a second (nothing changes over
 * the same area faster: no flashing), and not at all under reduced motion. */
enum { RAIN_COLS = 32, RAIN_ROWS = 40, RAIN_SRC = 4, RAIN_STEP_MS = 334 };
typedef struct { uint8_t row[RAIN_ROWS][RAIN_COLS]; char tag[RAIN_ROWS]; unsigned step; double last_ms; } MonRain;
static const char RAIN_TAG[RAIN_SRC + 1] = "AMPR";      /* audio, MIDI, pixels, RAM: the letter at the head of a row says which */
/* One step is due: at most every 334 ms (3 a second), never under reduced motion. Does not step by itself: the caller feeds the new row. */
static int rain_due(const MonRain *r, double now_ms, int reduced_motion) { return !reduced_motion && now_ms - r->last_ms >= RAIN_STEP_MS; }
/* The newest row, from `n` bytes of source `src` (repeated across the row when there are fewer than RAIN_COLS); the rows before it move one down. */
static void rain_feed(MonRain *r, double now_ms, int src, const uint8_t *bytes, int n) {
    unsigned at = r->step % RAIN_ROWS;
    for (int c = 0; c < RAIN_COLS; c++) r->row[at][c] = n > 0 ? bytes[(c * (n < RAIN_COLS ? 1 : n / RAIN_COLS)) % n] : 0;
    r->tag[at] = RAIN_TAG[src >= 0 && src < RAIN_SRC ? src : 0];
    r->step++;
    r->last_ms = now_ms;
}
/* The row `i` back from the newest (0: newest), or NULL when fewer than i+1 rows were ever fed. */
static const uint8_t *rain_row(const MonRain *r, int i, char *tag) {
    if (i < 0 || i >= RAIN_ROWS || (unsigned)i >= r->step) return NULL;
    unsigned at = (r->step - 1 - (unsigned)i) % RAIN_ROWS;
    if (tag) *tag = r->tag[at];
    return r->row[at];
}

/* ---- text for the panel -------------------------------------------------------------------------------------------------------------------- */
static void mon_kb_text(char *out, size_t cap, unsigned long long kb) {
    if (kb >= 10ULL * 1048576) snprintf(out, cap, "%lluG", kb / 1048576);
    else if (kb >= 1048576) snprintf(out, cap, "%.1fG", (double)kb / 1048576.0);
    else if (kb >= 10240) snprintf(out, cap, "%lluM", kb / 1024);
    else if (kb >= 1024) snprintf(out, cap, "%.1fM", (double)kb / 1024.0);
    else snprintf(out, cap, "%lluK", kb);
}

#endif
