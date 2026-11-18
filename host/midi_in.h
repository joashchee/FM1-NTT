/* FM1-NTT host: USB MIDI input (a keyboard, a controller, the FM-1 itself over USB). MIT.
 *
 * Linux (the R36S): the kernel's raw MIDI devices, /dev/snd/midiC<card>D<device>, read as plain files. No library: ALSA's own driver does the USB class.
 * A thread rescans the folder once a second (a keyboard plugged in later is picked up, one pulled out is dropped), polls every open device and parses the bytes.
 * macOS: CoreMIDI, every source, connected from midi_in_tick (the main thread calls it about once a second). Windows: not done.
 * Both put whole messages, stamped with the host's monotonic microsecond clock, into one bounded ring that the main thread empties (midi_in_pop); the audio
 * thread never touches it. A message arriving on an empty ring wakes the main loop with an SDL user event.
 * The parser takes bytes as the wire sends them: running status, real-time bytes (F8 clock, FA/FB/FC) inside a message, system exclusive skipped.
 * Untested on a real keyboard or an R36S: the parser and the Linux reader are tested here through a FIFO named like a device (FM1NTT_MIDI_DIR points the scan elsewhere);
 * CoreMIDI is built on the Mac only. */
#ifndef FM1NTT_MIDI_IN_H
#define FM1NTT_MIDI_IN_H
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <SDL.h>
#ifdef __APPLE__
#include <CoreMIDI/CoreMIDI.h>
#endif

#define MIN_RING 256
#define MIN_DEVS 8

typedef struct { uint64_t ts; uint8_t n, b[3]; } MidiMsg;
typedef struct { uint8_t run, need, have, b[3], sysex; } MidiParser;

/* One byte in; the length (1 to 3) of a finished message put in out[], else 0. Real-time bytes (F8 to FF) come out alone and leave a message under way alone;
 * system common messages (F1 to F6) cancel running status and are not passed on; system exclusive (F0 ... F7) is skipped whole; a data byte with no status is dropped. */
static int midi_parse(MidiParser *p, uint8_t c, uint8_t out[3]) {
    if (c >= 0xF8) { out[0] = c; return 1; }
    if (c >= 0xF0) { p->run = 0; p->have = 0; p->sysex = c == 0xF0; return 0; }          /* F0 starts exclusive; F1-F6 and F7 end whatever was under way */
    if (c >= 0x80) { p->run = c; p->need = (c >= 0xC0 && c < 0xE0) ? 1 : 2; p->have = 0; p->sysex = 0; return 0; }
    if (p->sysex || !p->run) return 0;
    p->b[p->have++] = c;
    if (p->have < p->need) return 0;
    p->have = 0;
    out[0] = p->run; out[1] = p->b[0]; out[2] = p->need == 2 ? p->b[1] : 0;
    return 1 + p->need;
}

typedef struct {
    SDL_mutex *mu;
    MidiMsg ring[MIN_RING];
    int head, n;                          /* guarded by mu */
    uint64_t dropped;                     /* a message the full ring refused */
    int devs;                             /* devices open now */
    uint64_t seen;                        /* messages accepted, ever */
    volatile int stop;
    SDL_Thread *th;
    char dir[200];
    uint32_t wake;                        /* the SDL user event type that wakes the main loop (0: none) */
#ifdef __APPLE__
    MIDIClientRef client;
    MIDIPortRef port;
    MIDIEndpointRef connected[32];
    int nconnected;
    MidiParser parsers[32];
#endif
} MidiIn;

static uint64_t midi_in_clock(void) { return (uint64_t)((double)SDL_GetPerformanceCounter() * 1e6 / (double)SDL_GetPerformanceFrequency()); }

static void midi_in_push(MidiIn *m, const uint8_t *b, int n) {
    int wake = 0;
    SDL_LockMutex(m->mu);
    if (m->n == MIN_RING) m->dropped++;
    else {
        MidiMsg *s = &m->ring[(m->head + m->n) % MIN_RING];
        s->ts = midi_in_clock(); s->n = (uint8_t)n; memset(s->b, 0, 3); memcpy(s->b, b, (size_t)n);
        wake = m->n == 0;
        m->n++; m->seen++;
    }
    SDL_UnlockMutex(m->mu);
    if (wake && m->wake) { SDL_Event e; SDL_zero(e); e.type = m->wake; SDL_PushEvent(&e); }
}
/* The oldest waiting message, or 0. */
static int midi_in_pop(MidiIn *m, MidiMsg *out) {
    int got = 0;
    if (!m->mu) return 0;
    SDL_LockMutex(m->mu);
    if (m->n) { *out = m->ring[m->head]; m->head = (m->head + 1) % MIN_RING; m->n--; got = 1; }
    SDL_UnlockMutex(m->mu);
    return got;
}

/* ---- the kernel's raw MIDI devices (Linux; built everywhere so a FIFO can stand in for one in the test) ---- */
typedef struct { int fd; char name[64]; MidiParser p; } MidiDev;

