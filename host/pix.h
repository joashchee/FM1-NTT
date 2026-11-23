/* FM1-NTT pixel graphics: a 640x480 ARGB canvas, an original 5x7 font, and the two layouts of the window. MIT.
 *
 * The R36S host does not use r3i6nos's text-mode theme: the FM-1's own 240x240 screen is drawn pixel for pixel at the top left,
 * and the space around it holds the sandbox's panels.
 *
 *   Standard  the FM-1 screen at 1x (0,0)-(239,239); panels at the right (x 240..639, the full height) and below it
 *             (x 0..239, y 240..479).
 *   Zoomed    the FM-1 screen at 2x (0,0)-(479,479), each pixel a 2x2 block; one panel at the right (x 480..639).
 *
 * F1 on a keyboard toggles between them. No file here depends on SDL: the canvas is a plain pixel
 * array, so the layout and the drawing are tested headless (host/fm1ntt.c --selftest). The font is original to this project.
 * Static functions, one header. */
#ifndef FM1NTT_PIX_H
#define FM1NTT_PIX_H

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "monitor.h"

enum { PX_W = 640, PX_H = 480, FM_W = 240, FM_H = 240 };
enum { MODE_STANDARD = 0, MODE_ZOOMED = 1 };

typedef struct { int x, y, w, h; } PxRect;

/* Where the FM-1's screen goes in each mode, and the free rectangles around it. */
static inline int px_scale(int mode) { return mode == MODE_ZOOMED ? 2 : 1; }
static inline PxRect px_screen_rect(int mode) { return (PxRect){0, 0, FM_W * px_scale(mode), FM_H * px_scale(mode)}; }
static inline int px_panel_rects(int mode, PxRect out[2]) {
    if (mode == MODE_ZOOMED) { out[0] = (PxRect){480, 0, 160, 480}; return 1; }
    out[0] = (PxRect){240, 0, 400, 480};
    out[1] = (PxRect){0, 240, 240, 240};
    return 2;
}

/* The carousel strip (the top 18 rows of the right panel) and the panel's content area under it. */
static inline PxRect px_carousel_rect(int mode) { PxRect p[2]; px_panel_rects(mode, p); return (PxRect){p[0].x, p[0].y, p[0].w, 18}; }
static inline PxRect px_content_rect(int mode) { PxRect p[2]; px_panel_rects(mode, p); return (PxRect){p[0].x, p[0].y + 20, p[0].w, p[0].h - 20}; }

/* ---- the palette (every pair used for text passes 4.5:1 in every colour theme; px_selftest checks them all) ----------------------------------- */
enum { PXC_BG, PXC_PANEL, PXC_FRAME, PXC_TEXT, PXC_DIM, PXC_HEAD, PXC_OK, PXC_WARN, PXC_ERR, PXC_KEYUP, PXC_KEYDN, PXC_BLACKKEY, PXC_SEL, PXC_COUNT };
/* The colour themes of the main menu's Colour theme item (r3i6nos rule 17). Slots keep their roles in every theme, as r3i6nos's do: BG the page, PANEL a box,
 * TEXT body text, DIM hints, HEAD titles, OK / WARN / ERR good news, caution, mistakes, SEL the selected row, KEYUP / KEYDN / BLACKKEY the piano. Midnight is the default. */
typedef struct { const char *name; uint32_t c[PXC_COUNT]; } PxTheme;
static const PxTheme px_themes[] = {
    {"MIDNIGHT", {0xFF101018, 0xFF181824, 0xFF3C3C58, 0xFFD8D8E0, 0xFF8C8CA0, 0xFF70D0F0, 0xFF60E080, 0xFFF0D060, 0xFFFF7070, 0xFFE8E8E8, 0xFF40B0FF, 0xFF202030, 0xFF2850A0}},
    {"HIGH CONTRAST", {0xFF000000, 0xFF000000, 0xFF505050, 0xFFFFFFFF, 0xFFC8C8C8, 0xFF00FFFF, 0xFF00FF00, 0xFFFFFF00, 0xFFFF8080, 0xFFFFFFFF, 0xFF00FFFF, 0xFF000000, 0xFF0000C0}},
    {"LIGHT", {0xFFF2F2F6, 0xFFFFFFFF, 0xFFB8B8CC, 0xFF1A1A24, 0xFF555568, 0xFF00507A, 0xFF0B6B2A, 0xFF7A5200, 0xFFA01818, 0xFFD8D8E4, 0xFF1F4FA0, 0xFF20202C, 0xFFBBD2FF}},
    {"MONO GREEN", {0xFF000800, 0xFF001000, 0xFF2C6A2C, 0xFF9CFF9C, 0xFF58B058, 0xFF9CFF9C, 0xFF9CFF9C, 0xFFD8FFD8, 0xFFD8FFD8, 0xFF9CFF9C, 0xFFD8FFD8, 0xFF003000, 0xFF0C5A0C}},
};
#define PX_THEME_COUNT ((int)(sizeof px_themes / sizeof *px_themes))
static uint32_t px_pal[PXC_COUNT] = {0xFF101018, 0xFF181824, 0xFF3C3C58, 0xFFD8D8E0, 0xFF8C8CA0, 0xFF70D0F0, 0xFF60E080, 0xFFF0D060, 0xFFFF7070, 0xFFE8E8E8, 0xFF40B0FF, 0xFF202030, 0xFF2850A0};
static int px_theme_now;
static inline void px_theme_apply(int i) {
    if (i < 0 || i >= PX_THEME_COUNT) i = 0;
    for (int k = 0; k < PXC_COUNT; k++) px_pal[k] = px_themes[i].c[k];
    px_theme_now = i;
}
static inline int px_theme_find(const char *name) {
    for (int i = 0; i < PX_THEME_COUNT && name; i++) if (!strcmp(px_themes[i].name, name)) return i;
    return -1;
}
#define C_BG (px_pal[PXC_BG])
#define C_PANEL (px_pal[PXC_PANEL])
#define C_FRAME (px_pal[PXC_FRAME])
#define C_TEXT (px_pal[PXC_TEXT])
#define C_DIM (px_pal[PXC_DIM])
#define C_HEAD (px_pal[PXC_HEAD])
#define C_OK (px_pal[PXC_OK])
#define C_WARN (px_pal[PXC_WARN])
#define C_ERR (px_pal[PXC_ERR])
#define C_KEYUP (px_pal[PXC_KEYUP])
#define C_KEYDN (px_pal[PXC_KEYDN])
#define C_BLACKKEY (px_pal[PXC_BLACKKEY])
#define C_SEL (px_pal[PXC_SEL])

/* ---- the canvas -------------------------------------------------------------------------------------------------------- */
static inline void px_rect(uint32_t *c, PxRect r, uint32_t col) {
    for (int y = r.y < 0 ? 0 : r.y; y < r.y + r.h && y < PX_H; y++)
        for (int x = r.x < 0 ? 0 : r.x; x < r.x + r.w && x < PX_W; x++) c[y * PX_W + x] = col;
}
static inline void px_frame(uint32_t *c, PxRect r, uint32_t col) {
    px_rect(c, (PxRect){r.x, r.y, r.w, 1}, col);
    px_rect(c, (PxRect){r.x, r.y + r.h - 1, r.w, 1}, col);
    px_rect(c, (PxRect){r.x, r.y, 1, r.h}, col);
    px_rect(c, (PxRect){r.x + r.w - 1, r.y, 1, r.h}, col);
}

/* ---- the font: 5x7 in a 6x8 cell, original; lowercase draws as capitals --------------------------------------------------- */
typedef struct { char ch; const char *rows; } PxGlyph;   /* seven rows of five columns, '#' set, '/' between rows */
static const PxGlyph px_glyphs[] = {
    {'A', ".###./#...#/#...#/#####/#...#/#...#/#...#"}, {'B', "####./#...#/#...#/####./#...#/#...#/####."},
    {'C', ".###./#...#/#..../#..../#..../#...#/.###."}, {'D', "####./#...#/#...#/#...#/#...#/#...#/####."},
    {'E', "#####/#..../#..../####./#..../#..../#####"}, {'F', "#####/#..../#..../####./#..../#..../#...."},
    {'G', ".###./#...#/#..../#.###/#...#/#...#/.###."}, {'H', "#...#/#...#/#...#/#####/#...#/#...#/#...#"},
    {'I', ".###./..#../..#../..#../..#../..#../.###."}, {'J', "..###/...#./...#./...#./...#./#..#./.##.."},
    {'K', "#...#/#..#./#.#../##.../#.#../#..#./#...#"}, {'L', "#..../#..../#..../#..../#..../#..../#####"},
    {'M', "#...#/##.##/#.#.#/#.#.#/#...#/#...#/#...#"}, {'N', "#...#/##..#/#.#.#/#..##/#...#/#...#/#...#"},
    {'O', ".###./#...#/#...#/#...#/#...#/#...#/.###."}, {'P', "####./#...#/#...#/####./#..../#..../#...."},
    {'Q', ".###./#...#/#...#/#...#/#.#.#/#..#./.##.#"}, {'R', "####./#...#/#...#/####./#.#../#..#./#...#"},
    {'S', ".####/#..../#..../.###./....#/....#/####."}, {'T', "#####/..#../..#../..#../..#../..#../..#.."},
    {'U', "#...#/#...#/#...#/#...#/#...#/#...#/.###."}, {'V', "#...#/#...#/#...#/#...#/#...#/.#.#./..#.."},
    {'W', "#...#/#...#/#...#/#.#.#/#.#.#/##.##/#...#"}, {'X', "#...#/#...#/.#.#./..#../.#.#./#...#/#...#"},
    {'Y', "#...#/#...#/.#.#./..#../..#../..#../..#.."}, {'Z', "#####/....#/...#./..#../.#.../#..../#####"},
    {'0', ".###./#...#/#..##/#.#.#/##..#/#...#/.###."}, {'1', "..#../.##../..#../..#../..#../..#../.###."},
    {'2', ".###./#...#/....#/...#./..#../.#.../#####"}, {'3', "####./....#/....#/.###./....#/....#/####."},
    {'4', "...#./..##./.#.#./#..#./#####/...#./...#."}, {'5', "#####/#..../####./....#/....#/#...#/.###."},
    {'6', ".###./#..../#..../####./#...#/#...#/.###."}, {'7', "#####/....#/...#./..#../.#.../.#.../.#..."},
    {'8', ".###./#...#/#...#/.###./#...#/#...#/.###."}, {'9', ".###./#...#/#...#/.####/....#/....#/.###."},
    {'.', "...../...../...../...../...../.##../.##.."}, {',', "...../...../...../...../.##../..#../.#..."},
    {':', "...../.##../.##../...../.##../.##../....."}, {';', "...../.##../.##../...../.##../..#../.#..."},
    {'-', "...../...../...../#####/...../...../....."}, {'_', "...../...../...../...../...../...../#####"},
    {'+', "...../..#../..#../#####/..#../..#../....."}, {'=', "...../...../#####/...../#####/...../....."},
    {'/', "....#/....#/...#./..#../.#.../#..../#...."}, {'\\', "#..../#..../.#.../..#../...#./....#/....#"},
    {'(', "...#./..#../.#.../.#.../.#.../..#../...#."}, {')', ".#.../..#../...#./...#./...#./..#../.#..."},
    {'[', ".###./.#.../.#.../.#.../.#.../.#.../.###."}, {']', ".###./...#./...#./...#./...#./...#./.###."},
    {'<', "...#./..#../.#.../#..../.#.../..#../...#."}, {'>', ".#.../..#../...#./....#/...#./..#../.#..."},
    {'!', "..#../..#../..#../..#../..#../...../..#.."}, {'?', ".###./#...#/....#/...#./..#../...../..#.."},
    {'%', "##..#/##..#/...#./..#../.#.../#..##/#..##"}, {'#', ".#.#./.#.#./#####/.#.#./#####/.#.#./.#.#."},
    {'*', "...../#.#.#/.###./#####/.###./#.#.#/....."}, {'\'', "..#../..#../.#.../...../...../...../....."},
    {'"', ".#.#./.#.#./...../...../...../...../....."}, {'|', "..#../..#../..#../..#../..#../..#../..#.."},
    {'^', "..#../.#.#./#...#/...../...../...../....."}, {'~', "...../.##.#/#.##./...../...../...../....."},
    {'@', ".###./#...#/#.###/#.#.#/#.###/#..../.###."}, {'&', ".##../#..#./#.#../.#.../#.#.#/#..#./.##.#"},
};
#define PX_NGLYPHS ((int)(sizeof px_glyphs / sizeof px_glyphs[0]))

