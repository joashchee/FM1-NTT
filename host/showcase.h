/* FM1-NTT host: the firmware showcase, a short headless demo of one firmware (ROADMAP 5d). MIT. Included by fm1ntt.c after demo.h (it uses its PNG writer).
 *
 *   fm1ntt --showcase [--entry 0xADDR] [--jit] [--out BASE] FIRMWARE
 *
 * Its own machine, no window and no SDL, on a stepped clock (30 ticks a second, 1470 frames of audio each), so the same firmware and the same tour give the
 * same bytes (the self-test checks). It writes to BASE/demos/<sha256 of the firmware>/ (BASE is the app's folder unless --out):
 *   NNNN.png      the FM-1's own 240 x 240 screen, only when it changed, at most 10 a second, at most 400 (our own uncompressed PNG, paletted when the screen has 256 colours or fewer, RGB otherwise: the app's player reads both back)
 *   audio.wav     the sound, 16-bit stereo at 44100 Hz
 *   frames.txt    "NNNN <ms>" for each picture and "end <ms>"
 *   state.txt     key=value: state (recording, ready, failed), reason, tour, frames, ms, attempts, jit; written first as recording and last as ready or failed
 *   progress.txt  pct=N while it runs
 * The tour (SHOW_TOUR 2: tour 1's audio had gaps of silence, see SHOW_BUDGET) is generic: 3 s of boot, three single notes, a chord, one turn of the SELECT encoder, a note, and a rest: 15 s. Bump SHOW_TOUR when it changes:
 * every demo made with an older one is made again.
 *
 * Change detection is the state file: a firmware with none is needed; ready and failed are not made again (a failure is never retried in a loop: rule 5) unless
 * the tour is a newer one; a run that was killed leaves "recording" and is tried once more (attempts), then counts as failed. Cancelled (SIGTERM) removes the
 * folder. A hard wall-time budget ends a run as "failed: time budget". A firmware that does not load, halts, or leaves the screen blank is "failed: <reason>".
 * Exit codes: 0 ready, 1 failed (state written), 2 could not start (nothing to key a folder by), 3 cancelled.
 * Demos made on the person's device from a file they hold are local output: never bundled, never in a package. Nothing here has run on an R36S. */
#ifndef FM1NTT_SHOWCASE_H
#define FM1NTT_SHOWCASE_H

#define SHOW_TOUR 2
#define SHOW_FPS 30
#define SHOW_AUDIO (FM1_SAMPLE_RATE / SHOW_FPS)
#define SHOW_TICKS 450
#define SHOW_FRAMES_MAX 400
#define SHOW_BUDGET 64000000u            /* instructions a tick may spend: the core stops there and pads the audio with silence, so it must never be reached (8 million was, in 46% of the ticks) */
#define SHOW_MIN_GAP 3                  /* ticks between pictures: at most 10 a second */

typedef struct { int have; char state[12], reason[96]; int tour, frames, ms, attempts, jit, shorts; } ShowState;

static int show_sha_ok(const char *s) {                    /* a path is built from it: exactly 64 lowercase hex characters */
    if (!s || strlen(s) != 64) return 0;
    for (int i = 0; i < 64; i++) if (!((s[i] >= '0' && s[i] <= '9') || (s[i] >= 'a' && s[i] <= 'f'))) return 0;
    return 1;
}
static int show_dir(char *out, size_t n, const char *base, const char *sha) {
    if (!show_sha_ok(sha)) return -1;
    int k = snprintf(out, n, "%s/demos/%s", base, sha);
    return k < 0 || (size_t)k >= n ? -1 : 0;
}

