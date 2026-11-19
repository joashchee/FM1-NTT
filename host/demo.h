/* FM1-NTT's Rec demo (r3i6nos CLAUDE.md rule 11, docs/DEMO-RECORDINGS.md), in pixels. MIT. Included by fm1ntt.c after report.h.
 *
 * `fm1ntt --rec-demo [--features a,b] [FIRMWARE]` or TOOLS > Rec demo runs the real app through a scripted tour on a stepped clock
 * (30 frames a second, 1470 frames of audio each), driving it through the same event path as the pad, and writes to $BASE/Demo/:
 *   NN-name.png        screenshots of the frames the app drew (640 x 480, uncompressed PNG), in tour order
 *   demo-<stamp>.mp4   H.264 + AAC, 640 x 480, 30 fps, opening on the splash with the version line (needs ffmpeg: /usr/bin, then PATH)
 *   demo-<stamp>.wav   the audio the video was made from (kept; so is an earlier run's mp4: every demo-*.mp4 stays)
 *   rec-demo.txt       the log in the device report's grammar, `end` last
 * Deterministic: nothing reads a clock while recording, so two runs give the same PNGs and the same WAV (the self-test checks), and
 * the video can be remade on a computer by running the same command. The tour starts from a clean state: a temporary app folder
 * ($Demo/.work, removed at the end), so the person's slots, recordings and settings neither show nor change.
 *
 * The demo file: FM1-NTT cannot make one. A firmware is another party's (and mostly GPL), so none is bundled and none can be generated
 * in code; the hero feature "firmware" runs only when the person gives one (`--rec-demo FILE`, or the one loaded), and is SKIPped with
 * a line in the log otherwise. The other hero features need none. */

#include <signal.h>

typedef struct { const char *id, *label; } Hero;
static const Hero HEROES[] = {
    {"carousel", "The nine panels on Fn"},
    {"zoom", "Standard and Zoomed with F1 or Tab"},
    {"midi", "Send notes to the MIDI jack"},
    {"tools", "Pause, reset and flash slots"},
    {"firmware", "Boot a firmware and play it (a bundled one, or yours)"},
};
#define NHERO ((int)(sizeof HEROES / sizeof HEROES[0]))
#define DEMO_FPS 30
#define DEMO_AUDIO_FRAMES (FM1_SAMPLE_RATE / DEMO_FPS)       /* 1470 */
/* The tour's length in frames, measured by the self-test (which fails when it is out of date): the splash and the end, then each feature. */
#define DEMO_FRAMES_BASE 120
static const int DEMO_FRAMES_FEATURE[NHERO] = {394, 100, 122, 264, 335};

static volatile sig_atomic_t g_cancel;
static void on_signal(int s) { (void)s; g_cancel = 1; }

typedef struct Rec {
    App app;
    UiState *ui;
    uint32_t *canvas;
    FILE *ff;                           /* the video pipe, NULL when no video is made */
    int16_t *pcm;                       /* interleaved stereo, the whole run */
    size_t pcm_n, pcm_cap;
    unsigned mask, seen;
    R3Report log;
    char dir[300], work[320], ffmpeg[300];
    int frames, expected, shots, cancelled, tour_failed;
    int (*on_progress)(struct Rec *, int percent, const char *doing, void *);   /* nonzero: cancel */
    void *ud;
    const char *firmware;
    int firmware_raw;
} Rec;

/* ---- the lock and the status files (r3i6nos's /run/loveos/demo) ------------------------------------------------------------- */
static void run_dir(char *out, size_t n) {
    const char *e = getenv("R3I6NOS_RUN");
    snprintf(out, n, "%s/demo", e && *e ? e : "/run/loveos");
}
static void status_write(const char *name, const char *body) {
    char d[300], p[360], t[380];
    run_dir(d, sizeof d);
    snprintf(p, sizeof p, "%s/%s", d, name);
    snprintf(t, sizeof t, "%s.tmp", p);
    FILE *f = fopen(t, "w");
    if (!f) return;
    fputs(body, f);
    fclose(f);
    rename(t, p);
}
/* 0 when the lock is ours (a dead holder's is taken over), -1 when a live recording holds it. A missing run folder is no lock at all. */
static int demo_lock(void) {
    char d[300], p[360], buf[32];
    run_dir(d, sizeof d);
    mkdir_p(d);
    snprintf(p, sizeof p, "%s/lock", d);
    FILE *f = fopen(p, "r");
    if (f) {
        long pid = fgets(buf, sizeof buf, f) ? atol(buf) : 0;
        fclose(f);
        if (pid > 0 && pid != (long)getpid() ? kill((pid_t)pid, 0) == 0 : pid == (long)getpid()) return -1;
    }
    snprintf(buf, sizeof buf, "%ld\n", (long)getpid());
    status_write("lock", buf);
    return 0;
}
static void demo_unlock(void) {
    char d[300], p[360];
    run_dir(d, sizeof d);
    snprintf(p, sizeof p, "%s/lock", d);
    remove(p);
}