/* The glyph's 35 bits (row-major, bit 34 = top left), or 0 when none (a space, an unknown byte draws as '?'). */
static inline uint64_t px_glyph_bits(int ch) {
    if (ch >= 'a' && ch <= 'z') ch -= 32;
    if (ch == ' ') return 0;
    const char *rows = NULL;
    for (int i = 0; i < PX_NGLYPHS && !rows; i++) if (px_glyphs[i].ch == ch) rows = px_glyphs[i].rows;
    if (!rows) for (int i = 0; i < PX_NGLYPHS; i++) if (px_glyphs[i].ch == '?') rows = px_glyphs[i].rows;
    uint64_t bits = 0;
    for (const char *p = rows; *p; p++) if (*p != '/') bits = (bits << 1) | (*p == '#');
    return bits;
}

enum { PX_CELL_W = 6, PX_CELL_H = 8 };

/* Draws text at (x, y) with each font pixel `scale` screen pixels; stops at max_w pixels (0: no limit). Returns the x after it. */
static inline int px_text(uint32_t *c, int x, int y, const char *s, uint32_t col, int scale, int max_w) {
    int x0 = x;
    for (; *s; s++) {
        if (max_w && x + PX_CELL_W * scale > x0 + max_w) break;
        uint64_t b = px_glyph_bits((unsigned char)*s);
        for (int i = 0; i < 35; i++)
            if (b >> (34 - i) & 1)
                px_rect(c, (PxRect){x + (i % 5) * scale, y + (i / 5) * scale, scale, scale}, col);
        x += PX_CELL_W * scale;
    }
    return x;
}
static inline int px_text_w(const char *s, int scale) { return (int)strlen(s) * PX_CELL_W * scale; }

/* A package's UI page (core fm1ntt_ui_op, a display list) painted into the FM-1's 240 x 240 screen (0x00RRGGBB). Rectangles and discs are the core's own (ui::rasterize paints the
 * same pixels: the host's self-test compares them); text is this file's 5 x 7 font at the op's scale, where the core's raster has solid cells. */
typedef struct { int kind, x, y, w, h, size; uint32_t color; char text[65]; } PxUiOp;
static inline void px_paint_ui(uint32_t *fb, const PxUiOp *ops, int n) {
    static uint32_t cv[PX_W * PX_H];
    for (int i = 0; i < n; i++) {
        const PxUiOp *o = &ops[i];
        uint32_t col = 0xFF000000u | o->color;
        if (o->kind == 0) px_rect(cv, (PxRect){o->x, o->y, o->w, o->h}, col);
        else if (o->kind == 1) {
            for (int y = o->y - o->w; y <= o->y + o->w; y++)
                for (int x = o->x - o->w; x <= o->x + o->w; x++)
                    if (x >= 0 && x < FM_W && y >= 0 && y < FM_H && (x - o->x) * (x - o->x) + (y - o->y) * (y - o->y) <= o->w * o->w) cv[y * PX_W + x] = col;
        } else if (o->kind == 2) px_text(cv, o->x, o->y, o->text, col, o->size < 1 ? 1 : o->size, 0);
    }
    for (int y = 0; y < FM_H; y++) for (int x = 0; x < FM_W; x++) fb[y * FM_W + x] = cv[y * PX_W + x] & 0xFFFFFFu;
}

/* Word-wrapped text in a column w pixels wide, at most max_lines lines of 10 px (a long word is broken); the rest is cut with "...". */
static inline void px_wrap(uint32_t *c, int x, int y, const char *s, uint32_t col, int w, int max_lines) {
    int per = w / PX_CELL_W, line = 0;
    if (per < 4) return;
    while (*s && line < max_lines) {
        while (*s == ' ') s++;
        int n = (int)strlen(s), take = n <= per ? n : per;
        if (n > per) { int b = take; while (b > 0 && s[b] != ' ') b--; if (b > 0) take = b; }
        char buf[128];
        int len = take < (int)sizeof buf - 4 ? take : (int)sizeof buf - 4;
        memcpy(buf, s, (size_t)len);
        buf[len] = 0;
        s += take;
        while (*s == ' ') s++;
        if (*s && line == max_lines - 1) { if (len > 3) strcpy(buf + (len > per - 3 ? per - 3 : len), "..."); }
        px_text(c, x, y + line * 10, buf, col, 1, 0);
        line++;
    }
}

/* ---- what the panels show ---------------------------------------------------------------------------------------------- */
enum { PANEL_MACHINE, PANEL_FILES, PANEL_CPU, PANEL_MEMORY, PANEL_CONSOLE, PANEL_MIDI, PANEL_SOUND, PANEL_MONITOR, PANEL_TOOLS, PANEL_COUNT };
static const char *const PX_PANEL_NAMES[PANEL_COUNT] = {"MACHINE", "FILES", "CPU", "MEMORY", "CONSOLE", "MIDI", "SOUND", "MONITOR", "TOOLS"};
static const char *const PX_BUTTON_NAMES[14] = {"OCT-", "OCT+", "FX", "SCL", "ENV", "LFO", "EDIT", "GLO", "HOME", "SAVE", "ARP", "SEQ", "PLAY", "REC"};
static const char *const PX_ENCODER_NAMES[7] = {"SELECT", "ALGORITHM", "KNOB1", "KNOB2", "KNOB3", "KNOB4", "PRESETS"};
static const char *const PX_REGION_NAMES[3] = {"RAM", "XIP IMAGE", "FLASH"};

enum { ED_ROWS = 24 };   /* the instrument editor's rows on a page */
enum { PX_NAME = 48, PX_MAX_FILES = 64, PX_MAX_TOOLS = 16, PX_SCOPE = 240, PX_MEM_BYTES = 256, PX_CONSOLE_BYTES = 1024, PX_MIDI_LOG = 48, PX_MENU_MAX = 12 };

typedef struct { uint32_t pc, sp, flags, regs[16]; uint64_t insns; } PxCpu;

typedef struct {
    int panel;                          /* PANEL_* */
    /* the machine */
    const char *fw_name;                /* "" when none */
    const char *fw_board;               /* "Fm1", "Smk37", "" */
    const char *run_state;              /* "NO FIRMWARE", "RUN", "PAUSED", "HALT" */
    uint64_t ms;                        /* emulated milliseconds since boot */
    uint64_t down;                      /* matrix ids held: bits 0..13 buttons, 14..40 note keys */
    int enc_turns[7];
    int pot;                            /* 0..1023 */
    const char *msg;                    /* the last thing that happened */
    int msg_style;                      /* 0 info, 1 ok, 2 warn, 3 error */
    uint32_t fb[FM_W * FM_H];           /* the FM-1's screen, 0x00RRGGBB */
    int fb_valid;
    /* files: the firmware folder's entries (Quit is a TOOLS row) */
    char file_dir[96];
    char bundled_dir[96];               /* where the bundled firmware is, "" when none */
    char pack_dir[96];                  /* where the copyleft pack is, "" when it is not installed */
    char files[PX_MAX_FILES + 1][PX_NAME];
    int nfiles, file_sel;               /* the files only */
    /* cpu */
    PxCpu cpu[2];
    int ncpu, cpu_sel;
    double ips, speed;                  /* instructions a second, emulated time over real time */
    int jit, pace;
    /* memory */
    int mem_region;
    uint32_t mem_base, mem_size, mem_off;
    uint8_t mem[PX_MEM_BYTES];
    int mem_n;
    /* console */
    char console[PX_CONSOLE_BYTES + 1];
    int console_n;
    /* midi */
    int midi_chan, midi_note, midi_vel, midi_held;
    uint8_t midi_log[PX_MIDI_LOG];
    int midi_log_n;
    uint64_t midi_queued, midi_dropped;
    int midi_in_devs;                 /* USB MIDI inputs open (-1: no reader) */
    uint64_t midi_in_got;             /* messages received from them */
    /* sound */
    int level_l, level_r, peak_l, peak_r;
    int16_t scope[PX_SCOPE];
    int rec_on;
    int pv_factory;                    /* 0 the firmware's voices, 1 the built-in voices, 2 an instrument package */
    int pv_solo;                       /* the machine is stopped, only the engine plays */
    int pv_on, pv_n, pv_sel, pv_oct;   /* the preview: on, voices found, the chosen one, the octave */
    char pv_name[PX_NAME];
    /* the instrument editor (SOUND, B on a loaded instrument): one page of the group's parameters, the chosen row marked */
    int ed_on, ed_n, ed_sel, ed_page, ed_pages, ed_total, ed_dirty, ed_group, ed_groups;
    char ed_title[PX_NAME], ed_inst[PX_NAME];
    struct { char name[PX_NAME]; int val, min, max; } ed_row[ED_ROWS];
    uint64_t rec_frames;
    char rec_file[PX_NAME];
    /* tools */
    char tools[PX_MAX_TOOLS][PX_NAME];
    int ntools, tool_sel;
    char confirm[96];                   /* a question awaiting A or B, "" when none */
    /* the overlay menus (the main menu, the context menu, About, the colour theme picker): kind 0 none, 1 a list (labels, values at the right), 2 a box of text lines */
    int menu_kind, menu_n, menu_sel;
    char menu_title[PX_NAME], menu_foot[PX_NAME];
    char menu_label[PX_MENU_MAX][PX_NAME], menu_value[PX_MENU_MAX][PX_NAME];
    /* monitor: the host's numbers (monitor.h), the page, and the data rain (off by default, opt-in from TOOLS) */
    MonView mon;
    MonRain rain;
    int mon_page, mon_paused, rain_on, rain_full, rain_still;
    /* the firmware showcase (showcase.h): the demo being made (a box on FILES), the firmware's own page, and the demo playing */
    char demo_run[PX_NAME];             /* the firmware a background demo is being made of, "" when none */
    int demo_pct, demo_secs, demo_paused;
    int info_on;                        /* the page of the picked firmware replaces the list */
    char info_name[PX_NAME], info_tier[40], info_licence[104], info_source[124], info_sha[65], info_demo[104], info_port[PX_NAME + 24];
    int info_port_ok;                 /* a package made from this firmware is in FILES: Right loads it */
    int info_demo_kind, info_pct, info_busy, info_can_play;   /* kind: 0 none yet, 1 being made, 2 ready, 3 failed; busy: a demo of THIS firmware is being made */
    int playing;                        /* a demo is playing: the whole window is the player (the picture at 2x, flush top left, and this column) */
    int pl_tick, pl_ticks, pl_idx, pl_nf;
    char pl_step[32], pl_name[PX_NAME], pl_tier[40], pl_licence[104];
    uint64_t pl_keys;                   /* the matrix bits the tour holds now */
    int cmp_on, cmp_side, cmp_step, cmp_nsteps, cmp_peak_a, cmp_peak_b, cmp_diff;   /* compare mode: the two demos on the same notes (the player's window, another column) */
    long cmp_first_ms;                  /* where the two sounds first differ, -1 when they never do */
    char cmp_name_a[PX_NAME], cmp_name_b[PX_NAME];
    char app_root[128], app_root_why[64];   /* the app's folder (where everything is written) and which rule chose it */
} UiState;

static inline void px_box(uint32_t *c, PxRect r, const char *title) {
    px_rect(c, r, C_PANEL);
    px_frame(c, r, C_FRAME);
    if (title) {
        px_rect(c, (PxRect){r.x + 6, r.y, px_text_w(title, 1) + 6, 1}, C_PANEL);
        px_text(c, r.x + 9, r.y - 3, title, C_HEAD, 1, 0);
    }
}

/* The 27 note keys F3..G5: 16 white keys with black keys over them. Matrix id of key k is 14 + k. */
static inline void px_keyboard(uint32_t *c, PxRect r, uint64_t down) {
    static const int white_of[27] = {0, -1, 1, -1, 2, 3, -1, 4, -1, 5, -1, 6, 7, -1, 8, -1, 9, 10, -1, 11, -1, 12, -1, 13, 14, -1, 15};
    int ww = r.w / 16, bh = r.h * 6 / 10;
    for (int k = 0; k < 27; k++)
        if (white_of[k] >= 0) px_rect(c, (PxRect){r.x + white_of[k] * ww, r.y, ww - 1, r.h}, down >> (14 + k) & 1 ? C_KEYDN : C_KEYUP);
    for (int k = 0; k < 27; k++)
        if (white_of[k] < 0) {
            int left = k > 0 ? white_of[k - 1] : 0;
            px_rect(c, (PxRect){r.x + left * ww + ww * 2 / 3, r.y, ww * 2 / 3, bh}, down >> (14 + k) & 1 ? C_KEYDN : C_BLACKKEY);
        }
}