/* ---- the state file --------------------------------------------------------------------------------------------------------- */
static void show_state_parse(const char *text, ShowState *s) {
    memset(s, 0, sizeof *s);
    for (const char *line = text; line && *line; ) {
        const char *next = strchr(line, '\n');
        const char *eq = memchr(line, '=', next ? (size_t)(next - line) : strlen(line));
        if (eq) {
            size_t kn = (size_t)(eq - line), vn = next ? (size_t)(next - eq - 1) : strlen(eq + 1);
            char v[128];
            if (vn >= sizeof v) vn = sizeof v - 1;
            memcpy(v, eq + 1, vn);
            v[vn] = 0;
            if (kn == 5 && !strncmp(line, "state", 5)) { snprintf(s->state, sizeof s->state, "%.11s", v); s->have = 1; }
            else if (kn == 6 && !strncmp(line, "reason", 6)) snprintf(s->reason, sizeof s->reason, "%.95s", v);
            else if (kn == 4 && !strncmp(line, "tour", 4)) s->tour = atoi(v);
            else if (kn == 6 && !strncmp(line, "frames", 6)) s->frames = atoi(v);
            else if (kn == 2 && !strncmp(line, "ms", 2)) s->ms = atoi(v);
            else if (kn == 8 && !strncmp(line, "attempts", 8)) s->attempts = atoi(v);
            else if (kn == 3 && !strncmp(line, "jit", 3)) s->jit = atoi(v);
            else if (kn == 6 && !strncmp(line, "shorts", 6)) s->shorts = atoi(v);
        }
        line = next ? next + 1 : NULL;
    }
    if (strcmp(s->state, "recording") && strcmp(s->state, "ready") && strcmp(s->state, "failed")) { s->have = 0; s->state[0] = 0; }   /* an odd word is no state */
}
static void show_state_read(const char *dir, ShowState *s) {
    char p[420], buf[2048];
    snprintf(p, sizeof p, "%s/state.txt", dir);
    FILE *f = fopen(p, "r");
    size_t n = f ? fread(buf, 1, sizeof buf - 1, f) : 0;
    if (f) fclose(f);
    buf[n] = 0;
    show_state_parse(buf, s);
}
static int show_state_write(const char *dir, const ShowState *s) {
    char p[420], t[512];
    snprintf(p, sizeof p, "%s/state.txt", dir);
    int n = snprintf(t, sizeof t, "state=%s\nreason=%s\ntour=%d\nframes=%d\nms=%d\nattempts=%d\njit=%d\nshorts=%d\n", s->state, s->reason, s->tour, s->frames, s->ms, s->attempts, s->jit, s->shorts);
    return n > 0 && (size_t)n < sizeof t ? write_atomic(p, t, (size_t)n) : -1;
}
/* Is a demo to be made? (the sidecar's rule, above) */
static int show_needed(const ShowState *s) {
    if (!s->have) return 1;
    if (s->tour != SHOW_TOUR) return 1;
    if (!strcmp(s->state, "recording")) return s->attempts < 2;
    return 0;                                              /* ready, or failed: not again */
}

/* Removes what a showcase folder holds (only our own file names) and the folder. */
static void show_clear(const char *dir) {
    DIR *d = opendir(dir);
    struct dirent *e;
    while (d && (e = readdir(d))) {
        const char *n = e->d_name;
        size_t len = strlen(n);
        int png = len == 8 && isdigit((unsigned char)n[0]) && isdigit((unsigned char)n[1]) && isdigit((unsigned char)n[2]) && isdigit((unsigned char)n[3]) && !strcmp(n + 4, ".png");
        int own = !strcmp(n, "audio.wav") || !strcmp(n, "frames.txt") || !strcmp(n, "state.txt") || !strcmp(n, "progress.txt") || (len > 5 && !strcmp(n + len - 5, ".part"));
        if (png || own) { char p[420]; snprintf(p, sizeof p, "%s/%s", dir, n); remove(p); }
    }
    if (d) closedir(d);
    rmdir(dir);
}


