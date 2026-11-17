/* FM1-NTT host: where the CPU time goes by thread, and where the threads run (docs/PERFORMANCE.md, "Using the other cores"). MIT.
 *
 * The emulated machine runs on ONE host thread (SDL's audio callback in the app, the job thread in a report or benchmark); the main thread draws and
 * takes input; the unit has four cores. This says how busy each thread is (Linux: /proc/self/task/<tid>/stat, utime + stime in clock ticks, 10 ms; other
 * systems: "not available") and can pin the emulating thread to the last core and the main thread to the first. Nothing here is on by default.
 * The parser and the sums are plain functions of strings and numbers, so the self-test checks them on any system. Nothing here has run on an R36S. */
#ifndef FM1NTT_THREADS_H
#define FM1NTT_THREADS_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifdef __linux__
#include <dirent.h>
#include <sched.h>
#include <sys/syscall.h>
#endif

#define THR_MAX 64

typedef struct { int tid; char name[24]; unsigned long long ticks; int cpu; } ThrCpu;
typedef struct { ThrCpu t[THR_MAX]; int n; double wall_ms; } ThrSnap;

/* One line of /proc/<pid>/task/<tid>/stat: "tid (comm) S ppid ... utime stime ... processor". The comm may hold spaces and parentheses, so the fields are
 * counted from the LAST ')'. Fields (man proc): 3 state, 14 utime, 15 stime, 39 processor. 1 when it parsed. */
static int thr_parse_stat(const char *s, ThrCpu *out) {
    const char *lp = strchr(s, '('), *rp = strrchr(s, ')');
    if (!lp || !rp || rp < lp) return 0;
    memset(out, 0, sizeof *out);
    out->tid = atoi(s);
    size_t n = (size_t)(rp - lp - 1);
    if (n >= sizeof out->name) n = sizeof out->name - 1;
    memcpy(out->name, lp + 1, n);
    out->name[n] = 0;
    out->cpu = -1;
    unsigned long long utime = 0, stime = 0;
    int have = 0;
    const char *p = rp + 1;
    for (int field = 3; field <= 39; field++) {
        while (*p == ' ') p++;
        if (!*p) break;
        char *end;
        long long v = strtoll(p, &end, 10);
        if (field == 14) { utime = (unsigned long long)v; have |= 1; }
        else if (field == 15) { stime = (unsigned long long)v; have |= 2; }
        else if (field == 39) { out->cpu = (int)v; have |= 4; }
        while (*end && *end != ' ') end++;
        p = end;
    }
    out->ticks = utime + stime;
    return (have & 3) == 3;
}

/* Every thread of this process and the CPU time it has used so far. `now_ms` is the caller's clock. 0 threads when the system has no /proc. */
static int thr_snapshot(ThrSnap *s, double now_ms) {
    s->n = 0;
    s->wall_ms = now_ms;
#ifdef __linux__
    DIR *d = opendir("/proc/self/task");
    if (!d) return 0;
    struct dirent *e;
    while ((e = readdir(d)) && s->n < THR_MAX) {
        if (e->d_name[0] < '0' || e->d_name[0] > '9') continue;
        char p[96], buf[1024];
        snprintf(p, sizeof p, "/proc/self/task/%s/stat", e->d_name);
        FILE *f = fopen(p, "r");
        if (!f) continue;
        size_t n = fread(buf, 1, sizeof buf - 1, f);
        fclose(f);
        buf[n] = 0;
        ThrCpu t;
        if (thr_parse_stat(buf, &t)) s->t[s->n++] = t;
    }
    closedir(d);
#endif
    return s->n;
}

static int thr_ncpu(void) {
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    return n > 0 ? (int)n : 1;
}

/* The cores the placement test uses: the emulating thread on the last, the main thread on the first. 0 when there is one core (nothing to place). */
static int thr_choose(int ncpu, int *emu_cpu, int *ui_cpu) {
    if (ncpu < 2) return 0;
    *emu_cpu = ncpu - 1;
    *ui_cpu = 0;
    return 1;
}

/* "over 12.3 s: fm1ntt-job 98.2% of a core (cpu 3), main 9.1% (cpu 0), SDLAudioP2 0.3%; all 5 threads 107.6% of 4 cores" between two snapshots.
 * Threads under 0.5% are left out of the list and counted in the total; `main_tid` (the process id) is named "main". */
