/* FM1-NTT host: C and SDL2, pixel graphics (host/pix.h), over the Rust core (core/fm1ntt.h). MIT.
 *
 *   fm1ntt [--entry 0xADDR] FIRMWARE   open a window; a .fwsc package, or with --entry a raw XIP image (Felucca: 0x02000120)
 *   fm1ntt                             open it with nothing loaded (the FILES panel lists the firmware folder)
 *   fm1ntt --selftest                  headless: layout, pixel accuracy on every panel, font, contrast, the panels' logic, the core
 *   fm1ntt --screenshot OUT.bmp [--zoomed] [--panel N] [FIRMWARE]   draw one frame to a file
 *   fm1ntt --report [FIRMWARE]         the Device report (r3i6nos rule 9) to $BASE/r36s-report.txt, with the power windows
 *   fm1ntt --bench [--seconds N] [--fw FILE]   the Benchmark report to $BASE/bench/ (compare with scripts/bench-compare.py)
 *   fm1ntt --rec-demo [--features a,b] [--no-video] [FIRMWARE]   Rec demo (rule 11) to $BASE/Demo/
 *   fm1ntt --pin ...                   (or FM1NTT_PIN=1) the emulating (audio) thread on the last core, the main thread on the first: off by default, to be measured
 *   fm1ntt --reduced-motion ...        (or FM1NTT_REDUCED_MOTION=1) the MONITOR panel's data rain is a still page
 *   fm1ntt --version
 *
 * The FM-1's 240x240 screen sits flush top left, pixel for pixel. Fn (raw button 16, Tab or F1) toggles Standard (1x, panels right and
 * below) and Zoomed (2x, one narrow panel at the right; Tab or F1 on a keyboard). On the pad Fn turns the panel carousel and L1+R1 together
 * open and close the menu (TOOLS); Home and End turn the carousel on a keyboard; on MACHINE the pad plays, see the pad section below. The carousel is as TinyNTT's:
 * MACHINE FILES CPU MEMORY CONSOLE MIDI SOUND MONITOR TOOLS. The audio device paces the emulation: SDL's callback asks the core for frames.
 * Files are only written under the app's folder ($FM1NTT_DIR, else r3i6nos's choice: the DATA card /roms2/FM1-NTT when a valid one is in, else /roms/FM1-NTT; else ~/FM1-NTT; see base_dir), to a temporary name
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
#include <fcntl.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/wait.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>

#include "fm1ntt.h"
#include "pix.h"
#include "threads.h"
#include "toast.h"
#include "midi_in.h"
#include "padmap.h"

#ifndef FM1NTT_BUILD
#define FM1NTT_BUILD "unknown"
#endif

#define AUDIO_FRAMES 512
#define PV_MAX 128                      /* voices the preview keeps */
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

#define NTOOLS 15
#define RAIN_IDLE_MS 30000u       /* no input for this long and the monitor's whole panel is the data rain (when it is on) */
#define PACE_AUTO 4
enum { CF_NONE, CF_RESET, CF_RESTORE, CF_QUIT };
enum { MK_NONE, MK_MAIN, MK_CTX, MK_ABOUT, MK_THEME, MK_KEYMAP };     /* the overlay menus (r3i6nos rule 17): the main menu, the context menu, About, the colour theme picker */

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
    int scale, key_shift;
    int panel_prev;                     /* the panel shown before this one, plus one (R1 + Fn goes back to it, and again); 0 none yet */
    int menu_kind, menu_sel, theme_before;   /* the overlay menu open (MK_*), its selection, and the theme to go back to when the picker is cancelled */
    int km_page, km_sel, km_state, km_other, km_new, km_panel;   /* the Keymap: page, row, 0 browsing / 1 waiting for the new button / 2 a conflict to swap / 3 reset all asked; the other slot; the panel in focus when it opened */
    char km_note[56];                   /* a line for the Keymap's foot: why a button was refused */
    char km_hist[5][48];                /* the Keymap's test page: the last presses */
    int km_nhist;
    PmStick rstick;                     /* the right stick as the d-pad on the panels that are not MACHINE */
    Toast toast;                        /* the hot-plug toast over row 0 */
    MidiIn min;                         /* USB MIDI input (host/midi_in.h): a ring the main loop empties into the instrument or the machine */
    int min_on;                         /* the reader started */
    uint64_t min_routed;                /* messages sent on */
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
    int remember, fl_dirty;               /* TOOLS > REMEMBER SETTINGS: the firmware's flash is kept per firmware between runs */
    uint32_t fl_rev, fl_t;
    char fl_sha[65];                      /* the loaded package's SHA-256, the name of its remembered flash */
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
    volatile int fw_mute;               /* the firmware's sound is muted: the emulation is too slow to fill the audio buffers, and a late callback rattles (speed_tick sets it) */
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
    int audio_asleep;                   /* the sound device is paused because nothing can make a sound and nobody has touched the unit (audio_wanted) */
    SDL_Thread *job;
    SDL_atomic_t job_done;
    int job_kind, job_style, want_demo;
    double job_seconds;
    int load_sync;                      /* a stepped clock (Rec demo) loads on the calling thread: no progress box, the same frames every run */
    char ld_path[300]; int ld_raw; uint32_t ld_entry; uint8_t *ld_buf; size_t ld_n; int ld_rc;   /* JOB_LOAD: what to load, and what the thread made */
    char job_result[200];
    int pin, pinned;                    /* --pin: the audio (emulating) thread on the last core, the main thread on the first; pinned: the audio thread has done it */
    /* the SOUND panel's preview: the fm6 engine plays the voices found in the loaded firmware's banks from the keys (docs/FM6.md); the machine's own keys are not touched */
    fm1ntt_fm6 *pv;
    int mq_live;                        /* the audio callback is running: the machine's MIDI may go through mevq */
    fm1ntt_evq *mevq;                   /* the machine's MIDI from the UI to the audio thread, the same way (NULL: the locked path; also used while paused, and by the Rec demo, which steps its own clock) */
    fm1ntt_evq *evq;                    /* the instrument's notes from the UI to the audio thread: timestamped, lock-free, each applied at its own frame (core/src/evq.rs); NULL means the locked path */
    fm1ntt_inst *inst;                  /* an instrument package (docs/PORTING.md) played instead of the fm6 preview: its notes, its program for the voice, its effects */
    uint8_t pv_voices[PV_MAX][128];
    int pv_factory;                     /* the voices are the built-in set (the firmware has none of its own, or none is loaded) */
    int pv_solo;                        /* the machine is not run, only the engine (R2): the live-play mode for a unit too slow to emulate the firmware */
    int pv_n, pv_sel, pv_on, pv_oct;    /* voices found, the chosen one, preview on, octave -2..2 */
    int ed_on, ed_group, ed_sel, ed_dirty;   /* the instrument editor (SOUND panel, B on a loaded instrument): its group (engine, arpeggiator, an effect), the chosen parameter, edited since loaded */
    char inst_path[300];                /* where the loaded instrument came from */
    int pv_key[27], pv_pad[4];          /* the MIDI note each held key or pad button started (+1; 0 none), so its release matches */
    /* the firmware showcase (host/showcase.h, ROADMAP 5d): a background worker process, the firmware's page in FILES, the demo player */
    int bg_demos;                       /* TOOLS > BACKGROUND DEMOS: make a demo of each firmware that has none (off by default) */
    int info_on, info_i;                /* the page of the firmware at info_i (an index into files) replaces the list */
    pid_t sw_pid;                       /* the worker (fm1ntt --showcase), 0 when none */
    int sw_stopped, sw_cancelling, sw_cursor, sw_low;
    char sw_name[PX_NAME];
    double sw_start_ms, sw_next_ms, sw_batt_ms, sw_cancel_ms;
    char fsha[PX_MAX_FILES][65];        /* each firmware's SHA-256 ("" not yet taken, "-" unreadable): the key of its demo folder */
    struct { int found; char licence[96], source[120], name[48]; } info_rec;
    struct { volatile int on; int16_t *pcm; size_t frames; volatile size_t pos; int nf, total_ms, cur; int ms[400]; char dir[420], name[PX_NAME], tier[40], licence[104]; } pl;
    uint32_t pl_pic[240 * 240];
    /* MONITOR (monitor.h, docs/MONITOR.md) */
    Mon mon;
    MonRain rain;
    int mon_page, rain_on, reduced_motion, njoy, cb_rate;
    char padn[2][28];                   /* the first two pads' names (SDL), read once a second while the monitor shows */
    uint32_t last_input_ms, frame_ms, frame_max_ms;
    int cb_prev; double cb_prev_ms;
    SDL_atomic_t late_cb;               /* audio buffers that took longer than their own length to make */
    uint32_t budget;                    /* instructions a buffer may spend (0: MAX_INSNS_PER_BUFFER); Rec demo's self-test lowers it */
    /* the emulation thread (emu_start): it renders the machine into a ring, and the audio callback only reads the ring (NULL emu: the callback renders, as before) */
    SDL_Thread *emu;
    SDL_mutex *mx;                      /* the machine: taken by the thread for each chunk it renders and by lock() for anything the UI does to it (always before the audio lock) */
    SDL_sem *emu_sem;                   /* the callback posts it when it has taken audio, so the thread wakes when there is room and sleeps when there is not */
    int16_t *emu_ring;                  /* EMU_RING stereo frames */
    SDL_atomic_t ring_w, ring_r;        /* frames written by the thread and read by the callback (monotonic, wrap by unsigned arithmetic) */
    volatile int emu_quit, emu_loaded, emu_primed;   /* emu_loaded: the machine has a firmware; emu_primed: the ring has filled once since the machine last ran (an empty ring before that is not a gap) */
} App;

enum { JOB_NONE, JOB_REPORT, JOB_BENCH, JOB_LOAD };
static void job_start(App *a, int kind);
static void scale_next(App *a);
static int job_pct(const App *a);
static void rain_still(App *a);
static int files_press(App *a, int act);
static void ui_showcase(App *a, UiState *u);
static void player_stop(App *a);

/* ---- small helpers ------------------------------------------------------------------------------------------------------ */
static void say(App *a, int style, const char *fmt, const char *arg) {
    snprintf(a->msg, sizeof a->msg, fmt, arg ? arg : "");
    a->msg_style = style;
}
/* The machine's mutex first, then the audio device's: the UI waits for the emulation thread's chunk (a few ms) without holding the callback up, and the callback
 * never takes the mutex, so the two orders cannot meet. Both are recursive. */
static void lock(App *a) { if (a->mx) SDL_LockMutex(a->mx); if (a->dev) SDL_LockAudioDevice(a->dev); }
static void unlock(App *a) { if (a->dev) SDL_UnlockAudioDevice(a->dev); if (a->mx) SDL_UnlockMutex(a->mx); }

/* ---- the SOUND preview ------------------------------------------------------------------------------------------------- */
static void pv_notes_off(App *a) {
    lock(a);
    if (a->pv) fm1ntt_fm6_all_off(a->pv);
    if (a->inst) fm1ntt_inst_all_off(a->inst);
    unlock(a);
    memset(a->pv_key, 0, sizeof a->pv_key);
    memset(a->pv_pad, 0, sizeof a->pv_pad);
}
static void pv_select(App *a, int sel) {
    if (a->pv_n <= 0) return;
    a->pv_sel = (sel % a->pv_n + a->pv_n) % a->pv_n;
    if (a->inst) { lock(a); fm1ntt_inst_set_program(a->inst, (size_t)a->pv_sel); unlock(a); return; }
    lock(a);
    if (a->pv) { fm1ntt_fm6_all_off(a->pv); fm1ntt_fm6_free(a->pv); a->pv = NULL; }
    /* the stock engine as measured on DX747 (docs/FM6.md): its tuning and output stage at the emulator's default pot */
    a->pv = fm1ntt_fm6_new(FM1_SAMPLE_RATE, a->pv_voices[a->pv_sel]);
    if (a->pv) fm1ntt_fm6_set_model(a->pv, 2, 900, 2, 0);
    unlock(a);
}
/* Takes the voices (n of 128 bytes) as the preview's, replacing the last firmware's. */
static void pv_set(App *a, const uint8_t *voices, int n) {
    pv_notes_off(a);
    if (a->inst) { lock(a); fm1ntt_inst_free(a->inst); a->inst = NULL; unlock(a); a->ed_on = 0; }   /* a firmware's voices replace an instrument */
    a->pv_n = n < 0 ? 0 : n > PV_MAX ? PV_MAX : n;
    a->pv_factory = 0;
    if (a->pv_n) memcpy(a->pv_voices, voices, (size_t)a->pv_n * 128);
    else {   /* none of the firmware's own: the built-in set, so the preview and Solo always have something to play (the bundled firmwares have no banks the extractor finds) */
        a->pv_n = (int)fm1ntt_factory_voices(&a->pv_voices[0][0], PV_MAX);
        a->pv_factory = a->pv_n > 0;
    }
    a->pv_sel = 0;
    if (a->pv_n) pv_select(a, 0);
    else { lock(a); if (a->pv) { fm1ntt_fm6_free(a->pv); a->pv = NULL; } unlock(a); a->pv_on = 0; }
}
static void pv_name(const App *a, char *out, size_t cap) {
    out[0] = 0;
    if (a->inst) { fm1ntt_inst_text(a->inst, a->pv_n > 1 ? 4 : 0, out, cap); return; }
    if (a->pv_sel < 0 || a->pv_sel >= a->pv_n) return;
    size_t k = 0;
    for (int i = 118; i < 128 && k + 1 < cap; i++) { int ch = a->pv_voices[a->pv_sel][i]; out[k++] = ch >= 32 && ch < 127 ? (char)ch : ' '; }
    while (k && out[k - 1] == ' ') k--;
    out[k] = 0;
}
/* The host's monotonic clock in microseconds: the stamp the queue's producer and consumer share. */
static uint64_t now_us(void) { return (uint64_t)((double)SDL_GetPerformanceCounter() * 1e6 / (double)SDL_GetPerformanceFrequency()); }
/* An instrument's note to the audio thread with no lock: the callback applies it at its frame. The slot bookkeeping stays on this side. */
static void inst_note_q(App *a, int *slot, int note, int down) {
    uint64_t t = now_us();
    uint8_t m[3];
    if (down) {
        if (*slot) { m[0] = 0x80; m[1] = (uint8_t)(*slot - 1); m[2] = 0; fm1ntt_evq_push(a->evq, t, m, 3); }
        *slot = note + 1; m[0] = 0x90; m[1] = (uint8_t)note; m[2] = 100; fm1ntt_evq_push(a->evq, t, m, 3);
    } else if (*slot) { m[0] = 0x80; m[1] = (uint8_t)(*slot - 1); m[2] = 0; fm1ntt_evq_push(a->evq, t, m, 3); *slot = 0; }
}
static void pv_note(App *a, int *slot, int note, int down) {
    if (!a->pv && !a->inst) return;
    if (a->inst && a->evq) { inst_note_q(a, slot, note, down); return; }
    lock(a);
    if (a->inst) {
        if (down) { if (*slot) fm1ntt_inst_note_off(a->inst, *slot - 1); *slot = note + 1; fm1ntt_inst_note_on(a->inst, note, 100); }
        else if (*slot) { fm1ntt_inst_note_off(a->inst, *slot - 1); *slot = 0; }
        unlock(a);
        return;
    }
    if (down) { if (*slot) fm1ntt_fm6_note_off(a->pv, *slot - 1); *slot = note + 1; fm1ntt_fm6_note_on(a->pv, note, 100); }
    else if (*slot) { fm1ntt_fm6_note_off(a->pv, *slot - 1); *slot = 0; }
    unlock(a);
}
static int pv_midi(const App *a, int key) { int n = 53 + key + 12 * a->pv_oct; return n < 0 ? 0 : n > 127 ? 127 : n; }
static void pv_toggle(App *a) {
    if (!a->pv_n) { say(a, 2, "No voices to play", NULL); return; }
    a->pv_on = !a->pv_on;
    lock(a);
    a->pv_solo = a->pv_on && !fm1ntt_loaded(a->m);   /* no firmware: there is no machine to hear, so the engine plays alone */
    unlock(a);
    pv_notes_off(a);
    say(a, 0, !a->pv_on ? "Preview off" : a->pv_solo ? "Preview on, no firmware: the engine plays alone" : a->pv_factory ? "Preview on: built-in voices (the firmware has none)" : "Preview on: the keys play the voice", NULL);
}
/* Leaving SOUND turns the preview off and lets its notes go. */
static void pv_leave(App *a) { if (a->panel != PANEL_SOUND) a->ed_on = 0; if (a->panel != PANEL_SOUND && (a->pv_on || a->pv_key[0] || a->pv || a->inst)) { a->pv_on = 0; a->pv_solo = 0; pv_notes_off(a); } }
/* Solo: the machine stops (its audio and screen freeze) and the engine has the whole processor; only with the preview on. */
static void pv_solo_toggle(App *a) {
    if (!a->pv_on) { say(a, 2, "Turn the preview on first (L2)", NULL); return; }
    lock(a);
    a->pv_solo = !a->pv_solo;
    unlock(a);
    say(a, 0, a->pv_solo ? "Solo: the machine is stopped, only the engine plays" : "Solo off: the machine runs again", NULL);
}
static void pv_free(App *a) { if (a->mevq) { fm1ntt_evq_free(a->mevq); a->mevq = NULL; } if (a->evq) { fm1ntt_evq_free(a->evq); a->evq = NULL; } if (a->pv) { fm1ntt_fm6_free(a->pv); a->pv = NULL; } if (a->inst) { fm1ntt_inst_free(a->inst); a->inst = NULL; } }

static int is_dir(const char *p) { struct stat st; return stat(p, &st) == 0 && S_ISDIR(st.st_mode); }
static int is_file(const char *p) { struct stat st; return stat(p, &st) == 0 && S_ISREG(st.st_mode); }

/* The app's folder: only here is anything written. Where it is, in order (docs/ARCHITECTURE.md, r3i6nos docs/APP-DATA.md):
 *   $FM1NTT_DIR                        the person's override (tests and the Rec demo's temporary folder use it); never remembered
 *   $R3I6NOS_APP_DATA                  r3i6nos already decided (a caller, a test)
 *   r3i6nos-appdata path FM1-NTT       r3i6nos's engine: the DATA card (/roms2) when a valid one is in, else the system card (/roms); unattended: it asks and moves nothing
 *   /roms2/FM1-NTT                     no engine (an old image, a laptop): our own check of the same rules says /roms2 is a valid data card
 *   /roms/FM1-NTT, ~/FM1-NTT, FM1-NTT  the system card, the home folder, the current folder
 * Resolved once per run (g_root) and exported as $R3I6NOS_APP_DATA for the worker and what else is started; root_why() says which rule chose it. */
#define APP_FOLDER "FM1-NTT"
static char g_root[300], g_root_why[64];
static int g_root_need_mount = 1;        /* a data card is a mount of its own: the self-test, which has no mounts to make, turns this off */
/* Is `dir` a valid data card? r3i6nos's four rules: a mount of its own (not the system card's folder), a filesystem that is not read-only, a probe file that can be
 * written, synced and removed, and at least 64 MiB free. 1 when valid; else 0 and a short reason. */
static int data_card_ok(const char *dir, int need_mount, char *why, size_t wn) {
    struct stat st, up;
    char p[400];
    if (stat(dir, &st) != 0 || !S_ISDIR(st.st_mode)) { snprintf(why, wn, "NO DATA CARD"); return 0; }
    snprintf(p, sizeof p, "%s/..", dir);
    if (need_mount && stat(p, &up) == 0 && up.st_dev == st.st_dev) { snprintf(why, wn, "NOT A SEPARATE CARD"); return 0; }
    struct statvfs sv;
    if (statvfs(dir, &sv) != 0) { snprintf(why, wn, "CANNOT READ ITS SPACE"); return 0; }
#ifdef ST_RDONLY
    if (sv.f_flag & ST_RDONLY) { snprintf(why, wn, "READ-ONLY"); return 0; }
#endif
    if ((double)sv.f_bavail * (double)sv.f_frsize < 64.0 * 1048576.0) { snprintf(why, wn, "FULL (UNDER 64 MIB)"); return 0; }
    snprintf(p, sizeof p, "%s/.fm1ntt-probe", dir);
    int fd = open(p, O_WRONLY | O_CREAT | O_TRUNC, 0644), ok = fd >= 0 && write(fd, "x", 1) == 1 && fsync(fd) == 0;
    if (fd >= 0) close(fd);
    if (fd >= 0) unlink(p);
    if (!ok) { snprintf(why, wn, "NOT WRITABLE"); return 0; }
    return 1;
}
/* r3i6nos's engine: the folder it says to use, made. 1 when it answered with an absolute path that is a folder. */
static int root_from_engine(char *out, size_t n) {
    const char *eng = getenv("R3I6NOS_APPDATA");
    if (!eng || !*eng) eng = "r3i6nos-appdata";
    if (strchr(eng, '\'') || strlen(eng) > 200) return 0;
    char cmd[300], line[400];
    snprintf(cmd, sizeof cmd, "'%s' path " APP_FOLDER " --create 2>/dev/null", eng);
    FILE *f = popen(cmd, "r");
    if (!f) return 0;
    char *got = fgets(line, sizeof line, f);
    int rc = pclose(f);
    if (!got || rc != 0) return 0;
    line[strcspn(line, "\r\n")] = 0;
    if (line[0] != '/' || strlen(line) >= n || strstr(line, "/../") || !is_dir(line)) return 0;
    snprintf(out, n, "%s", line);
    return 1;
}
/* The choice, with no side effects but what the engine does: `roms2` and `roms` are the two cards' mount points (tests give others). */
static void root_pick(const char *roms2, const char *roms, char *out, size_t n, char *why, size_t wn) {
    const char *pre = getenv("R3I6NOS_APP_DATA"), *h = getenv("HOME");
    char dc[64] = "";
    if (pre && *pre) { snprintf(out, n, "%s", pre); snprintf(why, wn, "SET BY R3I6NOS"); return; }
    char eng[300];
    if (root_from_engine(eng, sizeof eng)) {
        snprintf(out, n, "%s", eng);
        size_t k = strlen(roms2);
        snprintf(why, wn, !strncmp(eng, roms2, k) && eng[k] == '/' ? "R3I6NOS: DATA CARD" : "R3I6NOS: SYSTEM CARD");
        return;
    }
    if (data_card_ok(roms2, g_root_need_mount, dc, sizeof dc)) { snprintf(out, n, "%s/" APP_FOLDER, roms2); snprintf(why, wn, "DATA CARD (%s)", roms2); return; }
    if (is_dir(roms)) {
        snprintf(out, n, "%s/" APP_FOLDER, roms);
        if (strcmp(dc, "NO DATA CARD")) snprintf(why, wn, "SYSTEM CARD: DATA CARD %.28s", dc); else snprintf(why, wn, "SYSTEM CARD (%s)", roms);
        return;
    }
    if (h && *h) { snprintf(out, n, "%s/" APP_FOLDER, h); snprintf(why, wn, "HOME FOLDER"); }
    else { snprintf(out, n, APP_FOLDER); snprintf(why, wn, "CURRENT FOLDER"); }
}
static void base_dir(char *out, size_t n) {
    const char *e = getenv("FM1NTT_DIR");
    if (e && *e) { snprintf(out, n, "%s", e); return; }
    if (!g_root[0]) {
        root_pick("/roms2", "/roms", g_root, sizeof g_root, g_root_why, sizeof g_root_why);
        if (strncmp(g_root_why, "HOME", 4) && strncmp(g_root_why, "CURRENT", 7) && !getenv("R3I6NOS_APP_DATA")) setenv("R3I6NOS_APP_DATA", g_root, 1);   /* the worker and what else is started use the same folder */
    }
    snprintf(out, n, "%s", g_root);
}
/* The folder in use and why, for the screens (nothing is read: the choice was made once). */
static void root_describe(char *root, size_t rn, char *why, size_t wn) {
    base_dir(root, rn);
    const char *e = getenv("FM1NTT_DIR");
    snprintf(why, wn, "%s", e && *e ? "SET BY FM1NTT_DIR" : g_root_why);
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
static char g_self_exe[1024];            /* this program, to start the showcase worker with */
static void self_exe_init(const char *argv0) {
    g_self_exe[0] = 0;
#ifdef __APPLE__
    uint32_t n = sizeof g_self_exe;
    if (_NSGetExecutablePath(g_self_exe, &n) != 0) g_self_exe[0] = 0;
#elif defined(__linux__)
    ssize_t k = readlink("/proc/self/exe", g_self_exe, sizeof g_self_exe - 1);
    if (k > 0) g_self_exe[k] = 0; else g_self_exe[0] = 0;
#endif
    if (!g_self_exe[0] && argv0 && strchr(argv0, '/')) snprintf(g_self_exe, sizeof g_self_exe, "%s", argv0);
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
/* ---- instrument packages (docs/PORTING.md) ----------------------------------------------------------------------------- */
static char g_inst_dir[1024];   /* where the package was read from: the sample files it names are beside it */
static int64_t inst_read(void *user, const char *name, uint8_t *buf, size_t cap) {
    (void)user;
    char path[1400];
    if (!name || strchr(name, '/') || strstr(name, "..") || snprintf(path, sizeof path, "%s/%s", g_inst_dir, name) >= (int)sizeof path) return -1;
    FILE *f = fopen(path, "rb");
    if (!f) return -1;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    int64_t r = -1;
    if (n >= 0 && n <= 64L * 1024 * 1024) {
        if (cap == 0) r = n;
        else if ((size_t)n == cap) { fseek(f, 0, SEEK_SET); if (fread(buf, 1, cap, f) == cap) r = n; }
    }
    fclose(f);
    return r;
}
/* Loads the package at `path` as the preview's engine (the SOUND panel's keys and Solo play it). 0 ok; otherwise the reason is on the status line. The package's licence
 * tier is shown: a copyleft package is the user's own file, played, never shipped from here. */
static int inst_load(App *a, const char *path) {
    char why[160];
    size_t len = 0;
    uint8_t *text = read_file(path, &len, why, sizeof why);
    if (!text) { say(a, 2, "Instrument: %s", why); return -1; }
    const char *slash = strrchr(path, '/');
    size_t dl = slash ? (size_t)(slash - path) : 0;
    if (dl >= sizeof g_inst_dir) dl = sizeof g_inst_dir - 1;
    if (slash) memcpy(g_inst_dir, path, dl);
    else dl = 0, g_inst_dir[0] = '.', dl = 1;
    g_inst_dir[dl] = 0;
    fm1ntt_inst *i = fm1ntt_inst_load(text, len, FM1_SAMPLE_RATE, inst_read, NULL);
    free(text);
    if (!i) { say(a, 2, "Instrument: %s", fm1ntt_error()); return -1; }
    pv_notes_off(a);
    lock(a);
    if (a->inst) fm1ntt_inst_free(a->inst);
    a->inst = i;
    unlock(a);
    a->pv_factory = 0;
    snprintf(a->inst_path, sizeof a->inst_path, "%s", path);
    a->ed_on = 0; a->ed_group = a->ed_sel = a->ed_dirty = 0;
    a->pv_n = (int)fm1ntt_inst_voice_count(i);
    if (a->pv_n < 1) a->pv_n = 1;
    a->pv_sel = (int)fm1ntt_inst_program(i);
    char nm[64];
    fm1ntt_inst_text(i, 0, nm, sizeof nm);
    say(a, 0, "Instrument loaded: %s", nm);
    return 0;
}
/* An instrument from the FILES list: loaded, the SOUND panel shown with the preview on, played alone when no firmware is loaded (the same as --instrument). */
static void open_instrument(App *a, const char *path) {
    if (inst_load(a, path) != 0) return;
    a->panel = PANEL_SOUND;
    a->pv_on = 1;
    lock(a); a->pv_solo = !fm1ntt_loaded(a->m); unlock(a);
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

/* ---- remembered settings: the flash a firmware saved its patches into, kept per firmware (saves/<sha256>.flash) ------------------------------ */
static void persist_path(App *a, char *out, size_t n) { char nm[96]; snprintf(nm, sizeof nm, "%s.flash", a->fl_sha); sub_path(out, n, "saves", nm); }
/* Writes the flash of the loaded package when it changed since the last write (and, with `force`, at once rather than after it has been still for 2 s). */
static void persist_save(App *a, int force, uint32_t now) {
    if (!a->remember || !a->fl_sha[0] || a->fw_raw || !fm1ntt_loaded(a->m)) return;
    if (a->job && a->job_kind == JOB_LOAD) return;          /* the machine is being replaced: its flash is not the old firmware's */
    uint32_t rev = fm1ntt_flash_revision(a->m);
    if (rev != a->fl_rev) { a->fl_rev = rev; a->fl_dirty = 1; a->fl_t = now; }
    if (!a->fl_dirty || (!force && now - a->fl_t < 2000)) return;
    size_t n = fm1ntt_flash_size();
    uint8_t *buf = malloc(n);
    if (!buf) return;
    lock(a);
    size_t got = fm1ntt_flash_save(a->m, buf, n);
    unlock(a);
    char dir[300], p[360];
    sub_path(dir, sizeof dir, "saves", NULL);
    persist_path(a, p, sizeof p);
    if (got == n && mkdir_p(dir) == 0 && write_atomic(p, buf, n) == 0) a->fl_dirty = 0;
    else if (force) a->fl_dirty = 0;                       /* a failed final write is not retried forever */
    free(buf);
}
/* After a load: the remembered flash of this firmware, if there is one of the right size, goes back in (the machine resets to it). */
static void persist_restore(App *a) {
    if (!a->remember || !a->fl_sha[0] || a->fw_raw) return;
    char p[360], why[128] = "";
    size_t n = 0;
    persist_path(a, p, sizeof p);
    if (!is_file(p)) return;
    uint8_t *buf = read_file(p, &n, why, sizeof why);
    if (!buf) return;
    int rc = -1;
    if (n == fm1ntt_flash_size()) { lock(a); rc = fm1ntt_flash_restore(a->m, buf, n); unlock(a); }
    free(buf);
    a->fl_rev = fm1ntt_flash_revision(a->m);
    a->fl_dirty = 0;
    say(a, rc == 0 ? 1 : 2, rc == 0 ? "Booted with your saved settings" : "Saved settings could not be used", NULL);
}
/* A factory reset forgets them. */
static void persist_forget(App *a) {
    char p[360];
    if (!a->fl_sha[0]) return;
    persist_path(a, p, sizeof p);
    unlink(p);
    a->fl_rev = fm1ntt_flash_revision(a->m);
    a->fl_dirty = 0;
}

/* Loading is two phases so the slow one can run on a job thread (FILES): load_boot reads the file and boots the machine (disk and emulator, touches no UI state: its
 * error goes in `why`), load_finish puts the result into the app (the voices, the name, the saved flash) and runs on the main thread. load_firmware is both, in a row,
 * for the start-up splash, the command line and the self-test. */
static void prog_set(const char *doing, int step, int steps);        /* report.h: the progress box's step */
static int load_boot(App *a, const char *path, int raw, uint32_t entry, uint8_t **bufp, size_t *np, char *why, size_t wn) {
    size_t n = 0;
    prog_set("reading the file", 0, 2);
    uint8_t *buf = read_file(path, &n, why, wn);
    if (!buf) return -1;
    prog_set("booting the firmware", 1, 2);
    lock(a);
    int rc = raw ? fm1ntt_load_raw(a->m, buf, n, entry) : fm1ntt_load_package(a->m, buf, n);
    unlock(a);
    if (rc != 0) { snprintf(why, wn, "%s", fm1ntt_error()); free(buf); return -1; }
    *bufp = buf; *np = n;
    return 0;
}
static void load_finish(App *a, const char *path, int raw, uint8_t *buf, size_t n) {   /* frees buf */
    {   /* the voices in its banks, for the SOUND preview (a raw image has no package to look in) */
        static uint8_t found[PV_MAX * 128];
        pv_set(a, found, raw ? 0 : (int)fm1ntt_package_voices(buf, n, found, PV_MAX));
    }
    if (raw) a->fl_sha[0] = 0; else fm1ntt_sha256_hex(buf, n, a->fl_sha);
    free(buf);
    apply_prefs(a);
    a->fb_ok = 0;
    const char *base = strrchr(path, '/');
    snprintf(a->file, sizeof a->file, "%s", base ? base + 1 : path);
    snprintf(a->fw_path, sizeof a->fw_path, "%s", path);
    a->fw_raw = raw;
    a->midi_held = 0;
    a->fl_rev = fm1ntt_flash_revision(a->m);
    a->fl_dirty = 0;
    speed_reset(a);
    say(a, 1, "Booted %s", a->file);
    persist_restore(a);
}
static int load_firmware(App *a, const char *path, int raw, uint32_t entry) {
    char why[128] = "";
    uint8_t *buf = NULL;
    size_t n = 0;
    persist_save(a, 1, SDL_GetTicks());                     /* the firmware being left keeps what it saved */
    if (load_boot(a, path, raw, entry, &buf, &n, why, sizeof why) != 0) { say(a, 3, "%s", why); return -1; }
    load_finish(a, path, raw, buf, n);
    return 0;
}
static void load_start(App *a, const char *path, int raw, uint32_t entry);   /* the same on a job thread, with the progress box (below, with the jobs) */

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
        if ((has_ext(e->d_name, ".fwsc") || has_ext(e->d_name, ".bin") || has_ext(e->d_name, ".fm1i")) && is_file(p) && strlen(e->d_name) < PX_NAME - 4 && strlen(p) < sizeof out->path) {
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
/* The firmware folder (yours: first), then the bundled firmware, whole. */
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
    char idir[300];
    sub_path(idir, sizeof idir, "instruments", NULL);                           /* instruments (.fm1i packages) sit in the same list; A on one plays it */
    n = add_dir(ents, n, idir, 0);
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
    memset(a->fsha, 0, sizeof a->fsha);                         /* the files may have changed: hash them again when asked */
    if (a->info_on && a->info_i >= a->nfiles) a->info_on = 0;
    if (a->file_sel >= a->nfiles) a->file_sel = a->nfiles > 0 ? a->nfiles - 1 : 0;
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
/* ---- the emulation thread: the machine renders ahead into a ring, the callback only reads it -------------------------------- */
/* On a unit that runs the firmware slower than real time the callback used to render inside itself, ran late every time and left the device dry (a rattle). Now the
 * thread renders EMU_CHUNK frames at a time while the ring holds under EMU_TARGET, and the callback copies out what is there and plays silence for what is not (one
 * counted late buffer, not a stall of the device). The latency is the ring's fill, about two buffers. Single producer (the thread), single consumer (the callback). */
#define EMU_RING (1 << 13)              /* frames; a power of two */
#define EMU_CHUNK 256                   /* frames rendered per lock of the machine, so the UI waits at most one chunk */
#define EMU_TARGET (2 * AUDIO_FRAMES)   /* frames kept ahead of the device */
static void emu_discard(App *a) { SDL_AtomicSet(&a->ring_r, SDL_AtomicGet(&a->ring_w)); a->emu_primed = 0; }
static void emu_pull(App *a, int16_t *out, size_t frames) {
    unsigned w = (unsigned)SDL_AtomicGet(&a->ring_w), r = (unsigned)SDL_AtomicGet(&a->ring_r);
    size_t have = (size_t)(w - r), n = have < frames ? have : frames;
    SDL_MemoryBarrierAcquire();
    for (size_t i = 0; i < n; i++) { size_t at = ((size_t)r + i) & (EMU_RING - 1); out[2 * i] = a->emu_ring[2 * at]; out[2 * i + 1] = a->emu_ring[2 * at + 1]; }
    SDL_MemoryBarrierRelease();
    SDL_AtomicSet(&a->ring_r, (int)(r + (unsigned)n));
    if (n < frames) {
        memset(out + 2 * n, 0, (frames - n) * 4);
        if (a->emu_primed) SDL_AtomicAdd(&a->late_cb, 1);          /* the machine did not keep up: a gap of silence, once per buffer */
    }
    if (SDL_SemValue(a->emu_sem) < 2) SDL_SemPost(a->emu_sem);
}
static int emu_main(void *ud) {
    App *a = ud;
    if (a->pin) { int e, u; if (thr_choose(thr_ncpu(), &e, &u)) thr_pin_self(e); }      /* the emulation runs here: the last core */
    SDL_SetThreadPriority(SDL_THREAD_PRIORITY_HIGH);
    int16_t tmp[EMU_CHUNK * 2];
    while (!a->emu_quit) {
        Uint32 wait = 20;
        SDL_LockMutex(a->mx);
        int loaded = fm1ntt_loaded(a->m);
        a->emu_loaded = loaded;
        int solo = a->pv_on && a->pv_solo && (a->pv || a->inst);
        Uint32 idle = a->audio_asleep ? 250 : 20;
        if (!loaded || a->pl.on || solo) { a->emu_primed = 0; wait = idle; }
        else if (a->paused) {
            a->emu_primed = 0;
            if (a->mevq) fm1ntt_apply_events(a->m, a->mevq);          /* an input sent while paused lands now, not in a burst on resume */
            wait = idle;
        } else {
            unsigned w = (unsigned)SDL_AtomicGet(&a->ring_w), r = (unsigned)SDL_AtomicGet(&a->ring_r);
            if ((size_t)(w - r) >= EMU_TARGET) { a->emu_primed = 1; wait = idle; }
            else {
                uint32_t bud = (a->budget ? a->budget : MAX_INSNS_PER_BUFFER) / (AUDIO_FRAMES / EMU_CHUNK);
                if (a->mevq) fm1ntt_render_events(a->m, a->mevq, now_us(), tmp, EMU_CHUNK, bud);
                else fm1ntt_render(a->m, tmp, EMU_CHUNK, bud);
                for (size_t i = 0; i < EMU_CHUNK; i++) { size_t at = ((size_t)w + i) & (EMU_RING - 1); a->emu_ring[2 * at] = tmp[2 * i]; a->emu_ring[2 * at + 1] = tmp[2 * i + 1]; }
                SDL_MemoryBarrierRelease();
                SDL_AtomicSet(&a->ring_w, (int)(w + EMU_CHUNK));
                wait = 0;
            }
        }
        SDL_UnlockMutex(a->mx);
        if (wait) SDL_SemWaitTimeout(a->emu_sem, wait);
    }
    return 0;
}
/* Start it (call once, with the sound device open and still paused, before any firmware plays); 0 ok, -1 not started (the callback then renders itself). */
static int emu_start(App *a) {
    if (a->emu) return 0;
    a->emu_ring = calloc((size_t)EMU_RING * 2, sizeof(int16_t));
    a->mx = SDL_CreateMutex();
    a->emu_sem = SDL_CreateSemaphore(0);
    a->emu_quit = 0;
    SDL_AtomicSet(&a->ring_w, 0); SDL_AtomicSet(&a->ring_r, 0);
    if (a->emu_ring && a->mx && a->emu_sem) a->emu = SDL_CreateThread(emu_main, "emulation", a);
    if (a->emu) return 0;
    if (a->mx) SDL_DestroyMutex(a->mx);
    if (a->emu_sem) SDL_DestroySemaphore(a->emu_sem);
    free(a->emu_ring);
    a->mx = NULL; a->emu_sem = NULL; a->emu_ring = NULL;
    return -1;
}
/* Stop it (the sound device is closed first, so the callback is no longer reading the ring). */
static void emu_stop(App *a) {
    if (!a->emu) return;
    a->emu_quit = 1;
    SDL_SemPost(a->emu_sem);
    SDL_WaitThread(a->emu, NULL);
    a->emu = NULL;
    SDL_DestroyMutex(a->mx); SDL_DestroySemaphore(a->emu_sem);
    free(a->emu_ring);
    a->mx = NULL; a->emu_sem = NULL; a->emu_ring = NULL;
}

/* One buffer of the emulated machine's sound, and the meters, the scope and the recorder's ring that go with it. The callback's whole body,
 * so Rec demo steps the app with the same code. */
static void audio_step(App *a, int16_t *out, size_t frames) {
    if (a->pl.on) {                                                    /* a demo is playing: its sound, the machine waits */
        size_t left = a->pl.frames > a->pl.pos ? a->pl.frames - a->pl.pos : 0, n = left < frames ? left : frames;
        if (n) memcpy(out, a->pl.pcm + a->pl.pos * 2, n * 4);
        if (n < frames) memset(out + n * 2, 0, (frames - n) * 4);
        a->pl.pos += n;
        if (a->pl.pos >= a->pl.frames) a->pl.on = 0;
        a->level_l = a->level_r = 0;
        return;
    }
    int solo = a->pv_on && a->pv_solo && (a->pv || a->inst);      /* the engine alone: the machine is not run, so a unit too slow for the firmware still plays live */
    if (a->emu) {                                                  /* the emulation thread has made the machine's sound: take what is ready, never wait for it */
        if (a->paused || (!solo && !a->emu_loaded)) { emu_discard(a); memset(out, 0, frames * 4); a->level_l = a->level_r = 0; return; }
        if (solo) { emu_discard(a); memset(out, 0, frames * 4); }
        else emu_pull(a, out, frames);
    } else
    if (a->paused || (!solo && !fm1ntt_loaded(a->m))) {
        if (a->mevq && fm1ntt_loaded(a->m)) fm1ntt_apply_events(a->m, a->mevq);      /* an input sent while paused lands now, not in a burst on resume */
        memset(out, 0, frames * 4); a->level_l = a->level_r = 0; return;
    }
    if (a->emu) {}
    else if (solo) memset(out, 0, frames * 4);
    else if (a->mevq) fm1ntt_render_events(a->m, a->mevq, now_us(), out, frames, a->budget ? a->budget : MAX_INSNS_PER_BUFFER);
    else fm1ntt_render(a->m, out, frames, a->budget ? a->budget : MAX_INSNS_PER_BUFFER);
    if (a->fw_mute && !solo) memset(out, 0, frames * 4);                  /* too slow to keep up: silence, not the rattle of a device left dry (the preview below still plays) */
    if (a->pv_on && (a->pv || a->inst)) {   /* the preview's voice on both channels (the caller holds the audio lock, as the machine's keys do) */
        int16_t mono[4096];
        for (size_t at = 0; at < frames; ) {
            size_t n = frames - at < 4096 ? frames - at : 4096;
            if (a->inst && a->evq) fm1ntt_inst_render_events(a->inst, a->evq, now_us(), mono, n);
            else if (a->inst) fm1ntt_inst_render(a->inst, mono, n);
            else fm1ntt_fm6_render(a->pv, mono, n);
            for (size_t i = 0; i < n; i++) for (int ch = 0; ch < 2; ch++) {
                int v = out[2 * (at + i) + ch] + mono[i];
                out[2 * (at + i) + ch] = (int16_t)(v > 32767 ? 32767 : v < -32768 ? -32768 : v);
            }
            at += n;
        }
    }
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
/* Whether the sound device must run now. It sleeps (SDL_PauseAudioDevice) when nothing can make a sound and nobody has touched the unit for AUDIO_QUIET_MS: no
 * firmware running (none loaded, or paused), no preview or Solo, no demo playing, no recording, no job (the report and benchmark count callbacks) and no
 * showcase worker. The power rule: an idle app must not wake the sound hardware 88 times a second. Any event wakes it before the next frame. */
#define AUDIO_QUIET_MS 10000u
static int audio_wanted(const App *a, uint32_t now) {
    if (now - a->last_input_ms < AUDIO_QUIET_MS) return 1;
    if (a->pv_on || a->pl.on || a->rec_on || a->rec || a->job || a->want_demo || a->sw_pid) return 1;
    return fm1ntt_loaded(a->m) && !a->paused;
}
static void audio_sleep_tick(App *a, uint32_t now) {
    if (!a->dev) return;
    int want = audio_wanted(a, now);
    if (want && a->audio_asleep) { SDL_PauseAudioDevice(a->dev, 0); a->audio_asleep = 0; }
    else if (!want && !a->audio_asleep) { SDL_PauseAudioDevice(a->dev, 1); a->audio_asleep = 1; }
}
static void audio_cb(void *ud, Uint8 *stream, int len) {
    App *a = ud;
    SDL_AtomicAdd(&a->callbacks, 1);
    if (a->pin && !a->pinned && !a->emu) { int e, u; a->pinned = 1; if (thr_choose(thr_ncpu(), &e, &u)) thr_pin_self(e); }   /* the emulation runs here: the last core */
    Uint64 t0 = SDL_GetPerformanceCounter();
    audio_step(a, (int16_t *)stream, (size_t)len / 4);
    double ms = (double)(SDL_GetPerformanceCounter() - t0) * 1000.0 / (double)SDL_GetPerformanceFrequency();
    if (ms > (double)(len / 4) * 1000.0 / FM1_SAMPLE_RATE) SDL_AtomicAdd(&a->late_cb, 1);       /* longer than the buffer it fills: the sound has a gap */
}

/* ---- the machine's inputs ---------------------------------------------------------------------------------------------- */
/* The machine's inputs go to the audio thread through its queue (core/src/evq.rs) while the callback runs and a firmware is loaded: no lock, each applied at its frame, in the
 * order sent. Otherwise (no audio device, the Rec demo, which steps its own clock) they go straight in under the lock. The callback also drains the queue while paused. */
static int mq_on(const App *a) { return a->mevq && a->mq_live && fm1ntt_loaded(a->m); }
static void key_id(App *a, int id, int down) {
    if (mq_on(a)) { fm1ntt_evq_push_key(a->mevq, now_us(), id, down); return; }
    lock(a); fm1ntt_key(a->m, id, down); unlock(a);
}
static void enc(App *a, int e, int clicks) {
    if (mq_on(a)) fm1ntt_evq_push_encoder(a->mevq, now_us(), e, clicks);
    else { lock(a); fm1ntt_encoder(a->m, e, clicks); unlock(a); }
    a->enc_turns[e] += clicks;
}
static void pot(App *a, int by) {
    a->pot = a->pot + by < 0 ? 0 : a->pot + by > 1023 ? 1023 : a->pot + by;
    if (mq_on(a)) { fm1ntt_evq_push_pot(a->mevq, now_us(), a->pot); return; }
    lock(a); fm1ntt_pot(a->m, a->pot); unlock(a);
}
static void midi_send_at(App *a, const uint8_t *b, int n, int log, uint64_t t) {
    if (mq_on(a)) {          /* no lock: the callback puts each message into the jack at its own frame (core/src/evq.rs) */
        for (int i = 0; i < n; i += 3) fm1ntt_evq_push(a->mevq, t, b + i, (size_t)(n - i < 3 ? n - i : 3));
    } else {
        lock(a);
        for (int i = 0; i < n; i++) fm1ntt_midi_in(a->m, b[i]);
        unlock(a);
    }
    for (int i = 0; i < n && log; i++) {
        if (a->midi_log_n == PX_MIDI_LOG) { memmove(a->midi_log, a->midi_log + 1, PX_MIDI_LOG - 1); a->midi_log_n--; }
        a->midi_log[a->midi_log_n++] = b[i];
    }
}
static void midi_send(App *a, const uint8_t *b, int n, int log) { midi_send_at(a, b, n, log, now_us()); }
/* A message from a USB MIDI device, with the time it arrived. It plays the instrument when one is on in SOUND (arpeggiator, sequencer and clock included), the
 * built-in voice when that is what the preview plays, else it goes into the machine's MIDI jack like the MIDI panel's own bytes. Clock bytes are not logged. */
static void midi_in_route(App *a, const MidiMsg *m) {
    a->last_input_ms = SDL_GetTicks();
    a->min_routed++;
    if (a->inst && a->evq && a->pv_on) { fm1ntt_evq_push(a->evq, m->ts, m->b, m->n); return; }
    if (a->pv && !a->inst && a->pv_on) {
        int st = m->b[0] & 0xF0;
        lock(a);
        if (st == 0x90 && m->b[2]) fm1ntt_fm6_note_on(a->pv, m->b[1], m->b[2]);
        else if (st == 0x80 || st == 0x90) fm1ntt_fm6_note_off(a->pv, m->b[1]);
        unlock(a);
        return;
    }
    if (fm1ntt_loaded(a->m)) midi_send_at(a, m->b, m->n, m->b[0] < 0xF8, m->ts);
}
static void midi_in_pump(App *a) {
    MidiMsg m;
    if (!a->min_on) return;
    for (int i = 0; i < 256 && midi_in_pop(&a->min, &m); i++) midi_in_route(a, &m);
}
static void panic_all(App *a) {
    uint8_t m[3];
    for (int ch = 0; ch < 16; ch++) {
        m[0] = (uint8_t)(0xB0 | ch); m[1] = 123; m[2] = 0; midi_send(a, m, 3, ch == 0);
        m[1] = 120; midi_send(a, m, 3, 0);
    }
    for (int id = 0; id < 41; id++) key_id(a, id, 0);
    a->midi_held = 0;
    say(a, 1, "All notes off on every channel; every key released", NULL);
}
static void midi_ok(App *a, int down) {
    uint8_t m[3] = {(uint8_t)((down ? 0x90 : 0x80) | a->midi_chan), (uint8_t)a->midi_note, (uint8_t)(down ? a->midi_vel : 0)};
    if (down == a->midi_held) return;
    midi_send(a, m, 3, 1);
    a->midi_held = down;
}

/* ---- the instrument editor: any engine's, the arpeggiator's and each effect's parameters, generic over what the core describes (core/src/engine.rs ParamDesc) ------------------ */
/* groups: 0 the engine, 1 the arpeggiator, 2 the sequencer's settings, 3 the pattern being edited (its voice and steps), 4 the global settings (channels, bend range, velocity, mono), then one for each effect */
enum { ED_GLOBAL = 4, ED_FX0 = 5 };
static int ed_groups(const App *a) { return ED_FX0 + (int)fm1ntt_inst_fx_count(a->inst); }
/* One extra row ends a group that has something to choose besides parameters (no new keys: Left and Right change it like any row): the engine's group gets BANK when the
 * instrument has more than one bank of voices (same slot, another bank), and an effect's group gets POSITION when the chain has more than one effect (Left and Right move it
 * earlier or later, and the editor follows it). Neither is a stored parameter: the bank is not saved, the order is (the package writes the chain in order). */
enum { ED_V_ID = 100000 };
static int ed_virtual(const App *a, int g) {
    if (!a->inst) return 0;
    if (g == 0) return fm1ntt_inst_bank_count(a->inst) > 1;
    return g >= ED_FX0 && fm1ntt_inst_fx_count(a->inst) > 1;
}
static int ed_count_real(const App *a, int g) {
    if (!a->inst) return 0;
    return (int)(g == 0 ? fm1ntt_inst_param_count(a->inst) : g == 1 ? fm1ntt_inst_arp_count(a->inst) : g == ED_GLOBAL ? fm1ntt_inst_global_count(a->inst) : g < ED_FX0 ? fm1ntt_inst_seq_count(a->inst, (size_t)g - 2) : fm1ntt_inst_fx_param_count(a->inst, (size_t)g - ED_FX0));
}
static int ed_count(const App *a, int g) { return ed_count_real(a, g) + ed_virtual(a, g); }
/* Parameter i of group g: { id, min, max, default } and its name; 0 ok. */
static int ed_info(const App *a, int g, int i, int32_t info[4], char *name, size_t cap) {
    if (!a->inst || i < 0) return -1;
    if (ed_virtual(a, g) && i == ed_count_real(a, g)) {
        int top = g == 0 ? (int)fm1ntt_inst_bank_count(a->inst) - 1 : (int)fm1ntt_inst_fx_count(a->inst) - 1;
        info[0] = ED_V_ID; info[1] = 0; info[2] = top; info[3] = 0;
        snprintf(name, cap, "%s", g == 0 ? "BANK (0 FIRST)" : "POSITION (0 FIRST)");
        return 0;
    }
    return g == 0 ? fm1ntt_inst_param(a->inst, (size_t)i, info, name, cap) : g == 1 ? fm1ntt_inst_arp_param(a->inst, (size_t)i, info, name, cap)
         : g == ED_GLOBAL ? fm1ntt_inst_global_param(a->inst, (size_t)i, info, name, cap) : g < ED_FX0 ? fm1ntt_inst_seq_param(a->inst, (size_t)g - 2, (size_t)i, info, name, cap) : fm1ntt_inst_fx_param(a->inst, (size_t)g - ED_FX0, (size_t)i, info, name, cap);
}
static int ed_get(const App *a, int g, int id) {
    if (id == ED_V_ID) return g == 0 ? (int)fm1ntt_inst_bank(a->inst) : g - ED_FX0;
    return g == 0 ? fm1ntt_inst_get(a->inst, id) : g == 1 ? fm1ntt_inst_arp_get(a->inst, id) : g == ED_GLOBAL ? fm1ntt_inst_global_get(a->inst, id) : g < ED_FX0 ? fm1ntt_inst_seq_get(a->inst, id) : fm1ntt_inst_fx_get(a->inst, (size_t)g - ED_FX0, id);
}
static void ed_title(const App *a, int g, char *out, size_t n) {
    if (g == 0) { char k[16]; fm1ntt_inst_text(a->inst, 1, k, sizeof k); for (char *q = k; *q; q++) if (*q >= 'a' && *q <= 'z') *q = (char)(*q - 32); snprintf(out, n, "ENGINE %s", k); }
    else if (g == 1) snprintf(out, n, "ARPEGGIATOR");
    else if (g == 2) snprintf(out, n, "SEQUENCER");
    else if (g == ED_GLOBAL) snprintf(out, n, "GLOBAL");
    else if (g == 3) { char pn[16]; snprintf(pn, sizeof pn, "%d", fm1ntt_inst_seq_get(a->inst, 7)); snprintf(out, n, "PATTERN %s", pn); }
    else { char f[24]; fm1ntt_inst_fx_name(a->inst, (size_t)g - ED_FX0, f, sizeof f); for (char *q = f; *q; q++) if (*q >= 'a' && *q <= 'z') *q = (char)(*q - 32); snprintf(out, n, "EFFECT %s", f); }
}
static void ed_set(App *a, int g, int id, int v) {
    lock(a);
    if (id == ED_V_ID) {
        if (g == 0) fm1ntt_inst_bank_set(a->inst, (size_t)v);          /* not an edit of the package: the bank is where the player is */
        else if (v != g - ED_FX0 && fm1ntt_inst_fx_move(a->inst, (size_t)g - ED_FX0, (size_t)v) == 0) { a->ed_group = ED_FX0 + v; a->ed_dirty = 1; }
        unlock(a);
        return;
    }
    if (g == 0) fm1ntt_inst_set(a->inst, id, v);
    else if (g == 1) fm1ntt_inst_arp_set(a->inst, id, v);
    else if (g == ED_GLOBAL) fm1ntt_inst_global_set(a->inst, id, v);
    else if (g < ED_FX0) fm1ntt_inst_seq_set(a->inst, id, v);
    else fm1ntt_inst_fx_set(a->inst, (size_t)g - ED_FX0, id, v);
    unlock(a);
    a->ed_dirty = 1;
}
/* Moves the chosen parameter's value by delta (clamped by the core), or back to its default (reset). */
static void ed_change(App *a, int delta, int reset) {
    int32_t info[4]; char nm[PX_NAME];
    if (ed_info(a, a->ed_group, a->ed_sel, info, nm, sizeof nm) != 0) return;
    int v = reset ? info[3] : ed_get(a, a->ed_group, info[0]) + delta;
    ed_set(a, a->ed_group, info[0], v < info[1] ? info[1] : v > info[2] ? info[2] : v);
}
/* Writes the instrument, edits included, to instruments/<name>-edit.fm1i (a temporary name, then a rename): never over the file it was loaded from. */
static void ed_save(App *a) {
    char nm[64], file[96], dir[300], path[400];
    fm1ntt_inst_text(a->inst, 0, nm, sizeof nm);
    size_t k = 0;
    for (const char *q = nm; *q && k < 40; q++) {
        char ch = *q >= 'A' && *q <= 'Z' ? (char)(*q + 32) : *q;
        if ((ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9')) file[k++] = ch;
        else if (k && file[k - 1] != '-') file[k++] = '-';
    }
    while (k && file[k - 1] == '-') k--;
    if (!k) { memcpy(file, "instrument", 10); k = 10; }
    snprintf(file + k, sizeof file - k, "-edit.fm1i");
    sub_path(dir, sizeof dir, "instruments", NULL);
    sub_path(path, sizeof path, "instruments", file);
    lock(a);
    size_t n = fm1ntt_inst_to_package(a->inst, a->inst_path, NULL, 0);
    uint8_t *buf = n ? malloc(n) : NULL;
    if (buf) fm1ntt_inst_to_package(a->inst, a->inst_path, buf, n);
    unlock(a);
    if (!buf) { say(a, 2, "Cannot save: nothing to write", NULL); return; }
    if (mkdir_p(dir) == 0 && write_atomic(path, buf, n) == 0) { say(a, 1, "Saved instruments/%s", file); a->ed_dirty = 0; scan_files(a); }
    else say(a, 2, "Cannot write the instruments folder", NULL);
    free(buf);
}
static void ed_press(App *a, int act) {
    if (!a->inst) { a->ed_on = 0; return; }
    int groups = ed_groups(a), n = ed_count(a, a->ed_group);
    int32_t info[4]; char nm[PX_NAME];
    int big = 10;
    if (ed_info(a, a->ed_group, a->ed_sel, info, nm, sizeof nm) == 0) { big = (info[2] - info[1]) / 10; if (big < 10) big = 10; }
    switch (act) {
    case A_UP: if (n) a->ed_sel = (a->ed_sel + n - 1) % n; break;
    case A_DOWN: if (n) a->ed_sel = (a->ed_sel + 1) % n; break;
    case A_LEFT: ed_change(a, -1, 0); break;
    case A_RIGHT: ed_change(a, 1, 0); break;
    case A_X: ed_change(a, -big, 0); break;
    case A_Y: ed_change(a, big, 0); break;
    case A_SELECT: a->ed_group = (a->ed_group + groups - 1) % groups; a->ed_sel = 0; break;
    case A_START: a->ed_group = (a->ed_group + 1) % groups; a->ed_sel = 0; break;
    case A_OK: ed_save(a); break;
    case A_BACK: a->ed_on = 0; break;
    default: break;
    }
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
    persist_forget(a);                                      /* a reset is a factory reset: what the firmware had saved goes too */
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
    case 14: a->remember = !a->remember; a->fl_dirty = 0; if (a->remember) a->fl_rev = fm1ntt_flash_revision(a->m); say(a, 0, a->remember ? "Remembering the firmware's saved settings between runs" : "Saved settings are no longer kept", NULL); break;
    case 13: a->bg_demos = !a->bg_demos; say(a, 0, a->bg_demos ? "Background demos on: one at a time, paused while you play" : "Background demos off", NULL); break;
    case 12: a->rain_on = !a->rain_on; if (a->rain_on && a->reduced_motion) rain_still(a); say(a, 0, a->rain_on ? (a->reduced_motion ? "Data rain on, as a still page (reduced motion)" : "Data rain on: it shows on the MONITOR panel") : "Data rain off", NULL); break;
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
    int rows = a->nfiles > 0 ? a->nfiles : 1;                       /* the rows of FILES (Quit is on TOOLS); never zero, so the wrap has a divisor */
    /* Start pauses on every panel that has no use for it (MIDI's and SOUND's are their own); Page up and down are R1 + up and down (docs/r3i6nos-mapping.md) */
    if (act == A_START && a->panel != PANEL_SOUND && a->panel != PANEL_MIDI) { a->paused = !a->paused; say(a, 0, a->paused ? "Paused (Start resumes)" : "Running", NULL); return; }
    switch (a->panel) {
    case PANEL_FILES:
        if (files_press(a, act)) break;                                     /* the firmware's page, when it is open or A opens it */
        if (act == A_HOME) a->file_sel = 0;
        else if (act == A_END) a->file_sel = rows - 1;
        else if (act == A_UP) a->file_sel = (a->file_sel + rows - 1) % rows;
        else if (act == A_DOWN) a->file_sel = (a->file_sel + 1) % rows;
        else if (act == A_PGUP) a->file_sel = a->file_sel - (a->mode == MODE_ZOOMED ? 20 : 22) < 0 ? 0 : a->file_sel - (a->mode == MODE_ZOOMED ? 20 : 22);
        else if (act == A_PGDN) a->file_sel = a->file_sel + (a->mode == MODE_ZOOMED ? 20 : 22) >= rows ? rows - 1 : a->file_sel + (a->mode == MODE_ZOOMED ? 20 : 22);
        else if (act == A_X) { scan_files(a); say(a, 0, "Rescanned the folder", NULL); }
        else if (act == A_OK) {
            if (a->file_sel < a->nfiles) {
                const char *p = a->fpaths[a->file_sel];
                if (has_ext(p, ".fm1i")) open_instrument(a, p);
                else load_start(a, p, has_ext(p, ".bin"), FM1_ENTRY_FELUCCA);
            }
        }
        break;
    case PANEL_CPU:
        if (act == A_UP || act == A_DOWN || act == A_LEFT || act == A_RIGHT) a->cpu_sel ^= 1;
        break;
    case PANEL_MEMORY:
        if (act == A_UP) mem_move(a, -8);
        else if (act == A_DOWN) mem_move(a, 8);
        else if (act == A_PGUP) mem_move(a, -(long)mem_page(a));
        else if (act == A_PGDN) mem_move(a, (long)mem_page(a));
        else if (act == A_X) mem_move(a, -0x10000);
        else if (act == A_Y) mem_move(a, 0x10000);
        else if (act == A_HOME) a->mem_off = 0;
        else if (act == A_END) mem_move(a, 1L << 30);
        else if (act == A_LEFT || act == A_RIGHT) { a->mem_region = (a->mem_region + (act == A_RIGHT ? 1 : 2)) % 3; a->mem_off = 0; }
        break;
    case PANEL_MIDI:
        if (act == A_LEFT && a->midi_note > 0) a->midi_note--;
        else if (act == A_RIGHT && a->midi_note < 127) a->midi_note++;
        else if (act == A_UP) a->midi_note = a->midi_note + 12 > 127 ? 127 : a->midi_note + 12;
        else if (act == A_DOWN) a->midi_note = a->midi_note - 12 < 0 ? 0 : a->midi_note - 12;
        else if (act == A_X) a->midi_vel = a->midi_vel - 8 < 1 ? 1 : a->midi_vel - 8;
        else if (act == A_Y) a->midi_vel = a->midi_vel + 8 > 127 ? 127 : a->midi_vel + 8;
        else if (act == A_SELECT) a->midi_chan = (a->midi_chan + 15) % 16;
        else if (act == A_START) a->midi_chan = (a->midi_chan + 1) % 16;
        else if (act == A_BACK) panic_all(a);
        break;
    case PANEL_SOUND:
        if (a->ed_on) { ed_press(a, act); break; }
        if (act == A_BACK && a->inst) { a->ed_on = 1; a->ed_group = 0; a->ed_sel = 0; break; }
        if (act == A_START) pv_toggle(a);
        else if (act == A_SELECT) pv_solo_toggle(a);
        else if (a->pv_on && act == A_UP) pv_select(a, a->pv_sel - 1);
        else if (a->pv_on && act == A_DOWN) pv_select(a, a->pv_sel + 1);
        else if (a->pv_on && act == A_LEFT) { if (a->pv_oct > -2) a->pv_oct--; }
        else if (a->pv_on && act == A_RIGHT) { if (a->pv_oct < 2) a->pv_oct++; }
        else if (act == A_OK) { if (a->rec) rec_stop(a); else rec_start(a); }
        else if (act == A_X) screenshot(a);
        else if (act == A_Y) { a->peak_l = a->peak_r = 0; say(a, 0, "Peaks cleared", NULL); }
        break;
    case PANEL_MONITOR: {
        int narrow = a->mode == MODE_ZOOMED, pages = mon_pages(narrow, a->mon.v.nprocs);
        if (act == A_HOME || act == A_END) { a->mon_page = act == A_HOME ? 0 : pages - 1; a->mon.force = 1; }
        else if (act == A_PGUP || act == A_PGDN) { a->mon_page = (a->mon_page + (act == A_PGDN ? 1 : pages - 1)) % pages; a->mon.force = 1; }
        else if (act == A_X) { a->mon.v.sort = (a->mon.v.sort + 1) % MON_SORTS; a->mon.force = 1; { static const char *const sn[MON_SORTS] = {"Sorted by CPU", "Sorted by memory", "Sorted by PID"}; say(a, 0, sn[a->mon.v.sort % MON_SORTS], NULL); } }
        else if (act == A_Y) { a->mon.paused = !a->mon.paused; a->mon.force = 0; say(a, 0, a->mon.paused ? "Monitor frozen: nothing is read" : "Monitor running", NULL); }
        break;
    }
    case PANEL_TOOLS:
        if (act == A_UP || act == A_DOWN || act == A_PGUP || act == A_PGDN || act == A_HOME || act == A_END) a->tool_sel = pm_list_step(a->tool_sel, NTOOLS, NTOOLS, act);
        else if (act == A_LEFT) a->slot = (a->slot + 2) % 4 + 1;
        else if (act == A_RIGHT) a->slot = a->slot % 4 + 1;
        else if (act == A_OK) tools_run(a);
        break;
    default: break;
    }
}

/* ---- the pad's bindings, and the Keymap that shows, tests and changes them (r3i6nos rule 17) ------------------------------------------------------------------------
 * Ten buttons are the app's to bind: B A X Y, the d-pad, Select and Start. L1, R1, L2, R2, Fn, L3 and R3 are the system's (the mapping) and are never bound. Each panel has its own
 * binding, a permutation of the ten buttons over ten SLOTS (a slot is named for the button that has it by default), so a remap is always a swap and two actions never share
 * a button. What a slot does is the panel's own (SLOT_LABEL); off MACHINE a slot is an action (SLOT_ACT), and R1 held turns the d-pad's four into pages and Home and End. */
#define NSLOT 10
enum { SL_UP, SL_LEFT, SL_DOWN, SL_RIGHT, SL_Y, SL_B, SL_A, SL_X, SL_SELECT, SL_START };
static const int DEF_BTN[NSLOT] = {8, 10, 9, 11, 3, 0, 1, 2, 12, 13};                    /* raw buttons: up left down right Y B A X Select Start (MACHINE's note order) */
static const int SLOT_ACT[NSLOT] = {A_UP, A_LEFT, A_DOWN, A_RIGHT, A_Y, A_BACK, A_OK, A_X, A_SELECT, A_START};
static int g_bind[PANEL_COUNT][NSLOT], g_bind_ready;                                     /* set up on first use, so no path has to remember to */
static void keymap_reset_all(void) { for (int p = 0; p < PANEL_COUNT; p++) memcpy(g_bind[p], DEF_BTN, sizeof DEF_BTN); g_bind_ready = 1; }
static int is_bindable(int b) { for (int i = 0; i < NSLOT; i++) if (DEF_BTN[i] == b) return 1; return 0; }
/* The slot a raw button has on a panel (panel -1: the defaults, which the menus and a question use), or -1. */
static int slot_of(int panel, int b) {
    if (!g_bind_ready) keymap_reset_all();
    const int *t = panel >= 0 && panel < PANEL_COUNT ? g_bind[panel] : DEF_BTN;
    for (int i = 0; i < NSLOT; i++) if (t[i] == b) return i;
    return -1;
}
/* The action a pad button makes off MACHINE, R1 held or not. */
static int pad_action(int panel, int b, int r1) { int sl = slot_of(panel, b); return sl < 0 ? A_NONE : pm_r1(SLOT_ACT[sl], r1); }
static int act_of_pad(int b) { return pad_action(-1, b, 0); }
static int act_of_pad_r1(int b, int r1) { return pad_action(-1, b, r1); }
/* A button's name, for the Keymap. */
static const char *btn_name(int b) {
    static const char *const n[17] = {"B", "A", "X", "Y", "L1", "R1", "L2", "R2", "UP", "DOWN", "LEFT", "RIGHT", "SELECT", "START", "L3", "R3", "FN"};
    return b >= 0 && b < 17 ? n[b] : "?";
}
/* What each slot does on each panel, for the Keymap ("" none). MACHINE's slots are notes 1 to 8, then OCTAVE - and +; the other panels follow act(). */
static const char *const SLOT_LABEL[PANEL_COUNT][NSLOT] = {
    /*        UP             LEFT            DOWN            RIGHT          Y                  B                 A                X                  SELECT           START */
    /*MACHINE*/ {"NOTE 1 (C)", "NOTE 2 (D)", "NOTE 3 (E)", "NOTE 4 (F)", "NOTE 5 (G)", "NOTE 6 (A)", "NOTE 7 (B)", "NOTE 8 (C)", "OCTAVE -", "OCTAVE +"},
    /*FILES*/   {"PREVIOUS FILE", "", "NEXT FILE", "", "MAKE OR STOP A DEMO", "BACK FROM A PAGE", "OPEN, LOAD", "RESCAN, PLAY DEMO", "", "PAUSE"},
    /*CPU*/     {"OTHER CORE", "OTHER CORE", "OTHER CORE", "OTHER CORE", "", "", "", "", "", "PAUSE"},
    /*MEMORY*/  {"ONE ROW UP", "PREVIOUS REGION", "ONE ROW DOWN", "NEXT REGION", "64 KIB FORTH", "", "", "64 KIB BACK", "", "PAUSE"},
    /*CONSOLE*/ {"", "", "", "", "", "", "", "", "", "PAUSE"},
    /*MIDI*/    {"OCTAVE UP", "NOTE -", "OCTAVE DOWN", "NOTE +", "VELOCITY +", "ALL NOTES OFF", "NOTE ON (HOLD)", "VELOCITY -", "CHANNEL -", "CHANNEL +"},
    /*SOUND*/   {"VOICE UP", "OCTAVE -", "VOICE DOWN", "OCTAVE +", "PEAKS / ROOT NOTE", "EDITOR / THIRD", "RECORD / FIFTH", "SHOT / OCTAVE NOTE", "SOLO ON OR OFF", "PREVIEW ON OR OFF"},
    /*MONITOR*/ {"", "", "", "", "FREEZE OR RESUME", "", "", "SORT", "", "PAUSE"},
    /*TOOLS*/   {"PREVIOUS TOOL", "SLOT -", "NEXT TOOL", "SLOT +", "", "NO TO A QUESTION", "RUN", "", "", "PAUSE"},
};
/* MACHINE under a held button: the same slots, other jobs (L1 the ten black-key functions, L2 the FM-1's second row, R2 the transport, R1 the key and the octave). */
static const char *const MOD_LABEL[4][NSLOT] = {
    {"OP1", "OP2", "OP3", "OP4", "OP5", "OP6", "PIT", "GLO", "MONO", "POLY"},
    {"SCL", "FX", "HOME", "SAVE", "ENV", "LFO", "ARP", "SEQ", "KEY DOWN", "KEY UP"},
    {"GLO", "EDIT", "PLAY / STOP", "REC", "", "", "", "", "", ""},
    {"KEY UP", "OCTAVE -", "KEY DOWN", "OCTAVE +", "", "", "", "", "", ""},
};
static const char *const MOD_NAME[4] = {"+ L1", "+ L2", "+ R2", "+ R1"};
/* The Keymap's pages: the system's (fixed), each panel's bindings, MACHINE's held-button sets, and the test. ctx is a panel, KM_SYS or KM_TEST; mod 0 none, 1..4 L1 L2 R2 R1. */
enum { KM_SYS = -2, KM_TEST = -3 };
typedef struct { int ctx, mod, part; } KmPage;
#define KM_MAXP 24
static const char *const KM_SYS_ROWS[][2] = {
    {"L1 + R1", "MAIN MENU"}, {"L2 + R2", "CONTEXT MENU"}, {"FN", "NEXT PANEL"}, {"L1 + FN", "FIRST PANEL"}, {"R1 + FN", "PANEL BEFORE (AGAIN: BACK)"},
    {"` KEY", "MAIN MENU"}, {"\\ KEY", "CONTEXT MENU"}, {"HOME / END KEYS", "PANEL BEFORE / NEXT"}, {"TAB, F1", "ZOOM 1X / 2X"}, {"P", "PAUSE"}, {"ESC", "CLOSES A MENU"},
    {"R1 + D-PAD UP DOWN", "PAGE UP, PAGE DOWN"}, {"R1 + D-PAD LEFT RIGHT", "HOME, END"}, {"R1 + RIGHT STICK", "THE SAME, 6 A SECOND"}, {"RIGHT STICK", "SELECTION (NOT MACHINE)"},
    {"STICKS ON MACHINE", "KNOBS 1 TO 4"}, {"L3", "NO VIRTUAL MOUSE"}, {"R3", "NO VIRTUAL KEYBOARD"}, {"L1 L2 R1 R2 FN L3 R3", "THE SYSTEM'S, NEVER BOUND"},
    {"IN A MENU", "D-PAD, A, B (FIXED)"}, {"QUIT", "THE MAIN MENU ONLY"},
};
#define KM_SYS_N ((int)(sizeof KM_SYS_ROWS / sizeof *KM_SYS_ROWS))
#define KM_SYS_PER 11
static int km_pages(KmPage out[KM_MAXP]) {
    int n = 0;
    for (int part = 0; part * KM_SYS_PER < KM_SYS_N; part++) out[n++] = (KmPage){KM_SYS, 0, part};
    for (int p = 0; p < PANEL_COUNT; p++) {
        int any = p == PANEL_MACHINE;
        for (int i = 0; i < NSLOT && !any; i++) if (SLOT_LABEL[p][i][0] && i != SL_START) any = 1;
        if (!any) continue;
        out[n++] = (KmPage){p, 0, 0};
        if (p == PANEL_MACHINE) for (int m = 1; m <= 4; m++) out[n++] = (KmPage){p, m, 0};
    }
    out[n++] = (KmPage){KM_TEST, 0, 0};
    return n;
}
/* The bindings are kept in keymap.txt in the app's folder: a line "PANEL b b b b b b b b b b" for each panel that is not the default. */
static void keymap_path(char *out, size_t n) { char b[300]; base_dir(b, sizeof b); snprintf(out, n, "%s/keymap.txt", b); }
static void keymap_save(void) {
    if (!g_bind_ready) keymap_reset_all();
    char path[340], d[300], text[PANEL_COUNT * 120];
    size_t len = 0;
    text[0] = 0;
    for (int p = 0; p < PANEL_COUNT; p++) {
        if (!memcmp(g_bind[p], DEF_BTN, sizeof DEF_BTN)) continue;
        len += (size_t)snprintf(text + len, sizeof text - len, "%s", PX_PANEL_NAMES[p]);
        for (int i = 0; i < NSLOT; i++) len += (size_t)snprintf(text + len, sizeof text - len, " %d", g_bind[p][i]);
        len += (size_t)snprintf(text + len, sizeof text - len, "\n");
    }
    keymap_path(path, sizeof path);
    base_dir(d, sizeof d);
    if (mkdir_p(d) == 0) write_atomic(path, text, len);
}
/* One saved line, accepted only when it is a permutation of the ten bindable buttons (a hand-edited or damaged file never leaves a button double-bound or a system button taken). */
static int keymap_parse_line(const char *line) {
    char name[40];
    int v[NSLOT], used = 0, off = 0;
    if (sscanf(line, "%39s%n", name, &off) != 1) return -1;
    int pn = -1;
    for (int p = 0; p < PANEL_COUNT; p++) if (!strcmp(name, PX_PANEL_NAMES[p])) pn = p;
    if (pn < 0) return -1;
    for (int i = 0; i < NSLOT; i++) {
        int k = 0;
        if (sscanf(line + off, "%d%n", &v[i], &k) != 1 || !is_bindable(v[i]) || (used >> v[i] & 1)) return -1;
        used |= 1 << v[i];
        off += k;
    }
    memcpy(g_bind[pn], v, sizeof v);
    return pn;
}
static void keymap_load(void) {
    char path[340], why[64];
    size_t n = 0;
    keymap_path(path, sizeof path);
    if (!g_bind_ready) keymap_reset_all();
    uint8_t *b = read_file(path, &n, why, sizeof why);
    if (!b) return;
    char *txt = malloc(n + 1);
    if (txt) {
        memcpy(txt, b, n);
        txt[n] = 0;
        for (char *l = strtok(txt, "\n"); l; l = strtok(NULL, "\n")) keymap_parse_line(l);
        free(txt);
    }
    free(b);
}

/* ---- the overlay menus: the main menu (L1 + R1), the context menu (L2 + R2), About, the colour theme (r3i6nos rule 17, docs/r3i6nos-mapping.md) ------------------------
 * The main menu's first items are fixed, in this order: About, Colour theme, Keymap, Device report, Rec demo, Benchmark, Quit; the app's own follow (here: Tools, the panel
 * that holds the rest). Never renamed, reordered or missing; Benchmark would be hidden only when there was nothing to measure, and there is (TOOLS > Benchmark). */
enum { MM_ABOUT, MM_THEME, MM_KEYMAP, MM_REPORT, MM_DEMO, MM_BENCH, MM_QUIT, MM_TOOLS, MM_COUNT };
static const char *const MM_LABEL[MM_COUNT] = {"ABOUT", "COLOUR THEME", "KEYMAP", "DEVICE REPORT", "REC DEMO", "BENCHMARK", "QUIT", "TOOLS"};
#define ABOUT_LINES 6

/* The panel changes, whoever asked (the carousel, a chord, the menu): the one before is remembered for R1 + Fn, and what the old panel held is let go. */
static void panel_switch(App *a, int p) {
    if (p < 0 || p >= PANEL_COUNT) return;
    for (int b = 0; b < 32; b++) if (a->pad_sent[b]) { key_id(a, a->pad_sent[b] - 1, 0); a->pad_sent[b] = 0; }
    if (p != a->panel) { a->panel_prev = a->panel + 1; a->panel = p; }
    pv_leave(a);
    if (a->panel != PANEL_MIDI && a->midi_held) midi_ok(a, 0);
}
/* R1 + Fn: the panel shown before this one (a history, not "number minus one"); pressed twice it comes back. */
static void panel_back(App *a) {
    if (a->panel_prev <= 0) { say(a, 0, "No earlier panel yet", NULL); return; }
    panel_switch(a, a->panel_prev - 1);
}

/* The colour theme is kept in the app's folder (theme.txt, temp name then rename), so it survives a restart. */
static void theme_path(char *out, size_t n) { char b[300]; base_dir(b, sizeof b); snprintf(out, n, "%s/theme.txt", b); }
static void theme_save(void) {
    char p[340], line[64];
    theme_path(p, sizeof p);
    snprintf(line, sizeof line, "%s\n", px_themes[px_theme_now].name);
    char d[300];
    base_dir(d, sizeof d);
    if (mkdir_p(d) == 0) write_atomic(p, line, strlen(line));
}
static void theme_load(void) {
    char p[340], why[64];
    size_t n = 0;
    theme_path(p, sizeof p);
    uint8_t *b = read_file(p, &n, why, sizeof why);
    if (!b) return;
    char name[PX_NAME] = {0};
    for (size_t i = 0; i < n && i < sizeof name - 1 && b[i] != '\n' && b[i] != '\r'; i++) name[i] = (char)b[i];
    free(b);
    int i = px_theme_find(name);
    if (i >= 0) px_theme_apply(i);
}

/* What the context menu holds on each panel: the panel's own actions, as the same code that a press runs (an action code), or a special one. */
enum { CX_PANIC = 1, CX_PAUSE, CX_SCALE, CX_SHOT, CX_ACT = 0x100 };
typedef struct { int panel; const char *label; int code; } CtxItem;
static const CtxItem CTX[] = {
    {PANEL_MACHINE, "PANIC: ALL NOTES OFF", CX_PANIC}, {PANEL_MACHINE, "PAUSE / RESUME", CX_PAUSE}, {PANEL_MACHINE, "NEXT SCALE", CX_SCALE},
    {PANEL_FILES, "LOAD THE SELECTED FILE", CX_ACT | A_OK}, {PANEL_FILES, "RESCAN THE FOLDER", CX_ACT | A_X},
    {PANEL_CPU, "SWITCH CPU", CX_ACT | A_UP},
    {PANEL_MEMORY, "NEXT REGION", CX_ACT | A_RIGHT}, {PANEL_MEMORY, "BACK TO THE TOP", CX_ACT | A_HOME}, {PANEL_MEMORY, "NEXT PAGE", CX_ACT | A_PGDN},
    {PANEL_MIDI, "PANIC: ALL NOTES OFF", CX_ACT | A_BACK},
    {PANEL_SOUND, "RECORD / STOP", CX_ACT | A_OK}, {PANEL_SOUND, "PREVIEW ON / OFF", CX_ACT | A_START}, {PANEL_SOUND, "SOLO ON / OFF", CX_ACT | A_SELECT}, {PANEL_SOUND, "CLEAR PEAKS", CX_ACT | A_Y},
    {PANEL_MONITOR, "SORT", CX_ACT | A_X}, {PANEL_MONITOR, "FREEZE / RESUME", CX_ACT | A_Y}, {PANEL_MONITOR, "NEXT PAGE", CX_ACT | A_PGDN},
    {PANEL_TOOLS, "RUN THE SELECTED TOOL", CX_ACT | A_OK},
};
#define NCTX ((int)(sizeof CTX / sizeof *CTX))
/* The items for the focus now: the panel's, then Screenshot (every panel has it); an open page or editor offers to close it instead. Returns how many (at most PX_MENU_MAX). */
static int ctx_items(const App *a, const char *label[], int code[]) {
    int n = 0;
    if (a->info_on || a->ed_on) { label[n] = "CLOSE THIS PAGE"; code[n++] = CX_ACT | A_BACK; }
    else {
        for (int i = 0; i < NCTX && n < PX_MENU_MAX - 2; i++) if (CTX[i].panel == a->panel) { label[n] = CTX[i].label; code[n++] = CTX[i].code; }
        if (a->panel == PANEL_SOUND && a->inst) { label[n] = "EDIT THE INSTRUMENT"; code[n++] = CX_ACT | A_BACK; }       /* B does it too, unless the preview is on and B plays a note */
    }
    label[n] = "SAVE A SCREENSHOT"; code[n++] = CX_SHOT;
    return n;
}
static void ctx_run(App *a, int code) {
    if (code == CX_PANIC) panic_all(a);
    else if (code == CX_PAUSE) { a->paused = !a->paused; say(a, 0, a->paused ? "Paused (Start resumes)" : "Running", NULL); }
    else if (code == CX_SCALE) scale_next(a);
    else if (code == CX_SHOT) screenshot(a);
    else if (code & CX_ACT) act(a, code & 0xFF);
}

static void menu_open_main(App *a) { a->menu_kind = MK_MAIN; a->menu_sel = 0; }
static void menu_open_ctx(App *a) { a->menu_kind = MK_CTX; a->menu_sel = 0; }
static void menu_close(App *a) { a->menu_kind = MK_NONE; a->menu_sel = 0; a->km_state = 0; }
/* The theme picker closed without a choice puts back the theme it was opened over. */
static void menu_cancel(App *a) {
    if (a->menu_kind == MK_THEME) px_theme_apply(a->theme_before);
    menu_close(a);
}
/* L1 + R1: opens the main menu, or closes whatever menu is open. */
static void menu_chord(App *a) {
    for (int b = 0; b < 32; b++) if (a->pad_sent[b]) { key_id(a, a->pad_sent[b] - 1, 0); a->pad_sent[b] = 0; }
    if (a->menu_kind) menu_cancel(a); else menu_open_main(a);
    if (a->panel != PANEL_MIDI && a->midi_held) midi_ok(a, 0);
}
/* ---- the Keymap: help, test and remap (every r3i6nos app has it in its main menu) --------------------------------------------------------------------------------------
 * Browsing: up and down pick a row, left and right turn the pages, A remaps the row (press the new button; one that another action has is a conflict, asked before it is swapped;
 * L1 L2 R1 R2 Fn L3 R3 are the system's and refused), X puts the row back, Y the page, Start everything (asked). Changes are kept in keymap.txt at once. The last page tests: every
 * press is shown with what it does on the panel the menu was opened over, and nothing is acted. L1 + R1 leaves. */
static const KmPage *km_cur(const App *a, KmPage pg[KM_MAXP], int *np) {
    *np = km_pages(pg);
    int i = a->km_page < 0 || a->km_page >= *np ? 0 : a->km_page;
    return &pg[i];
}
static void km_set(App *a, int panel, int slot, int btn) {                  /* slot gets btn; whoever had btn gets the old one: always a permutation */
    int old = g_bind[panel][slot], o = slot_of(panel, btn);
    if (o >= 0 && o != slot) g_bind[panel][o] = old;
    g_bind[panel][slot] = btn;
    keymap_save();
}
static void km_open(App *a) {
    KmPage pg[KM_MAXP];
    int np = km_pages(pg);
    a->menu_kind = MK_KEYMAP; a->menu_sel = 0; a->km_sel = 0; a->km_state = 0; a->km_panel = a->panel; a->km_nhist = 0; a->km_note[0] = 0;
    a->km_page = 0;
    for (int i = 0; i < np; i++) if (pg[i].ctx == a->panel && pg[i].mod == 0) a->km_page = i;       /* the page of the panel the menu was opened over */
}
/* A pad button went down while the Keymap waits for the new button. */
static void km_capture(App *a, int b, const KmPage *pg) {
    int slot = a->km_sel;
    if (!is_bindable(b)) { snprintf(a->km_note, sizeof a->km_note, "%s IS THE SYSTEM'S: NOT BINDABLE", btn_name(b)); return; }
    if (b == g_bind[pg->ctx][slot]) { a->km_state = 0; return; }              /* the same button: nothing changes */
    int o = slot_of(pg->ctx, b);
    if (o >= 0 && SLOT_LABEL[pg->ctx][o][0]) { a->km_state = 2; a->km_other = o; a->km_new = b; return; }   /* in use for something: ask */
    km_set(a, pg->ctx, slot, b);                                              /* free, or used for nothing here: just swap */
    a->km_state = 0;
}
/* One action while the Keymap is open and no button is awaited (a press or a stick step). */
static void keymap_press(App *a, int action) {
    KmPage pg[KM_MAXP];
    int np;
    const KmPage *p = km_cur(a, pg, &np);
    int edit = p->ctx >= 0 && p->mod == 0;
    a->km_note[0] = 0;
    if (a->km_state == 2) {
        if (action == A_OK) { km_set(a, p->ctx, a->km_sel, a->km_new); snprintf(a->km_note, sizeof a->km_note, "SWAPPED AND SAVED"); a->km_state = 0; }
        else if (action == A_BACK) a->km_state = 0;
        return;
    }
    if (a->km_state == 3) {
        if (action == A_OK) { keymap_reset_all(); keymap_save(); snprintf(a->km_note, sizeof a->km_note, "EVERY PANEL IS BACK TO ITS DEFAULTS"); a->km_state = 0; }
        else if (action == A_BACK) a->km_state = 0;
        return;
    }
    if (a->km_state == 1) { if (action == A_BACK) a->km_state = 0; return; }
    if (action == A_BACK) { menu_cancel(a); return; }
    if (p->ctx == KM_TEST) return;
    if (action == A_LEFT || action == A_RIGHT) { a->km_page = (a->km_page + (action == A_RIGHT ? 1 : np - 1)) % np; a->km_sel = 0; return; }
    if (action == A_HOME || action == A_END) { a->km_page = action == A_HOME ? 0 : np - 1; a->km_sel = 0; return; }
    if (p->ctx == KM_SYS) return;
    if (action == A_UP || action == A_DOWN || action == A_PGUP || action == A_PGDN) { a->km_sel = pm_list_step(a->km_sel, NSLOT, NSLOT, action == A_PGUP ? A_HOME : action == A_PGDN ? A_END : action); return; }
    if (action == A_OK) {
        if (!edit) snprintf(a->km_note, sizeof a->km_note, "CHANGE A BUTTON ON THE BASE PAGE");
        else a->km_state = 1;
    } else if (action == A_X && edit) {
        km_set(a, p->ctx, a->km_sel, DEF_BTN[a->km_sel]);
        snprintf(a->km_note, sizeof a->km_note, "THAT ROW IS BACK TO ITS DEFAULT");
    } else if (action == A_Y && edit) {
        memcpy(g_bind[p->ctx], DEF_BTN, sizeof DEF_BTN); keymap_save();
        snprintf(a->km_note, sizeof a->km_note, "THIS PAGE IS BACK TO ITS DEFAULTS");
    } else if (action == A_START) a->km_state = 3;
}
/* The test page: a press is noted, with what it does on the panel the Keymap was opened over, and nothing else happens. */
static void km_test_note(App *a, const char *what, const char *does) {
    if (a->km_nhist == 5) { memmove(a->km_hist[0], a->km_hist[1], 4 * sizeof a->km_hist[0]); a->km_nhist = 4; }
    snprintf(a->km_hist[a->km_nhist++], sizeof a->km_hist[0], "%s  %s", what, does);
}
static void km_test_pad(App *a, int b) {
    static const char *const sys[17] = {"", "", "", "", "A MODIFIER (HOLD)", "INCREASE (HOLD)", "A MODIFIER, L2 + R2", "A MODIFIER, L2 + R2", "", "", "", "", "", "", "NO VIRTUAL MOUSE", "NO VIRTUAL KEYBOARD", "NEXT PANEL"};
    int sl = slot_of(a->km_panel, b);
    const char *d = b >= 0 && b < 17 && sys[b][0] ? sys[b] : sl >= 0 && SLOT_LABEL[a->km_panel][sl][0] ? SLOT_LABEL[a->km_panel][sl] : "NOTHING HERE";
    km_test_note(a, btn_name(b), d);
}
/* One action while a menu is open (a press, or a step of the right stick). A chooses, B closes, the rest moves the selection. */
static void menu_press(App *a, int action) {
    const char *cl[PX_MENU_MAX];
    int cc[PX_MENU_MAX], n = MM_COUNT;
    if (a->menu_kind == MK_CTX) n = ctx_items(a, cl, cc);
    else if (a->menu_kind == MK_THEME) n = PX_THEME_COUNT;
    else if (a->menu_kind == MK_ABOUT) { menu_close(a); return; }          /* any key closes About */
    else if (a->menu_kind == MK_KEYMAP) { keymap_press(a, action); return; }
    if (action == A_BACK) { menu_cancel(a); return; }
    if (action == A_OK) {
        int sel = a->menu_sel < n ? a->menu_sel : 0, kind = a->menu_kind;
        if (kind == MK_THEME) { theme_save(); say(a, 0, "Colour theme %s", px_themes[px_theme_now].name); menu_close(a); return; }
        menu_close(a);
        if (kind == MK_CTX) { ctx_run(a, cc[sel]); return; }
        switch (sel) {
        case MM_ABOUT: a->menu_kind = MK_ABOUT; break;
        case MM_THEME: a->menu_kind = MK_THEME; a->menu_sel = px_theme_now; a->theme_before = px_theme_now; break;
        case MM_KEYMAP: km_open(a); break;
        case MM_REPORT: job_start(a, JOB_REPORT); break;
        case MM_DEMO: a->want_demo = 1; break;
        case MM_BENCH: job_start(a, JOB_BENCH); break;
        case MM_QUIT: request_quit(a); break;
        case MM_TOOLS: panel_switch(a, PANEL_TOOLS); break;
        default: break;
        }
        return;
    }
    a->menu_sel = pm_list_step(a->menu_sel, n, PX_MENU_MAX, action);
    if (a->menu_kind == MK_THEME) px_theme_apply(a->menu_sel);               /* the picker shows each theme as the selection moves */
}
static void fill_menu(const App *a, UiState *u) {
    u->menu_kind = a->menu_kind == MK_ABOUT ? 2 : a->menu_kind ? 1 : 0;      /* how it is drawn: 1 a list with a chosen row, 2 lines of text */
    if (!a->menu_kind) return;
    u->menu_sel = a->menu_sel;
    const char *cl[PX_MENU_MAX];
    int cc[PX_MENU_MAX];
    if (a->menu_kind == MK_MAIN) {
        snprintf(u->menu_title, sizeof u->menu_title, "MENU");
        snprintf(u->menu_foot, sizeof u->menu_foot, "A CHOOSE  B CLOSE");
        u->menu_n = MM_COUNT;
        for (int i = 0; i < MM_COUNT; i++) snprintf(u->menu_label[i], PX_NAME, "%s", MM_LABEL[i]);
        snprintf(u->menu_value[MM_THEME], PX_NAME, "%s", px_themes[px_theme_now].name);
    } else if (a->menu_kind == MK_CTX) {
        snprintf(u->menu_title, sizeof u->menu_title, "%s", PX_PANEL_NAMES[a->panel]);
        snprintf(u->menu_foot, sizeof u->menu_foot, "A CHOOSE  B CLOSE");
        u->menu_n = ctx_items(a, cl, cc);
        for (int i = 0; i < u->menu_n; i++) snprintf(u->menu_label[i], PX_NAME, "%s", cl[i]);
    } else if (a->menu_kind == MK_THEME) {
        snprintf(u->menu_title, sizeof u->menu_title, "COLOUR THEME");
        snprintf(u->menu_foot, sizeof u->menu_foot, "A KEEP  B BACK");
        u->menu_n = PX_THEME_COUNT;
        for (int i = 0; i < PX_THEME_COUNT; i++) { snprintf(u->menu_label[i], PX_NAME, "%s", px_themes[i].name); snprintf(u->menu_value[i], PX_NAME, "%s", i == a->theme_before ? "*" : ""); }
    } else if (a->menu_kind == MK_KEYMAP) {
        KmPage pg[KM_MAXP];
        int np;
        const KmPage *p = km_cur(a, pg, &np);
        int pi = (int)(p - pg) + 1;
        const char *name = p->ctx == KM_SYS ? "SYSTEM" : p->ctx == KM_TEST ? "TEST" : PX_PANEL_NAMES[p->ctx];
        snprintf(u->menu_title, sizeof u->menu_title, "KEYMAP < %d/%d > %s%s", pi, np, name, p->ctx >= 0 && p->mod ? MOD_NAME[p->mod - 1] : p->ctx == KM_SYS && p->part ? " 2" : "");
        if (p->ctx == KM_SYS) {
            int n = 0;
            for (int i = p->part * KM_SYS_PER; i < KM_SYS_N && n < KM_SYS_PER; i++, n++) { snprintf(u->menu_label[n], PX_NAME, "%s", KM_SYS_ROWS[i][0]); snprintf(u->menu_value[n], PX_NAME, "%s", KM_SYS_ROWS[i][1]); }
            u->menu_n = n; u->menu_sel = -1;
            snprintf(u->menu_foot, sizeof u->menu_foot, "< > PAGES  B CLOSE");
        } else if (p->ctx == KM_TEST) {
            u->menu_kind = 2;
            snprintf(u->menu_label[0], PX_NAME, "PRESS ANY BUTTON: NOTHING IS DONE");
            for (int i = 0; i < 5; i++) snprintf(u->menu_label[1 + i], PX_NAME, "%s", i < a->km_nhist ? a->km_hist[i] : "");
            char held[PX_NAME] = "HELD:";
            for (int b = 0; b < 17; b++) if (a->pad_mask >> b & 1) { size_t l = strlen(held); snprintf(held + l, sizeof held - l, " %s", btn_name(b)); }
            snprintf(u->menu_label[6], PX_NAME, "%s", held);
            snprintf(u->menu_label[7], PX_NAME, "STICKS L %.1f %.1f  R %.1f %.1f", a->axis[0], a->axis[1], a->axis[2], a->axis[3]);
            u->menu_n = 8;
            snprintf(u->menu_foot, sizeof u->menu_foot, "L1 + R1 LEAVES");
        } else {
            u->menu_n = NSLOT;
            for (int i = 0; i < NSLOT; i++) {
                const char *lab = p->mod ? MOD_LABEL[p->mod - 1][i] : SLOT_LABEL[p->ctx][i];
                snprintf(u->menu_label[i], PX_NAME, "%s", lab[0] ? lab : "-");
                snprintf(u->menu_value[i], PX_NAME, "%s%s", btn_name(g_bind[p->ctx][i]), g_bind[p->ctx][i] != DEF_BTN[i] ? " *" : "");
            }
            u->menu_sel = a->km_sel;
            if (a->km_note[0]) snprintf(u->menu_foot, sizeof u->menu_foot, "%s", a->km_note);
            else if (a->km_state == 1) snprintf(u->menu_foot, sizeof u->menu_foot, "PRESS THE NEW BUTTON (SAME: CANCEL)");
            else if (a->km_state == 2) snprintf(u->menu_foot, sizeof u->menu_foot, "%s USES %s. A SWAPS  B NO", SLOT_LABEL[p->ctx][a->km_other], btn_name(a->km_new));
            else if (a->km_state == 3) snprintf(u->menu_foot, sizeof u->menu_foot, "ALL BACK TO DEFAULTS? A YES  B NO");
            else if (p->mod) snprintf(u->menu_foot, sizeof u->menu_foot, "SET BUTTONS ON THE BASE PAGE  B CLOSE");
            else snprintf(u->menu_foot, sizeof u->menu_foot, "A REMAP  X/Y/START RESET ROW/PAGE/ALL");
        }
    } else {
        char v[64];
        snprintf(u->menu_title, sizeof u->menu_title, "ABOUT");
        snprintf(u->menu_foot, sizeof u->menu_foot, "ANY KEY CLOSES");
        snprintf(v, sizeof v, "V%s %s", fm1ntt_version(), FM1NTT_BUILD);
        const char *lines[ABOUT_LINES] = {"FM1-NTT", v, "FM-1 FIRMWARE SANDBOX AND EMULATOR", "MIT LICENCE", "BASED ON THE MIT FM1-EMU", "EACH FIRMWARE KEEPS ITS OWN LICENCE"};
        u->menu_n = ABOUT_LINES;
        for (int i = 0; i < ABOUT_LINES; i++) snprintf(u->menu_label[i], PX_NAME, "%s", lines[i]);
    }
}

/* ---- the UI state, from the app and the core --------------------------------------------------------------------------- */
static void fill_ui(App *a, UiState *u, int use_lock) {
    memset(u, 0, sizeof *u);
    u->midi_in_devs = a->min_on ? a->min.devs : -1; u->midi_in_got = a->min_routed;
    u->panel = a->panel;
    fill_menu(a, u);
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
    u->pv_factory = a->inst ? 2 : a->pv_factory; u->pv_solo = a->pv_solo; u->pv_on = a->pv_on; u->pv_n = a->pv_n; u->pv_sel = a->pv_sel; u->pv_oct = a->pv_oct;
    pv_name(a, u->pv_name, sizeof u->pv_name);
    u->ed_on = a->ed_on && a->inst != NULL;
    if (u->ed_on) {                                               /* (the instrument is read under the audio lock: the callback renders it) one whole page of the group, the chosen row marked (nothing scrolls) */
        if (use_lock) lock(a);
        int g = a->ed_group, total = ed_count(a, g), page = total ? a->ed_sel / ED_ROWS : 0;
        u->ed_total = total; u->ed_group = g; u->ed_groups = ed_groups(a); u->ed_page = page; u->ed_pages = total ? (total + ED_ROWS - 1) / ED_ROWS : 1;
        u->ed_sel = a->ed_sel - page * ED_ROWS; u->ed_dirty = a->ed_dirty;
        ed_title(a, g, u->ed_title, sizeof u->ed_title);
        fm1ntt_inst_text(a->inst, 0, u->ed_inst, sizeof u->ed_inst);
        u->ed_n = 0;
        for (int i = page * ED_ROWS; i < total && u->ed_n < ED_ROWS; i++) {
            int32_t info[4];
            if (ed_info(a, g, i, info, u->ed_row[u->ed_n].name, PX_NAME) != 0) break;
            u->ed_row[u->ed_n].min = info[1]; u->ed_row[u->ed_n].max = info[2];
            u->ed_row[u->ed_n].val = ed_get(a, g, info[0]);
            u->ed_n++;
        }
        if (use_lock) unlock(a);
    }
    u->msg = a->msg;
    u->msg_style = a->msg_style;
    /* files */
    char dir[300];
    sub_path(dir, sizeof dir, "firmware", NULL);
    snprintf(u->file_dir, sizeof u->file_dir, "%s", dir);
    snprintf(u->bundled_dir, sizeof u->bundled_dir, "%s", a->bdir);
    snprintf(u->pack_dir, sizeof u->pack_dir, "%s", a->pdir);
    for (int i = 0; i < a->nfiles; i++) snprintf(u->files[i], PX_NAME, "%c%c %s", !strcmp(a->fpaths[i], a->fw_path) && a->fw_path[0] ? '*' : ' ', a->fbundled[i] == 2 ? 'G' : a->fbundled[i] ? 'B' : ' ', a->files[i]);
    u->nfiles = a->nfiles;
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
    /* MONITOR: the host's numbers as the sampler last took them (nothing is read here), and what the app itself knows */
    u->mon_page = a->mon_page;
    u->mon_paused = a->mon.paused;
    if (a->panel == PANEL_MONITOR) {
        u->mon = a->mon.v;
        MonApp *p = &u->mon.app;
        int loaded_now = fm1ntt_loaded(a->m);
        p->speed_pct = loaded_now && !a->paused && a->speed > 0 ? (int)(a->speed * 100.0 + 0.5) : MON_NA;
        p->jit = u->jit;
        p->loaded = loaded_now; p->jit_pref = a->jit_pref; p->jit_avail = fm1ntt_jit_available();
        p->cb_per_s = a->cb_rate;
        p->late_cb = SDL_AtomicGet(&a->late_cb);
        p->frame_ms = (int)a->frame_ms; p->frame_max_ms = (int)a->frame_max_ms;
        if (a->job) { snprintf(p->job, sizeof p->job, "%s", a->job_kind == JOB_REPORT ? "REPORT" : a->job_kind == JOB_LOAD ? "LOAD" : "BENCHMARK"); p->job_pct = job_pct(a); } else p->job_pct = MON_NA;
        snprintf(p->fw, sizeof p->fw, "%s", a->file);
        p->njoy = a->njoy;
        { char rr[300], rw[64]; root_describe(rr, sizeof rr, rw, sizeof rw); snprintf(p->root, sizeof p->root, "%s", rr); snprintf(p->root_why, sizeof p->root_why, "%s", rw); }
        memcpy(p->pad, a->padn, sizeof p->pad);
        p->jit_blocks = p->jit_given_up = MON_NA;
        if (loaded_now && u->jit) { char js[400]; if (use_lock) lock(a); size_t jn = fm1ntt_jit_report(a->m, js, sizeof js); if (use_lock) unlock(a); if (!jn || !mon_parse_jit(js, &p->jit_blocks, &p->jit_given_up)) p->jit_blocks = p->jit_given_up = MON_NA; }
        if (u->midi_queued) snprintf(p->midi, sizeof p->midi, "SENT %llu DROP %llu", (unsigned long long)u->midi_queued, (unsigned long long)u->midi_dropped);
        for (int k = 0; k < 3; k++) snprintf(p->hot[k], sizeof p->hot[k], "%s", a->toast.hist[k]);
        if (loaded_now) { size_t kb = 0; for (int rgn = 0; rgn < 3; rgn++) { uint32_t b32; size_t sz = 0; fm1ntt_region(a->m, rgn, &b32, &sz); kb += sz / 1024; } p->emu_kb = (int)kb; }
        u->rain_on = a->rain_on;
        u->rain = a->rain;
        u->rain_still = a->reduced_motion;
        u->rain_full = a->rain_on && SDL_GetTicks() - a->last_input_ms >= RAIN_IDLE_MS && !a->job;
    }
    root_describe(u->app_root, sizeof u->app_root, u->app_root_why, sizeof u->app_root_why);
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
    snprintf(u->tools[14], PX_NAME, "  REMEMBER SETTINGS: %s", a->remember ? "ON" : "OFF");
    if (a->pace_mode == PACE_AUTO) snprintf(u->tools[11], PX_NAME, "  PACE: AUTO (NOW %d)", a->pace_lvl);
    else if (a->pace_mode) snprintf(u->tools[11], PX_NAME, "  PACE: %d", a->pace_mode);
    else snprintf(u->tools[11], PX_NAME, "  PACE: OFF");
    snprintf(u->tools[12], PX_NAME, "  DATA RAIN: %s", a->rain_on ? "ON" : "OFF");
    snprintf(u->tools[13], PX_NAME, "  BACKGROUND DEMOS: %s", a->bg_demos ? (a->sw_pid ? "ON (MAKING ONE)" : "ON") : "OFF");
    u->ntools = NTOOLS;
    u->tool_sel = a->tool_sel;
    if (a->confirm == CF_RESET) snprintf(u->confirm, sizeof u->confirm, "Reset the machine? The emulated flash goes back to what the firmware package carried, and the saved settings are forgotten.");
    else if (a->confirm == CF_RESTORE) snprintf(u->confirm, sizeof u->confirm, "Restore slot %d into the flash? The machine reboots.", a->slot);
    else if (a->confirm == CF_QUIT) snprintf(u->confirm, sizeof u->confirm, "A recording is running. Stop it, save it and quit?");
    ui_showcase(a, u);
}

/* AUTO pacing: one level up when the unit falls behind real time, one down when it has a lot to spare (exact timing is kept when it can) */
static void pace_auto(App *a) {
    if (a->pace_mode != PACE_AUTO || a->paused || a->speed <= 0 || !fm1ntt_loaded(a->m)) return;
    int l = a->pace_lvl;
    if (a->speed < 0.97 && l < 3) l++;
    else if (a->speed > 3.0 && l > 0) l--;
    if (l != a->pace_lvl) { a->pace_lvl = l; lock(a); fm1ntt_pace(a->m, l); unlock(a); }
}

/* A firmware that runs under half of real time cannot fill an audio buffer in the buffer's own time: the callback is late every time and the sound device, left dry,
 * rattles (the unit, 2026-11-19: the MONITOR's LATE BUFFERS climbing at the buffer rate, idle). Its sound is muted until it is back above 0.7x (a hysteresis, so it does not
 * flap at the edge); the SOUND preview and Solo, which the callback does not wait for, are never muted. The machine still runs, so the speed stays measured. */
#define SLOW_MUTE_BELOW 0.5
#define SLOW_MUTE_ABOVE 0.7
static void slow_mute_update(App *a) {
    int was = a->fw_mute, now = was;
    if (a->paused || a->speed <= 0 || !fm1ntt_loaded(a->m)) now = 0;
    else if (a->speed < SLOW_MUTE_BELOW) now = 1;
    else if (a->speed >= SLOW_MUTE_ABOVE) now = 0;
    a->fw_mute = now;
    if (now && !was) { char m[96]; snprintf(m, sizeof m, "Firmware sound muted: too slow to keep up (%.2fx real time). Solo still plays.", a->speed); say(a, 2, "%s", m); }
    else if (!now && was) say(a, 0, "Firmware sound back: keeping up again", NULL);
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
    slow_mute_update(a);
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
/* The R36S pad's raw buttons: B 0, A 1, X 2, Y 3, L1 4, R1 5, L2 6, R2 7, up 8, down 9, left 10, right 11, Select 12, Start 13, L3 14, R3 15, Fn 16;
 * axes 0 1 the left stick (x, y), 2 3 the right (y is down in SDL). What the ten bindable buttons do is each panel's binding (g_bind, the Keymap). */
/* On MACHINE the pad plays. Without a modifier: the d-pad and face buttons are eight notes of the chosen scale (up C, left D, down E,
 * right F, Y G, B A, A B, X the next C), Select OCT-, Start OCT+, the sticks the four knobs. L1 held: the sticks are master, presets,
 * select and algorithm; the d-pad, faces, Select and Start the ten black-key functions (OP1-OP6, PIT, GLO, MONO, POLY); Fn changes the
 * scale. L2 held: SCL FX HOME SAVE ENV LFO ARP SEQ; Select and Start transpose (my addition: L1's Select and Start are MONO and POLY). R2 held: GLO EDIT PLAY REC. */
enum { MOD_NONE, MOD_L1, MOD_L2, MOD_R2 };
static const struct { const char *name; int n; int semi[8]; } SCALES[] = {
    {"MAJOR", 7, {0, 2, 4, 5, 7, 9, 11}}, {"MINOR", 7, {0, 2, 3, 5, 7, 8, 10}}, {"DORIAN", 7, {0, 2, 3, 5, 7, 9, 10}},
    {"MIXOLYDIAN", 7, {0, 2, 4, 5, 7, 9, 10}}, {"MAJOR PENTATONIC", 5, {0, 2, 4, 7, 9}}, {"MINOR PENTATONIC", 5, {0, 3, 5, 7, 10}},
    {"CHROMATIC", 8, {0, 1, 2, 3, 4, 5, 6, 7}},
};
#define NSCALES ((int)(sizeof SCALES / sizeof SCALES[0]))
static void scale_next(App *a) { a->scale = (a->scale + 1) % NSCALES; say(a, 0, "Scale %s", SCALES[a->scale].name); }
#define KEY_MIN (-6)
#define KEY_MAX 5
static const char *const KEY_NAMES[12] = {"C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"};
static const char *key_name(int shift) { return KEY_NAMES[((shift % 12) + 12) % 12]; }
/* The pad's button as a position 0..7 in the scale (by default up left down right Y B A X; the Keymap can swap them), or -1. */
static int degree_of_pad(int b) { int sl = slot_of(PANEL_MACHINE, b); return sl >= 0 && sl < 8 ? sl : -1; }
/* The matrix id of a scale position: C4 is note key 7 (F3 is key 0) plus the key's shift; -1 when it falls off the 27 keys. */
static int note_id_of_degree(int scale, int shift, int deg) {
    int n = SCALES[scale].n, semi = SCALES[scale].semi[deg % n] + 12 * (deg / n), k = 7 + shift + semi;
    return k >= 0 && k < 27 ? FM1_KEY_BASE + k : -1;
}
static int pad_mod(unsigned mask) {
    return mask >> PAD_L1 & 1 ? MOD_L1 : mask >> PAD_L2 & 1 ? MOD_L2 : mask >> PAD_R2 & 1 ? MOD_R2 : MOD_NONE;
}
/* Under a modifier, the FM-1 button a pad button presses (matrix id 0..13), -1 none. By slot, so the Keymap's swaps carry through. */
static int button_of_pad_mod(int mod, int b) {
    static const int l2[NSLOT] = {3, 2, 8, 9, 4, 5, 10, 11, -1, -1}, r2[NSLOT] = {7, 6, 12, 13, -1, -1, -1, -1, -1, -1};
    int sl = slot_of(PANEL_MACHINE, b);
    if (sl < 0) return -1;
    return mod == MOD_L2 ? l2[sl] : mod == MOD_R2 ? r2[sl] : -1;
}
/* The manual puts ten functions on the piano's black keys: OP1 to OP6, PIT, GLO, MONO, POLY. They are note keys (F#3, G#3, A#3, C#4, D#4,
 * F#4, G#4, A#4, C#5, D#5: note key indexes 1 3 5 8 10 13 15 17 20 22), in that order; the ORDER is assumed from the manual's list, not
 * measured. The pad under L1 presses them in the d-pad and face order: up left down right Y B A X, then Select and Start. */
static const int FUNCTION_KEYS[10] = {1, 3, 5, 8, 10, 13, 15, 17, 20, 22};
static const char *const FUNCTION_NAMES[10] = {"OP1", "OP2", "OP3", "OP4", "OP5", "OP6", "PIT", "GLO", "MONO", "POLY"};
static int function_of_pad(int b) { return slot_of(PANEL_MACHINE, b); }
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
    if (a->panel != PANEL_MACHINE || a->confirm || a->menu_kind) {
        for (int i = 0; i < 4; i++) a->axis_acc[i] = 0;
        int steps[8], n = pm_stick_tick(&a->rstick, a->axis[2], a->axis[3], a->pad_mask >> PAD_R1 & 1, now, steps, 8);   /* the right stick moves the selection, 6 a second at most */
        for (int i = 0; i < n; i++) { if (a->menu_kind) menu_press(a, steps[i]); else act(a, steps[i]); }
        return;
    }
    memset(&a->rstick, 0, sizeof a->rstick);
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
    int mod = pad_mod(a->pad_mask), id = -1, sl = slot_of(PANEL_MACHINE, b), r1 = a->pad_mask >> PAD_R1 & 1;
    if (sl < 0) return;
    if (mod == MOD_L2 && (sl == SL_SELECT || sl == SL_START)) {
        a->key_shift += sl == SL_START ? 1 : -1;
        if (a->key_shift > KEY_MAX) a->key_shift = KEY_MIN;
        if (a->key_shift < KEY_MIN) a->key_shift = KEY_MAX;
        say(a, 0, "Key %s", key_name(a->key_shift));
        return;
    }
    if (mod == MOD_NONE && r1 && sl <= SL_RIGHT) {              /* R1 held, "increase": up and down transpose the key, left and right the octave (the machine's OCT buttons) */
        if (sl == SL_UP || sl == SL_DOWN) {
            a->key_shift += sl == SL_UP ? 1 : -1;
            if (a->key_shift > KEY_MAX) a->key_shift = KEY_MIN;
            if (a->key_shift < KEY_MIN) a->key_shift = KEY_MAX;
            say(a, 0, "Key %s", key_name(a->key_shift));
        } else id = sl == SL_RIGHT ? 1 : 0;
    } else if (mod == MOD_NONE) {
        int d = degree_of_pad(b);
        if (d >= 0) { id = note_id_of_degree(a->scale, a->key_shift, d); if (id < 0) say(a, 0, "That note is off the FM-1's keys", NULL); }
        else if (sl == SL_SELECT) id = 0;
        else if (sl == SL_START) id = 1;
    } else if (mod == MOD_L1) {
        int f = function_of_pad(b);
        if (f >= 0) { id = FM1_KEY_BASE + FUNCTION_KEYS[f]; snprintf(a->msg, sizeof a->msg, "%s", FUNCTION_NAMES[f]); a->msg_style = 0; }
    } else {
        id = button_of_pad_mod(mod, b);
    }
    if (id >= 0 && b < 32) { key_id(a, id, 1); a->pad_sent[b] = id + 1; }
}
static int act_of_key(SDL_Keycode k) {
    switch (k) {
    case SDLK_UP: return A_UP; case SDLK_DOWN: return A_DOWN; case SDLK_LEFT: return A_LEFT; case SDLK_RIGHT: return A_RIGHT;
    case SDLK_RETURN: case SDLK_KP_ENTER: return A_OK; case SDLK_BACKSPACE: return A_BACK;
    case SDLK_INSERT: return A_X; case SDLK_DELETE: return A_Y; case SDLK_PAGEUP: return A_PGUP; case SDLK_PAGEDOWN: return A_PGDN;
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
        if (n >= 0 && a->panel == PANEL_SOUND && a->pv_on) { pv_note(a, &a->pv_key[n], pv_midi(a, n), down); return; }   /* the preview's keys, not the machine's */
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
        } else {
            int ac = act_of_key(k);
            if (a->panel == PANEL_SOUND) ac = ac == A_PGUP ? A_START : ac == A_PGDN ? A_SELECT : ac;       /* Page Up the preview, Page Down Solo (and, in the editor, the group) */
            else if (a->panel == PANEL_MIDI) ac = ac == A_PGUP ? A_SELECT : ac == A_PGDN ? A_START : ac;   /* the channel */
            act(a, ac);
        }
    } else if (e->type == SDL_JOYAXISMOTION) {
        if (e->jaxis.axis < 4) a->axis[e->jaxis.axis] = e->jaxis.value / 32768.0f;
    } else if (e->type == SDL_JOYBUTTONDOWN || e->type == SDL_JOYBUTTONUP) {
        int down = e->type == SDL_JOYBUTTONDOWN, b = e->jbutton.button;
        if (b < 32 && !down && a->pad_sent[b]) { key_id(a, a->pad_sent[b] - 1, 0); a->pad_sent[b] = 0; return; }   /* the release matches the press */
        if (b == PAD_L3 || b == PAD_R3) { if (down) say(a, 0, b == PAD_L3 ? "FM1-NTT has no virtual mouse" : "FM1-NTT has no virtual keyboard", NULL); return; }   /* a reply, not a silent no-op (the mapping) */
        if (machine_panel) { if (down) pad_machine_press(a, b); return; }
        int ctx = a->confirm ? -1 : a->panel, sl = slot_of(ctx, b), base = sl >= 0 ? SLOT_ACT[sl] : A_NONE;   /* a question is answered with A and B wherever they were moved to */
        if (a->panel == PANEL_MIDI && !a->confirm && base == A_OK) { midi_ok(a, down); return; }
        if (a->panel == PANEL_SOUND && a->pv_on && !a->ed_on && !a->confirm && (base == A_Y || base == A_BACK || base == A_OK || base == A_X)) {   /* the pad plays a chord's notes: Y the root, B the third, A the fifth, X the octave */
            int k = base == A_Y ? 0 : base == A_BACK ? 1 : base == A_OK ? 2 : 3;
            static const int semi[4] = {0, 4, 7, 12};
            pv_note(a, &a->pv_pad[k], 60 + 12 * a->pv_oct + semi[k], down);
            return;
        }
        if (down) act(a, pad_action(ctx, b, a->pad_mask >> PAD_R1 & 1));
    }
}

/* One event as the whole app takes it: quit, Fn, the carousel, then the panel's own controls. Rec demo's tour calls this too, so it
 * records the real input path. */
/* The keyboard's way to the menus (the pad has L1 + R1 and L2 + R2): ` the main menu, \ the context menu. r3i6nos proposes F10 for the main menu, but F10 is the FM-1's
 * button 10 here (button_of_key), so it cannot be. */
static int is_menu_key(const SDL_Event *e) { return e->type == SDL_KEYDOWN && !e->key.repeat && e->key.keysym.sym == SDLK_BACKQUOTE; }
static int is_ctx_key(const SDL_Event *e) { return e->type == SDL_KEYDOWN && !e->key.repeat && e->key.keysym.sym == SDLK_BACKSLASH; }
/* While a menu is open it takes every press: the d-pad (with R1 held, pages and Home and End), A, B, a keyboard's arrows, Enter and Esc. The rest is let go. */
static void menu_input(App *a, const SDL_Event *e) {
    if (a->menu_kind == MK_KEYMAP) {                                        /* the Keymap wants the raw button: to bind it, or to show it */
        KmPage pg[KM_MAXP];
        int np;
        const KmPage *p = km_cur(a, pg, &np);
        if (e->type == SDL_JOYBUTTONDOWN) {
            if (p->ctx == KM_TEST) { km_test_pad(a, e->jbutton.button); return; }
            if (a->km_state == 1) { a->km_note[0] = 0; km_capture(a, e->jbutton.button, p); return; }
        } else if (e->type == SDL_KEYDOWN && p->ctx == KM_TEST && e->key.keysym.sym != SDLK_ESCAPE && !e->key.repeat) { km_test_note(a, SDL_GetKeyName(e->key.keysym.sym), "KEY"); return; }
    }
    if (e->type == SDL_KEYDOWN) {
        if (e->key.repeat && e->key.keysym.sym != SDLK_UP && e->key.keysym.sym != SDLK_DOWN) return;
        SDL_Keycode k = e->key.keysym.sym;
        menu_press(a, k == SDLK_ESCAPE ? A_BACK : k == SDLK_HOME ? A_HOME : k == SDLK_END ? A_END : k == SDLK_PAGEUP ? A_PGUP : k == SDLK_PAGEDOWN ? A_PGDN : act_of_key(k));
    } else if (e->type == SDL_JOYBUTTONDOWN) menu_press(a, act_of_pad_r1(e->jbutton.button, a->pad_mask >> PAD_R1 & 1));
}
static void dispatch(App *a, const SDL_Event *e) {
    if (a->pl.on && (e->type == SDL_KEYDOWN || e->type == SDL_JOYBUTTONDOWN) && !(e->type == SDL_KEYDOWN && e->key.repeat)) { player_stop(a); return; }   /* a press ends the demo and is used up */
    if (e->type == SDL_KEYDOWN || e->type == SDL_JOYBUTTONDOWN || e->type == SDL_JOYAXISMOTION || e->type == SDL_JOYHATMOTION) a->last_input_ms = SDL_GetTicks();   /* the data rain's whole-panel mode ends on any of them */
    if ((e->type == SDL_KEYDOWN || e->type == SDL_JOYBUTTONDOWN) && toast_key(&a->toast, SDL_GetTicks())) return;   /* a failure's key is used up */
    int turn = is_carousel(e), pad_down = e->type == SDL_JOYBUTTONDOWN, pb = pad_down || e->type == SDL_JOYBUTTONUP ? e->jbutton.button : -1;
    if (pad_down && pb < 32) a->pad_mask |= 1u << pb;
    if (e->type == SDL_JOYBUTTONUP && pb < 32) a->pad_mask &= ~(1u << pb);
    int l1 = a->pad_mask >> PAD_L1 & 1, r1 = a->pad_mask >> PAD_R1 & 1, l2 = a->pad_mask >> PAD_L2 & 1, r2 = a->pad_mask >> PAD_R2 & 1;
    int chord = pad_down && (pb == PAD_L1 || pb == PAD_R1) && l1 && r1;                                   /* L1 + R1: the main menu */
    int ctx_chord = pad_down && (pb == PAD_L2 || pb == PAD_R2) && l2 && r2 && !l1 && !r1;                 /* L2 + R2: the context menu (L1 or R1 too is reserved: nothing) */
    int fn_down = pad_down && pb == PAD_FN && !a->confirm && !a->menu_kind;
    int fn_first = fn_down && l1, fn_prev = fn_down && r1 && !l1;
    if (e->type == SDL_QUIT) { rec_stop(a); a->quit = 1; }
    else if (e->type == SDL_KEYDOWN && e->key.keysym.sym == SDLK_ESCAPE && !e->key.repeat) { if (a->menu_kind) menu_cancel(a); else if (a->confirm) a->confirm = CF_NONE; else if (a->info_on) a->info_on = 0; else if (a->ed_on) a->ed_on = 0; else say(a, 0, "Quit is in the main menu (L1 + R1, or `)", NULL); }   /* leaving is deliberate: only the menu's Quit does it */
    else if (is_toggle(e)) a->mode ^= 1;
    else if (chord || (is_menu_key(e) && !a->confirm)) { if (!a->confirm) menu_chord(a); }
    else if (ctx_chord || (is_ctx_key(e) && !a->confirm)) {
        if (a->confirm) { /* a question is open: A or B first */ }
        else if (a->menu_kind == MK_CTX) menu_cancel(a);
        else if (!a->menu_kind) { for (int b = 0; b < 32; b++) if (a->pad_sent[b]) { key_id(a, a->pad_sent[b] - 1, 0); a->pad_sent[b] = 0; } menu_open_ctx(a); }
    }
    else if (a->menu_kind && !a->confirm) menu_input(a, e);                                               /* a menu takes every press */
    else if (fn_first) panel_switch(a, 0);                                                                /* L1 + Fn: the first panel */
    else if (fn_prev) panel_back(a);                                                                      /* R1 + Fn: the one before */
    else if (turn && !a->confirm) panel_switch(a, panel_step(a->panel, turn));
    else if (turn) { /* a question is open: A or B first */ }
    else handle(a, e);
}

/* ---- the splash: shown at once, before anything heavy ------------------------------------------------------------------ */
/* `elapsed_ms` >= 0 adds the elapsed time under the bar and `err` (when set) shows a load error in red: the real start-up shows both (rule 8), while Rec demo's
 * first frame passes neither, so it stays the same bytes every run. */
static void draw_splash_x(uint32_t *c, const char *what, int percent, int elapsed_ms, const char *err) {
    for (int i = 0; i < PX_W * PX_H; i++) c[i] = C_BG;
    char line[160];
    int cx = PX_W / 2;
    px_text(c, cx - px_text_w("FM1-NTT", 4) / 2, 150, "FM1-NTT", C_HEAD, 4, 0);
    px_text(c, cx - px_text_w("FM-1 FIRMWARE SANDBOX", 1) / 2, 196, "FM-1 FIRMWARE SANDBOX", C_TEXT, 1, 0);
    px_text(c, cx - px_text_w(what, 1) / 2, 250, what, C_TEXT, 1, 0);
    px_meter(c, (PxRect){cx - 150, 266, 300, 10}, percent, C_HEAD);
    if (elapsed_ms >= 0) {
        snprintf(line, sizeof line, "%d%%  %d.%d S", percent, elapsed_ms / 1000, (elapsed_ms % 1000) / 100);
        px_text(c, cx - px_text_w(line, 1) / 2, 282, line, C_DIM, 1, 0);
    }
    snprintf(line, sizeof line, "V%s %s", fm1ntt_version(), FM1NTT_BUILD);
    px_text(c, cx - px_text_w(line, 1) / 2, 296, line, C_DIM, 1, 0);
    if (err && *err) {
        char e[96];
        snprintf(e, sizeof e, "%.90s", err);
        for (char *q = e; *q; q++) if (*q >= 'a' && *q <= 'z') *q = (char)(*q - 32);
        px_text(c, cx - px_text_w("COULD NOT LOAD THE FIRMWARE", 1) / 2, 322, "COULD NOT LOAD THE FIRMWARE", C_ERR, 1, 0);
        px_text(c, cx - px_text_w(e, 1) / 2, 336, e, C_ERR, 1, 0);
    }
}
static void draw_splash(uint32_t *c, const char *what, int percent) { draw_splash_x(c, what, percent, -1, NULL); }

static void present(SDL_Renderer *r, SDL_Texture *t, const uint32_t *c) {
    SDL_UpdateTexture(t, NULL, c, PX_W * 4);
    SDL_RenderClear(r);
    SDL_RenderCopy(r, t, NULL, NULL);
    SDL_RenderPresent(r);
}

#include "report.h"
#include "demo.h"
#include "showcase.h"

/* ---- the showcase in the app: the worker, the queue, the firmware's page, the player (ROADMAP 5d, host/showcase.h) ----------------------------------------------------
 * The worker is a separate process (`fm1ntt --showcase FILE`), not a thread: it has its own machine and its own FM1NTT_DIR-independent state, it can be stopped
 * (SIGSTOP while you play, SIGCONT after), cancelled (SIGTERM: it removes what it wrote) and cannot touch the app's memory. It runs at the lowest priority (nice 19,
 * SCHED_IDLE on Linux). Nothing starts unless TOOLS > BACKGROUND DEMOS is on, or Y is pressed on a firmware's page. Untested on an R36S. */
static int fw_is_firmware(const char *path) { return has_ext(path, ".fwsc") || has_ext(path, ".bin"); }
static void file_sha(App *a, int i) {
    if (i < 0 || i >= a->nfiles || a->fsha[i][0]) return;
    char why[64];
    size_t n = 0;
    uint8_t *b = read_file(a->fpaths[i], &n, why, sizeof why);
    if (b) { fm1ntt_sha256_hex(b, n, a->fsha[i]); free(b); } else snprintf(a->fsha[i], sizeof a->fsha[i], "-");
}
static int demo_dir_of(App *a, int i, char *dir, size_t n) {
    char base[300];
    file_sha(a, i);
    base_dir(base, sizeof base);
    return show_dir(dir, n, base, a->fsha[i]);
}
static int host_battery_low(void) {
    MonBat b;
    memset(&b, 0, sizeof b);
#ifdef __APPLE__
    if (!mon_mac_battery(&b)) return 0;
#else
    if (!mon_read_battery(&b)) return 0;
#endif
    return b.has && b.pct >= 0 && b.pct <= 20 && !strncmp(b.status, "Discharging", 11);
}
/* May the worker run now? Not while you play a running firmware (input in the last 15 s), measure (a report, a benchmark, a Rec demo), record, or on a low battery. */
static int show_may_run(App *a, double now) {
    if (a->sw_pid || a->bg_demos) {
        if (now - a->sw_batt_ms >= 30000.0 || a->sw_batt_ms == 0) { a->sw_low = host_battery_low(); a->sw_batt_ms = now ? now : 1; }
    }
    int playing = fm1ntt_loaded(a->m) && !a->paused && now - (double)a->last_input_ms < 15000.0;
    return !playing && !a->job && !a->want_demo && !a->rec && !a->pl.on && !a->sw_low;
}
static int show_spawn(App *a, int i) {
    if (a->sw_pid || !g_self_exe[0] || i < 0 || i >= a->nfiles || !fw_is_firmware(a->fpaths[i])) return -1;
    file_sha(a, i);
    if (!show_sha_ok(a->fsha[i])) return -1;
    pid_t pid = fork();
    if (pid < 0) return -1;
    if (pid == 0) {                                                    /* only async-signal-safe calls until the exec */
        setpriority(PRIO_PROCESS, 0, 19);
#ifdef __linux__
        struct sched_param sp = {0};
        sched_setscheduler(0, SCHED_IDLE, &sp);
#endif
        int dn = open("/dev/null", O_RDWR);
        if (dn >= 0) { dup2(dn, 0); dup2(dn, 1); dup2(dn, 2); }
        if (fm1ntt_jit_available()) execl(g_self_exe, g_self_exe, "--showcase", "--jit", a->fpaths[i], (char *)NULL);
        else execl(g_self_exe, g_self_exe, "--showcase", a->fpaths[i], (char *)NULL);
        _exit(127);
    }
    a->sw_pid = pid;
    a->sw_stopped = a->sw_cancelling = 0;
    snprintf(a->sw_name, sizeof a->sw_name, "%s", a->files[i]);
    a->sw_start_ms = (double)SDL_GetTicks();
    return 0;
}
/* Asks the worker to stop; it cleans up and exits (the poll reaps it; after 5 s it is killed and its folder removed here). */
static void show_cancel(App *a) {
    if (!a->sw_pid || a->sw_cancelling) return;
    kill(a->sw_pid, SIGCONT);
    kill(a->sw_pid, SIGTERM);
    a->sw_cancelling = 1;
    a->sw_cancel_ms = (double)SDL_GetTicks();
}
/* The worker's folder, from the name it was started for. */
static int worker_dir(App *a, char *dir, size_t n) {
    for (int i = 0; i < a->nfiles; i++) if (!strcmp(a->files[i], a->sw_name) && show_sha_ok(a->fsha[i])) { char base[300]; base_dir(base, sizeof base); return show_dir(dir, n, base, a->fsha[i]); }
    return -1;
}
/* Once a loop pass. Reaps a finished worker, stops and continues it by the policy. 1 when something to draw changed. */
static int show_poll(App *a, double now) {
    if (!a->pl.on && a->pl.pcm) player_stop(a);                          /* a demo played to its end: free its sound */
    if (!a->sw_pid) return 0;
    int st = 0;
    pid_t r = waitpid(a->sw_pid, &st, WNOHANG);
    if (r == 0) {
        if (a->sw_cancelling) { if (now - a->sw_cancel_ms > 5000.0) kill(a->sw_pid, SIGKILL); return 0; }
        int may = show_may_run(a, now);
        if (!may && !a->sw_stopped) { kill(a->sw_pid, SIGSTOP); a->sw_stopped = 1; return 1; }
        if (may && a->sw_stopped) { kill(a->sw_pid, SIGCONT); a->sw_stopped = 0; return 1; }
        return 0;
    }
    char dir[420];
    int have_dir = worker_dir(a, dir, sizeof dir) == 0;
    ShowState s;
    if (have_dir) show_state_read(dir, &s); else memset(&s, 0, sizeof s);
    int cancelled = a->sw_cancelling;
    a->sw_pid = 0; a->sw_stopped = a->sw_cancelling = 0;
    a->sw_next_ms = now + 2000.0;
    if (cancelled || (WIFEXITED(st) && WEXITSTATUS(st) == 3)) { if (have_dir) show_clear(dir); say(a, 0, "Demo stopped", NULL); return 1; }
    if (WIFEXITED(st) && WEXITSTATUS(st) == 0 && have_dir && !strcmp(s.state, "ready")) { say(a, 1, "A demo of %s is ready", a->sw_name); return 1; }
    if (have_dir && !strcmp(s.state, "failed")) { say(a, 2, "The demo of %s failed: see its page", a->sw_name); return 1; }
    if (have_dir) {                                                     /* the worker died without saying why: a failure, written here, never retried in a loop */
        mkdir_p(dir);
        s.tour = SHOW_TOUR; s.attempts = s.attempts ? s.attempts : 1;
        snprintf(s.state, sizeof s.state, "failed");
        snprintf(s.reason, sizeof s.reason, WIFSIGNALED(st) ? "the worker stopped (signal %d)" : "the worker could not run (exit %d)", WIFSIGNALED(st) ? WTERMSIG(st) : WEXITSTATUS(st));
        show_state_write(dir, &s);
        say(a, 2, "The demo of %s could not be made", a->sw_name);
    }
    return 1;
}
/* One file looked at every 2 s (hashed once, its sidecar read): the first that needs a demo starts the worker. A pass over the folder, then a minute's rest. */
static void show_queue(App *a, double now) {
    if (!a->bg_demos || a->sw_pid || now < a->sw_next_ms) return;
    if (!show_may_run(a, now)) { a->sw_next_ms = now + 5000.0; return; }
    a->sw_next_ms = now + 2000.0;
    if (a->sw_cursor >= a->nfiles) { a->sw_cursor = 0; a->sw_next_ms = now + 60000.0; return; }
    int i = a->sw_cursor++;
    if (!fw_is_firmware(a->fpaths[i])) return;
    char dir[420];
    if (demo_dir_of(a, i, dir, sizeof dir) != 0) return;
    ShowState s;
    show_state_read(dir, &s);
    if (show_needed(&s)) show_spawn(a, i);
}
/* The app is closing: the worker is asked to stop and given 2 s; then it is killed and its half folder removed. */
static void show_shutdown(App *a) {
    if (!a->sw_pid) return;
    show_cancel(a);
    for (int k = 0; k < 40; k++) { int st; if (waitpid(a->sw_pid, &st, WNOHANG) == a->sw_pid) { a->sw_pid = 0; return; } usleep(50000); }
    kill(a->sw_pid, SIGKILL);
    int st;
    waitpid(a->sw_pid, &st, 0);
    char dir[420];
    if (worker_dir(a, dir, sizeof dir) == 0) show_clear(dir);
    a->sw_pid = 0;
}

/* ---- the firmware's page ------------------------------------------------------------------------------------------------------------------------ */
/* The manifest line of this SHA-256 in `dir`/MANIFEST.tsv: tab-separated path, sha256, bytes, licence, name, version, source, built_from. */
static int manifest_find(const char *dir, const char *sha, char *licence, size_t ln, char *source, size_t sn, char *name, size_t nn) {
    char p[420];
    snprintf(p, sizeof p, "%s/MANIFEST.tsv", dir);
    FILE *f = fopen(p, "r");
    if (!f) return 0;
    char line[1024];
    int found = 0;
    while (!found && fgets(line, sizeof line, f)) {
        char *col[8];
        int n = 0;
        for (char *c = line; n < 8; ) { col[n++] = c; char *t = strchr(c, '\t'); if (!t) break; *t = 0; c = t + 1; }
        if (n >= 7 && !strcmp(col[1], sha)) {
            char *nl = strchr(col[n - 1], '\n'); if (nl) *nl = 0;
            snprintf(licence, ln, "%s", col[3]); snprintf(name, nn, "%s %s", col[4], col[5]); snprintf(source, sn, "%s", col[6]);
            found = 1;
        }
    }
    fclose(f);
    return found;
}
static void info_open(App *a, int i) {
    memset(&a->info_rec, 0, sizeof a->info_rec);
    file_sha(a, i);
    const char *dir = a->fbundled[i] == 1 ? a->bdir : a->fbundled[i] == 2 ? a->pdir : "";
    if (dir[0] && show_sha_ok(a->fsha[i])) {
        /* beside the firmware (the repository), else in the installed package's doc folder (usr/share/doc/..., two levels up from usr/share/fm1ntt/firmware) */
        const char *docs = a->fbundled[i] == 1 ? "doc/fm1ntt/bundled" : "doc/fm1ntt-firmware-pack";
        char alt[460];
        snprintf(alt, sizeof alt, "%s/../../%s", dir, docs);
        for (int k = 0; k < 2 && !a->info_rec.found; k++)
            a->info_rec.found = manifest_find(k ? alt : dir, a->fsha[i], a->info_rec.licence, sizeof a->info_rec.licence, a->info_rec.source, sizeof a->info_rec.source, a->info_rec.name, sizeof a->info_rec.name);
    }
    a->info_on = 1;
    a->info_i = i;
}
static void player_stop(App *a) {
    if (!a->pl.pcm && !a->pl.on) return;
    lock(a);
    a->pl.on = 0;
    unlock(a);
    free(a->pl.pcm);
    a->pl.pcm = NULL;
    a->pl.cur = -1;
}
static int player_start(App *a, int i) {
    char dir[420];
    if (demo_dir_of(a, i, dir, sizeof dir) != 0) { say(a, 2, "No demo to play", NULL); return -1; }
    ShowState s;
    show_state_read(dir, &s);
    if (!s.have || strcmp(s.state, "ready")) { say(a, 2, "No demo to play yet: Y makes it", NULL); return -1; }
    player_stop(a);
    int total = 0, nf = show_frames_read(dir, a->pl.ms, 400, &total);
    int16_t *pcm = NULL;
    size_t fr = 0;
    char wp[460];
    snprintf(wp, sizeof wp, "%s/audio.wav", dir);
    if (nf < 1 || show_wav_read(wp, &pcm, &fr) != 0) { free(pcm); say(a, 3, "The demo's files cannot be read", NULL); return -1; }
    snprintf(a->pl.dir, sizeof a->pl.dir, "%s", dir);
    snprintf(a->pl.name, sizeof a->pl.name, "%s", a->files[i]);
    snprintf(a->pl.tier, sizeof a->pl.tier, "%s", a->fbundled[i] == 1 ? "BUNDLED (PERMISSIVE)" : a->fbundled[i] == 2 ? "THE GPL PACK (COPYLEFT)" : "YOUR OWN FILE");
    snprintf(a->pl.licence, sizeof a->pl.licence, "%s", a->info_on && a->info_i == i && a->info_rec.found ? a->info_rec.licence : "");
    a->pl.nf = nf; a->pl.total_ms = total; a->pl.cur = -1; a->pl.pos = 0;
    lock(a);
    a->pl.pcm = pcm; a->pl.frames = fr; a->pl.on = 1;
    unlock(a);
    say(a, 0, "Playing the demo: any key stops it", NULL);
    return 0;
}
/* A press on FILES: the page when it is open, or A opening it on a firmware. 1 when handled. */
static int files_press(App *a, int act) {
    if (a->info_on) {
        int i = a->info_i;
        if (i < 0 || i >= a->nfiles) { a->info_on = 0; return 1; }
        if (act == A_OK) { a->info_on = 0; const char *p = a->fpaths[i]; load_start(a, p, has_ext(p, ".bin"), FM1_ENTRY_FELUCCA); }
        else if (act == A_X) player_start(a, i);
        else if (act == A_Y) {
            if (a->sw_pid && !strcmp(a->sw_name, a->files[i])) { show_cancel(a); say(a, 0, "Stopping the demo", NULL); }
            else if (a->sw_pid) say(a, 2, "Another demo is being made: wait for it or stop it", NULL);
            else if (show_spawn(a, i) == 0) say(a, 0, "Making the demo: it runs at the lowest priority", NULL);
            else say(a, 2, "Cannot start the demo worker", NULL);
        } else if (act == A_BACK) a->info_on = 0;
        else if (act == A_UP || act == A_DOWN || act == A_PGUP || act == A_PGDN) {}       /* the page does not move */
        return 1;
    }
    if (act == A_OK && a->file_sel < a->nfiles && fw_is_firmware(a->fpaths[a->file_sel])) { info_open(a, a->file_sel); return 1; }
    return 0;
}

/* The UI's side of it: the progress box, the page's text, and the demo's picture in the FM-1's screen while it plays. */
static void ui_showcase(App *a, UiState *u) {
    char dir[420], p[460];
    if (a->sw_pid) {
        snprintf(u->demo_run, sizeof u->demo_run, "%s", a->sw_name);
        u->demo_secs = (int)(((double)SDL_GetTicks() - a->sw_start_ms) / 1000.0);
        u->demo_paused = a->sw_stopped;
        if (worker_dir(a, dir, sizeof dir) == 0) {
            char t[40]; snprintf(p, sizeof p, "%s/progress.txt", dir);
            FILE *f = fopen(p, "r"); size_t n = f ? fread(t, 1, sizeof t - 1, f) : 0; if (f) fclose(f); t[n] = 0;
            const char *e = strstr(t, "pct=");
            u->demo_pct = e ? atoi(e + 4) : 0;
        }
    }
    if (a->info_on && a->info_i >= 0 && a->info_i < a->nfiles) {
        int i = a->info_i;
        u->info_on = 1;
        snprintf(u->info_name, sizeof u->info_name, "%s", a->files[i]);
        snprintf(u->info_tier, sizeof u->info_tier, "%s", a->fbundled[i] == 1 ? "BUNDLED (PERMISSIVE)" : a->fbundled[i] == 2 ? "THE GPL PACK (COPYLEFT)" : "YOUR OWN FILE");
        snprintf(u->info_licence, sizeof u->info_licence, "%s", a->info_rec.found ? a->info_rec.licence : "NOT KNOWN: A FILE YOU BROUGHT");
        snprintf(u->info_source, sizeof u->info_source, "%s", a->info_rec.found ? a->info_rec.source : "-");
        if (show_sha_ok(a->fsha[i])) snprintf(u->info_sha, sizeof u->info_sha, "%s", a->fsha[i]);
        u->info_busy = a->sw_pid && !strcmp(a->sw_name, a->files[i]);
        ShowState s;
        memset(&s, 0, sizeof s);
        if (demo_dir_of(a, i, dir, sizeof dir) == 0) show_state_read(dir, &s);
        if (u->info_busy) { u->info_demo_kind = 1; u->info_pct = u->demo_pct; snprintf(u->info_demo, sizeof u->info_demo, "BEING MADE%s: %d%%, %ds", u->demo_paused ? " (PAUSED WHILE YOU PLAY)" : "", u->demo_pct, u->demo_secs); }
        else if (s.have && !strcmp(s.state, "ready")) { u->info_demo_kind = 2; u->info_can_play = 1; snprintf(u->info_demo, sizeof u->info_demo, "READY: %d PICTURES, %d S", s.frames, s.ms / 1000); }
        else if (s.have && !strcmp(s.state, "failed")) { u->info_demo_kind = 3; snprintf(u->info_demo, sizeof u->info_demo, "FAILED: %.90s", s.reason); }
        else if (s.have) snprintf(u->info_demo, sizeof u->info_demo, "NOT FINISHED: Y MAKES IT");
        else snprintf(u->info_demo, sizeof u->info_demo, "NONE YET: Y MAKES IT%s", a->bg_demos ? " (OR IT IS QUEUED)" : "");
    }
    if (a->pl.on && a->pl.nf > 0) {                                      /* the demo's own picture, by the sound's position */
        int t = (int)((unsigned long long)a->pl.pos * 1000 / FM1_SAMPLE_RATE), idx = 0;
        for (int k = 0; k < a->pl.nf; k++) if (a->pl.ms[k] <= t) idx = k;
        if (idx != a->pl.cur) {
            char pp[460];
            snprintf(pp, sizeof pp, "%s/%04d.png", a->pl.dir, idx);
            if (show_png_read(pp, a->pl_pic, FM1_SCREEN_W, FM1_SCREEN_H) == 0) a->pl.cur = idx;
        }
        if (a->pl.cur >= 0) { memcpy(u->fb, a->pl_pic, sizeof u->fb); u->fb_valid = 1; }
        u->run_state = "DEMO";
        u->playing = 1;
        u->pl_tick = (int)((unsigned long long)a->pl.pos * SHOW_FPS / FM1_SAMPLE_RATE);
        u->pl_ticks = SHOW_TICKS; u->pl_idx = a->pl.cur >= 0 ? a->pl.cur : 0; u->pl_nf = a->pl.nf;
        snprintf(u->pl_step, sizeof u->pl_step, "%s", show_step(u->pl_tick));
        snprintf(u->pl_name, sizeof u->pl_name, "%s", a->pl.name); snprintf(u->pl_tier, sizeof u->pl_tier, "%s", a->pl.tier); snprintf(u->pl_licence, sizeof u->pl_licence, "%s", a->pl.licence);
        u->pl_keys = show_keys_at(u->pl_tick);
        size_t pos = a->pl.pos < a->pl.frames ? a->pl.pos : a->pl.frames;           /* the sound just played: the scope (the last 960 frames, mono) and the peak of the last 1470 */
        for (int k = 0; k < PX_SCOPE; k++) { size_t f = pos > 960 ? pos - 960 + (size_t)k * 4 : (size_t)k * 4; u->scope[k] = f < a->pl.frames ? (int16_t)((a->pl.pcm[f * 2] + a->pl.pcm[f * 2 + 1]) / 2) : 0; }
        int pl_ = 0, pr_ = 0;
        for (size_t f = pos > 1470 ? pos - 1470 : 0; f < pos; f++) { int l = abs(a->pl.pcm[f * 2]), r = abs(a->pl.pcm[f * 2 + 1]); if (l > pl_) pl_ = l; if (r > pr_) pr_ = r; }
        u->level_l = pl_ * 100 / 32768; u->level_r = pr_ * 100 / 32768;
    }
}

/* ---- the data rain (monitor.h): one row of real data, from the first source that has some; the source rotates ---------------------------------- */
static void rain_row_feed(App *a, double now_ms) {
    uint8_t b[RAIN_COLS];
    for (int k = 0; k < RAIN_SRC; k++) {
        int src = (int)((a->rain.step + (unsigned)k) % RAIN_SRC), n = 0;
        if (src == 0) { for (int i = 0; i < RAIN_COLS; i++) b[i] = (uint8_t)(a->scope[(i * 7) % PX_SCOPE] >> 8); n = RAIN_COLS; }     /* the audio out, high bytes */
        else if (src == 1) { n = a->midi_log_n < RAIN_COLS ? a->midi_log_n : RAIN_COLS; if (n) memcpy(b, a->midi_log + a->midi_log_n - n, (size_t)n); }
        else if (src == 2) {                                                                                                  /* the LCD's pixels as RGB bytes */
            if (a->fb_ok) { int y = (int)(a->rain.step % 240); for (int i = 0; i < RAIN_COLS; i++) { uint32_t px = a->fbc[y * 240 + 10 + i / 3]; b[i] = (uint8_t)(px >> (8 * (2 - i % 3))); } n = RAIN_COLS; }
        } else if (fm1ntt_loaded(a->m)) {                                                                                     /* a page of the emulated RAM */
            uint32_t b32; size_t size = 0;
            lock(a);
            fm1ntt_region(a->m, 0, &b32, &size);
            if (size > RAIN_COLS) n = (int)fm1ntt_read_mem(a->m, 0, (uint32_t)((a->rain.step * RAIN_COLS) % (size - RAIN_COLS)), b, RAIN_COLS);
            unlock(a);
        }
        if (n > 0) { rain_feed(&a->rain, now_ms, src, b, n); return; }
    }
}
/* Reduced motion: the page is filled once and never steps. */
static void rain_still(App *a) { for (int i = 0; i < RAIN_ROWS; i++) rain_row_feed(a, 0); }

/* ---- the progress box: shown over the window for as long as a job (Device report, Benchmark) runs, so a wait is never silent ---- */
static int job_pct(const App *a) {
    if (!a->job) return MON_NA;
    int step, steps; char doing[96]; uint32_t since, step_since;
    prog_get(&step, &steps, doing, sizeof doing, &since, &step_since);
    uint32_t now = SDL_GetTicks();
    if (steps < 1) steps = 1;
    double within = 1.0 - exp(-(double)(now - step_since) / 3000.0);       /* the current step creeps toward its end, never reaching it */
    int pct = (int)(100.0 * ((double)step + 0.9 * within) / steps);
    return pct > 99 ? 99 : pct;
}
static void draw_job_progress(uint32_t *c, const App *a) {
    if (!a->job) return;
    int step, steps; char doing[96]; uint32_t since, step_since;
    prog_get(&step, &steps, doing, sizeof doing, &since, &step_since);
    uint32_t now = SDL_GetTicks();
    if (steps < 1) steps = 1;
    int pct = job_pct(a);
    PxRect box = {PX_W / 2 - 220, PX_H / 2 - 56, 440, 112};
    px_rect(c, (PxRect){box.x - 2, box.y - 2, box.w + 4, box.h + 4}, C_BG);
    px_box(c, box, a->job_kind == JOB_REPORT ? "DEVICE REPORT" : a->job_kind == JOB_LOAD ? "LOADING THE FIRMWARE" : "BENCHMARK");
    char line[160];
    snprintf(line, sizeof line, "STEP %d OF %d: %s", step < steps ? step + 1 : steps, steps, doing[0] ? doing : "starting");
    px_text(c, box.x + 14, box.y + 20, line, C_TEXT, 1, 0);
    px_meter(c, (PxRect){box.x + 14, box.y + 44, box.w - 28, 14}, pct, C_HEAD);
    if (a->job_kind == JOB_LOAD) snprintf(line, sizeof line, "%d%%   %u.%u s elapsed", pct, (unsigned)((now - since) / 1000), (unsigned)((now - since) % 1000 / 100));
    else snprintf(line, sizeof line, "%d%%   %us elapsed   the emulation is paused while it measures", pct, (now - since) / 1000);
    px_text(c, box.x + 14, box.y + 68, line, C_DIM, 1, 0);
    px_text(c, box.x + 14, box.y + 86, a->job_kind == JOB_LOAD ? "The firmware is being read and started" : "Nothing is wrong: wait for the result", C_DIM, 1, 0);
}

/* ---- the self-test ----------------------------------------------------------------------------------------------------- */
#ifdef __linux__
/* the placement test on a job thread, as the app runs it (the self-test checks it end to end with short windows) */
typedef struct { char fw[300]; PinAB ab; int rc; } PinJob;
static int pin_job(void *p) {
    PinJob *j = p;
    j->rc = speed_firmware_pin_ab(j->fw, 0, fm1ntt_jit_available(), fm1ntt_jit_available() ? 3 : 0, 0.05, &j->ab);
    return 0;
}
#endif
/* ---- the MONITOR panel's tests (monitor.h, docs/MONITOR.md) -------------------------------------------------------------------------------- */
static void mon_dump_bmp(const char *path, const uint32_t *canvas) {      /* FM1NTT_MON_SHOTS=dir: the panel's pictures, for a person to look at */
    FILE *f = fopen(path, "wb");
    if (!f) return;
    uint32_t size = 54 + PX_W * PX_H * 3;
    uint8_t h[54] = {'B', 'M'};
    memcpy(h + 2, &size, 4); h[10] = 54; h[14] = 40;
    int32_t w = PX_W, hh = PX_H; memcpy(h + 18, &w, 4); memcpy(h + 22, &hh, 4); h[26] = 1; h[28] = 24;
    fwrite(h, 1, 54, f);
    for (int y = PX_H - 1; y >= 0; y--) for (int x = 0; x < PX_W; x++) { uint32_t p = canvas[y * PX_W + x]; uint8_t b[3] = {(uint8_t)p, (uint8_t)(p >> 8), (uint8_t)(p >> 16)}; fwrite(b, 1, 3, f); }
    fclose(f);
}
static void mon_put(const char *dir, const char *name, const char *text) {
    char p[480];
    mkdir_p(dir);
    snprintf(p, sizeof p, "%s/%s", dir, name);
    write_atomic(p, text, strlen(text));
}
static void mon_put_proc(const char *procdir, int pid, const char *name, unsigned long long ut, unsigned long long st, unsigned long long rss_pages) {
    char d[400], t[300];
    snprintf(d, sizeof d, "%s/%d", procdir, pid);
    snprintf(t, sizeof t, "%d (%s) S 1 1 1 0 -1 0 0 0 0 0 %llu %llu 0 0 20 0 1 0 100 1000 %llu 0\n", pid, name, ut, st, rss_pages);
    mon_put(d, "stat", t);
}

static int mon_selftest(const char *root, App *app) {
    int bad = 0;
    static UiState mui;                                              /* its own: the caller's has been freed by now */
    UiState *ui = &mui;
#define MCHECK(c) do { if (!(c)) { fprintf(stderr, "selftest FAIL (monitor): %s (line %d)\n", #c, __LINE__); bad++; } } while (0)
    /* thresholds: one table, inclusive edges, and the glyph says what the colour says */
    MCHECK(mon_level(TH_CPU, 69) == LV_OK && mon_level(TH_CPU, 70) == LV_WARN && mon_level(TH_CPU, 89) == LV_WARN && mon_level(TH_CPU, 90) == LV_CRIT && mon_level(TH_CPU, -1) == LV_OK);
    MCHECK(mon_level(TH_BATT, 31) == LV_OK && mon_level(TH_BATT, 30) == LV_WARN && mon_level(TH_BATT, 16) == LV_WARN && mon_level(TH_BATT, 15) == LV_CRIT && mon_level(TH_BATT, 0) == LV_CRIT);
    MCHECK(mon_level(TH_SPEED, 97) == LV_OK && mon_level(TH_SPEED, 96) == LV_WARN && mon_level(TH_SPEED, 50) == LV_WARN && mon_level(TH_SPEED, 49) == LV_CRIT);
    MCHECK(mon_level(TH_FRAME, 10) == LV_OK && mon_level(TH_FRAME, 11) == LV_WARN && mon_level(TH_FRAME, 25) == LV_CRIT);
    MCHECK(mon_level(TH_TEMP, 64) == LV_OK && mon_level(TH_TEMP, 65) == LV_WARN && mon_level(TH_TEMP, 80) == LV_CRIT);
    MCHECK(mon_level(TH_MEM, 74) == LV_OK && mon_level(TH_MEM, 75) == LV_WARN && mon_level(TH_MEM, 90) == LV_CRIT);
    MCHECK(mon_level(TH_CARD, 16) == LV_OK && mon_level(TH_CARD, 15) == LV_WARN && mon_level(TH_CARD, 5) == LV_CRIT);
    MCHECK(mon_level(99, 50) == LV_OK && mon_level(-1, 50) == LV_OK);
    MCHECK(mon_glyph(LV_OK) == ' ' && mon_glyph(LV_WARN) == '*' && mon_glyph(LV_CRIT) == '!');
    for (int i = 0; i < TH_COUNT; i++) MCHECK(MON_TH[i].low_is_bad ? MON_TH[i].warn > MON_TH[i].crit : MON_TH[i].warn < MON_TH[i].crit);
    /* the colours: text pairs 4.5:1 (the whole-palette check above), and a bar's fill 3:1 against the bar's own ground, and the three verdict colours differ */
    MCHECK(px_contrast(C_OK, C_BG) >= 3.0 && px_contrast(C_WARN, C_BG) >= 3.0 && px_contrast(C_ERR, C_BG) >= 3.0);
    MCHECK(px_contrast(C_OK, C_FRAME) >= 3.0 && px_contrast(C_WARN, C_FRAME) >= 3.0 && px_contrast(C_ERR, C_FRAME) >= 3.0);
    MCHECK(px_lv_col(LV_OK) != px_lv_col(LV_WARN) && px_lv_col(LV_WARN) != px_lv_col(LV_CRIT) && px_lv_col(LV_OK) != px_lv_col(LV_CRIT));
    /* the percent maths: a counter that goes backwards, stands still, or does not add up is unknown, never a number */
    MonCpuRaw a0 = {1000, 400, 1}, a1 = {1200, 500, 1};
    MCHECK(mon_pct(&a0, &a1) == 50);
    MonCpuRaw back = {900, 300, 1}, still = {1000, 400, 1}, idle_back = {1200, 300, 1}, idle_over = {1100, 600, 1}, none = {0, 0, 0};
    MCHECK(mon_pct(&a0, &back) == MON_NA && mon_pct(&a0, &still) == MON_NA && mon_pct(&a0, &idle_back) == MON_NA && mon_pct(&a0, &idle_over) == MON_NA);
    MCHECK(mon_pct(&none, &a1) == MON_NA && mon_pct(&a0, &none) == MON_NA);
    MonCpuRaw full0 = {0, 0, 1}, full1 = {100, 0, 1}, big0 = {1ULL << 60, 1ULL << 59, 1}, big1 = {(1ULL << 60) + 1000, (1ULL << 59) + 250, 1};
    MCHECK(mon_pct(&full0, &full1) == 100 && mon_pct(&big0, &big1) == 75);
    /* /proc/stat: a normal file, a truncated line, garbage, more cores than the cap, numbers that would wrap a sum */
    MonCpuRaw all, cores[MON_CORES];
    MCHECK(mon_parse_stat("cpu  400 0 0 400 0 0 0 0 0 0\ncpu0 100 0 0 100 0 0 0 0\ncpu1 300 0 0 300 5 1 2 3\nintr 5\nctxt 6\n", &all, cores, MON_CORES) == 2);
    MCHECK(all.ok && all.total == 800 && all.idle == 400 && cores[0].total == 200 && cores[0].idle == 100 && cores[1].idle == 305 && cores[1].total == 611 && !cores[2].ok);
    MCHECK(mon_parse_stat("cpu0 1 2\ncpux 1 2 3 4\ncpu 1 2\n\n\n", &all, cores, MON_CORES) == 0 && !all.ok && !cores[0].ok);
    MCHECK(mon_parse_stat("", &all, cores, MON_CORES) == 0 && mon_parse_stat("garbage \xff\x01 no newline", &all, cores, MON_CORES) == 0);
    char many[4096] = "";
    for (int i = 0; i < 40; i++) { char l[64]; snprintf(l, sizeof l, "cpu%d 1 1 1 1 1 1 1 1\n", i); strcat(many, l); }
    MCHECK(mon_parse_stat(many, &all, cores, MON_CORES) == MON_CORES);
    MCHECK(mon_parse_stat("cpu0 99999999999999999999999 99999999999999999999999 99999999999999999999999 99999999999999999999999 99999999999999999999999 99999999999999999999999 99999999999999999999999 99999999999999999999999\n", &all, cores, 4) == 1 && cores[0].total >= (1ULL << 60) && cores[0].total <= 8 * (1ULL << 60));
    MCHECK(mon_parse_stat("cpu-1 1 1 1 1\ncpu5 1 1 1 1\n", &all, cores, 4) == 0);                 /* a core number outside the table is dropped, not written */
    /* /proc/meminfo */
    MonMem mm;
    mon_parse_meminfo("MemTotal:  2000000 kB\nMemFree:  100000 kB\nMemAvailable:  800000 kB\nBuffers: 1 kB\nCached:  500000 kB\nSwapTotal: 100000 kB\nSwapFree: 40000 kB\n", &mm);
    MCHECK(mm.has && mm.total_kb == 2000000 && mm.used_kb == 1200000 && mm.cached_kb == 500000 && mm.swap_used_kb == 60000 && mm.free_kb == 100000);
    mon_parse_meminfo("MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 150 kB\n", &mm);
    MCHECK(mm.has && mm.used_kb == 700);                                                           /* no MemAvailable: free + buffers + cached */
    mon_parse_meminfo("MemTotal: 99999999999999999999 kB\nMemAvailable: 5 kB\nSwapTotal: 1 kB\nSwapFree: 9 kB\n", &mm);
    MCHECK(mm.has && mm.total_kb <= (1ULL << 40) && mm.used_kb <= mm.total_kb && mm.swap_used_kb == 0);
    mon_parse_meminfo("garbage\n\xff", &mm);
    MCHECK(!mm.has && mm.used_kb == 0);
    mon_parse_meminfo("MemTotal: 100 kB\nMemAvailable: 900 kB\n", &mm);
    MCHECK(mm.used_kb == 0);                                                                       /* available above total: clamped, not a wrapped number */
    double la[3];
    MCHECK(mon_parse_loadavg("0.52 0.40 0.31 1/200 999\n", la) && la[0] > 0.51 && la[2] < 0.32);
    MCHECK(!mon_parse_loadavg("", la) && !mon_parse_loadavg("a b c", la) && !mon_parse_loadavg("-1 0 0", la) && !mon_parse_loadavg("nan 0 0", la) && !mon_parse_loadavg("1e99 0 0", la) && !mon_parse_loadavg("0.5 0.4", la));
    /* /proc/<pid>/stat: a name with spaces and a ")", a short line, garbage, a huge resident size */
    MonProc mp;
    MCHECK(mon_parse_pstat("77 (my (odd) name) S 1 1 1 0 -1 0 0 0 0 0 120 30 0 0 20 0 1 0 100 1000 250 0\n", &mp, 4) && mp.pid == 77 && !strcmp(mp.name, "my (odd) name") && mp.ticks == 150 && mp.rss_kb == 1000);
    MCHECK(!mon_parse_pstat("77 (x) S 1 1 1", &mp, 4) && !mon_parse_pstat("", &mp, 4) && !mon_parse_pstat("no parens at all", &mp, 4) && !mon_parse_pstat(") (", &mp, 4));
    MCHECK(mon_parse_pstat("5 (a) S 1 1 1 0 -1 0 0 0 0 0 99999999999999999999 1 0 0 20 0 1 0 1 1 99999999999999999999 0", &mp, 4) && mp.ticks <= (2ULL << 50) && mp.rss_kb >= 0);
    MCHECK(mon_parse_pstat("5 (a) S 1 1 1 0 -1 0 0 0 0 0 1 2", &mp, 4) && mp.ticks == 3 && mp.rss_kb == 0);          /* no rss field: the CPU time is still good */
    /* the battery, the temperature */
    MonBat bt;
    mon_battery_from("87\n", "Discharging\n", "-500000\n", "3800000\n", &bt);
    MCHECK(bt.has && bt.pct == 87 && !strcmp(bt.status, "Discharging") && bt.mw == 1900);
    mon_battery_from("abc", "\xff\x01zz", "x", "y", &bt);
    MCHECK(bt.has && bt.pct == MON_NA && bt.mw == MON_NA);
    mon_battery_from("150", "Full", "99999999999999999999", "99999999999999999999", &bt);
    MCHECK(bt.pct == MON_NA && bt.mw == MON_NA);
    mon_battery_from("50", "Charging", "400000", "0", &bt);
    MCHECK(bt.pct == 50 && bt.mw == MON_NA);
    MCHECK(mon_parse_milli("54000\n") == 54 && mon_parse_milli("x") == MON_NA && mon_parse_milli("") == MON_NA && mon_parse_milli("999999999") == MON_NA && mon_parse_milli("-300000") == MON_NA && mon_parse_milli("-5500") == -5);
    /* the bounded sorted list */
    static MonProc big[300], top[MON_TOP];
    for (int i = 0; i < 300; i++) { big[i].pid = 1000 - i; big[i].cpu_x10 = (i * 37) % 1000; big[i].rss_kb = (i * 91) % 5000; snprintf(big[i].name, sizeof big[i].name, "p%d", i); }
    for (int sort = 0; sort < MON_SORTS; sort++) {
        int n = mon_top(big, 300, top, sort), ok = n == MON_TOP;
        for (int i = 1; i < n; i++) ok &= !mon_before(&top[i], &top[i - 1], sort);
        for (int i = 0; i < 300 && ok; i++) { int worse = 0; for (int k = 0; k < n; k++) worse += mon_before(&big[i], &top[k], sort) ? 1 : 0; if (worse && i >= 0) { int in = 0; for (int k = 0; k < n; k++) in |= top[k].pid == big[i].pid; if (!in) { /* an entry left out must sort after the last kept one */ ok &= !mon_before(&big[i], &top[n - 1], sort); } } }
        MCHECK(ok);
    }
    MCHECK(mon_top(big, 3, top, MON_SORT_PID) == 3 && top[0].pid < top[1].pid && mon_top(big, 0, top, 0) == 0);
    MCHECK(mon_pages(0, 0) == 2 && mon_pages(1, 0) == 3 && mon_pages(0, MON_ROWS_STD) == 2 && mon_pages(0, MON_ROWS_STD + 1) == 3 && mon_pages(1, MON_TOP) == 2 + (MON_TOP + MON_ROWS_NARROW - 1) / MON_ROWS_NARROW);
    MCHECK(!mon_page_has_procs(0, 0) && mon_page_has_procs(0, 1) && !mon_page_has_procs(1, 1) && mon_page_has_procs(1, 2));
    int jc = -5, jg = -5;
    MCHECK(mon_parse_jit("{\"blocks_cold\":3,\"blocks_compiled\":120,\"blocks_given_up\":2,\"cache_bytes\":9}", &jc, &jg) && jc == 120 && jg == 2);
    MCHECK(!mon_parse_jit("", &jc, &jg) && !mon_parse_jit("{\"blocks_compiled\":1}", &jc, &jg));
    MCHECK(mon_parse_jit("{\"blocks_compiled\":-4,\"blocks_given_up\":99999999999999}", &jc, &jg) && jc == 0 && jg == 99999999);
    char kb[16];
    mon_kb_text(kb, sizeof kb, 512); MCHECK(!strcmp(kb, "512K"));
    mon_kb_text(kb, sizeof kb, 3 * 1024 * 1024 / 2); MCHECK(!strcmp(kb, "1.5G"));
    mon_kb_text(kb, sizeof kb, 99999999999ULL); MCHECK(strlen(kb) < 8);

    /* a fixture /proc and /sys: the sampler reads them, and reads NOTHING while the panel is not showing */
    char pr[420], sy[420], d[480];
    snprintf(pr, sizeof pr, "%s/monfix/proc", root);
    snprintf(sy, sizeof sy, "%s/monfix/sys", root);
    setenv("FM1NTT_MON_PROC", pr, 1);
    setenv("FM1NTT_MON_SYS", sy, 1);
    long hz = sysconf(_SC_CLK_TCK);
    char one[200];
    mon_put(pr, "stat", "cpu  400 0 0 400 0 0 0 0\ncpu0 100 0 0 100 0 0 0 0\ncpu1 300 0 0 300 0 0 0 0\n");
    mon_put(pr, "loadavg", "0.50 0.40 0.30 1/100 123\n");
    mon_put(pr, "meminfo", "MemTotal: 1000000 kB\nMemAvailable: 100000 kB\nMemFree: 50000 kB\nCached: 300000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n");
    snprintf(d, sizeof d, "%s/self", pr); mon_put(d, "statm", "1000 500 0 0 0 0 0\n");
    mon_put_proc(pr, 1234, "busy", 100, 0, 10);
    mon_put_proc(pr, 2222, "bigmem", 5, 5, 900000);
    mon_put_proc(pr, 3, "idle", 1, 1, 5);
    snprintf(d, sizeof d, "%s/4444", pr); mon_put(d, "stat", "garbage \xff no parens");
    snprintf(d, sizeof d, "%s/class/power_supply/BAT0", sy);
    mon_put(d, "type", "Battery\n"); mon_put(d, "capacity", "12\n"); mon_put(d, "status", "Discharging\n"); mon_put(d, "current_now", "-500000\n"); mon_put(d, "voltage_now", "3800000\n");
    snprintf(d, sizeof d, "%s/class/thermal/thermal_zone0", sy); mon_put(d, "temp", "71000\n");
    snprintf(d, sizeof d, "%s/devices/system/cpu/cpu0/cpufreq", sy); mon_put(d, "scaling_cur_freq", "1512000\n"); mon_put(d, "scaling_governor", "schedutil\n");
    snprintf(d, sizeof d, "%s/bus/usb/devices/1-1", sy); mon_put(d, "product", "USB Keyboard\n");
    snprintf(d, sizeof d, "%s/bus/usb/devices/usb1", sy); mon_put(d, "product", "xHCI Host Controller\n");
    snprintf(d, sizeof d, "%s/bus/usb/devices/1-2", sy); mon_put(d, "product", "Generic Hub\n");
    snprintf(d, sizeof d, "%s/bus/usb/devices/1-3", sy); mon_put(d, "product", "M-VAVE FM-1 \xff\n");
    snprintf(d, sizeof d, "%s/class/net/eth0", sy); mon_put(d, "operstate", "up\n");
    snprintf(d, sizeof d, "%s/class/net/lo", sy); mon_put(d, "operstate", "unknown\n");
    static Mon m;
    mon_init(&m, root);
    unsigned long r0 = g_mon_reads;
    for (int i = 0; i < 2000; i++) MCHECK(mon_tick(&m, 0, i * 7.0, 1) == 0);                          /* not showing: no sample, no read, however often it is asked */
    MCHECK(g_mon_reads == r0);
    MCHECK(mon_tick(&m, 1, 10000, 0) == 1 && g_mon_reads > r0 && !m.v.sampled && m.v.core_pct[0] == MON_NA && m.v.ncores == 2);   /* the first sample is a baseline: dashes, not guesses */
    MCHECK(m.v.mem.has && m.v.mem.used_kb == 900000 && m.v.has_load && m.v.temp_c == 71 && m.v.core_mhz[0] == 1512 && !strcmp(m.v.gov, "SCHEDUTIL") && !strcmp(m.v.net, "ETH0 UP"));
    MCHECK(m.v.nusb == 2 && ((!strcmp(m.v.usb[0], "USB Keyboard") && !strncmp(m.v.usb[1], "M-VAVE FM-1", 11)) || (!strncmp(m.v.usb[0], "M-VAVE FM-1", 11) && !strcmp(m.v.usb[1], "USB Keyboard"))));   /* hubs and host controllers are not listed; a odd byte is a ?, not passed on */
    MCHECK(m.v.bat.has && m.v.bat.pct == 12 && m.v.bat.mw == 1900 && m.v.self_rss_kb == 500 * mon_page_kb());
    MCHECK(!m.v.procs_read && m.v.nprocs == 0);                                                      /* the process list was not wanted: not read */
    unsigned long r1 = g_mon_reads;
    MCHECK(mon_tick(&m, 1, 10500, 0) == 0 && g_mon_reads == r1);                                      /* half a second later: nothing is due */
    mon_put(pr, "stat", "cpu  680 0 0 520 0 0 0 0\ncpu0 200 0 0 200 0 0 0 0\ncpu1 480 0 0 320 0 0 0 0\n");   /* cpu0 +100 busy +100 idle: 50%; cpu1 +180 busy +20 idle: 90%; all +280 of 400: 70% */
    mon_put_proc(pr, 1234, "busy", 100 + (unsigned long long)hz, 0, 10);
    unsigned long r2 = g_mon_reads;
    MCHECK(mon_tick(&m, 1, 11000, 1) == 1);
    unsigned long with_procs = g_mon_reads - r2;
    MCHECK(m.v.sampled && m.v.core_pct[0] == 50 && m.v.core_pct[1] == 90 && m.v.hist[1][MON_HIST - 1] == 90 && m.v.cpu_pct == 70);
    MCHECK(m.v.procs_read && m.v.nprocs == 3 && m.v.nproc_total >= 3 && !m.v.proc_capped);                  /* the garbage one is skipped, not fatal */
    MCHECK(m.v.procs[0].pid == 1234 || m.v.procs[0].cpu_x10 == 0);                                           /* the first list has no delta: sorted as idle */
    mon_put_proc(pr, 1234, "busy", 100 + 2 * (unsigned long long)hz, 0, 10);
    mon_put(pr, "stat", "cpu  500 0 0 440 0 0 0 0\ncpu0 200 0 0 200 0 0 0 0\ncpu1 480 0 0 320 0 0 0 0\n");   /* every counter still or lower: unknown */
    r2 = g_mon_reads;
    MCHECK(mon_tick(&m, 1, 12000, 0) == 1);
    MCHECK(g_mon_reads - r2 < with_procs && m.v.core_pct[0] == MON_NA && m.v.core_pct[1] == MON_NA);       /* a page with no list reads fewer files than one with it */
    mon_put_proc(pr, 1234, "busy", 100 + 3 * (unsigned long long)hz, 0, 10);
    mon_put(pr, "stat", "cpu  780 0 0 760 0 0 0 0\ncpu0 250 0 0 250 0 0 0 0\ncpu1 530 0 0 370 0 0 0 0\n");
    MCHECK(mon_tick(&m, 1, 13000, 1) == 1);
    MCHECK(m.v.procs[0].pid == 1234 && m.v.procs[0].cpu_x10 == 1000 && !strcmp(m.v.procs[0].name, "busy"));    /* one second of CPU in one second: 100.0% of a core */
    m.v.sort = MON_SORT_MEM; m.force = 1;
    MCHECK(mon_tick(&m, 1, 13300, 1) == 1 && m.v.procs[0].pid == 2222 && m.v.procs[0].rss_kb == 900000 * mon_page_kb());   /* a sort key asks for a sample now, once in 250 ms */
    m.v.sort = MON_SORT_PID; m.force = 1;
    MCHECK(mon_tick(&m, 1, 13400, 1) == 0);                                                          /* two keys within 250 ms: the second waits */
    MCHECK(mon_tick(&m, 1, 13600, 1) == 1 && m.v.procs[0].pid == 3);
    m.paused = 1;
    unsigned long r3 = g_mon_reads;
    for (int i = 0; i < 500; i++) MCHECK(mon_tick(&m, 1, 20000 + i * 50.0, 1) == 0);                    /* frozen: showing, but nothing is read */
    MCHECK(g_mon_reads == r3);
    m.paused = 0;
    MCHECK(mon_tick(&m, 0, 50000, 1) == 0 && !m.on && !m.v.sampled);                                    /* leaving the panel forgets the baseline: no gap is averaged into the next reading */
    MCHECK(mon_tick(&m, 1, 90000, 0) == 1 && !m.v.sampled && m.v.core_pct[0] == MON_NA);
    for (int i = 0; i < 20; i++) { snprintf(one, sizeof one, "%s/%d", pr, 5000 + i); mon_put(one, "stat", "1 (x) S 1 1 1 0 -1 0 0 0 0 0 1 1 0 0 20 0 1 0 1 1 1 0\n"); }
    char rm[520]; snprintf(rm, sizeof rm, "rm -rf '%s/monfix'", root); if (system(rm) != 0) fprintf(stderr, "could not remove the monitor fixture\n");
    MCHECK(mon_tick(&m, 1, 120000, 1) == 1 && !m.v.mem.has && m.v.nprocs == 0 && !m.v.has_load && m.v.temp_c == MON_NA && !m.v.bat.has && m.v.ncores == 0);  /* every file gone: n/a everywhere, no crash */
    unsetenv("FM1NTT_MON_PROC"); unsetenv("FM1NTT_MON_SYS");

    /* the data rain: the same bytes and step give the same rows, it steps at most 3 times a second, and not at all under reduced motion */
    static MonRain r1a, r1b;
    memset(&r1a, 0, sizeof r1a); memset(&r1b, 0, sizeof r1b);
    uint8_t bytes[7] = {1, 2, 3, 4, 5, 6, 7};
    for (int i = 0; i < 12; i++) { rain_feed(&r1a, i * 334.0, i % RAIN_SRC, bytes, 7); rain_feed(&r1b, i * 334.0, i % RAIN_SRC, bytes, 7); bytes[0]++; }
    MCHECK(!memcmp(&r1a, &r1b, sizeof r1a));
    char tg = 0;
    const uint8_t *row0 = rain_row(&r1a, 0, &tg);
    MCHECK(row0 && tg == RAIN_TAG[11 % RAIN_SRC] && row0[0] == 12 && rain_row(&r1a, 11, NULL) && !rain_row(&r1a, 12, NULL) && !rain_row(&r1a, -1, NULL) && !rain_row(&r1a, RAIN_ROWS, NULL));
    for (int i = 0; i < RAIN_ROWS + 9; i++) rain_feed(&r1a, 0, 0, bytes, 7);
    MCHECK(rain_row(&r1a, RAIN_ROWS - 1, NULL) && !rain_row(&r1a, RAIN_ROWS, NULL));                 /* the ring is bounded */
    rain_feed(&r1a, 0, 99, bytes, 0);                                                                  /* an odd source and no bytes: a row of zeros, no crash */
    MCHECK(rain_row(&r1a, 0, &tg)[5] == 0);
    static MonRain st;
    memset(&st, 0, sizeof st);
    int steps_total = 0, worst_window = 0;
    int stamps[4000], ns = 0;
    for (int ms = 0; ms < 4000; ms++) if (rain_due(&st, (double)ms, 0)) { rain_feed(&st, (double)ms, 0, bytes, 7); stamps[ns++] = ms; steps_total++; }
    for (int i = 0; i < ns; i++) { int c = 0; for (int k = i; k < ns && stamps[k] < stamps[i] + 1000; k++) c++; if (c > worst_window) worst_window = c; }
    MCHECK(worst_window <= 3 && steps_total >= 10);                                                    /* no second holds more than 3 changes */
    memset(&st, 0, sizeof st);
    int stepped = 0;
    for (int ms = 0; ms < 4000; ms++) stepped += rain_due(&st, (double)ms, 1);
    MCHECK(stepped == 0);                                                                                /* reduced motion: a still page */

    /* the panel, drawn: every page of both layouts at low, mid and high load, 1, 4 and 12 cores; it stays above the foot, is the same every time, and reads nothing */
    static UiState u;
    static uint32_t cv[PX_W * PX_H], cv2[PX_W * PX_H];
    unsigned long reads_before = g_mon_reads;
    const int loads[3] = {5, 55, 97};
    const int corecounts[3] = {1, 4, 12};
    const char *shots = getenv("FM1NTT_MON_SHOTS");
    for (int mode = 0; mode < 2; mode++)
        for (int li = 0; li < 3; li++)
            for (int ci = 0; ci < 3; ci++) {
                memset(&u, 0, sizeof u);
                for (int i = 0; i < FM_W * FM_H; i++) u.fb[i] = (uint32_t)(i * 2654435761u) & 0xFFFFFF;
                u.fb_valid = 1; u.panel = PANEL_MONITOR; u.run_state = "RUN"; u.fw_name = "X.FWSC";
                MonView *v = &u.mon;
                v->platform = MON_PLAT_LINUX; v->sampled = 1; v->ncores = corecounts[ci]; v->cpu_pct = loads[li]; v->has_load = 1; v->load[0] = loads[li] / 25.0; v->load[1] = 0.4; v->load[2] = 0.3;
                snprintf(v->gov, sizeof v->gov, "SCHEDUTIL"); v->temp_c = 40 + loads[li] / 3; v->hist_n = MON_HIST;
                for (int k = 0; k < v->ncores && k < MON_CORES; k++) { v->core_pct[k] = (loads[li] + k * 7) % 101; v->core_mhz[k] = 1512; for (int h = 0; h < MON_HIST; h++) v->hist[k][h] = (uint8_t)((loads[li] + h * 5 + k) % 101); }
                v->mem.has = 1; v->mem.total_kb = 2000000; v->mem.used_kb = 2000000ULL * (unsigned)loads[li] / 100; v->mem.cached_kb = 300000; v->mem.free_kb = 80000; v->mem.swap_total_kb = 500000; v->mem.swap_used_kb = 1000;
                v->self_rss_kb = 36000; v->has_card = 1; v->card_free_pct = 100 - loads[li]; v->card_free_mb = 12000;
                v->procs_read = 1; v->nproc_total = 300; v->proc_capped = 1; v->nprocs = MON_TOP; v->self_pid = 1;
                for (int k = 0; k < MON_TOP; k++) { v->procs[k].pid = 1 + k; snprintf(v->procs[k].name, MON_NAME, "%.15s", "proc-with-a-long-name"); v->procs[k].cpu_x10 = (1000 - k * 15) > 0 ? 1000 - k * 15 : 0; v->procs[k].rss_kb = 120000 - k * 1000; }
                v->nthr = MON_THR; for (int k = 0; k < MON_THR; k++) { snprintf(v->thr[k].name, sizeof v->thr[k].name, "thread-name-%d", k); v->thr[k].cpu_x10 = 900 - k * 80; v->thr[k].core = k % 4; }
                v->bat.has = 1; v->bat.pct = 100 - loads[li]; v->bat.mw = 1840; snprintf(v->bat.status, sizeof v->bat.status, "Discharging");
                v->nusb = 3; snprintf(v->usb[0], 28, "USB MIDI KEYBOARD"); snprintf(v->usb[1], 28, "M-VAVE FM-1"); snprintf(v->usb[2], 28, "GAMEPAD"); snprintf(v->app.pad[0], 28, "GAMEPAD NAME"); v->app.jit_blocks = 120; v->app.jit_given_up = li;
                v->wake_ps = 25; snprintf(v->net, sizeof v->net, "WLAN0 UP"); v->sample_us = 450; v->share_x100 = 5; v->app_cpu_x10 = loads[li] * 10;
                v->app.speed_pct = loads[li] < 50 ? 120 : 20; v->app.jit = 1; v->app.cb_per_s = 86; v->app.late_cb = li; v->app.frame_ms = 4 + li * 12; v->app.frame_max_ms = 30; v->app.njoy = 2;
                snprintf(v->app.job, sizeof v->app.job, "BENCHMARK"); v->app.job_pct = 40; snprintf(v->app.fw, sizeof v->app.fw, "A-LONG-FIRMWARE-NAME.FWSC"); snprintf(v->app.midi, sizeof v->app.midi, "SENT 12 DROP 12");
                snprintf(v->app.hot[0], sizeof v->app.hot[0], "USB MIDI KEYBOARD PLUGGED IN"); snprintf(v->app.hot[1], sizeof v->app.hot[1], "GAMEPAD OUT"); snprintf(v->app.hot[2], sizeof v->app.hot[2], "STORAGE IN");
                u.rain_on = 1;
                for (int i = 0; i < RAIN_ROWS; i++) { uint8_t b[RAIN_COLS]; for (int k = 0; k < RAIN_COLS; k++) b[k] = (uint8_t)(i * 31 + k * 7); rain_feed(&u.rain, 0, i, b, RAIN_COLS); }
                int pages = mon_pages(mode, v->nprocs);
                MCHECK(pages >= 3);
                for (int pg = 0; pg < pages; pg++) {
                    u.mon_page = pg;
                    px_draw(cv, mode, &u);
                    px_draw(cv2, mode, &u);
                    MCHECK(!memcmp(cv, cv2, sizeof cv));                                               /* the same state, the same pixels */
                    PxRect cr = px_content_rect(mode);
                    static uint32_t scratch[PX_W * PX_H];
                    int bottom = px_panel_monitor(scratch, cr, &u, mode);
                    if (bottom > cr.y + cr.h - 26) fprintf(stderr, "  layout: mode %d cores %d page %d bottom %d limit %d\n", mode, corecounts[ci], pg, bottom, cr.y + cr.h - 26);
                    MCHECK(bottom <= cr.y + cr.h - 26);                                                /* above the page line and the foot */
                    int s = px_scale(mode), ok = 1;
                    for (int y = 0; y < FM_H * s && ok; y++) for (int x = 0; x < FM_W * s; x++) if (cv[y * PX_W + x] != (0xFF000000u | u.fb[(y / s) * FM_W + x / s])) { ok = 0; break; }
                    MCHECK(ok);
                    long lit = 0;
                    for (int y = cr.y; y < cr.y + cr.h; y++) for (int x = cr.x; x < cr.x + cr.w; x++) lit += cv[y * PX_W + x] != C_BG;
                    MCHECK(lit > 3000);
                    if (shots && (ci == 1 || pg == 0)) { char fn[420]; snprintf(fn, sizeof fn, "%s/mon-m%d-load%d-cores%d-page%d.bmp", shots, mode, loads[li], corecounts[ci], pg); mon_dump_bmp(fn, cv); }
                }
                u.rain_full = 1;
                px_draw(cv, mode, &u);
                if (shots && li == 1 && ci == 1) { char fn[420]; snprintf(fn, sizeof fn, "%s/mon-m%d-rain-full.bmp", shots, mode); mon_dump_bmp(fn, cv); }
                memcpy(cv2, cv, sizeof cv);
                px_draw(cv, mode, &u);
                MCHECK(!memcmp(cv, cv2, sizeof cv));                                                   /* the rain draws the same bytes for the same input */
            }
    MCHECK(g_mon_reads == reads_before);                                                               /* drawing reads nothing from the host */
    /* an empty view (the Rec demo's, or the first frame): dashes and n/a, deterministic, no verdict colours on a number nobody read */
    memset(&u, 0, sizeof u);
    mon_init(&m, NULL);
    u.panel = PANEL_MONITOR; u.mon = m.v;
    for (int mode = 0; mode < 2; mode++) {
        for (int pg = 0; pg < mon_pages(mode, 0); pg++) { u.mon_page = pg; px_draw(cv, mode, &u); px_draw(cv2, mode, &u); MCHECK(!memcmp(cv, cv2, sizeof cv)); }
        u.mon_page = 99; px_draw(cv, mode, &u);                                                      /* a page past the end is clamped, not drawn from nowhere */
    }

    /* the keys, the TOOLS row, the cost rule through the app */
    memset(app->mon.card_path, 0, sizeof app->mon.card_path);
    int reads_app = (int)g_mon_reads;
    app->panel = PANEL_CPU; app->mode = MODE_STANDARD; app->rain_on = 0;
    fill_ui(app, ui, 0);
    MCHECK((int)g_mon_reads == reads_app && !ui->rain_on && ui->mon.ncores == 0);                      /* another panel: the monitor's data is not even copied */
    app->panel = PANEL_MONITOR; app->mon_page = 0;
    fill_ui(app, ui, 0); MCHECK(ui->mon.app.loaded == fm1ntt_loaded(app->m) && ui->mon.app.jit_pref == app->jit_pref && ui->mon.app.jit_avail == fm1ntt_jit_available());   /* the setting reaches the panel, loaded or not */
    act(app, A_PGDN); MCHECK(app->mon_page == 1 && app->mon.force);
    act(app, A_PGDN); MCHECK(app->mon_page == 2 || mon_pages(0, app->mon.v.nprocs) == 2);
    for (int i = 0; i < 5; i++) act(app, A_PGUP);
    MCHECK(app->mon_page >= 0 && app->mon_page < mon_pages(0, app->mon.v.nprocs));
    app->mon_page = 0;
    act(app, A_PGUP); MCHECK(app->mon_page == mon_pages(0, app->mon.v.nprocs) - 1);                      /* the pages wrap */
    int s0 = app->mon.v.sort;
    act(app, A_X); MCHECK(app->mon.v.sort == (s0 + 1) % MON_SORTS);
    act(app, A_Y); MCHECK(app->mon.paused && !app->mon.force);
    act(app, A_Y); MCHECK(!app->mon.paused);
    app->mode = MODE_ZOOMED; act(app, A_PGDN); app->mode = MODE_STANDARD;
    MCHECK(app->rain_on == 0);                                                                         /* off by default: nothing turns it on but its TOOLS row */
    app->panel = PANEL_TOOLS; app->tool_sel = 12;
    act(app, A_OK); MCHECK(app->rain_on == 1);
    fill_ui(app, ui, 0); MCHECK(strstr(ui->tools[12], "DATA RAIN: ON") != NULL && ui->ntools == NTOOLS);
    act(app, A_OK); MCHECK(app->rain_on == 0);
    fill_ui(app, ui, 0); MCHECK(strstr(ui->tools[12], "DATA RAIN: OFF") != NULL);
    app->reduced_motion = 1; app->rain.step = 0;
    act(app, A_OK); MCHECK(app->rain_on == 1 && app->rain.step == RAIN_ROWS);                           /* reduced motion: one still page, filled at once */
    app->panel = PANEL_MONITOR; app->last_input_ms = SDL_GetTicks() - RAIN_IDLE_MS - 1;
    fill_ui(app, ui, 0); MCHECK(ui->rain_on && ui->rain_full && ui->rain_still);
    app->last_input_ms = SDL_GetTicks();
    fill_ui(app, ui, 0); MCHECK(ui->rain_on && !ui->rain_full);                                         /* any input ends the whole-panel rain */
    app->rain_on = 0; app->reduced_motion = 0; app->panel = PANEL_MACHINE;
    /* the hot-plug history behind the DEVICE box */
    static Toast tt;
    memset(&tt, 0, sizeof tt);
    toast_add(&tt, "USB MIDI IN", 0, 100); toast_add(&tt, "PAD OUT", 0, 200); toast_add(&tt, "DISK ERROR", 1, 300); toast_add(&tt, "PAD IN", 0, 400);
    MCHECK(!strcmp(tt.hist[0], "PAD IN") && !strcmp(tt.hist[1], "! DISK ERROR") && !strcmp(tt.hist[2], "PAD OUT"));
#undef MCHECK
    return bad;
}

/* How fast is this CPU at the emulator? The self-test's real showcase runs, workers and demo playback take about a minute natively and many minutes under qemu-user (CI's arm64
 * package job, an x86_64 runner whose container says `uname -m` aarch64, so the architecture cannot tell): below 15 million emulated instructions a second (a Mac and native arm64 Linux manage 25 to 35 million; qemu-user a few) the self-test is light.
 * $FM1NTT_SELFTEST_LIGHT=1 forces light, $FM1NTT_SELFTEST_FULL=1 forces everything. */
static double emu_speed_ips(void) {
    fm1ntt_machine *m = fm1ntt_new();
    static uint8_t img[1 << 16];
    static int16_t buf[1024];
    size_t n = fm1ntt_bench_image(img, sizeof img);
    if (!m || !n || fm1ntt_load_raw(m, img, n, FM1_ENTRY_FELUCCA) != 0) { if (m) fm1ntt_free(m); return 1e12; }
    double t0 = mon_now_ms();
    while (mon_now_ms() - t0 < 300.0) fm1ntt_render(m, buf, 512, 2000000);
    double dt = mon_now_ms() - t0;
    Fm1Cpu c;
    double ips = fm1ntt_cpu(m, 0, &c) && dt > 0 ? (double)c.insns * 1000.0 / dt : 1e12;
    fm1ntt_free(m);
    return ips;
}
static int selftest_light(void) {
    static int v = -1;
    if (v < 0) {
        if (getenv("FM1NTT_SELFTEST_FULL")) v = 0;
        else if (getenv("FM1NTT_SELFTEST_LIGHT")) v = 1;
        else { double ips = emu_speed_ips(); v = ips < 15e6; fprintf(stderr, "  selftest: the emulator runs %.0f million instructions a second here: %s\n", ips / 1e6, v ? "light run" : "full run"); }
    }
    return v;
}

/* ---- the firmware showcase's tests (host/showcase.h) -------------------------------------------------------------------------------- */
static unsigned long long show_tree_hash(const char *dir) {                 /* FNV-1a over every file's name and bytes, in name order */
    DIR *d = opendir(dir);
    char names[600][24];
    int n = 0;
    struct dirent *e;
    while (d && (e = readdir(d)) && n < 600) if (e->d_name[0] != '.') snprintf(names[n++], sizeof names[0], "%s", e->d_name);
    if (d) closedir(d);
    for (int i = 1; i < n; i++) for (int j = i; j > 0 && strcmp(names[j], names[j - 1]) < 0; j--) { char t[24]; memcpy(t, names[j], 24); memcpy(names[j], names[j - 1], 24); memcpy(names[j - 1], t, 24); }
    unsigned long long h = 1469598103934665603ULL;
    for (int i = 0; i < n; i++) {
        for (const char *c = names[i]; *c; c++) h = (h ^ (unsigned char)*c) * 1099511628211ULL;
        char p[460]; size_t len = 0; char why[64];
        snprintf(p, sizeof p, "%s/%s", dir, names[i]);
        uint8_t *b = read_file(p, &len, why, sizeof why);
        for (size_t k = 0; b && k < len; k++) h = (h ^ b[k]) * 1099511628211ULL;
        free(b);
    }
    return h;
}
static int show_selftest(const char *root) {
    int fails = 0;
#define SCHECK(c) do { if (!(c)) { fprintf(stderr, "selftest FAIL (showcase): %s (line %d)\n", #c, __LINE__); fails++; } } while (0)
    /* the sidecar's rules */
    ShowState s;
    { char t[160]; snprintf(t, sizeof t, "state=ready\nreason=\ntour=%d\nframes=140\nms=15000\nattempts=1\njit=0\n", SHOW_TOUR); show_state_parse(t, &s); }
    SCHECK(s.have && !strcmp(s.state, "ready") && s.tour == SHOW_TOUR && s.frames == 140 && s.ms == 15000 && !show_needed(&s));
    { char t[160]; snprintf(t, sizeof t, "state=failed\nreason=the screen stayed blank\ntour=%d\nattempts=1\n", SHOW_TOUR); show_state_parse(t, &s); }
    SCHECK(s.have && !show_needed(&s) && !strcmp(s.reason, "the screen stayed blank"));         /* a failure is not retried */
    show_state_parse("state=failed\ntour=1\n", &s);   /* an older tour */
    SCHECK(show_needed(&s));                                                                      /* ... until the tour is a newer one */
    { char t[160]; snprintf(t, sizeof t, "state=recording\ntour=%d\nattempts=1\n", SHOW_TOUR); show_state_parse(t, &s); }
    SCHECK(show_needed(&s));                                                                      /* a run that was killed: once more */
    { char t[160]; snprintf(t, sizeof t, "state=recording\ntour=%d\nattempts=2\n", SHOW_TOUR); show_state_parse(t, &s); }
    SCHECK(!show_needed(&s));                                                                     /* ... and then it counts as failed */
    show_state_parse("", &s); SCHECK(!s.have && show_needed(&s));
    show_state_parse("state=banana\ntour=2\n", &s); SCHECK(!s.have && show_needed(&s));
    show_state_parse("garbage\n\xff\x01=\n=state\nstate", &s); SCHECK(!s.have);
    show_state_parse("state=ready\ntour=99999999999999999999\nframes=-4\n", &s); SCHECK(s.have && s.tour != SHOW_TOUR && show_needed(&s));
    char dir[420], base[400], res[240];
    /* the tour as data: the keys and the step the player shows are what the script does, tick by tick */
    { fm1ntt_machine *tm = fm1ntt_new();
      static uint8_t bimg[1 << 16];
      size_t bn = fm1ntt_bench_image(bimg, sizeof bimg);
      SCHECK(tm && bn > 0 && fm1ntt_load_raw(tm, bimg, bn, FM1_ENTRY_FELUCCA) == 0);          /* a machine that takes keys (the benchmark's own image) */
      int agree = 1, steps_seen = 0;
      const char *last = "";
      for (int t = 0; t < SHOW_TICKS && tm; t++) {
          show_script(tm, t);
          if ((fm1ntt_keys_down(tm) >> 14) != (show_keys_at(t) >> 14)) { if (agree) fprintf(stderr, "  tour keys differ at tick %d: machine %llx table %llx\n", t, (unsigned long long)fm1ntt_keys_down(tm), (unsigned long long)show_keys_at(t)); agree = 0; }
          const char *st = show_step(t);
          if (strcmp(st, last)) { steps_seen++; last = st; }
      }
      SCHECK(tm && agree);
      SCHECK(!strcmp(show_step(0), "BOOTING") && !strcmp(show_step(100), "NOTE 1") && !strcmp(show_step(230), "A CHORD") && !strcmp(show_step(305), "SELECT KNOB TURNED") && !strcmp(show_step(SHOW_TICKS - 1), "THE TAIL"));
      SCHECK(steps_seen >= 10 && show_keys_at(0) == 0 && show_keys_at(SHOW_TICKS - 1) == 0 && show_keys_at(250) != 0);
      if (tm) fm1ntt_free(tm); }
    snprintf(base, sizeof base, "%s/showbase", root);
    SCHECK(show_dir(dir, sizeof dir, base, "../../etc/passwd") != 0 && show_dir(dir, sizeof dir, base, "ABCDEF") != 0 && show_dir(dir, sizeof dir, base, NULL) != 0);
    char sha64[65];
    memset(sha64, 'a', 64); sha64[64] = 0;
    SCHECK(show_dir(dir, sizeof dir, base, sha64) == 0 && strstr(dir, "/demos/aaaa"));
    sha64[10] = 'G'; SCHECK(show_dir(dir, sizeof dir, base, sha64) != 0);
    /* the pictures: paletted and RGB both come back exactly; a cut or damaged file is refused, never read past its end */
    static uint32_t pic[FM1_SCREEN_W * FM1_SCREEN_H], back[FM1_SCREEN_W * FM1_SCREEN_H];
    char pp[460];
    snprintf(pp, sizeof pp, "%s/t.png", root);
    for (int colours = 5; colours <= 300; colours += 295) {
        for (int i = 0; i < FM1_SCREEN_W * FM1_SCREEN_H; i++) pic[i] = ((uint32_t)(i / 97 % colours) * 0x010203u + (uint32_t)(i / 97 % colours) * 7u) & 0xFFFFFF;
        SCHECK(show_png_write(pp, pic, FM1_SCREEN_W, FM1_SCREEN_H) == 0 && show_png_read(pp, back, FM1_SCREEN_W, FM1_SCREEN_H) == 0 && !memcmp(pic, back, sizeof pic));
        size_t len = 0; char why[64];
        uint8_t *png = read_file(pp, &len, why, sizeof why);
        SCHECK(png && ((colours <= 256) == (png[25] == 3)));                                          /* 5 colours: paletted; 300: RGB */
        int refused = 0, tried = 0;
        for (size_t cut = 0; png && cut < len; cut += len / 40 + 1) {
            FILE *f = fopen(pp, "wb"); fwrite(png, 1, cut, f); fclose(f);
            tried++; refused += show_png_read(pp, back, FM1_SCREEN_W, FM1_SCREEN_H) != 0;
        }
        SCHECK(tried > 10 && refused == tried);
        for (int k = 0; png && k < 200; k++) {                                                        /* a damaged byte: refused or still a picture, never a crash or an overread */
            uint8_t *dmg = malloc(len);
            memcpy(dmg, png, len);
            dmg[(size_t)(k * 2654435761u) % len] ^= (uint8_t)(1 + k);
            FILE *f = fopen(pp, "wb"); fwrite(dmg, 1, len, f); fclose(f); free(dmg);
            (void)show_png_read(pp, back, FM1_SCREEN_W, FM1_SCREEN_H);
        }
        free(png);
    }
    SCHECK(show_png_read("/nonexistent/x.png", back, FM1_SCREEN_W, FM1_SCREEN_H) != 0);
    { FILE *f = fopen(pp, "wb"); fputs("not a png at all, just text that is long enough to be read as a header", f); fclose(f); }
    SCHECK(show_png_read(pp, back, FM1_SCREEN_W, FM1_SCREEN_H) != 0);
    /* the table and the sound */
    snprintf(dir, sizeof dir, "%s/tbl", root);
    mkdir_p(dir);
    { char t[460]; snprintf(t, sizeof t, "%s/frames.txt", dir); const char *ok = "0000 100\n0001 400\nend 15000\n"; write_atomic(t, ok, strlen(ok)); }
    int ms[8], total = 0;
    SCHECK(show_frames_read(dir, ms, 8, &total) == 2 && ms[0] == 100 && ms[1] == 400 && total == 15000);
    const char *bad_tables[] = {"0001 100\nend 5\n", "0000 100\n", "end 5\n", "0000\nend 5\n", "0000 1\n0000 2\nend 3\n", "x\n", ""};
    for (unsigned i = 0; i < sizeof bad_tables / sizeof *bad_tables; i++) { char t[460]; snprintf(t, sizeof t, "%s/frames.txt", dir); write_atomic(t, bad_tables[i], strlen(bad_tables[i])); { int rcx = show_frames_read(dir, ms, 8, &total); if (rcx >= 0) fprintf(stderr, "  table %u read as %d\n", i, rcx); SCHECK(rcx < 0); } }
    { char t[460]; snprintf(t, sizeof t, "%s/frames.txt", dir); const char *two = "0000 1\n0001 2\nend 3\n"; write_atomic(t, two, strlen(two)); SCHECK(show_frames_read(dir, ms, 1, &total) < 0); }   /* more pictures than the table holds */
    { char t[460], junk[100]; snprintf(t, sizeof t, "%s/audio.wav", dir); memset(junk, 'x', sizeof junk); write_atomic(t, junk, sizeof junk); int16_t *pcm = NULL; size_t fr = 0; SCHECK(show_wav_read(t, &pcm, &fr) != 0 && !pcm); }
    show_clear(dir);
    SCHECK(access(dir, F_OK) != 0);
    snprintf(dir, sizeof dir, "%s/clr", root);                                                   /* only its own files go: a folder with anything else stays */
    mkdir_p(dir);
    { char t[460]; snprintf(t, sizeof t, "%s/keep.txt", dir); write_atomic(t, "mine", 4); snprintf(t, sizeof t, "%s/0001.png", dir); write_atomic(t, "x", 1); }
    show_clear(dir);
    { char t[460]; snprintf(t, sizeof t, "%s/keep.txt", dir); SCHECK(is_file(t)); snprintf(t, sizeof t, "%s/0001.png", dir); SCHECK(!is_file(t)); }
    /* a firmware that is not one: failed with a reason, and not made again; nothing to read: no folder */
    char junkfw[460];
    snprintf(junkfw, sizeof junkfw, "%s/not-firmware.fwsc", root);
    write_atomic(junkfw, "this is not a firmware package", 30);
    SCHECK(showcase_run(junkfw, 0, FM1_ENTRY_FELUCCA, 0, base, 0, res, sizeof res) == 1 && strstr(res, "failed: cannot load"));
    { char sh[65], d[460], b2[64]; (void)b2; size_t n = 0, w2[1]; (void)w2; char why[64]; uint8_t *bb = read_file(junkfw, &n, why, sizeof why); fm1ntt_sha256_hex(bb, n, sh); free(bb); show_dir(d, sizeof d, base, sh);
      ShowState st; show_state_read(d, &st); SCHECK(st.have && !strcmp(st.state, "failed") && strstr(st.reason, "cannot load") && !show_needed(&st)); }
    { struct sigaction cur; sigaction(SIGTERM, NULL, &cur); SCHECK(cur.sa_handler != show_on_term); sigaction(SIGINT, NULL, &cur); SCHECK(cur.sa_handler != show_on_term); }   /* a finished run puts the signals back: `timeout` still ends a process that goes on */
    SCHECK(showcase_run("/nonexistent/fw.fwsc", 0, 0, 0, base, 0, res, sizeof res) == 2);
    char empty[460]; snprintf(empty, sizeof empty, "%s/empty.fwsc", root); { FILE *f = fopen(empty, "wb"); if (f) fclose(f); }
    SCHECK(showcase_run(empty, 0, 0, 0, base, 0, res, sizeof res) == 2);
    if (selftest_light()) { fprintf(stderr, "  showcase: light run (a slow CPU): the real runs, the worker and the cancel are left to a fast one\n"); return fails; }
    char dflt[300];
    { char *sv = getenv("FM1NTT_BUNDLED") ? strdup(getenv("FM1NTT_BUNDLED")) : NULL; unsetenv("FM1NTT_BUNDLED"); default_firmware(dflt, sizeof dflt); if (sv) { setenv("FM1NTT_BUNDLED", sv, 1); free(sv); } }
    if (!dflt[0]) { fprintf(stderr, "  showcase: no bundled firmware here, the runs are skipped\n"); return fails; }
    /* the time budget ends a run as failed, and that is not retried */
    SCHECK(showcase_run(dflt, 0, 0, 0, base, 0.0001, res, sizeof res) == 1 && strstr(res, "time budget"));
    { char sh[65], d[460], why[64]; size_t n = 0; uint8_t *bb = read_file(dflt, &n, why, sizeof why); fm1ntt_sha256_hex(bb, n, sh); free(bb); show_dir(d, sizeof d, base, sh);
      ShowState st; show_state_read(d, &st); SCHECK(!strcmp(st.state, "failed") && !show_needed(&st)); }
    /* a cancelled run (SIGTERM to a child) leaves no half files and no state: it is needed again */
    { pid_t pid = fork();
      if (pid == 0) { char r2[200]; int rc = showcase_run(dflt, 0, 0, 0, base, 0, r2, sizeof r2); _exit(rc); }
      usleep(1500000);
      kill(pid, SIGTERM);
      int stt = 0; waitpid(pid, &stt, 0);
      SCHECK(WIFEXITED(stt) && WEXITSTATUS(stt) == 3);
      char sh[65], d[460], why[64]; size_t n = 0; uint8_t *bb = read_file(dflt, &n, why, sizeof why); fm1ntt_sha256_hex(bb, n, sh); free(bb); show_dir(d, sizeof d, base, sh);
      ShowState st; show_state_read(d, &st);
      SCHECK(access(d, F_OK) != 0 && !st.have && show_needed(&st)); }
    /* two real runs: the same bytes (pictures, table, sound, state), and what they wrote reads back */
    char a_base[420], b_base[420], da[460], db[460];
    snprintf(a_base, sizeof a_base, "%s/runa", root); snprintf(b_base, sizeof b_base, "%s/runb", root);
    { int rcx = showcase_run(dflt, 0, 0, 0, a_base, 0, res, sizeof res); if (rcx != 0 || !strstr(res, "ready")) fprintf(stderr, "  showcase run said (%d): %s\n", rcx, res); SCHECK(rcx == 0 && strstr(res, "ready")); }
    SCHECK(showcase_run(dflt, 0, 0, 0, b_base, 0, res, sizeof res) == 0);
    { char sh[65], why[64]; size_t n = 0; uint8_t *bb = read_file(dflt, &n, why, sizeof why); fm1ntt_sha256_hex(bb, n, sh); free(bb); show_dir(da, sizeof da, a_base, sh); show_dir(db, sizeof db, b_base, sh); }
    SCHECK(show_tree_hash(da) == show_tree_hash(db));
    ShowState st;
    show_state_read(da, &st);
    { char pf[480]; int16_t *pcm = NULL; size_t fr = 0; snprintf(pf, sizeof pf, "%s/audio.wav", da);
      SCHECK(show_wav_read(pf, &pcm, &fr) == 0);
      int gaps = 0, run = 0, started = 0;                                                        /* silence in the middle of the sound: a buffer the core had to pad */
      for (size_t i = (size_t)3 * FM1_SAMPLE_RATE; pcm && i < fr; i++) { if (pcm[i * 2] || pcm[i * 2 + 1]) { if (started && run >= 40) gaps++; started = 1; run = 0; } else run++; }
      SCHECK(started && gaps == 0); free(pcm); }
    SCHECK(!strcmp(st.state, "ready") && st.shorts == 0 && st.frames > 20 && st.frames <= SHOW_FRAMES_MAX && st.ms == 15000 && st.tour == SHOW_TOUR && !show_needed(&st));
    static int fms[SHOW_FRAMES_MAX]; int ftotal = 0;
    int nf = show_frames_read(da, fms, SHOW_FRAMES_MAX, &ftotal);
    SCHECK(nf == st.frames && ftotal == 15000);
    int rising = 1;
    for (int i = 1; i < nf; i++) rising &= fms[i] - fms[i - 1] >= SHOW_MIN_GAP * 1000 / SHOW_FPS;
    SCHECK(rising);                                                                                  /* at most 10 pictures a second */
    for (int i = 0; i < nf && i < 3; i++) { char pf[480]; snprintf(pf, sizeof pf, "%s/%04d.png", da, i); SCHECK(show_png_read(pf, back, FM1_SCREEN_W, FM1_SCREEN_H) == 0); }
    { char pf[480]; int16_t *pcm = NULL; size_t fr = 0; snprintf(pf, sizeof pf, "%s/audio.wav", da); SCHECK(show_wav_read(pf, &pcm, &fr) == 0 && fr == (size_t)SHOW_TICKS * SHOW_AUDIO);
      int loud = 0; for (size_t i = 0; pcm && i < fr * 2; i++) loud |= abs(pcm[i]) > 200; SCHECK(loud); free(pcm);                  /* the notes were heard */
      snprintf(pf, sizeof pf, "%s/progress.txt", da); SCHECK(!is_file(pf)); }
    show_state_read(da, &st);
    /* a changed tour version makes the same folder needed again (a state written by an older tour) */
    st.tour = SHOW_TOUR - 1; show_state_write(da, &st); show_state_read(da, &st); SCHECK(show_needed(&st));
#undef SCHECK
    return fails;
}

/* ---- the showcase inside the app: the page, the policy, the worker, the queue, the player (needs the app `a` and its machine) ----------------------------------------------- */
static int show_app_selftest(const char *root, App *a, UiState *ui) {
    int fails = 0;
#define ACHECK(c) do { if (!(c)) { fprintf(stderr, "selftest FAIL (showcase app): %s (line %d)\n", #c, __LINE__); fails++; } } while (0)
    (void)root;
    char dflt[300];
    { char *sv = getenv("FM1NTT_BUNDLED") ? strdup(getenv("FM1NTT_BUNDLED")) : NULL; unsetenv("FM1NTT_BUNDLED"); default_firmware(dflt, sizeof dflt); bundled_dir(a->bdir, sizeof a->bdir); if (sv) { setenv("FM1NTT_BUNDLED", sv, 1); free(sv); } }
    if (!dflt[0] || !g_self_exe[0]) { fprintf(stderr, "  showcase app: no bundled firmware or no program path, skipped\n"); return fails; }
    memset(a->fsha, 0, sizeof a->fsha);
    a->nfiles = 2;
    snprintf(a->fpaths[0], sizeof a->fpaths[0], "%s", dflt); snprintf(a->files[0], PX_NAME, "%s", strrchr(dflt, '/') + 1); a->fbundled[0] = 1;
    char own[460]; snprintf(own, sizeof own, "%s/own-firmware.fwsc", root); write_atomic(own, "own bytes of no manifest", 24);
    snprintf(a->fpaths[1], sizeof a->fpaths[1], "%s", own); snprintf(a->files[1], PX_NAME, "own-firmware.fwsc"); a->fbundled[1] = 0;
    a->panel = PANEL_FILES; a->mode = MODE_STANDARD; a->info_on = 0; a->file_sel = 0;
    char d0[420], d1[420];
    ACHECK(demo_dir_of(a, 0, d0, sizeof d0) == 0 && demo_dir_of(a, 1, d1, sizeof d1) == 0);
    show_clear(d0); show_clear(d1);
    /* the page: a bundled firmware knows its licence and source from the manifest, a file you brought says so */
    act(a, A_OK);
    ACHECK(a->info_on && a->info_i == 0 && a->info_rec.found);
    fill_ui(a, ui, 0);
    ACHECK(ui->info_on && !strcmp(ui->info_tier, "BUNDLED (PERMISSIVE)") && strstr(ui->info_licence, "MPL-2.0") && strstr(ui->info_source, "github.com") && strlen(ui->info_sha) == 64 && !ui->info_can_play && ui->info_demo_kind == 0 && strstr(ui->info_demo, "NONE YET") && !ui->demo_run[0]);
    act(a, A_BACK);
    a->file_sel = 1; act(a, A_OK); fill_ui(a, ui, 0);
    ACHECK(ui->info_on && !strcmp(ui->info_tier, "YOUR OWN FILE") && strstr(ui->info_licence, "NOT KNOWN") && !strcmp(ui->info_source, "-"));
    act(a, A_X); ACHECK(!a->pl.on && a->msg_style == 2);                                        /* no demo: X says so */
    act(a, A_BACK);
    /* the policy: not while you play, measure, record, or on a low battery */
    a->last_input_ms = 1000; a->sw_low = 0; a->sw_batt_ms = 99999;
    ACHECK(show_may_run(a, 100000.0) == 1);
    a->last_input_ms = 99000;
    int was_paused = a->paused; a->paused = 0;
    ACHECK(fm1ntt_loaded(a->m) && show_may_run(a, 100000.0) == 0);                                 /* a running firmware and a key a second ago */
    a->paused = 1; ACHECK(show_may_run(a, 100000.0) == 1); a->paused = was_paused;
    a->last_input_ms = 1000;
    a->job = (SDL_Thread *)(uintptr_t)1; ACHECK(show_may_run(a, 100000.0) == 0); a->job = NULL;
    a->sw_low = 1; ACHECK(show_may_run(a, 100000.0) == 0); a->sw_low = 0;
    if (selftest_light()) goto draw_tests;                                         /* an emulated CPU: no real worker, no real demo to play */
    /* the queue starts nothing while it is off, and no more than one worker; a worker is cancelled and leaves nothing */
    a->bg_demos = 0; a->sw_next_ms = 0; a->sw_cursor = 0; a->paused = 1;
    show_queue(a, 100000.0); ACHECK(a->sw_pid == 0);
    a->bg_demos = 1; a->sw_next_ms = 0; a->sw_cursor = 0;
    show_queue(a, 100000.0);                                                                       /* file 0: no demo, so a worker */
    ACHECK(a->sw_pid > 0);
    pid_t first = a->sw_pid;
    show_queue(a, 200000.0); ACHECK(a->sw_pid == first);
    fill_ui(a, ui, 0);
    ACHECK(ui->demo_run[0] && !ui->demo_paused);
    a->info_on = 1; a->info_i = 0; fill_ui(a, ui, 0);
    ACHECK(ui->info_busy && ui->info_demo_kind == 1 && strstr(ui->info_demo, "BEING MADE"));
    a->job = (SDL_Thread *)(uintptr_t)1;                                                           /* a report starts: the worker is stopped */
    ACHECK(show_poll(a, 100500.0) == 1 && a->sw_stopped == 1);
    { int st = 0; usleep(200000); pid_t w = waitpid(a->sw_pid, &st, WUNTRACED | WNOHANG); ACHECK(w == a->sw_pid && WIFSTOPPED(st)); }
    fill_ui(a, ui, 0); ACHECK(ui->demo_paused && strstr(ui->info_demo, "PAUSED"));
    a->job = NULL;                                                                                 /* ... and goes on when it ends */
    ACHECK(show_poll(a, 101000.0) == 1 && a->sw_stopped == 0);
    act(a, A_Y); ACHECK(a->sw_cancelling == 1);                                                    /* Y on its page stops it */
    for (int k = 0; k < 200 && a->sw_pid; k++) { show_poll(a, 102000.0 + k * 100.0); usleep(100000); }
    ACHECK(a->sw_pid == 0 && access(d0, F_OK) != 0 && strstr(a->msg, "stopped"));
    ShowState s;
    show_state_read(d0, &s); ACHECK(!s.have && show_needed(&s));
    /* a whole run through the queue: ready, and not made again */
    a->sw_next_ms = 0; a->sw_cursor = 0;
    show_queue(a, 300000.0); ACHECK(a->sw_pid > 0);
    for (int k = 0; k < 1200 && a->sw_pid; k++) { show_poll(a, 300000.0 + k * 100.0); usleep(100000); }
    show_state_read(d0, &s);
    if (a->sw_pid || strcmp(s.state, "ready")) fprintf(stderr, "  worker: pid %d, state '%s', reason '%s', msg '%s'\n", (int)a->sw_pid, s.state, s.reason, a->msg);
    ACHECK(a->sw_pid == 0 && !strcmp(s.state, "ready") && s.frames > 20 && strstr(a->msg, "ready"));
    a->sw_next_ms = 0; a->sw_cursor = 0;
    show_queue(a, 500000.0); ACHECK(a->sw_pid == 0);                                               /* a demo exists: no worker */
    a->sw_next_ms = 0; a->sw_cursor = 1;
    show_queue(a, 500000.0); ACHECK(a->sw_pid > 0);                                                /* the other file has none */
    show_cancel(a);
    for (int k = 0; k < 200 && a->sw_pid; k++) { show_poll(a, 500000.0 + k * 100.0); usleep(100000); }
    ACHECK(a->sw_pid == 0);
    /* a worker that dies without a word is a failure, written once, and not tried in a loop */
    { char gone[460]; snprintf(gone, sizeof gone, "%s/gone.fwsc", root); write_atomic(gone, "x", 1);
      snprintf(a->fpaths[1], sizeof a->fpaths[1], "%s", gone); memset(a->fsha[1], 0, 65); a->sw_cursor = 1; a->sw_next_ms = 0;
      char saved[1024]; snprintf(saved, sizeof saved, "%s", g_self_exe);
      snprintf(g_self_exe, sizeof g_self_exe, "/usr/bin/false");                                   /* a worker that exits 1 with no state of its own */
      show_queue(a, 600000.0); ACHECK(a->sw_pid > 0);
      for (int k = 0; k < 100 && a->sw_pid; k++) { show_poll(a, 600000.0 + k * 100.0); usleep(50000); }
      snprintf(g_self_exe, sizeof g_self_exe, "%s", saved);
      demo_dir_of(a, 1, d1, sizeof d1); show_state_read(d1, &s);
      ACHECK(a->sw_pid == 0 && !strcmp(s.state, "failed") && strstr(s.reason, "could not run") && !show_needed(&s));
      a->sw_cursor = 1; a->sw_next_ms = 0; show_queue(a, 700000.0); ACHECK(a->sw_pid == 0); }
    a->info_on = 1; a->info_i = 1; fill_ui(a, ui, 0);
    ACHECK(ui->info_demo_kind == 3 && strstr(ui->info_demo, "FAILED: the worker could not run"));
    /* the player: the demo's sound and pictures, a press ends it, the end ends it, and the machine's own screen is back */
    info_open(a, 0);
    act(a, A_X);
    ACHECK(a->pl.on && a->pl.nf > 20 && a->pl.frames == (size_t)SHOW_TICKS * SHOW_AUDIO);
    int16_t buf[1470 * 2];
    audio_step(a, buf, 1470);
    ACHECK(a->pl.pos == 1470);
    for (int k = 1; k < 100; k++) audio_step(a, buf, 1470);                                         /* 3.3 s in: the first notes */
    fill_ui(a, ui, 0);
    ACHECK(ui->playing && ui->fb_valid && !strcmp(ui->run_state, "DEMO"));
    ACHECK(ui->pl_ticks == SHOW_TICKS && ui->pl_tick == (int)((unsigned long long)a->pl.pos * SHOW_FPS / FM1_SAMPLE_RATE) && ui->pl_nf == a->pl.nf && !strcmp(ui->pl_name, a->files[0]) && !strcmp(ui->pl_tier, "BUNDLED (PERMISSIVE)") && strstr(ui->pl_licence, "MPL-2.0") && !strcmp(ui->pl_step, show_step(ui->pl_tick)) && ui->pl_keys == show_keys_at(ui->pl_tick));
    { static uint32_t cvp[PX_W * PX_H], cvq[PX_W * PX_H];
      for (int md = 0; md < 2; md++) {
          a->mode = md;                                                                         /* whatever layout the app was in: a demo plays at 2x, flush top left, with its own column */
          px_draw(cvp, md, ui); px_draw(cvq, md, ui);
          int okp = !memcmp(cvp, cvq, sizeof cvp);
          for (int y = 0; y < 480 && okp; y++) for (int x = 0; x < 480; x++) if (cvp[y * PX_W + x] != (0xFF000000u | ui->fb[(y / 2) * FM_W + x / 2])) { okp = 0; break; }
          ACHECK(okp);
          long lit = 0, below = 0;
          for (int y = 0; y < PX_H; y++) for (int x = 480; x < PX_W; x++) lit += cvp[y * PX_W + x] != C_BG;
          ACHECK(lit > 3000);
          for (int y = 472; y < PX_H; y++) for (int x = 480; x < PX_W; x++) below += cvp[y * PX_W + x] != C_BG;
          ACHECK(below < 400);                                                                  /* the last line is the one-line hint, nothing runs past it */
      }
      a->mode = MODE_STANDARD;
      if (getenv("FM1NTT_MON_SHOTS")) { char fn[420]; snprintf(fn, sizeof fn, "%s/player.bmp", getenv("FM1NTT_MON_SHOTS")); mon_dump_bmp(fn, cvp); } }
    { static uint32_t want[FM_W * FM_H]; char pf[480]; int t = (int)((unsigned long long)a->pl.pos * 1000 / FM1_SAMPLE_RATE), idx = 0; for (int k = 0; k < a->pl.nf; k++) if (a->pl.ms[k] <= t) idx = k;
      snprintf(pf, sizeof pf, "%s/%04d.png", d0, idx); ACHECK(show_png_read(pf, want, FM_W, FM_H) == 0 && !memcmp(want, ui->fb, sizeof want)); }
    { int loud = 0; for (int k = 0; k < 40; k++) { audio_step(a, buf, 1470); for (int i = 0; i < 1470 * 2; i++) loud |= abs(buf[i]) > 200; } ACHECK(loud); }
    SDL_Event ev;
    memset(&ev, 0, sizeof ev);
    ev.type = SDL_KEYDOWN; ev.key.keysym.sym = SDLK_z;
    unsigned long long keys_before = fm1ntt_keys_down(a->m);
    dispatch(a, &ev);
    ACHECK(!a->pl.on && fm1ntt_keys_down(a->m) == keys_before);                                    /* a press ends it and is used up: no note */
    show_poll(a, 800000.0); ACHECK(!a->pl.pcm);
    act(a, A_X); ACHECK(a->pl.on);
    for (int k = 0; k < SHOW_TICKS + 5 && a->pl.on; k++) audio_step(a, buf, 1470);
    ACHECK(!a->pl.on && a->pl.pos == a->pl.frames);                                                 /* played to its end */
    show_poll(a, 800100.0); ACHECK(!a->pl.pcm);
    fill_ui(a, ui, 0); ACHECK(!ui->playing && strcmp(ui->run_state, "DEMO") != 0);
draw_tests:;
    /* the page and the progress box drawn, both layouts: the FM-1's screen intact, the same pixels every time, above the foot */
    static uint32_t cv[PX_W * PX_H], cv2[PX_W * PX_H];
    const char *shots = getenv("FM1NTT_MON_SHOTS");
    for (int mode = 0; mode < 2; mode++) {
        for (int st = 0; st < 5; st++) {
            fill_ui(a, ui, 0);
            ui->info_on = st < 4;
            ui->info_demo_kind = st == 0 ? 0 : st == 1 ? 1 : st == 2 ? 2 : 3; ui->info_can_play = st == 2; ui->info_busy = st == 1; ui->info_pct = 45;
            snprintf(ui->info_demo, sizeof ui->info_demo, "%s", st == 0 ? "NONE YET: Y MAKES IT" : st == 1 ? "BEING MADE: 45%, 12S" : st == 2 ? "READY: 140 PICTURES, 15 S" : "FAILED: the screen stayed blank, so there is nothing to show");
            if (st == 4) { snprintf(ui->demo_run, sizeof ui->demo_run, "untrue-0.1.fwsc"); ui->demo_pct = 45; ui->demo_secs = 12; }
            px_draw(cv, mode, ui); px_draw(cv2, mode, ui);
            ACHECK(!memcmp(cv, cv2, sizeof cv));
            int s2 = px_scale(mode), ok = 1;
            for (int y = 0; y < FM_H * s2 && ok; y++) for (int x = 0; x < FM_W * s2; x++) if (cv[y * PX_W + x] != (0xFF000000u | ui->fb[(y / s2) * FM_W + x / s2])) { ok = 0; break; }
            ACHECK(ok);
            PxRect cr = px_content_rect(mode);
            long lit = 0, foot = 0;
            for (int y = cr.y; y < cr.y + cr.h - 30; y++) for (int x = cr.x; x < cr.x + cr.w; x++) lit += cv[y * PX_W + x] != C_BG;
            for (int y = cr.y + cr.h - 12; y < cr.y + cr.h; y++) for (int x = cr.x + cr.w - 4; x < cr.x + cr.w; x++) foot += cv[y * PX_W + x] != C_BG;
            ACHECK(lit > 2000 && foot == 0);
            if (shots) { char fn[420]; snprintf(fn, sizeof fn, "%s/files-m%d-state%d.bmp", shots, mode, st); mon_dump_bmp(fn, cv); }
        }
    }
    /* the TOOLS row */
    a->panel = PANEL_TOOLS; a->tool_sel = 13; a->bg_demos = 0;
    act(a, A_OK); ACHECK(a->bg_demos == 1); fill_ui(a, ui, 0); ACHECK(strstr(ui->tools[13], "BACKGROUND DEMOS: ON"));
    act(a, A_OK); ACHECK(a->bg_demos == 0); fill_ui(a, ui, 0); ACHECK(strstr(ui->tools[13], "BACKGROUND DEMOS: OFF"));
    a->panel = PANEL_MACHINE; a->info_on = 0; a->paused = was_paused; a->nfiles = 0;
#undef ACHECK
    return fails;
}

/* ---- where the app's folder is (base_dir, root_pick, data_card_ok) ----------------------------------------------------------------------------------------------------------- */
static int root_selftest(const char *root, App *a, UiState *ui) {
    int fails = 0;
#define RCHECK(c) do { if (!(c)) { fprintf(stderr, "selftest FAIL (root): %s (line %d)\n", #c, __LINE__); fails++; } } while (0)
    char why[64], out[300], dc[400], rm[400], eng[460];
    snprintf(dc, sizeof dc, "%s/rt/roms2", root); snprintf(rm, sizeof rm, "%s/rt/roms", root);
    mkdir_p(rm);
    char *sv_pre = getenv("R3I6NOS_APP_DATA") ? strdup(getenv("R3I6NOS_APP_DATA")) : NULL, *sv_eng = getenv("R3I6NOS_APPDATA") ? strdup(getenv("R3I6NOS_APPDATA")) : NULL, *sv_home = getenv("HOME") ? strdup(getenv("HOME")) : NULL;
    unsetenv("R3I6NOS_APP_DATA");
    setenv("R3I6NOS_APPDATA", "/nonexistent/r3i6nos-appdata", 1);                                    /* no engine */
    int sv_need = g_root_need_mount;
    /* the data card's own rules */
    RCHECK(!data_card_ok(dc, 0, why, sizeof why) && !strcmp(why, "NO DATA CARD"));                      /* nothing there */
    mkdir_p(dc);
    RCHECK(data_card_ok(dc, 0, why, sizeof why));                                                     /* a writable folder with room */
    RCHECK(!data_card_ok(dc, 1, why, sizeof why) && strstr(why, "NOT A SEPARATE CARD"));              /* ... but a folder on the system card's own filesystem is no card */
    { char probe[460]; snprintf(probe, sizeof probe, "%s/.fm1ntt-probe", dc); RCHECK(!is_file(probe)); }  /* the probe is removed */
    if (geteuid() != 0) { chmod(dc, 0555); RCHECK(!data_card_ok(dc, 0, why, sizeof why) && !strcmp(why, "NOT WRITABLE")); chmod(dc, 0755); }
    RCHECK(!data_card_ok("/nonexistent/card", 1, why, sizeof why));
    /* the order: a data card (our own check), else the system card, else home */
    g_root_need_mount = 0;
    root_pick(dc, rm, out, sizeof out, why, sizeof why);
    RCHECK(!strncmp(out, dc, strlen(dc)) && !strcmp(out + strlen(dc), "/FM1-NTT") && strstr(why, "DATA CARD"));
    root_pick("/nonexistent/roms2", rm, out, sizeof out, why, sizeof why);
    RCHECK(!strncmp(out, rm, strlen(rm)) && !strcmp(out + strlen(rm), "/FM1-NTT") && strstr(why, "SYSTEM CARD") && !strstr(why, "DATA CARD"));   /* no card in: nothing said about one */
    g_root_need_mount = 1;
    root_pick(dc, rm, out, sizeof out, why, sizeof why);
    RCHECK(!strncmp(out, rm, strlen(rm)) && strstr(why, "SYSTEM CARD: DATA CARD NOT A SEPARATE CARD"));      /* a card that is not valid is not used, and the reason is kept */
    g_root_need_mount = 0;
    setenv("HOME", "/home/test", 1);
    root_pick("/nonexistent/roms2", "/nonexistent/roms", out, sizeof out, why, sizeof why);
    RCHECK(!strcmp(out, "/home/test/FM1-NTT") && !strcmp(why, "HOME FOLDER"));
    unsetenv("HOME");
    root_pick("/nonexistent/roms2", "/nonexistent/roms", out, sizeof out, why, sizeof why);
    RCHECK(!strcmp(out, "FM1-NTT") && !strcmp(why, "CURRENT FOLDER"));
    if (sv_home) setenv("HOME", sv_home, 1);
    /* r3i6nos decided: its variable wins over everything else; its engine comes before our own check */
    char want[460]; snprintf(want, sizeof want, "%s/rt/decided/FM1-NTT", root);
    setenv("R3I6NOS_APP_DATA", want, 1);
    root_pick(dc, rm, out, sizeof out, why, sizeof why);
    RCHECK(!strcmp(out, want) && !strcmp(why, "SET BY R3I6NOS"));
    unsetenv("R3I6NOS_APP_DATA");
    char dcard[460]; snprintf(dcard, sizeof dcard, "%s/rt/roms2/FM1-NTT", root);
    snprintf(eng, sizeof eng, "%s/rt/engine-good.sh", root);
    { char sh[600]; snprintf(sh, sizeof sh, "#!/bin/sh\n[ \"$1\" = path ] && [ \"$2\" = FM1-NTT ] && [ \"$3\" = --create ] || exit 3\nmkdir -p '%s'\necho '%s'\n", dcard, dcard); write_atomic(eng, sh, strlen(sh)); chmod(eng, 0755); }
    setenv("R3I6NOS_APPDATA", eng, 1);
    root_pick("/nonexistent/roms2", rm, out, sizeof out, why, sizeof why);
    RCHECK(!strcmp(out, dcard) && is_dir(dcard) && !strcmp(why, "R3I6NOS: SYSTEM CARD"));            /* the engine's word, even where we would have said otherwise */
    root_pick(dc, rm, out, sizeof out, why, sizeof why);
    RCHECK(!strcmp(out, dcard) && !strcmp(why, "R3I6NOS: DATA CARD"));                                /* under the data card's mount: said so */
    const char *bad_engines[] = {"echo relative/path", "echo /roms/../etc", "echo /nonexistent/folder/FM1-NTT", "echo", "echo /x; exit 1", "exit 1"};
    for (unsigned i = 0; i < sizeof bad_engines / sizeof *bad_engines; i++) {
        char sh[300];
        snprintf(eng, sizeof eng, "%s/rt/engine-bad%u.sh", root, i);
        snprintf(sh, sizeof sh, "#!/bin/sh\n%s\n", bad_engines[i]);
        write_atomic(eng, sh, strlen(sh)); chmod(eng, 0755);
        setenv("R3I6NOS_APPDATA", eng, 1);
        root_pick(dc, rm, out, sizeof out, why, sizeof why);
        RCHECK(!strncmp(out, dc, strlen(dc)) && strstr(why, "DATA CARD ("));                          /* an engine that fails or answers oddly: our own check decides */
    }
    setenv("R3I6NOS_APPDATA", "/tmp/it's-a-quote", 1);                                                 /* a name with a quote is not run */
    root_pick("/nonexistent/roms2", rm, out, sizeof out, why, sizeof why);
    RCHECK(!strncmp(out, rm, strlen(rm)));
    /* the override is the person's, never remembered; the screens say where and why */
    const char *sv_dir = getenv("FM1NTT_DIR");
    char rr[300], rw[64], bd[300];
    base_dir(bd, sizeof bd); RCHECK(sv_dir && !strcmp(bd, sv_dir));
    root_describe(rr, sizeof rr, rw, sizeof rw); RCHECK(!strcmp(rr, sv_dir) && !strcmp(rw, "SET BY FM1NTT_DIR"));
    a->panel = PANEL_TOOLS; fill_ui(a, ui, 0);
    RCHECK(!strcmp(ui->app_root, sv_dir) && !strcmp(ui->app_root_why, "SET BY FM1NTT_DIR"));
    a->panel = PANEL_MONITOR; fill_ui(a, ui, 0);
    RCHECK(!strcmp(ui->mon.app.root, sv_dir) && !strcmp(ui->mon.app.root_why, "SET BY FM1NTT_DIR"));
    a->panel = PANEL_MACHINE;
    /* the screens, with a long path: nothing runs past the box */
    static UiState u;
    static uint32_t cv[PX_W * PX_H];
    for (int mode = 0; mode < 2; mode++) for (int pn = 0; pn < 2; pn++) {
        memset(&u, 0, sizeof u);
        u.panel = pn ? PANEL_MONITOR : PANEL_TOOLS; u.ntools = 14; u.run_state = "RUN";
        snprintf(u.app_root, sizeof u.app_root, "/roms2/FM1-NTT/with/a/very/long/path/that/goes/on/and/on/FM1-NTT");
        snprintf(u.app_root_why, sizeof u.app_root_why, "R3I6NOS: DATA CARD");
        u.mon.platform = MON_PLAT_LINUX;
        snprintf(u.mon.app.root, sizeof u.mon.app.root, "%s", u.app_root); snprintf(u.mon.app.root_why, sizeof u.mon.app.root_why, "%s", u.app_root_why);
        px_draw(cv, mode, &u);
        PxRect cr = px_content_rect(mode);
        long foot = 0;
        for (int y = cr.y + cr.h - 12; y < cr.y + cr.h; y++) for (int x = cr.x + cr.w - 4; x < cr.x + cr.w; x++) foot += cv[y * PX_W + x] != C_BG;
        RCHECK(foot == 0);
        if (px_panel_monitor(cv, cr, &u, mode) > cr.y + cr.h - 26) RCHECK(0);
    }
    g_root_need_mount = sv_need;
    if (sv_pre) { setenv("R3I6NOS_APP_DATA", sv_pre, 1); free(sv_pre); }
    if (sv_eng) { setenv("R3I6NOS_APPDATA", sv_eng, 1); free(sv_eng); } else unsetenv("R3I6NOS_APPDATA");
    free(sv_home);
#undef RCHECK
    return fails;
}

/* The emulation thread: it fills the ring ahead of the callback, the callback takes from it without waiting, a pause empties it and silences the callback, a machine
 * too slow to fill it costs a counted gap and not a stall, and the UI's lock() still reaches the machine. Needs a firmware: the bundled one given, "" skips. */
static int emu_selftest(const char *fw) {
    int fails = 0;
#define ECHECK(c) do { if (!(c)) { fprintf(stderr, "selftest FAIL (emulation thread): %s (line %d)\n", #c, __LINE__); fails++; } } while (0)
    if (!fw[0]) { fprintf(stderr, "  emulation thread: no bundled firmware here, skipped\n"); return 0; }
    App *t = calloc(1, sizeof *t);
    if (!t) return 1;
    t->m = fm1ntt_new();
    char why[64]; size_t n = 0;
    uint8_t *img = read_file(fw, &n, why, sizeof why);
    ECHECK(t->m && img && fm1ntt_load_package(t->m, img, n) == 0);
    free(img);
    ECHECK(emu_start(t) == 0 && t->emu && t->mx);
    int16_t out[AUDIO_FRAMES * 2];
    int light = selftest_light();                      /* an emulated CPU cannot fill the ring in a test's time: it checks that the callback never waits, and leaves the filling to a fast one */
    if (!light) {
        for (int i = 0; i < 400 && !t->emu_primed; i++) SDL_Delay(5);
        ECHECK(t->emu_primed && (unsigned)(SDL_AtomicGet(&t->ring_w) - SDL_AtomicGet(&t->ring_r)) >= EMU_TARGET);   /* it filled the ring on its own */
        unsigned r0 = (unsigned)SDL_AtomicGet(&t->ring_r);
        audio_step(t, out, AUDIO_FRAMES);
        ECHECK((unsigned)SDL_AtomicGet(&t->ring_r) - r0 == AUDIO_FRAMES && SDL_AtomicGet(&t->late_cb) == 0);        /* a buffer taken, not a gap */
        for (int i = 0; i < 400 && (unsigned)(SDL_AtomicGet(&t->ring_w) - SDL_AtomicGet(&t->ring_r)) < EMU_TARGET; i++) SDL_Delay(5);
        ECHECK((unsigned)(SDL_AtomicGet(&t->ring_w) - SDL_AtomicGet(&t->ring_r)) >= EMU_TARGET);                   /* and it refilled after the take */
    }
    lock(t); ECHECK(fm1ntt_loaded(t->m)); unlock(t);                                                            /* the UI's way in */
    /* a callback that comes faster than the thread can fill: it never waits, and the missing frames are silence */
    { Uint32 t0 = SDL_GetTicks(); for (int i = 0; i < 40; i++) audio_step(t, out, AUDIO_FRAMES); ECHECK(SDL_GetTicks() - t0 < 1000); }
    /* pause: the callback is silent and the ring is dropped, so nothing stale plays on resume */
    t->paused = 1;
    for (int i = 0; i < 3; i++) SDL_Delay(30);
    for (int i = 0; i < AUDIO_FRAMES * 2; i++) out[i] = 5;
    audio_step(t, out, AUDIO_FRAMES);
    int nz = 0; for (int i = 0; i < AUDIO_FRAMES * 2; i++) nz += out[i] != 0;
    ECHECK(nz == 0 && SDL_AtomicGet(&t->ring_r) == SDL_AtomicGet(&t->ring_w));
    t->paused = 0;
    if (!light) { for (int i = 0; i < 400 && !t->emu_primed; i++) SDL_Delay(5); ECHECK(t->emu_primed); }        /* it runs again */
    emu_stop(t);
    ECHECK(!t->emu && !t->mx);
    fm1ntt_free(t->m);
    free(t);
#undef ECHECK
    return fails;
}

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
            u.nfiles = 2; snprintf(u.files[0], PX_NAME, "  A.FWSC"); snprintf(u.files[1], PX_NAME, "  B.BIN");
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
        dispatch(&p, &l1); CHECK(p.panel == PANEL_MEMORY && p.menu_kind == MK_NONE);        /* L1 alone does nothing */
        dispatch(&p, &r1); CHECK(p.menu_kind == MK_MAIN && p.panel == PANEL_MEMORY);        /* both: the main menu, the panel stays under it */
        dispatch(&p, &r1); CHECK(p.menu_kind == MK_NONE && p.panel == PANEL_MEMORY);        /* both again: closed */
        l1.type = r1.type = fn.type = SDL_JOYBUTTONUP; dispatch(&p, &l1); dispatch(&p, &r1); dispatch(&p, &fn); CHECK(p.pad_mask == 0);
        fn.type = SDL_JOYBUTTONDOWN;
        l1.type = r1.type = SDL_JOYBUTTONDOWN;
    }
    /* rule 17, the input mapping (docs/r3i6nos-mapping.md): the main menu's six fixed items, the chords, R1 held as "increase", L2 and R2, L3 and R3 */
    {
        App p;
        memset(&p, 0, sizeof p);
        SDL_Event ev = {.type = SDL_JOYBUTTONDOWN};
#define PD(btn) do { ev.type = SDL_JOYBUTTONDOWN; ev.jbutton.button = (btn); dispatch(&p, &ev); } while (0)
#define PU(btn) do { ev.type = SDL_JOYBUTTONUP; ev.jbutton.button = (btn); dispatch(&p, &ev); } while (0)
#define TAP(btn) do { PD(btn); PU(btn); } while (0)
        UiState *u = calloc(1, sizeof *u);
        CHECK(u != NULL);
        if (!u) return 1;
        /* theme.txt and keymap.txt go to a folder of their own: a self-test never touches the person's real ones */
        char kdir[300];
        snprintf(kdir, sizeof kdir, "%s/fm1ntt-keymap-XXXXXX", getenv("TMPDIR") && *getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp");
        char *kd = mkdtemp(kdir), *sv_dir = getenv("FM1NTT_DIR") ? strdup(getenv("FM1NTT_DIR")) : NULL;
        CHECK(kd != NULL);
        if (kd) setenv("FM1NTT_DIR", kd, 1);
        /* the carousel and its history: Fn next; R1 + Fn the panel before (again: back); L1 + Fn the first panel (MACHINE keeps its next-scale) */
        p.panel = PANEL_CPU;
        TAP(PAD_FN); CHECK(p.panel == PANEL_MEMORY);
        PD(PAD_R1); TAP(PAD_FN); CHECK(p.panel == PANEL_CPU); TAP(PAD_FN); CHECK(p.panel == PANEL_MEMORY); PU(PAD_R1);
        PD(PAD_L1); TAP(PAD_FN); CHECK(p.panel == PANEL_MACHINE); PU(PAD_L1);
        PD(PAD_R1); TAP(PAD_FN); CHECK(p.panel == PANEL_MEMORY); PU(PAD_R1);                /* the one before MACHINE was MEMORY */
        memset(&p, 0, sizeof p); p.panel = PANEL_CPU;
        PD(PAD_R1); TAP(PAD_FN); CHECK(p.panel == PANEL_CPU && !strncmp(p.msg, "No earlier", 10)); PU(PAD_R1);   /* no history yet: says so */
        p.panel = PANEL_MACHINE; p.scale = 0;
        PD(PAD_L1); TAP(PAD_FN); CHECK(p.panel == PANEL_MACHINE && p.scale == 0); PU(PAD_L1);   /* L1 + Fn is the first panel everywhere: there is no scale binding on it any more */
        /* the main menu: L1 + R1; the six fixed items in this order and then the app's own; Quit leaves; B and a second L1 + R1 close it */
        p.panel = PANEL_CPU;
        PD(PAD_L1); PD(PAD_R1); CHECK(p.menu_kind == MK_MAIN); PU(PAD_R1); PU(PAD_L1);
        fill_menu(&p, u);
        CHECK(u->menu_n == MM_COUNT && MM_COUNT == 8 && !strcmp(u->menu_label[0], "ABOUT") && !strcmp(u->menu_label[1], "COLOUR THEME") && !strcmp(u->menu_label[2], "KEYMAP") && !strcmp(u->menu_label[3], "DEVICE REPORT") &&
              !strcmp(u->menu_label[4], "REC DEMO") && !strcmp(u->menu_label[5], "BENCHMARK") && !strcmp(u->menu_label[6], "QUIT") && !strcmp(u->menu_label[7], "TOOLS"));
        CHECK(u->menu_kind == 1);                                                           /* drawn as a list with a chosen row */
        CHECK(u->menu_value[MM_THEME][0] != 0);
        TAP(1 /* A */); CHECK(p.menu_kind == MK_ABOUT);                                     /* About: A on the first item; any key closes it */
        fill_menu(&p, u); CHECK(u->menu_n == ABOUT_LINES && !strcmp(u->menu_label[0], "FM1-NTT") && u->menu_label[1][0] == 'V');
        TAP(5 /* R1 */); CHECK(p.menu_kind == MK_NONE);
        PD(PAD_L1); PD(PAD_R1); PU(PAD_R1); PU(PAD_L1); TAP(0 /* B */); CHECK(p.menu_kind == MK_NONE && !p.quit);
        PD(PAD_L1); PD(PAD_R1); PU(PAD_R1); PU(PAD_L1); for (int i = 0; i < MM_QUIT; i++) TAP(9 /* down */); CHECK(p.menu_sel == MM_QUIT); TAP(1); CHECK(p.quit == 1 && p.menu_kind == MK_NONE); p.quit = 0;
        PD(PAD_L1); PD(PAD_R1); PU(PAD_R1); PU(PAD_L1); TAP(8 /* up wraps */); CHECK(p.menu_sel == MM_COUNT - 1); TAP(1); CHECK(p.panel == PANEL_TOOLS);   /* Tools: the app's own item */
        /* R1 held in a menu: Page down goes to the last row, Home the first */
        PD(PAD_L1); PD(PAD_R1); PU(PAD_R1); PU(PAD_L1); PD(PAD_R1); TAP(9); CHECK(p.menu_sel == MM_COUNT - 1); TAP(10); CHECK(p.menu_sel == 0); PU(PAD_R1); TAP(0);
        /* the colour theme: moving shows each theme, B puts back the one in use, A keeps it; every theme passes the 4.5:1 text pairs */
        px_theme_apply(0);
        PD(PAD_L1); PD(PAD_R1); PU(PAD_R1); PU(PAD_L1); TAP(9); TAP(1); CHECK(p.menu_kind == MK_THEME && p.menu_sel == 0);
        TAP(9); CHECK(px_theme_now == 1 && px_pal[PXC_BG] == px_themes[1].c[PXC_BG]); TAP(0); CHECK(p.menu_kind == MK_NONE && px_theme_now == 0 && C_BG == px_themes[0].c[PXC_BG]);
        for (int t = 0; t < PX_THEME_COUNT; t++) {
            px_theme_apply(t);
            const uint32_t fgs[] = {C_TEXT, C_DIM, C_HEAD, C_OK, C_WARN, C_ERR};
            for (unsigned i = 0; i < sizeof fgs / sizeof *fgs; i++) { CHECK(px_contrast(fgs[i], C_PANEL) >= 4.5); CHECK(px_contrast(fgs[i], C_BG) >= 4.5); }
            CHECK(px_contrast(C_TEXT, C_SEL) >= 4.5 && px_contrast(C_BG, C_KEYDN) >= 4.5 && px_contrast(C_KEYUP, C_BLACKKEY) >= 4.5);
            CHECK(px_contrast(C_OK, C_FRAME) >= 3.0 && px_contrast(C_WARN, C_FRAME) >= 3.0 && px_contrast(C_ERR, C_FRAME) >= 3.0);
            CHECK(px_theme_find(px_themes[t].name) == t);
        }
        px_theme_apply(0);
        CHECK(px_theme_find("NOPE") == -1 && px_theme_find(NULL) == -1);
        /* the context menu: L2 + R2, the panel's own items and a screenshot; nothing pressed before the chord acts; B closes; L1 or R1 too is reserved */
        p.panel = PANEL_MIDI; p.midi_chan = 5;
        PD(PAD_L2); PD(PAD_R2); CHECK(p.menu_kind == MK_CTX && p.midi_chan == 5); PU(PAD_R2); PU(PAD_L2); CHECK(p.midi_chan == 5);
        fill_menu(&p, u); CHECK(u->menu_n == 2 && !strcmp(u->menu_title, "MIDI") && !strcmp(u->menu_label[1], "SAVE A SCREENSHOT"));
        TAP(0); CHECK(p.menu_kind == MK_NONE);
        PD(PAD_L1); PD(PAD_L2); PD(PAD_R2); CHECK(p.menu_kind == MK_NONE); PU(PAD_R2); PU(PAD_L2); PU(PAD_L1);
        TAP(PAD_L2); TAP(PAD_R2); CHECK(p.midi_chan == 5);                                  /* L2 and R2 are the system's: a panel has no use for them */
        TAP(PAD_SELECT); CHECK(p.midi_chan == 4); TAP(PAD_START); TAP(PAD_START); CHECK(p.midi_chan == 6 && !p.paused);   /* MIDI's own: the channel (Start does not pause here) */
        { const char *cl[PX_MENU_MAX]; int cc[PX_MENU_MAX]; for (int pn = 0; pn < PANEL_COUNT; pn++) { p.panel = pn; int n = ctx_items(&p, cl, cc); CHECK(n >= 1 && n <= PX_MENU_MAX && !strcmp(cl[n - 1], "SAVE A SCREENSHOT")); } }
        /* R1 held is "increase": the d-pad pages and jumps; the pad's L2 and R2 are not keys on press */
        CHECK(act_of_pad_r1(9, 1) == A_PGDN && act_of_pad_r1(8, 1) == A_PGUP && act_of_pad_r1(10, 1) == A_HOME && act_of_pad_r1(11, 1) == A_END);
        CHECK(act_of_pad_r1(9, 0) == A_DOWN && act_of_pad_r1(8, 0) == A_UP && act_of_pad_r1(10, 0) == A_LEFT && act_of_pad_r1(11, 0) == A_RIGHT);
        CHECK(act_of_pad_r1(PAD_L2, 0) == A_NONE && act_of_pad_r1(PAD_R2, 1) == A_NONE && act_of_pad_r1(1, 1) == A_OK && act_of_pad_r1(0, 1) == A_BACK);
        p.panel = PANEL_TOOLS; p.tool_sel = 3;
        PD(PAD_R1); TAP(9); CHECK(p.tool_sel == NTOOLS - 1); TAP(8); CHECK(p.tool_sel == 0); TAP(11); CHECK(p.tool_sel == NTOOLS - 1); TAP(10); CHECK(p.tool_sel == 0); PU(PAD_R1);
        TAP(9); CHECK(p.tool_sel == 1);
        /* L3 and R3: the app has no virtual mouse or keyboard and says so */
        p.panel = PANEL_CPU; TAP(PAD_L3); CHECK(strstr(p.msg, "virtual mouse") != NULL); TAP(PAD_R3); CHECK(strstr(p.msg, "virtual keyboard") != NULL);
        /* the keyboard: ` the main menu, \ the context menu, Esc closes a menu before it would quit; F10 stays the FM-1's button */
        SDL_Event kb = {.type = SDL_KEYDOWN};
        kb.key.keysym.sym = SDLK_BACKQUOTE; dispatch(&p, &kb); CHECK(p.menu_kind == MK_MAIN);
        kb.key.keysym.sym = SDLK_ESCAPE; dispatch(&p, &kb); CHECK(p.menu_kind == MK_NONE && !p.quit);
        kb.key.keysym.sym = SDLK_BACKSLASH; dispatch(&p, &kb); CHECK(p.menu_kind == MK_CTX);
        kb.key.keysym.sym = SDLK_BACKSLASH; dispatch(&p, &kb); CHECK(p.menu_kind == MK_NONE);
        CHECK(!is_menu_key(&kb) && button_of_key(SDLK_F10) == 10);
        /* the right stick steps a list 6 a second at most; Page up and down with R1; Home or End once for a push; nothing inside the dead zone */
        PmStick st;
        memset(&st, 0, sizeof st);
        int out[16], total = 0;
        CHECK(pm_stick_tick(&st, 0, 0.3f, 0, 1000, out, 16) == 0 && pm_stick_tick(&st, 0, -0.3f, 0, 1100, out, 16) == 0);
        total = pm_stick_tick(&st, 0, 1.0f, 0, 1200, out, 16); CHECK(total == 1 && out[0] == A_DOWN);   /* the first step of a push comes at once */
        for (int t = 1; t <= 10; t++) total += pm_stick_tick(&st, 0, 1.0f, 0, 1200 + 100 * t, out, 16);
        CHECK(total >= 6 && total <= 7);                                                                 /* about 6 a second */
        CHECK(pm_stick_tick(&st, 0, 0, 0, 2300, out, 16) == 0);
        CHECK(pm_stick_tick(&st, 0, -1.0f, 1, 2400, out, 16) == 1 && out[0] == A_PGUP);
        CHECK(pm_stick_tick(&st, 0, 0, 1, 2500, out, 16) == 0 && pm_stick_tick(&st, 1.0f, 0, 1, 2600, out, 16) == 1 && out[0] == A_END);
        int more = 0; for (int t = 1; t <= 10; t++) more += pm_stick_tick(&st, 1.0f, 0, 1, 2600 + 100 * t, out, 16); CHECK(more == 0);   /* held: End is not repeated */
        CHECK(pm_stick_tick(&st, 0, 0, 1, 3700, out, 16) == 0 && pm_stick_tick(&st, -1.0f, 0, 1, 3800, out, 16) == 1 && out[0] == A_HOME);
        CHECK(pm_stick_rate(0.35f) == 0 && pm_stick_rate(1.0f) == PM_STICK_RATE && pm_stick_rate(-1.0f) == PM_STICK_RATE);
        { int rows = 20, per = 6; CHECK(pm_list_step(0, rows, per, A_UP) == 19 && pm_list_step(19, rows, per, A_DOWN) == 0 && pm_list_step(15, rows, per, A_PGDN) == 19 && pm_list_step(3, rows, per, A_PGUP) == 0 &&
                                        pm_list_step(7, rows, per, A_HOME) == 0 && pm_list_step(7, rows, per, A_END) == 19 && pm_list_step(7, rows, per, A_LEFT) == 7 && pm_list_step(7, 0, per, A_DOWN) == 0); }
        /* the right stick drives a menu and a list through sticks_tick, and MACHINE's knobs keep their 12 a second */
        memset(&p, 0, sizeof p); p.panel = PANEL_TOOLS; p.axis[3] = 1.0f;
        sticks_tick(&p, 5000); for (int i = 1; i <= 10; i++) sticks_tick(&p, 5000 + 100 * i); CHECK(p.tool_sel >= 6 && p.tool_sel <= 7);
        memset(&p, 0, sizeof p); p.panel = PANEL_MACHINE; menu_open_main(&p); p.axis[3] = 1.0f; sticks_tick(&p, 6000); CHECK(p.menu_sel == 1);
        /* Esc closes a menu and never quits: leaving is the main menu's Quit */
        memset(&p, 0, sizeof p); p.panel = PANEL_CPU;
        kb.key.keysym.sym = SDLK_ESCAPE; dispatch(&p, &kb); CHECK(!p.quit && strstr(p.msg, "Quit is in the main menu") != NULL);
        /* Start pauses where a panel has no use for it; L2 and R2 are the system's and do nothing on a panel */
        TAP(PAD_START); CHECK(p.paused == 1); TAP(PAD_START); CHECK(p.paused == 0); TAP(PAD_L2); TAP(PAD_R2); CHECK(p.paused == 0 && p.menu_kind == MK_NONE);
        /* MACHINE: R1 held shifts the octave with left and right (the OCT buttons) and transposes with up and down; the d-pad plays notes without it */
        memset(&p, 0, sizeof p); p.panel = PANEL_MACHINE; p.m = fm1ntt_new();
        PD(PAD_R1); PD(10); CHECK(p.pad_sent[10] == 0 + 1 && p.key_shift == 0); PU(10); CHECK(p.pad_sent[10] == 0);
        PD(11); CHECK(p.pad_sent[11] == 1 + 1); PU(11);
        PD(8); CHECK(p.key_shift == 1 && p.pad_sent[8] == 0); PU(8); PD(9); PD(9); CHECK(p.key_shift == -1); PU(9);
        PU(PAD_R1); PD(8); CHECK(p.pad_sent[8] == FM1_KEY_BASE + 6 + 1); PU(8);   /* the key is one lower: two down transposes */
        fm1ntt_free(p.m); p.m = NULL;
        /* the Keymap: a main menu item, pages (system, each panel, MACHINE's held sets, the test), remap with conflict checking, swaps, reset, saving */
        keymap_reset_all();
        { KmPage pg[KM_MAXP]; int np = km_pages(pg), sys = 0, mach = 0;
          for (int i = 0; i < np; i++) { sys += pg[i].ctx == KM_SYS; mach += pg[i].ctx == PANEL_MACHINE; }
          CHECK(np <= KM_MAXP && pg[0].ctx == KM_SYS && pg[np - 1].ctx == KM_TEST && sys >= 2 && mach == 5);   /* MACHINE: the base page and L1, L2, R2, R1 */
          for (int i = 0; i < np; i++) CHECK(pg[i].ctx != PANEL_CONSOLE); }
        memset(&p, 0, sizeof p); p.panel = PANEL_TOOLS;
        menu_open_main(&p); p.menu_sel = MM_KEYMAP; menu_press(&p, A_OK);
        CHECK(p.menu_kind == MK_KEYMAP && p.km_state == 0 && p.km_panel == PANEL_TOOLS);
        fill_menu(&p, u); CHECK(u->menu_kind == 1 && u->menu_n == NSLOT && strstr(u->menu_title, "TOOLS") != NULL && !strcmp(u->menu_label[SL_A], "RUN") && !strcmp(u->menu_value[SL_A], "A"));
        for (int i = 0; i < SL_A; i++) TAP(9);                                              /* down to the RUN row */
        CHECK(p.km_sel == SL_A);
        TAP(1); CHECK(p.km_state == 1);                                                     /* A: waits for the new button */
        PD(PAD_L1); CHECK(p.km_state == 1 && strstr(p.km_note, "L1") != NULL && g_bind[PANEL_TOOLS][SL_A] == 1); PU(PAD_L1);   /* a system button is refused and says why */
        fill_menu(&p, u); CHECK(strstr(u->menu_foot, "L1") != NULL);
        PD(PAD_START); PU(PAD_START); CHECK(p.km_state == 2 && p.km_other == SL_START);      /* Start has PAUSE here: a conflict, asked */
        fill_menu(&p, u); CHECK(strstr(u->menu_foot, "PAUSE") != NULL && strstr(u->menu_foot, "START") != NULL);
        TAP(0); CHECK(p.km_state == 0 && g_bind[PANEL_TOOLS][SL_A] == 1 && g_bind[PANEL_TOOLS][SL_START] == 13);   /* B cancels: nothing changed */
        TAP(1); PD(PAD_START); PU(PAD_START); TAP(1);                                       /* again, then A swaps */
        CHECK(p.km_state == 0 && g_bind[PANEL_TOOLS][SL_A] == 13 && g_bind[PANEL_TOOLS][SL_START] == 1);
        { int seen = 0; for (int i = 0; i < NSLOT; i++) seen |= 1 << g_bind[PANEL_TOOLS][i]; CHECK(seen == (1 << 0 | 1 << 1 | 1 << 2 | 1 << 3 | 1 << 8 | 1 << 9 | 1 << 10 | 1 << 11 | 1 << 12 | 1 << 13)); }   /* still a permutation of the ten buttons */
        CHECK(pad_action(PANEL_TOOLS, 13, 0) == A_OK && pad_action(PANEL_TOOLS, 1, 0) == A_START && pad_action(PANEL_CPU, 1, 0) == A_OK && act_of_pad(1) == A_OK);   /* per panel; the menus keep the defaults */
        TAP(9); CHECK(p.km_sel == SL_A + 1);
        TAP(1); PD(PAD_R1 - 2 /* L1 */); PU(PAD_R1 - 2); TAP(2 /* X: held by SL_X, which has no label on TOOLS */); CHECK(p.km_state == 0 && g_bind[PANEL_TOOLS][SL_X] == g_bind[PANEL_TOOLS][SL_X] && g_bind[PANEL_TOOLS][SL_A + 1] == 2);   /* a button used for nothing here is swapped without asking */
        TAP(2 /* X */);
        TAP(0);                                                                             /* B closes the Keymap */
        CHECK(p.menu_kind == MK_NONE);
        /* the new binding is what the panel answers to: RUN is on Select... wait for the pad on TOOLS */
        p.panel = PANEL_TOOLS; p.tool_sel = 2; p.msg[0] = 0; TAP(13); CHECK(p.msg[0] != 0);   /* the RUN tool (panic) answers to the button RUN was moved to */
        /* saved to keymap.txt, parsed back only when sound, and reverted by row, page and all */
        keymap_save();
        { int keep[NSLOT]; memcpy(keep, g_bind[PANEL_TOOLS], sizeof keep); keymap_reset_all(); CHECK(g_bind[PANEL_TOOLS][SL_A] == 1); keymap_load(); CHECK(!memcmp(keep, g_bind[PANEL_TOOLS], sizeof keep) && !memcmp(g_bind[PANEL_CPU], DEF_BTN, sizeof DEF_BTN)); }
        CHECK(keymap_parse_line("MIDI 8 10 9 11 3 0 1 2 12 13") == PANEL_MIDI);
        CHECK(keymap_parse_line("MIDI 8 8 9 11 3 0 1 2 12 13") == -1 && keymap_parse_line("MIDI 4 10 9 11 3 0 1 2 12 13") == -1 && keymap_parse_line("MIDI 8 10 9 11 3 0 1 2 12") == -1 &&
              keymap_parse_line("NOPE 8 10 9 11 3 0 1 2 12 13") == -1 && keymap_parse_line("") == -1 && keymap_parse_line("MIDI 8 10 9 11 3 0 1 2 12 99") == -1);
        menu_open_main(&p); p.menu_sel = MM_KEYMAP; menu_press(&p, A_OK); p.km_sel = SL_A; TAP(2 /* X: this row back */);
        CHECK(g_bind[PANEL_TOOLS][SL_A] == 1 && strstr(p.km_note, "BACK") != NULL);
        km_set(&p, PANEL_TOOLS, SL_Y, 12); TAP(3 /* Y: this page back */); CHECK(!memcmp(g_bind[PANEL_TOOLS], DEF_BTN, sizeof DEF_BTN));
        km_set(&p, PANEL_CPU, SL_Y, 12); km_set(&p, PANEL_MIDI, SL_A, 12);
        TAP(PAD_START); CHECK(p.km_state == 3); TAP(0); CHECK(p.km_state == 0 && g_bind[PANEL_CPU][SL_Y] == 12);   /* Start asks; B says no */
        TAP(PAD_START); TAP(1); CHECK(!memcmp(g_bind[PANEL_CPU], DEF_BTN, sizeof DEF_BTN) && !memcmp(g_bind[PANEL_MIDI], DEF_BTN, sizeof DEF_BTN));
        /* the pages turn with left and right (R1 + right the last: the test page, where every press is shown and nothing is done, and L1 + R1 leaves) */
        int pg0 = p.km_page; TAP(10); CHECK(p.km_page == pg0 - 1); TAP(11); CHECK(p.km_page == pg0);
        PD(PAD_R1); TAP(11); PU(PAD_R1);
        { KmPage pg[KM_MAXP]; int np = km_pages(pg); CHECK(p.km_page == np - 1); }
        fill_menu(&p, u); CHECK(u->menu_kind == 2 && strstr(u->menu_title, "TEST") != NULL);
        p.paused = 0; TAP(PAD_START); TAP(0); TAP(1);
        CHECK(p.menu_kind == MK_KEYMAP && !p.paused && p.km_nhist == 3);
        fill_menu(&p, u); CHECK(!strncmp(u->menu_label[1], "START", 5) && !strncmp(u->menu_label[2], "B", 1) && strstr(u->menu_label[3], "A") != NULL);
        PD(PAD_L1); fill_menu(&p, u); CHECK(strstr(u->menu_label[6], "L1") != NULL); PD(PAD_R1); CHECK(p.menu_kind == MK_NONE); PU(PAD_R1); PU(PAD_L1);   /* the chord leaves */
        keymap_reset_all(); keymap_save();
        if (kd) {
            char f[340];
            snprintf(f, sizeof f, "%s/keymap.txt", kd); remove(f);
            snprintf(f, sizeof f, "%s/theme.txt", kd); remove(f);
            rmdir(kd);
        }
        if (sv_dir) { setenv("FM1NTT_DIR", sv_dir, 1); free(sv_dir); } else unsetenv("FM1NTT_DIR");
        free(u);
#undef PD
#undef PU
#undef TAP
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
        /* L2: SCL FX HOME SAVE ENV LFO ARP SEQ; R2: GLO EDIT PLAY REC; L1: GLO on X, OP1-OP6 and PIT have no button */
        CHECK(button_of_pad_mod(MOD_L2, 10) == 2 && button_of_pad_mod(MOD_L2, 9) == 8 && button_of_pad_mod(MOD_L2, 11) == 9 && button_of_pad_mod(MOD_L2, 3) == 4 &&
              button_of_pad_mod(MOD_L2, 0) == 5 && button_of_pad_mod(MOD_L2, 1) == 10 && button_of_pad_mod(MOD_L2, 2) == 11 && button_of_pad_mod(MOD_L2, 8) == 3);
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
        ev.type = SDL_JOYBUTTONDOWN; ev.jbutton.button = PAD_FN; dispatch(&p, &ev);       /* L1 + Fn: the first panel; no longer the scale */
        CHECK(p.scale == 0 && p.panel == PANEL_MACHINE);
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
    a.load_sync = 1;                                         /* the tests below press A and look at once; the job-thread load has its own block */
    g_quiet_logs = 1;
    a.m = fm1ntt_new();
    mon_init(&a.mon, NULL);
    a.midi_note = 60; a.midi_vel = 100; a.slot = 1;
    /* the sound device sleeps after quiet seconds when nothing can make a sound, and wakes for anything that can */
    a.last_input_ms = 1000;
    CHECK(audio_wanted(&a, 1000 + AUDIO_QUIET_MS - 1));                       /* touched a moment ago */
    CHECK(!audio_wanted(&a, 1000 + AUDIO_QUIET_MS));                          /* no firmware, nothing playing, quiet */
    a.pv_on = 1; CHECK(audio_wanted(&a, 99000)); a.pv_on = 0;
    a.pl.on = 1; CHECK(audio_wanted(&a, 99000)); a.pl.on = 0;
    a.rec_on = 1; CHECK(audio_wanted(&a, 99000)); a.rec_on = 0;
    a.sw_pid = 1; CHECK(audio_wanted(&a, 99000)); a.sw_pid = 0;
    a.want_demo = 1; CHECK(audio_wanted(&a, 99000)); a.want_demo = 0;
    CHECK(!audio_wanted(&a, 99000));
    /* the firmware folder: the right extensions, sorted, a garbage file refused with a reason, a raw file booted */
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
    CHECK(a.info_on && a.info_i == 0 && !fm1ntt_loaded(a.m));                  /* A opens the firmware's page first: nothing is loaded yet */
    act(&a, A_BACK); CHECK(!a.info_on && !fm1ntt_loaded(a.m));                 /* B closes it */
    act(&a, A_OK); act(&a, A_OK);                                              /* the page, then Load */
    CHECK(!a.info_on && !fm1ntt_loaded(a.m) && a.msg_style == 3 && a.msg[0]);  /* A.FWSC is garbage: refused with a reason */
    a.file_sel = 2; act(&a, A_OK); act(&a, A_OK);
    CHECK(!fm1ntt_loaded(a.m) && a.msg_style == 3);                            /* empty file: refused */
    a.file_sel = 1; act(&a, A_OK); act(&a, A_OK);
    CHECK(fm1ntt_loaded(a.m) && !strcmp(a.file, "b.bin") && a.msg_style == 1); /* the .bin boots as a raw image */
    act(&a, A_UP); CHECK(a.file_sel == 0);
    act(&a, A_UP); CHECK(a.file_sel == 2);                                     /* wraps to the last file: Quit is a TOOLS row now */
    act(&a, A_DOWN); CHECK(a.file_sel == 0);
    menu_open_main(&a); a.menu_sel = MM_QUIT; menu_press(&a, A_OK); CHECK(a.quit == 1 && a.menu_kind == MK_NONE); a.quit = 0; a.panel = PANEL_FILES;   /* Quit is the main menu's, and only its */
    /* the CPU view and the memory view */
    UiState *ui = calloc(1, sizeof *ui);
    a.panel = PANEL_CPU; a.mode = MODE_STANDARD;
    fill_ui(&a, ui, 0);
    CHECK(ui->ncpu >= 1 && (!strcmp(ui->run_state, "RUN") || !strcmp(ui->run_state, "HALT")));
    CHECK(ui->nfiles == 3 && ui->files[1][0] == '*' && ui->ntools == NTOOLS && strstr(ui->tools[NTOOLS - 1], "REMEMBER") != NULL);   /* Quit is not a TOOLS row: leaving is the main menu's */
    a.cpu_sel = 0; act(&a, A_LEFT); CHECK(a.cpu_sel == 1); act(&a, A_UP); CHECK(a.cpu_sel == 0);
    a.panel = PANEL_MEMORY; a.mem_region = 1;
    fill_ui(&a, ui, 0);
    CHECK(ui->mem_n == 256 && ui->mem[0] == 0xAA && ui->mem_base == 0x02000000u);
    act(&a, A_UP); CHECK(a.mem_off == 0);                                      /* clamped at the start */
    act(&a, A_DOWN); CHECK(a.mem_off == 8);
    act(&a, A_PGDN); CHECK(a.mem_off == 264);
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
    act(&a, A_START); CHECK(a.midi_chan == 1);
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
    {   /* a load on a job thread: the progress box shows while it runs, the result lands on the main thread, a bad file says why and changes nothing */
        char fwp[300]; default_firmware(fwp, sizeof fwp);
        if (fwp[0]) {
            a.file[0] = 0; a.load_sync = 0;
            load_start(&a, fwp, 0, 0);
            CHECK(a.job != NULL && a.job_kind == JOB_LOAD || a.file[0]);                       /* (a very fast thread may be done already) */
            for (int i = 0; i < 4000 && (a.job || !a.file[0]); i++) { if (!job_poll(&a)) SDL_Delay(5); }
            CHECK(!a.job && a.job_kind == JOB_NONE && a.file[0] && strlen(a.fl_sha) == 64 && fm1ntt_loaded(a.m) && a.msg_style == 1);
            char keep[sizeof a.file]; snprintf(keep, sizeof keep, "%s", a.file);
            load_start(&a, "/nonexistent/x.fwsc", 0, 0);
            for (int i = 0; i < 4000 && a.job; i++) { if (!job_poll(&a)) SDL_Delay(5); }
            CHECK(!a.job && a.msg_style == 3 && !strcmp(a.file, keep) && fm1ntt_loaded(a.m));
            a.load_sync = 1;
        }
    }
    {   /* remembered settings: what a firmware saved into its flash comes back, per firmware, until a reset */
        char fwp[300]; default_firmware(fwp, sizeof fwp);
        if (fwp[0]) {
            size_t fn = fm1ntt_flash_size(); uint8_t *img = malloc(fn), *back = malloc(fn);
            a.remember = 1;
            CHECK(load_firmware(&a, fwp, 0, 0) == 0 && a.fl_sha[0] && strlen(a.fl_sha) == 64);
            char pp[360]; persist_path(&a, pp, sizeof pp);
            unlink(pp);
            persist_save(&a, 1, 100000); CHECK(!is_file(pp));                      /* nothing changed: nothing written */
            fm1ntt_flash_save(a.m, img, fn); memset(img + fn - 4096, 0x5A, 4096);   /* the firmware saves a setting into the last sector */
            CHECK(fm1ntt_flash_restore(a.m, img, fn) == 0);
            persist_save(&a, 0, 100000); CHECK(!is_file(pp) && a.fl_dirty);           /* changed a moment ago: waits for it to be still */
            persist_save(&a, 0, 102500); CHECK(is_file(pp) && !a.fl_dirty);
            struct stat ps; CHECK(stat(pp, &ps) == 0 && (size_t)ps.st_size == fn);
            char pt[380]; snprintf(pt, sizeof pt, "%s.part", pp); CHECK(!is_file(pt));
            a.remember = 0; fm1ntt_flash_save(a.m, back, fn); memset(back + fn - 4096, 0x33, 4096); fm1ntt_flash_restore(a.m, back, fn);
            persist_save(&a, 1, 200000); CHECK(stat(pp, &ps) == 0);                   /* off: nothing is written (the file is as it was) */
            a.remember = 1;
            CHECK(load_firmware(&a, fwp, 0, 0) == 0);                                 /* the next load puts the saved settings back */
            fm1ntt_flash_save(a.m, back, fn); CHECK(back[fn - 1] == 0x5A && back[fn - 4096] == 0x5A);
            CHECK(a.msg_style == 1 && !fm1ntt_halted(a.m));
            do_reset(&a); CHECK(!is_file(pp));                                        /* a reset forgets them */
            fm1ntt_flash_save(a.m, back, fn); CHECK(back[fn - 1] != 0x5A);
            a.remember = 0; CHECK(load_firmware(&a, fwp, 0, 0) == 0); fm1ntt_flash_save(a.m, back, fn); CHECK(back[fn - 1] != 0x5A);
            free(img); free(back);
        }
    }
    a.tool_sel = 3; act(&a, A_OK); CHECK(fm1ntt_jit_on(a.m) == fm1ntt_jit_available()); act(&a, A_OK); CHECK(fm1ntt_jit_on(a.m) == 0);
    a.tool_sel = 11; act(&a, A_OK); CHECK(a.pace_mode == 1 && fm1ntt_pace_level(a.m) == 1);
    act(&a, A_OK); act(&a, A_OK); CHECK(a.pace_mode == 3 && fm1ntt_pace_level(a.m) == 3);
    act(&a, A_OK); CHECK(a.pace_mode == PACE_AUTO && fm1ntt_pace_level(a.m) == 0);   /* AUTO starts exact */
    a.speed = 0.5; pace_auto(&a); pace_auto(&a); CHECK(a.pace_lvl == 2 && fm1ntt_pace_level(a.m) == 2);   /* behind: up a level a second */
    a.speed = 1.5; pace_auto(&a); CHECK(a.pace_lvl == 2);                                                 /* keeping up: stays */
    a.speed = 5.0; pace_auto(&a); pace_auto(&a); pace_auto(&a); CHECK(a.pace_lvl == 0 && fm1ntt_pace_level(a.m) == 0);   /* plenty to spare: back to exact */
    /* too slow to fill a buffer in time: the firmware's sound is muted (with a hysteresis) and the message says so; faster again, it is back */
    a.speed = 0.3; slow_mute_update(&a); CHECK(a.fw_mute == 1 && a.msg_style == 2 && strstr(a.msg, "muted"));
    a.speed = 0.6; slow_mute_update(&a); CHECK(a.fw_mute == 1);                                            /* between the two limits: unchanged */
    { int16_t mb[1024]; for (int i = 0; i < 1024; i++) mb[i] = 7; audio_step(&a, mb, 512); int nz = 0; for (int i = 0; i < 1024; i++) nz += mb[i] != 0; CHECK(nz == 0); }
    a.speed = 0.9; slow_mute_update(&a); CHECK(a.fw_mute == 0 && a.msg_style == 0);
    a.speed = 0.3; slow_mute_update(&a); a.paused = 1; slow_mute_update(&a); CHECK(a.fw_mute == 0); a.paused = 0;   /* paused: nothing to mute */
    act(&a, A_OK); CHECK(a.pace_mode == 0 && fm1ntt_pace_level(a.m) == 0);
    a.speed = 0;
    /* the SOUND preview: voices in, a toggle, notes from the keys and the pad reach the mix and not the machine, and leaving the panel lets them go */
    {
        static uint8_t two[2 * 128];
        for (int v = 0; v < 2; v++) {
            uint8_t *p = two + v * 128;
            for (int op = 0; op < 6; op++) { uint8_t *o = p + op * 17; o[0] = o[1] = o[2] = o[3] = 99; o[4] = o[5] = o[6] = 99; o[14] = 99; o[15] = (uint8_t)(1 << 1); o[16] = 7; }
            p[102] = p[103] = p[104] = p[105] = 99; p[106] = p[107] = p[108] = p[109] = 50; p[110] = 31;
            memcpy(p + 118, v ? "SECOND    " : "FIRST     ", 10);
        }
        a.panel = PANEL_SOUND;
        pv_set(&a, two, 0);                                                   /* none of the firmware's own: the built-in eight */
        CHECK(a.pv_n == 8 && a.pv_factory && a.pv != NULL);
        { char fn[16]; pv_name(&a, fn, sizeof fn); CHECK(!strcmp(fn, "SINE")); }
        pv_set(&a, two, 2);
        CHECK(!a.pv_factory);
        CHECK(a.pv_n == 2 && a.pv_sel == 0 && a.pv != NULL);
        char nm[16]; pv_name(&a, nm, sizeof nm); CHECK(!strcmp(nm, "FIRST"));
        int16_t quiet[2 * 2048], loud[2 * 2048];
        audio_step(&a, quiet, 2048);
        {   /* USB MIDI input (host/midi_in.h): the byte parser, then the reader on a FIFO named like a raw MIDI device */
            MidiParser mp; uint8_t o[3];
            memset(&mp, 0, sizeof mp);
            CHECK(midi_parse(&mp, 0x90, o) == 0 && midi_parse(&mp, 60, o) == 0 && midi_parse(&mp, 100, o) == 3 && o[0] == 0x90 && o[1] == 60 && o[2] == 100);
            CHECK(midi_parse(&mp, 62, o) == 0 && midi_parse(&mp, 0xF8, o) == 1 && o[0] == 0xF8 && midi_parse(&mp, 90, o) == 3 && o[0] == 0x90 && o[1] == 62 && o[2] == 90);   /* running status; a clock byte inside a message */
            CHECK(midi_parse(&mp, 0xC0, o) == 0 && midi_parse(&mp, 7, o) == 2 && o[0] == 0xC0 && o[1] == 7 && o[2] == 0);                       /* program change: one data byte */
            CHECK(midi_parse(&mp, 0xF0, o) == 0 && midi_parse(&mp, 1, o) == 0 && midi_parse(&mp, 2, o) == 0 && midi_parse(&mp, 0xF7, o) == 0 && midi_parse(&mp, 5, o) == 0);   /* exclusive skipped; no status after it */
            CHECK(midi_parse(&mp, 0xFA, o) == 1 && midi_parse(&mp, 0xFC, o) == 1);
            CHECK(midi_parse(&mp, 0xE0, o) == 0 && midi_parse(&mp, 0, o) == 0 && midi_parse(&mp, 64, o) == 3 && o[0] == 0xE0 && o[2] == 64);
            CHECK(midi_in_isdev("midiC1D0") && midi_in_isdev("midiC12D3") && !midi_in_isdev("midiC1") && !midi_in_isdev("pcmC0D0p") && !midi_in_isdev("midiCD0"));
            char dir[300], fifo[340];
            snprintf(dir, sizeof dir, "%s/fm1ntt-midiin-%d", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp", (int)getpid());
            snprintf(fifo, sizeof fifo, "%s/midiC9D0", dir);
            mkdir(dir, 0700);
            if (mkfifo(fifo, 0600) == 0) {
                int wfd = open(fifo, O_RDWR | O_NONBLOCK);               /* the "device"; held open so the reader sees no end of file */
                MidiIn mi;
                CHECK(wfd >= 0 && midi_in_start(&mi, dir) == 0);
                const uint8_t wire[] = {0x90, 60, 100, 62, 90, 0xF8, 0x80, 60, 0};
                CHECK(write(wfd, wire, sizeof wire) == (ssize_t)sizeof wire);
                MidiMsg got[8]; int ng = 0;
                for (int i = 0; i < 400 && ng < 4; i++) { if (!midi_in_pop(&mi, &got[ng])) SDL_Delay(5); else ng++; }
                CHECK(ng == 4 && mi.devs == 1 && mi.dropped == 0);
                CHECK(ng == 4 && got[0].n == 3 && got[0].b[0] == 0x90 && got[0].b[1] == 60 && got[1].b[1] == 62 && got[1].b[2] == 90 && got[2].n == 1 && got[2].b[0] == 0xF8 && got[3].b[0] == 0x80 && got[3].b[2] == 0);
                CHECK(ng == 4 && got[1].ts >= got[0].ts && got[3].ts >= got[2].ts && now_us() - got[3].ts < 5000000);
                midi_in_stop(&mi);
                close(wfd);
            } else CHECK(0);
            remove(fifo); rmdir(dir);
        }
        {   /* the machine's MIDI through its queue: pushed without the lock, taken by the callback, none left; not while paused (that goes straight in) */
            uint8_t on[3] = {0x90, 60, 100};
            a.mevq = fm1ntt_evq_new(64); a.mq_live = 1;
            midi_send(&a, on, 3, 0);
            CHECK(fm1ntt_evq_len(a.mevq) == 1);
            SDL_Delay(2);
            audio_step(&a, loud, 2048);
            CHECK(fm1ntt_evq_len(a.mevq) == 0 && fm1ntt_evq_dropped(a.mevq) == 0);
            /* keys, encoders and the pot take the same road, in order; the callback applies them at their frames, and while paused it applies them at once */
            key_id(&a, 20, 1); enc(&a, 2, 3); pot(&a, 5); key_id(&a, 21, 1);
            CHECK(fm1ntt_evq_len(a.mevq) == 4 && a.enc_turns[2] >= 3 && fm1ntt_keys_down(a.m) == 0);
            SDL_Delay(2);
            audio_step(&a, loud, 2048);
            CHECK(fm1ntt_evq_len(a.mevq) == 0 && fm1ntt_keys_down(a.m) == ((1ULL << 20) | (1ULL << 21)));
            a.paused = 1; key_id(&a, 20, 0); key_id(&a, 21, 0);
            CHECK(fm1ntt_evq_len(a.mevq) == 2);
            audio_step(&a, loud, 2048);
            CHECK(fm1ntt_evq_len(a.mevq) == 0 && fm1ntt_keys_down(a.m) == 0);       /* paused: applied at once, nothing kept for later */
            a.paused = 0;
            fm1ntt_evq_free(a.mevq); a.mevq = NULL; a.mq_live = 0;
        }
        act(&a, A_START); CHECK(a.pv_on == 1);
        act(&a, A_DOWN); CHECK(a.pv_sel == 1); pv_name(&a, nm, sizeof nm); CHECK(!strcmp(nm, "SECOND"));
        act(&a, A_DOWN); CHECK(a.pv_sel == 0);
        act(&a, A_RIGHT); act(&a, A_RIGHT); act(&a, A_RIGHT); CHECK(a.pv_oct == 2);
        act(&a, A_LEFT); act(&a, A_LEFT); act(&a, A_LEFT); act(&a, A_LEFT); act(&a, A_LEFT); CHECK(a.pv_oct == -2);
        act(&a, A_RIGHT); act(&a, A_RIGHT); CHECK(a.pv_oct == 0);
        SDL_Event ev; memset(&ev, 0, sizeof ev);
        ev.type = SDL_KEYDOWN; ev.key.keysym.sym = 'q';                          /* key 12: F4 + 12 = MIDI 65 */
        handle(&a, &ev);
        CHECK(a.pv_key[12] == 65 + 1 && fm1ntt_keys_down(a.m) == 0);          /* the preview heard it, the machine did not */
        audio_step(&a, loud, 2048);
        long e0 = 0, e1 = 0;
        for (int i = 0; i < 2 * 2048; i++) { e0 += abs(quiet[i]); e1 += abs(loud[i]); }
        CHECK(e1 > e0 + 2048 * 20);                                              /* it sounds */
        ev.type = SDL_KEYUP; handle(&a, &ev); CHECK(a.pv_key[12] == 0);
        memset(&ev, 0, sizeof ev); ev.type = SDL_JOYBUTTONDOWN; ev.jbutton.button = 3; handle(&a, &ev);   /* the pad's Y: the root */
        CHECK(a.pv_pad[0] == 60 + 1);
        /* Solo (R2): the machine is not run, the engine still sounds, and leaving the preview ends it */
        {
            Fm1Cpu c0, c1;
            fm1ntt_cpu(a.m, 0, &c0);
            act(&a, A_SELECT); CHECK(a.pv_solo == 1);
            int16_t solo_out[2 * 2048];
            audio_step(&a, solo_out, 2048);
            fm1ntt_cpu(a.m, 0, &c1);
            long es = 0; for (int i = 0; i < 2 * 2048; i++) es += abs(solo_out[i]);
            CHECK(c1.insns == c0.insns && es > 2048 * 20);                       /* no instruction ran, and the held chord sounds */
            act(&a, A_SELECT); CHECK(a.pv_solo == 0);
            act(&a, A_SELECT); act(&a, A_START); CHECK(!a.pv_on && !a.pv_solo);          /* turning the preview off turns solo off */
            act(&a, A_SELECT); CHECK(!a.pv_solo && a.msg_style == 2);                 /* solo with the preview off says why not */
            act(&a, A_START); act(&a, A_SELECT); CHECK(a.pv_solo == 1);
        }
        a.panel = PANEL_MIDI; pv_leave(&a);                                      /* leaving SOUND: off, every note released */
        CHECK(!a.pv_on && !a.pv_solo && a.pv_pad[3] == 0);
        /* an instrument package (docs/PORTING.md): loaded from a file, its notes reach the mix, Solo plays it alone, and a firmware's voices replace it */
        {
            const char *pk = "fm1i = 1\nname = Selftest\nengine = analog\nlicence = MIT\n";
            char ip[256];
            snprintf(ip, sizeof ip, "%s/selftest-instrument.fm1i", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp");
            FILE *tf = fopen(ip, "wb");
            CHECK(tf != NULL);
            if (tf) {
                fputs(pk, tf);
                fclose(tf);
                CHECK(inst_load(&a, ip) == 0 && a.inst != NULL && a.pv_n == 1);
                { char fn[32]; pv_name(&a, fn, sizeof fn); CHECK(!strcmp(fn, "Selftest")); }
                a.pv_on = 1; lock(&a); a.pv_solo = 1; unlock(&a);
                pv_note(&a, &a.pv_key[0], 60, 1);
                int16_t io[2 * 2048];
                audio_step(&a, io, 2048);
                long ie = 0; for (int i = 0; i < 2 * 2048; i++) ie += abs(io[i]);
                CHECK(ie > 2048L * 50);
                pv_note(&a, &a.pv_key[0], 60, 0);
                /* with the queue: the note goes in without the lock, the callback takes it, and it sounds on the next buffer */
                a.evq = fm1ntt_evq_new(64);
                pv_note(&a, &a.pv_key[0], 60, 1);
                CHECK(fm1ntt_evq_len(a.evq) == 1);
                SDL_Delay(2);                                  /* a message stamped at the very instant of the callback belongs to the next buffer */
                audio_step(&a, io, 2048);
                CHECK(fm1ntt_evq_len(a.evq) == 0 && fm1ntt_evq_dropped(a.evq) == 0);
                audio_step(&a, io, 2048);
                { long qe = 0; for (int i = 0; i < 2 * 2048; i++) qe += abs(io[i]); CHECK(qe > 2048L * 50); }
                pv_note(&a, &a.pv_key[0], 60, 0);
                CHECK(fm1ntt_evq_len(a.evq) == 1);
                {   /* a USB MIDI message plays the instrument through its queue, with the time it arrived, and wakes the audio device */
                    MidiMsg mm = {.ts = now_us(), .n = 3, .b = {0x90, 64, 100}};
                    a.pv_on = 1; a.last_input_ms = 0;
                    midi_in_route(&a, &mm);
                    CHECK(fm1ntt_evq_len(a.evq) == 2 && a.min_routed == 1 && a.last_input_ms != 0);
                }
                fm1ntt_evq_free(a.evq); a.evq = NULL;
                CHECK(inst_load(&a, "/nonexistent/none.fm1i") != 0 && a.inst != NULL && a.msg_style == 2);   /* a bad load keeps the instrument and says why */
                {   /* the instrument editor: B opens it, a page of the group's parameters, +-1 and a big step, the groups, a save that never overwrites the source, and the picker */
                    a.panel = PANEL_SOUND; a.pv_on = 0;
                    act(&a, A_BACK); CHECK(a.ed_on == 1);
                    fill_ui(&a, ui, 0);
                    CHECK(ui->ed_on && ui->ed_total == 11 && ui->ed_n == 11 && ui->ed_groups == 5 && ui->ed_page == 0 && ui->ed_pages == 1 && !strcmp(ui->ed_title, "ENGINE ANALOG") && !strcmp(ui->ed_inst, "Selftest"));
                    a.ed_sel = 2;                                                              /* CUTOFF, 0 to 127 */
                    int c0 = fm1ntt_inst_get(a.inst, 2);
                    act(&a, A_RIGHT); CHECK(fm1ntt_inst_get(a.inst, 2) == c0 + 1 && a.ed_dirty);
                    act(&a, A_X); CHECK(fm1ntt_inst_get(a.inst, 2) == c0 + 1 - 12);            /* the big step is a tenth of the range, at least 10 */
                    for (int k = 0; k < 30; k++) act(&a, A_X);
                    CHECK(fm1ntt_inst_get(a.inst, 2) == 0);                                    /* clamped at the bottom */
                    act(&a, A_Y); act(&a, A_Y);
                    int c1 = fm1ntt_inst_get(a.inst, 2);
                    act(&a, A_UP); act(&a, A_UP); act(&a, A_UP); CHECK(a.ed_sel == 10 && a.ed_sel != 2);   /* up wraps through the top */
                    act(&a, A_START); CHECK(a.ed_group == 1 && a.ed_sel == 0);
                    fill_ui(&a, ui, 0);
                    CHECK(ui->ed_total == 11 && !strcmp(ui->ed_title, "ARPEGGIATOR") && ui->ed_row[3].min == 30 && ui->ed_row[3].max == 300);
                    act(&a, A_RIGHT); CHECK(fm1ntt_inst_arp_get(a.inst, 0) == 1);               /* ARP ON */
                    act(&a, A_START); CHECK(a.ed_group == 2);                                       /* the sequencer's settings, then the pattern's steps */
                    fill_ui(&a, ui, 0);
                    CHECK(ui->ed_total == 11 && !strcmp(ui->ed_title, "SEQUENCER"));
                    act(&a, A_START); CHECK(a.ed_group == 3);
                    fill_ui(&a, ui, 0);
                    CHECK(ui->ed_total == 33 && ui->ed_pages == 2 && !strcmp(ui->ed_title, "PATTERN 1"));
                    a.ed_sel = 1; act(&a, A_Y); CHECK(fm1ntt_inst_seq_get(a.inst, 101) == 12);   /* step 1's note: a big step (a tenth of 0 to 127) up from a rest */
                    a.ed_sel = 30; fill_ui(&a, ui, 0); CHECK(ui->ed_page == 1 && ui->ed_n == 9 && ui->ed_sel == 6);   /* the second page holds the last 9 rows */
                    act(&a, A_START); CHECK(a.ed_group == 4);                                       /* the global settings: channels, bend range, velocity, mono */
                    fill_ui(&a, ui, 0);
                    CHECK(ui->ed_total == 8 && !strcmp(ui->ed_title, "GLOBAL") && ui->ed_row[2].max == 24);
                    a.ed_sel = 2; act(&a, A_RIGHT); CHECK(fm1ntt_inst_global_get(a.inst, 2) == 3 && a.ed_dirty);   /* BEND RANGE 2 to 3 */
                    a.ed_group = 3; a.ed_sel = 1; act(&a, A_X); CHECK(fm1ntt_inst_seq_get(a.inst, 101) == 0);
                    act(&a, A_SELECT); act(&a, A_SELECT); act(&a, A_SELECT); CHECK(a.ed_group == 0);
                    act(&a, A_OK);                                                               /* saved to instruments/selftest-edit.fm1i, not over the source */
                    char sp2[400]; sub_path(sp2, sizeof sp2, "instruments", "selftest-edit.fm1i");
                    CHECK(is_file(sp2) && a.msg_style == 1 && !a.ed_dirty && is_file(ip));
                    CHECK(inst_load(&a, sp2) == 0 && fm1ntt_inst_get(a.inst, 2) == c1 && fm1ntt_inst_arp_get(a.inst, 0) == 1 && fm1ntt_inst_global_get(a.inst, 2) == 3);   /* the edits came back */
                    {   /* the effect chain's order from the editor: POSITION ends an effect's group when there are two or more; Left and Right move it and the editor follows */
                        char fp[256];
                        snprintf(fp, sizeof fp, "%s/selftest-fx.fm1i", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp");
                        FILE *ff = fopen(fp, "wb");
                        CHECK(ff != NULL);
                        if (ff) {
                            fputs("fm1i = 1\nname = FxTest\nengine = analog\nlicence = MIT\nfx = delay\nfx = reverb\n", ff);
                            fclose(ff);
                            CHECK(inst_load(&a, fp) == 0 && fm1ntt_inst_fx_count(a.inst) == 2);
                            char n0[24], n1[24]; int np = (int)fm1ntt_inst_fx_param_count(a.inst, 0);
                            fm1ntt_inst_fx_name(a.inst, 0, n0, sizeof n0);
                            a.ed_on = 1; a.ed_group = ED_FX0; a.ed_sel = np;
                            fill_ui(&a, ui, 0);
                            CHECK(ui->ed_total == np + 1 && !strcmp(ui->ed_row[(np) % ED_ROWS].name, "POSITION (0 FIRST)") && ui->ed_row[np % ED_ROWS].val == 0);
                            act(&a, A_RIGHT);                                                  /* later: the delay is second now, and the editor is on it */
                            fm1ntt_inst_fx_name(a.inst, 1, n1, sizeof n1);
                            CHECK(!strcmp(n0, n1) && a.ed_group == ED_FX0 + 1 && a.ed_dirty);
                            act(&a, A_RIGHT); CHECK(a.ed_group == ED_FX0 + 1);                  /* clamped at the end */
                            act(&a, A_LEFT); CHECK(a.ed_group == ED_FX0);
                            remove(fp);
                        }
                    }
                    /* it is in the FILES list and A plays it */
                    a.panel = PANEL_FILES; scan_files(&a);
                    int fi = -1; for (int i = 0; i < a.nfiles; i++) if (!strcmp(a.files[i], "selftest-edit.fm1i")) fi = i;
                    CHECK(fi >= 0);
                    if (fi >= 0) { a.file_sel = fi; act(&a, A_OK); CHECK(a.panel == PANEL_SOUND && a.pv_on && a.inst != NULL && !a.ed_on); }
                    /* the context menu opens it with the preview playing (B plays the third then); the pad's faces edit while it is open instead of playing; the panel draws in both layouts */
                    SDL_Event se; memset(&se, 0, sizeof se); se.type = SDL_JOYBUTTONDOWN; se.jbutton.button = 0;
                    { const char *cl[PX_MENU_MAX]; int cc[PX_MENU_MAX], n = ctx_items(&a, cl, cc), has = 0; for (int i = 0; i < n; i++) has |= !strcmp(cl[i], "EDIT THE INSTRUMENT"); CHECK(has); }
                    ctx_run(&a, CX_ACT | A_BACK); CHECK(a.ed_on == 1);
                    se.jbutton.button = 3; handle(&a, &se); CHECK(a.pv_pad[0] == 0);            /* Y did not play a note */
                    uint32_t *ecv = calloc((size_t)PX_W * PX_H, 4);
                    for (int md = 0; md < 2 && ecv; md++) {
                        a.mode = md ? MODE_ZOOMED : MODE_STANDARD;
                        a.ed_group = 0; a.ed_sel = 0; fill_ui(&a, ui, 0); px_draw(ecv, a.mode, ui);
                        PxRect co = px_content_rect(a.mode);
                        long sel = 0, bottom = 0;
                        for (int y = co.y; y < co.y + co.h; y++) for (int x = co.x; x < co.x + co.w; x++) { uint32_t px = ecv[y * PX_W + x]; sel += px == C_SEL; if (y >= co.y + co.h - 14 && px != C_BG && px != C_PANEL) bottom++; }
                        if (getenv("FM1NTT_MON_SHOTS")) { char fn[420]; snprintf(fn, sizeof fn, "%s/editor-%s.bmp", getenv("FM1NTT_MON_SHOTS"), md ? "zoomed" : "standard"); mon_dump_bmp(fn, ecv); }
                        CHECK(sel > 300);                                                       /* the chosen row is marked */
                        CHECK(bottom < (long)co.w * 14 / 2);                                    /* the foot is the panel's own one-line hint, not the editor's */
                    }
                    free(ecv);
                    a.mode = MODE_STANDARD;
                    se.jbutton.button = 0; handle(&a, &se); CHECK(a.ed_on == 0);                 /* B closes it */
                    a.panel = PANEL_SOUND;
                    remove(sp2);
                }
                pv_set(&a, two, 2); CHECK(a.inst == NULL && a.pv != NULL);
                remove(ip);
            }
            a.pv_on = 0; lock(&a); a.pv_solo = 0; unlock(&a);
        }
        pv_set(&a, two, 0); CHECK(a.pv_n == 8 && a.pv_factory && a.pv != NULL && !a.pv_on);
    }
    /* the recorder: the ring to a real WAV, header sizes right, renamed from .part, a quit while recording asks first */
    a.panel = PANEL_SOUND;
    a.rec_ring = malloc((size_t)REC_RING_FRAMES * 2 * sizeof(int16_t));
    act(&a, A_OK); CHECK(a.rec != NULL && a.rec_on);
    for (int i = 0; i < 4000; i++) a.rec_ring[i] = (int16_t)i;
    SDL_AtomicSet(&a.rec_w, 4000);
    rec_pump(&a); CHECK(a.rec_frames == 2000);
    /* a quit while recording asks first, and stopping saves the file */
    a.panel = PANEL_TOOLS; menu_open_main(&a); a.menu_sel = MM_QUIT;                             /* Quit is the main menu's */
    menu_press(&a, A_OK); CHECK(a.confirm == CF_QUIT && !a.quit);
    act(&a, A_BACK); CHECK(a.confirm == CF_NONE && a.rec != NULL && !a.quit);
    menu_open_main(&a); a.menu_sel = MM_QUIT; menu_press(&a, A_OK); CHECK(a.confirm == CF_QUIT); act(&a, A_OK); CHECK(a.quit == 1 && a.rec == NULL);
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
            CHECK(strstr(txt, "# fm1ntt benchmark\nformat: 1\n") == txt && strstr(txt, "\nworkload: 3\n") && strstr(txt, "\nplatform: ") && strstr(txt, "\ncpu: ") && strstr(txt, "\n== results ==\n"));
            CHECK(strstr(txt, "\nBENCH core.nop.interp: ") && strstr(txt, "\nBENCH core.boot: ") && strstr(txt, "\nBENCH core.reset: ") && strstr(txt, "\nBENCH mem.read: ") &&
                  strstr(txt, "\nBENCH screen.draw.standard: ") && strstr(txt, "\nBENCH screen.draw.zoomed: ") && strstr(txt, "\nBENCH io.flash.write: ") && strstr(txt, "\nBENCH io.flash.read: "));
            CHECK(strstr(txt, "\nBENCH sound.fm6.voices1.load: ") && strstr(txt, "\nBENCH sound.fm6.voices6.load: ") && strstr(txt, "\nBENCH sound.fm6.voices12.load: "));
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
            const char *secs[] = {"== system ==", "== card ==", "== core ==", "== speed ==", "== screen ==", "== sound ==", "== files ==", "== mapping ==", "== power ==", "== memory ==", "== summary =="};
            for (unsigned k = 0; k < sizeof secs / sizeof *secs; k++) CHECK(strstr(txt, secs[k]) != NULL);
            CHECK(strstr(txt, "FM-1 reference: the FM-1 itself runs about 12 h") != NULL);   /* the real machine's figures, to read the unit's against */
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
        CHECK(nb - np >= 6);                                                      /* the permissive tier: always shipped (five firmwares and an instrument package) */
        CHECK(pk[0] ? np >= 3 : np == 0);                                         /* the copyleft pack: its three, or none when it is not installed */
        int booted = 0, nfw = 0, ninst = 0;
        for (int i = 0; i < b2.nfiles; i++) {
            if (!b2.fbundled[i]) continue;
            if (has_ext(b2.fpaths[i], ".fm1i")) {                                /* an instrument package: it loads into the engine, it is not booted */
                ninst++;
                uint8_t *ib = NULL; size_t ilen = 0; char iw[64];
                ib = read_file(b2.fpaths[i], &ilen, iw, sizeof iw);
                fm1ntt_inst *ih = ib ? fm1ntt_inst_load(ib, ilen, FM1_SAMPLE_RATE, NULL, NULL) : NULL;
                CHECK(ih != NULL);
                if (ih) fm1ntt_inst_free(ih);
                free(ib);
                continue;
            }
            nfw++;
            char why2[64]; size_t n2 = 0;
            uint8_t *fwb = read_file(b2.fpaths[i], &n2, why2, sizeof why2);
            fm1ntt_machine *bm = fm1ntt_new();
            int ok = fwb && fm1ntt_load_package(bm, fwb, n2) == 0;
            free(fwb);
            int16_t pcm2[1024];
            /* a firmware that draws late (a MIDI monitor waits for its first message) gets up to 8 emulated seconds to light its screen; the rest stop at once */
            static uint32_t scr[FM1_SCREEN_W * FM1_SCREEN_H];
            int colours = 0;
            for (int k = 0; ok && k < 700 && !fm1ntt_halted(bm) && (colours < 8); k++) {
                fm1ntt_render(bm, pcm2, 512, 2000000);
                if (k % 40 != 39) continue;
                fm1ntt_screen(bm, scr);
                uint32_t seen[16] = {0};
                colours = 0;
                for (int q = 0; q < FM1_SCREEN_W * FM1_SCREEN_H && colours < 16; q++) { int f = 0; for (int r = 0; r < colours; r++) if (seen[r] == scr[q]) f = 1; if (!f) seen[colours++] = scr[q]; }
            }
            if (!ok || fm1ntt_halted(bm) || colours < 8) fprintf(stderr, "  bundled firmware %s: %s\n", b2.files[i], !ok ? "did not load" : fm1ntt_halted(bm) ? "halted" : "drew almost nothing");
            booted += ok && !fm1ntt_halted(bm) && colours >= 8;
            fm1ntt_free(bm);
        }
        CHECK(booted == nfw && ninst >= 1);                                       /* every bundled firmware boots; the bundled instrument package loads */
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
        CHECK(emu_selftest(dflt) == 0);                                           /* the emulation thread, on the bundled firmware */
        setenv("FM1NTT_BUNDLED", nob, 1);
        setenv("FM1NTT_PACK", nob, 1);
        scan_files(&a);
    }
    /* a pause makes the audio callback silent without touching the machine */
    int16_t pcm[256];
    CHECK(fm1ntt_render(a.m, pcm, 128, 100000) <= 128);
    CHECK(root_selftest(root, &a, ui) == 0);                          /* where the app's folder is: the order of the rules, r3i6nos's engine, a data card's own rules, what the screens say */
    CHECK(show_selftest(root) == 0);                                  /* the firmware showcase: its sidecar, its files, a cancel, a failure, two identical runs */
    CHECK(show_app_selftest(root, &a, ui) == 0);                      /* the showcase in the app: page, policy, a real worker, the queue, the player (uses a.m and ui: before they are freed) */
    CHECK(mon_selftest(root, &a) == 0);                               /* the MONITOR panel: parsers, maths, the fixture /proc, the cost rule, the layout, the keys (uses a.m: before it is freed) */
    free(ui);
    free(a.rec_ring);
    fm1ntt_free(a.m);
    /* a missing file says so */
    App b = {0};
    b.m = fm1ntt_new();
    CHECK(load_firmware(&b, "/nonexistent/x.fwsc", 0, 0) == -1 && b.msg_style == 3);
    fm1ntt_free(b.m);
    /* threads.h: the /proc parser, the per-thread sums and the placement choice (the parts that are plain functions, on any system), and /proc and the
     * affinity calls themselves where there is a Linux */
    {
        ThrCpu t;
        char line[800], num[24];
        snprintf(line, sizeof line, "1234 (tmux: srv (x)) S");
        for (int f = 4; f <= 52; f++) { snprintf(num, sizeof num, " %d", f == 14 ? 250 : f == 15 ? 50 : f == 39 ? 3 : 0); strcat(line, num); }
        CHECK(thr_parse_stat(line, &t) && t.tid == 1234 && !strcmp(t.name, "tmux: srv (x)") && t.ticks == 300 && t.cpu == 3);
        CHECK(thr_parse_stat("5 (a) S 1 1 1 0 0 0 0 0 0 0 10 20", &t) && t.ticks == 30 && t.cpu == -1);      /* a short line: no processor field */
        CHECK(!thr_parse_stat("garbage", &t) && !thr_parse_stat("1 (x", &t) && !thr_parse_stat("5 (a) S 1 1", &t) && !thr_parse_stat("", &t));
        long hz = sysconf(_SC_CLK_TCK);
        ThrSnap sa = {0}, sb = {0};
        sa.wall_ms = 1000; sb.wall_ms = 5000;                                  /* 4 s */
        sa.n = 1; sa.t[0] = (ThrCpu){.tid = 100, .ticks = 10, .cpu = 0}; strcpy(sa.t[0].name, "fm1ntt");
        sb.n = 3;
        sb.t[0] = (ThrCpu){.tid = 100, .ticks = 10 + (unsigned long long)hz * 4 / 10, .cpu = 0}; strcpy(sb.t[0].name, "fm1ntt");          /* 0.4 s: 10% */
        sb.t[1] = (ThrCpu){.tid = 200, .ticks = (unsigned long long)hz * 2, .cpu = 3}; strcpy(sb.t[1].name, "fm1ntt-job");              /* 2 s: 50% */
        sb.t[2] = (ThrCpu){.tid = 300, .ticks = 0, .cpu = 1}; strcpy(sb.t[2].name, "idle");
        char tx[400];
        thr_describe(&sa, &sb, 100, 4, tx, sizeof tx);
        CHECK(strstr(tx, "over 4.0 s: fm1ntt-job 50.0% (cpu 3), main 10.0% (cpu 0)") && strstr(tx, "all 3 threads 60.0% of 4 cores") && !strstr(tx, "idle"));
        ThrSnap none = {0};
        thr_describe(&sa, &none, 100, 4, tx, sizeof tx);
        CHECK(strstr(tx, "not available"));
        int e = -1, u = -1;
        CHECK(!thr_choose(1, &e, &u) && thr_choose(4, &e, &u) && e == 3 && u == 0 && thr_choose(2, &e, &u) && e == 1 && u == 0);
#ifdef __linux__
        ThrSnap live;
        int found = 0;
        CHECK(thr_snapshot(&live, 0) >= 1);
        for (int i = 0; i < live.n; i++) if (live.t[i].tid == (int)getpid()) found = 1;
        CHECK(found);                                                                          /* the main thread is a task of this process */
        cpu_set_t old, now;
        CHECK(sched_getaffinity(0, sizeof old, &old) == 0);
        int first = -1;
        for (int i = 0; i < CPU_SETSIZE && first < 0; i++) if (CPU_ISSET(i, &old)) first = i;
        CHECK(first >= 0 && thr_pin_self(first) == 0 && sched_getaffinity(0, sizeof now, &now) == 0 && CPU_COUNT(&now) == 1 && CPU_ISSET(first, &now));
        sched_setaffinity(0, sizeof old, &old);                                                 /* put it back */
        ThrPin pp;
        CHECK(thr_pin_begin(&pp, 0, 0, 2) == -2);                                              /* this IS the main thread */
        char fw[300];
        unsetenv("FM1NTT_BUNDLED");                                                            /* (the user-folder tests above pointed it at an empty folder) */
        default_firmware(fw, sizeof fw);
        if (getenv("FM1NTT_VERBOSE")) fprintf(stderr, "pin test: firmware '%s', %d cpus\n", fw, thr_ncpu());
        if (fw[0] && thr_ncpu() >= 2) {                                                        /* end to end, on a job thread, with the bundled firmware */
            cpu_set_t main_before, main_after;
            sched_getaffinity((pid_t)getpid(), sizeof main_before, &main_before);
            PinJob *pj = calloc(1, sizeof *pj);
            snprintf(pj->fw, sizeof pj->fw, "%s", fw);
            SDL_Thread *jt = SDL_CreateThread(pin_job, "pin-test", pj);
            if (jt) SDL_WaitThread(jt, NULL);
            sched_getaffinity((pid_t)getpid(), sizeof main_after, &main_after);
            CHECK(jt && pj->rc == 1);
            for (int i = 0; i < BENCH_RUNS; i++) CHECK(pj->ab.free_x[i] > 0 && pj->ab.pin_x[i] > 0);
            CHECK(pj->ab.emu_cpu == thr_ncpu() - 1 && pj->ab.ui_cpu == 0);
            CHECK(CPU_EQUAL(&main_before, &main_after));                                       /* the main thread's mask is put back */
            char pt[500];
            pin_ab_text(&pj->ab, pt, sizeof pt);
            CHECK(strstr(pt, "free ") && strstr(pt, "pinned ") && strstr(pt, "cpu "));
            if (getenv("FM1NTT_VERBOSE")) fprintf(stderr, "pin test: %s\n", pt);
            free(pj);
        }
#endif
    }
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
    self_exe_init(argv[0]);
    int do_show = 0, show_jit = 0;
    const char *show_out = NULL;
    int raw = 0, zoomed = 0, preview_opt = 0, panel = 0, do_report = 0, do_bench = 0, do_demo = 0, no_video = 0, no_jit = 0, pin_opt = 0, pace_opt = PACE_AUTO;
    const char *inst_opt = NULL;
    double seconds = 1.0;
    uint32_t entry = FM1_ENTRY_FELUCCA;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--version")) { printf("fm1ntt %s (%s)\n", fm1ntt_version(), FM1NTT_BUILD); return 0; }
        else if (!strcmp(argv[i], "--selftest")) return selftest();
        else if (!strcmp(argv[i], "--entry") && i + 1 < argc) { raw = 1; entry = (uint32_t)strtoul(argv[++i], NULL, 0); }
        else if (!strcmp(argv[i], "--screenshot") && i + 1 < argc) shot = argv[++i];
        else if (!strcmp(argv[i], "--panel") && i + 1 < argc) panel = atoi(argv[++i]) % PANEL_COUNT;
        else if (!strcmp(argv[i], "--zoomed")) zoomed = 1;
        else if (!strcmp(argv[i], "--instrument") && i + 1 < argc) inst_opt = argv[++i];   /* an instrument package (.fm1i) for the SOUND preview, with Solo's engine-alone play when no firmware is given */
        else if (!strcmp(argv[i], "--preview")) preview_opt = 1;   /* the SOUND preview on from the start (a screenshot shows it playing) */
        else if (!strcmp(argv[i], "--report")) do_report = 1;
        else if (!strcmp(argv[i], "--bench")) do_bench = 1;
        else if (!strcmp(argv[i], "--seconds") && i + 1 < argc) { seconds = atof(argv[++i]); if (seconds < 0.05) seconds = 0.05; if (seconds > 60) seconds = 60; }
        else if (!strcmp(argv[i], "--fw") && i + 1 < argc) fwopt = argv[++i];
        else if (!strcmp(argv[i], "--pace") && i + 1 < argc) { const char *v = argv[++i]; pace_opt = !strcmp(v, "auto") ? PACE_AUTO : atoi(v) % 4; }
        else if (!strcmp(argv[i], "--no-jit")) no_jit = 1;
        else if (!strcmp(argv[i], "--pin")) pin_opt = 1;
        else if (!strcmp(argv[i], "--rec-demo")) do_demo = 1;
        else if (!strcmp(argv[i], "--showcase")) do_show = 1;
        else if (!strcmp(argv[i], "--jit")) show_jit = 1;
        else if (!strcmp(argv[i], "--out") && i + 1 < argc) show_out = argv[++i];
        else if (!strcmp(argv[i], "--features") && i + 1 < argc) features = argv[++i];
        else if (!strcmp(argv[i], "--no-video")) no_video = 1;
        else if (!strcmp(argv[i], "--reduced-motion")) setenv("FM1NTT_REDUCED_MOTION", "1", 1);
        else if (argv[i][0] != '-') path = argv[i];
        else { fprintf(stderr, "fm1ntt: unknown option %s\n", argv[i]); return 2; }
    }

    static App a;
    static UiState u;
    static uint32_t canvas[PX_W * PX_H];
    a.m = fm1ntt_new();
    { char cp[300]; sub_path(cp, sizeof cp, "firmware", NULL); mon_init(&a.mon, cp); }
    a.reduced_motion = getenv("FM1NTT_REDUCED_MOTION") != NULL;
    a.last_input_ms = SDL_GetTicks();
    a.mode = zoomed ? MODE_ZOOMED : MODE_STANDARD;
    a.panel = panel;
    a.midi_note = 60; a.midi_vel = 100; a.slot = 1;
    a.remember = !getenv("FM1NTT_FORGET");               /* on: a firmware's saved settings come back next run (TOOLS turns it off) */
    a.jit_pref = !no_jit && !getenv("FM1NTT_NO_JIT");     /* on wherever the CPU supports it: the interpreter cannot keep up on a handheld */
    a.pace_mode = pace_opt;
    a.pin = pin_opt || (getenv("FM1NTT_PIN") && *getenv("FM1NTT_PIN") && strcmp(getenv("FM1NTT_PIN"), "0"));
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
    if (do_show) {                                       /* the headless showcase (host/showcase.h): no window, no SDL, its own machine */
        if (!path) { fprintf(stderr, "fm1ntt --showcase: give a firmware\n"); return 2; }
        char base[300], res[240];
        if (show_out) snprintf(base, sizeof base, "%s", show_out); else base_dir(base, sizeof base);
        const char *bs = getenv("FM1NTT_SHOWCASE_BUDGET_S");
        int rc = showcase_run(path, raw || has_ext(path, ".bin"), entry, show_jit, base, bs ? atof(bs) : 0, res, sizeof res);
        printf("%s\n", res);
        return rc;
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
        pv_set(&a, NULL, 0);                    /* the built-in voices until a firmware brings its own */
        if (path && load_firmware(&a, path, raw, entry) == 0) {
            int16_t pcm[2048];
            for (int i = 0; i < 60; i++) fm1ntt_render(a.m, pcm, 1024, MAX_INSNS_PER_BUFFER);
        }
        if (inst_opt && inst_load(&a, inst_opt) == 0) { a.panel = PANEL_SOUND; preview_opt = 1; }
        if (preview_opt && a.pv_n) a.pv_on = 1;
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
    a.evq = fm1ntt_evq_new(1024);
    a.mevq = fm1ntt_evq_new(1024);
    a.mq_live = a.dev != 0;

    uint32_t t_start = SDL_GetTicks();
#define SPLASH(what, pct) do { draw_splash_x(canvas, what, pct, (int)(SDL_GetTicks() - t_start), NULL); present(ren, tex, canvas); } while (0)
    SPLASH("STARTING", 10);                              /* at once, before anything heavy */
    keymap_load();                                       /* the buttons the person changed in the Keymap (keymap.txt) */
    theme_load();                                        /* the colour theme the person chose last time (theme.txt in the app's folder; a tiny read) */
    SPLASH("OPENING THE SOUND DEVICE", 25);

    if (a.pin) { int e, u; if (thr_choose(thr_ncpu(), &e, &u)) thr_pin_self(u); }       /* the window's thread: the first core (the audio thread takes the last) */
    SDL_AudioSpec want = {.freq = FM1_SAMPLE_RATE, .format = AUDIO_S16SYS, .channels = 2, .samples = AUDIO_FRAMES, .callback = audio_cb, .userdata = &a}, got;
    a.dev = SDL_OpenAudioDevice(NULL, 0, &want, &got, 0);
    if (!a.dev) say(&a, 2, "No sound device: %s", SDL_GetError());
    else if (!(getenv("FM1NTT_NO_EMU_THREAD") && *getenv("FM1NTT_NO_EMU_THREAD"))) emu_start(&a);       /* the machine renders on its own thread; FM1NTT_NO_EMU_THREAD=1 puts it back in the callback */
    a.mq_live = a.dev != 0;
    SPLASH("READING THE FIRMWARE FOLDER", 40);
    scan_files(&a);
    pv_set(&a, NULL, 0);                        /* the built-in voices until a firmware brings its own */
    if (path) {
        SPLASH("LOADING THE FIRMWARE", 60);
        /* read and booted on the job thread: the splash keeps drawing (and the window answering) with the job's own percent */
        load_start(&a, path, raw, entry);
        while (a.job) {
            SDL_Event se;
            while (SDL_PollEvent(&se)) if (se.type == SDL_QUIT) a.quit = 1;
            if (job_poll(&a)) break;
            int jp = job_pct(&a);
            SPLASH("LOADING THE FIRMWARE", 60 + (jp < 0 ? 0 : jp > 100 ? 100 : jp) * 20 / 100);
            SDL_Delay(30);
        }
        if (!fm1ntt_loaded(a.m)) {
            a.panel = PANEL_FILES;
            /* the error stays on the splash until a press or 4 s, never a blank or frozen window (rule 8) */
            draw_splash_x(canvas, "LOADING THE FIRMWARE FAILED", 60, (int)(SDL_GetTicks() - t_start), a.msg);
            present(ren, tex, canvas);
            for (uint32_t t0 = SDL_GetTicks(); SDL_GetTicks() - t0 < 4000;) {
                SDL_Event se;
                if (SDL_WaitEventTimeout(&se, 50) && (se.type == SDL_KEYDOWN || se.type == SDL_JOYBUTTONDOWN || se.type == SDL_QUIT)) break;
            }
        } else SPLASH("STARTING THE ENGINE", 80);
    }
    if (inst_opt && inst_load(&a, inst_opt) == 0) { a.panel = PANEL_SOUND; a.pv_on = 1; a.pv_solo = !fm1ntt_loaded(a.m); }   /* an instrument: play it now, alone when no firmware is given */
    if (!getenv("FM1NTT_NO_MIDI_IN")) a.min_on = midi_in_start(&a.min, NULL) == 0;
    trace_open();
    toast_init(&a.toast, "/run/r3i6nos-hotplug/events", SDL_GetTicks());
    if (a.dev) SDL_PauseAudioDevice(a.dev, 0);
    SPLASH("READY", 100);                                 /* the last splash frame, held for no time: the app is drawn on the next pass */
#undef SPLASH
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
        if (sticks_active(&a)) { dirty = 1; a.last_input_ms = SDL_GetTicks(); }
        audio_sleep_tick(&a, SDL_GetTicks());
        if (toast_poll(&a.toast, SDL_GetTicks())) dirty = 1;
        persist_save(&a, 0, SDL_GetTicks());
        sticks_tick(&a, SDL_GetTicks());
        { static uint32_t mt; uint32_t nowt = SDL_GetTicks(); if (a.min_on && nowt - mt >= 1000) { mt = nowt; midi_in_tick(&a.min); } }
        if (a.min_on) { uint64_t before = a.min_routed; midi_in_pump(&a); if (a.min_routed != before) dirty = 1; }
        rec_pump(&a);
        speed_tick(&a);
        if (job_poll(&a)) dirty = 1;
        { double nd = (double)SDL_GetTicks(); if (show_poll(&a, nd)) dirty = 1; show_queue(&a, nd); }
        {   /* MONITOR: nothing runs unless the panel shows; one sample a second while it does (monitor.h) */
            double nowm = (double)SDL_GetTicks(), before_ms = a.mon.last_ms;
            int mon_on = a.panel == PANEL_MONITOR;
            if (mon_tick(&a.mon, mon_on, nowm, mon_page_has_procs(a.mode == MODE_ZOOMED, a.mon_page))) dirty = 1;
            if (!mon_on) a.cb_prev_ms = 0;
            else if (a.mon.last_ms != before_ms) {                      /* a sample was just taken: the app's own once-a-second figures move with it */
                int cb = SDL_AtomicGet(&a.callbacks);
                if (a.cb_prev_ms > 0 && nowm > a.cb_prev_ms) a.cb_rate = (int)((double)(cb - a.cb_prev) * 1000.0 / (nowm - a.cb_prev_ms) + 0.5);
                a.cb_prev = cb; a.cb_prev_ms = nowm;
                a.njoy = SDL_NumJoysticks();
                for (int pi = 0; pi < 2; pi++) { const char *pn = pi < a.njoy ? SDL_JoystickNameForIndex(pi) : NULL; snprintf(a.padn[pi], sizeof a.padn[pi], "%s", pn ? pn : ""); for (char *q = a.padn[pi]; *q; q++) if (*q >= 'a' && *q <= 'z') *q = (char)(*q - 32); }
                a.frame_max_ms = 0;
            }
            if (mon_on && a.rain_on && !a.job && !(a.mon.v.bat.has && a.mon.v.bat.pct >= 0 && a.mon.v.bat.pct <= 15) && rain_due(&a.rain, nowm, a.reduced_motion)) { rain_row_feed(&a, nowm); dirty = 1; }
        }
        if (a.want_demo) {                                /* Rec demo owns the unit until it ends */
            a.want_demo = 0;
            char dflt[300];
            default_firmware(dflt, sizeof dflt);
            int have_fw = (fm1ntt_loaded(a.m) && a.fw_path[0]) || dflt[0];
            unsigned mask = demo_checklist(ren, tex, canvas, have_fw);
            if (mask) {
                char res[240];
                DemoWin w = {ren, tex, canvas, 0, 0, -1};
                int pz = a.paused;
                lock(&a); a.paused = 1; a.mq_live = 0; if (a.mevq) fm1ntt_apply_events(a.m, a.mevq); unlock(&a);      /* the demo steps its own clock: its inputs go straight in */
                rec_demo_run(mask, fm1ntt_loaded(a.m) && a.fw_path[0] ? a.fw_path : (dflt[0] ? dflt : NULL), fm1ntt_loaded(a.m) && a.fw_path[0] ? a.fw_raw : (dflt[0] && has_ext(dflt, ".bin")), 1, demo_win_progress, &w, res, sizeof res);
                a.paused = pz; a.mq_live = a.dev != 0;
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
        uint32_t panel_ms = a.pl.on ? 100 : (a.sw_pid && a.panel == PANEL_FILES) ? 500 : a.panel == PANEL_SOUND ? 100 : a.panel == PANEL_MONITOR ? 1000 : a.job ? 250 : 200;       /* a job's progress box moves 4 times a second: the report measures idle while it shows */
        if (dirty || (rev != a.rev && now - last_draw >= 60) || ((fm1ntt_loaded(a.m) || a.job || a.pl.on || (a.sw_pid && a.panel == PANEL_FILES)) && now - last_draw > panel_ms)) {
            uint32_t t0 = SDL_GetTicks();
            int by_input = dirty;
            fill_ui(&a, &u, 1);
            px_draw(canvas, a.mode, &u);
            draw_job_progress(canvas, &a);
            toast_draw(&a.toast, canvas);
            present(ren, tex, canvas);
            a.frame_ms = SDL_GetTicks() - t0;
            if (a.frame_ms > a.frame_max_ms) a.frame_max_ms = a.frame_ms;
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
    if (a.min_on) midi_in_stop(&a.min);
    rec_stop(&a);
    show_shutdown(&a);
    player_stop(&a);
    if (a.job) SDL_WaitThread(a.job, NULL);
    if (a.dev) SDL_CloseAudioDevice(a.dev);
    emu_stop(&a);
    free(a.rec_ring);
    pv_free(&a);
    persist_save(&a, 1, SDL_GetTicks());
    fm1ntt_free(a.m);
    SDL_DestroyTexture(tex);
    SDL_Quit();
    return 0;
}