/* The picture as a PNG: 8-bit paletted when it has at most 256 colours (a third of the size), else png_write's RGB. 0 when written. */
static int show_png_write(const char *path, const uint32_t *argb, int w, int h) {
    uint32_t pal[256];
    int np = 0, ok = 1;
    uint8_t *idx = malloc((size_t)w * h);
    if (!idx) return png_write(path, argb, w, h);
    static int16_t slot[4096];                               /* a hash of the colour to its palette index + 1 (open addressing) */
    static uint32_t key[4096];
    memset(slot, 0, sizeof slot);
    for (int i = 0; i < w * h && ok; i++) {
        uint32_t c = argb[i] & 0xFFFFFF;
        unsigned hsh = (c * 2654435761u) >> 20;
        while (slot[hsh] && key[hsh] != c) hsh = (hsh + 1) & 4095;
        if (!slot[hsh]) { if (np == 256) { ok = 0; break; } key[hsh] = c; pal[np] = c; slot[hsh] = (int16_t)(++np); }
        idx[i] = (uint8_t)(slot[hsh] - 1);
    }
    if (!ok) { free(idx); return png_write(path, argb, w, h); }
    size_t row = 1 + (size_t)w, raw = row * (size_t)h;
    uint8_t *img = malloc(raw);
    if (!img) { free(idx); return -1; }
    for (int y = 0; y < h; y++) { img[(size_t)y * row] = 0; memcpy(img + (size_t)y * row + 1, idx + (size_t)y * w, (size_t)w); }
    free(idx);
    uint32_t a1 = 1, a2 = 0;
    for (size_t i = 0; i < raw; i++) { a1 = (a1 + img[i]) % 65521; a2 = (a2 + a1) % 65521; }
    size_t blocks = (raw + 65534) / 65535, zn = 2 + raw + blocks * 5 + 4, o = 0;
    uint8_t *z = malloc(zn);
    if (!z) { free(img); return -1; }
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
    free(img);
    uint8_t plte[768];
    for (int i = 0; i < np; i++) { plte[i * 3] = (uint8_t)(pal[i] >> 16); plte[i * 3 + 1] = (uint8_t)(pal[i] >> 8); plte[i * 3 + 2] = (uint8_t)pal[i]; }
    char tmp[480];
    snprintf(tmp, sizeof tmp, "%s.part", path);
    FILE *f = fopen(tmp, "wb");
    int good = f != NULL;
    if (f) {
        static const uint8_t sig[8] = {137, 80, 78, 71, 13, 10, 26, 10};
        uint8_t ihdr[13];
        be32(ihdr, (uint32_t)w); be32(ihdr + 4, (uint32_t)h);
        ihdr[8] = 8; ihdr[9] = 3; ihdr[10] = ihdr[11] = ihdr[12] = 0;
        good = fwrite(sig, 1, 8, f) == 8 && png_chunk(f, "IHDR", ihdr, 13) && png_chunk(f, "PLTE", plte, (size_t)np * 3) && png_chunk(f, "IDAT", z, o) && png_chunk(f, "IEND", NULL, 0);
        good = fclose(f) == 0 && good;
    }
    free(z);
    if (!good || rename(tmp, path) != 0) { remove(tmp); return -1; }
    return 0;
}