/* ---- PNG (RGB, uncompressed deflate blocks: no zlib needed) ---------------------------------------------------------------- */
static uint32_t crc_table[256];
static uint32_t crc32_of(uint32_t crc, const uint8_t *b, size_t n) {
    if (!crc_table[1]) for (uint32_t i = 0; i < 256; i++) { uint32_t c = i; for (int k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1; crc_table[i] = c; }
    crc = ~crc;
    for (size_t i = 0; i < n; i++) crc = crc_table[(crc ^ b[i]) & 255] ^ (crc >> 8);
    return ~crc;
}
static void be32(uint8_t *p, uint32_t v) { p[0] = (uint8_t)(v >> 24); p[1] = (uint8_t)(v >> 16); p[2] = (uint8_t)(v >> 8); p[3] = (uint8_t)v; }
static int png_chunk(FILE *f, const char *type, const uint8_t *data, size_t n) {
    uint8_t h[8];
    be32(h, (uint32_t)n);
    memcpy(h + 4, type, 4);
    uint32_t crc = crc32_of(0, h + 4, 4);
    crc = crc32_of(crc, data, n);
    uint8_t t[4];
    be32(t, crc);
    return fwrite(h, 1, 8, f) == 8 && (n == 0 || fwrite(data, 1, n, f) == n) && fwrite(t, 1, 4, f) == 4;
}
static int png_write(const char *path, const uint32_t *argb, int w, int h) {
    size_t row = 1 + (size_t)w * 3, raw = row * (size_t)h;
    uint8_t *img = malloc(raw);
    if (!img) return -1;
    for (int y = 0; y < h; y++) {
        uint8_t *r = img + (size_t)y * row;
        r[0] = 0;
        for (int x = 0; x < w; x++) { uint32_t p = argb[(size_t)y * w + x]; r[1 + x * 3] = (uint8_t)(p >> 16); r[2 + x * 3] = (uint8_t)(p >> 8); r[3 + x * 3] = (uint8_t)p; }
    }
    uint32_t a1 = 1, a2 = 0;
    for (size_t i = 0; i < raw; i++) { a1 = (a1 + img[i]) % 65521; a2 = (a2 + a1) % 65521; }
    size_t blocks = (raw + 65534) / 65535, zn = 2 + raw + blocks * 5 + 4;
    uint8_t *z = malloc(zn);
    if (!z) { free(img); return -1; }
    size_t o = 0;
    z[o++] = 0x78; z[o++] = 0x01;
    for (size_t off = 0; off < raw; off += 65535) {
        size_t n = raw - off < 65535 ? raw - off : 65535;
        z[o++] = off + n >= raw;
        z[o++] = (uint8_t)n; z[o++] = (uint8_t)(n >> 8); z[o++] = (uint8_t)~n; z[o++] = (uint8_t)(~n >> 8);
        memcpy(z + o, img + off, n);
        o += n;
    }
    be32(z + o, (a2 << 16) | a1);
    o += 4;
    char tmp[420];
    snprintf(tmp, sizeof tmp, "%s.part", path);
    FILE *f = fopen(tmp, "wb");
    int ok = f != NULL;
    if (f) {
        static const uint8_t sig[8] = {137, 80, 78, 71, 13, 10, 26, 10};
        uint8_t ihdr[13];
        be32(ihdr, (uint32_t)w); be32(ihdr + 4, (uint32_t)h);
        ihdr[8] = 8; ihdr[9] = 2; ihdr[10] = ihdr[11] = ihdr[12] = 0;
        ok = fwrite(sig, 1, 8, f) == 8 && png_chunk(f, "IHDR", ihdr, 13) && png_chunk(f, "IDAT", z, o) && png_chunk(f, "IEND", NULL, 0);
        ok = fclose(f) == 0 && ok;
    }
    free(img);
    free(z);
    if (!ok || rename(tmp, path) != 0) { remove(tmp); return -1; }
    return 0;
}

/* ---- the recorder ------------------------------------------------------------------------------------------------------------ */
static void draw_splash(uint32_t *c, const char *what, int percent);

static int rec_find_ffmpeg(char *out, size_t n) {
    if (access("/usr/bin/ffmpeg", X_OK) == 0) { snprintf(out, n, "/usr/bin/ffmpeg"); return 0; }
    const char *path = getenv("PATH");
    char *copy = path ? strdup(path) : NULL, *save = NULL;
    for (char *d = copy ? strtok_r(copy, ":", &save) : NULL; d; d = strtok_r(NULL, ":", &save)) {
        char p[400];
        snprintf(p, sizeof p, "%s/ffmpeg", d);
        if (access(p, X_OK) == 0) { snprintf(out, n, "%s", p); free(copy); return 0; }
    }
    free(copy);
    return -1;
}

/* One video frame and its audio: steps the app's emulation 1/30 s (the callback's own code) and draws what it shows. */
static void rec_frame(Rec *r, int draw_ui) {
    App *a = &r->app;
    if (r->pcm_n + DEMO_AUDIO_FRAMES * 2 > r->pcm_cap) {
        r->pcm_cap = r->pcm_cap ? r->pcm_cap * 2 : (size_t)DEMO_AUDIO_FRAMES * 2 * 600;
        r->pcm = realloc(r->pcm, r->pcm_cap * sizeof(int16_t));
    }
    if (!r->pcm) { r->tour_failed = 1; return; }
    audio_step(a, r->pcm + r->pcm_n, DEMO_AUDIO_FRAMES);
    r->pcm_n += DEMO_AUDIO_FRAMES * 2;
    if (draw_ui) {
        fill_ui(a, r->ui, 0);
        /* deterministic speed readouts: instructions over emulated time, and the emulation keeps pace by definition */
        r->ui->ips = r->ui->ncpu ? (double)r->ui->cpu[0].insns / (r->ui->ms ? (double)r->ui->ms / 1000.0 : 1.0) : 0;
        r->ui->speed = r->ui->ncpu ? 1.0 : 0;
        px_draw(r->canvas, a->mode, r->ui);
    }
    if (r->ff && fwrite(r->canvas, 4, (size_t)PX_W * PX_H, r->ff) != (size_t)PX_W * PX_H) { r->tour_failed = 1; pclose(r->ff); r->ff = NULL; }
    r->frames++;
    if (r->on_progress) {                 /* every frame: on a slow unit a frame takes long, and the cancel hold must be seen within one */
        int pct = r->expected > 0 ? r->frames * 100 / r->expected : 0;
        if (r->on_progress(r, pct > 99 ? 99 : pct, "recording", r->ud) || g_cancel) r->cancelled = 1;
    }
}
static void rec_hold(Rec *r, int n, int draw_ui) { for (int i = 0; i < n && !r->cancelled && !r->tour_failed; i++) rec_frame(r, draw_ui); }

static void rec_event(Rec *r, SDL_Event *e) { dispatch(&r->app, e); }
static void rec_joy(Rec *r, int button, int down) {
    SDL_Event e;
    memset(&e, 0, sizeof e);
    e.type = down ? SDL_JOYBUTTONDOWN : SDL_JOYBUTTONUP;
    e.jbutton.button = (Uint8)button;
    rec_event(r, &e);
}
static void rec_press(Rec *r, int button, int hold) { rec_joy(r, button, 1); rec_hold(r, hold, 1); rec_joy(r, button, 0); rec_hold(r, 4, 1); }
static void rec_key(Rec *r, SDL_Keycode k, int hold) {
    SDL_Event e;
    memset(&e, 0, sizeof e);
    e.type = SDL_KEYDOWN; e.key.keysym.sym = k;
    rec_event(r, &e);
    rec_hold(r, hold, 1);
    e.type = SDL_KEYUP;
    rec_event(r, &e);
    rec_hold(r, 3, 1);
}
static double rec_time(const Rec *r) { return (double)r->frames / DEMO_FPS; }

static void rec_shot(Rec *r, const char *name) {
    if (r->cancelled || r->tour_failed) return;
    char p[420];
    snprintf(p, sizeof p, "%s/%02d-%s.png", r->dir, ++r->shots, name);
    if (png_write(p, r->canvas, PX_W, PX_H) != 0) r3_report_line(&r->log, R3_FAIL, "screenshot", "could not write %02d-%s.png", r->shots, name);
}
static void rec_cap(Rec *r, int hero, const char *text) {
    if (r->cancelled || r->tour_failed) return;            /* a cancelled tour runs its remaining steps with no frames: they did not happen, so the log does not say they did */
    r->seen |= 1u << hero;
    r3_report_line(&r->log, R3_INFO, HEROES[hero].id, "t=%.1fs %s", rec_time(r), text);
}
#define HERO_ON(i) ((r->mask >> (i)) & 1)

/* Puts the app at a known place without a feature depending on where the last one left it. */
static void rec_place(Rec *r, int panel, int mode) {
    App *a = &r->app;
    a->panel = panel; a->mode = mode; a->confirm = CF_NONE;
}

/* The tour. Pad buttons by their raw numbers: B 0, A 1, X 2, Y 3, L1 4, R1 5, L2 6, R2 7, up 8, down 9, left 10, right 11, Start 13, Fn 16. */
static void tour(Rec *r) {
    App *a = &r->app;
    char what[96];
    /* the app's own splash, as it starts: the video opens on it */
    for (int i = 0; i < 60 && !r->cancelled; i++) {
        draw_splash(r->canvas, i < 30 ? "STARTING" : "READY", 20 + i * 80 / 59);
        rec_frame(r, 0);
        if (i == 59) rec_shot(r, "splash");
    }
    rec_place(r, PANEL_MACHINE, MODE_STANDARD);
    /* the firmware (when the person gave one) is loaded through the FILES panel, as a person would */
    if (r->firmware && HERO_ON(4)) {
        rec_place(r, PANEL_FILES, MODE_STANDARD);
        rec_hold(r, 30, 1);
        rec_shot(r, "files");
        rec_press(r, 1, 8);                                                  /* A on the first file: its page */
        rec_hold(r, 20, 1);
        rec_shot(r, "files-page");
        rec_press(r, 1, 8);                                                  /* A again: load it */
        rec_cap(r, 4, "booted the firmware from the FILES panel");
        rec_hold(r, 30, 1);
    }
    if (HERO_ON(0)) {
        rec_place(r, PANEL_MACHINE, MODE_STANDARD);
        rec_cap(r, 0, "Fn turns the carousel through every panel");
        rec_hold(r, 30, 1);
        for (int i = 1; i < PANEL_COUNT && !r->cancelled; i++) {
            rec_press(r, PAD_FN, 6);                                         /* Fn */
            rec_hold(r, 30, 1);
            snprintf(what, sizeof what, "carousel-%s", PX_PANEL_NAMES[a->panel]);
            for (char *p = what; *p; p++) *p = (char)tolower((unsigned char)*p);
            rec_shot(r, what);
        }
        rec_press(r, PAD_FN, 6);                                             /* round to MACHINE */
        rec_cap(r, 0, "L1 and R1 together open the menu");
        rec_joy(r, PAD_L1, 1); rec_joy(r, PAD_R1, 1); rec_hold(r, 6, 1); rec_joy(r, PAD_R1, 0); rec_joy(r, PAD_L1, 0); rec_hold(r, 4, 1);
        rec_hold(r, 24, 1);
    }
    if (HERO_ON(1)) {
        rec_place(r, PANEL_CPU, MODE_STANDARD);
        rec_hold(r, 24, 1);
        rec_cap(r, 1, "F1 doubles the FM-1's screen to 2x, flush top left");
        rec_key(r, SDLK_F1, 5);
        rec_hold(r, 52, 1);
        rec_shot(r, "zoomed");
        rec_cap(r, 1, "F1 again: back to 1x with the panels around it");
        rec_key(r, SDLK_F1, 5);
        rec_hold(r, 8, 1);
    }
    if (HERO_ON(2)) {
        rec_place(r, PANEL_MIDI, MODE_STANDARD);
        rec_hold(r, 20, 1);
        rec_cap(r, 2, "choose a note and send it to the emulated MIDI jack");
        rec_press(r, 11, 4); rec_press(r, 11, 4); rec_press(r, 8, 4);       /* note +2, then up an octave */
        rec_joy(r, 1, 1);
        rec_hold(r, 30, 1);
        rec_shot(r, "midi-note-on");
        rec_joy(r, 1, 0);
        rec_hold(r, 20, 1);
        rec_press(r, 2, 4);                                                  /* X: velocity down */
        rec_hold(r, 20, 1);
    }
    if (HERO_ON(3)) {
        rec_place(r, PANEL_TOOLS, MODE_STANDARD);
        a->tool_sel = 0;
        rec_hold(r, 20, 1);
        rec_cap(r, 3, "pause the emulation");
        rec_press(r, 1, 6);
        rec_hold(r, 24, 1);
        rec_shot(r, "tools-paused");
        rec_press(r, 1, 6);                                                  /* resume */
        a->tool_sel = 1;
        rec_cap(r, 3, "reset asks before it boots the firmware again");
        rec_press(r, 1, 6);
        rec_hold(r, 30, 1);
        rec_shot(r, "tools-reset-asks");
        rec_press(r, 0, 6);                                                  /* B: no */
        a->tool_sel = 5;
        rec_cap(r, 3, "save the emulated flash to a slot");
        rec_press(r, 1, 6);
        rec_hold(r, 30, 1);
        rec_shot(r, "tools-slot-saved");
        a->tool_sel = 6;
        rec_cap(r, 3, "restore it: asks first, then reboots into the saved flash");
        rec_press(r, 1, 6);
        rec_hold(r, 20, 1);
        rec_press(r, 1, 6);                                                  /* A: yes */
        rec_hold(r, 30, 1);
        a->tool_sel = 7;
        rec_cap(r, 3, "save a screenshot of the FM-1's screen");
        rec_press(r, 1, 6);
        rec_hold(r, 30, 1);
        rec_shot(r, "tools-screenshot");
    }
    if (HERO_ON(4)) {
        if (r->firmware) {
            rec_place(r, PANEL_MACHINE, MODE_STANDARD);
            rec_cap(r, 4, "play the FM-1: keys, a button and an encoder");
            static const char notes[] = "zxcvbnm";
            for (unsigned i = 0; i < sizeof notes - 1 && !r->cancelled; i++) rec_key(r, (SDL_Keycode)notes[i], 12);
            rec_press(r, 1, 10);                                             /* A: PLAY */
            rec_press(r, 11, 4); rec_press(r, 11, 4);                        /* SELECT clockwise */
            rec_hold(r, 60, 1);
            rec_shot(r, "firmware-playing");
            rec_hold(r, 36, 1);
        } else { r->seen |= 1u << 4; r3_report_line(&r->log, R3_SKIP, "firmware", "no firmware given: none can be bundled or generated in code, so this feature is not shown (run with --rec-demo FILE)"); }
    }
    rec_place(r, PANEL_MACHINE, MODE_STANDARD);
    rec_hold(r, 30, 1);
    rec_shot(r, "end");
    for (int i = 0; i < 30 && !r->cancelled; i++) { draw_splash(r->canvas, "DONE", 100); rec_frame(r, 0); }
}

/* ---- running it -------------------------------------------------------------------------------------------------------------- */
static void demo_clean(const char *dir) {
    DIR *d = opendir(dir);
    struct dirent *e;
    while (d && (e = readdir(d))) {
        const char *n = e->d_name;
        int png = strlen(n) > 7 && isdigit((unsigned char)n[0]) && isdigit((unsigned char)n[1]) && n[2] == '-' && has_ext(n, ".png");
        int own = !strcmp(n, "rec-demo.txt") || (!strncmp(n, "demo-", 5) && (has_ext(n, ".wav") || has_ext(n, ".part")));   /* every demo-*.mp4 stays */
        if (png || own) { char p[420]; snprintf(p, sizeof p, "%s/%s", dir, n); remove(p); }
    }
    if (d) closedir(d);
}
static int rmtree_demo_work(const char *p) {
    size_t n = strlen(p);
    if (n < 12 || strcmp(p + n - 11, "/Demo/.work") != 0 || strchr(p, '\'')) return -1;     /* only our own temporary folder */
    char cmd[400];
    snprintf(cmd, sizeof cmd, "rm -rf '%s'", p);
    return system(cmd);
}

/* Records the tour into `dir`/Demo. `mask`: the ticked hero features. `video`: make the mp4 (when ffmpeg exists). Returns 0 when it ran to the end. */
static int rec_demo_run(unsigned mask, const char *firmware, int firmware_raw, int video, int (*progress)(Rec *, int, const char *, void *), void *ud, char *result, size_t rn) {
    char base[300];
    base_dir(base, sizeof base);
    Rec *r = calloc(1, sizeof *r);
    if (!r) { snprintf(result, rn, "Out of memory"); return -1; }
    r->mask = mask;
    r->on_progress = progress;
    r->ud = ud;
    r->ui = calloc(1, sizeof *r->ui);
    r->canvas = calloc((size_t)PX_W * PX_H, 4);
    r->firmware = firmware && *firmware ? firmware : NULL;
    r->firmware_raw = firmware_raw;
    snprintf(r->dir, sizeof r->dir, "%s/Demo", base);
    snprintf(r->work, sizeof r->work, "%s/.work", r->dir);
    if (!r->ui || !r->canvas || mkdir_p(r->dir) != 0) { snprintf(result, rn, "Cannot make %s", r->dir); free(r->ui); free(r->canvas); free(r); return -1; }
    if (demo_lock() != 0) { snprintf(result, rn, "A Rec demo is already recording"); free(r->ui); free(r->canvas); free(r); return -1; }
    status_write("running", "FM1-NTT\n");
    status_write("progress", "step=0\nsteps=100\ndoing=starting\nfile=\n");
    SDL_DisableScreenSaver();
    g_cancel = 0;
    demo_clean(r->dir);
    /* a clean state: a temporary app folder with the firmware (when given) as its only file; the person's own folder is not read or written */
    char saved_dir[300];
    snprintf(saved_dir, sizeof saved_dir, "%s", getenv("FM1NTT_DIR") ? getenv("FM1NTT_DIR") : "");
    char fwdir[360];
    rmtree_demo_work(r->work);
    setenv("FM1NTT_DIR", r->work, 1);
    snprintf(fwdir, sizeof fwdir, "%s/firmware", r->work);
    mkdir_p(fwdir);
    if (r->firmware) {
        size_t n = 0; char why[64];
        uint8_t *fw = read_file(r->firmware, &n, why, sizeof why);
        const char *bn = strrchr(r->firmware, '/');
        char dst[460];
        snprintf(dst, sizeof dst, "%s/%s", fwdir, bn ? bn + 1 : r->firmware);
        if (!fw || write_atomic(dst, fw, n) != 0) { r->firmware = NULL; }
        free(fw);
    }
    App *a = &r->app;
    a->m = fm1ntt_new();
    mon_init(&a->mon, NULL);                   /* the tour never samples the host: the MONITOR frame is the same every time (a recording stays deterministic) */
    a->budget = g_demo_budget ? g_demo_budget : 64000000u;      /* offline: never reach the cap (at 8 million instructions a buffer the core pads the sound with silence: gaps in the demo's audio) */
    a->midi_note = 60; a->midi_vel = 100; a->slot = 1; a->mode = MODE_STANDARD;
    say(a, 0, "Pick a firmware in FILES", NULL);
    scan_files(a);
    if (r->firmware && !has_ext(r->firmware, ".fwsc") && !has_ext(r->firmware, ".bin")) r->firmware = NULL;   /* FILES lists only these */
    r->expected = DEMO_FRAMES_BASE;
    for (int i = 0; i < NHERO; i++) if ((r->mask >> i & 1) && (i != 4 || r->firmware)) r->expected += DEMO_FRAMES_FEATURE[i];

    /* the log */
    char stamp_s[32], build[96];
    stamp(stamp_s, sizeof stamp_s);
    r->log.quiet = g_quiet_logs;
    int logok = r3_report_open_named(&r->log, r->dir, "rec-demo.txt", "FM1-NTT", fm1ntt_version(), "r36s-fm1ntt", "TOOLS > Rec demo", NULL) == 0;
    snprintf(build, sizeof build, "v%s %s", fm1ntt_version(), FM1NTT_BUILD);
    r3_report_section(&r->log, "build");
    r3_report_line(&r->log, R3_INFO, "build", "%s", build);
    r3_report_line(&r->log, R3_INFO, "shows", "%s", "see the tour lines below");
    for (int i = 0; i < NHERO; i++) {
        if (HEROES[i].id && (mask >> i & 1)) { if (i == 4 && !r->firmware) { r3_report_line(&r->log, R3_SKIP, "firmware", "no firmware given"); continue; } r3_report_line(&r->log, R3_INFO, "shows", "%s: %s", HEROES[i].id, HEROES[i].label); }
        else r3_report_line(&r->log, R3_SKIP, HEROES[i].id, "not chosen");
    }
    r3_report_line(&r->log, R3_INFO, "firmware", "%s", r->firmware ? r->firmware : "none given");
    r3_report_section(&r->log, "tour");

    /* the video: frames to ffmpeg's stdin; the audio is joined afterwards from the WAV */
    char vtmp[400] = "", wavp[420], mp4[440];
    snprintf(wavp, sizeof wavp, "%s/demo-%s.wav", r->dir, stamp_s);
    snprintf(mp4, sizeof mp4, "%s/demo-%s.mp4", r->dir, stamp_s);
    int want_video = video, have_ffmpeg = rec_find_ffmpeg(r->ffmpeg, sizeof r->ffmpeg) == 0;
    if (want_video && have_ffmpeg && !strchr(r->dir, '\'') && !strchr(r->ffmpeg, '\'')) {
        snprintf(vtmp, sizeof vtmp, "%s/video.mp4", r->work);
        char cmd[1100];
        snprintf(cmd, sizeof cmd, "'%s' -y -loglevel error -f rawvideo -pix_fmt bgra -s %dx%d -r %d -i - -an -c:v libx264 -pix_fmt yuv420p -preset veryfast -crf 20 '%s' 2>'%s/ffmpeg.log'", r->ffmpeg, PX_W, PX_H, DEMO_FPS, vtmp, r->work);
        r->ff = popen(cmd, "w");
    }
    double t0 = r3_report_ms();
    if (r->on_progress) r->on_progress(r, 0, "starting", r->ud);
    g_stamp_override = "20000101-000000";                 /* what the tour names its files with: the same on every run */
    tour(r);
    g_stamp_override = NULL;
    if (r->firmware && fm1ntt_halted(a->m)) r3_report_line(&r->log, R3_FAIL, "firmware", "the emulated machine HALTED during the tour (a fault in the firmware or the emulator)");
    double rec_ms = r3_report_ms() - t0;
    int ff_ok = 1;
    if (r->ff) ff_ok = pclose(r->ff) == 0;
    r->ff = NULL;

    /* the audio, always kept */
    int wav_ok = 0;
    if (r->pcm_n) {
        FILE *f = fopen(wavp, "wb");
        if (f) {
            uint8_t h[44];
            wav_header(h, r->pcm_n / 2);
            wav_ok = fwrite(h, 1, 44, f) == 44 && fwrite(r->pcm, sizeof(int16_t), r->pcm_n, f) == r->pcm_n;
            wav_ok = fclose(f) == 0 && wav_ok;
        }
    }
    int mp4_ok = 0;
    if (r->cancelled) r3_report_line(&r->log, R3_FAIL, "cancelled", "the tour was cancelled at %.1f s: the files so far are kept", rec_time(r));
    else if (r->tour_failed) r3_report_line(&r->log, R3_FAIL, "tour", "recording failed (out of memory or the video pipe closed)");
    r3_report_section(&r->log, "files");
    r3_report_line(&r->log, r->shots > 0 ? R3_PASS : R3_FAIL, "screenshots", "%d PNG files, 640 x 480", r->shots);
    r3_report_line(&r->log, wav_ok ? R3_PASS : R3_FAIL, "audio", "%.1f s of 16-bit stereo at %d Hz in %s%s", (double)r->pcm_n / 2 / FM1_SAMPLE_RATE, FM1_SAMPLE_RATE, strrchr(wavp, '/') + 1,
                     (r->firmware && HERO_ON(4)) ? "" : " (silent: no firmware ran)");
    if (!video) r3_report_line(&r->log, R3_SKIP, "video", "not asked for (--no-video)");
    else if (!have_ffmpeg) r3_report_line(&r->log, R3_FAIL, "video", "no ffmpeg in /usr/bin or on PATH: the screenshots and the sound are kept; run the same command on a computer with ffmpeg to make the video (the tour is deterministic)");
    else if (!vtmp[0]) r3_report_line(&r->log, R3_FAIL, "video", "could not start ffmpeg");
    else if (r->cancelled) r3_report_line(&r->log, R3_INFO, "video", "not made: the run was cancelled (the screenshots and the sound so far are kept)");
    else if (!ff_ok || r->tour_failed) r3_report_line(&r->log, R3_FAIL, "video", "ffmpeg did not finish (see %s/ffmpeg.log)", r->work);
    else {
        char cmd[1300];
        snprintf(cmd, sizeof cmd, "'%s' -y -loglevel error -i '%s' -i '%s' -c:v copy -c:a aac -b:a 128k -shortest '%s' 2>>'%s/ffmpeg.log'", r->ffmpeg, vtmp, wavp, mp4, r->work);
        if (r->on_progress) r->on_progress(r, 99, "joining the sound", r->ud);
        mp4_ok = system(cmd) == 0 && is_file(mp4);
        r3_report_line(&r->log, mp4_ok ? R3_PASS : R3_FAIL, "video", "%s: H.264 + AAC, 640 x 480, %d fps, %.1f s", mp4_ok ? strrchr(mp4, '/') + 1 : "the mux failed", DEMO_FPS, (double)r->frames / DEMO_FPS);
    }
    r3_report_section(&r->log, "timing");
    r3_report_line(&r->log, rec_time(r) <= 90.0 ? R3_PASS : R3_FAIL, "length", "%.1f s of video (goal at most 90 s), %d frames (expected %d)", rec_time(r), r->frames, r->expected);
    r3_report_line(&r->log, R3_INFO, "recording", "%.1f s to record (not real time: the tour steps its own clock)", rec_ms / 1000.0);
    if (!r->cancelled && !r->tour_failed) r3_report_line(&r->log, r->frames == r->expected ? R3_PASS : R3_INFO, "frames", "%d recorded, %d expected by the table in host/demo.h", r->frames, r->expected);
    int unseen = 0;
    for (int i = 0; i < NHERO; i++) if ((r->mask >> i & 1) && !(r->seen >> i & 1)) { unseen++; r3_report_line(&r->log, R3_FAIL, HEROES[i].id, "no step of the tour named this hero feature"); }
    (void)unseen;
    if (logok) r3_report_close(&r->log);

    int rc = (r->cancelled || r->tour_failed) ? -1 : 0;
    snprintf(result, rn, rc == 0 ? "Rec demo done: Demo/ has %d screenshots, %.0f s%s" : "Rec demo stopped: see Demo/rec-demo.txt", r->shots, (double)r->frames / DEMO_FPS, mp4_ok ? ", and the video" : "");
    if (r->on_progress) r->on_progress(r, 100, "done", r->ud);
    char done[32];
    snprintf(done, sizeof done, "exit=%d\n", rc == 0 ? 0 : 1);
    status_write("done", done);
    unsigned seen = r->seen, tmask = r->mask;
    (void)seen; (void)tmask;
    free(a->rec_ring);
    fm1ntt_free(a->m);
    rmtree_demo_work(r->work);
    if (saved_dir[0]) setenv("FM1NTT_DIR", saved_dir, 1); else unsetenv("FM1NTT_DIR");
    SDL_EnableScreenSaver();
    demo_unlock();
    free(r->pcm);
    free(r->ui);
    free(r->canvas);
    free(r);
    return rc;
}

/* ---- the checklist and the progress screen (a window) ------------------------------------------------------------------------ */
static void draw_checklist(uint32_t *c, unsigned mask, int sel, int have_fw) {
    for (int i = 0; i < PX_W * PX_H; i++) c[i] = C_BG;
    PxRect b = {80, 80, 480, 300};
    px_box(c, b, "REC DEMO: WHAT TO SHOW");
    char line[120];
    for (int i = 0; i < NHERO; i++) {
        int on = mask >> i & 1, usable = i != 4 || have_fw;
        if (i == sel) px_rect(c, (PxRect){b.x + 10, b.y + 22 + i * 24 - 4, b.w - 20, 18}, C_SEL);
        snprintf(line, sizeof line, "[%c] %s", on && usable ? 'X' : ' ', HEROES[i].label);
        px_text(c, b.x + 18, b.y + 22 + i * 24, line, usable ? C_TEXT : C_DIM, 1, b.w - 36);
    }
    if (!have_fw) px_wrap(c, b.x + 18, b.y + 22 + NHERO * 24 + 6, "NO FIRMWARE IS LOADED OR BUNDLED HERE, SO THE LAST ONE IS SKIPPED. PUT ONE IN THE FIRMWARE FOLDER.", C_WARN, b.w - 36, 3);
    px_text(c, b.x + 18, b.y + b.h - 22, "UP DOWN PICK   A TICK   START BEGIN   B CANCEL", C_HEAD, 1, b.w - 36);
}
static void draw_demo_progress(uint32_t *c, int percent, const char *doing, int hold) {
    for (int i = 0; i < PX_W * PX_H; i++) c[i] = C_BG;
    px_text(c, 320 - px_text_w("REC DEMO", 3) / 2, 150, "REC DEMO", C_HEAD, 3, 0);
    px_text(c, 320 - px_text_w(doing, 1) / 2, 230, doing, C_TEXT, 1, 0);
    px_meter(c, (PxRect){170, 250, 300, 12}, percent, C_HEAD);
    char l[32];
    snprintf(l, sizeof l, "%d%%", percent);
    px_text(c, 320 - px_text_w(l, 1) / 2, 272, l, C_DIM, 1, 0);
    px_text(c, 320 - px_text_w("THE UNIT IS BUSY WITH THIS ONLY. SELECT + START HELD: CANCEL", 1) / 2, 320, "THE UNIT IS BUSY WITH THIS ONLY. SELECT + START HELD: CANCEL", C_DIM, 1, 0);
    if (hold > 0) {                                        /* the hold is being read: a bar fills in the hold's second, so the press is seen to count */
        px_text(c, 320 - px_text_w("HOLD TO CANCEL", 1) / 2, 348, "HOLD TO CANCEL", C_TEXT, 1, 0);
        px_meter(c, (PxRect){220, 362, 200, 10}, hold > 100 ? 100 : hold, C_HEAD);
    }
}

typedef struct { SDL_Renderer *ren; SDL_Texture *tex; uint32_t *canvas; uint32_t both_since; uint32_t drawn; int pct; } DemoWin;

static int demo_win_progress(Rec *r, int percent, const char *doing, void *ud) {
    DemoWin *w = ud;
    (void)r;
    SDL_Event e;
    while (SDL_PollEvent(&e)) if (e.type == SDL_QUIT || (e.type == SDL_KEYDOWN && e.key.keysym.sym == SDLK_ESCAPE)) return 1;
    int both = 0;
    SDL_Joystick *j = SDL_NumJoysticks() > 0 ? SDL_JoystickFromInstanceID(0) : NULL;
    if (j) both = SDL_JoystickGetButton(j, 12) && SDL_JoystickGetButton(j, 13);
    uint32_t now = SDL_GetTicks();
    if (!both) w->both_since = 0;
    else if (!w->both_since) w->both_since = now ? now : 1;
    int cancel = both && now - w->both_since >= 1000;
    int hold = both ? (int)((now - w->both_since) / 10) : 0;      /* 0..100 over the hold's second */
    if (hold > 0 || percent != w->pct || now - w->drawn >= 250) {   /* called every frame: the screen is drawn when it changes, the hold at once */
        draw_demo_progress(w->canvas, percent, doing, hold);
        present(w->ren, w->tex, w->canvas);
        w->drawn = now; w->pct = percent;
    }
    return cancel;
}

/* The window's Rec demo: the checklist, then the run with a progress screen. Returns after saying how it went in `result`. */
static unsigned demo_checklist(SDL_Renderer *ren, SDL_Texture *tex, uint32_t *canvas, int have_fw) {
    unsigned mask = (1u << NHERO) - 1;
    if (!have_fw) mask &= ~(1u << 4);
    int sel = 0;
    for (;;) {
        draw_checklist(canvas, mask, sel, have_fw);
        present(ren, tex, canvas);
        SDL_Event e;
        if (!SDL_WaitEvent(&e)) return 0;
        int act = A_NONE, start = 0;
        if (e.type == SDL_QUIT) return 0;
        if (e.type == SDL_KEYDOWN) { act = act_of_key(e.key.keysym.sym); if (e.key.keysym.sym == SDLK_SPACE) act = A_OK; if (e.key.keysym.sym == SDLK_ESCAPE) act = A_BACK; if (e.key.keysym.sym == SDLK_s) start = 1; }
        if (e.type == SDL_JOYBUTTONDOWN) { act = act_of_pad(e.jbutton.button); if (e.jbutton.button == 13) start = 1; }
        if (act == A_BACK) return 0;
        if (act == A_UP) sel = (sel + NHERO - 1) % NHERO;
        if (act == A_DOWN) sel = (sel + 1) % NHERO;
        if (act == A_OK && (sel != 4 || have_fw)) { unsigned bit = 1u << sel; if (!(mask & ~bit) && (mask & bit)) continue; mask ^= bit; }
        if (start && mask) return mask;
    }
}
