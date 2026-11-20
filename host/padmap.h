/* The r3i6nos input mapping, FM1-NTT's side of it (../r3i6nos/docs/r3i6nos-mapping.md, CLAUDE.md rule 17). MIT.
 *
 * The pure part, no SDL and no App: the actions every panel understands, what R1 held ("increase") does to the d-pad, and the right stick as the d-pad
 * (dead zone 0.35, at most 6 steps a second; Page up and down with R1 held, Home and End once for each push). The same four numbers as r3i6nos's theme
 * (theme/r3i6nos_input.h: R3_STICK_DEAD 0.35, R3_STICK_RATE 6); FM1-NTT's own knobs keep 12 a second on MACHINE (STICK_MAX_CPS), a list is easier to overshoot.
 * This is written here, not copied from that header: it needs r3i6nos's text-mode theme, which this pixel app does not use (rule 6).
 * Raw pad buttons, 0-based as SDL counts them: B 0, A 1, X 2, Y 3, L1 4, R1 5, L2 6, R2 7, up 8, down 9, left 10, right 11, Select 12, Start 13, L3 14, R3 15, Fn 16.
 * Untested on a unit. */
#ifndef FM1NTT_PADMAP_H
#define FM1NTT_PADMAP_H

#include <math.h>
#include <stdint.h>

#define PM_STICK_DEAD 0.35f     /* of the axis's travel: FM1-NTT's STICK_DEAD */
#define PM_STICK_RATE 6.0f      /* steps a second at full deflection, for a list or a menu */

#define PAD_L3 14
#define PAD_R3 15

/* What a press means on the panels that are not the FM-1's own controls. L1, R1, L2, R2 and Fn are never actions of a panel: they are the system's (R1 held turns UP DOWN LEFT RIGHT
 * into PGUP PGDN HOME END). A_SELECT and A_START are the panel's own (a channel, a toggle; Start pauses on a panel with no use for it). */
enum { A_NONE, A_UP, A_DOWN, A_LEFT, A_RIGHT, A_OK, A_BACK, A_X, A_Y, A_SELECT, A_START, A_PGUP, A_PGDN, A_HOME, A_END };

/* R1 held is "increase": up and down become Page up and Page down, left and right Home and End. Any other action is as it was. */
static inline int pm_r1(int action, int r1) {
    if (!r1) return action;
    switch (action) {
    case A_UP: return A_PGUP;
    case A_DOWN: return A_PGDN;
    case A_LEFT: return A_HOME;
    case A_RIGHT: return A_END;
    default: return action;
    }
}
/* The d-pad's raw button as an action (the default binding), with R1's meaning. A_NONE for any other button. */
static inline int pm_dpad(int b, int r1) {
    switch (b) {
    case 8: return pm_r1(A_UP, r1);
    case 9: return pm_r1(A_DOWN, r1);
    case 10: return pm_r1(A_LEFT, r1);
    case 11: return pm_r1(A_RIGHT, r1);
    default: return A_NONE;
    }
}

/* Steps a second for a deflection v: 0 inside the dead zone, PM_STICK_RATE at full. */
static inline float pm_stick_rate(float v) {
    float m = fabsf(v);
    if (m <= PM_STICK_DEAD) return 0;
    if (m > 1) m = 1;
    return (m - PM_STICK_DEAD) / (1 - PM_STICK_DEAD) * PM_STICK_RATE;
}

typedef struct {
    int zone[2];            /* the right stick's x and y: -1, 0 or +1 once past the dead zone (a push is counted once, for Home and End) */
    float frac[2];          /* the part of a step carried between frames, so a slow stick is not lost */
    uint32_t last_ms;
    int have_last;
} PmStick;

/* Once a pass of the main loop: the right stick's steps since the last call, as actions (up to cap). rx, ry are -1..1 (down is positive, as SDL reports it).
 * Up and down step at the stick's rate (Page up and down with R1 held); left and right step likewise, but with R1 held fire Home or End once for each push.
 * The first step of a push comes at once; then the rate runs. A stalled frame never turns into a burst. Returns how many were written to out. */
static inline int pm_stick_tick(PmStick *s, float rx, float ry, int r1, uint32_t now_ms, int out[], int cap) {
    float dt = s->have_last ? (float)(now_ms - s->last_ms) / 1000.0f : 0;
    if (dt > 0.25f) dt = 0.25f;
    s->last_ms = now_ms;
    s->have_last = 1;
    int n = 0;
    for (int k = 0; k < 2; k++) {
        float v = k == 0 ? rx : ry;
        int z = v <= -PM_STICK_DEAD ? -1 : v >= PM_STICK_DEAD ? 1 : 0;
        if (z != s->zone[k]) {
            s->zone[k] = z;
            s->frac[k] = z ? 1.0f : 0;
            if (z && k == 0 && r1) {                 /* Home or End: once for the push, never repeated */
                if (n < cap) out[n++] = z < 0 ? A_HOME : A_END;
                s->frac[k] = 0;
                continue;
            }
        } else if (z && !(k == 0 && r1)) s->frac[k] += dt * pm_stick_rate(v);
        if (!z || (k == 0 && r1)) continue;
        int act = k == 0 ? (z < 0 ? A_LEFT : A_RIGHT) : r1 ? (z < 0 ? A_PGUP : A_PGDN) : (z < 0 ? A_UP : A_DOWN);
        while (s->frac[k] >= 1.0f && n < cap) { out[n++] = act; s->frac[k] -= 1.0f; }
        if (s->frac[k] >= 1.0f) s->frac[k] = 0.99f;  /* the caller's buffer was full: keep the rest for the next pass */
    }
    return n;
}

/* The list rule on a 0-based selection of n rows with per rows to a page: up and down wrap, Page up and down move by a page and stop at the ends,
 * Home and End jump. Anything else leaves the selection alone. */
static inline int pm_list_step(int sel, int n, int per, int action) {
    if (n <= 0) return 0;
    if (per < 1) per = 1;
    switch (action) {
    case A_UP: return (sel + n - 1) % n;
    case A_DOWN: return (sel + 1) % n;
    case A_PGUP: return sel - per < 0 ? 0 : sel - per;
    case A_PGDN: return sel + per > n - 1 ? n - 1 : sel + per;
    case A_HOME: return 0;
    case A_END: return n - 1;
    default: return sel;
    }
}

#endif /* FM1NTT_PADMAP_H */