/* ---- reading it back: the pictures, the table, the sound -------------------------------------------------------------------------- */
/* Our own PNG (png_write: RGB 8 bit, one IDAT of stored blocks, filter 0) into 0x00RRGGBB words. Anything else is refused: this is no general decoder. 0 when read. */
static int show_png_read(const char *path, uint32_t *out, int w, int h) {
    FILE *f = fopen(path, "rb");
    if (!f) return -1;
    size_t cap = (size_t)w * h * 3 + (size_t)h + 1024;
    uint8_t *buf = malloc(cap + 64);
    size_t n = buf ? fread(buf, 1, cap + 64, f) : 0;
    fclose(f);
    int rc = -1;
    if (!buf || n < 8 + 25 || n > cap + 32 || memcmp(buf, "\x89PNG\r\n\x1a\n", 8)) goto done;
    {
        size_t at = 8, zlen = 0;
        uint8_t *z = malloc(n);
        int have_hdr = 0, pal_mode = 0, npal = 0;
        uint8_t plte[768];
        if (!z) goto done;
        while (at + 12 <= n) {
            uint32_t len = (uint32_t)buf[at] << 24 | (uint32_t)buf[at + 1] << 16 | (uint32_t)buf[at + 2] << 8 | buf[at + 3];
            if (len > n || at + 12 + len > n) break;
            const uint8_t *type = buf + at + 4, *data = buf + at + 8;
            if (!memcmp(type, "IHDR", 4) && len == 13) {
                uint32_t pw = (uint32_t)data[0] << 24 | (uint32_t)data[1] << 16 | (uint32_t)data[2] << 8 | data[3], ph = (uint32_t)data[4] << 24 | (uint32_t)data[5] << 16 | (uint32_t)data[6] << 8 | data[7];
                have_hdr = pw == (uint32_t)w && ph == (uint32_t)h && data[8] == 8 && (data[9] == 2 || data[9] == 3) && !data[10] && !data[11] && !data[12];
                pal_mode = data[9] == 3;
            } else if (!memcmp(type, "PLTE", 4) && len <= 768 && len % 3 == 0) { memcpy(plte, data, len); npal = (int)(len / 3); }
            else if (!memcmp(type, "IDAT", 4)) { memcpy(z + zlen, data, len); zlen += len; }
            else if (!memcmp(type, "IEND", 4)) break;
            at += 12 + len;
        }
        size_t row = 1 + (size_t)w * (pal_mode ? 1 : 3), need = row * (size_t)h, got = 0, zp = 2;
        if (pal_mode && npal < 1) have_hdr = 0;
        uint8_t *raw = have_hdr && zlen > 6 && z[0] == 0x78 ? malloc(need) : NULL;
        while (raw && zp + 5 <= zlen && got < need) {                       /* stored blocks only */
            uint8_t fin = z[zp];
            size_t len = (size_t)z[zp + 1] | (size_t)z[zp + 2] << 8, nlen = (size_t)z[zp + 3] | (size_t)z[zp + 4] << 8;
            zp += 5;
            if ((fin & 6) != 0 || (len ^ 0xFFFF) != nlen || zp + len > zlen || got + len > need) { got = need + 1; break; }
            memcpy(raw + got, z + zp, len);
            got += len; zp += len;
            if (fin & 1) break;
        }
        if (raw && got == need) {
            rc = 0;
            for (int y = 0; y < h && rc == 0; y++) {
                const uint8_t *r = raw + (size_t)y * row;
                if (r[0] != 0) { rc = -1; break; }
                for (int x = 0; x < w; x++) {
                    if (pal_mode) { int ix = r[1 + x]; if (ix >= npal) { rc = -1; break; } out[(size_t)y * w + x] = (uint32_t)plte[ix * 3] << 16 | (uint32_t)plte[ix * 3 + 1] << 8 | plte[ix * 3 + 2]; }
                    else out[(size_t)y * w + x] = (uint32_t)r[1 + x * 3] << 16 | (uint32_t)r[2 + x * 3] << 8 | r[3 + x * 3];
                }
            }
        }
        free(raw);
        free(z);
    }
done:
    free(buf);
    return rc;
}
/* frames.txt: "NNNN <ms>" per picture (the number must be the next index), "end <ms>". Fills ms[]; returns the pictures, *total the end ms; -1 when it is not one. */
static int show_frames_read(const char *dir, int *ms, int max, int *total) {
    char p[420], buf[16384];
    snprintf(p, sizeof p, "%s/frames.txt", dir);
    FILE *f = fopen(p, "r");
    if (!f) return -1;
    size_t n = fread(buf, 1, sizeof buf - 1, f);
    fclose(f);
    buf[n] = 0;
    int k = 0, have_end = 0;
    *total = 0;
    for (const char *line = buf; line && *line; ) {
        const char *next = strchr(line, '\n');
        if (!strncmp(line, "end ", 4)) { *total = atoi(line + 4); have_end = 1; }
        else if (isdigit((unsigned char)line[0])) {
            if (atoi(line) != k || k >= max) return -1;
            const char *sp = memchr(line, ' ', next ? (size_t)(next - line) : strlen(line));      /* in this line only */
            if (!sp) return -1;
            ms[k++] = atoi(sp + 1);
        }
        line = next ? next + 1 : NULL;
    }
    return have_end && k > 0 ? k : -1;
}
/* audio.wav as written here: a 44-byte header, 16-bit stereo, 44100 Hz. Frames (stereo pairs) in *frames; the samples are malloc'd. 0 when read. */
static int show_wav_read(const char *path, int16_t **pcm, size_t *frames) {
    FILE *f = fopen(path, "rb");
    if (!f) return -1;
    uint8_t h[44];
    int rc = -1;
    if (fread(h, 1, 44, f) == 44 && !memcmp(h, "RIFF", 4) && !memcmp(h + 8, "WAVEfmt ", 8) && h[22] == 2 && h[34] == 16 && !memcmp(h + 36, "data", 4)) {
        uint32_t rate = (uint32_t)h[24] | (uint32_t)h[25] << 8 | (uint32_t)h[26] << 16 | (uint32_t)h[27] << 24, data = (uint32_t)h[40] | (uint32_t)h[41] << 8 | (uint32_t)h[42] << 16 | (uint32_t)h[43] << 24;
        if (rate == FM1_SAMPLE_RATE && data > 0 && data <= 32u * 1024 * 1024 && data % 4 == 0) {
            int16_t *b = malloc(data);
            if (b && fread(b, 1, data, f) == data) { *pcm = b; *frames = data / 4; rc = 0; } else free(b);
        }
    }
    fclose(f);
    return rc;
}

