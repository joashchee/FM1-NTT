/* FM1-NTT host: C and SDL2, pixel graphics (host/pix.h), over the Rust core (core/fm1ntt.h). MIT.
 *
 *   fm1ntt [--entry 0xADDR] FIRMWARE   open a window; a .fwsc package, or with --entry a raw XIP image (Felucca: 0x02000120)
 *   fm1ntt                             open it with nothing loaded (the FILES panel lists the firmware folder)
 *   fm1ntt --selftest                  headless: layout, pixel accuracy on every panel, font, contrast, the panels' logic, the core
 *   fm1ntt --screenshot OUT.bmp [--zoomed] [--panel N] [FIRMWARE]   draw one frame to a file
 *   fm1ntt --report [FIRMWARE]         the Device report (r3i6nos rule 9) to $BASE/r36s-report.txt, with the power windows
 *   fm1ntt --bench [--seconds N] [--fw FILE]   the Benchmark report to $BASE/bench/ (compare with scripts/bench-compare.py)
 *   fm1ntt --rec-demo [--features a,b] [--no-video] [FIRMWARE]   Rec demo (rule 11) to $BASE/Demo/
 *   fm1ntt --version
 *
 * The FM-1's 240x240 screen sits flush top left, pixel for pixel. Fn (raw button 16, Tab or F1) toggles Standard (1x, panels right and
 * below) and Zoomed (2x, one narrow panel at the right; Tab or F1 on a keyboard). On the pad Fn turns the panel carousel and L1+R1 together
 * open and close the menu (TOOLS); Home and End turn the carousel on a keyboard; on MACHINE the pad plays, see the pad section below. The carousel is as TinyNTT's:
 * MACHINE FILES CPU MEMORY CONSOLE MIDI SOUND TOOLS. The audio device paces the emulation: SDL's callback asks the core for frames.
 * Files are only written under the app's folder ($FM1NTT_DIR, else /roms/FM1-NTT on the R36S, else ~/FM1-NTT), to a temporary name
 * and then renamed. Nothing here has run on an R36S. */
#define _GNU_SOURCE
#include <math.h>
#include <stdarg.h>
#include <SDL.h>
#include <dirent.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>

#include "fm1ntt.h"
#include "pix.h"
#include "toast.h"

#ifndef FM1NTT_BUILD
#define FM1NTT_BUILD "unknown"
#endif

#define AUDIO_FRAMES 512
#define MAX_INSNS_PER_BUFFER 8000000u
#define PAD_FN 16
#define PAD_L1 4
#define PAD_R1 5
#define PAD_L2 6
#define PAD_R2 7
#define PAD_SELECT 12
#define PAD_START 13
#define STICK_DEAD 0.35f
#define STICK_MAX_CPS 12.0f     /* clicks a second at full deflection */
#define REC_RING_FRAMES (1 << 18)

#define NTOOLS 12
#define PACE_AUTO 4
enum { CF_NONE, CF_RESET, CF_RESTORE, CF_QUIT };
enum { A_NONE, A_UP, A_DOWN, A_LEFT, A_RIGHT, A_OK, A_BACK, A_X, A_Y, A_L2, A_R2 };

typedef struct {
    fm1ntt_machine *m;
    SDL_AudioDeviceID dev;
    int mode, panel, quit;
    char msg[160];
    int msg_style;
    int enc_turns[7];
    /* the pad: held buttons, the matrix id each press sent (+1; 0 none) so its release matches, the sticks, the note scale and key */
    unsigned pad_mask;
    int pad_sent[32];
    float axis[4], axis_acc[4];
    uint32_t axis_t;
    int scale, key_shift, menu_back;
    Toast toast;                        /* the hot-plug toast over row 0 */
    volatile int master_att;            /* percent the master level is cut by (0: unity) */
    int pot;
    /* files */
    char files[PX_MAX_FILES][PX_NAME];
    char fpaths[PX_MAX_FILES][300];     /* each entry's full path */
    char fbundled[PX_MAX_FILES];        /* 1 when it ships with the app (bundled-firmware/) */
    int nfiles, file_sel;
    uint32_t fbc[240 * 240];            /* the firmware's screen as last converted, with the LCD revision and emulated time it was taken at */
    uint32_t fb_rev;
    uint64_t fb_ms;
    int fb_ok;
    char bdir[300], pdir[300];          /* where scan_files found the bundled firmware and the pack ("" none) */
    char file[PX_NAME];                 /* the loaded firmware's name */
    /* cpu, memory, midi, tools */
    int cpu_sel, mem_region, midi_chan, midi_note, midi_vel, midi_held, tool_sel, slot;
    int jit_pref;                       /* the native JIT, applied to each firmware as it boots */
    int pace_mode, pace_lvl;            /* main-loop pacing: 0 OFF, 1..3 fixed, PACE_AUTO follows the speed; pace_lvl is the level AUTO has reached */
    uint32_t mem_off;
    uint8_t midi_log[PX_MIDI_LOG];
    int midi_log_n;
    int confirm;
    /* speed */
    uint32_t sp_real0;
    uint64_t sp_insns0, sp_ms0;
    double ips, speed;
    /* shared with the audio callback */
    volatile int paused, level_l, level_r, peak_l, peak_r;
    int16_t scope[PX_SCOPE];
    /* recorder */
    volatile int rec_on;
    int16_t *rec_ring;                  /* REC_RING_FRAMES stereo frames */
    SDL_atomic_t rec_w;                 /* samples the callback has written (monotonic) */
    unsigned long long rec_r;           /* samples the main thread has taken */
    FILE *rec;
    char rec_final[300], rec_tmp[310], rec_name[PX_NAME];
    unsigned long long rec_frames;
    int rec_dropped;
    uint32_t rev;
    /* the firmware's own path (the report and Rec demo use it), and the background jobs */
    char fw_path[300];
    int fw_raw;
    SDL_atomic_t callbacks;
    SDL_Thread *job;
    SDL_atomic_t job_done;
    int job_kind, job_style, want_demo;
    double job_seconds;
    char job_result[200];
    uint32_t budget;                    /* instructions a buffer may spend (0: MAX_INSNS_PER_BUFFER); Rec demo's self-test lowers it */
} App;

enum { JOB_NONE, JOB_REPORT, JOB_BENCH };
static void job_start(App *a, int kind);

/* ---- small helpers ------------------------------------------------------------------------------------------------------ */
static void say(App *a, int style, const char *fmt, const char *arg) {
    snprintf(a->msg, sizeof a->msg, fmt, arg ? arg : "");
    a->msg_style = style;
}
static void lock(App *a) { if (a->dev) SDL_LockAudioDevice(a->dev); }
static void unlock(App *a) { if (a->dev) SDL_UnlockAudioDevice(a->dev); }

static int is_dir(const char *p) { struct stat st; return stat(p, &st) == 0 && S_ISDIR(st.st_mode); }
static int is_file(const char *p) { struct stat st; return stat(p, &st) == 0 && S_ISREG(st.st_mode); }

/* The app's folder: only here is anything written. */
static void base_dir(char *out, size_t n) {
    const char *e = getenv("FM1NTT_DIR"), *h = getenv("HOME");
    if (e && *e) snprintf(out, n, "%s", e);
    else if (is_dir("/roms")) snprintf(out, n, "/roms/FM1-NTT");
    else if (h && *h) snprintf(out, n, "%s/FM1-NTT", h);
    else snprintf(out, n, "FM1-NTT");
}
static int mkdir_p(const char *path) {
    char buf[400];
    snprintf(buf, sizeof buf, "%s", path);
    for (char *p = buf + 1; *p; p++)
        if (*p == '/') { *p = 0; mkdir(buf, 0755); *p = '/'; }
    return mkdir(buf, 0755) == 0 || errno == EEXIST ? 0 : -1;
}
static void sub_path(char *out, size_t n, const char *sub, const char *name) {
    char b[300];
    base_dir(b, sizeof b);
    if (name) snprintf(out, n, "%s/%s/%s", b, sub, name); else snprintf(out, n, "%s/%s", b, sub);
}
/* Write to a temporary name, flush, then rename: a half-written file is never left under the real name. */
static int write_atomic(const char *path, const void *data, size_t len) {
    char tmp[420];
    snprintf(tmp, sizeof tmp, "%s.part", path);
    FILE *f = fopen(tmp, "wb");
    if (!f) return -1;
    int ok = fwrite(data, 1, len, f) == len && fflush(f) == 0;
    ok = fclose(f) == 0 && ok;
    if (!ok || rename(tmp, path) != 0) { remove(tmp); return -1; }
    return 0;
}
static uint8_t *read_file(const char *path, size_t *len, char *why, size_t whylen) {
    FILE *f = fopen(path, "rb");
    if (!f) { snprintf(why, whylen, "Cannot open the file"); return NULL; }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *buf = (n > 0 && n <= 32 * 1024 * 1024) ? malloc((size_t)n) : NULL;
    if (!buf) snprintf(why, whylen, n <= 0 ? "The file is empty" : "The file is too big to be firmware");
    else if (fread(buf, 1, (size_t)n, f) != (size_t)n) { snprintf(why, whylen, "Cannot read the file"); free(buf); buf = NULL; }
    else *len = (size_t)n;
    fclose(f);
    return buf;
}
static const char *g_stamp_override;   /* Rec demo fixes the stamp in what the tour writes, so two recordings are the same */
static void stamp(char *out, size_t n) {
    if (g_stamp_override) { snprintf(out, n, "%s", g_stamp_override); return; }
    time_t t = time(NULL);
    struct tm tmv;
    localtime_r(&t, &tmv);
    strftime(out, n, "%Y%m%d-%H%M%S", &tmv);
}

/* ---- loading ----------------------------------------------------------------------------------------------------------- */
static void speed_reset(App *a) { a->sp_real0 = SDL_GetTicks(); a->sp_insns0 = 0; a->sp_ms0 = 0; a->ips = a->speed = 0; }

/* The JIT and pacing settings onto the machine (a load makes a new one, with both off). Callers hold no lock. */
static void apply_prefs(App *a) {
    lock(a);
    fm1ntt_jit(a->m, a->jit_pref && fm1ntt_jit_available());
    fm1ntt_pace(a->m, a->pace_mode == PACE_AUTO ? a->pace_lvl : a->pace_mode);
    unlock(a);
}

static int load_firmware(App *a, const char *path, int raw, uint32_t entry) {
    char why[128] = "";
    size_t n = 0;
    uint8_t *buf = read_file(path, &n, why, sizeof why);
    if (!buf) { say(a, 3, "%s", why); return -1; }
    lock(a);
    int rc = raw ? fm1ntt_load_raw(a->m, buf, n, entry) : fm1ntt_load_package(a->m, buf, n);
    unlock(a);
    free(buf);
    if (rc != 0) { say(a, 3, "%s", fm1ntt_error()); return -1; }
    apply_prefs(a);
    a->fb_ok = 0;
    const char *base = strrchr(path, '/');
    snprintf(a->file, sizeof a->file, "%s", base ? base + 1 : path);
    snprintf(a->fw_path, sizeof a->fw_path, "%s", path);
    a->fw_raw = raw;
    a->midi_held = 0;
    speed_reset(a);
    say(a, 1, "Booted %s", a->file);
    return 0;
}

/* ---- the firmware folder ------------------------------------------------------------------------------------------------ */
static int has_ext(const char *name, const char *ext) {
    size_t n = strlen(name), e = strlen(ext);
    return n > e && strcasecmp(name + n - e, ext) == 0;
}
/* 1 when `dir` (or a folder in it, to `depth` levels) holds a firmware file: a folder that exists but is empty or the wrong one is not the answer. */
static int dir_has_fw(const char *dir, int depth) {
    DIR *d = opendir(dir);
    struct dirent *e;
    int found = 0;
    while (d && !found && (e = readdir(d))) {
        char p[600];
        if (e->d_name[0] == '.') continue;
        snprintf(p, sizeof p, "%s/%s", dir, e->d_name);
        if (is_dir(p)) found = depth > 0 && strcmp(e->d_name, "source") && strcmp(e->d_name, "docs") && dir_has_fw(p, depth - 1);
        else found = (has_ext(e->d_name, ".fwsc") || has_ext(e->d_name, ".bin")) && is_file(p);
    }
    if (d) closedir(d);
    return found;
}
/* The first of these that holds firmware: `env` (taken as given when set), then `exe_rel` beside the program, then `abs` and `cwd_rel` as written. */
static void first_fw_dir(char *out, size_t n, const char *env, const char *const *exe_rel, const char *const *abs) {
    out[0] = 0;
    const char *e = getenv(env);
    if (e && *e) { if (is_dir(e)) snprintf(out, n, "%s", e); return; }
    char *bp = SDL_GetBasePath();
    for (int i = 0; bp && exe_rel[i] && !out[0]; i++) {
        char p[600];
        snprintf(p, sizeof p, "%s%s", bp, exe_rel[i]);
        if (is_dir(p) && dir_has_fw(p, 2)) snprintf(out, n, "%s", p);
    }
    if (bp) SDL_free(bp);
    for (int i = 0; abs[i] && !out[0]; i++)
        if (is_dir(abs[i]) && dir_has_fw(abs[i], 2)) snprintf(out, n, "%s", abs[i]);
}
/* Where the firmware that ships with the app is: $FM1NTT_BUNDLED, else beside the program (the installed package's /usr/share/fm1ntt/firmware, or
 * a build's ../bundled-firmware), else the installed path itself, else ./bundled-firmware. "" when there is none. */
static void bundled_dir(char *out, size_t n) {
    static const char *const rel[] = {"../share/fm1ntt/firmware", "../bundled-firmware", "../../bundled-firmware", "bundled-firmware", NULL};
    static const char *const abs[] = {"/usr/share/fm1ntt/firmware", "bundled-firmware", "../bundled-firmware", NULL};
    first_fw_dir(out, n, "FM1NTT_BUNDLED", rel, abs);
}

/* Where the copyleft firmware pack is (the GPL tier: its own repository, FM1-NTT Firmware Pack, and its own package): $FM1NTT_PACK, else beside the program
 * (/usr/share/fm1ntt/firmware-gpl, or the pack repository inside or beside this one), else the installed path itself, else ./FM1-NTT-Firmware-Pack.
 * "" when it is not installed. */
static void pack_dir(char *out, size_t n) {
    static const char *const rel[] = {"../share/fm1ntt/firmware-gpl", "../FM1-NTT-Firmware-Pack", "../../FM1-NTT-Firmware-Pack", "FM1-NTT-Firmware-Pack", NULL};
    static const char *const abs[] = {"/usr/share/fm1ntt/firmware-gpl", "FM1-NTT-Firmware-Pack", "../FM1-NTT-Firmware-Pack", NULL};
    first_fw_dir(out, n, "FM1NTT_PACK", rel, abs);
}