static inline void px_meter(uint32_t *c, PxRect r, int percent, uint32_t col) {
    px_rect(c, r, C_BG);
    int n = percent < 0 ? 0 : percent > 100 ? 100 : percent;
    px_rect(c, (PxRect){r.x, r.y, (r.w - 2) * n / 100, r.h}, col);
    px_frame(c, r, C_FRAME);
}

static inline uint32_t px_style(int s) { return s == 1 ? C_OK : s == 2 ? C_WARN : s == 3 ? C_ERR : C_TEXT; }

static inline void px_buttons(uint32_t *c, PxRect r, int per_row, uint64_t down) {
    int cw = r.w / per_row, rows = (14 + per_row - 1) / per_row, ch = r.h / rows;
    for (int i = 0; i < 14; i++) {
        PxRect b = {r.x + (i % per_row) * cw, r.y + (i / per_row) * ch, cw - 3, ch - 3};
        int on = (int)(down >> i & 1);
        px_rect(c, b, on ? C_KEYDN : C_BG);
        px_frame(c, b, C_FRAME);
        px_text(c, b.x + (b.w - px_text_w(PX_BUTTON_NAMES[i], 1)) / 2, b.y + (b.h - 7) / 2, PX_BUTTON_NAMES[i], on ? C_BG : C_TEXT, 1, b.w);
    }
}

static inline const char *px_state_of(const UiState *u) { return u->run_state ? u->run_state : "-"; }
static inline uint32_t px_state_col(const UiState *u) {
    const char *s = px_state_of(u);
    return !strcmp(s, "HALT") ? C_ERR : !strcmp(s, "RUN") ? C_OK : C_WARN;
}

/* A page of a list, no scrolling: `per` rows; the row `sel` is highlighted and its page shown. Returns the page count; *page is this page (0-based). */
static inline int px_list(uint32_t *c, PxRect r, const char (*items)[PX_NAME], int n, int sel, int per, int *page) {
    int pages = n > 0 ? (n + per - 1) / per : 1, pg = sel >= 0 && n > 0 ? sel / per : 0;
    if (page) *page = pg;
    for (int i = 0; i < per && pg * per + i < n; i++) {
        int idx = pg * per + i, y = r.y + i * 12;
        if (idx == sel) px_rect(c, (PxRect){r.x - 2, y - 2, r.w + 4, 11}, C_SEL);
        px_text(c, r.x + 2, y, items[idx], C_TEXT, 1, r.w - 4);
    }
    return pages;
}

static inline const char *px_note_name(int n, char *buf) {
    static const char *const nm[12] = {"C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"};
    snprintf(buf, 8, "%s%d", nm[((n % 12) + 12) % 12], n / 12 - 1);
    return buf;
}

/* ---- the carousel (TinyNTT's): the panel before with L1, the current in the middle, the one after with R1 -------------------- */
static inline void px_carousel(uint32_t *c, int mode, int cur) {
    PxRect r = px_carousel_rect(mode);
    px_rect(c, r, C_BG);
    const char *prev = PX_PANEL_NAMES[(cur + PANEL_COUNT - 1) % PANEL_COUNT], *next = PX_PANEL_NAMES[(cur + 1) % PANEL_COUNT], *now = PX_PANEL_NAMES[cur];
    char buf[40];
    int mid_w = px_text_w(now, 1) + 16;
    if (mode == MODE_STANDARD) {
        snprintf(buf, sizeof buf, "< L1 %s", prev);
        px_text(c, r.x + 6, r.y + 5, buf, C_DIM, 1, (r.w - mid_w) / 2 - 8);
        snprintf(buf, sizeof buf, "%s R1 >", next);
        px_text(c, r.x + r.w - 6 - px_text_w(buf, 1), r.y + 5, buf, C_DIM, 1, (r.w - mid_w) / 2 - 8);
    } else {
        px_text(c, r.x + 4, r.y + 5, "<", C_DIM, 1, 0);
        px_text(c, r.x + r.w - 4 - 6, r.y + 5, ">", C_DIM, 1, 0);
    }
    PxRect m = {r.x + (r.w - mid_w) / 2, r.y + 1, mid_w, 16};
    px_rect(c, m, C_SEL);
    px_text(c, m.x + 8, m.y + 4, now, C_TEXT, 1, 0);
}

/* What the buttons do, per panel: the area below the FM-1's screen in Standard. */
static const char *const PX_HINTS[PANEL_COUNT][9] = {
    {"PAD ON MACHINE", "D-PAD A B X Y  NOTES", "SELECT START  OCT- OCT+", "STICKS KNOBS  R1+PAD OCT KEY", "L1  OP1-6 PIT GLO MONO POLY", "L1 STICKS MASTER PRESETS...", "L2  FX HOME SAVE ENV ARP SEQ", "R2  GLO EDIT PLAY REC", "L1+R1 MENU  L2+R2 CONTEXT"},
    {"THE FIRMWARE FOLDER", "UP DOWN  PICK", "R1 UP DOWN  PAGE", "R1 LEFT RIGHT  FIRST LAST", "A  OPEN ITS PAGE", "X  RESCAN THE FOLDER", "ON A PAGE: A  LOAD IT", "X  PLAY ITS DEMO  Y  MAKE IT", "RIGHT  LOAD ITS PORT  B  BACK"},
    {"BOTH CORES", "UP DOWN  CORE 0 1", "LEFT RIGHT  CORE 0 1", "", "", "", "", "", ""},
    {"RAM, XIP IMAGE, FLASH", "UP DOWN  ONE ROW", "R1 UP DOWN  PAGE", "R1 LEFT RIGHT  START END", "LEFT RIGHT  RAM XIP FLASH", "X Y  64 KIB BACK FORTH", "", "", ""},
    {"THE DEBUG BOX", "WHAT THE FIRMWARE", "WRITES TO IT", "", "", "", "", "", ""},
    {"SEND TO THE MIDI JACK", "A  NOTE ON, THEN OFF", "LEFT RIGHT  NOTE", "UP DOWN  OCTAVE", "X Y  VELOCITY", "SELECT START  CHANNEL", "B  ALL NOTES OFF", "", ""},
    {"SCOPE, RECORDER, PREVIEW", "A  RECORD WAV", "X  SAVE SCREENSHOT", "Y  CLEAR PEAKS", "START  PREVIEW ON OR OFF", "SELECT  SOLO ON OR OFF", "UP DOWN VOICE  LEFT RIGHT OCT", "Y B A X  PLAY (PREVIEW)", "B  EDITOR (PREVIEW OFF)"},
    {"THIS DEVICE, NOT THE FM-1", "R1 UP DOWN  PAGE", "X  SORT CPU MEM PID", "Y  FREEZE OR RESUME", "A BAR NEVER LIES: THE", "NUMBER AND ! * SAY IT TOO", "TOOLS: DATA RAIN", "", ""},
    {"RESET, SLOTS, JIT, PANIC", "UP DOWN  PICK", "LEFT RIGHT  SLOT", "A  RUN", "B  NO TO A QUESTION", "", "", "", ""},
};

/* A hint row at the foot of a rectangle: what the buttons do. */
static inline void px_hint(uint32_t *c, PxRect r, const char *s) { px_text(c, r.x + 8, r.y + r.h - 12, s, C_DIM, 1, r.w - 16); }

/* ---- the panels --------------------------------------------------------------------------------------------------------- */
static inline void px_panel_machine(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    char line[96];
    int x = r.x + 8, w = r.w - 16, has_fw = u->fw_name && u->fw_name[0];
    if (!narrow) {
        x = r.x + 12; w = r.w - 24;
        px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, 84}, "MACHINE");
        snprintf(line, sizeof line, "FIRMWARE %s", has_fw ? u->fw_name : "(NONE)");
        px_text(c, x, r.y + 18, line, has_fw ? C_TEXT : C_WARN, 1, w);
        snprintf(line, sizeof line, "BOARD    %s", u->fw_board && u->fw_board[0] ? u->fw_board : "-");
        px_text(c, x, r.y + 32, line, C_TEXT, 1, w);
        snprintf(line, sizeof line, "STATE    %s", px_state_of(u));
        px_text(c, x, r.y + 46, line, px_state_col(u), 1, w);
        snprintf(line, sizeof line, "TIME     %llu.%03llu S", (unsigned long long)(u->ms / 1000), (unsigned long long)(u->ms % 1000));
        px_text(c, x, r.y + 60, line, C_TEXT, 1, w);
        px_text(c, x, r.y + 74, "SOC      AC791N 2X PI32V2", C_DIM, 1, w);
        px_box(c, (PxRect){r.x + 4, r.y + 102, r.w - 8, 48}, "BUTTONS");
        px_buttons(c, (PxRect){r.x + 10, r.y + 112, r.w - 20, 34}, 7, u->down);
        px_box(c, (PxRect){r.x + 4, r.y + 158, r.w - 8, 74}, "KEYS F3-G5");
        px_keyboard(c, (PxRect){r.x + 12, r.y + 170, r.w - 24, 54}, u->down);
        px_box(c, (PxRect){r.x + 4, r.y + 240, r.w - 8, 88}, "ENCODERS AND POT");
        for (int e = 0; e < 7; e++) {
            snprintf(line, sizeof line, "%-9s %+d", PX_ENCODER_NAMES[e], u->enc_turns[e]);
            px_text(c, x + (e < 4 ? 0 : w / 2), r.y + 252 + (e < 4 ? e : e - 4) * 12, line, C_TEXT, 1, w / 2 - 6);
        }
        px_text(c, x + w / 2, r.y + 300, "POT", C_TEXT, 1, 0);
        px_meter(c, (PxRect){x + w / 2 + 24, r.y + 299, w / 2 - 30, 8}, u->pot * 100 / 1023, C_HEAD);
        px_box(c, (PxRect){r.x + 4, r.y + 336, r.w - 8, 44}, "SOUND");
        px_text(c, x, r.y + 348, "L", C_DIM, 1, 0);
        px_meter(c, (PxRect){x + 12, r.y + 347, w - 12, 8}, u->level_l, C_OK);
        px_text(c, x, r.y + 362, "R", C_DIM, 1, 0);
        px_meter(c, (PxRect){x + 12, r.y + 361, w - 12, 8}, u->level_r, C_OK);
        px_box(c, (PxRect){r.x + 4, r.y + 388, r.w - 8, 56}, "LOG");
        px_wrap(c, x, r.y + 400, u->msg ? u->msg : "", px_style(u->msg_style), w, 4);
    } else {
        px_box(c, (PxRect){r.x + 2, r.y + 8, r.w - 4, 76}, "STATE");
        px_text(c, x, r.y + 20, has_fw ? u->fw_name : "(NO FIRMWARE)", has_fw ? C_TEXT : C_WARN, 1, w);
        px_text(c, x, r.y + 34, px_state_of(u), px_state_col(u), 1, w);
        snprintf(line, sizeof line, "%llu.%01llu S", (unsigned long long)(u->ms / 1000), (unsigned long long)(u->ms % 1000 / 100));
        px_text(c, x, r.y + 48, line, C_TEXT, 1, w);
        px_text(c, x, r.y + 62, "AC791N", C_DIM, 1, w);
        px_box(c, (PxRect){r.x + 2, r.y + 92, r.w - 4, 44}, "SOUND");
        px_meter(c, (PxRect){x, r.y + 104, w, 8}, u->level_l, C_OK);
        px_meter(c, (PxRect){x, r.y + 118, w, 8}, u->level_r, C_OK);
        px_box(c, (PxRect){r.x + 2, r.y + 144, r.w - 4, 74}, "KEYS");
        px_keyboard(c, (PxRect){x, r.y + 156, w, 54}, u->down);
        px_box(c, (PxRect){r.x + 2, r.y + 226, r.w - 4, 62}, "BUTTONS");
        px_buttons(c, (PxRect){x, r.y + 238, w, 46}, 4, u->down);
        px_box(c, (PxRect){r.x + 2, r.y + 296, r.w - 4, 120}, "LOG");
        px_wrap(c, x, r.y + 308, u->msg ? u->msg : "", px_style(u->msg_style), w, 10);
    }
}