static int midi_in_isdev(const char *name) {      /* midiC<digits>D<digits> */
    if (strncmp(name, "midiC", 5)) return 0;
    const char *s = name + 5;
    int d = 0;
    while (*s >= '0' && *s <= '9') { s++; d++; }
    if (!d || *s++ != 'D') return 0;
    d = 0;
    while (*s >= '0' && *s <= '9') { s++; d++; }
    return d && !*s;
}
static void midi_in_scan(MidiIn *m, MidiDev *dev, int *nd) {
    DIR *d = opendir(m->dir);
    if (!d) return;
    struct dirent *e;
    while ((e = readdir(d)) != NULL && *nd < MIN_DEVS) {
        if (!midi_in_isdev(e->d_name)) continue;
        int have = 0;
        for (int i = 0; i < *nd; i++) if (!strcmp(dev[i].name, e->d_name)) have = 1;
        if (have) continue;
        char path[300];
        snprintf(path, sizeof path, "%s/%s", m->dir, e->d_name);
        int fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) continue;
        memset(&dev[*nd], 0, sizeof dev[*nd]);
        dev[*nd].fd = fd; snprintf(dev[*nd].name, sizeof dev[*nd].name, "%s", e->d_name);
        (*nd)++;
    }
    closedir(d);
}
static int midi_in_thread(void *arg) {
    MidiIn *m = (MidiIn *)arg;
    MidiDev dev[MIN_DEVS];
    int nd = 0;
    uint32_t last_scan = 0;
    while (!m->stop) {
        uint32_t t = SDL_GetTicks();
        if (!nd || t - last_scan >= 1000) { midi_in_scan(m, dev, &nd); last_scan = t; m->devs = nd; }
        if (!nd) { SDL_Delay(200); continue; }
        struct pollfd pf[MIN_DEVS];
        for (int i = 0; i < nd; i++) { pf[i].fd = dev[i].fd; pf[i].events = POLLIN; pf[i].revents = 0; }
        if (poll(pf, (nfds_t)nd, 100) <= 0) continue;
        for (int i = nd - 1; i >= 0; i--) {
            if (!pf[i].revents) continue;
            uint8_t buf[256];
            ssize_t r = read(dev[i].fd, buf, sizeof buf);
            if (r > 0) {
                for (ssize_t k = 0; k < r; k++) { uint8_t o[3]; int n = midi_parse(&dev[i].p, buf[k], o); if (n) midi_in_push(m, o, n); }
            } else if (r == 0 || (errno != EAGAIN && errno != EINTR)) {      /* unplugged (ENODEV), or a FIFO nobody writes to: closed, found again by a later scan */
                close(dev[i].fd);
                dev[i] = dev[--nd];
                m->devs = nd;
                SDL_Delay(50);
            }
        }
    }
    for (int i = 0; i < nd; i++) close(dev[i].fd);
    m->devs = 0;
    return 0;
}

#ifdef __APPLE__
static void midi_in_read(const MIDIPacketList *pl, void *rc, void *src) {
    MidiIn *m = (MidiIn *)rc;
    int slot = (int)(intptr_t)src - 1;
    if (slot < 0 || slot >= 32) return;
    const MIDIPacket *p = &pl->packet[0];
    for (UInt32 i = 0; i < pl->numPackets; i++) {
        for (UInt16 k = 0; k < p->length; k++) { uint8_t o[3]; int n = midi_parse(&m->parsers[slot], p->data[k], o); if (n) midi_in_push(m, o, n); }
        p = MIDIPacketNext(p);
    }
}
#endif

/* Called by the main thread about once a second: CoreMIDI sources come and go; on Linux the reader thread does its own scan. */
static void midi_in_tick(MidiIn *m) {
#ifdef __APPLE__
    if (!m->port) return;
    ItemCount ns = MIDIGetNumberOfSources();
    for (ItemCount i = 0; i < ns && m->nconnected < 32; i++) {
        MIDIEndpointRef e = MIDIGetSource(i);
        int have = 0;
        for (int k = 0; k < m->nconnected; k++) if (m->connected[k] == e) have = 1;
        if (have || !e) continue;
        int slot = m->nconnected;
        memset(&m->parsers[slot], 0, sizeof m->parsers[slot]);
        if (MIDIPortConnectSource(m->port, e, (void *)(intptr_t)(slot + 1)) == noErr) m->connected[m->nconnected++] = e;
    }
    m->devs = m->nconnected;
#else
    (void)m;
#endif
}

static int midi_in_start(MidiIn *m, const char *dir) {
    memset(m, 0, sizeof *m);
    m->mu = SDL_CreateMutex();
    if (!m->mu) return -1;
    m->wake = SDL_RegisterEvents(1);
    if (m->wake == (uint32_t)-1) m->wake = 0;
    const char *env = getenv("FM1NTT_MIDI_DIR");
    snprintf(m->dir, sizeof m->dir, "%s", env && *env ? env : dir ? dir : "/dev/snd");
    int rc = -1;
#ifdef __APPLE__
    if (!dir && !(env && *env)) {
        if (MIDIClientCreate(CFSTR("FM1-NTT"), NULL, NULL, &m->client) == noErr &&
            MIDIInputPortCreate(m->client, CFSTR("in"), midi_in_read, m, &m->port) == noErr) { midi_in_tick(m); return 0; }
        return -1;
    }
#endif
    m->th = SDL_CreateThread(midi_in_thread, "midi-in", m);
    rc = m->th ? 0 : -1;
    return rc;
}
static void midi_in_stop(MidiIn *m) {
    if (!m->mu) return;
    m->stop = 1;
    if (m->th) SDL_WaitThread(m->th, NULL);
#ifdef __APPLE__
    if (m->client) MIDIClientDispose(m->client);       /* its port goes with it, and no callback runs after this returns */
#endif
    SDL_DestroyMutex(m->mu);
    m->mu = NULL; m->th = NULL;
}
#endif