typedef struct { char name[PX_NAME]; char path[300]; int bundled; } FileEnt;   /* bundled: 0 yours, 1 bundled (permissive), 2 the copyleft pack */
static int cmp_ents(const void *x, const void *y) {
    const FileEnt *a = x, *b = y;
    return a->bundled != b->bundled ? a->bundled - b->bundled : strcasecmp(a->name, b->name);
}
static int add_dir(FileEnt *out, int n, const char *dir, int bundled) {
    DIR *d = opendir(dir);
    struct dirent *e;
    while (d && (e = readdir(d)) && n < PX_MAX_FILES) {
        char p[600];
        if (e->d_name[0] == '.') continue;
        snprintf(p, sizeof p, "%s/%s", dir, e->d_name);
        if (bundled && is_dir(p)) { if (strcmp(e->d_name, "source") && strcmp(e->d_name, "docs")) n = add_dir(out, n, p, bundled); continue; }   /* one folder a family */
        if ((has_ext(e->d_name, ".fwsc") || has_ext(e->d_name, ".bin")) && is_file(p) && strlen(e->d_name) < PX_NAME - 4 && strlen(p) < sizeof out->path) {
            struct stat sb;
            long long size = stat(p, &sb) == 0 ? (long long)sb.st_size : -1;
            int dup = 0;                  /* the same file twice (a pack repository carries a built copy of its own folders) */
            for (int i = 0; i < n && !dup; i++) {
                struct stat sb2;
                dup = out[i].bundled == bundled && !strcmp(out[i].name, e->d_name) && stat(out[i].path, &sb2) == 0 && (long long)sb2.st_size == size;
            }
            if (dup) continue;
            snprintf(out[n].name, PX_NAME, "%s", e->d_name);
            snprintf(out[n].path, sizeof out[n].path, "%s", p);
            out[n++].bundled = bundled;
        }
    }
    if (d) closedir(d);
    return n;
}
/* The firmware folder (yours: first), then the bundled firmware, whole; the Quit row is the one after the last. */
static void scan_files(App *a) {
    char dir[300], bdir[300], pdir[300];
    sub_path(dir, sizeof dir, "firmware", NULL);
    mkdir_p(dir);
    bundled_dir(bdir, sizeof bdir);
    pack_dir(pdir, sizeof pdir);
    snprintf(a->bdir, sizeof a->bdir, "%s", bdir);
    snprintf(a->pdir, sizeof a->pdir, "%s", pdir);
    FileEnt *ents = calloc(PX_MAX_FILES, sizeof *ents);
    if (!ents) return;
    int n = add_dir(ents, 0, dir, 0);
    if (bdir[0]) n = add_dir(ents, n, bdir, 1);
    if (pdir[0]) n = add_dir(ents, n, pdir, 2);
    qsort(ents, (size_t)n, sizeof *ents, cmp_ents);
    a->nfiles = n;
    for (int i = 0; i < n; i++) {
        snprintf(a->files[i], PX_NAME, "%s", ents[i].name);
        snprintf(a->fpaths[i], sizeof a->fpaths[i], "%s", ents[i].path);
        a->fbundled[i] = (char)ents[i].bundled;
    }
    free(ents);
    if (a->file_sel > a->nfiles) a->file_sel = a->nfiles;       /* nfiles is the Quit row */
}
/* The firmware Rec demo and the benchmark use when the person gave none: a bundled one with a screen worth recording. "" when none ships. */
static void default_firmware(char *out, size_t n) {
    char bdir[300];
    out[0] = 0;
    bundled_dir(bdir, sizeof bdir);
    if (!bdir[0]) return;
    FileEnt *ents = calloc(PX_MAX_FILES, sizeof *ents);
    if (!ents) return;
    int c = add_dir(ents, 0, bdir, 1);
    for (int pass = 0; pass < 2 && !out[0]; pass++)
        for (int i = 0; i < c && !out[0]; i++)
            if (pass == 1 || !strncmp(ents[i].name, "untrue", 6)) snprintf(out, n, "%s", ents[i].path);   /* UNTRUE STEP: a grid of drums that moves */
    free(ents);
}

/* ---- the recorder: the callback fills a ring, the main thread drains it to a WAV ---------------------------------------------- */
static void wav_header(uint8_t h[44], unsigned long long frames) {
    uint32_t data = (uint32_t)(frames * 4), riff = 36 + data, rate = FM1_SAMPLE_RATE, brate = rate * 4;
    memcpy(h, "RIFF", 4); memcpy(h + 4, &riff, 4); memcpy(h + 8, "WAVEfmt ", 8);
    uint32_t fmt = 16; memcpy(h + 16, &fmt, 4);
    uint16_t pcm = 1, ch = 2, align = 4, bits = 16;
    memcpy(h + 20, &pcm, 2); memcpy(h + 22, &ch, 2); memcpy(h + 24, &rate, 4); memcpy(h + 28, &brate, 4);
    memcpy(h + 32, &align, 2); memcpy(h + 34, &bits, 2); memcpy(h + 36, "data", 4); memcpy(h + 40, &data, 4);
}
static void rec_pump(App *a) {
    if (!a->rec) return;
    /* the callback counts samples modulo 2^31 (about 6.7 hours of stereo); a recording is not meant to be longer */
    unsigned long long w = (unsigned long long)SDL_AtomicGet(&a->rec_w);
    const unsigned long long cap = (unsigned long long)REC_RING_FRAMES * 2;
    if (w - a->rec_r > cap) { a->rec_dropped = 1; a->rec_r = w - cap; }
    while (a->rec_r < w) {
        size_t i = (size_t)(a->rec_r % cap), n = (size_t)(w - a->rec_r);
        if (n > cap - i) n = (size_t)(cap - i);
        if (fwrite(a->rec_ring + i, sizeof(int16_t), n, a->rec) != n) { say(a, 3, "Writing the recording failed", NULL); break; }
        a->rec_r += n;
        a->rec_frames += n / 2;
    }
}
static int rec_start(App *a) {
    if (a->rec) return 0;
    char dir[300], name[PX_NAME], st[32];
    sub_path(dir, sizeof dir, "recordings", NULL);
    if (mkdir_p(dir) != 0) { say(a, 3, "Cannot make the recordings folder", NULL); return -1; }
    stamp(st, sizeof st);
    snprintf(name, sizeof name, "rec-%s.wav", st);
    snprintf(a->rec_final, sizeof a->rec_final, "%s/%s", dir, name);
    snprintf(a->rec_tmp, sizeof a->rec_tmp, "%s.part", a->rec_final);
    a->rec = fopen(a->rec_tmp, "wb");
    if (!a->rec) { say(a, 3, "Cannot write %s", name); return -1; }
    uint8_t h[44] = {0};
    fwrite(h, 1, 44, a->rec);
    if (!a->rec_ring) a->rec_ring = malloc((size_t)REC_RING_FRAMES * 2 * sizeof(int16_t));
    if (!a->rec_ring) { fclose(a->rec); a->rec = NULL; say(a, 3, "Out of memory", NULL); return -1; }
    lock(a);
    SDL_AtomicSet(&a->rec_w, 0);
    a->rec_r = 0;
    a->rec_on = 1;
    unlock(a);
    a->rec_frames = 0;
    a->rec_dropped = 0;
    snprintf(a->rec_name, sizeof a->rec_name, "%s", name);
    say(a, 1, "Recording %s", name);
    return 0;
}
static void rec_stop(App *a) {
    if (!a->rec) return;
    lock(a);
    a->rec_on = 0;
    unlock(a);
    rec_pump(a);
    uint8_t h[44];
    wav_header(h, a->rec_frames);
    fseek(a->rec, 0, SEEK_SET);
    fwrite(h, 1, 44, a->rec);
    int ok = fflush(a->rec) == 0;
    ok = fclose(a->rec) == 0 && ok;
    a->rec = NULL;
    if (ok && rename(a->rec_tmp, a->rec_final) == 0) {
        say(a, a->rec_dropped ? 2 : 1, a->rec_dropped ? "Saved %s (some audio was dropped)" : "Saved %s", a->rec_name);
    } else { remove(a->rec_tmp); say(a, 3, "Saving the recording failed", NULL); }
}

/* ---- audio: the device paces the emulated machine ---------------------------------------------------------------------- */
/* One buffer of the emulated machine's sound, and the meters, the scope and the recorder's ring that go with it. The callback's whole body,
 * so Rec demo steps the app with the same code. */
static void audio_step(App *a, int16_t *out, size_t frames) {
    if (a->paused || !fm1ntt_loaded(a->m)) { memset(out, 0, frames * 4); a->level_l = a->level_r = 0; return; }
    fm1ntt_render(a->m, out, frames, a->budget ? a->budget : MAX_INSNS_PER_BUFFER);
    if (a->master_att > 0) { int g = 100 - a->master_att; for (size_t i = 0; i < frames * 2; i++) out[i] = (int16_t)(out[i] * g / 100); }
    int pl = 0, pr = 0;
    for (size_t i = 0; i < frames; i++) {
        int l = abs(out[2 * i]), r = abs(out[2 * i + 1]);
        if (l > pl) pl = l;
        if (r > pr) pr = r;
    }
    a->level_l = pl * 100 / 32768;
    a->level_r = pr * 100 / 32768;
    if (a->level_l > a->peak_l) a->peak_l = a->level_l;
    if (a->level_r > a->peak_r) a->peak_r = a->level_r;
    /* the scope: the last 240 samples (every second one when there are enough) of the mix */
    size_t step = frames >= 2 * PX_SCOPE ? 2 : 1, n = frames >= 2 * PX_SCOPE ? PX_SCOPE : (frames < PX_SCOPE ? frames : PX_SCOPE);
    for (size_t i = 0; i < n; i++) { size_t f = frames - (n - i) * step; a->scope[i] = (int16_t)((out[2 * f] + out[2 * f + 1]) / 2); }
    if (a->rec_on && a->rec_ring) {
        unsigned long long w = (unsigned long long)(unsigned)SDL_AtomicGet(&a->rec_w);
        const unsigned long long cap = (unsigned long long)REC_RING_FRAMES * 2;
        for (size_t i = 0; i < frames * 2; i++) a->rec_ring[(w + i) % cap] = out[i];
        SDL_MemoryBarrierRelease();
        SDL_AtomicSet(&a->rec_w, (int)((w + frames * 2) & 0x7fffffff));
    }
}
static uint32_t g_demo_budget;
static int g_quiet_logs;                /* the self-test keeps report text off stdout */
static void audio_cb(void *ud, Uint8 *stream, int len) {
    App *a = ud;
    SDL_AtomicAdd(&a->callbacks, 1);
    audio_step(a, (int16_t *)stream, (size_t)len / 4);
}

/* ---- the machine's inputs ---------------------------------------------------------------------------------------------- */
static void key_id(App *a, int id, int down) { lock(a); fm1ntt_key(a->m, id, down); unlock(a); }
static void enc(App *a, int e, int clicks) { lock(a); fm1ntt_encoder(a->m, e, clicks); unlock(a); a->enc_turns[e] += clicks; }
static void pot(App *a, int by) {
    a->pot = a->pot + by < 0 ? 0 : a->pot + by > 1023 ? 1023 : a->pot + by;
    lock(a); fm1ntt_pot(a->m, a->pot); unlock(a);
}
static void midi_send(App *a, const uint8_t *b, int n, int log) {
    lock(a);
    for (int i = 0; i < n; i++) fm1ntt_midi_in(a->m, b[i]);
    unlock(a);
    for (int i = 0; i < n && log; i++) {
        if (a->midi_log_n == PX_MIDI_LOG) { memmove(a->midi_log, a->midi_log + 1, PX_MIDI_LOG - 1); a->midi_log_n--; }
        a->midi_log[a->midi_log_n++] = b[i];
    }
}
static void panic_all(App *a) {
    uint8_t m[3];
    for (int ch = 0; ch < 16; ch++) {
        m[0] = (uint8_t)(0xB0 | ch); m[1] = 123; m[2] = 0; midi_send(a, m, 3, ch == 0);
        m[1] = 120; midi_send(a, m, 3, 0);
    }
    lock(a);
    for (int id = 0; id < 41; id++) fm1ntt_key(a->m, id, 0);
    unlock(a);
    a->midi_held = 0;
    say(a, 1, "All notes off on every channel; every key released", NULL);
}
static void midi_ok(App *a, int down) {
    uint8_t m[3] = {(uint8_t)((down ? 0x90 : 0x80) | a->midi_chan), (uint8_t)a->midi_note, (uint8_t)(down ? a->midi_vel : 0)};
    if (down == a->midi_held) return;
    midi_send(a, m, 3, 1);
    a->midi_held = down;
}

/* ---- flash slots, screenshots, reset ----------------------------------------------------------------------------------- */
static void slot_path(App *a, char *out, size_t n) { char nm[32]; snprintf(nm, sizeof nm, "slot%d.flash", a->slot); sub_path(out, n, "slots", nm); }

static void save_flash(App *a) {
    if (!fm1ntt_loaded(a->m)) { say(a, 2, "Load a firmware first", NULL); return; }
    size_t n = fm1ntt_flash_size();
    uint8_t *buf = malloc(n);
    if (!buf) { say(a, 3, "Out of memory", NULL); return; }
    lock(a);
    size_t got = fm1ntt_flash_save(a->m, buf, n);
    unlock(a);
    char dir[300], p[320];
    sub_path(dir, sizeof dir, "slots", NULL);
    slot_path(a, p, sizeof p);
    if (got == n && mkdir_p(dir) == 0 && write_atomic(p, buf, n) == 0) { char s[8]; snprintf(s, sizeof s, "%d", a->slot); say(a, 1, "Flash saved to slot %s", s); }
    else say(a, 3, "Saving the flash failed", NULL);
    free(buf);
}
static void restore_flash(App *a) {
    char p[320], why[128] = "";
    size_t n = 0;
    slot_path(a, p, sizeof p);
    uint8_t *buf = read_file(p, &n, why, sizeof why);
    if (!buf) { say(a, 2, "Slot is empty or unreadable", NULL); return; }
    lock(a);
    int rc = fm1ntt_flash_restore(a->m, buf, n);
    unlock(a);
    free(buf);
    if (rc == 0) { speed_reset(a); a->midi_held = 0; char s[8]; snprintf(s, sizeof s, "%d", a->slot); say(a, 1, "Restored slot %s and rebooted", s); }
    else say(a, 3, "%s", fm1ntt_error());
}
static void screenshot(App *a) {
    if (!fm1ntt_loaded(a->m)) { say(a, 2, "Nothing on the screen yet", NULL); return; }
    static uint32_t px[FM1_SCREEN_W * FM1_SCREEN_H];
    lock(a); fm1ntt_screen(a->m, px); unlock(a);
    for (int i = 0; i < FM1_SCREEN_W * FM1_SCREEN_H; i++) px[i] |= 0xFF000000u;
    char dir[300], st[32], name[PX_NAME], p[360];
    sub_path(dir, sizeof dir, "screenshots", NULL);
    stamp(st, sizeof st);
    snprintf(name, sizeof name, "fm1-%s.bmp", st);
    snprintf(p, sizeof p, "%s/%s", dir, name);
    SDL_Surface *s = SDL_CreateRGBSurfaceFrom(px, FM1_SCREEN_W, FM1_SCREEN_H, 32, FM1_SCREEN_W * 4, 0xFF0000, 0xFF00, 0xFF, 0xFF000000);
    char tmp[380];
    snprintf(tmp, sizeof tmp, "%s.part", p);
    if (s && mkdir_p(dir) == 0 && SDL_SaveBMP(s, tmp) == 0 && rename(tmp, p) == 0) say(a, 1, "Saved %s", name);
    else { remove(tmp); say(a, 3, "Saving the screenshot failed", NULL); }
    if (s) SDL_FreeSurface(s);
}
static void do_reset(App *a) {
    lock(a); fm1ntt_reset(a->m); unlock(a);
    a->midi_held = 0;
    memset(a->enc_turns, 0, sizeof a->enc_turns);
    speed_reset(a);
    say(a, 1, "Reset: the firmware booted again from power-on", NULL);
}

/* ---- the panels' controls ---------------------------------------------------------------------------------------------- */
static int panel_step(int p, int d) { return (p + d + PANEL_COUNT) % PANEL_COUNT; }

static uint32_t mem_page(const App *a) { return a->mode == MODE_ZOOMED ? PX_MEM_BYTES / 2 : PX_MEM_BYTES; }
static void mem_move(App *a, long by) {
    size_t base, size = 0;
    uint32_t b32 = 0;
    (void)base;
    fm1ntt_region(a->m, a->mem_region, &b32, &size);
    long lim = size > 8 ? (long)size - 8 : 0, o = (long)a->mem_off + by;
    o = o < 0 ? 0 : o > lim ? lim : o;
    a->mem_off = (uint32_t)(o & ~7L);
}

static void request_quit(App *a) {
    if (a->rec) { a->confirm = CF_QUIT; return; }
    a->quit = 1;
}