/* ---- the tour ------------------------------------------------------------------------------------------------------------------ */
/* The tour as data, so what the player shows (which keys, which step) is what was played: notes are matrix ids 14 + k, pressed at `down` and released at `up` (ticks, 30 a second). */
typedef struct { int down, up, n, key[3]; const char *label; } ShowNote;
static const ShowNote SHOW_NOTES[] = {
    {90, 120, 1, {14 + 11}, "NOTE 1"}, {135, 165, 1, {14 + 14}, "NOTE 2"}, {180, 210, 1, {14 + 18}, "NOTE 3"},
    {225, 285, 3, {14 + 11, 14 + 15, 14 + 18}, "A CHORD"}, {315, 345, 1, {14 + 14}, "NOTE 2 AGAIN"},
};
#define SHOW_NNOTES ((int)(sizeof SHOW_NOTES / sizeof SHOW_NOTES[0]))
#define SHOW_ENC_TICK 300                                   /* one click of the SELECT encoder (the next preset) */
/* What the person does at tick t: applied before that tick's sound is made. */
static void show_script(fm1ntt_machine *m, int t) {
    for (int i = 0; i < SHOW_NNOTES; i++) {
        if (t == SHOW_NOTES[i].down) for (int k = 0; k < SHOW_NOTES[i].n; k++) fm1ntt_key(m, SHOW_NOTES[i].key[k], 1);
        if (t == SHOW_NOTES[i].up) for (int k = 0; k < SHOW_NOTES[i].n; k++) fm1ntt_key(m, SHOW_NOTES[i].key[k], 0);
    }
    if (t == SHOW_ENC_TICK) fm1ntt_encoder(m, 0, 1);
}
/* The matrix bits held at tick t, after its script (bit n is id n), and what the tour is doing then. */
static uint64_t show_keys_at(int t) {
    uint64_t bits = 0;
    for (int i = 0; i < SHOW_NNOTES; i++) if (t >= SHOW_NOTES[i].down && t < SHOW_NOTES[i].up) for (int k = 0; k < SHOW_NOTES[i].n; k++) bits |= 1ULL << SHOW_NOTES[i].key[k];
    return bits;
}
static const char *show_step(int t) {
    for (int i = 0; i < SHOW_NNOTES; i++) if (t >= SHOW_NOTES[i].down && t < SHOW_NOTES[i].up) return SHOW_NOTES[i].label;
    if (t < SHOW_NOTES[0].down) return "BOOTING";
    if (t >= SHOW_ENC_TICK && t < SHOW_NOTES[SHOW_NNOTES - 1].down) return "SELECT KNOB TURNED";
    if (t >= SHOW_NOTES[SHOW_NNOTES - 1].up) return "THE TAIL";
    return "LISTENING";
}

static volatile sig_atomic_t g_show_cancel;
static void show_on_term(int s) { (void)s; g_show_cancel = 1; }

static int show_fail(const char *dir, ShowState *st, const char *why, char *result, size_t rn) {
    snprintf(st->state, sizeof st->state, "failed");
    snprintf(st->reason, sizeof st->reason, "%s", why);
    show_state_write(dir, st);
    { char p[420]; snprintf(p, sizeof p, "%s/progress.txt", dir); remove(p); }
    snprintf(result, rn, "failed: %s", why);
    return 1;
}