/* The firmware's own page: what it is, where it came from, its demo; then what the buttons do. Whole page, nothing scrolls, and it names no file of the app's own. */
static inline void px_panel_info(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    int x = r.x + 10, w = r.w - 20, y = r.y + 14;
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, narrow ? 330 : 290}, "FIRMWARE");
    px_wrap(c, x, y, u->info_name, C_TEXT, w, 2);
    y += narrow ? 22 : 12;
    px_text(c, x, y, "TIER", C_HEAD, 1, 0);
    if (narrow) { px_wrap(c, x, y + 10, u->info_tier, C_TEXT, w, 2); y += 34; }
    else { px_text(c, x + 42, y, u->info_tier, C_TEXT, 1, w - 42); y += 14; }
    px_text(c, x, y, "LICENCE", C_HEAD, 1, 0);
    px_wrap(c, x, y + 10, u->info_licence, C_TEXT, w, 3);
    y += 44;
    px_text(c, x, y, "SOURCE", C_HEAD, 1, 0);
    px_wrap(c, x, y + 10, u->info_source, C_TEXT, w, 4);
    y += narrow ? 54 : 44;
    px_text(c, x, y, "SHA-256", C_HEAD, 1, 0);
    char half[40];
    for (int k = 0; k < 2; k++) { snprintf(half, sizeof half, "%.32s", u->info_sha[0] ? u->info_sha + k * 32 : "-"); px_text(c, x, y + 10 + k * 10, narrow ? "" : half, C_DIM, 1, w); }
    if (narrow) { for (int k = 0; k < 4; k++) { snprintf(half, sizeof half, "%.16s", u->info_sha[0] ? u->info_sha + k * 16 : "-"); px_text(c, x, y + 10 + k * 10, half, C_DIM, 1, w); } y += 20; }
    y += 36;
    px_text(c, x, y, "DEMO", C_HEAD, 1, 0);
    uint32_t col = u->info_demo_kind == 2 ? C_OK : u->info_demo_kind == 3 ? C_ERR : u->info_demo_kind == 1 ? C_HEAD : C_DIM;
    px_wrap(c, x, y + 10, u->info_demo, col, w, 4);
    if (u->info_demo_kind == 1) px_meter(c, (PxRect){x, y + 44, w, 8}, u->info_pct, C_HEAD);
    px_text(c, x, y + 58, "PORT", C_HEAD, 1, 0);
    px_wrap(c, x, y + 68, u->info_port, u->info_port_ok ? C_OK : C_DIM, w, 2);
    PxRect b = {r.x + 4, r.y + (narrow ? 346 : 306), r.w - 8, 60};
    px_box(c, b, NULL);
    px_text(c, x, b.y + 8, "A  LOAD IT", C_TEXT, 1, w);
    px_text(c, x, b.y + 19, u->info_can_play ? "X  PLAY THE DEMO" : "X  NO DEMO YET", u->info_can_play ? C_TEXT : C_DIM, 1, w);
    px_text(c, x, b.y + 30, u->info_busy ? "Y  STOP MAKING THE DEMO" : "Y  MAKE THE DEMO NOW", C_TEXT, 1, w);
    px_text(c, x, b.y + 41, u->info_port_ok ? "RIGHT  LOAD ITS PORT" : "RIGHT  NO PORT YET", u->info_port_ok ? C_TEXT : C_DIM, 1, w);
}

static inline void px_panel_files(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    char line[96];
    int x = r.x + 10, w = r.w - 20, per = narrow ? 20 : 22;
    if (u->info_on) { px_panel_info(c, r, u, narrow); return; }
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, 28}, "FOLDER");
    px_text(c, x, r.y + 20, u->file_dir, C_DIM, 1, w);
    int page = 0, pages;
    PxRect lr = {x, r.y + 52, w, per * 12};
    px_box(c, (PxRect){r.x + 4, r.y + 42, r.w - 8, per * 12 + 28}, NULL);
    pages = px_list(c, lr, u->files, u->nfiles, u->file_sel, per, &page);
    snprintf(line, sizeof line, "PAGE %d/%d", page + 1, pages);
    px_text(c, r.x + r.w - 12 - px_text_w(line, 1), r.y + 39, line, C_HEAD, 1, 0);
    if (u->nfiles == 0) px_wrap(c, x, r.y + 60, "NO FIRMWARE FILES (.FWSC OR .BIN) HERE. COPY YOURS TO THE FOLDER, THEN X TO RESCAN.", C_WARN, w, 4);
    {
        const char *foot = u->bundled_dir[0] || u->pack_dir[0]
            ? (narrow ? "B BUNDLED  G GPL PACK" : (u->pack_dir[0] ? "B = BUNDLED (MIT)   G = THE GPL PACK (SOURCE INCLUDED)" : "B = BUNDLED WITH THE APP: SEE NOTICE"))
            : "NO BUNDLED FIRMWARE OR PACK FOUND NEXT TO THE APP";
        px_text(c, x, r.y + 52 + per * 12 + 4, u->bundled_dir[0] ? foot : (u->pack_dir[0] ? (narrow ? "NO BUNDLED FIRMWARE FOUND" : "G = THE GPL PACK. NO BUNDLED FIRMWARE FOUND") : foot), u->bundled_dir[0] ? C_DIM : C_WARN, 1, w);
    }
    int by = r.y + 42 + per * 12 + 34;
    px_box(c, (PxRect){r.x + 4, by, r.w - 8, 56}, "LOADED");
    int has_fw = u->fw_name && u->fw_name[0];
    px_text(c, x, by + 12, has_fw ? u->fw_name : "(NONE)", has_fw ? C_TEXT : C_WARN, 1, w);
    snprintf(line, sizeof line, "BOARD %s", u->fw_board && u->fw_board[0] ? u->fw_board : "-");
    px_text(c, x, by + 24, line, C_DIM, 1, w);
    px_wrap(c, x, by + 36, u->msg ? u->msg : "", px_style(u->msg_style), w, 2);
    if (u->demo_run[0]) {                              /* a demo is being made in the background: step, bar, time (it is paused while you play) */
        PxRect b = {r.x + 4, by + 62, r.w - 8, 46};
        px_box(c, b, u->demo_paused ? "MAKING A DEMO (PAUSED)" : "MAKING A DEMO");
        px_text(c, x, b.y + 10, u->demo_run, C_TEXT, 1, w);
        px_meter(c, (PxRect){x, b.y + 22, w, 8}, u->demo_pct, C_HEAD);
        snprintf(line, sizeof line, "%d%%  %ds", u->demo_pct, u->demo_secs);
        px_text(c, x, b.y + 33, line, u->demo_paused ? C_WARN : C_DIM, 1, w);
    }
}

static inline void px_regs(uint32_t *c, int x, int y, const PxCpu *p, int cols, int colw) {
    char line[48];
    for (int i = 0; i < 16; i++) {
        snprintf(line, sizeof line, "R%-2d %08X", i, (unsigned)p->regs[i]);
        px_text(c, x + (i / (16 / cols)) * colw, y + (i % (16 / cols)) * 11, line, C_TEXT, 1, 0);
    }
}

static inline void px_panel_cpu(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    char line[96];
    int x = r.x + 10, w = r.w - 20;
    snprintf(line, sizeof line, "CORE %d", u->cpu_sel);
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, narrow ? 262 : 168}, line);
    if (u->cpu_sel >= u->ncpu) {
        px_text(c, x, r.y + 20, u->ncpu ? "NOT RELEASED BY THE FIRMWARE YET" : "NO FIRMWARE", C_WARN, 1, w);
    } else {
        const PxCpu *p = &u->cpu[u->cpu_sel];
        int ly = r.y + 20;
        if (narrow) {
            snprintf(line, sizeof line, "PC %08X", (unsigned)p->pc); px_text(c, x, ly, line, C_TEXT, 1, w); ly += 11;
            snprintf(line, sizeof line, "SP %08X", (unsigned)p->sp); px_text(c, x, ly, line, C_TEXT, 1, w); ly += 11;
        } else {
            snprintf(line, sizeof line, "PC %08X  SP %08X", (unsigned)p->pc, (unsigned)p->sp); px_text(c, x, ly, line, C_TEXT, 1, w); ly += 12;
        }
        snprintf(line, sizeof line, "FLAGS %08X", (unsigned)p->flags);
        px_text(c, x, ly, line, C_TEXT, 1, w); ly += narrow ? 11 : 12;
        snprintf(line, sizeof line, "INSNS %llu", (unsigned long long)p->insns);
        px_text(c, x, ly, line, C_TEXT, 1, w); ly += narrow ? 14 : 18;
        if (narrow) px_regs(c, x, ly, p, 1, 0);
        else px_regs(c, x, ly, p, 2, w / 2);
    }
    int sy = r.y + (narrow ? 280 : 184);
    px_box(c, (PxRect){r.x + 4, sy, r.w - 8, narrow ? 74 : 62}, "SPEED");
    snprintf(line, sizeof line, "%.1f M INSNS/S", u->ips / 1e6);
    px_text(c, x, sy + 12, line, C_TEXT, 1, w);
    snprintf(line, sizeof line, "%.2fX REAL TIME", u->speed);
    px_text(c, x, sy + 24, line, u->speed >= 1.0 || !u->ncpu ? C_OK : C_WARN, 1, w);
    if (narrow) {
        snprintf(line, sizeof line, "JIT %s  PACE %d", u->jit ? "ON" : "OFF", u->pace);
        px_text(c, x, sy + 36, line, C_TEXT, 1, w);
        px_text(c, x, sy + 48, px_state_of(u), px_state_col(u), 1, w);
    } else {
        snprintf(line, sizeof line, "JIT %s  PACE %d  STATE %s", u->jit ? "ON" : "OFF", u->pace, px_state_of(u));
        px_text(c, x, sy + 36, line, px_state_col(u), 1, w);
    }
}

static inline void px_panel_memory(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    char line[96];
    int x = r.x + 10, w = r.w - 20, per = narrow ? 4 : 8, rows = (narrow ? PX_MEM_BYTES / 2 : PX_MEM_BYTES) / per;
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, narrow ? 52 : 40}, PX_REGION_NAMES[u->mem_region % 3]);
    if (narrow) {
        snprintf(line, sizeof line, "CHIP %08X", (unsigned)u->mem_base); px_text(c, x, r.y + 20, line, C_TEXT, 1, w);
        snprintf(line, sizeof line, "OFFSET %06X", (unsigned)u->mem_off); px_text(c, x, r.y + 32, line, C_TEXT, 1, w);
        snprintf(line, sizeof line, "SIZE %X", (unsigned)u->mem_size); px_text(c, x, r.y + 44, line, C_DIM, 1, w);
    } else {
        snprintf(line, sizeof line, "CHIP %08X  OFFSET %06X", (unsigned)u->mem_base, (unsigned)u->mem_off);
        px_text(c, x, r.y + 20, line, C_TEXT, 1, w);
        snprintf(line, sizeof line, "SIZE %X BYTES", (unsigned)u->mem_size);
        px_text(c, x, r.y + 32, line, C_DIM, 1, w);
    }
    int top = narrow ? 68 : 56;
    px_box(c, (PxRect){r.x + 4, r.y + top, r.w - 8, rows * 11 + 14}, NULL);
    for (int row = 0; row < rows; row++) {
        int o = row * per;
        if (o >= u->mem_n) break;
        char *p = line;
        { uint32_t ad = u->mem_base + u->mem_off + (uint32_t)o; p += narrow ? snprintf(p, 24, "%06X ", (unsigned)(ad & 0xFFFFFF)) : snprintf(p, 24, "%08X ", (unsigned)ad); }
        for (int i = 0; i < per; i++) p += o + i < u->mem_n ? snprintf(p, 4, "%02X ", u->mem[o + i]) : snprintf(p, 4, "   ");
        for (int i = 0; i < per; i++) { int b = o + i < u->mem_n ? u->mem[o + i] : ' '; *p++ = b >= 32 && b < 127 ? (char)b : '.'; }
        *p = 0;
        px_text(c, x, r.y + top + 7 + row * 11, line, u->mem_n ? C_TEXT : C_DIM, 1, w);
    }
    if (!u->mem_n) px_text(c, x, r.y + top + 7, "NOTHING TO SHOW", C_WARN, 1, w);
}

/* The console's text in lines of `cols` characters, the last `rows` of them. */
static inline void px_panel_console(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    int cols = (r.w - 24) / PX_CELL_W, rows = (r.h - 40) / 10;
    enum { MAXL = 200 };
    static char lines[MAXL][80];
    int n = 0, col = 0;
    if (cols > 78) cols = 78;
    lines[0][0] = 0;
    for (int i = 0; i < u->console_n; i++) {
        unsigned char ch = (unsigned char)u->console[i];
        if (ch == '\r') continue;
        if (ch == '\n' || col >= cols) {
            lines[n][col] = 0;
            if (n < MAXL - 1) n++; else { memmove(lines[0], lines[1], sizeof lines - sizeof lines[0]); }
            col = 0; lines[n][0] = 0;
            if (ch == '\n') continue;
        }
        lines[n][col++] = ch >= 32 && ch < 127 ? (char)ch : '.';
    }
    lines[n][col] = 0;
    n++;
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, r.h - 24}, "FIRMWARE CONSOLE");
    if (!u->console_n) { px_text(c, r.x + 12, r.y + 20, "NOTHING WRITTEN YET", C_DIM, 1, r.w - 24); return; }
    int first = n > rows ? n - rows : 0;
    for (int i = first; i < n; i++) px_text(c, r.x + 12, r.y + 20 + (i - first) * 10, lines[i], C_TEXT, 1, r.w - 24);
    (void)narrow;
}