static void thr_describe(const ThrSnap *a, const ThrSnap *b, int main_tid, int ncpu, char *out, size_t cap) {
    double secs = (b->wall_ms - a->wall_ms) / 1000.0;
    long hz = sysconf(_SC_CLK_TCK);
    if (b->n == 0 || secs <= 0 || hz <= 0) { snprintf(out, cap, "not available on this system (no /proc/self/task)"); return; }
    double pct[THR_MAX], total = 0;
    int ord[THR_MAX];
    for (int i = 0; i < b->n; i++) {
        unsigned long long before = 0;
        for (int j = 0; j < a->n; j++) if (a->t[j].tid == b->t[i].tid) { before = a->t[j].ticks; break; }
        unsigned long long d = b->t[i].ticks >= before ? b->t[i].ticks - before : 0;
        pct[i] = 100.0 * ((double)d / (double)hz) / secs;
        total += pct[i];
        ord[i] = i;
    }
    for (int i = 0; i < b->n; i++)                                  /* a selection sort: at most THR_MAX threads */
        for (int j = i + 1; j < b->n; j++) if (pct[ord[j]] > pct[ord[i]]) { int t = ord[i]; ord[i] = ord[j]; ord[j] = t; }
    size_t w = (size_t)snprintf(out, cap, "over %.1f s: ", secs);
    int listed = 0;
    for (int k = 0; k < b->n && w < cap; k++) {
        int i = ord[k];
        if (pct[i] < 0.5) break;
        w += (size_t)snprintf(out + w, cap - w, "%s%s %.1f%%", listed ? ", " : "", b->t[i].tid == main_tid ? "main" : b->t[i].name, pct[i]);
        if (b->t[i].cpu >= 0 && w < cap) w += (size_t)snprintf(out + w, cap - w, " (cpu %d)", b->t[i].cpu);
        listed++;
    }
    if (w < cap) snprintf(out + w, cap - w, "%sall %d threads %.1f%% of %d core%s (clock ticks of %ld ms)", listed ? "; " : "no thread over 0.5%; ", b->n, total, ncpu, ncpu == 1 ? "" : "s", 1000 / hz);
}

/* ---- placement ---------------------------------------------------------------------------------------------------------------- */

typedef struct {
    int ok;
    int emu_cpu, ui_cpu, ncpu;
#ifdef __linux__
    cpu_set_t self_old, main_old;
#endif
} ThrPin;

#ifdef __linux__
static int thr_self_tid(void) { return (int)syscall(SYS_gettid); }
#endif

/* Pins the CALLING thread to `cpu` for good (the audio thread, the main thread). 0 on success; -1 when there is no such control or it was refused. */
static int thr_pin_self(int cpu) {
#ifdef __linux__
    cpu_set_t s;
    CPU_ZERO(&s);
    CPU_SET(cpu, &s);
    return sched_setaffinity(0, sizeof s, &s) == 0 ? 0 : -1;
#else
    (void)cpu;
    return -1;
#endif
}

/* Remembers the calling thread's and the main thread's masks. The caller is the emulating thread, not the main thread. 0 on success, -2 when the
 * caller IS the main thread (a run from the command line: there is no other thread to keep apart), -1 when the system will not say. */
static int thr_pin_begin(ThrPin *p, int emu_cpu, int ui_cpu, int ncpu) {
    memset(p, 0, sizeof *p);
    p->emu_cpu = emu_cpu;
    p->ui_cpu = ui_cpu;
    p->ncpu = ncpu;
#ifdef __linux__
    if (thr_self_tid() == getpid()) return -2;
    if (sched_getaffinity(0, sizeof p->self_old, &p->self_old) != 0) return -1;
    if (sched_getaffinity((pid_t)getpid(), sizeof p->main_old, &p->main_old) != 0) return -1;
    p->ok = 1;
    return 0;
#else
    return -1;
#endif
}

/* mode 0: both threads free on every online core; mode 1: the calling thread on emu_cpu and the main thread on ui_cpu. 0 on success. */
static int thr_pin_set(ThrPin *p, int mode) {
#ifdef __linux__
    if (!p->ok) return -1;
    cpu_set_t all, e, u;
    CPU_ZERO(&all); CPU_ZERO(&e); CPU_ZERO(&u);
    for (int i = 0; i < p->ncpu && i < CPU_SETSIZE; i++) CPU_SET(i, &all);
    CPU_SET(p->emu_cpu, &e);
    CPU_SET(p->ui_cpu, &u);
    int a = sched_setaffinity(0, sizeof all, mode ? &e : &all);
    int b = sched_setaffinity((pid_t)getpid(), sizeof all, mode ? &u : &all);
    return a == 0 && b == 0 ? 0 : -1;
#else
    (void)p; (void)mode;
    return -1;
#endif
}

/* Puts both masks back as thr_pin_begin found them. */
static void thr_pin_end(ThrPin *p) {
#ifdef __linux__
    if (!p->ok) return;
    sched_setaffinity(0, sizeof p->self_old, &p->self_old);
    sched_setaffinity((pid_t)getpid(), sizeof p->main_old, &p->main_old);
    p->ok = 0;
#else
    (void)p;
#endif
}

#endif