static void tools_run(App *a) {
    switch (a->tool_sel) {
    case 0: a->paused = !a->paused; say(a, 0, a->paused ? "Paused (Start resumes)" : "Running", NULL); break;
    case 1: if (fm1ntt_loaded(a->m)) a->confirm = CF_RESET; else say(a, 2, "Nothing to reset", NULL); break;
    case 2: panic_all(a); break;
    case 3: a->jit_pref = !a->jit_pref; apply_prefs(a); say(a, 0, a->jit_pref ? "Native JIT on" : "Native JIT off", NULL); break;
    case 4: a->slot = a->slot % 4 + 1; say(a, 0, "Slot changed", NULL); break;
    case 5: save_flash(a); break;
    case 6: { char p[320]; slot_path(a, p, sizeof p); if (is_file(p)) a->confirm = CF_RESTORE; else say(a, 2, "That slot is empty", NULL); } break;
    case 7: screenshot(a); break;
    case 8: job_start(a, JOB_REPORT); break;
    case 9: a->want_demo = 1; break;
    case 10: job_start(a, JOB_BENCH); break;
    case 11: a->pace_mode = (a->pace_mode + 1) % 5; if (a->pace_mode == PACE_AUTO) a->pace_lvl = 0; apply_prefs(a); say(a, 0, a->pace_mode == PACE_AUTO ? "Pace AUTO: slows the main loop only when the unit cannot keep up" : a->pace_mode ? "Pace fixed: the main loop is slowed, the sound is not" : "Pace off: exact timing", NULL); break;
    default: break;
    }
}

/* One button press as an action, on the panels that are not the FM-1's own controls. */
static void act(App *a, int act) {
    if (a->confirm) {                       /* a question: A yes, B no, nothing else */
        int k = a->confirm;
        if (act == A_OK) {
            a->confirm = CF_NONE;
            if (k == CF_RESET) do_reset(a);
            else if (k == CF_RESTORE) restore_flash(a);
            else if (k == CF_QUIT) { rec_stop(a); a->quit = 1; }
        } else if (act == A_BACK) { a->confirm = CF_NONE; say(a, 0, "Cancelled", NULL); }
        return;
    }
    int rows = a->nfiles + 1;
    switch (a->panel) {
    case PANEL_FILES:
        if (act == A_UP) a->file_sel = (a->file_sel + rows - 1) % rows;
        else if (act == A_DOWN) a->file_sel = (a->file_sel + 1) % rows;
        else if (act == A_L2) a->file_sel = a->file_sel - 24 < 0 ? 0 : a->file_sel - 24;
        else if (act == A_R2) a->file_sel = a->file_sel + 24 >= rows ? rows - 1 : a->file_sel + 24;
        else if (act == A_X) { scan_files(a); say(a, 0, "Rescanned the folder", NULL); }
        else if (act == A_OK) {
            if (a->file_sel >= a->nfiles) request_quit(a);
            else {
                const char *p = a->fpaths[a->file_sel];
                load_firmware(a, p, has_ext(p, ".bin"), FM1_ENTRY_FELUCCA);
            }
        }
        break;
    case PANEL_CPU:
        if (act == A_UP || act == A_DOWN || act == A_LEFT || act == A_RIGHT) a->cpu_sel ^= 1;
        break;
    case PANEL_MEMORY:
        if (act == A_UP) mem_move(a, -8);
        else if (act == A_DOWN) mem_move(a, 8);
        else if (act == A_L2) mem_move(a, -(long)mem_page(a));
        else if (act == A_R2) mem_move(a, (long)mem_page(a));
        else if (act == A_X) mem_move(a, -0x10000);
        else if (act == A_Y) mem_move(a, 0x10000);
        else if (act == A_LEFT || act == A_RIGHT) { a->mem_region = (a->mem_region + (act == A_RIGHT ? 1 : 2)) % 3; a->mem_off = 0; }
        break;
    case PANEL_MIDI:
        if (act == A_LEFT && a->midi_note > 0) a->midi_note--;
        else if (act == A_RIGHT && a->midi_note < 127) a->midi_note++;
        else if (act == A_UP) a->midi_note = a->midi_note + 12 > 127 ? 127 : a->midi_note + 12;
        else if (act == A_DOWN) a->midi_note = a->midi_note - 12 < 0 ? 0 : a->midi_note - 12;
        else if (act == A_X) a->midi_vel = a->midi_vel - 8 < 1 ? 1 : a->midi_vel - 8;
        else if (act == A_Y) a->midi_vel = a->midi_vel + 8 > 127 ? 127 : a->midi_vel + 8;
        else if (act == A_L2) a->midi_chan = (a->midi_chan + 15) % 16;
        else if (act == A_R2) a->midi_chan = (a->midi_chan + 1) % 16;
        else if (act == A_BACK) panic_all(a);
        break;
    case PANEL_SOUND:
        if (act == A_OK) { if (a->rec) rec_stop(a); else rec_start(a); }
        else if (act == A_X) screenshot(a);
        else if (act == A_Y) { a->peak_l = a->peak_r = 0; say(a, 0, "Peaks cleared", NULL); }
        break;
    case PANEL_TOOLS:
        if (act == A_UP) a->tool_sel = (a->tool_sel + NTOOLS - 1) % NTOOLS;
        else if (act == A_DOWN) a->tool_sel = (a->tool_sel + 1) % NTOOLS;
        else if (act == A_LEFT) a->slot = (a->slot + 2) % 4 + 1;
        else if (act == A_RIGHT) a->slot = a->slot % 4 + 1;
        else if (act == A_OK) tools_run(a);
        break;
    default: break;
    }
}

/* ---- the UI state, from the app and the core --------------------------------------------------------------------------- */
static void fill_ui(App *a, UiState *u, int use_lock) {
    memset(u, 0, sizeof *u);
    u->panel = a->panel;
    if (use_lock) lock(a);
    int loaded = fm1ntt_loaded(a->m);
    u->fw_name = a->file;
    u->fw_board = loaded ? fm1ntt_board(a->m) : "";
    u->run_state = !loaded ? "NO FIRMWARE" : fm1ntt_halted(a->m) ? "HALT" : a->paused ? "PAUSED" : "RUN";
    u->ms = fm1ntt_ms(a->m);
    u->down = fm1ntt_keys_down(a->m);
    if (loaded) {
        uint32_t rev = fm1ntt_lcd_revision(a->m);
        if (!a->fb_ok || rev != a->fb_rev || u->ms < a->fb_ms) {      /* convert the 57,600 pixels only when the LCD changed: the audio waits while this lock is held */
            fm1ntt_screen(a->m, a->fbc);
            a->fb_rev = rev; a->fb_ok = 1;
        }
        a->fb_ms = u->ms;
        u->fb_valid = 1;
        a->rev = rev;
        Fm1Cpu c;
        for (int k = 0; k < 2; k++)
            if (fm1ntt_cpu(a->m, k, &c)) {
                PxCpu *p = &u->cpu[u->ncpu++];
                p->pc = c.pc; p->sp = c.sp; p->flags = c.flags; p->insns = c.insns;
                memcpy(p->regs, c.regs, sizeof p->regs);
            }
        u->jit = fm1ntt_jit_on(a->m);
        u->pace = fm1ntt_pace_level(a->m);
        fm1ntt_midi_stats(a->m, &u->midi_queued, &u->midi_dropped);
        if (a->panel == PANEL_MEMORY) {
            size_t size = 0;
            u->mem_region = a->mem_region;
            fm1ntt_region(a->m, a->mem_region, &u->mem_base, &size);
            u->mem_size = (uint32_t)size;
            u->mem_off = a->mem_off;
            u->mem_n = (int)fm1ntt_read_mem(a->m, a->mem_region, a->mem_off, u->mem, a->mode == MODE_ZOOMED ? PX_MEM_BYTES / 2 : PX_MEM_BYTES);
        }
        if (a->panel == PANEL_CONSOLE) u->console_n = (int)fm1ntt_console(a->m, (uint8_t *)u->console, PX_CONSOLE_BYTES);
    } else u->mem_region = a->mem_region;
    memcpy(u->scope, a->scope, sizeof u->scope);
    if (use_lock) unlock(a);
    if (u->fb_valid) memcpy(u->fb, a->fbc, sizeof u->fb);
    memcpy(u->enc_turns, a->enc_turns, sizeof u->enc_turns);
    u->pot = a->pot;
    u->level_l = a->level_l; u->level_r = a->level_r; u->peak_l = a->peak_l; u->peak_r = a->peak_r;
    u->msg = a->msg;
    u->msg_style = a->msg_style;
    /* files */
    char dir[300];
    sub_path(dir, sizeof dir, "firmware", NULL);
    snprintf(u->file_dir, sizeof u->file_dir, "%s", dir);
    snprintf(u->bundled_dir, sizeof u->bundled_dir, "%s", a->bdir);
    snprintf(u->pack_dir, sizeof u->pack_dir, "%s", a->pdir);
    for (int i = 0; i < a->nfiles; i++) snprintf(u->files[i], PX_NAME, "%c%c %s", !strcmp(a->fpaths[i], a->fw_path) && a->fw_path[0] ? '*' : ' ', a->fbundled[i] == 2 ? 'G' : a->fbundled[i] ? 'B' : ' ', a->files[i]);
    snprintf(u->files[a->nfiles], PX_NAME, "   [ QUIT ]");
    u->nfiles = a->nfiles + 1;
    u->file_sel = a->file_sel;
    u->cpu_sel = a->cpu_sel;
    u->ips = a->ips;
    u->speed = a->speed;
    u->midi_chan = a->midi_chan; u->midi_note = a->midi_note; u->midi_vel = a->midi_vel; u->midi_held = a->midi_held;
    memcpy(u->midi_log, a->midi_log, sizeof u->midi_log);
    u->midi_log_n = a->midi_log_n;
    u->rec_on = a->rec != NULL;
    u->rec_frames = a->rec_frames;
    snprintf(u->rec_file, sizeof u->rec_file, "%s", a->rec_name);
    /* tools */
    char slotp[320];
    slot_path(a, slotp, sizeof slotp);
    snprintf(u->tools[0], PX_NAME, "  %s", a->paused ? "RESUME" : "PAUSE");
    snprintf(u->tools[1], PX_NAME, "  RESET (BOOT AGAIN)");
    snprintf(u->tools[2], PX_NAME, "  PANIC: ALL NOTES OFF");
    snprintf(u->tools[3], PX_NAME, "  NATIVE JIT: %s", u->jit ? "ON" : "OFF");
    snprintf(u->tools[4], PX_NAME, "  SLOT %d (%s)   < >", a->slot, is_file(slotp) ? "SAVED" : "EMPTY");
    snprintf(u->tools[5], PX_NAME, "  SAVE FLASH TO SLOT %d", a->slot);
    snprintf(u->tools[6], PX_NAME, "  RESTORE SLOT %d", a->slot);
    snprintf(u->tools[7], PX_NAME, "  SAVE SCREENSHOT");
    snprintf(u->tools[8], PX_NAME, "  DEVICE REPORT%s", a->job_kind == JOB_REPORT ? " (RUNNING)" : "");
    snprintf(u->tools[9], PX_NAME, "  REC DEMO");
    snprintf(u->tools[10], PX_NAME, "  BENCHMARK%s", a->job_kind == JOB_BENCH ? " (RUNNING)" : "");
    if (a->pace_mode == PACE_AUTO) snprintf(u->tools[11], PX_NAME, "  PACE: AUTO (NOW %d)", a->pace_lvl);
    else if (a->pace_mode) snprintf(u->tools[11], PX_NAME, "  PACE: %d", a->pace_mode);
    else snprintf(u->tools[11], PX_NAME, "  PACE: OFF");
    u->ntools = NTOOLS;
    u->tool_sel = a->tool_sel;
    if (a->confirm == CF_RESET) snprintf(u->confirm, sizeof u->confirm, "Reset the machine? The emulated flash goes back to what the firmware package carried.");
    else if (a->confirm == CF_RESTORE) snprintf(u->confirm, sizeof u->confirm, "Restore slot %d into the flash? The machine reboots.", a->slot);
    else if (a->confirm == CF_QUIT) snprintf(u->confirm, sizeof u->confirm, "A recording is running. Stop it, save it and quit?");
}

/* AUTO pacing: one level up when the unit falls behind real time, one down when it has a lot to spare (exact timing is kept when it can) */
static void pace_auto(App *a) {
    if (a->pace_mode != PACE_AUTO || a->paused || a->speed <= 0 || !fm1ntt_loaded(a->m)) return;
    int l = a->pace_lvl;
    if (a->speed < 0.97 && l < 3) l++;
    else if (a->speed > 3.0 && l > 0) l--;
    if (l != a->pace_lvl) { a->pace_lvl = l; lock(a); fm1ntt_pace(a->m, l); unlock(a); }
}

/* speed: once a second, instructions a second and emulated time over real time */
static void speed_tick(App *a) {
    uint32_t now = SDL_GetTicks();
    if (now - a->sp_real0 < 1000) return;
    lock(a);
    Fm1Cpu c0, c1;
    uint64_t ins = fm1ntt_cpu(a->m, 0, &c0) ? c0.insns : 0, ms = fm1ntt_ms(a->m);
    if (fm1ntt_cpu(a->m, 1, &c1)) ins += c1.insns;
    unlock(a);
    double dt = (now - a->sp_real0) / 1000.0;
    if (!a->paused && ins >= a->sp_insns0) { a->ips = (double)(ins - a->sp_insns0) / dt; a->speed = (double)(ms - a->sp_ms0) / 1000.0 / dt; }
    else a->ips = a->speed = 0;
    a->sp_real0 = now; a->sp_insns0 = ins; a->sp_ms0 = ms;
    pace_auto(a);
}

/* ---- input ------------------------------------------------------------------------------------------------------------- */
/* The computer keyboard as the FM-1's note keys: two rows, as a tracker's. Returns the note key 0..26, or -1. */
static int note_of_key(SDL_Keycode k) {
    static const char lower[] = "zsxdcvgbhnjm,l.;/";   /* keys 0..16 */
    static const char upper[] = "q2w3er5t6y7ui9o";      /* keys 12..26 */
    if (k < 128 && k > 0) {
        const char *p = strchr(lower, (int)k);
        if (p && *p) return (int)(p - lower);
        p = strchr(upper, (int)k);
        if (p && *p) return 12 + (int)(p - upper);
    }
    return -1;
}
static int button_of_key(SDL_Keycode k) {
    switch (k) {
    case SDLK_LEFTBRACKET: return 0;
    case SDLK_RIGHTBRACKET: return 1;
    case SDLK_F2: return 2;  case SDLK_F3: return 3;  case SDLK_F4: return 4;  case SDLK_F5: return 5;
    case SDLK_F6: return 6;  case SDLK_F7: return 7;  case SDLK_F8: return 8;  case SDLK_F9: return 9;
    case SDLK_F10: return 10; case SDLK_F11: return 11; case SDLK_SPACE: return 12; case SDLK_F12: return 13;
    default: return -1;
    }
}
/* The R36S pad's raw buttons: B 0, A 1, X 2, Y 3, L1 4, R1 5, L2 6, R2 7, up 8, down 9, left 10, right 11, Select 12, Start 13, Fn 16;
 * axes 0 1 the left stick (x, y), 2 3 the right (y is down in SDL). Every other panel: the d-pad and A B X Y L2 R2 as actions. */
static int act_of_pad(int b) {
    switch (b) {
    case 8: return A_UP; case 9: return A_DOWN; case 10: return A_LEFT; case 11: return A_RIGHT;
    case 1: return A_OK; case 0: return A_BACK; case 2: return A_X; case 3: return A_Y; case 6: return A_L2; case 7: return A_R2;
    default: return A_NONE;
    }
}

/* On MACHINE the pad plays. Without a modifier: the d-pad and face buttons are eight notes of the chosen scale (up C, left D, down E,
 * right F, Y G, B A, A B, X the next C), Select OCT-, Start OCT+, the sticks the four knobs. L1 held: the sticks are master, presets,
 * select and algorithm; the d-pad, faces, Select and Start the ten black-key functions (OP1-OP6, PIT, GLO, MONO, POLY); Fn changes the
 * scale. L2 held: SEL FX HOME SAVE ENV LFO ARP SEQ; Select and Start transpose (my addition: L1's Select and Start are MONO and POLY). R2 held: GLO EDIT PLAY REC. */