static inline void px_panel_midi(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    char line[96], nn[8];
    int x = r.x + 10, w = r.w - 20;
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, 64}, "SEND");
    snprintf(line, sizeof line, "CHANNEL  %d", u->midi_chan + 1);
    px_text(c, x, r.y + 20, line, C_TEXT, 1, w);
    snprintf(line, sizeof line, "NOTE     %d  %s", u->midi_note, px_note_name(u->midi_note, nn));
    px_text(c, x, r.y + 32, line, u->midi_held ? C_KEYDN : C_TEXT, 1, w);
    snprintf(line, sizeof line, "VELOCITY %d", u->midi_vel);
    px_text(c, x, r.y + 44, line, C_TEXT, 1, w);
    px_text(c, x, r.y + 56, u->midi_held ? "NOTE ON" : "READY", u->midi_held ? C_OK : C_DIM, 1, w);
    px_box(c, (PxRect){r.x + 4, r.y + 80, r.w - 8, 50}, "THE JACK");
    snprintf(line, sizeof line, "QUEUED   %llu", (unsigned long long)u->midi_queued);
    px_text(c, x, r.y + 92, line, C_TEXT, 1, w);
    snprintf(line, sizeof line, "DROPPED  %llu", (unsigned long long)u->midi_dropped);
    px_text(c, x, r.y + 104, line, u->midi_dropped ? C_WARN : C_TEXT, 1, w);
    px_text(c, x, r.y + 116, "(NO RECEIVER SET UP = DROPPED)", C_DIM, 1, w);
    int per = narrow ? 8 : 16;
    px_box(c, (PxRect){r.x + 4, r.y + 138, r.w - 8, 90}, "SENT BYTES");
    for (int i = 0; i < u->midi_log_n; i++) {
        snprintf(line, sizeof line, "%02X", u->midi_log[i]);
        px_text(c, x + (i % per) * 18, r.y + 150 + (i / per) * 11, line, C_TEXT, 1, 0);
    }
    if (!u->midi_log_n) px_text(c, x, r.y + 150, "NOTHING SENT YET", C_DIM, 1, w);
    px_box(c, (PxRect){r.x + 4, r.y + 236, r.w - 8, 40}, "FROM THE FM-1");
    px_wrap(c, x, r.y + 248, "ITS MIDI OUT IS NOT EXPOSED BY THE CORE YET", C_DIM, w, 2);
    px_box(c, (PxRect){r.x + 4, r.y + 284, r.w - 8, 40}, "USB MIDI IN");
    if (u->midi_in_devs < 0) px_text(c, x, r.y + 296, "NOT AVAILABLE", C_DIM, 1, w);
    else {
        snprintf(line, sizeof line, "DEVICES  %d", u->midi_in_devs);
        px_text(c, x, r.y + 296, line, u->midi_in_devs ? C_OK : C_DIM, 1, w);
        snprintf(line, sizeof line, "RECEIVED %llu", (unsigned long long)u->midi_in_got);
        px_text(c, x, r.y + 308, line, C_TEXT, 1, w);
    }
}

/* The instrument editor: a whole page of the chosen group's parameters (engine, arpeggiator, each effect), the row marked, its value at the right and a bar for where it sits in
 * its range. Nothing scrolls: a long group is more pages. Replaces the SOUND panel while it is on. */
static inline void px_panel_inst_edit(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    char line[96];
    int x = r.x + 10, w = r.w - 20;
    px_box(c, (PxRect){r.x + 4, r.y + 6, r.w - 8, 50}, "INSTRUMENT EDITOR");
    px_text(c, x, r.y + 18, u->ed_inst, u->ed_dirty ? C_WARN : C_TEXT, 1, w - (narrow ? 0 : 60));
    if (!narrow) px_text(c, r.x + r.w - 70, r.y + 18, u->ed_dirty ? "EDITED" : "", C_WARN, 1, 0);
    if (narrow) snprintf(line, sizeof line, "%s %d/%d", u->ed_title, u->ed_group + 1, u->ed_groups);
    else snprintf(line, sizeof line, "%s %d/%d  PAGE %d/%d", u->ed_title, u->ed_group + 1, u->ed_groups, u->ed_page + 1, u->ed_pages);
    px_text(c, x, r.y + 32, line, C_HEAD, 1, w);
    if (narrow) snprintf(line, sizeof line, "%d PARAMS  PAGE %d/%d", u->ed_total, u->ed_page + 1, u->ed_pages);
    else snprintf(line, sizeof line, "%d PARAMETERS", u->ed_total);
    px_text(c, x, r.y + 44, line, C_DIM, 1, w);
    int top = r.y + 62;
    if (!u->ed_n) px_text(c, x, top + 4, "NO PARAMETERS HERE", C_DIM, 1, w);
    for (int i = 0; i < u->ed_n; i++) {
        int y = top + i * 14, sel = i == u->ed_sel;
        if (sel) px_rect(c, (PxRect){r.x + 6, y - 2, r.w - 12, 13}, C_SEL);
        char val[16];
        snprintf(val, sizeof val, "%d", u->ed_row[i].val);
        int vw = (int)strlen(val) * 6;
        px_text(c, r.x + r.w - 10 - vw, y, val, C_TEXT, 1, 0);
        px_text(c, x, y, u->ed_row[i].name, sel ? C_TEXT : C_DIM, 1, w - vw - 8);
        int span = u->ed_row[i].max - u->ed_row[i].min;
        if (span > 0 && !narrow) {   /* where the value sits in its range: a hairline under the name */
            int bw = (w - 8) * (u->ed_row[i].val - u->ed_row[i].min) / span;
            px_rect(c, (PxRect){x, y + 9, bw < 1 ? 1 : bw, 1}, sel ? C_HEAD : C_FRAME);
        }
    }
    int fy = r.y + r.h - 54;
    if (narrow) {
        px_text(c, x, fy, "UP DOWN PICK  LR +-1", C_DIM, 1, w);
        px_text(c, x, fy + 12, "X Y BIG  L2 R2 GROUP", C_DIM, 1, w);
        px_text(c, x, fy + 24, "A SAVE   B LEAVES", C_DIM, 1, w);
    } else {
        px_text(c, x, fy, "UP DOWN PICK   LEFT RIGHT +-1   X Y BIG STEP", C_DIM, 1, w);
        px_text(c, x, fy + 12, "L2 R2 GROUP   A SAVE (INSTRUMENTS FOLDER)   B LEAVES", C_DIM, 1, w);
    }
}

static inline void px_panel_sound(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    if (u->ed_on) { px_panel_inst_edit(c, r, u, narrow); return; }
    char line[96];
    int x = r.x + 10, w = r.w - 20;
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, 104}, "SCOPE");
    PxRect sc = {x, r.y + 18, w, 88};
    px_rect(c, sc, C_BG);
    px_rect(c, (PxRect){sc.x, sc.y + sc.h / 2, sc.w, 1}, C_FRAME);
    int prev = sc.h / 2;
    for (int i = 0; i < sc.w; i++) {
        int v = u->scope[i * PX_SCOPE / sc.w], y = sc.h / 2 - v * (sc.h / 2 - 1) / 32768;
        int lo = y < prev ? y : prev, hi = y < prev ? prev : y;
        px_rect(c, (PxRect){sc.x + i, sc.y + lo, 1, hi - lo + 1}, C_OK);
        prev = y;
    }
    px_box(c, (PxRect){r.x + 4, r.y + 120, r.w - 8, 52}, "LEVEL AND PEAK");
    px_text(c, x, r.y + 132, "L", C_DIM, 1, 0);
    px_meter(c, (PxRect){x + 12, r.y + 131, w - 12, 8}, u->level_l, C_OK);
    px_rect(c, (PxRect){x + 12 + (w - 14) * (u->peak_l > 100 ? 100 : u->peak_l) / 100, r.y + 131, 2, 8}, C_WARN);
    px_text(c, x, r.y + 148, "R", C_DIM, 1, 0);
    px_meter(c, (PxRect){x + 12, r.y + 147, w - 12, 8}, u->level_r, C_OK);
    px_rect(c, (PxRect){x + 12 + (w - 14) * (u->peak_r > 100 ? 100 : u->peak_r) / 100, r.y + 147, 2, 8}, C_WARN);
    px_box(c, (PxRect){r.x + 4, r.y + 180, r.w - 8, 62}, "RECORDER");
    snprintf(line, sizeof line, "%s", u->rec_on ? "RECORDING" : "STOPPED");
    px_text(c, x, r.y + 192, line, u->rec_on ? C_ERR : C_DIM, 1, w);
    unsigned long long f = (unsigned long long)u->rec_frames;
    snprintf(line, sizeof line, "%llu:%02llu.%01llu", f / 44100 / 60, f / 44100 % 60, f * 10 / 44100 % 10);
    px_text(c, x, r.y + 204, line, C_TEXT, 1, w);
    px_text(c, x, r.y + 216, u->rec_file[0] ? u->rec_file : "(NO FILE YET)", C_DIM, 1, w);
    px_box(c, (PxRect){r.x + 4, r.y + 250, r.w - 8, 56}, "LOG");
    px_wrap(c, x, r.y + 262, u->msg ? u->msg : "", px_style(u->msg_style), w, narrow ? 4 : 4);
    /* the preview of the voices in the firmware's banks, played by the fm6 engine from the keys */
    px_box(c, (PxRect){r.x + 4, r.y + 314, r.w - 8, 104}, "PREVIEW");
    if (!u->pv_n) {
        px_wrap(c, x, r.y + 326, "NO VOICES TO PLAY", C_DIM, w, 2);
    } else {
        px_text(c, x, r.y + 326, u->pv_on ? (u->pv_solo ? "ON  SOLO (R2 ENDS)" : "ON  R2: SOLO") : "OFF  L2 TURNS IT ON", u->pv_on ? C_OK : C_DIM, 1, w);
        snprintf(line, sizeof line, "%d/%d %s", u->pv_sel + 1, u->pv_n, u->pv_name);
        px_text(c, x, r.y + 338, line, C_TEXT, 1, w);
        snprintf(line, sizeof line, "OCTAVE %+d  %s", u->pv_oct, u->pv_factory == 2 ? "INSTRUMENT" : u->pv_factory ? "BUILT-IN" : "FIRMWARE");
        px_text(c, x, r.y + 350, line, C_TEXT, 1, w);
        px_wrap(c, x, r.y + 364, narrow ? "UP DOWN VOICE  LEFT RIGHT OCTAVE  Y B A X PLAY" : "UP DOWN: VOICE   LEFT RIGHT: OCTAVE   Y B A X: C E G C (KEYBOARD ZSXDC...)", C_DIM, w, 3);
    }
}

/* ---- MONITOR: a `top` for the machine the app runs on (monitor.h, docs/MONITOR.md). Colour is never alone: the number and a glyph (* amber, ! red) say the
 * same, and a bar's length is the value. An unknown value is a dash, in the dim colour; a box with no source here says n/a. ----------------------------- */
static inline uint32_t px_lv_col(int lv) { return lv == LV_CRIT ? C_ERR : lv == LV_WARN ? C_WARN : C_OK; }
static inline void px_textf(uint32_t *c, int x, int y, uint32_t col, int max_w, const char *fmt, ...) {
    char b[160];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(b, sizeof b, fmt, ap);
    va_end(ap);
    px_text(c, x, y, b, col, 1, max_w);
}
/* "LABEL 54C!" in the verdict's colour; "LABEL -" dim when unknown. th is a TH_* row, or -1 for a plain number. */
static inline int px_mon_num(uint32_t *c, int x, int y, int w, const char *label, int val, const char *unit, int th) {
    char b[64];
    int lv = th >= 0 ? mon_level(th, val) : LV_OK;
    if (val < 0) snprintf(b, sizeof b, "%s -", label);
    else snprintf(b, sizeof b, "%s %d%s%c", label, val, unit, th >= 0 ? mon_glyph(lv) : ' ');
    return px_text(c, x, y, b, val < 0 ? C_DIM : th < 0 ? C_TEXT : px_lv_col(lv), 1, w);
}
static inline void px_mon_hist(uint32_t *c, int x, int y, const uint8_t *h, int n, int sw) {
    px_rect(c, (PxRect){x, y, n * sw, 8}, C_BG);
    for (int i = 0; i < n; i++) {
        int p = h[MON_HIST - n + i], bh = p > 0 ? 1 + p * 7 / 100 : 0;
        if (bh) px_rect(c, (PxRect){x + i * sw, y + 8 - bh, sw, bh}, px_lv_col(mon_level(TH_CPU, p)));
    }
}

