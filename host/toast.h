/* The hot-plug toast (r3i6nos rule 13, ../r3i6nos/docs/HOTPLUG.md), in pixels: a bar over row 0 of the window.
 * What is plugged in or out comes from r3i6nos's daemon, which writes /run/r3i6nos-hotplug/events (FM1NTT_HOTPLUG overrides the path):
 * one line per event, `<nanoseconds>\t<info|error>\t<text>`, the numbers only rise. A change shows for 3 seconds; a failure stays until a key
 * is pressed, and that key is used up (toast_key returns 1). What was in the file at start is history, not replayed. Several at once show one
 * after another; a failure goes first and replaces a change on show; the queue holds 8 changes (the oldest goes), never a failure.
 * Untested on a unit: the daemon's file has never been read here (r3i6nos says the same of its own side). */
#ifndef FM1NTT_TOAST_H
#define FM1NTT_TOAST_H
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include "pix.h"

#ifdef __APPLE__
#define TOAST_MTIME_NS(st) ((long long)(st).st_mtimespec.tv_sec * 1000000000LL + (st).st_mtimespec.tv_nsec)
#else
#define TOAST_MTIME_NS(st) ((long long)(st).st_mtim.tv_sec * 1000000000LL + (st).st_mtim.tv_nsec)
#endif

#define TOAST_TEXT 160
#define TOAST_Q 8
#define TOAST_MS 3000u
#define TOAST_INFO_BG 0xFFB8F0B8u   /* black on light green: 4.5:1 or better, checked by the self-test */
#define TOAST_ERR_BG 0xFFC00000u    /* white on red */

typedef struct { char text[TOAST_TEXT]; int error; } ToastItem;
typedef struct {
    char path[300];
    unsigned long long last_ns;     /* the newest event seen */
    long long mtime, size;          /* of the file at the last read, to skip a read when nothing changed */
    ToastItem q[TOAST_Q * 2];       /* failures first, then changes in order */
    int n;
    ToastItem cur;
    int has;
    uint32_t until;
    int dirty;
} Toast;

static void toast_show_next(Toast *t, uint32_t now) {
    if (t->has && (t->cur.error || now < t->until)) return;
    if (t->has) t->dirty = 1;
    t->has = 0;
    if (t->n > 0) {
        t->cur = t->q[0];
        memmove(t->q, t->q + 1, (size_t)(t->n - 1) * sizeof t->q[0]);
        t->n--;
        t->has = 1;
        t->until = now + TOAST_MS;
        t->dirty = 1;
    }
}
static void toast_add(Toast *t, const char *text, int error, uint32_t now) {
    ToastItem it;
    memset(&it, 0, sizeof it);
    snprintf(it.text, sizeof it.text, "%s", text);
    it.error = error;
    if (error) {
        int at = 0;
        while (at < t->n && t->q[at].error) at++;
        if (t->n == TOAST_Q * 2) t->n--;                       /* only changes are dropped in earnest; this keeps the array bounded */
        memmove(t->q + at + 1, t->q + at, (size_t)(t->n - at) * sizeof t->q[0]);
        t->q[at] = it;
        t->n++;
        if (t->has && !t->cur.error) t->has = 0;               /* a failure replaces a change on show */
    } else {
        int changes = 0;
        for (int i = 0; i < t->n; i++) changes += !t->q[i].error;
        if (changes >= TOAST_Q) {                              /* the oldest change goes */
            for (int i = 0; i < t->n; i++) if (!t->q[i].error) { memmove(t->q + i, t->q + i + 1, (size_t)(t->n - i - 1) * sizeof t->q[0]); t->n--; break; }
        }
        t->q[t->n++] = it;
    }
    toast_show_next(t, now);
    t->dirty = 1;
}

/* Reads the events file when it changed; returns the events newer than last_ns to `emit` (or, with emit NULL, only learns last_ns). */
static void toast_read(Toast *t, uint32_t now, int emit) {
    struct stat st;
    if (stat(t->path, &st) != 0) { t->mtime = t->size = -1; return; }
    if (TOAST_MTIME_NS(st) == t->mtime && (long long)st.st_size == t->size) return;
    t->mtime = TOAST_MTIME_NS(st);
    t->size = (long long)st.st_size;
    if (st.st_size <= 0 || st.st_size > 65536) return;          /* the daemon keeps 40 lines: a bigger file is not its */
    FILE *f = fopen(t->path, "rb");
    if (!f) return;
    char *buf = malloc((size_t)st.st_size + 1);
    size_t n = buf ? fread(buf, 1, (size_t)st.st_size, f) : 0;
    fclose(f);
    if (!buf) return;
    buf[n] = 0;
    unsigned long long top = t->last_ns;
    for (char *line = buf, *next; line && *line; line = next) {
        next = strchr(line, '\n');
        if (next) *next++ = 0;
        char *end;
        unsigned long long ns = strtoull(line, &end, 10);
        if (end == line || *end != '\t' || ns <= t->last_ns) continue;
        char *kind = end + 1, *text = strchr(kind, '\t');
        if (!text) continue;
        *text++ = 0;
        if (strcmp(kind, "info") && strcmp(kind, "error")) continue;
        if (ns > top) top = ns;
        if (emit && *text) toast_add(t, text, kind[0] == 'e', now);
    }
    t->last_ns = top;
    free(buf);
}
static void toast_init(Toast *t, const char *path, uint32_t now) {
    memset(t, 0, sizeof *t);
    const char *e = getenv("FM1NTT_HOTPLUG");
    snprintf(t->path, sizeof t->path, "%s", e && *e ? e : path);
    t->mtime = t->size = -1;
    toast_read(t, now, 0);                                       /* history is not replayed */
}
/* Once a frame. Returns 1 when what is on screen changed. */
static int toast_poll(Toast *t, uint32_t now) {
    t->dirty = 0;
    toast_read(t, now, 1);
    if (t->has && !t->cur.error && now >= t->until) { t->has = 0; t->dirty = 1; }
    toast_show_next(t, now);
    return t->dirty;
}
static int toast_active(const Toast *t) { return t->has || t->n > 0; }
/* First on every key or button press: a failure on show is cleared and the press is used up (returns 1). A change never consumes one. */
static int toast_key(Toast *t, uint32_t now) {
    if (!t->has || !t->cur.error) return 0;
    t->has = 0;
    toast_show_next(t, now);
    t->dirty = 1;
    return 1;
}
static void toast_draw(const Toast *t, uint32_t *c) {
    if (!t->has) return;
    uint32_t bg = t->cur.error ? TOAST_ERR_BG : TOAST_INFO_BG, fg = t->cur.error ? 0xFFFFFFFFu : 0xFF000000u;
    px_rect(c, (PxRect){0, 0, PX_W, 14}, bg);
    char line[TOAST_TEXT + 24];
    snprintf(line, sizeof line, t->cur.error ? "%s  PRESS A KEY" : "%s", t->cur.text);
    px_text(c, 6, 3, line, fg, 1, PX_W - 12);
}
#endif