enum { MOD_NONE, MOD_L1, MOD_L2, MOD_R2 };
static const struct { const char *name; int n; int semi[8]; } SCALES[] = {
    {"MAJOR", 7, {0, 2, 4, 5, 7, 9, 11}}, {"MINOR", 7, {0, 2, 3, 5, 7, 8, 10}}, {"DORIAN", 7, {0, 2, 3, 5, 7, 9, 10}},
    {"MIXOLYDIAN", 7, {0, 2, 4, 5, 7, 9, 10}}, {"MAJOR PENTATONIC", 5, {0, 2, 4, 7, 9}}, {"MINOR PENTATONIC", 5, {0, 3, 5, 7, 10}},
    {"CHROMATIC", 8, {0, 1, 2, 3, 4, 5, 6, 7}},
};
#define NSCALES ((int)(sizeof SCALES / sizeof SCALES[0]))
#define KEY_MIN (-6)
#define KEY_MAX 5
static const char *const KEY_NAMES[12] = {"C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"};
static const char *key_name(int shift) { return KEY_NAMES[((shift % 12) + 12) % 12]; }
/* The pad's button as a position 0..7 in the scale (up left down right Y B A X), or -1. */
static int degree_of_pad(int b) {
    switch (b) { case 8: return 0; case 10: return 1; case 9: return 2; case 11: return 3; case 3: return 4; case 0: return 5; case 1: return 6; case 2: return 7; default: return -1; }
}
/* The matrix id of a scale position: C4 is note key 7 (F3 is key 0) plus the key's shift; -1 when it falls off the 27 keys. */
static int note_id_of_degree(int scale, int shift, int deg) {
    int n = SCALES[scale].n, semi = SCALES[scale].semi[deg % n] + 12 * (deg / n), k = 7 + shift + semi;
    return k >= 0 && k < 27 ? FM1_KEY_BASE + k : -1;
}
static int pad_mod(unsigned mask) {
    return mask >> PAD_L1 & 1 ? MOD_L1 : mask >> PAD_L2 & 1 ? MOD_L2 : mask >> PAD_R2 & 1 ? MOD_R2 : MOD_NONE;
}
/* Under a modifier, the FM-1 button a pad button presses (matrix id 0..13), -1 none, -2 a name the FM-1 has no button for. */
static int button_of_pad_mod(int mod, int b) {
    if (mod == MOD_L2) switch (b) { case 8: return -2; case 10: return 2; case 9: return 8; case 11: return 9; case 3: return 4; case 0: return 5; case 1: return 10; case 2: return 11; default: return -1; }
    if (mod == MOD_R2) switch (b) { case 8: return 7; case 10: return 6; case 9: return 12; case 11: return 13; default: return -1; }
    return -1;
}
/* The manual puts ten functions on the piano's black keys: OP1 to OP6, PIT, GLO, MONO, POLY. They are note keys (F#3, G#3, A#3, C#4, D#4,
 * F#4, G#4, A#4, C#5, D#5: note key indexes 1 3 5 8 10 13 15 17 20 22), in that order; the ORDER is assumed from the manual's list, not
 * measured. The pad under L1 presses them in the d-pad and face order: up left down right Y B A X, then Select and Start. */
static const int FUNCTION_KEYS[10] = {1, 3, 5, 8, 10, 13, 15, 17, 20, 22};
static const char *const FUNCTION_NAMES[10] = {"OP1", "OP2", "OP3", "OP4", "OP5", "OP6", "PIT", "GLO", "MONO", "POLY"};
static int function_of_pad(int b) { return b == PAD_SELECT ? 8 : b == PAD_START ? 9 : degree_of_pad(b); }
/* A stick's deflection (-1..1, positive = up for y, right for x) as whole clicks over dt ms; the remainder stays in acc. */
static int stick_clicks(float *acc, float v, uint32_t dt_ms) {
    float m = v < 0 ? -v : v;
    if (m <= STICK_DEAD) { *acc = 0; return 0; }
    *acc += (v < 0 ? -1.0f : 1.0f) * (m - STICK_DEAD) / (1.0f - STICK_DEAD) * STICK_MAX_CPS * (float)dt_ms / 1000.0f;
    int n = (int)*acc;
    *acc -= (float)n;
    return n;
}
static int sticks_active(const App *a) {
    for (int i = 0; i < 4; i++) if (a->axis[i] > STICK_DEAD || a->axis[i] < -STICK_DEAD) return 1;
    return 0;
}
static void sticks_tick(App *a, uint32_t now) {
    uint32_t dt = a->axis_t ? now - a->axis_t : 0;
    a->axis_t = now;
    if (dt > 200) dt = 200;
    if (a->panel != PANEL_MACHINE || a->confirm) { for (int i = 0; i < 4; i++) a->axis_acc[i] = 0; return; }
    int l1 = pad_mod(a->pad_mask) == MOD_L1;
    /* knob 1..4 are encoders 2..5; with L1: presets 6, select 0, algorithm 1, and the left stick's x is the master level */
    static const int plain[4] = {2, 3, 4, 5}, held[4] = {-1, 6, 0, 1};
    for (int i = 0; i < 4; i++) {
        float v = i & 1 ? -a->axis[i] : a->axis[i];       /* the y axes: up is positive */
        int n = stick_clicks(&a->axis_acc[i], v, dt), e = l1 ? held[i] : plain[i];
        if (!n) continue;
        if (e >= 0) enc(a, e, n);
        else {
            int att = a->master_att - n * 2;                /* clicks of 2% */
            a->master_att = att < 0 ? 0 : att > 100 ? 100 : att;
            snprintf(a->msg, sizeof a->msg, "Master %d%%", 100 - a->master_att);
            a->msg_style = 0;
        }
    }
}
/* One pad button pressed on MACHINE. */
static void pad_machine_press(App *a, int b) {
    int mod = pad_mod(a->pad_mask), id = -1;
    if (mod == MOD_L2 && (b == PAD_SELECT || b == PAD_START)) {
        a->key_shift += b == PAD_START ? 1 : -1;
        if (a->key_shift > KEY_MAX) a->key_shift = KEY_MIN;
        if (a->key_shift < KEY_MIN) a->key_shift = KEY_MAX;
        say(a, 0, "Key %s", key_name(a->key_shift));
        return;
    }
    if (mod == MOD_NONE) {
        int d = degree_of_pad(b);
        if (d >= 0) { id = note_id_of_degree(a->scale, a->key_shift, d); if (id < 0) say(a, 0, "That note is off the FM-1's keys", NULL); }
        else if (b == PAD_SELECT) id = 0;
        else if (b == PAD_START) id = 1;
    } else if (mod == MOD_L1) {
        int f = function_of_pad(b);
        if (f >= 0) { id = FM1_KEY_BASE + FUNCTION_KEYS[f]; snprintf(a->msg, sizeof a->msg, "%s", FUNCTION_NAMES[f]); a->msg_style = 0; }
    } else {
        id = button_of_pad_mod(mod, b);
        if (id == -2) { say(a, 0, "SEL: the FM-1 has no matrix button for it", NULL); id = -1; }
    }
    if (id >= 0 && b < 32) { key_id(a, id, 1); a->pad_sent[b] = id + 1; }
}
static int act_of_key(SDL_Keycode k) {
    switch (k) {
    case SDLK_UP: return A_UP; case SDLK_DOWN: return A_DOWN; case SDLK_LEFT: return A_LEFT; case SDLK_RIGHT: return A_RIGHT;
    case SDLK_RETURN: case SDLK_KP_ENTER: return A_OK; case SDLK_BACKSPACE: return A_BACK;
    case SDLK_INSERT: return A_X; case SDLK_DELETE: return A_Y; case SDLK_PAGEUP: return A_L2; case SDLK_PAGEDOWN: return A_R2;
    default: return A_NONE;
    }
}

/* Zoom is a keyboard key (Tab or F1); the pad's Fn is the carousel. */
static int is_toggle(const SDL_Event *e) {
    return e->type == SDL_KEYDOWN && !e->key.repeat && (e->key.keysym.sym == SDLK_TAB || e->key.keysym.sym == SDLK_F1);
}
/* Fn on the pad and Home / End on a keyboard turn the carousel: -1 before, +1 after, 0 neither. */
static int is_carousel(const SDL_Event *e) {
    if (e->type == SDL_JOYBUTTONDOWN) return e->jbutton.button == PAD_FN ? 1 : 0;
    if (e->type == SDL_KEYDOWN && !e->key.repeat) return e->key.keysym.sym == SDLK_HOME ? -1 : e->key.keysym.sym == SDLK_END ? 1 : 0;
    return 0;
}

static void handle(App *a, const SDL_Event *e) {
    int machine_panel = a->panel == PANEL_MACHINE && !a->confirm;
    if (e->type == SDL_KEYDOWN || e->type == SDL_KEYUP) {
        int down = e->type == SDL_KEYDOWN;
        SDL_Keycode k = e->key.keysym.sym;
        if (a->panel == PANEL_MIDI && !a->confirm && (k == SDLK_RETURN || k == SDLK_KP_ENTER)) { midi_ok(a, down); return; }
        if (e->key.repeat) return;
        if (down && (k == SDLK_p || k == SDLK_PAUSE)) { a->paused = !a->paused; say(a, 0, a->paused ? "Paused (P or Start resumes)" : "Running", NULL); return; }
        if (a->confirm) { if (down) act(a, act_of_key(k)); return; }
        int n = note_of_key(k), b = button_of_key(k);
        if (n >= 0) { key_id(a, FM1_KEY_BASE + n, down); return; }
        if (b >= 0) { key_id(a, b, down); return; }
        if (!down) return;
        if (machine_panel) switch (k) {
            case SDLK_LEFT: enc(a, 0, -1); break;
            case SDLK_RIGHT: enc(a, 0, 1); break;
            case SDLK_UP: enc(a, 1, 1); break;
            case SDLK_DOWN: enc(a, 1, -1); break;
            case SDLK_PAGEUP: enc(a, 6, 1); break;
            case SDLK_PAGEDOWN: enc(a, 6, -1); break;
            case SDLK_MINUS: pot(a, -64); break;
            case SDLK_EQUALS: pot(a, 64); break;
            default: break;
        } else act(a, act_of_key(k));
    } else if (e->type == SDL_JOYAXISMOTION) {
        if (e->jaxis.axis < 4) a->axis[e->jaxis.axis] = e->jaxis.value / 32768.0f;
    } else if (e->type == SDL_JOYBUTTONDOWN || e->type == SDL_JOYBUTTONUP) {
        int down = e->type == SDL_JOYBUTTONDOWN, b = e->jbutton.button;
        if (b < 32 && !down && a->pad_sent[b]) { key_id(a, a->pad_sent[b] - 1, 0); a->pad_sent[b] = 0; return; }   /* the release matches the press */
        if (b == PAD_START && !machine_panel) { if (down) { a->paused = !a->paused; say(a, 0, a->paused ? "Paused (Start resumes)" : "Running", NULL); } return; }
        if (a->panel == PANEL_MIDI && !a->confirm && b == 1) { midi_ok(a, down); return; }
        if (machine_panel) { if (down) pad_machine_press(a, b); }
        else if (down) act(a, act_of_pad(b));
    }
}

/* One event as the whole app takes it: quit, Fn, the carousel, then the panel's own controls. Rec demo's tour calls this too, so it
 * records the real input path. */
static void release_pad(App *a) {
    for (int b = 0; b < 32; b++) if (a->pad_sent[b]) { key_id(a, a->pad_sent[b] - 1, 0); a->pad_sent[b] = 0; }
}
/* L1 and R1 together open the menu (TOOLS) and close it again, back to the panel it was opened from. */
static void menu_toggle(App *a) {
    release_pad(a);
    if (a->panel != PANEL_TOOLS) { a->menu_back = a->panel; a->panel = PANEL_TOOLS; }
    else a->panel = a->menu_back;
    if (a->panel != PANEL_MIDI && a->midi_held) midi_ok(a, 0);
}
static void dispatch(App *a, const SDL_Event *e) {
    if ((e->type == SDL_KEYDOWN || e->type == SDL_JOYBUTTONDOWN) && toast_key(&a->toast, SDL_GetTicks())) return;   /* a failure's key is used up */
    int turn = is_carousel(e);
    if (e->type == SDL_JOYBUTTONDOWN && e->jbutton.button < 32) a->pad_mask |= 1u << e->jbutton.button;
    if (e->type == SDL_JOYBUTTONUP && e->jbutton.button < 32) a->pad_mask &= ~(1u << e->jbutton.button);
    int chord = e->type == SDL_JOYBUTTONDOWN && (e->jbutton.button == PAD_L1 || e->jbutton.button == PAD_R1) && (a->pad_mask >> PAD_L1 & 1) && (a->pad_mask >> PAD_R1 & 1);
    int fn_scale = e->type == SDL_JOYBUTTONDOWN && e->jbutton.button == PAD_FN && a->panel == PANEL_MACHINE && !a->confirm && (a->pad_mask >> PAD_L1 & 1);
    if (e->type == SDL_QUIT) { rec_stop(a); a->quit = 1; }
    else if (e->type == SDL_KEYDOWN && e->key.keysym.sym == SDLK_ESCAPE && !e->key.repeat) { if (a->confirm) a->confirm = CF_NONE; else request_quit(a); }
    else if (is_toggle(e)) a->mode ^= 1;
    else if (chord) { if (!a->confirm) menu_toggle(a); }
    else if (fn_scale) { a->scale = (a->scale + 1) % NSCALES; say(a, 0, "Scale %s", SCALES[a->scale].name); }
    else if (turn && !a->confirm) { release_pad(a); a->panel = panel_step(a->panel, turn); if (a->panel != PANEL_MIDI && a->midi_held) midi_ok(a, 0); }
    else if (turn) { /* a question is open: A or B first */ }
    else handle(a, e);
}

/* ---- the splash: shown at once, before anything heavy ------------------------------------------------------------------ */
static void draw_splash(uint32_t *c, const char *what, int percent) {
    for (int i = 0; i < PX_W * PX_H; i++) c[i] = C_BG;
    char line[96];
    int cx = PX_W / 2;
    px_text(c, cx - px_text_w("FM1-NTT", 4) / 2, 150, "FM1-NTT", C_HEAD, 4, 0);
    px_text(c, cx - px_text_w("FM-1 FIRMWARE SANDBOX", 1) / 2, 196, "FM-1 FIRMWARE SANDBOX", C_TEXT, 1, 0);
    px_text(c, cx - px_text_w(what, 1) / 2, 250, what, C_TEXT, 1, 0);
    px_meter(c, (PxRect){cx - 150, 266, 300, 10}, percent, C_HEAD);
    snprintf(line, sizeof line, "V%s %s", fm1ntt_version(), FM1NTT_BUILD);
    px_text(c, cx - px_text_w(line, 1) / 2, 296, line, C_DIM, 1, 0);
}

static void present(SDL_Renderer *r, SDL_Texture *t, const uint32_t *c) {
    SDL_UpdateTexture(t, NULL, c, PX_W * 4);
    SDL_RenderClear(r);
    SDL_RenderCopy(r, t, NULL, NULL);
    SDL_RenderPresent(r);
}

#include "report.h"
#include "demo.h"

/* ---- the progress box: shown over the window for as long as a job (Device report, Benchmark) runs, so a wait is never silent ---- */
static void draw_job_progress(uint32_t *c, const App *a) {
    if (!a->job) return;
    int step, steps; char doing[96]; uint32_t since, step_since;
    prog_get(&step, &steps, doing, sizeof doing, &since, &step_since);
    uint32_t now = SDL_GetTicks();
    if (steps < 1) steps = 1;
    double within = 1.0 - exp(-(double)(now - step_since) / 3000.0);       /* the current step creeps toward its end, never reaching it */
    int pct = (int)(100.0 * ((double)step + 0.9 * within) / steps);
    if (pct > 99) pct = 99;
    PxRect box = {PX_W / 2 - 220, PX_H / 2 - 56, 440, 112};
    px_rect(c, (PxRect){box.x - 2, box.y - 2, box.w + 4, box.h + 4}, C_BG);
    px_box(c, box, a->job_kind == JOB_REPORT ? "DEVICE REPORT" : "BENCHMARK");
    char line[160];
    snprintf(line, sizeof line, "STEP %d OF %d: %s", step < steps ? step + 1 : steps, steps, doing[0] ? doing : "starting");
    px_text(c, box.x + 14, box.y + 20, line, C_TEXT, 1, 0);
    px_meter(c, (PxRect){box.x + 14, box.y + 44, box.w - 28, 14}, pct, C_HEAD);
    snprintf(line, sizeof line, "%d%%   %us elapsed   the emulation is paused while it measures", pct, (now - since) / 1000);
    px_text(c, box.x + 14, box.y + 68, line, C_DIM, 1, 0);
    px_text(c, box.x + 14, box.y + 86, "Nothing is wrong: wait for the result", C_DIM, 1, 0);
}