static inline int px_mon_cpu(uint32_t *c, PxRect b, const MonView *v, int narrow) {
    int x = b.x + 8, y = b.y + 10, w = b.w - 16, shown = v->ncores < MON_SHOW_CORES ? v->ncores : MON_SHOW_CORES;
    px_box(c, b, "CPU");
    char s[48];
    if (v->has_load) snprintf(s, sizeof s, "LOAD %.2f %.2f %.2f", v->load[0], v->load[1], v->load[2]); else snprintf(s, sizeof s, "LOAD -");
    px_text(c, x, y, s, v->has_load ? C_TEXT : C_DIM, 1, w);
    int xa = narrow ? x : x + 130, ya = narrow ? y + 11 : y;
    px_mon_num(c, xa, ya, 0, "ALL", v->cpu_pct, "%", TH_CPU);
    px_mon_num(c, narrow ? x + 66 : x + 210, narrow ? y + 11 : y, 0, "TEMP", v->temp_c, "C", TH_TEMP);
    int yb = narrow ? y + 22 : y + 11;
    snprintf(s, sizeof s, "GOV %s", v->gov[0] ? v->gov : "-");
    px_text(c, x, yb, s, v->gov[0] ? C_TEXT : C_DIM, 1, narrow ? 80 : 124);
    if (!v->sampled) px_text(c, narrow ? x + 84 : x + 130, yb, "SAMPLING...", C_WARN, 1, 0);
    else px_textf(c, narrow ? x + 84 : x + 130, yb, C_DIM, 0, "%d CORE%s", v->ncores, v->ncores == 1 ? "" : "S");
    int y0 = yb + 13, bw = narrow ? 28 : 130;
    int hn = narrow ? 20 : MON_HIST, sw = narrow ? 2 : 3;
    if (v->ncores == 0) px_text(c, x, y0, v->platform ? "NO CORE COUNTERS" : "N/A: NO HOST SOURCE", C_DIM, 1, w);
    for (int i = 0; i < shown; i++) {
        int ry = y0 + i * 11, pc = v->core_pct[i], bx = x + 20;
        px_textf(c, x, ry, C_DIM, 0, "C%d", i);
        px_meter(c, (PxRect){bx, ry, bw, 8}, pc < 0 ? 0 : pc, px_lv_col(mon_level(TH_CPU, pc)));
        int tx = bx + bw + 4;
        if (pc < 0) px_text(c, tx, ry, "-", C_DIM, 1, 0);
        else px_textf(c, tx, ry, px_lv_col(mon_level(TH_CPU, pc)), 0, "%d%%%c", pc, mon_glyph(mon_level(TH_CPU, pc)));
        int hx = tx + 34;
        if (!narrow) { if (v->core_mhz[i] >= 0) px_textf(c, hx, ry, C_TEXT, 0, "%dM", v->core_mhz[i]); else px_text(c, hx, ry, "-", C_DIM, 1, 0); hx += 36; }
        px_mon_hist(c, hx, ry, v->hist[i], hn, sw);
    }
    if (v->ncores > shown) px_textf(c, x, y0 + shown * 11, C_DIM, w, "+%d MORE CORES", v->ncores - shown);
    return y0 + (shown + (v->ncores > shown)) * 11 + 6 - b.y;       /* the height this box wanted */
}
static inline int px_mon_cpu_h(const MonView *v, int narrow) { int sh = v->ncores < MON_SHOW_CORES ? v->ncores : MON_SHOW_CORES; if (sh < 1) sh = 1; return 10 + (narrow ? 33 : 22) + 13 + (sh + (v->ncores > sh)) * 11 + 6; }

static inline void px_mon_mem(uint32_t *c, PxRect b, const MonView *v) {
    int x = b.x + 8, y = b.y + 10, w = b.w - 16;
    char a[16], t[16];
    px_box(c, b, "MEMORY");
    if (v->mem.has) {
        int pct = (int)(v->mem.used_kb * 100 / (v->mem.total_kb ? v->mem.total_kb : 1)), lv = mon_level(TH_MEM, pct);
        mon_kb_text(a, sizeof a, v->mem.used_kb); mon_kb_text(t, sizeof t, v->mem.total_kb);
        px_textf(c, x, y, px_lv_col(lv), w, "RAM %s/%s %d%%%c", a, t, pct, mon_glyph(lv));
        px_meter(c, (PxRect){x, y + 10, w, 7}, pct, px_lv_col(lv));
        mon_kb_text(a, sizeof a, v->mem.cached_kb); mon_kb_text(t, sizeof t, v->mem.free_kb);
        px_textf(c, x, y + 21, C_TEXT, w, "CACHE %s FREE %s", a, t);
        mon_kb_text(a, sizeof a, v->mem.swap_used_kb); mon_kb_text(t, sizeof t, v->mem.swap_total_kb);
        px_textf(c, x, y + 32, C_TEXT, w, "SWAP %s/%s", a, t);
    } else {
        px_text(c, x, y, "RAM N/A", C_DIM, 1, w);
        px_text(c, x, y + 21, "CACHE N/A", C_DIM, 1, w);
        px_text(c, x, y + 32, "SWAP N/A", C_DIM, 1, w);
    }
    if (v->self_rss_kb > 0) { mon_kb_text(a, sizeof a, (unsigned long long)v->self_rss_kb); px_textf(c, x, y + 43, C_TEXT, w, "APP RSS %s", a); }
    else px_text(c, x, y + 43, "APP RSS N/A", C_DIM, 1, w);
    if (v->app.emu_kb > 0) { mon_kb_text(a, sizeof a, (unsigned long long)v->app.emu_kb); px_textf(c, x, y + 54, C_TEXT, w, "EMU BUFFERS %s", a); }
    else px_text(c, x, y + 54, "EMU BUFFERS NONE", C_DIM, 1, w);
    if (v->has_card) {
        int lv = mon_level(TH_CARD, v->card_free_pct);
        mon_kb_text(a, sizeof a, v->card_free_mb * 1024);
        px_textf(c, x, y + 65, px_lv_col(lv), w, "CARD %s FREE %d%%%c", a, v->card_free_pct, mon_glyph(lv));
    } else px_text(c, x, y + 65, "CARD N/A", C_DIM, 1, w);
    px_textf(c, x, y + 76, C_HEAD, w, "ROOT %s", v->app.root[0] ? v->app.root : "-");
    px_textf(c, x, y + 87, C_DIM, w, "%s", v->app.root_why[0] ? v->app.root_why : "-");
}

static inline void px_mon_app(uint32_t *c, PxRect b, const MonView *v) {
    int x = b.x + 8, y = b.y + 10, w = b.w - 16;
    const MonApp *p = &v->app;
    px_box(c, b, "THIS APP");
    if (p->speed_pct >= 0) { int lv = mon_level(TH_SPEED, p->speed_pct); px_textf(c, x, y, px_lv_col(lv), w, "SPEED %d.%02dX%c", p->speed_pct / 100, p->speed_pct % 100, mon_glyph(lv)); }
    else px_text(c, x, y, "SPEED -  (NOT RUNNING)", C_DIM, 1, w);
    if (!p->loaded) px_textf(c, x, y + 11, C_TEXT, w, "JIT %s AUDIO %d/S", !p->jit_avail ? "N/A" : p->jit_pref ? "SET ON" : "SET OFF", p->cb_per_s);   /* nothing is emulated: the setting, not the machine */
    else if (p->jit && p->jit_blocks >= 0) px_textf(c, x, y + 11, C_TEXT, w, "JIT %d BLK%s AUDIO %d/S", p->jit_blocks, p->jit_given_up ? "!" : "", p->cb_per_s);
    else px_textf(c, x, y + 11, C_TEXT, w, "JIT %s  AUDIO %d/S", p->jit ? "ON" : "OFF", p->cb_per_s);
    px_textf(c, x, y + 22, p->late_cb ? C_ERR : C_TEXT, w, "LATE BUFFERS %d%s", p->late_cb, p->late_cb ? "!" : "");
    int fl = mon_level(TH_FRAME, p->frame_ms);
    px_textf(c, x, y + 33, px_lv_col(fl), w, "FRAME %dMS%c MAX %d", p->frame_ms, mon_glyph(fl), p->frame_max_ms);
    if (p->job[0]) px_textf(c, x, y + 44, C_HEAD, w, "JOB %s %d%%", p->job, p->job_pct < 0 ? 0 : p->job_pct); else px_text(c, x, y + 44, "JOB NONE", C_DIM, 1, w);
    px_textf(c, x, y + 55, p->fw[0] ? C_TEXT : C_DIM, w, "FW %s", p->fw[0] ? p->fw : "NONE");
    if (v->app_cpu_x10 >= 0) { int pc = v->app_cpu_x10 / 10; px_textf(c, x, y + 66, px_lv_col(mon_level(TH_CPU, pc)), w, "APP CPU %d.%d%%%c", v->app_cpu_x10 / 10, v->app_cpu_x10 % 10, mon_glyph(mon_level(TH_CPU, pc))); }
    else px_text(c, x, y + 66, "APP CPU -", C_DIM, 1, w);
    px_textf(c, x, y + 77, C_DIM, w, "PANEL %d.%02d%% %dUS", v->share_x100 / 100, v->share_x100 % 100, v->sample_us);
}

static inline int px_mon_dev_h(int narrow) { return narrow ? 10 + 13 * 11 + 6 : 10 + 6 * 11 + 6; }
static inline void px_mon_dev(uint32_t *c, PxRect b, const MonView *v, int narrow) {
    int x = b.x + 8, y = b.y + 10, w = b.w - 16, x2 = narrow ? x : x + 196, w2 = narrow ? w : 184;
    int r2 = narrow ? 4 : 0;                         /* the second column's first row: below the first column's in Zoomed, beside it in Standard */
    px_box(c, b, "DEVICE");
    if (v->bat.has) {
        int lv = mon_level(TH_BATT, v->bat.pct);
        px_textf(c, x, y, v->bat.pct < 0 ? C_DIM : px_lv_col(lv), w, "BATTERY %d%%%c", v->bat.pct, mon_glyph(lv));
        px_text(c, x, y + 11, v->bat.status[0] ? v->bat.status : "STATE -", C_TEXT, 1, w);
        if (v->bat.mw >= 0) px_textf(c, x, y + 22, C_TEXT, w, "DRAW %d MW", v->bat.mw); else px_text(c, x, y + 22, "DRAW -", C_DIM, 1, w);
    } else {
        px_text(c, x, y, "BATTERY N/A", C_DIM, 1, w);
        px_text(c, x, y + 11, "STATE N/A", C_DIM, 1, w);
        px_text(c, x, y + 22, "DRAW N/A", C_DIM, 1, w);
    }
    if (v->wake_ps >= 0) px_textf(c, x, y + 33, C_TEXT, w, "WAKE-UPS %d/S", v->wake_ps); else px_text(c, x, y + 33, "WAKE-UPS -", C_DIM, 1, w);
    int y2 = y + r2 * 11;
    px_textf(c, x2, y2, C_TEXT, w2, "PADS %d %s", v->app.njoy, v->app.pad[0]);
    px_textf(c, x2, y2 + 11, v->app.midi[0] ? C_TEXT : C_DIM, w2, "MIDI %s", v->app.midi[0] ? v->app.midi : "NONE");
    px_textf(c, x2, y2 + 22, v->net[0] ? C_TEXT : C_DIM, w2, "NET %s", v->net[0] ? v->net : "N/A");
    if (v->platform == MON_PLAT_LINUX) {
        char u3[96] = "";
        for (int i = 0; i < v->nusb; i++) { size_t k = strlen(u3); snprintf(u3 + k, sizeof u3 - k, "%s%s", i ? ", " : "", v->usb[i]); }
        px_textf(c, x2, y2 + 33, v->nusb ? C_TEXT : C_DIM, w2, "USB %s", v->nusb ? u3 : "NONE");
    } else px_text(c, x2, y2 + 33, "USB N/A", C_DIM, 1, w2);
    int nh = narrow ? 3 : 2;                         /* the hot-plug history, newest first */
    if (narrow) px_text(c, x2, y2 + 44, "HOT-PLUG:", C_HEAD, 1, w2);
    for (int i = 0; i < nh; i++) {
        int hy = narrow ? y2 + 55 + i * 11 : y2 + 44 + i * 11;
        if (v->app.hot[i][0]) px_text(c, x2, hy, v->app.hot[i], i ? C_DIM : C_TEXT, 1, w2);
        else if (i == 0) px_text(c, x2, hy, narrow ? "NONE YET" : "HOT-PLUG: NONE YET", C_DIM, 1, w2);
    }
}