/* Makes the showcase of `fw` under `base`/demos/<sha>/. `budget_s`: the wall-time budget (0: 900 s). `jit`: the native JIT (faster on a slow unit). See the top. */
static int showcase_run_inner(const char *fw, int raw, uint32_t entry, int jit, const char *base, double budget_s, char *result, size_t rn) {
    char why[64];
    size_t fn = 0;
    uint8_t *bytes = read_file(fw, &fn, why, sizeof why);
    if (!bytes) { snprintf(result, rn, "cannot read the firmware: %s", why); return 2; }
    char sha[65], dir[420], p[460];
    fm1ntt_sha256_hex(bytes, fn, sha);
    if (show_dir(dir, sizeof dir, base, sha) != 0) { free(bytes); snprintf(result, rn, "no folder for this firmware"); return 2; }
    ShowState prev, st;
    show_state_read(dir, &prev);
    show_clear(dir);
    memset(&st, 0, sizeof st);
    st.tour = SHOW_TOUR; st.attempts = prev.have && prev.tour == SHOW_TOUR ? prev.attempts + 1 : 1; st.jit = jit != 0;
    snprintf(p, sizeof p, "%s/demos", base);
    if (mkdir_p(p) != 0 || mkdir_p(dir) != 0) { free(bytes); snprintf(result, rn, "cannot make %.180s", dir); return 2; }
    snprintf(st.state, sizeof st.state, "recording");
    show_state_write(dir, &st);
    g_show_cancel = 0;

    fm1ntt_machine *m = fm1ntt_new();
    int lr = m ? (raw ? fm1ntt_load_raw(m, bytes, fn, entry) : fm1ntt_load_package(m, bytes, fn)) : -1;
    free(bytes);
    if (lr != 0) { char w[100]; snprintf(w, sizeof w, "cannot load: %.80s", fm1ntt_error()[0] ? fm1ntt_error() : "not a firmware"); if (m) fm1ntt_free(m); return show_fail(dir, &st, w, result, rn); }
    if (jit && fm1ntt_jit_available()) fm1ntt_jit(m, 1);
    size_t pcm_cap = (size_t)SHOW_TICKS * SHOW_AUDIO * 2;
    int16_t *pcm = malloc(pcm_cap * sizeof(int16_t));
    uint32_t *scr = malloc(sizeof(uint32_t) * FM1_SCREEN_W * FM1_SCREEN_H);
    char *ftxt = malloc(SHOW_FRAMES_MAX * 20 + 64);
    if (!pcm || !scr || !ftxt) { free(pcm); free(scr); free(ftxt); fm1ntt_free(m); return show_fail(dir, &st, "out of memory", result, rn); }
    size_t ft = 0;
    int shorts = 0, shots = 0, last_tick = -10, pending = 0, halted_at = -1;
    uint32_t last_rev = 0xFFFFFFFFu;
    double t0 = mon_now_ms(), budget = budget_s > 0 ? budget_s : 900.0;
    int outcome = 0;
    char fail_why[100] = "";
    for (int t = 0; t < SHOW_TICKS && !outcome; t++) {
        if (g_show_cancel) { outcome = 3; break; }
        if (mon_now_ms() - t0 > budget * 1000.0) { snprintf(fail_why, sizeof fail_why, "time budget of %.0f s used up at %.1f s of the tour", budget, (double)t / SHOW_FPS); outcome = 1; break; }
        show_script(m, t);
        size_t got = fm1ntt_render(m, pcm + (size_t)t * SHOW_AUDIO * 2, SHOW_AUDIO, SHOW_BUDGET);
        if (got < SHOW_AUDIO) shorts++;                                  /* the budget ran out before the sound was made: silence pads it (should not happen: SHOW_BUDGET) */
        if (got < SHOW_AUDIO) memset(pcm + ((size_t)t * SHOW_AUDIO + got) * 2, 0, (SHOW_AUDIO - got) * 4);
        if (fm1ntt_halted(m) && halted_at < 0) halted_at = t;
        uint32_t rev = fm1ntt_lcd_revision(m);
        if (rev != last_rev) pending = 1;
        last_rev = rev;
        if (pending && t - last_tick >= SHOW_MIN_GAP && shots < SHOW_FRAMES_MAX && t >= 1) {
            fm1ntt_screen(m, scr);
            snprintf(p, sizeof p, "%s/%04d.png", dir, shots);
            if (show_png_write(p, scr, FM1_SCREEN_W, FM1_SCREEN_H) != 0) { snprintf(fail_why, sizeof fail_why, "cannot write the pictures"); outcome = 1; break; }
            ft += (size_t)snprintf(ftxt + ft, 40, "%04d %d\n", shots, t * 1000 / SHOW_FPS);
            shots++; last_tick = t; pending = 0;
        }
        if (t % 15 == 0) { char pr[48]; int n = snprintf(pr, sizeof pr, "pct=%d\n", t * 100 / SHOW_TICKS); snprintf(p, sizeof p, "%s/progress.txt", dir); write_atomic(p, pr, (size_t)n); }
    }
    if (!outcome && halted_at >= 0) { snprintf(fail_why, sizeof fail_why, "the emulated machine halted at %.1f s (a fault in the firmware or the emulator)", (double)halted_at / SHOW_FPS); outcome = 1; }
    if (!outcome) {                                       /* a screen that never drew is no showcase */
        fm1ntt_screen(m, scr);
        int colours = 0; uint32_t seen[4] = {0};
        for (int k = 0; k < FM1_SCREEN_W * FM1_SCREEN_H && colours < 4; k++) { int f = 0; for (int q = 0; q < colours; q++) if (seen[q] == scr[k]) f = 1; if (!f) seen[colours++] = scr[k]; }
        if (colours < 2 || shots < 1) { snprintf(fail_why, sizeof fail_why, "the screen stayed blank"); outcome = 1; }
    }
    fm1ntt_free(m);
    if (outcome == 3) { free(pcm); free(scr); free(ftxt); show_clear(dir); snprintf(result, rn, "cancelled"); return 3; }
    if (outcome == 1) { free(pcm); free(scr); free(ftxt); return show_fail(dir, &st, fail_why, result, rn); }
    ft += (size_t)snprintf(ftxt + ft, 40, "end %d\n", SHOW_TICKS * 1000 / SHOW_FPS);
    snprintf(p, sizeof p, "%s/frames.txt", dir);
    int ok = write_atomic(p, ftxt, ft) == 0;
    uint8_t h[44];
    wav_header(h, (unsigned long long)SHOW_TICKS * SHOW_AUDIO);
    size_t wn = 44 + pcm_cap * sizeof(int16_t);
    uint8_t *wav = malloc(wn);
    if (wav) { memcpy(wav, h, 44); memcpy(wav + 44, pcm, pcm_cap * sizeof(int16_t)); snprintf(p, sizeof p, "%s/audio.wav", dir); ok = ok && write_atomic(p, wav, wn) == 0; } else ok = 0;
    free(wav); free(pcm); free(scr); free(ftxt);
    if (!ok) return show_fail(dir, &st, "cannot write the sound or the table", result, rn);
    st.frames = shots; st.ms = SHOW_TICKS * 1000 / SHOW_FPS; st.shorts = shorts;
    snprintf(st.state, sizeof st.state, "ready");
    st.reason[0] = 0;
    show_state_write(dir, &st);
    snprintf(p, sizeof p, "%s/progress.txt", dir);
    remove(p);
    snprintf(result, rn, "ready: %d pictures, %.0f s, in demos/%.12s", shots, (double)SHOW_TICKS / SHOW_FPS, sha);
    return 0;
}

/* The run, with SIGTERM and SIGINT meaning "stop and clean up" only while it runs: the handlers are put back afterwards, so a process that makes a showcase and goes on
 * (the self-test) is still ended by `timeout`'s SIGTERM instead of swallowing it as a cancel. */
static int showcase_run(const char *fw, int raw, uint32_t entry, int jit, const char *base, double budget_s, char *result, size_t rn) {
    struct sigaction sa, old_term, old_int;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = show_on_term;
    sigaction(SIGTERM, &sa, &old_term);
    sigaction(SIGINT, &sa, &old_int);
    int rc = showcase_run_inner(fw, raw, entry, jit, base, budget_s, result, rn);
    sigaction(SIGTERM, &old_term, NULL);
    sigaction(SIGINT, &old_int, NULL);
    return rc;
}

#endif