/* ---- the self-test ----------------------------------------------------------------------------------------------------- */
static int selftest(void) {
    int bad = 0;
#define CHECK(c) do { if (!(c)) { fprintf(stderr, "selftest FAIL: %s (line %d)\n", #c, __LINE__); bad++; } } while (0)
    /* the layouts: the screen flush top left at its size, the panels fill the rest and never overlap it or each other */
    PxRect s1 = px_screen_rect(MODE_STANDARD), s2 = px_screen_rect(MODE_ZOOMED);
    CHECK(s1.x == 0 && s1.y == 0 && s1.w == 240 && s1.h == 240);
    CHECK(s2.x == 0 && s2.y == 0 && s2.w == 480 && s2.h == 480);
    for (int mode = 0; mode < 2; mode++) {
        PxRect sr = px_screen_rect(mode), pr[2];
        int n = px_panel_rects(mode, pr);
        long area = (long)sr.w * sr.h;
        for (int i = 0; i < n; i++) {
            CHECK(pr[i].x >= 0 && pr[i].y >= 0 && pr[i].x + pr[i].w <= PX_W && pr[i].y + pr[i].h <= PX_H);
            CHECK(pr[i].x >= sr.x + sr.w || pr[i].y >= sr.y + sr.h);
            for (int j = 0; j < i; j++) CHECK(pr[i].x >= pr[j].x + pr[j].w || pr[j].x >= pr[i].x + pr[i].w || pr[i].y >= pr[j].y + pr[j].h || pr[j].y >= pr[i].y + pr[i].h);
            area += (long)pr[i].w * pr[i].h;
        }
        CHECK(area == (long)PX_W * PX_H);
        PxRect cr = px_carousel_rect(mode), co = px_content_rect(mode);
        CHECK(cr.x == pr[0].x && cr.y == pr[0].y && cr.w == pr[0].w && co.y == cr.y + cr.h + 2 && co.y + co.h == pr[0].y + pr[0].h);
    }
    /* pixel accuracy on EVERY panel in both modes: the FM-1's screen lands exactly, 1x and 2x, whatever the panel draws */
    static UiState u;
    static uint32_t canvas[PX_W * PX_H];
    for (int mode = 0; mode < 2; mode++)
        for (int panel = 0; panel < PANEL_COUNT; panel++) {
            memset(&u, 0, sizeof u);
            for (int i = 0; i < FM_W * FM_H; i++) u.fb[i] = (uint32_t)(i * 2654435761u) & 0xFFFFFF;
            u.fb_valid = 1; u.panel = panel;
            u.run_state = "RUN"; u.fw_name = "TEST.FWSC"; u.fw_board = "Fm1"; u.msg = "HELLO WORLD THIS IS A LONG LOG LINE THAT MUST WRAP NICELY";
            u.ncpu = 1; u.cpu[0].pc = 0x2000120; u.cpu[0].regs[3] = 0xDEADBEEF; u.cpu[0].insns = 123456789; u.ips = 1.5e7; u.speed = 1.25;
            u.nfiles = 3; snprintf(u.files[0], PX_NAME, "  A.FWSC"); snprintf(u.files[1], PX_NAME, "  B.BIN"); snprintf(u.files[2], PX_NAME, "  [ QUIT ]");
            snprintf(u.file_dir, sizeof u.file_dir, "/x/firmware");
            u.mem_n = mode ? 128 : 256; u.mem_size = 0x100000; u.mem_base = 0x1c00000; for (int i = 0; i < PX_MEM_BYTES; i++) u.mem[i] = (uint8_t)i;
            snprintf(u.console, sizeof u.console, "BOOT OK\nHELLO FROM THE FIRMWARE\nA VERY LONG LINE THAT HAS TO BE BROKEN ACROSS SEVERAL ROWS OF THE PANEL"); u.console_n = (int)strlen(u.console);
            u.midi_note = 60; u.midi_vel = 100; u.midi_log_n = 6; u.midi_log[0] = 0x90; u.rec_on = 1; u.rec_frames = 44100 * 5;
            for (int i = 0; i < PX_SCOPE; i++) u.scope[i] = (int16_t)(i * 100 - 12000);
            u.ntools = 8; for (int i = 0; i < 8; i++) snprintf(u.tools[i], PX_NAME, "  TOOL %d", i);
            snprintf(u.confirm, sizeof u.confirm, "Really?");
            px_draw(canvas, mode, &u);
            int s = px_scale(mode), ok = 1;
            for (int y = 0; y < FM_H * s && ok; y++)
                for (int x = 0; x < FM_W * s; x++)
                    if (canvas[y * PX_W + x] != (0xFF000000u | u.fb[(y / s) * FM_W + x / s])) { ok = 0; break; }
            CHECK(ok);
            CHECK(canvas[0] == (0xFF000000u | u.fb[0]));
            CHECK(canvas[(FM_H * s - 1) * PX_W + FM_W * s - 1] == (0xFF000000u | u.fb[FM_H * FM_W - 1]));
            PxRect co = px_content_rect(mode);
            long lit = 0;
            for (int y = co.y; y < co.y + co.h; y++) for (int x = co.x; x < co.x + co.w; x++) lit += canvas[y * PX_W + x] != C_BG;
            CHECK(lit > 500);                                                /* the panel drew something */
            if (!ok) fprintf(stderr, "  (panel %d, mode %d)\n", panel, mode);
        }
    /* the font has every printable ASCII it claims */
    for (int ch = '!'; ch <= '~'; ch++) {
        if (strchr("`{}$", ch)) continue;
        CHECK(px_glyph_bits(ch) != 0);
    }
    CHECK(px_glyph_bits('a') == px_glyph_bits('A'));
    /* text colours pass 4.5:1 on the panel, the plain background and the highlight */
    const uint32_t fg[] = {C_TEXT, C_DIM, C_HEAD, C_OK, C_WARN, C_ERR};
    for (unsigned i = 0; i < sizeof fg / sizeof *fg; i++) { CHECK(px_contrast(fg[i], C_PANEL) >= 4.5); CHECK(px_contrast(fg[i], C_BG) >= 4.5); }
    CHECK(px_contrast(C_TEXT, C_SEL) >= 4.5 && px_contrast(C_BG, C_KEYDN) >= 4.5);
    /* the hot-plug toast: colours pass 4.5:1; history is not replayed; a change flashes 3 s; a failure stays and its key is used up */
    CHECK(px_contrast(0xFF000000u, TOAST_INFO_BG) >= 4.5 && px_contrast(0xFFFFFFFFu, TOAST_ERR_BG) >= 4.5);
    {
        char tp[320];
        snprintf(tp, sizeof tp, "%s/fm1ntt-toast-%d", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp", (int)getpid());
        FILE *tf = fopen(tp, "wb");
        CHECK(tf != NULL);
        if (tf) {
            fputs("100\tinfo\tOld news\n", tf);
            fclose(tf);
            Toast t;
            toast_init(&t, tp, 1000);
            CHECK(!toast_poll(&t, 1000) && !t.has && !toast_active(&t));              /* what was there at start is history */
            tf = fopen(tp, "ab");
            fputs("200\tinfo\tHeadphones connected\n300\tinfo\tUSB audio connected: Dac\ngarbage\n400\tbad\tx\n", tf);
            fclose(tf);
            CHECK(toast_poll(&t, 2000) && t.has && !strcmp(t.cur.text, "Headphones connected") && t.n == 1);
            CHECK(!toast_key(&t, 2100) && t.has);                                      /* a change never consumes a key */
            toast_poll(&t, 4999); CHECK(!strcmp(t.cur.text, "Headphones connected"));
            toast_poll(&t, 5000); CHECK(t.has && !strcmp(t.cur.text, "USB audio connected: Dac") && t.until == 8000);
            toast_poll(&t, 8000); CHECK(!t.has && !toast_active(&t));
            tf = fopen(tp, "ab");
            fputs("500\tinfo\tOne\n600\terror\tA USB device did not start\n", tf);
            fclose(tf);
            toast_poll(&t, 9000);
            CHECK(t.has && t.cur.error && t.n == 0);                                   /* the failure replaced the change on show */
            toast_poll(&t, 60000); CHECK(t.has && t.cur.error);                       /* and stays */
            CHECK(toast_key(&t, 60001) && !t.has);                                     /* the key is used up */
            App ta;
            memset(&ta, 0, sizeof ta);
            ta.toast = t;
            toast_add(&ta.toast, "Failure", 1, 70000);
            SDL_Event any = {.type = SDL_JOYBUTTONDOWN};
            any.jbutton.button = 8;
            dispatch(&ta, &any);                                                       /* the press clears it and does nothing else */
            CHECK(!ta.toast.has && ta.pad_sent[8] == 0 && ta.panel == PANEL_MACHINE);
            for (int i = 0; i < 12; i++) { char x[16]; snprintf(x, sizeof x, "c%d", i); toast_add(&t, x, 0, 80000); }
            toast_add(&t, "Late failure", 1, 80000);
            CHECK(t.has && t.cur.error && t.n <= TOAST_Q && t.n >= TOAST_Q - 1);      /* 8 changes at most; the failure shows first */
            uint32_t *cv = calloc((size_t)PX_W * PX_H, 4);
            if (cv) { toast_draw(&t, cv); CHECK(cv[5 * PX_W + 3] == TOAST_ERR_BG && cv[20 * PX_W + 3] == 0); free(cv); }
            remove(tp);
        }
    }
    /* the carousel: Fn goes round on the pad, Home and End both ways on a keyboard; L1 and R1 together are the menu */
    CHECK(panel_step(PANEL_MACHINE, -1) == PANEL_TOOLS && panel_step(PANEL_TOOLS, 1) == PANEL_MACHINE && panel_step(PANEL_CPU, 1) == PANEL_MEMORY);
    SDL_Event l1 = {.type = SDL_JOYBUTTONDOWN}, r1 = {.type = SDL_JOYBUTTONDOWN}, fn = {.type = SDL_JOYBUTTONDOWN}, tab = {.type = SDL_KEYDOWN}, home = {.type = SDL_KEYDOWN}, end = {.type = SDL_KEYDOWN};
    l1.jbutton.button = PAD_L1; r1.jbutton.button = PAD_R1; fn.jbutton.button = PAD_FN; tab.key.keysym.sym = SDLK_TAB; home.key.keysym.sym = SDLK_HOME; end.key.keysym.sym = SDLK_END;
    CHECK(is_carousel(&fn) == 1 && is_carousel(&home) == -1 && is_carousel(&end) == 1 && is_carousel(&l1) == 0 && is_carousel(&r1) == 0);
    CHECK(is_toggle(&tab) && !is_toggle(&fn) && !is_toggle(&l1));
    {
        App p;
        memset(&p, 0, sizeof p);
        p.panel = PANEL_CPU;
        dispatch(&p, &fn); CHECK(p.panel == PANEL_MEMORY);
        dispatch(&p, &l1); CHECK(p.panel == PANEL_MEMORY);                /* L1 alone does nothing */
        dispatch(&p, &r1); CHECK(p.panel == PANEL_TOOLS && p.menu_back == PANEL_MEMORY);   /* both: the menu */
        dispatch(&p, &r1); CHECK(p.panel == PANEL_MEMORY);                /* both again: back */
        l1.type = r1.type = fn.type = SDL_JOYBUTTONUP; dispatch(&p, &l1); dispatch(&p, &r1); dispatch(&p, &fn); CHECK(p.pad_mask == 0);
        fn.type = SDL_JOYBUTTONDOWN;
        l1.type = r1.type = SDL_JOYBUTTONDOWN;
    }
    /* the controls map to the matrix */
    CHECK(note_of_key('z') == 0 && note_of_key('/') == 16 && note_of_key('q') == 12 && note_of_key('o') == 26 && note_of_key('1') == -1);
    CHECK(button_of_key(SDLK_SPACE) == 12 && button_of_key(SDLK_F12) == 13 && button_of_key(SDLK_a) == -1);
    CHECK(act_of_pad(0) == A_BACK && act_of_pad(4) == A_NONE);
    /* the pad on MACHINE: notes of the scale (C major: up C, left D, down E, right F, Y G, B A, A B, X C), modifiers, sticks */
    {
        const int pad_btn[8] = {8, 10, 9, 11, 3, 0, 1, 2}, major[8] = {0, 2, 4, 5, 7, 9, 11, 12};
        for (int d = 0; d < 8; d++) CHECK(degree_of_pad(pad_btn[d]) == d && note_id_of_degree(0, 0, d) == FM1_KEY_BASE + 7 + major[d]);
        CHECK(note_id_of_degree(1, 0, 2) == FM1_KEY_BASE + 7 + 3 && note_id_of_degree(0, 2, 0) == FM1_KEY_BASE + 9 && note_id_of_degree(0, KEY_MIN, 0) == FM1_KEY_BASE + 1);
        CHECK(note_id_of_degree(4, 0, 5) == FM1_KEY_BASE + 7 + 12 && note_id_of_degree(6, 5, 7) == FM1_KEY_BASE + 19);
        CHECK(note_id_of_degree(4, 5, 7) == -1 || note_id_of_degree(4, 5, 7) <= FM1_KEY_BASE + 26);
        for (int sc = 0; sc < NSCALES; sc++) for (int d = 0; d < 8; d++) for (int k = KEY_MIN; k <= KEY_MAX; k++) { int id = note_id_of_degree(sc, k, d); CHECK(id == -1 || (id >= FM1_KEY_BASE && id < 41)); }
        CHECK(!strcmp(key_name(0), "C") && !strcmp(key_name(-1), "B") && !strcmp(key_name(5), "F"));
        CHECK(pad_mod(0) == MOD_NONE && pad_mod(1u << PAD_L1) == MOD_L1 && pad_mod(1u << PAD_L2) == MOD_L2 && pad_mod(1u << PAD_R2) == MOD_R2 && pad_mod(1u << PAD_R1) == MOD_NONE);
        /* L2: SEL FX HOME SAVE ENV LFO ARP SEQ; R2: GLO EDIT PLAY REC; L1: GLO on X, OP1-OP6 and PIT have no button */
        CHECK(button_of_pad_mod(MOD_L2, 10) == 2 && button_of_pad_mod(MOD_L2, 9) == 8 && button_of_pad_mod(MOD_L2, 11) == 9 && button_of_pad_mod(MOD_L2, 3) == 4 &&
              button_of_pad_mod(MOD_L2, 0) == 5 && button_of_pad_mod(MOD_L2, 1) == 10 && button_of_pad_mod(MOD_L2, 2) == 11 && button_of_pad_mod(MOD_L2, 8) == -2);
        CHECK(button_of_pad_mod(MOD_R2, 8) == 7 && button_of_pad_mod(MOD_R2, 10) == 6 && button_of_pad_mod(MOD_R2, 9) == 12 && button_of_pad_mod(MOD_R2, 11) == 13 && button_of_pad_mod(MOD_R2, 2) == -1);
        for (int f = 0; f < 10; f++) CHECK(function_of_pad(f < 8 ? pad_btn[f] : f == 8 ? PAD_SELECT : PAD_START) == f && FUNCTION_KEYS[f] >= 0 && FUNCTION_KEYS[f] < 27);
        CHECK(function_of_pad(4) == -1 && function_of_pad(PAD_FN) == -1);
        /* a stick: nothing inside the dead zone, clicks past it, the sign follows the direction, the remainder is kept */
        float acc = 0;
        CHECK(stick_clicks(&acc, 0.3f, 1000) == 0 && stick_clicks(&acc, 1.0f, 1000) == 12 && stick_clicks(&acc, -1.0f, 1000) == -12);
        acc = 0;
        int sum = 0;
        for (int i = 0; i < 100; i++) sum += stick_clicks(&acc, 1.0f, 10);
        CHECK(sum == 12);
        /* the whole path: notes press and release by the id they sent, modifiers, the key, the scale, the master level */
        App p;
        memset(&p, 0, sizeof p);
        p.panel = PANEL_MACHINE;
        p.m = fm1ntt_new();
        SDL_Event ev = {.type = SDL_JOYBUTTONDOWN};
        ev.jbutton.button = 8; dispatch(&p, &ev);
        CHECK(p.pad_sent[8] == FM1_KEY_BASE + 7 + 1);   /* the id sent, plus one (no firmware: the core ignores the key itself) */
        l1.type = SDL_JOYBUTTONDOWN; dispatch(&p, &l1);
        ev.type = SDL_JOYBUTTONUP; dispatch(&p, &ev);                                   /* released after L1 went down: still the same key */
        CHECK(p.pad_sent[8] == 0);
        ev.type = SDL_JOYBUTTONDOWN; ev.jbutton.button = PAD_START; dispatch(&p, &ev);    /* L1 + Start: POLY, the black key D#5 */
        CHECK(p.key_shift == 0 && p.pad_sent[PAD_START] == FM1_KEY_BASE + 22 + 1);
        ev.type = SDL_JOYBUTTONUP; dispatch(&p, &ev);
        CHECK(p.pad_sent[PAD_START] == 0);
        ev.type = SDL_JOYBUTTONDOWN; ev.jbutton.button = PAD_FN; dispatch(&p, &ev);       /* L1 + Fn: next scale, the panel stays */
        CHECK(p.scale == 1 && p.panel == PANEL_MACHINE);
        ev.type = SDL_JOYBUTTONUP; dispatch(&p, &ev);
        l1.type = SDL_JOYBUTTONUP; dispatch(&p, &l1);
        SDL_Event l2 = {.type = SDL_JOYBUTTONDOWN};
        l2.jbutton.button = PAD_L2; dispatch(&p, &l2);                                    /* L2 + Start: key up */
        ev.type = SDL_JOYBUTTONDOWN; ev.jbutton.button = PAD_START; dispatch(&p, &ev);
        CHECK(p.key_shift == 1 && p.pad_sent[PAD_START] == 0);
        ev.type = SDL_JOYBUTTONUP; dispatch(&p, &ev);
        l2.type = SDL_JOYBUTTONUP; dispatch(&p, &l2);
        ev.type = SDL_JOYBUTTONDOWN;
        ev.jbutton.button = PAD_START; dispatch(&p, &ev);                                 /* Start alone: OCT+ */
        CHECK(p.pad_sent[PAD_START] == 2);
        ev.type = SDL_JOYBUTTONUP; dispatch(&p, &ev);
        CHECK(p.pad_sent[PAD_START] == 0 && !p.paused);
        p.pad_mask = 0; p.axis[0] = 1.0f; p.axis_t = 1000; for (int i = 1; i <= 5; i++) sticks_tick(&p, 1000 + 200 * i); CHECK(p.enc_turns[2] == 12);   /* knob 1 */
        p.axis[0] = 0; p.axis[1] = -1.0f; for (int i = 1; i <= 5; i++) sticks_tick(&p, 2000 + 200 * i); CHECK(p.enc_turns[3] == 12);                      /* knob 2: up is clockwise */
        p.pad_mask = 1u << PAD_L1; p.axis[1] = 0; p.axis[3] = -1.0f; for (int i = 1; i <= 5; i++) sticks_tick(&p, 3000 + 200 * i); CHECK(p.enc_turns[1] == 12); /* L1: algorithm */
        p.axis[3] = 0; p.axis[0] = -1.0f; for (int i = 1; i <= 5; i++) sticks_tick(&p, 4000 + 200 * i); CHECK(p.master_att == 24);                        /* L1: master down */
        fm1ntt_free(p.m);
    }

    /* the app's folder is a temporary one for the rest, so the files written are real and nothing else is touched */
    char tmpl[300];
    snprintf(tmpl, sizeof tmpl, "%s/fm1ntt-selftest-XXXXXX", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp");
    char *root = mkdtemp(tmpl);
    CHECK(root != NULL);
    if (!root) { fprintf(stderr, "fm1ntt selftest: no temporary folder\n"); return 1; }
    setenv("FM1NTT_DIR", root, 1);
    static App a;
    memset(&a, 0, sizeof a);
    g_quiet_logs = 1;
    a.m = fm1ntt_new();
    a.midi_note = 60; a.midi_vel = 100; a.slot = 1;
    /* the firmware folder: the right extensions, sorted, a Quit row last, a garbage file refused with a reason, a raw file booted */
    char fdir[300], p[400];
    sub_path(fdir, sizeof fdir, "firmware", NULL);
    mkdir_p(fdir);
    uint8_t junk[4096];
    memset(junk, 0xAA, sizeof junk);
    snprintf(p, sizeof p, "%s/b.bin", fdir); write_atomic(p, junk, sizeof junk);
    snprintf(p, sizeof p, "%s/A.FWSC", fdir); write_atomic(p, junk, 64);
    snprintf(p, sizeof p, "%s/notes.txt", fdir); write_atomic(p, "x", 1);
    snprintf(p, sizeof p, "%s/empty.fwsc", fdir); write_atomic(p, "", 0);
    char nob[300]; snprintf(nob, sizeof nob, "%s/no-bundled", root); mkdir_p(nob);
    setenv("FM1NTT_BUNDLED", nob, 1);                                         /* none, for the user-folder tests */
    setenv("FM1NTT_PACK", nob, 1);
    scan_files(&a);
    CHECK(a.nfiles == 3 && !strcasecmp(a.files[0], "A.FWSC") && !strcasecmp(a.files[1], "b.bin") && !strcasecmp(a.files[2], "empty.fwsc"));
    a.panel = PANEL_FILES;
    a.file_sel = 0; act(&a, A_OK);
    CHECK(!fm1ntt_loaded(a.m) && a.msg_style == 3 && a.msg[0]);                /* A.FWSC is garbage: refused with a reason */
    a.file_sel = 2; act(&a, A_OK);
    CHECK(!fm1ntt_loaded(a.m) && a.msg_style == 3);                            /* empty file: refused */
    a.file_sel = 1; act(&a, A_OK);
    CHECK(fm1ntt_loaded(a.m) && !strcmp(a.file, "b.bin") && a.msg_style == 1); /* the .bin boots as a raw image */
    act(&a, A_UP); CHECK(a.file_sel == 0);
    act(&a, A_UP); CHECK(a.file_sel == 3);                                     /* wraps to the Quit row */
    act(&a, A_DOWN); CHECK(a.file_sel == 0);
    a.file_sel = 3; act(&a, A_OK); CHECK(a.quit == 1); a.quit = 0;
    /* the CPU view and the memory view */
    UiState *ui = calloc(1, sizeof *ui);
    a.panel = PANEL_CPU; a.mode = MODE_STANDARD;
    fill_ui(&a, ui, 0);
    CHECK(ui->ncpu >= 1 && (!strcmp(ui->run_state, "RUN") || !strcmp(ui->run_state, "HALT")));
    CHECK(ui->nfiles == 4 && !strcmp(ui->files[3], "   [ QUIT ]") && ui->files[1][0] == '*');
    a.cpu_sel = 0; act(&a, A_LEFT); CHECK(a.cpu_sel == 1); act(&a, A_UP); CHECK(a.cpu_sel == 0);
    a.panel = PANEL_MEMORY; a.mem_region = 1;
    fill_ui(&a, ui, 0);
    CHECK(ui->mem_n == 256 && ui->mem[0] == 0xAA && ui->mem_base == 0x02000000u);
    act(&a, A_UP); CHECK(a.mem_off == 0);                                      /* clamped at the start */
    act(&a, A_DOWN); CHECK(a.mem_off == 8);
    act(&a, A_R2); CHECK(a.mem_off == 264);
    for (int i = 0; i < 100; i++) act(&a, A_Y);
    uint32_t base = 0; size_t size = 0;
    fm1ntt_region(a.m, 1, &base, &size);
    CHECK(a.mem_off + 8 <= size && a.mem_off % 8 == 0);                        /* clamped at the end */
    act(&a, A_RIGHT); CHECK(a.mem_region == 2 && a.mem_off == 0);
    act(&a, A_LEFT); act(&a, A_LEFT); CHECK(a.mem_region == 0);
    a.mode = MODE_ZOOMED; fill_ui(&a, ui, 0); CHECK(ui->mem_n == 128); a.mode = MODE_STANDARD;
    /* the MIDI panel: note on and off, panic, bounded log */
    a.panel = PANEL_MIDI;
    act(&a, A_RIGHT); CHECK(a.midi_note == 61);
    act(&a, A_UP); CHECK(a.midi_note == 73);
    for (int i = 0; i < 20; i++) act(&a, A_Y);
    CHECK(a.midi_vel == 127);
    act(&a, A_R2); CHECK(a.midi_chan == 1);
    midi_ok(&a, 1); CHECK(a.midi_held == 1 && a.midi_log_n == 3 && a.midi_log[0] == 0x91 && a.midi_log[1] == 73);
    midi_ok(&a, 1); CHECK(a.midi_log_n == 3);                                  /* no double note on */
    midi_ok(&a, 0); CHECK(a.midi_held == 0 && a.midi_log[3] == 0x81 && a.midi_log[5] == 0);
    for (int i = 0; i < 40; i++) { midi_ok(&a, 1); midi_ok(&a, 0); }
    CHECK(a.midi_log_n == PX_MIDI_LOG);
    act(&a, A_BACK); CHECK(a.msg_style == 1);                                  /* panic */
    /* the tools: pause, a confirmed reset, slots (save, refuse an empty one, restore), the screenshot */
    a.panel = PANEL_TOOLS;
    a.tool_sel = 0; act(&a, A_OK); CHECK(a.paused == 1); act(&a, A_OK); CHECK(a.paused == 0);
    a.tool_sel = 1; act(&a, A_OK); CHECK(a.confirm == CF_RESET);
    fill_ui(&a, ui, 0); CHECK(ui->confirm[0]);
    act(&a, A_DOWN); CHECK(a.confirm == CF_RESET);                             /* nothing else moves while asking */
    act(&a, A_BACK); CHECK(a.confirm == CF_NONE);
    a.enc_turns[0] = 5; a.tool_sel = 1; act(&a, A_OK); act(&a, A_OK);
    CHECK(a.confirm == CF_NONE && a.enc_turns[0] == 0 && a.msg_style == 1);
    a.tool_sel = 6; act(&a, A_OK); CHECK(a.confirm == CF_NONE && a.msg_style == 2);   /* slot 1 is empty */
    act(&a, A_RIGHT); CHECK(a.slot == 2); act(&a, A_LEFT); act(&a, A_LEFT); CHECK(a.slot == 4);
    a.slot = 1; a.tool_sel = 5; act(&a, A_OK);
    char sp[320]; slot_path(&a, sp, sizeof sp);
    CHECK(a.msg_style == 1 && is_file(sp));
    struct stat st; CHECK(stat(sp, &st) == 0 && (size_t)st.st_size == fm1ntt_flash_size());
    char tmpf[340]; snprintf(tmpf, sizeof tmpf, "%s.part", sp); CHECK(!is_file(tmpf));    /* no half-written file left */
    a.tool_sel = 6; act(&a, A_OK); CHECK(a.confirm == CF_RESTORE); act(&a, A_OK); CHECK(a.msg_style == 1 && fm1ntt_loaded(a.m));
    a.tool_sel = 7; act(&a, A_OK);
    char sdir[300]; sub_path(sdir, sizeof sdir, "screenshots", NULL);
    DIR *d = opendir(sdir); int shots = 0; struct dirent *e;
    while (d && (e = readdir(d))) shots += has_ext(e->d_name, ".bmp");
    if (d) closedir(d);
    CHECK(shots == 1);
    a.tool_sel = 3; act(&a, A_OK); CHECK(fm1ntt_jit_on(a.m) == fm1ntt_jit_available()); act(&a, A_OK); CHECK(fm1ntt_jit_on(a.m) == 0);
    a.tool_sel = 11; act(&a, A_OK); CHECK(a.pace_mode == 1 && fm1ntt_pace_level(a.m) == 1);
    act(&a, A_OK); act(&a, A_OK); CHECK(a.pace_mode == 3 && fm1ntt_pace_level(a.m) == 3);
    act(&a, A_OK); CHECK(a.pace_mode == PACE_AUTO && fm1ntt_pace_level(a.m) == 0);   /* AUTO starts exact */
    a.speed = 0.5; pace_auto(&a); pace_auto(&a); CHECK(a.pace_lvl == 2 && fm1ntt_pace_level(a.m) == 2);   /* behind: up a level a second */
    a.speed = 1.5; pace_auto(&a); CHECK(a.pace_lvl == 2);                                                 /* keeping up: stays */
    a.speed = 5.0; pace_auto(&a); pace_auto(&a); pace_auto(&a); CHECK(a.pace_lvl == 0 && fm1ntt_pace_level(a.m) == 0);   /* plenty to spare: back to exact */
    act(&a, A_OK); CHECK(a.pace_mode == 0 && fm1ntt_pace_level(a.m) == 0);
    a.speed = 0;
    /* the recorder: the ring to a real WAV, header sizes right, renamed from .part, a quit while recording asks first */
    a.panel = PANEL_SOUND;
    a.rec_ring = malloc((size_t)REC_RING_FRAMES * 2 * sizeof(int16_t));
    act(&a, A_OK); CHECK(a.rec != NULL && a.rec_on);
    for (int i = 0; i < 4000; i++) a.rec_ring[i] = (int16_t)i;
    SDL_AtomicSet(&a.rec_w, 4000);
    rec_pump(&a); CHECK(a.rec_frames == 2000);
    /* a quit while recording asks first, and stopping saves the file */
    a.panel = PANEL_FILES; a.file_sel = a.nfiles;
    act(&a, A_OK); CHECK(a.confirm == CF_QUIT && !a.quit);
    act(&a, A_BACK); CHECK(a.confirm == CF_NONE && a.rec != NULL && !a.quit);
    act(&a, A_OK); act(&a, A_OK); CHECK(a.quit == 1 && a.rec == NULL);
    char rdir[300]; sub_path(rdir, sizeof rdir, "recordings", NULL);
    d = opendir(rdir);
    int wavs = 0, parts = 0;
    char wavp[400] = "";
    while (d && (e = readdir(d))) {
        if (has_ext(e->d_name, ".wav")) { wavs++; snprintf(wavp, sizeof wavp, "%s/%s", rdir, e->d_name); }
        if (has_ext(e->d_name, ".part")) parts++;
    }
    if (d) closedir(d);
    CHECK(wavs == 1 && parts == 0);
    size_t wn = 0; char why[64];
    uint8_t *w = wavs ? read_file(wavp, &wn, why, sizeof why) : NULL;
    CHECK(w && wn == 44 + 8000 && !memcmp(w, "RIFF", 4) && !memcmp(w + 8, "WAVEfmt ", 8) && !memcmp(w + 36, "data", 4));
    if (w && wn >= 44) { uint32_t ds, rs; memcpy(&rs, w + 4, 4); memcpy(&ds, w + 40, 4); CHECK(ds == 8000 && rs == 36 + 8000); int16_t s1; memcpy(&s1, w + 44 + 2 * 3, 2); CHECK(s1 == 3); }
    free(w);
    /* the Benchmark report: fixed workloads, one BENCH line each, a platform header, `end` last, nothing half-written left */
    {
        char res[200] = "";
        CHECK(bench_run(NULL, 0.05, NULL, 0, res, sizeof res) == 0 && strstr(res, "Benchmark saved"));
        char bdir[300]; sub_path(bdir, sizeof bdir, "bench", NULL);
        char latest[330]; snprintf(latest, sizeof latest, "%s/latest.txt", bdir);
        size_t bn = 0; char bw[64];
        uint8_t *bt = read_file(latest, &bn, bw, sizeof bw);
        CHECK(bt != NULL);
        if (bt) {
            char *txt = malloc(bn + 1);
            memcpy(txt, bt, bn); txt[bn] = 0;
            CHECK(strstr(txt, "# fm1ntt benchmark\nformat: 1\n") == txt && strstr(txt, "\nworkload: 1\n") && strstr(txt, "\nplatform: ") && strstr(txt, "\ncpu: ") && strstr(txt, "\n== results ==\n"));
            CHECK(strstr(txt, "\nBENCH core.nop.interp: ") && strstr(txt, "\nBENCH core.boot: ") && strstr(txt, "\nBENCH core.reset: ") && strstr(txt, "\nBENCH mem.read: ") &&
                  strstr(txt, "\nBENCH screen.draw.standard: ") && strstr(txt, "\nBENCH screen.draw.zoomed: ") && strstr(txt, "\nBENCH io.flash.write: ") && strstr(txt, "\nBENCH io.flash.read: "));
            CHECK(strstr(txt, "\nend\n") && !strcmp(strstr(txt, "\nend\n"), "\nend\n"));
            int benches = 0;
            for (char *p = txt; (p = strstr(p, "\nBENCH ")); p++) benches++;
            CHECK(benches >= 9);
            double ips = 0; const char *ip = strstr(txt, "BENCH core.nop.interp: ");
            if (ip) ips = atof(ip + strlen("BENCH core.nop.interp: "));
            CHECK(ips > 0);
            free(txt);
        }
        free(bt);
        DIR *bd = opendir(bdir); int parts = 0, files = 0;
        while (bd && (e = readdir(bd))) { if (has_ext(e->d_name, ".tmp")) parts++; if (has_ext(e->d_name, ".txt")) files++; }
        if (bd) closedir(bd);
        CHECK(parts == 0 && files >= 2);
    }
    /* the Device report: r3i6nos's grammar, every section, the power windows (shortened), `end` last */
    {
        char res[240] = "";
        setenv("R3I6NOS_REPORT_POWER_S", "0.3", 1);
        CHECK(device_report(&a, "selftest", NULL, 0, res, sizeof res) == 0 && strstr(res, "Report written"));
        unsetenv("R3I6NOS_REPORT_POWER_S");
        char rp[330];
        char rbase[300]; base_dir(rbase, sizeof rbase); snprintf(rp, sizeof rp, "%s/r36s-report.txt", rbase);
        size_t rn = 0; char rw[64];
        uint8_t *rt = read_file(rp, &rn, rw, sizeof rw);
        CHECK(rt != NULL);
        if (rt) {
            char *txt = malloc(rn + 1); memcpy(txt, rt, rn); txt[rn] = 0;
            CHECK(strstr(txt, "# r3i6nos device report\napp: FM1-NTT ") == txt && strstr(txt, "\n# NEEDS: "));
            const char *secs[] = {"== system ==", "== card ==", "== core ==", "== speed ==", "== screen ==", "== sound ==", "== files ==", "== power ==", "== memory ==", "== summary =="};
            for (unsigned k = 0; k < sizeof secs / sizeof *secs; k++) CHECK(strstr(txt, secs[k]) != NULL);
            CHECK(strstr(txt, "boot a machine:") && strstr(txt, "nop loop, interpreter:") && strstr(txt, "\nPASS ") != NULL);
            CHECK(!strcmp(txt + rn - 4, "end\n"));
            free(txt);
        }
        free(rt);
        char tmpr[340]; snprintf(tmpr, sizeof tmpr, "%s.tmp", rp); CHECK(!is_file(tmpr));
    }
    /* Rec demo: the lock, the run, the files, byte-identical twice, every hero feature named, the splash first */
    {
        char runs[320]; snprintf(runs, sizeof runs, "%s/run", root); mkdir_p(runs);
        setenv("R3I6NOS_RUN", runs, 1);
        unsetenv("FM1NTT_BUNDLED");
        char fw[400];
        default_firmware(fw, sizeof fw);                                        /* a REAL firmware: the demo file the rule asks for */
        CHECK(fw[0] != 0);
        g_demo_budget = 150000;                                                 /* the tour's frames are fixed; this only makes the self-test quick */
        char dirs[2][320]; char res[240] = "";
        unsigned all = (1u << NHERO) - 1;
        snprintf(dirs[0], sizeof dirs[0], "%s/rec", root);
        snprintf(dirs[1], sizeof dirs[1], "%s/rec", root);                                    /* the same folder: its path is on a panel */
        mkdir_p(dirs[0]);
        char d0[360], d1[360];
        snprintf(d0, sizeof d0, "%s/Demo-first", dirs[0]); snprintf(d1, sizeof d1, "%s/Demo", dirs[1]);
        for (int run = 0; run < 2; run++) {
            setenv("FM1NTT_DIR", dirs[0], 1);
            CHECK(rec_demo_run(all, fw, 0, 0, NULL, NULL, res, sizeof res) == 0);
            CHECK(strstr(res, "Rec demo done") != NULL);
            CHECK(getenv("FM1NTT_DIR") && !strcmp(getenv("FM1NTT_DIR"), dirs[0]));            /* the person's folder variable is put back */
            if (run == 0) { char from[360]; snprintf(from, sizeof from, "%s/Demo", dirs[0]); CHECK(rename(from, d0) == 0); }
        }
        DIR *dd = opendir(d0); int pngs = 0, same = 1, wavs2 = 0, work_left = 0;
        while (dd && (e = readdir(dd))) {
            if (!strcmp(e->d_name, ".work")) work_left = 1;
            if (e->d_name[0] == '.') continue;
            char p0[480], p1[480];
            snprintf(p0, sizeof p0, "%s/%s", d0, e->d_name); snprintf(p1, sizeof p1, "%s/%s", d1, e->d_name);
            int is_png = has_ext(e->d_name, ".png"), is_wav = has_ext(e->d_name, ".wav");
            if (!is_png && !is_wav) continue;
            pngs += is_png; wavs2 += is_wav;
            size_t n0 = 0, n1 = 0; char w0[64];
            uint8_t *b0 = read_file(p0, &n0, w0, sizeof w0);
            uint8_t *b1 = is_wav ? NULL : read_file(p1, &n1, w0, sizeof w0);
            if (is_wav) {                                                    /* the stamp is in a wav's name: find the other run's */
                DIR *d2 = opendir(d1); struct dirent *e2;
                while (d2 && (e2 = readdir(d2))) if (has_ext(e2->d_name, ".wav")) { snprintf(p1, sizeof p1, "%s/%s", d1, e2->d_name); b1 = read_file(p1, &n1, w0, sizeof w0); }
                if (d2) closedir(d2);
            }
            if (!b0 || !b1 || n0 != n1 || memcmp(b0, b1, n0)) { same = 0; fprintf(stderr, "  differs: %s\n", e->d_name); }
            free(b0); free(b1);
        }
        if (dd) closedir(dd);
        CHECK(pngs >= 14 && wavs2 == 1 && same && !work_left);
        /* the splash is the first frame recorded: 01-splash.png is the splash at 100%, byte for byte */
        static uint32_t sc[PX_W * PX_H];
        draw_splash(sc, "READY", 100);
        char sp1[400], sp2[400]; snprintf(sp1, sizeof sp1, "%s/01-splash.png", d1); snprintf(sp2, sizeof sp2, "%s/ref-splash.png", root);
        CHECK(png_write(sp2, sc, PX_W, PX_H) == 0);
        size_t s1n = 0, s2n = 0; char w9[64];
        uint8_t *s1b = read_file(sp1, &s1n, w9, sizeof w9), *s2b = read_file(sp2, &s2n, w9, sizeof w9);
        CHECK(s1b && s2b && s1n == s2n && !memcmp(s1b, s2b, s1n));
        free(s1b); free(s2b);
        /* the PNG is a PNG: signature, IHDR 640x480, 8-bit RGB */
        size_t pn = 0; uint8_t *pb = read_file(sp1, &pn, w9, sizeof w9);
        CHECK(pb && pn > 900000 && !memcmp(pb, "\x89PNG\r\n\x1a\n", 8) && pb[18] == 0x02 && pb[19] == 0x80 && pb[22] == 0x01 && pb[23] == 0xE0 && pb[24] == 8 && pb[25] == 2);
        free(pb);
        /* the log */
        char lp[400]; snprintf(lp, sizeof lp, "%s/rec-demo.txt", d1);
        size_t ln = 0; uint8_t *lb = read_file(lp, &ln, w9, sizeof w9);
        CHECK(lb != NULL);
        if (lb) {
            char *txt = malloc(ln + 1); memcpy(txt, lb, ln); txt[ln] = 0;
            char vl[96]; snprintf(vl, sizeof vl, "INFO build: v%s ", fm1ntt_version());
            CHECK(strstr(txt, vl) != NULL);
            for (int i = 0; i < NHERO; i++) { char k[48]; snprintf(k, sizeof k, "INFO %s: t=", HEROES[i].id); CHECK(strstr(txt, k) != NULL); }   /* every hero feature is named by a step */
            CHECK(strstr(txt, "PASS frames:") != NULL && strstr(txt, "PASS length:") != NULL);
            CHECK(strstr(txt, "\nFAIL ") == NULL);          /* no ffmpeg in a test run is the one allowed FAIL */
            CHECK(ln >= 4 && !strcmp(txt + ln - 4, "end\n"));
            free(txt);
        }
        free(lb);
        char dn[400]; snprintf(dn, sizeof dn, "%s/demo/done", runs);
        size_t dnn = 0; uint8_t *db = read_file(dn, &dnn, w9, sizeof w9);
        CHECK(db && dnn >= 6 && !memcmp(db, "exit=0", 6));
        free(db);
        char lk[400]; snprintf(lk, sizeof lk, "%s/demo/lock", runs); CHECK(!is_file(lk));      /* the lock is gone */
        /* a live holder's lock refuses a second recording; a dead one's is taken over */
        char lockbody[32]; snprintf(lockbody, sizeof lockbody, "%ld\n", (long)getpid());
        write_atomic(lk, lockbody, strlen(lockbody));
        setenv("FM1NTT_DIR", dirs[0], 1);
        CHECK(rec_demo_run(all, NULL, 0, 0, NULL, NULL, res, sizeof res) == -1 && strstr(res, "already recording"));
        write_atomic(lk, "999999\n", 7);
        CHECK(rec_demo_run(1u << 1, NULL, 0, 0, NULL, NULL, res, sizeof res) == 0);
        /* without a firmware the firmware feature is SKIPped with a line, and the others record */
        lp[0] = 0; snprintf(lp, sizeof lp, "%s/Demo/rec-demo.txt", dirs[0]);
        lb = read_file(lp, &ln, w9, sizeof w9);
        CHECK(lb != NULL);
        free(lb);
        setenv("FM1NTT_DIR", dirs[0], 1);
        CHECK(rec_demo_run(all, NULL, 0, 0, NULL, NULL, res, sizeof res) == 0);
        lb = read_file(lp, &ln, w9, sizeof w9);
        if (lb) { char *txt = malloc(ln + 1); memcpy(txt, lb, ln); txt[ln] = 0; CHECK(strstr(txt, "SKIP firmware:") != NULL); free(txt); }
        free(lb);
        g_demo_budget = 0;
        setenv("FM1NTT_DIR", root, 1);
        unsetenv("R3I6NOS_RUN");
    }
    /* the firmware that ships with the app: found beside the program, listed after the user's, and EVERY ONE boots and draws its screen */
    {
        unsetenv("FM1NTT_BUNDLED");
        unsetenv("FM1NTT_PACK");
        char bd[300];
        bundled_dir(bd, sizeof bd);
        CHECK(bd[0] != 0);
        static App b2;
        memset(&b2, 0, sizeof b2);
        scan_files(&b2);
        int nb = 0, np = 0;
        for (int i = 0; i < b2.nfiles; i++) { nb += b2.fbundled[i] != 0; np += b2.fbundled[i] == 2; }
        char pk[300];
        pack_dir(pk, sizeof pk);
        CHECK(nb - np >= 5);                                                      /* the permissive tier: always shipped */
        CHECK(pk[0] ? np >= 3 : np == 0);                                         /* the copyleft pack: its three, or none when it is not installed */
        int booted = 0;
        for (int i = 0; i < b2.nfiles; i++) {
            if (!b2.fbundled[i]) continue;
            char why2[64]; size_t n2 = 0;
            uint8_t *fwb = read_file(b2.fpaths[i], &n2, why2, sizeof why2);
            fm1ntt_machine *bm = fm1ntt_new();
            int ok = fwb && fm1ntt_load_package(bm, fwb, n2) == 0;
            free(fwb);
            int16_t pcm2[1024];
            for (int k = 0; ok && k < 40 && !fm1ntt_halted(bm); k++) fm1ntt_render(bm, pcm2, 512, 2000000);
            static uint32_t scr[FM1_SCREEN_W * FM1_SCREEN_H];
            fm1ntt_screen(bm, scr);
            int colours = 0; uint32_t seen[16] = {0};
            for (int k = 0; k < FM1_SCREEN_W * FM1_SCREEN_H && colours < 16; k++) { int f = 0; for (int q = 0; q < colours; q++) if (seen[q] == scr[k]) f = 1; if (!f) seen[colours++] = scr[k]; }
            if (!ok || fm1ntt_halted(bm) || colours < 8) fprintf(stderr, "  bundled firmware %s: %s\n", b2.files[i], !ok ? "did not load" : fm1ntt_halted(bm) ? "halted" : "drew almost nothing");
            booted += ok && !fm1ntt_halted(bm) && colours >= 8;
            fm1ntt_free(bm);
        }
        CHECK(booted == nb);
        /* the FILES panel marks them, and the default firmware is one of them */
        a.panel = PANEL_FILES;
        memcpy(a.files, b2.files, sizeof a.files); memcpy(a.fpaths, b2.fpaths, sizeof a.fpaths); memcpy(a.fbundled, b2.fbundled, sizeof a.fbundled); a.nfiles = b2.nfiles;
        fill_ui(&a, ui, 0);
        int marked = 0;
        for (int i = 0; i < ui->nfiles; i++) marked += ui->files[i][1] == 'B' || ui->files[i][1] == 'G';
        CHECK(marked == nb && ui->bundled_dir[0]);
        char dflt[300];
        default_firmware(dflt, sizeof dflt);
        CHECK(dflt[0] && strstr(dflt, "untrue") && is_file(dflt));
        setenv("FM1NTT_BUNDLED", nob, 1);
        setenv("FM1NTT_PACK", nob, 1);
        scan_files(&a);
    }
    /* a pause makes the audio callback silent without touching the machine */
    int16_t pcm[256];
    CHECK(fm1ntt_render(a.m, pcm, 128, 100000) <= 128);
    free(ui);
    free(a.rec_ring);
    fm1ntt_free(a.m);
    /* a missing file says so */
    App b = {0};
    b.m = fm1ntt_new();
    CHECK(load_firmware(&b, "/nonexistent/x.fwsc", 0, 0) == -1 && b.msg_style == 3);
    fm1ntt_free(b.m);
    /* clean up the temporary folder (and only it) */
    char cmd[400];
    if (!getenv("FM1NTT_KEEP") && strstr(root, "fm1ntt-selftest-")) { snprintf(cmd, sizeof cmd, "rm -rf '%s'", root); if (system(cmd) != 0) fprintf(stderr, "could not remove %s\n", root); }
    if (bad) { fprintf(stderr, "fm1ntt selftest: %d failure(s)\n", bad); return 1; }
    puts("fm1ntt selftest: ok");
    return 0;
}

/* ---- main -------------------------------------------------------------------------------------------------------------- */
static int cli_progress(Rec *r, int percent, const char *doing, void *ud) {
    static int last = -10;
    (void)r; (void)ud;
    if (percent >= last + 10 || percent == 100) { printf("rec-demo: %3d%% %s\n", percent, doing); fflush(stdout); last = percent; }
    return 0;
}
static unsigned parse_features(const char *csv, int *bad) {
    unsigned m = 0;
    char *copy = strdup(csv), *save = NULL;
    for (char *t = strtok_r(copy, ",", &save); t; t = strtok_r(NULL, ",", &save)) {
        int found = 0;
        for (int i = 0; i < NHERO; i++) if (!strcmp(t, HEROES[i].id)) { m |= 1u << i; found = 1; }
        if (!found) { fprintf(stderr, "fm1ntt: unknown hero feature '%s' (", t); for (int i = 0; i < NHERO; i++) fprintf(stderr, "%s%s", i ? ", " : "", HEROES[i].id); fprintf(stderr, ")\n"); *bad = 1; }
    }
    free(copy);
    return m;
}

/* ---- the input trace: make an empty file `trace.on` in the app's folder and run the app; `trace.txt` there then holds every event and
 * every draw with milliseconds (4000 lines at most), to find a press that is handled but not shown. Off otherwise. ------------------ */
static FILE *g_trace; static int g_trace_n;
static void trace_open(void) {
    char on[320], out[320];
    sub_path(on, sizeof on, "trace.on", NULL);
    if (!is_file(on)) return;
    sub_path(out, sizeof out, "trace.txt", NULL);
    g_trace = fopen(out, "w");
}
static void trace(const char *fmt, ...) {
    if (!g_trace || g_trace_n >= 4000) { if (g_trace && g_trace_n == 4000) { fclose(g_trace); g_trace = NULL; } return; }
    va_list ap; va_start(ap, fmt);
    fprintf(g_trace, "%8u ", SDL_GetTicks());
    vfprintf(g_trace, fmt, ap);
    fputc('\n', g_trace); fflush(g_trace);
    va_end(ap);
    g_trace_n++;
}

/* SDL_WaitEventTimeout is a 1 ms poll loop on KMSDRM (SDL 2.26: ~1000 wake-ups/s, 15% of a core idle, the R36S report of
   2026-11-17). There, sleep in short slices and poll between them; elsewhere block (the key wakes it at once). */
static int wait_event(SDL_Event *e, int ms) {
    static int blocking = -1;                    /* a real blocking wait wakes on the key itself: no slice of lag (everywhere but KMSDRM) */
    if (blocking < 0) { const char *d = SDL_GetCurrentVideoDriver(); blocking = !(d && !strcasecmp(d, "KMSDRM")); }
    if (blocking) return SDL_WaitEventTimeout(e, ms);
    static uint32_t last_act;                    /* when a key last arrived */
    uint32_t end = SDL_GetTicks() + (uint32_t)ms;
    for (;;) {
        if (SDL_PollEvent(e)) { last_act = SDL_GetTicks(); return 1; }
        uint32_t now = SDL_GetTicks();
        if ((int32_t)(end - now) <= 0) return 0;
        uint32_t left = end - now;
        /* 8 ms for 3 s after a key (it feels instant), 16 ms while a firmware runs, else 40 ms: idle stays near the 25 wake-ups/s goal (R36S report, 2026-11-17) */
        uint32_t slice = now - last_act < 3000 ? 8 : ms < 100 ? 16 : 40;
        SDL_Delay(left < slice ? left : slice);
    }
}

int main(int argc, char **argv) {
    const char *path = NULL, *shot = NULL, *fwopt = NULL, *features = NULL;
    int raw = 0, zoomed = 0, panel = 0, do_report = 0, do_bench = 0, do_demo = 0, no_video = 0, no_jit = 0, pace_opt = PACE_AUTO;
    double seconds = 1.0;
    uint32_t entry = FM1_ENTRY_FELUCCA;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--version")) { printf("fm1ntt %s (%s)\n", fm1ntt_version(), FM1NTT_BUILD); return 0; }
        else if (!strcmp(argv[i], "--selftest")) return selftest();
        else if (!strcmp(argv[i], "--entry") && i + 1 < argc) { raw = 1; entry = (uint32_t)strtoul(argv[++i], NULL, 0); }
        else if (!strcmp(argv[i], "--screenshot") && i + 1 < argc) shot = argv[++i];
        else if (!strcmp(argv[i], "--panel") && i + 1 < argc) panel = atoi(argv[++i]) % PANEL_COUNT;
        else if (!strcmp(argv[i], "--zoomed")) zoomed = 1;
        else if (!strcmp(argv[i], "--report")) do_report = 1;
        else if (!strcmp(argv[i], "--bench")) do_bench = 1;
        else if (!strcmp(argv[i], "--seconds") && i + 1 < argc) { seconds = atof(argv[++i]); if (seconds < 0.05) seconds = 0.05; if (seconds > 60) seconds = 60; }
        else if (!strcmp(argv[i], "--fw") && i + 1 < argc) fwopt = argv[++i];
        else if (!strcmp(argv[i], "--pace") && i + 1 < argc) { const char *v = argv[++i]; pace_opt = !strcmp(v, "auto") ? PACE_AUTO : atoi(v) % 4; }
        else if (!strcmp(argv[i], "--no-jit")) no_jit = 1;
        else if (!strcmp(argv[i], "--rec-demo")) do_demo = 1;
        else if (!strcmp(argv[i], "--features") && i + 1 < argc) features = argv[++i];
        else if (!strcmp(argv[i], "--no-video")) no_video = 1;
        else if (argv[i][0] != '-') path = argv[i];
        else { fprintf(stderr, "fm1ntt: unknown option %s\n", argv[i]); return 2; }
    }

    static App a;
    static UiState u;
    static uint32_t canvas[PX_W * PX_H];
    a.m = fm1ntt_new();
    a.mode = zoomed ? MODE_ZOOMED : MODE_STANDARD;
    a.panel = panel;
    a.midi_note = 60; a.midi_vel = 100; a.slot = 1;
    a.jit_pref = !no_jit && !getenv("FM1NTT_NO_JIT");     /* on wherever the CPU supports it: the interpreter cannot keep up on a handheld */
    a.pace_mode = pace_opt;
    say(&a, 0, "Pick a firmware in FILES, or give a path on the command line", NULL);

    /* the three runs with no window (over SSH): each prints what it did and exits 0 when its file is written */
    if (do_bench) {
        if (SDL_Init(SDL_INIT_TIMER) != 0) return 1;
        char res[240], dflt[300];
        default_firmware(dflt, sizeof dflt);                       /* a real firmware by default, so reports compare on a real workload */
        const char *fw = fwopt ? fwopt : path ? path : (dflt[0] ? dflt : NULL);
        int rc = bench_run(NULL, seconds, fw, raw || (fw && has_ext(fw, ".bin")), res, sizeof res);
        printf("%s\n", res);
        return rc == 0 ? 0 : 1;
    }
    if (do_demo) {
        int bad = 0;
        unsigned mask = features ? parse_features(features, &bad) : (1u << NHERO) - 1;
        if (bad || !mask) return 2;
        if (SDL_Init(SDL_INIT_TIMER) != 0) return 1;
        signal(SIGINT, on_signal);
        signal(SIGTERM, on_signal);
        char res[240], dflt[300];
        default_firmware(dflt, sizeof dflt);                       /* the demo file: a bundled firmware when none is given */
        const char *fw = path ? path : (dflt[0] ? dflt : NULL);
        int rc = rec_demo_run(mask, fw, fw && has_ext(fw, ".bin"), !no_video, cli_progress, NULL, res, sizeof res);
        printf("%s\n", res);
        return rc == 0 ? 0 : (strstr(res, "already recording") ? 75 : 1);
    }
    if (do_report) {
        if (SDL_Init(SDL_INIT_AUDIO | SDL_INIT_JOYSTICK | SDL_INIT_TIMER) != 0) return 1;
        SDL_AudioSpec want = {.freq = FM1_SAMPLE_RATE, .format = AUDIO_S16SYS, .channels = 2, .samples = AUDIO_FRAMES, .callback = audio_cb, .userdata = &a}, got;
        a.dev = SDL_OpenAudioDevice(NULL, 0, &want, &got, 0);
        char dflt[300];
        default_firmware(dflt, sizeof dflt);
        const char *fwp = path ? path : (dflt[0] ? dflt : NULL);                 /* a real firmware to measure, bundled when none is given */
        if (fwp) load_firmware(&a, fwp, raw || has_ext(fwp, ".bin"), entry);
        if (a.dev) SDL_PauseAudioDevice(a.dev, 0);
        char res[300];
        int rc = device_report(&a, "fm1ntt --report", a.fw_path[0] ? a.fw_path : NULL, a.fw_raw, res, sizeof res);
        printf("%s\n", res);
        if (a.dev) SDL_CloseAudioDevice(a.dev);
        return rc == 0 ? 0 : 1;
    }

    if (shot) {                 /* one frame to a file: no window, no sound */
        if (SDL_Init(0) != 0) return 1;
        scan_files(&a);
        if (path && load_firmware(&a, path, raw, entry) == 0) {
            int16_t pcm[2048];
            for (int i = 0; i < 60; i++) fm1ntt_render(a.m, pcm, 1024, MAX_INSNS_PER_BUFFER);
        }
        fill_ui(&a, &u, 0);
        px_draw(canvas, a.mode, &u);
        SDL_Surface *s = SDL_CreateRGBSurfaceFrom(canvas, PX_W, PX_H, 32, PX_W * 4, 0xFF0000, 0xFF00, 0xFF, 0xFF000000);
        int rc = s ? SDL_SaveBMP(s, shot) : -1;
        printf("%s: %s\n", shot, rc == 0 ? "written" : SDL_GetError());
        return rc == 0 ? 0 : 1;
    }

    if (SDL_Init(SDL_INIT_VIDEO | SDL_INIT_AUDIO | SDL_INIT_JOYSTICK) != 0) { fprintf(stderr, "fm1ntt: SDL_Init: %s\n", SDL_GetError()); return 1; }
    SDL_Window *win = SDL_CreateWindow("FM1-NTT", SDL_WINDOWPOS_UNDEFINED, SDL_WINDOWPOS_UNDEFINED, PX_W, PX_H, SDL_WINDOW_RESIZABLE);
    SDL_Renderer *ren = win ? SDL_CreateRenderer(win, -1, 0) : NULL;
    if (!ren) { fprintf(stderr, "fm1ntt: video: %s\n", SDL_GetError()); return 1; }
    SDL_SetHint(SDL_HINT_RENDER_SCALE_QUALITY, "0");
    SDL_RenderSetLogicalSize(ren, PX_W, PX_H);
    SDL_RenderSetIntegerScale(ren, SDL_TRUE);
    SDL_Texture *tex = SDL_CreateTexture(ren, SDL_PIXELFORMAT_ARGB8888, SDL_TEXTUREACCESS_STREAMING, PX_W, PX_H);
    if (!tex) { fprintf(stderr, "fm1ntt: texture: %s\n", SDL_GetError()); return 1; }
    for (int i = 0; i < SDL_NumJoysticks(); i++) SDL_JoystickOpen(i);

    draw_splash(canvas, "STARTING", 20);                 /* at once, before anything heavy */
    present(ren, tex, canvas);

    SDL_AudioSpec want = {.freq = FM1_SAMPLE_RATE, .format = AUDIO_S16SYS, .channels = 2, .samples = AUDIO_FRAMES, .callback = audio_cb, .userdata = &a}, got;
    a.dev = SDL_OpenAudioDevice(NULL, 0, &want, &got, 0);
    if (!a.dev) say(&a, 2, "No sound device: %s", SDL_GetError());
    draw_splash(canvas, "READING THE FIRMWARE FOLDER", 40);
    present(ren, tex, canvas);
    scan_files(&a);
    if (path) {
        draw_splash(canvas, "LOADING THE FIRMWARE", 60);
        present(ren, tex, canvas);
        if (load_firmware(&a, path, raw, entry) != 0) a.panel = PANEL_FILES;
    }
    trace_open();
    toast_init(&a.toast, "/run/r3i6nos-hotplug/events", SDL_GetTicks());
    if (a.dev) SDL_PauseAudioDevice(a.dev, 0);
    draw_splash(canvas, "READY", 100);                    /* the last splash frame, held for no time: the app is drawn on the next pass */
    present(ren, tex, canvas);
    SDL_PumpEvents();

    uint32_t last_draw = 0, last_settle = 0;
    int dirty = 1, settle = 0;     /* settle: re-presents of the last frame still owed (a page flip can hold a frame back until the next present) */
    for (int run = 1; run && !a.quit;) {
        SDL_Event e;
        /* idle: block, never spin; with a firmware running the panels are redrawn about ten times a second */
        int wait = settle ? 16 : sticks_active(&a) ? 16 : a.job ? 100 : fm1ntt_loaded(a.m) ? 60 : toast_active(&a.toast) ? 100 : 250;
        if (dirty ? SDL_PollEvent(&e) : wait_event(&e, wait)) {
            do {
                if (e.type == SDL_JOYBUTTONDOWN || e.type == SDL_JOYBUTTONUP) trace("event %s button %d (panel %d)", e.type == SDL_JOYBUTTONDOWN ? "down" : "up", e.jbutton.button, a.panel);
                else if (e.type == SDL_KEYDOWN || e.type == SDL_KEYUP) trace("event key %s %d", e.type == SDL_KEYDOWN ? "down" : "up", (int)e.key.keysym.sym);
                dispatch(&a, &e); dirty = 1;
            } while (SDL_PollEvent(&e));
        }
        if (sticks_active(&a)) dirty = 1;
        if (toast_poll(&a.toast, SDL_GetTicks())) dirty = 1;
        sticks_tick(&a, SDL_GetTicks());
        rec_pump(&a);
        speed_tick(&a);
        if (job_poll(&a)) dirty = 1;
        if (a.want_demo) {                                /* Rec demo owns the unit until it ends */
            a.want_demo = 0;
            char dflt[300];
            default_firmware(dflt, sizeof dflt);
            int have_fw = (fm1ntt_loaded(a.m) && a.fw_path[0]) || dflt[0];
            unsigned mask = demo_checklist(ren, tex, canvas, have_fw);
            if (mask) {
                char res[240];
                DemoWin w = {ren, tex, canvas, 0};
                int pz = a.paused;
                a.paused = 1;
                rec_demo_run(mask, fm1ntt_loaded(a.m) && a.fw_path[0] ? a.fw_path : (dflt[0] ? dflt : NULL), fm1ntt_loaded(a.m) && a.fw_path[0] ? a.fw_raw : (dflt[0] && has_ext(dflt, ".bin")), 1, demo_win_progress, &w, res, sizeof res);
                a.paused = pz;
                say(&a, strstr(res, "done") ? 1 : 2, "%s", res);
            } else say(&a, 0, "Rec demo cancelled", NULL);
            dirty = 1;
        }
        lock(&a);
        uint32_t rev = fm1ntt_lcd_revision(a.m);
        unlock(&a);
        uint32_t now = SDL_GetTicks();
        /* Redraw rates (audio comes first, and a repaint costs ~3.5 ms of the unit's time and a lock on the audio): the firmware's screen at most 15 times a
         * second, the panels and the clock 5 times (the SOUND meters 10), and at once for anything the person did. */
        uint32_t panel_ms = a.panel == PANEL_SOUND ? 100 : a.job ? 250 : 200;       /* a job's progress box moves 4 times a second: the report measures idle while it shows */
        if (dirty || (rev != a.rev && now - last_draw >= 60) || ((fm1ntt_loaded(a.m) || a.job) && now - last_draw > panel_ms)) {
            uint32_t t0 = SDL_GetTicks();
            int by_input = dirty;
            fill_ui(&a, &u, 1);
            px_draw(canvas, a.mode, &u);
            draw_job_progress(canvas, &a);
            toast_draw(&a.toast, canvas);
            present(ren, tex, canvas);
            if (dirty) trace("draw (by input) took %u ms, sel %d/%d", SDL_GetTicks() - t0, a.tool_sel, a.file_sel);
            last_draw = last_settle = now;
            dirty = 0;
            if (by_input) settle = 3;                         /* only a press needs it: the periodic redraws are followed by another soon enough */
        } else if (settle && now - last_settle >= 16) {     /* the same pixels again: it costs a texture copy, no repaint and no lock */
            present(ren, tex, canvas);
            trace("re-present");
            last_settle = now;
            settle--;
        }
    }
    rec_stop(&a);
    if (a.job) SDL_WaitThread(a.job, NULL);
    if (a.dev) SDL_CloseAudioDevice(a.dev);
    free(a.rec_ring);
    fm1ntt_free(a.m);
    SDL_DestroyTexture(tex);
    SDL_Quit();
    return 0;
}