static inline void px_mon_procs(uint32_t *c, PxRect b, const MonView *v, int narrow, int page_in_procs) {
    int rows = narrow ? MON_ROWS_NARROW : MON_ROWS_STD, x = b.x + 8, y = b.y + 10, w = b.w - 16;
    static const char *const sortn[MON_SORTS] = {"CPU", "MEM", "PID"};
    char s[64];
    px_box(c, b, "PROCESSES");
    if (!v->procs_read) { px_text(c, x, y, "READING...", C_WARN, 1, w); return; }
    if (narrow) snprintf(s, sizeof s, "%d TASKS  SORT %s", v->nproc_total, sortn[v->sort % MON_SORTS]);
    else snprintf(s, sizeof s, "%d TASKS%s  SORT %s   (%%CPU OF ONE CORE)", v->nproc_total, v->proc_capped ? " (FIRST 192 READ)" : "", sortn[v->sort % MON_SORTS]);
    px_text(c, x, y, s, C_DIM, 1, w);
    if (!v->platform) { px_text(c, x, y + 13, "N/A: NO HOST SOURCE", C_DIM, 1, w); return; }
    if (narrow) px_text(c, x, y + 11, "PID   NAME   CPU  RSS", C_HEAD, 1, w);
    else { px_text(c, x, y + 11, "PID", C_HEAD, 1, 0); px_text(c, x + 42, y + 11, "NAME", C_HEAD, 1, 0); px_text(c, x + 134, y + 11, "CPU", C_HEAD, 1, 0); px_text(c, x + 288, y + 11, "RSS", C_HEAD, 1, 0); }
    for (int i = 0; i < rows; i++) {
        int idx = page_in_procs * rows + i;
        if (idx >= v->nprocs) break;
        const MonProc *p = &v->procs[idx];
        int ry = y + 22 + i * 11, pc = p->cpu_x10 / 10 > 100 ? 100 : p->cpu_x10 / 10, lv = mon_level(TH_CPU, pc);
        uint32_t col = p->pid == v->self_pid ? C_HEAD : C_TEXT;
        char rss[16];
        mon_kb_text(rss, sizeof rss, (unsigned long long)p->rss_kb);
        if (narrow) {
            px_textf(c, x, ry, col, 0, "%-5d %-6.6s", p->pid, p->name);
            px_textf(c, x + 78, ry, px_lv_col(lv), 0, "%d%%%c", p->cpu_x10 / 10, mon_glyph(lv));
            px_text(c, x + 112, ry, rss, col, 1, 0);
        } else {
            px_textf(c, x, ry, col, 0, "%-6d %-14.14s", p->pid, p->name);
            px_textf(c, x + 134, ry, px_lv_col(lv), 0, "%d.%d%%%c", p->cpu_x10 / 10, p->cpu_x10 % 10, mon_glyph(lv));
            px_meter(c, (PxRect){x + 182, ry, 100, 8}, pc, px_lv_col(lv));
            px_text(c, x + 288, ry, rss, col, 1, 0);
        }
    }
}
static inline void px_mon_threads(uint32_t *c, PxRect b, const MonView *v, int narrow) {
    int x = b.x + 8, y = b.y + 10, w = b.w - 16;
    px_box(c, b, "THIS APP'S THREADS");
    if (!v->nthr) { px_text(c, x, y, v->platform ? "NONE LISTED" : "N/A: NO HOST SOURCE", C_DIM, 1, w); return; }
    for (int i = 0; i < v->nthr; i++) {
        const MonThread *t = &v->thr[i];
        int pc = t->cpu_x10 / 10 > 100 ? 100 : t->cpu_x10 / 10, lv = mon_level(TH_CPU, t->cpu_x10 < 0 ? -1 : pc);
        px_textf(c, x, y + i * 11, C_TEXT, narrow ? 66 : 150, "%s", t->name);
        if (t->cpu_x10 < 0) px_text(c, x + (narrow ? 70 : 154), y + i * 11, "-", C_DIM, 1, 0);
        else px_textf(c, x + (narrow ? 70 : 154), y + i * 11, px_lv_col(lv), 0, "%d.%d%%%c", t->cpu_x10 / 10, t->cpu_x10 % 10, mon_glyph(lv));
        if (!narrow && t->core >= 0) px_textf(c, x + 224, y + i * 11, C_DIM, 0, "CORE %d", t->core);
    }
}

/* The data rain: the newest row at the top, each row led by the letter of its source (A audio out, M MIDI bytes, P LCD pixels, R emulated RAM). */
static inline void px_mon_rain(uint32_t *c, PxRect b, const UiState *u, int narrow) {
    px_box(c, b, u->rain_still ? "DATA IN FLIGHT (STILL)" : "DATA IN FLIGHT");
    int cols = (b.w - 32) / 18, rows = (b.h - 16) / 10;
    if (cols > RAIN_COLS) cols = RAIN_COLS;
    if (narrow && cols > 7) cols = 7;
    for (int i = 0; i < rows; i++) {
        char tag = '?';
        const uint8_t *r = rain_row(&u->rain, i, &tag);
        if (!r) break;
        int ry = b.y + 10 + i * 10;
        px_textf(c, b.x + 8, ry, C_DIM, 0, "%c", tag);
        for (int col = 0; col < cols; col++) px_textf(c, b.x + 22 + col * 18, ry, i == 0 ? C_OK : C_DIM, 0, "%02X", r[col]);
    }
}

/* Draws the panel; returns the lowest y of any box it drew (the self-test checks it stays above the foot: the page line and the hint). */
static inline int px_panel_monitor(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    const MonView *v = &u->mon;
    int pages = mon_pages(narrow, v->nprocs), page = u->mon_page < 0 ? 0 : u->mon_page >= pages ? pages - 1 : u->mon_page;
    int foot = r.y + r.h - 26, lx = r.x + 4, bw = r.w - 8;
    if (u->rain_on && u->rain_full) {             /* after a stretch of no input: the whole panel is the rain; any key ends it */
        px_mon_rain(c, (PxRect){lx, r.y + 8, bw, r.h - 40}, u, narrow);
        px_text(c, r.x + 10, foot, "ANY KEY ENDS THIS", C_DIM, 1, r.w - 20);
        return r.y + r.h - 32;
    }
    int free_y = r.y + 8, bottom = r.y + 8;
    if (narrow) {
        if (page == 0) {
            int h = px_mon_cpu_h(v, 1);
            px_mon_cpu(c, (PxRect){lx, r.y + 8, bw, h}, v, 1);
            px_mon_mem(c, (PxRect){lx, r.y + 8 + h + 8, bw, 112}, v);
            free_y = r.y + 8 + h + 8 + 112 + 8; bottom = free_y - 8;
        } else if (page == 1) {
            px_mon_app(c, (PxRect){lx, r.y + 8, bw, 100}, v);
            int th = 14 + (v->nthr ? v->nthr : 1) * 11;
            px_mon_threads(c, (PxRect){lx, r.y + 116, bw, th}, v, 1);
            int dy = r.y + 116 + th + 8;
            px_mon_dev(c, (PxRect){lx, dy, bw, px_mon_dev_h(1)}, v, 1);
            free_y = dy + px_mon_dev_h(1) + 8; bottom = free_y - 8;
        } else { px_mon_procs(c, (PxRect){lx, r.y + 8, bw, 22 + MON_ROWS_NARROW * 11 + 14}, v, 1, page - 2); bottom = r.y + 8 + 22 + MON_ROWS_NARROW * 11 + 14; }
    } else {
        if (page == 0) {
            int h = px_mon_cpu_h(v, 0), y = r.y + 8;
            px_mon_cpu(c, (PxRect){lx, y, bw, h}, v, 0);
            y += h + 8;
            px_mon_mem(c, (PxRect){lx, y, bw / 2 - 2, 122}, v);
            px_mon_app(c, (PxRect){lx + bw / 2 + 2, y, bw / 2 - 2, 122}, v);
            y += 130;
            px_mon_dev(c, (PxRect){lx, y, bw, px_mon_dev_h(0)}, v, 0);
            free_y = y + px_mon_dev_h(0) + 8; bottom = free_y - 8;
        } else {
            px_mon_procs(c, (PxRect){lx, r.y + 8, bw, 22 + MON_ROWS_STD * 11 + 14}, v, 0, page - 1);
            int ty = r.y + 8 + 22 + MON_ROWS_STD * 11 + 14 + 8;
            px_mon_threads(c, (PxRect){lx, ty, bw, 14 + (v->nthr ? v->nthr : 1) * 11}, v, 0);
            bottom = ty + 14 + (v->nthr ? v->nthr : 1) * 11;
        }
    }
    if (u->rain_on && foot - free_y >= 40 && (narrow ? page <= 1 : page == 0))          /* "if there is space": whatever the page left free */
        px_mon_rain(c, (PxRect){lx, free_y, bw, foot - free_y - 2}, u, narrow);
    char s[64];
    snprintf(s, sizeof s, "%s PAGE %d/%d%s", v->platform == MON_PLAT_LINUX ? "LINUX" : v->platform == MON_PLAT_MAC ? "MACOS" : "NO HOST SOURCE", page + 1, pages, u->mon_paused ? "  FROZEN" : "");
    px_text(c, r.x + 10, foot + 4, s, u->mon_paused ? C_WARN : C_DIM, 1, r.w - 20);
    return bottom;
}

static inline void px_panel_tools(uint32_t *c, PxRect r, const UiState *u, int narrow) {
    int x = r.x + 10, w = r.w - 20;
    int page = 0;
    px_box(c, (PxRect){r.x + 4, r.y + 8, r.w - 8, u->ntools * 12 + 14}, "SANDBOX TOOLS");
    px_list(c, (PxRect){x, r.y + 20, w, u->ntools * 12}, u->tools, u->ntools, u->tool_sel, PX_MAX_TOOLS, &page);
    int ly = r.y + u->ntools * 12 + 30;
    px_box(c, (PxRect){r.x + 4, ly, r.w - 8, 70}, "LOG");
    px_wrap(c, x, ly + 12, u->msg ? u->msg : "", px_style(u->msg_style), w, narrow ? 5 : 5);
    px_box(c, (PxRect){r.x + 4, ly + 78, r.w - 8, 62}, "APP FOLDER (WHERE EVERYTHING IS SAVED)");
    px_wrap(c, x, ly + 90, u->app_root[0] ? u->app_root : "-", C_HEAD, w, 2);
    px_wrap(c, x, ly + 112, u->app_root_why[0] ? u->app_root_why : "-", C_DIM, w, 2);
}

/* A question over the panel: A is yes, B is no. */
static inline void px_confirm(uint32_t *c, PxRect r, const char *q) {
    PxRect b = {r.x + 8, r.y + r.h / 2 - 40, r.w - 16, 80};
    px_rect(c, b, C_BG);
    px_frame(c, b, C_WARN);
    px_wrap(c, b.x + 8, b.y + 10, q, C_TEXT, b.w - 16, 4);
    px_text(c, b.x + 8, b.y + b.h - 14, "A  YES      B  NO", C_HEAD, 1, b.w - 16);
}

/* An overlay menu, centred on the whole window: a list with the chosen row on the selection colour and a value at the right of a row, or a box of text lines. */
static inline void px_menu(uint32_t *c, const UiState *u) {
    int n = u->menu_n < 0 ? 0 : u->menu_n > PX_MENU_MAX ? PX_MENU_MAX : u->menu_n, w = 340, h = (n < 1 ? 1 : n) * 12 + 40;
    PxRect b = {(PX_W - w) / 2, (PX_H - h) / 2, w, h};
    px_rect(c, b, C_PANEL);
    px_frame(c, b, C_HEAD);
    px_text(c, b.x + 10, b.y + 8, u->menu_title, C_HEAD, 1, w - 20);
    for (int i = 0; i < n; i++) {
        PxRect row = {b.x + 4, b.y + 22 + i * 12, w - 8, 12};
        if (u->menu_kind == 1 && i == u->menu_sel) px_rect(c, row, C_SEL);
        px_text(c, row.x + 6, row.y + 2, u->menu_label[i], u->menu_kind == 1 ? C_TEXT : i == 0 ? C_TEXT : C_DIM, 1, row.w - 12);
        if (u->menu_kind == 1 && u->menu_value[i][0]) {
            int vw = px_text_w(u->menu_value[i], 1);
            px_text(c, row.x + row.w - 6 - vw, row.y + 2, u->menu_value[i], C_TEXT, 1, vw + 1);
        }
    }
    px_text(c, b.x + 10, b.y + h - 14, u->menu_foot, C_HEAD, 1, w - 20);
}

/* The player's column (160 x 480 at the right of the 2x picture): whose demo, how far, what the tour is doing, which keys, the sound, and what it is. */
static inline void px_panel_player(uint32_t *c, PxRect r, const UiState *u) {
    int x = r.x + 8, w = r.w - 16;
    char line[64];
    px_box(c, (PxRect){r.x + 2, r.y + 8, r.w - 4, 96}, "DEMO");
    px_wrap(c, x, r.y + 20, u->pl_name, C_TEXT, w, 2);
    px_wrap(c, x, r.y + 42, u->pl_tier, C_HEAD, w, 2);
    px_wrap(c, x, r.y + 64, u->pl_licence[0] ? u->pl_licence : "-", C_DIM, w, 4);
    px_box(c, (PxRect){r.x + 2, r.y + 114, r.w - 4, 82}, "NOW");
    px_wrap(c, x, r.y + 126, u->pl_step, C_OK, w, 2);
    int total = u->pl_ticks > 0 ? u->pl_ticks : 1, pct = u->pl_tick * 100 / total;
    px_meter(c, (PxRect){x, r.y + 148, w, 8}, pct > 100 ? 100 : pct, C_HEAD);
    snprintf(line, sizeof line, "%d:%02d / %d:%02d", u->pl_tick / 30 / 60, u->pl_tick / 30 % 60, u->pl_ticks / 30 / 60, u->pl_ticks / 30 % 60);
    px_text(c, x, r.y + 162, line, C_TEXT, 1, w);
    snprintf(line, sizeof line, "PICTURE %d OF %d", u->pl_idx + 1, u->pl_nf);
    px_text(c, x, r.y + 174, line, C_DIM, 1, w);
    px_box(c, (PxRect){r.x + 2, r.y + 206, r.w - 4, 52}, "KEYS HELD");
    px_keyboard(c, (PxRect){x, r.y + 218, w, 32}, u->pl_keys);
    px_box(c, (PxRect){r.x + 2, r.y + 268, r.w - 4, 92}, "SOUND");
    PxRect sc = {x, r.y + 280, w, 48};
    px_rect(c, sc, C_BG);
    px_rect(c, (PxRect){sc.x, sc.y + sc.h / 2, sc.w, 1}, C_FRAME);
    int prev = sc.h / 2;
    for (int i = 0; i < sc.w; i++) {
        int v = u->scope[i * PX_SCOPE / sc.w], y = sc.h / 2 - v * (sc.h / 2 - 1) / 32768;
        int lo = y < prev ? y : prev, hi = y < prev ? prev : y;
        px_rect(c, (PxRect){sc.x + i, sc.y + lo, 1, hi - lo + 1}, C_OK);
        prev = y;
    }
    px_text(c, x, r.y + 333, "L", C_DIM, 1, 0);
    px_meter(c, (PxRect){x + 10, r.y + 332, w - 10, 7}, u->level_l, C_OK);
    px_text(c, x, r.y + 345, "R", C_DIM, 1, 0);
    px_meter(c, (PxRect){x + 10, r.y + 344, w - 10, 7}, u->level_r, C_OK);
    px_box(c, (PxRect){r.x + 2, r.y + 370, r.w - 4, 80}, "WHAT THIS IS");
    px_wrap(c, x, r.y + 382, "A 15 S TOUR THE APP PLAYED ON THIS FIRMWARE: BOOT, THREE NOTES, A CHORD, ONE TURN OF THE SELECT KNOB. ITS OWN SCREEN AT 2X.", C_DIM, w, 6);
    px_text(c, x, r.y + 460, "ANY KEY STOPS", C_HEAD, 1, w);
}

/* Compare mode's column: the same 160 x 480 as the player's, for two firmwares on the same notes. */
static inline void px_panel_compare(uint32_t *c, PxRect r, const UiState *u) {
    int x = r.x + 8, w = r.w - 16;
    char line[64];
    px_box(c, (PxRect){r.x + 2, r.y + 8, r.w - 4, 64}, "COMPARE");
    snprintf(line, sizeof line, "%c A %s", u->cmp_side == 0 ? '>' : ' ', u->cmp_name_a);
    px_text(c, x, r.y + 22, line, u->cmp_side == 0 ? C_TEXT : C_DIM, 1, w);
    snprintf(line, sizeof line, "%c B %s", u->cmp_side == 1 ? '>' : ' ', u->cmp_name_b);
    px_text(c, x, r.y + 36, line, u->cmp_side == 1 ? C_TEXT : C_DIM, 1, w);
    px_text(c, x, r.y + 52, "THE SAME NOTES IN BOTH", C_DIM, 1, w);
    px_box(c, (PxRect){r.x + 2, r.y + 82, r.w - 4, 70}, "NOW");
    px_wrap(c, x, r.y + 94, u->pl_step, C_OK, w, 2);
    int total = u->pl_ticks > 0 ? u->pl_ticks : 1, pct = u->pl_tick * 100 / total;
    px_meter(c, (PxRect){x, r.y + 116, w, 8}, pct > 100 ? 100 : pct, C_HEAD);
    snprintf(line, sizeof line, "%d:%02d / %d:%02d   STEP %d/%d", u->pl_tick / 30 / 60, u->pl_tick / 30 % 60, u->pl_ticks / 30 / 60, u->pl_ticks / 30 % 60, u->cmp_step + 1, u->cmp_nsteps);
    px_text(c, x, r.y + 130, line, C_TEXT, 1, w);
    px_box(c, (PxRect){r.x + 2, r.y + 162, r.w - 4, 82}, "THIS STEP");
    snprintf(line, sizeof line, "PEAK A %d%%", u->cmp_peak_a); px_text(c, x, r.y + 174, line, C_TEXT, 1, w);
    snprintf(line, sizeof line, "PEAK B %d%%", u->cmp_peak_b); px_text(c, x, r.y + 186, line, C_TEXT, 1, w);
    snprintf(line, sizeof line, "DIFFERENCE %d%%", u->cmp_diff); px_text(c, x, r.y + 200, line, C_TEXT, 1, w);
    px_meter(c, (PxRect){x, r.y + 214, w, 8}, u->cmp_diff, u->cmp_diff > 50 ? C_WARN : C_OK);
    px_text(c, x, r.y + 228, "0 = THE SAME SOUND", C_DIM, 1, w);
    px_box(c, (PxRect){r.x + 2, r.y + 254, r.w - 4, 46}, "WHOLE TOUR");
    if (u->cmp_first_ms < 0) px_text(c, x, r.y + 268, "IDENTICAL", C_OK, 1, w);
    else { snprintf(line, sizeof line, "DIFFERS FROM %ld.%ld S", u->cmp_first_ms / 1000, u->cmp_first_ms % 1000 / 100); px_text(c, x, r.y + 268, line, C_TEXT, 1, w); }
    px_box(c, (PxRect){r.x + 2, r.y + 310, r.w - 4, 40}, "SOUND");
    px_text(c, x, r.y + 322, "L", C_DIM, 1, 0);
    px_meter(c, (PxRect){x + 10, r.y + 321, w - 10, 7}, u->level_l, C_OK);
    px_text(c, x, r.y + 334, "R", C_DIM, 1, 0);
    px_meter(c, (PxRect){x + 10, r.y + 333, w - 10, 7}, u->level_r, C_OK);
    px_box(c, (PxRect){r.x + 2, r.y + 360, r.w - 4, 112}, "KEYS");
    px_text(c, x, r.y + 374, "LEFT  HEAR A", C_TEXT, 1, w);
    px_text(c, x, r.y + 386, "RIGHT HEAR B", C_TEXT, 1, w);
    px_text(c, x, r.y + 398, "UP DOWN  STEP", C_TEXT, 1, w);
    px_text(c, x, r.y + 410, "A  FROM THE START", C_TEXT, 1, w);
    px_text(c, x, r.y + 422, "START  PAUSE", C_TEXT, 1, w);
    px_text(c, x, r.y + 434, "B  CLOSE", C_TEXT, 1, w);
    px_text(c, x, r.y + 450, "ITS SCREEN AT 2X", C_DIM, 1, w);
}

/* Draws the whole window into c (PX_W x PX_H ARGB): the FM-1's screen first, then the carousel and the current panel around it. */
static inline void px_draw(uint32_t *c, int mode, const UiState *u) {
    if (u->playing) mode = MODE_ZOOMED;                          /* a demo plays at 2x whatever the layout was */
    for (int i = 0; i < PX_W * PX_H; i++) c[i] = C_BG;
    /* the FM-1's screen, pixel accurate: 1x copies, 2x makes each pixel a 2x2 block (no smoothing) */
    int s = px_scale(mode);
    for (int y = 0; y < FM_H; y++)
        for (int x = 0; x < FM_W; x++) {
            uint32_t v = 0xFF000000u | (u->fb_valid ? u->fb[y * FM_W + x] : 0);
            for (int dy = 0; dy < s; dy++)
                for (int dx = 0; dx < s; dx++) c[(y * s + dy) * PX_W + x * s + dx] = v;
        }
    if (!u->fb_valid) px_text(c, 8, 8, "NO SIGNAL", C_DIM, 1, 0);
    if (u->playing) { PxRect pp[2]; px_panel_rects(MODE_ZOOMED, pp); px_rect(c, pp[0], C_BG); if (u->cmp_on) px_panel_compare(c, pp[0], u); else px_panel_player(c, pp[0], u); return; }
    int narrow = mode == MODE_ZOOMED, p = u->panel >= 0 && u->panel < PANEL_COUNT ? u->panel : 0;
    PxRect pr[2], cr = px_content_rect(mode);
    px_panel_rects(mode, pr);
    px_rect(c, pr[0], C_BG);
    px_carousel(c, mode, p);
    switch (p) {
    case PANEL_MACHINE: px_panel_machine(c, cr, u, narrow); break;
    case PANEL_FILES: px_panel_files(c, cr, u, narrow); break;
    case PANEL_CPU: px_panel_cpu(c, cr, u, narrow); break;
    case PANEL_MEMORY: px_panel_memory(c, cr, u, narrow); break;
    case PANEL_CONSOLE: px_panel_console(c, cr, u, narrow); break;
    case PANEL_MIDI: px_panel_midi(c, cr, u, narrow); break;
    case PANEL_SOUND: px_panel_sound(c, cr, u, narrow); break;
    case PANEL_MONITOR: px_panel_monitor(c, cr, u, narrow); break;
    default: px_panel_tools(c, cr, u, narrow); break;
    }
    if (u->confirm[0]) px_confirm(c, cr, u->confirm);
    /* the foot of the panel */
    px_hint(c, pr[0], narrow ? "L1 R1 MENU  FN PANEL" : "L1 R1 MENU  L2 R2 CONTEXT  FN PANEL  START PAUSE  TAB ZOOM");
    if (!narrow) {
        PxRect b = pr[1];
        px_box(c, (PxRect){b.x + 4, b.y + 8, b.w - 8, b.h - 16}, PX_PANEL_NAMES[p]);
        for (int i = 0; i < 9 && PX_HINTS[p][i][0]; i++)
            px_text(c, b.x + 12, b.y + 22 + i * 14, PX_HINTS[p][i], i == 0 ? C_HEAD : C_DIM, 1, b.w - 24);
        px_text(c, b.x + 12, b.y + b.h - 30, "B ESC LEAVES A QUESTION", C_DIM, 1, b.w - 24);
    }
    if (u->menu_kind) px_menu(c, u);
}

/* ---- contrast, for the self-test ---------------------------------------------------------------------------------------- */
static inline double px_lum(uint32_t c) {
    double v[3] = {(c >> 16 & 255) / 255.0, (c >> 8 & 255) / 255.0, (c & 255) / 255.0}, w[3] = {0.2126, 0.7152, 0.0722}, s = 0;
    for (int i = 0; i < 3; i++) s += w[i] * (v[i] <= 0.03928 ? v[i] / 12.92 : __builtin_pow((v[i] + 0.055) / 1.055, 2.4));
    return s;
}
static inline double px_contrast(uint32_t a, uint32_t b) {
    double la = px_lum(a), lb = px_lum(b);
    return (la > lb ? la + 0.05 : lb + 0.05) / (la > lb ? lb + 0.05 : la + 0.05);
}

#endif
