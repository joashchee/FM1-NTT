//! `fm4`: a 4-operator FM engine, written fresh for FM1-NTT (docs/PORTING.md; the engine the OPERATOR / Mighty Arp family needs).
//!
//! Four sine operators, OPERATOR's eleven algorithms (how they modulate each other and which are heard), self-feedback on each operator, a ratio, a fine
//! detune, a level, a velocity sensitivity and an attack / decay / sustain / release envelope for each. Eight voices, oldest stolen. Floating point, deterministic on one
//! platform, no allocation after `new`. **Fitted to no firmware** (OPERATOR and Mighty Arp boot in the emulator but their sound has not been compared).

use crate::engine::{Engine, ParamDesc};
use std::f64::consts::TAU;

pub const VOICES: usize = 8;
pub const OPS: usize = 4;
/// The decibels a decay or release time spans (see `render`).
const ENV_DB: f64 = 64.0;

pub const P_ALGORITHM: u16 = 0;
/// The self-feedback phase swing at 100 %, in hundredths of a radian (OPERATOR 0.7: about 310, a two-sample average as the DX7 does it).
pub const P_FEEDBACK: u16 = 1;
pub const P_LEVEL: u16 = 2;
/// The phase swing of a modulator at full level, in hundredths of a radian (OPERATOR 0.7: 628, one whole cycle; the generic default is 4 radians).
pub const P_INDEX: u16 = 3;
/// How an operator's level maps to amplitude: 0 a DX-style law (0.75 dB a step down from 99, OPERATOR 0.7), 1 linear (level / 100, Mighty Arp 1.5).
pub const P_LEVEL_LAW: u16 = 4;
/// How FINE RATIO acts: 0 a percentage of the coarse ratio (generic; 0 to 99), 1 thousandths added to it (OPERATOR 0.7: coarse 1 and fine 26 is 1.026; 0 to 999).
pub const P_RATIO_LAW: u16 = 5;
/// How SUSTAIN reads: 0 linear (value / 99, generic), 1 decibels (99 is 0 dB and each step below is 1 dB down; OPERATOR 0.7 shows its sustain in dB).
pub const P_SUSTAIN_LAW: u16 = 6;
/// Operator `n` (0 to 3, shown as OP1 to OP4) has `OP_PARAMS` parameters from `OP_BASE + n * OP_PARAMS`.
pub const OP_BASE: u16 = 10;
pub const OP_PARAMS: u16 = 11;
pub const O_RATIO: u16 = 0;
pub const O_FINE: u16 = 1;
pub const O_DETUNE: u16 = 2;
pub const O_LEVEL: u16 = 3;
pub const O_ATTACK: u16 = 4;
pub const O_DECAY: u16 = 5;
pub const O_SUSTAIN: u16 = 6;
pub const O_RELEASE: u16 = 7;
pub const O_VELOCITY: u16 = 8;
pub const O_KEYSCALE: u16 = 9;
pub const O_FEEDBACK: u16 = 10;
/// The second per-operator block (2026-11-19, from OPERATOR 0.7's pages): `EXT_PARAMS` parameters per operator from `EXT_BASE`, after the first block so every older id
/// keeps its number. `ext_param(op, E_*)` is the id.
pub const EXT_BASE: u16 = OP_BASE + OPS as u16 * OP_PARAMS;
pub const EXT_PARAMS: u16 = 6;
/// The waveform, `WAVES` (0 to 18).
pub const E_WAVE: u16 = 0;
/// The phase a note starts at, in percent of a cycle (OPERATOR's Phase; 66 % starts the sine at -0.83).
pub const E_PHASE: u16 = 1;
/// Key scaling in percent, -100 to 100, about note 60: the level changes by `2^(percent / 100 * octaves)` (OPERATOR's Key: +100 % is 6 dB an octave up).
pub const E_KEYSCALE: u16 = 2;
/// Fixed-frequency mode (OPERATOR's Fixed): 1 plays `FIXED FREQ` x `FIXED MULTI` Hz whatever the note.
pub const E_FIXED: u16 = 3;
/// The fixed frequency in Hz, 10 to 1024 (OPERATOR's Freq).
pub const E_FIXFREQ: u16 = 4;
/// The fixed frequency's multiplier: 0 to 4 for x0.001, x0.01, x0.1, x1, x10 (OPERATOR's Multi).
pub const E_FIXMULTI: u16 = 5;
/// The voice filter (OPERATOR's Filter page, fitted 2026-11-19): after the operators, before the master level. Off by default so older patches are unchanged.
pub const FILT_BASE: u16 = EXT_BASE + OPS as u16 * EXT_PARAMS;
pub const F_ON: u16 = 0;
/// 0 low-pass, 1 high-pass, 2 band-pass, 3 notch.
pub const F_TYPE: u16 = 1;
/// The cutoff as OPERATOR displays it, Hz (30 to 20000). The filter's own corner is this times `2^(key / 100 x (note - 60) / 12)`.
pub const F_FREQ: u16 = 2;
/// OPERATOR's Res, in hundredths (0 to 125); the quality factor is `0.5 / (1 - 0.97 x min(res, 0.93))` (0.5 at 0, 0.65 at 0.27).
pub const F_RES: u16 = 3;
/// 0 for 12 dB an octave (one two-pole stage), 1 for 24 dB (two stages).
pub const F_SLOPE: u16 = 4;
/// Key tracking in percent (OPERATOR's Freq<Key; its default is +100).
pub const F_KEY: u16 = 5;
pub const FILT_PARAMS: u16 = 6;
/// The LFO (OPERATOR's LFO page, measured 2026-11-20): pitch only so far. Off by default so older patches are unchanged.
pub const LFO_BASE: u16 = FILT_BASE + FILT_PARAMS;
pub const L_ON: u16 = 0;
/// 0 sine, 1 square, 2 triangle, 3 saw up, 4 saw down, 5 sample and hold (a uniform value each cycle: 45 cycles of OPERATOR's spanned -112 to 113 cents of 120), 6 noise (white
/// noise at 400 Hz through a one-pole low-pass whose corner is the LFO rate: correlation times of 6.4, 27 and 200 ms at 27.5, 5.8 and 0.7 Hz, and a spread of sqrt(rate)).
pub const L_TYPE: u16 = 1;
/// Hertz times 100 (OPERATOR's default is 6.32 Hz).
pub const L_RATE: u16 = 2;
/// Percent: the pitch swings by plus and minus an octave times this (measured: 10 % gave 120 cents, 69 % gave 827, 12 cents a percent).
pub const L_AMOUNT: u16 = 3;
/// 1 each note restarts its own LFO at phase 0, 0 one LFO runs free for every voice.
pub const L_RETRIG: u16 = 4;
/// Bit k set: operator k's pitch follows the LFO (OPERATOR's Osc A to D buttons; all four by default); bit 4 the filter's cutoff.
pub const L_DEST: u16 = 5;
/// OPERATOR's Amount A, percent (-100 to 100): scales the LFO's depth on every target of destination A (the operators and the filter); the pitch and cutoff swing are proportional to it (34 % gave 0.35 of the 100 % swing).
pub const L_AMOUNT_A: u16 = 6;
/// OPERATOR's Dest B (one target, its own Amount B): 0 Off, 1 to 4 Osc Vol A to D, 5 X-Fade A/C, 6 X-Fade B/D, 7 Feedback, 8 Fixed Freq, 9 FM Drive, 10 Filter Freq, 11 Filter Q,
/// 12 Filter Res, 13 Morph, 14 Flt Drive, 15 Filter Env, 16 Shp Drive, 17 LFO Rate, 18 LFO Amt, 19 Pitch Env, 20 Volume, 21 Pan, 22 Tone, 23 Time (the menu's order). **Only Osc Vol A to D
/// and Volume, FM Drive, Feedback, Fixed Freq and the filter's Freq, Q and Res act so far**: FM Drive scales every modulator's index by the same gain as Osc Vol; Feedback adds amount x Amount B x value (a fraction of the full feedback, never below 0) to the feedback of operators B to D (OPERATOR's A has none). The gain is 1 plus the LFO's amount times Amount B times its value (measured on Osc Vol A: 10 % gave 0.9 to 1.1, 69 % 0.32 to 1.69, 100 % 0 to 2).
pub const L_DEST_B: u16 = 7;
pub const L_AMOUNT_B: u16 = 8;
/// OPERATOR's Rate<Key, percent (-100 to 100): each note's LFO rate is the set rate times `2^(percent / 100 x (note - 60) / 12)` (measured at +100 %: 2.44, 4.88 and 9.78 Hz on notes 45, 57 and 69 from 5.8). Not applied to the free-running LFO.
pub const L_RATE_KEY: u16 = 9;
/// OPERATOR's Amt<Vel, percent (0 to 100; negatives not measured): the depth of every target is multiplied by `(1 - p) + p x velocity / 127` (+100 %: swings of 115, 60 and 30 cents at velocity 127, 64 and 32).
pub const L_AMT_VEL: u16 = 10;
/// OPERATOR's Range = Sync (0 free: the rate knob; 1 tempo-synced: `L_SYNC_DIV` at `L_TEMPO`). Range High is only a higher rate: the same knob position read 2.53 kHz instead of 6.32 Hz, and
/// the sidebands of a 6.54 kHz setting sat at 6.0 kHz either side of the carrier, so `L_RATE` simply reaches 25 kHz here.
pub const L_SYNC: u16 = 11;
/// The Synced menu in length order, an index: 0 1/32, 1 1/16T, 2 1/16, 3 1/8T (the default), 4 1/8, 5 1/4T, 6 3/16, 7 1/4, 8 1/2T, 9 3/8, 10 1/2, 11 3/4, 12 1 Bar, 13 2 Bars, 14 4 Bars.
pub const L_SYNC_DIV: u16 = 12;
/// OPERATOR's GLO Tempo, beats a minute (its default is 120). 1/8T at 120 measured 6.0 Hz times the engine factor (5.52).
pub const L_TEMPO: u16 = 13;
pub const LFO_PARAMS: u16 = 14;
/// The filter's second block (after the LFO so no earlier id moves): OPERATOR's Morph, and later its circuits and drive.
pub const F2_BASE: u16 = LFO_BASE + LFO_PARAMS;
/// Morph, percent: with `F_TYPE` 4 the filter is a mix of its low-pass, band-pass and high-pass outputs along keyframes, **LP at 0 %, BP at 25 %, HP at 50 %, notch (LP plus HP) at 75 %, LP again at 100 %**, mixed linearly between them, in each of the two stages of 24 dB (fitted to OPERATOR 0.7 at 26 settings: 0.7 dB rms).
pub const F2_MORPH: u16 = 0;
/// OPERATOR's filter Circuit: 0 Clean (linear; Drive does nothing), 1 to 4 OSR, MS2, SMP, PRD, **which measured identical** (a sine, a saw, and a high Res, at several Drives): a
/// saturating stage `u / (1 + |u|)` with `u = 0.96 x 10^(drive / 20) x input` (the input in units of a full-level sine of operator A), fitted to the sine's fundamental and its 3rd and 5th
/// harmonics at nine Drives (0.10 dB rms; tanh, atan, clip and others fitted 1.4 to 4 dB worse). Odd harmonics only, even at Drive 0 (a sine at full level has its 3rd at -19.5 dB).
pub const F2_CIRCUIT: u16 = 1;
/// Drive in tenths of a dB, 0 to 24.0 (the knob read +0.3, +0.5, +0.8, +2.8, +6.8, +11, +15 and +23 dB; it ends near +24).
pub const F2_DRIVE: u16 = 2;
/// OPERATOR's Shaper, after the filter: 0 Off, 1 Soft (`u / (1 + |u|)` with `u = 0.96 x gain x input`, the circuit's own curve), 2 Hard (the input times the gain, clipped at plus and
/// minus 1), 3 Sine (`sin(pi / 2 x u)`, folding back above 1), 4 Bit (`round(8 x clipped u) / 8`); `u` is the input times the Shp Drive gain, in units of a full-level sine of
/// operator A. Fitted to a sine at several drives (Hard to the clip level 1.0 within 0.5 dB, Sine and 4 Bit to 0.3 dB rms); the stage follows the filter (a clipped saw has its harmonics
/// above the cutoff filled in).
pub const F2_SHAPER: u16 = 3;
/// Shp Drive in tenths of a dB, 0 to 12.0 (0.25 dB a single click on OPERATOR's knob).
pub const F2_SHP_DRIVE: u16 = 4;
/// Dry/Wet in percent (100 is all shaped): the output is the dry signal plus the wet fraction of the difference.
pub const F2_DRYWET: u16 = 5;
/// OPERATOR's Freq<Vel, percent (-100 to 100): the cutoff moves by `4 x percent / 100 x velocity / 127` octaves (measured at 650 Hz: 1300 and 2600 Hz at velocity 32 and 64 and +100 %, 5770 Hz at 100; 2555 Hz at 127 and +50 %; none at velocity 0).
pub const F2_FREQ_VEL: u16 = 6;
/// OPERATOR's filter Envelope amount, percent (-100 to 100): the cutoff moves by `6 x percent / 100 x env` octaves, `env` running from 1 at the strike to the sustain level. On the instrument's
/// default envelope (Attack 0.1 ms, Decay 598 ms, Sustain 0, Release 50 ms) at 25 and 50 % the fitted cutoff started 1.5 and 2.6 octaves up and fell back to 650 Hz in about 0.65 s: twice the
/// amount, twice the octaves at every time. Its fall was not a line or one exponential: about `2.9 e^(-t / 60 ms) + 3.4 e^(-t / 170 ms)` octaves at 100 % (scaled with the decay time here).
pub const F2_ENV_AMOUNT: u16 = 7;
/// Filter envelope attack in tenths of a millisecond (the instrument's default is 0.1 ms).
pub const F2_ENV_ATTACK: u16 = 8;
/// Filter envelope decay in milliseconds (598 by default).
pub const F2_ENV_DECAY: u16 = 9;
/// Filter envelope sustain, percent (0 by default).
pub const F2_ENV_SUSTAIN: u16 = 10;
/// Filter envelope release in milliseconds (50 by default).
pub const F2_ENV_RELEASE: u16 = 11;
/// Filter envelope levels page (percent of the envelope's own scale): where the attack starts (0 by default), the peak it rises to and falls from (100) and where the release goes (0).
/// Measured on OPERATOR: Peak 0 removes the envelope; Initial 1.00 with a 135 ms attack holds the peak level from the strike; End 1.00 raises the cutoff after the note is let go.
/// Peak between 0 and 1 and the in-between Initial and End values are assumed linear, not measured.
pub const F2_ENV_INITIAL: u16 = 12;
pub const F2_ENV_PEAK: u16 = 13;
pub const F2_ENV_END: u16 = 14;
/// Filter envelope Time<Vel, percent (-100 to 100): every time of the envelope is multiplied by `e^(-1.4 x percent / 100 x velocity / 127)`. Measured at +100 % (decay scales 0.25 at
/// velocity 127 and 0.7 at 32, the attack and release likewise); other percentages and the negative side are assumed to follow the same law.
pub const F2_ENV_TIMEVEL: u16 = 15;
/// Filter envelope Mode (0 None, 1 Loop, 2 Beat) and Repeat (the LFO's Synced ladder, 3 = 1/8T by default). **Beat** is measured: the envelope strikes again at every Repeat period at the
/// GLO tempo (1/8T at 120 restarted it every 0.175 to 0.185 s, the same engine factor as the LFO's sync, and each restart peaks as high as the first strike). **Loop** is accepted and does
/// nothing here: with Loop Times of 3 ms, 100 ms and 2.45 s, sustain 0 and 40 %, the cutoff followed the plain envelope, so what it loops is not understood.
pub const F2_ENV_MODE: u16 = 16;
pub const F2_ENV_REPEAT: u16 = 17;
pub const F2_PARAMS: u16 = 18;
const F_ENV_OCTAVES: f64 = 9.0;
/// The Synced menu's lengths in quarter notes.
/// How often OPERATOR's Noise LFO draws a new white value (fitted from three rates: the spread went as the square root of the rate, 0.126 of full scale at 5.8 Hz).
const NOISE_HZ: f64 = 400.0;
const SYNC_QUARTERS: [f64; 15] = [0.125, 1.0 / 6.0, 0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 0.75, 1.0, 4.0 / 3.0, 1.5, 2.0, 3.0, 4.0, 8.0, 16.0];
/// Bit 4 of `L_DEST`: the filter's cutoff follows the LFO. Measured: the cutoff swings by plus and minus this many octaves at 100 % (a sine, geometrically about its own value).
pub const L_FILTER_BIT: i32 = 16;
const L_FILTER_OCTAVES: f64 = 2.84;
/// Dest B's Filter Freq swings the cutoff by this many octaves either way at 100 % (0.75 octave peak to peak at 10 %; Dest A's is 2.84).
const L_FILTER_B_OCTAVES: f64 = 3.75;
/// Dest B's Fixed Freq swings a fixed operator's frequency by this many octaves either way at 100 % (+-0.40 octave at 10 %, the same on 92 and 306 Hz, symmetric in log).
const L_FIXED_B_OCTAVES: f64 = 4.0;
pub const P_COUNT: usize = F2_BASE as usize + F2_PARAMS as usize;
const MULTI: [f64; 5] = [0.001, 0.01, 0.1, 1.0, 10.0];

/// Parameter id of operator `op` (0 to 3), field `E_*` of the second block.
pub const fn ext_param(op: usize, field: u16) -> u16 {
    EXT_BASE + op as u16 * EXT_PARAMS + field
}
/// The waveform names, in OPERATOR 0.7's order: Sine, the sine in 4 and 8 bits, a saw of N harmonics (3 4 6 8 16 32 64) and an ideal saw (D), a square of N odd
/// harmonics and an ideal square. A table of 2048 points each, built once, peak 1 (measured: harmonics at 1/k, every wave the same peak level).
pub const WAVES: [&str; 19] = [
    "SINE", "SINE 4 BIT", "SINE 8 BIT", "SAW 3", "SAW 4", "SAW 6", "SAW 8", "SAW 16", "SAW 32", "SAW 64", "SAW D", "SQUARE 3", "SQUARE 4", "SQUARE 6", "SQUARE 8", "SQUARE 16",
    "SQUARE 32", "SQUARE 64", "SQUARE D",
];
const TABLE: usize = 2048;

/// One cycle of waveform `w`, `TABLE` points plus a repeated first for interpolation, scaled to peak 1.
fn make_wave(w: usize) -> Vec<f32> {
    let n_of = |i: usize| [3usize, 4, 6, 8, 16, 32, 64][i];
    let mut t: Vec<f64> = (0..TABLE)
        .map(|i| {
            let th = TAU * i as f64 / TABLE as f64;
            let ph = i as f64 / TABLE as f64;
            match w {
                0 => th.sin(),
                1 | 2 => {
                    let q = if w == 1 { 16.0 } else { 256.0 };
                    ((th.sin() + 1.0) * 0.5 * (q - 1.0)).round() / (q - 1.0) * 2.0 - 1.0
                }
                3..=9 => (1..=n_of(w - 3)).map(|k| (th * k as f64).sin() / k as f64).sum(),
                10 => 1.0 - 2.0 * ph,
                11..=17 => (0..n_of(w - 11)).map(|j| (th * (2 * j + 1) as f64).sin() / (2 * j + 1) as f64).sum(),
                _ => {
                    if ph < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
            }
        })
        .collect();
    let peak = t.iter().fold(0.0f64, |a, v| a.max(v.abs())).max(1e-9);
    t.iter_mut().for_each(|v| *v /= peak);
    let mut out: Vec<f32> = t.iter().map(|v| *v as f32).collect();
    out.push(out[0]);
    out
}

/// Parameter id of operator `op` (0 to 3), field `field` (`O_*`).
pub const fn op_param(op: usize, field: u16) -> u16 {
    OP_BASE + op as u16 * OP_PARAMS + field
}

/// name, min, max, default, unit
const GLOBAL: [(&str, i32, i32, i32, &str); 7] = [("ALGORITHM (1-11)", 0, 10, 0, ""), ("FEEDBACK AT 100% (0.01 RAD)", 0, 1000, 310, ""), ("LEVEL", 0, 100, 70, "%"), ("MOD INDEX (0.01 RAD)", 1, 2000, 400, ""), ("LEVEL LAW (0 DB STEPS 1 LINEAR)", 0, 1, 0, ""), ("RATIO LAW (0 PERCENT 1 THOUSANDTHS)", 0, 1, 0, ""), ("SUSTAIN LAW (0 LINEAR 1 DB)", 0, 1, 0, "")];
const PER_OP: [(&str, i32, i32, i32, &str); OP_PARAMS as usize] = [
    ("RATIO (0 = 0.5)", 0, 48, 1, ""),
    ("FINE RATIO", 0, 999, 0, ""),
    ("DETUNE", -50, 50, 0, "cents"),
    ("LEVEL", 0, 100, 99, ""),
    ("ATTACK", 0, 5000, 2, "ms"),
    ("DECAY", 1, 5000, 400, "ms"),
    ("SUSTAIN", 0, 99, 60, ""),
    ("RELEASE", 1, 5000, 250, "ms"),
    ("VELOCITY SENS", 0, 100, 50, "%"),
    ("KEY SCALING", 0, 7, 0, ""),
    ("FEEDBACK", 0, 100, 0, "%"),
];

fn desc(id: usize) -> Option<(String, i32, i32, i32, &'static str)> {
    if id < GLOBAL.len() {
        let d = GLOBAL[id];
        return Some((d.0.to_string(), d.1, d.2, d.3, d.4));
    }
    if id >= F2_BASE as usize {
        let (name, lo, hi, def, unit): (&str, i32, i32, i32, &'static str) = match (id - F2_BASE as usize) as u16 {
            F2_MORPH => ("FILTER MORPH (TYPE 4)", 0, 100, 0, "%"),
            F2_CIRCUIT => ("FILTER CIRCUIT (0 CLEAN 1-4 SATURATING)", 0, 4, 0, ""),
            F2_DRIVE => ("FILTER DRIVE (0.1 DB)", 0, 240, 0, ""),
            F2_SHAPER => ("SHAPER (0 OFF 1 SOFT 2 HARD 3 SINE 4 4 BIT)", 0, 4, 0, ""),
            F2_SHP_DRIVE => ("SHAPER DRIVE (0.1 DB)", 0, 120, 0, ""),
            F2_DRYWET => ("SHAPER DRY/WET", 0, 100, 100, "%"),
            F2_FREQ_VEL => ("FILTER FREQ<VEL", -100, 100, 0, "%"),
            F2_ENV_AMOUNT => ("FILTER ENVELOPE AMOUNT", -100, 100, 0, "%"),
            F2_ENV_ATTACK => ("FILTER ENV ATTACK (0.1 MS)", 1, 50_000, 1, ""),
            F2_ENV_DECAY => ("FILTER ENV DECAY", 1, 10_000, 598, "ms"),
            F2_ENV_SUSTAIN => ("FILTER ENV SUSTAIN", 0, 100, 0, "%"),
            F2_ENV_RELEASE => ("FILTER ENV RELEASE", 1, 10_000, 50, "ms"),
            F2_ENV_INITIAL => ("FILTER ENV INITIAL", 0, 100, 0, "%"),
            F2_ENV_PEAK => ("FILTER ENV PEAK", 0, 100, 100, "%"),
            F2_ENV_END => ("FILTER ENV END", 0, 100, 0, "%"),
            F2_ENV_TIMEVEL => ("FILTER ENV TIME<VEL", -100, 100, 0, "%"),
            F2_ENV_MODE => ("FILTER ENV MODE (0 NONE 1 LOOP 2 BEAT)", 0, 2, 0, ""),
            F2_ENV_REPEAT => ("FILTER ENV REPEAT (LFO SYNCED INDEX)", 0, 14, 3, ""),
            _ => return None,
        };
        return Some((name.to_string(), lo, hi, def, unit));
    }
    if id >= LFO_BASE as usize {
        let (name, lo, hi, def, unit): (&str, i32, i32, i32, &'static str) = match (id - LFO_BASE as usize) as u16 {
            L_ON => ("LFO (0 OFF 1 ON)", 0, 1, 0, ""),
            L_TYPE => ("LFO TYPE (0 SINE 1 SQR 2 TRI 3 SAW UP 4 SAW DN 5 S&H 6 NOISE)", 0, 6, 0, ""),
            L_RATE => ("LFO RATE (0.01 HZ, TO 25 KHZ)", 1, 2_500_000, 632, ""),
            L_AMOUNT => ("LFO AMOUNT (PITCH)", 0, 100, 10, "%"),
            L_RETRIG => ("LFO RETRIG (0 FREE 1 PER NOTE)", 0, 1, 1, ""),
            L_DEST => ("LFO TARGETS (BITS: 1-8 OPERATORS 16 FILTER)", 0, 31, 15, ""),
            L_AMOUNT_A => ("LFO AMOUNT A", -100, 100, 100, "%"),
            L_DEST_B => ("LFO DEST B (1-4 OSC VOL, 7 FEEDBACK, 9 FM DRIVE, 8 FIXED FREQ, 10-12 FILTER FREQ Q RES, 20 VOLUME)", 0, 23, 0, ""),
            L_AMOUNT_B => ("LFO AMOUNT B", -100, 100, 100, "%"),
            L_RATE_KEY => ("LFO RATE<KEY", -100, 100, 0, "%"),
            L_AMT_VEL => ("LFO AMT<VEL", 0, 100, 0, "%"),
            L_SYNC => ("LFO SYNC (0 FREE 1 TEMPO)", 0, 1, 0, ""),
            L_SYNC_DIV => ("LFO SYNCED (0 1/32 ... 3 1/8T ... 14 4 BARS)", 0, 14, 3, ""),
            L_TEMPO => ("LFO TEMPO", 30, 300, 120, "bpm"),
            _ => return None,
        };
        return Some((name.to_string(), lo, hi, def, unit));
    }
    if id >= FILT_BASE as usize {
        let (name, lo, hi, def, unit): (&str, i32, i32, i32, &'static str) = match (id - FILT_BASE as usize) as u16 {
            F_ON => ("FILTER (0 OFF 1 ON)", 0, 1, 0, ""),
            F_TYPE => ("FILTER TYPE (0 LP 1 HP 2 BP 3 NOTCH 4 MORPH)", 0, 4, 0, ""),
            F_FREQ => ("FILTER FREQ", 30, 20000, 12000, "Hz"),
            F_RES => ("FILTER RES (0.01)", 0, 125, 27, ""),
            F_SLOPE => ("FILTER SLOPE (0 12 DB 1 24 DB)", 0, 1, 1, ""),
            F_KEY => ("FILTER KEY TRACK", -100, 100, 100, "%"),
            _ => return None,
        };
        return Some((name.to_string(), lo, hi, def, unit));
    }
    if id >= EXT_BASE as usize {
        let i = id - EXT_BASE as usize;
        if i >= OPS * EXT_PARAMS as usize {
            return None;
        }
        let (op, f) = (i / EXT_PARAMS as usize, i % EXT_PARAMS as usize);
        let (name, lo, hi, def, unit): (&str, i32, i32, i32, &'static str) = match f as u16 {
            E_WAVE => ("WAVE (0 SINE ... 18 SQUARE D)", 0, WAVES.len() as i32 - 1, 0, ""),
            E_PHASE => ("PHASE", 0, 100, 0, "%"),
            E_KEYSCALE => ("KEY SCALE (ABOUT NOTE 60)", -100, 100, 0, "%"),
            E_FIXED => ("FIXED (0 OFF 1 ON)", 0, 1, 0, ""),
            E_FIXFREQ => ("FIXED FREQ", 10, 1024, 100, "Hz"),
            _ => ("FIXED MULTI (0 X0.001 ... 4 X10)", 0, 4, 3, ""),
        };
        return Some((format!("OP{} {}", op + 1, name), lo, hi, def, unit));
    }
    let i = id.checked_sub(OP_BASE as usize)?;
    if i >= OPS * OP_PARAMS as usize {
        return None;
    }
    let (op, f) = (i / OP_PARAMS as usize, i % OP_PARAMS as usize);
    let d = PER_OP[f];
    // the default of a quiet operator: only OP1 is loud, so a fresh patch is one clean sine until the player opens the others
    let default = if f == O_LEVEL as usize && op > 0 { 0 } else { d.3 };
    Some((format!("OP{} {}", op + 1, d.0), d.1, d.2, default, d.4))
}

/// For each algorithm: `modulators[op]` is the mask of operators (bit n = OPn+1) that modulate `op`, and `carriers` the mask of those heard. Operators are
/// computed 4, 3, 2, 1, so an operator is modulated only by a higher-numbered one.
struct Algorithm {
    modulators: [u8; OPS],
    carriers: u8,
}

const fn m(ops: &[usize]) -> u8 {
    let mut i = 0;
    let mut v = 0u8;
    while i < ops.len() {
        v |= 1 << ops[i];
        i += 1;
    }
    v
}

/// OPERATOR 0.7's eleven algorithms, read off its panel's diagrams (2026-11-18; A is OP1, D is OP4; algorithms 1 and 2 are confirmed by their sound, the rest
/// by the drawing and, where `fm4fit` was run, by the spectrum, docs/PORTING.md).
const ALGORITHMS: [Algorithm; 11] = [
    Algorithm { modulators: [m(&[1]), m(&[2]), m(&[3]), 0], carriers: m(&[0]) },       //  1: D > C > B > A
    Algorithm { modulators: [m(&[1]), m(&[2, 3]), 0, 0], carriers: m(&[0]) },          //  2: (C + D) > B > A
    Algorithm { modulators: [m(&[1, 3]), m(&[2]), 0, 0], carriers: m(&[0]) },          //  3: C > B > A, D > A
    Algorithm { modulators: [m(&[1, 2]), 0, m(&[3]), 0], carriers: m(&[0]) },          //  4: D > C > A, B > A
    Algorithm { modulators: [m(&[1, 2]), m(&[3]), m(&[3]), 0], carriers: m(&[0]) },    //  5: D > B > A, D > C > A
    Algorithm { modulators: [m(&[1, 2, 3]), 0, 0, 0], carriers: m(&[0]) },             //  6: B + C + D > A
    Algorithm { modulators: [m(&[1]), 0, m(&[3]), 0], carriers: m(&[0, 2]) },          //  7: B > A and D > C
    Algorithm { modulators: [0, m(&[2]), m(&[3]), 0], carriers: m(&[0, 1]) },          //  8: D > C > B, A alone
    Algorithm { modulators: [m(&[3]), m(&[3]), m(&[3]), 0], carriers: m(&[0, 1, 2]) }, //  9: D > A, B, C
    Algorithm { modulators: [0, 0, m(&[3]), 0], carriers: m(&[0, 1, 2]) },             // 10: D > C, A and B alone
    Algorithm { modulators: [0, 0, 0, 0], carriers: m(&[0, 1, 2, 3]) },                // 11: all four alone
];

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy)]
struct Op {
    stage: Stage,
    ph: f64,
    env: f64,
    out: f64,
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    note: u8,
    held: bool,
    age: u64,
    f0: f64,
    /// where the pitch is going (`f0` slides to it when glide is on)
    tf0: f64,
    /// this note's LFO phase (0 to 1) and its held random value
    lph: f64,
    lval: f64,
    /// the Noise type's low-passed value and the counter of its 400 Hz white-noise updates
    lnz: f64,
    nacc: f64,
    /// the filter envelope: seconds since the strike, its value, the value and seconds at release
    fe_t: f64,
    fe_val: f64,
    fe_r0: f64,
    fe_tr: f64,
    vel: f64,
    ops: [Op; OPS],
    fb: [[f64; 2]; OPS],
    /// the filter's two stages, transposed direct form II: (z1, z2) each
    fz: [[f64; 2]; 2],
}

const IDLE_OP: Op = Op { stage: Stage::Idle, ph: 0.0, env: 0.0, out: 0.0 };
const IDLE: Voice = Voice { active: false, note: 0, held: false, age: 0, f0: 0.0, tf0: 0.0, lph: 0.0, lval: 0.0, lnz: 0.0, nacc: 0.0, fe_t: 0.0, fe_val: 0.0, fe_r0: 0.0, fe_tr: 0.0, vel: 0.0, ops: [IDLE_OP; OPS], fb: [[0.0; 2]; OPS], fz: [[0.0; 2]; 2] };

pub struct Fm4 {
    rate: f64,
    p: [i32; P_COUNT],
    voices: [Voice; VOICES],
    clock: u64,
    bend: f64,
    bend_raw: i32,
    bend_semis: f64,
    /// portamento time in ms (0 off), and the pitch of the last note started
    glide_ms: i32,
    last_f0: f64,
    /// the free-running LFO's phase and the noise generator
    lfo_ph: f64,
    rng: u32,
    waves: Vec<Vec<f32>>,
}

/// A two-pole filter's coefficients `[b0, b1, b2, a1, a2]` (a0 = 1) for a corner of `fc` Hz: the bilinear form of `1 / (s^2 + s / Q + 1)`, its high-pass, band-pass (**peak gain
/// Q**, as OPERATOR's: a constant-skirt band-pass) and notch relatives, and for kind 4 the Morph mix of the three outputs (`morph` 0 to 1; all share the denominator, so only the numerator blends).
fn biquad(kind: i32, fc: f64, q: f64, rate: f64, morph: f64) -> [f64; 5] {
    let w = TAU * fc / rate;
    let (sn, cs) = (w.sin(), w.cos());
    let al = sn / (2.0 * q);
    let a0 = 1.0 + al;
    let lp = ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0);
    let hp = ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0);
    let bp = (sn / 2.0, 0.0, -sn / 2.0);
    let (b0, b1, b2) = match kind {
        1 => hp,
        2 => bp,
        3 => (lp.0 + hp.0, lp.1 + hp.1, lp.2 + hp.2),
        4 => {
            let m = morph.clamp(0.0, 1.0);
            let (cl, cb, ch) = if m < 0.25 {
                (1.0 - m * 4.0, m * 4.0, 0.0)
            } else if m < 0.5 {
                (0.0, 1.0 - (m - 0.25) * 4.0, (m - 0.25) * 4.0)
            } else if m < 0.75 {
                ((m - 0.5) * 4.0, 0.0, 1.0)
            } else {
                (1.0, 0.0, 1.0 - (m - 0.75) * 4.0)
            };
            (cl * lp.0 + cb * bp.0 + ch * hp.0, cl * lp.1 + cb * bp.1 + ch * hp.1, cl * lp.2 + cb * bp.2 + ch * hp.2)
        }
        _ => lp,
    };
    [b0 / a0, b1 / a0, b2 / a0, -2.0 * cs / a0, (1.0 - al) / a0]
}

/// The amplitude of a level 0 to 99: 0.75 dB a step down from 99 (a DX-like law), and silence at 0.
fn amp_of(level: i32, linear: bool) -> f64 {
    if linear {
        return level.clamp(0, 100) as f64 / 100.0;
    }
    let level = level.min(99);
    if level <= 0 {
        0.0
    } else {
        10f64.powf((level - 99) as f64 * 0.75 / 20.0)
    }
}

impl Fm4 {
    pub fn new(rate: f64) -> Fm4 {
        let rate = rate.clamp(8000.0, 192_000.0);
        let mut p = [0; P_COUNT];
        for (i, v) in p.iter_mut().enumerate() {
            if let Some(d) = desc(i) {
                *v = d.3;
            }
        }
        Fm4 { rate, p, voices: [IDLE; VOICES], clock: 0, bend: 1.0, bend_raw: 8192, bend_semis: 2.0, glide_ms: 0, last_f0: 0.0, lfo_ph: 0.0, rng: 0x1234_5678, waves: (0..WAVES.len()).map(make_wave).collect() }
    }

    fn op(&self, op: usize, field: u16) -> i32 {
        self.p[op_param(op, field) as usize]
    }

    fn ratio(&self, op: usize) -> f64 {
        let c = self.op(op, O_RATIO);
        let base = if c == 0 { 0.5 } else { c as f64 };
        let fine = self.op(op, O_FINE) as f64;
        let ratio = if self.p[P_RATIO_LAW as usize] == 1 { base + fine / 1000.0 } else { base * (1.0 + fine.min(99.0) / 100.0) };
        ratio * 2f64.powf(self.op(op, O_DETUNE) as f64 / 1200.0)
    }
}

impl Engine for Fm4 {
    fn kind(&self) -> &'static str {
        "fm4"
    }

    fn params(&self) -> Vec<ParamDesc> {
        (0..P_COUNT)
            .filter_map(|i| desc(i).map(|d| ParamDesc { id: i as u16, name: d.0, min: d.1, max: d.2, default: d.3, unit: d.4 }))
            .collect()
    }

    fn get(&self, id: u16) -> Option<i32> {
        desc(id as usize).map(|_| self.p[id as usize])
    }

    fn set(&mut self, id: u16, value: i32) -> bool {
        let Some(d) = desc(id as usize) else { return false };
        self.p[id as usize] = value.clamp(d.1, d.2);
        true
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(note);
        }
        let note = note.min(127);
        self.clock += 1;
        let i = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => (0..VOICES).min_by_key(|&i| (self.voices[i].held, self.voices[i].age)).unwrap_or(0),
        };
        let mut v = IDLE;
        v.active = true;
        v.note = note;
        v.held = true;
        v.age = self.clock;
        let f0 = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
        v.f0 = if self.glide_ms > 0 && self.last_f0 > 0.0 { self.last_f0 } else { f0 };
        v.tf0 = f0;
        v.lph = 0.0;
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        v.lval = f64::from(self.rng >> 8) / f64::from(1u32 << 23) - 1.0;
        self.last_f0 = f0;
        v.vel = velocity.min(127) as f64 / 127.0;
        for (k, o) in v.ops.iter_mut().enumerate() {
            o.stage = Stage::Attack;
            o.ph = self.p[ext_param(k, E_PHASE) as usize] as f64 / 100.0;
        }
        self.voices[i] = v;
    }

    fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.active && v.held && v.note == note {
                v.held = false;
                for o in &mut v.ops {
                    o.stage = Stage::Release;
                }
            }
        }
    }

    fn all_off(&mut self) {
        for v in &mut self.voices {
            if v.active {
                v.held = false;
                for o in &mut v.ops {
                    o.stage = Stage::Release;
                }
            }
        }
    }

    fn glide(&mut self, ms: i32) {
        self.glide_ms = ms.clamp(0, 5000);
    }

    fn legato(&mut self, from: u8, to: u8) -> bool {
        let to = to.min(127);
        let Some(v) = self.voices.iter_mut().find(|v| v.active && v.held && v.note == from) else { return false };
        v.note = to;
        v.tf0 = 440.0 * 2f64.powf((f64::from(to) - 69.0) / 12.0);
        self.last_f0 = v.tf0;
        true
    }

    fn bend_range(&mut self, semitones: i32) {
        self.bend_semis = f64::from(semitones.clamp(0, 24));
        self.bend = 2f64.powf(f64::from(self.bend_raw - 8192) / 8192.0 * self.bend_semis / 12.0);
    }

    fn pitch_bend(&mut self, value: i32) {
        self.bend_raw = value.clamp(0, 16383);
        self.bend = 2f64.powf(f64::from(self.bend_raw - 8192) / 8192.0 * self.bend_semis / 12.0);
    }

    fn render(&mut self, out: &mut [i16]) {
        let rate = self.rate;
        // per operator, once a block: the envelope's three rates, the sustain, the level, the ratio and the velocity law
        let mut ratio = [0.0; OPS];
        let mut a = [[0.0; 3]; OPS];
        let mut sustain = [0.0; OPS];
        let mut wave = [0usize; OPS];
        let mut fixed = [0.0; OPS]; // a fixed operator's frequency in Hz, or 0
        let mut kscale2 = [0.0; OPS];
        let mut level = [0.0; OPS];
        let mut vsens = [0.0; OPS];
        let mut kscale = [0.0; OPS];
        // decay and release times are the time to fall 64 dB (fitted to OPERATOR 0.7: 399 ms fell 64 dB, 16 dB a 100 ms); the attack aims past 1 and stops at 1
        let fall = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / (ENV_DB / 8.685_889_6))).exp();
        let step = |ms: i32| 1.0 - (-1.0 / (ms.max(1) as f64 * 0.001 * rate / 4.6)).exp();
        for op in 0..OPS {
            ratio[op] = self.ratio(op);
            a[op] = [step(self.op(op, O_ATTACK).max(1)), fall(self.op(op, O_DECAY)), fall(self.op(op, O_RELEASE))];
            let sv = self.op(op, O_SUSTAIN);
            sustain[op] = if self.p[P_SUSTAIN_LAW as usize] == 1 { 10f64.powf((sv - 99) as f64 / 20.0) } else { sv as f64 / 99.0 };
            wave[op] = self.p[ext_param(op, E_WAVE) as usize].clamp(0, WAVES.len() as i32 - 1) as usize;
            if self.p[ext_param(op, E_FIXED) as usize] == 1 {
                fixed[op] = self.p[ext_param(op, E_FIXFREQ) as usize] as f64 * MULTI[self.p[ext_param(op, E_FIXMULTI) as usize].clamp(0, 4) as usize];
            }
            kscale2[op] = self.p[ext_param(op, E_KEYSCALE) as usize] as f64 / 100.0;
            level[op] = amp_of(self.op(op, O_LEVEL), self.p[P_LEVEL_LAW as usize] == 1);
            vsens[op] = self.op(op, O_VELOCITY) as f64 / 100.0;
            kscale[op] = self.op(op, O_KEYSCALE) as f64 / 7.0;
        }
        let alg = &ALGORITHMS[self.p[P_ALGORITHM as usize].clamp(0, 10) as usize];
        // feedback: each operator's own output (level and envelope included) fed back into its phase, the mean of the last two samples, up to P_FEEDBACK at 100 %
        let fb_max = self.p[P_FEEDBACK as usize] as f64 / 100.0;
        let mut fb = [0.0; OPS];
        let mut fbf = [0.0; OPS];
        for (op, f) in fb.iter_mut().enumerate() {
            fbf[op] = self.op(op, O_FEEDBACK) as f64 / 100.0;
            *f = fbf[op] * fb_max;
        }
        let master = self.p[P_LEVEL as usize] as f64 / 100.0 * 0.3;
        let live = self.voices.iter().filter(|v| v.active).count().max(1);
        let poly = if live > 4 { (4.0 / live as f64).sqrt() } else { 1.0 };
        let bend = self.bend;
        // the filter: per voice (the corner follows the note), computed once a block
        let f_on = self.p[(FILT_BASE + F_ON) as usize] == 1;
        let f_stages = if self.p[(FILT_BASE + F_SLOPE) as usize] == 1 { 2 } else { 1 };
        let f_res = (self.p[(FILT_BASE + F_RES) as usize] as f64 / 100.0).min(0.93);
        let f_q = 0.5 / (1.0 - 0.97 * f_res);
        let mut fco = [[0.0f64; 5]; VOICES];
        let mut fkey = [0.0f64; VOICES];
        let f_kind = self.p[(FILT_BASE + F_TYPE) as usize];
        let f_morph = f64::from(self.p[(F2_BASE + F2_MORPH) as usize]) / 100.0;
        let f_sat = self.p[(F2_BASE + F2_CIRCUIT) as usize] > 0;
        let f_fv = f64::from(self.p[(F2_BASE + F2_FREQ_VEL) as usize]) / 100.0;
        let f_env_amt = f64::from(self.p[(F2_BASE + F2_ENV_AMOUNT) as usize]) / 100.0;
        let f_env_on = f_on && f_env_amt != 0.0;
        let fe_att = f64::from(self.p[(F2_BASE + F2_ENV_ATTACK) as usize]) / 10_000.0;
        let fe_sus = f64::from(self.p[(F2_BASE + F2_ENV_SUSTAIN) as usize]) / 100.0;
        let fe_scale = f64::from(self.p[(F2_BASE + F2_ENV_DECAY) as usize]) / 598.0;
        let fe_rel = f64::from(self.p[(F2_BASE + F2_ENV_RELEASE) as usize]) / 1000.0;
        let fe_init = f64::from(self.p[(F2_BASE + F2_ENV_INITIAL) as usize]) / 100.0;
        let fe_peak = f64::from(self.p[(F2_BASE + F2_ENV_PEAK) as usize]) / 100.0;
        let fe_end = f64::from(self.p[(F2_BASE + F2_ENV_END) as usize]) / 100.0;
        let fe_tv = f64::from(self.p[(F2_BASE + F2_ENV_TIMEVEL) as usize]) / 100.0;
        let fe_beat = if self.p[(F2_BASE + F2_ENV_MODE) as usize] == 2 {
            60.0 / f64::from(self.p[(LFO_BASE + L_TEMPO) as usize]).max(1.0) * SYNC_QUARTERS[self.p[(F2_BASE + F2_ENV_REPEAT) as usize].clamp(0, 14) as usize]
        } else {
            0.0
        };
        // the release fell more slowly than a time constant of rel / 4.6 for long times: about rel / (4.9 (rel / 41 ms)^0.24) fits 41 to 128 ms (fitted to the sound, `fm4fenv`)
        let fe_rel_tau = fe_rel.max(1e-4) / (4.9 * (fe_rel.max(1e-4) / 0.041).powf(0.24));
        let shaper = self.p[(F2_BASE + F2_SHAPER) as usize];
        let shp_gain = 10f64.powf(f64::from(self.p[(F2_BASE + F2_SHP_DRIVE) as usize]) / 200.0);
        let shp_wet = f64::from(self.p[(F2_BASE + F2_DRYWET) as usize]) / 100.0;
        let f_drive = 0.96 * 10f64.powf(f64::from(self.p[(F2_BASE + F2_DRIVE) as usize]) / 200.0);
        if f_on {
            let kind = self.p[(FILT_BASE + F_TYPE) as usize];
            for (i, v) in self.voices.iter().enumerate() {
                let keyed = self.p[(FILT_BASE + F_FREQ) as usize] as f64 * 2f64.powf(self.p[(FILT_BASE + F_KEY) as usize] as f64 / 100.0 * (v.note as f64 - 60.0) / 12.0);
                let keyed = keyed * 2f64.powf(4.0 * f_fv * v.vel);
                fkey[i] = keyed;
                fco[i] = biquad(kind, keyed.clamp(5.0, rate * 0.45), f_q, rate, f_morph);
            }
        }
        // the pitch slides in the log domain, 99% of the way in the glide time, stepped every eighth sample
        let slide = if self.glide_ms > 0 { (-4.6 * 8.0 / (f64::from(self.glide_ms) * 0.001 * rate)).exp() } else { 0.0 };
        // the LFO (pitch): its phase step a sample, its depth in octaves at a value of 1
        let lfo_on = self.p[(LFO_BASE + L_ON) as usize] == 1;
        let (l_type, l_retrig, l_dest) = (self.p[(LFO_BASE + L_TYPE) as usize], self.p[(LFO_BASE + L_RETRIG) as usize] == 1, self.p[(LFO_BASE + L_DEST) as usize]);
        let l_hz = if self.p[(LFO_BASE + L_SYNC) as usize] == 1 {
            f64::from(self.p[(LFO_BASE + L_TEMPO) as usize]) / 60.0 / SYNC_QUARTERS[self.p[(LFO_BASE + L_SYNC_DIV) as usize].clamp(0, 14) as usize]
        } else {
            f64::from(self.p[(LFO_BASE + L_RATE) as usize]) / 100.0
        };
        let l_dph = l_hz / rate;
        let l_depth = f64::from(self.p[(LFO_BASE + L_AMOUNT) as usize]) / 100.0 * f64::from(self.p[(LFO_BASE + L_AMOUNT_A) as usize]) / 100.0;
        let l_filter = lfo_on && l_dest & L_FILTER_BIT != 0;
        let l_dest_b = self.p[(LFO_BASE + L_DEST_B) as usize];
        let l_depth_b = f64::from(self.p[(LFO_BASE + L_AMOUNT_B) as usize]) / 100.0 * f64::from(self.p[(LFO_BASE + L_AMOUNT) as usize]) / 100.0;
        let (l_ratekey, l_amtvel) = (f64::from(self.p[(LFO_BASE + L_RATE_KEY) as usize]) / 100.0, f64::from(self.p[(LFO_BASE + L_AMT_VEL) as usize]) / 100.0);
        let (mut free_ph, mut rng) = (self.lfo_ph, self.rng);
        let index = self.p[P_INDEX as usize] as f64 / 100.0; // the phase swing (radians) of a modulator at full level
        for (n, o) in out.iter_mut().enumerate() {
            let refresh = n & 7 == 0;
            let mut acc = 0.0;
            for (vi, v) in self.voices.iter_mut().enumerate().filter(|(_, v)| v.active) {
                if refresh && v.f0 != v.tf0 {
                    v.f0 = if slide == 0.0 { v.tf0 } else { v.tf0 * (v.f0 / v.tf0).powf(slide) };
                    if (v.f0 / v.tf0 - 1.0).abs() < 1e-5 {
                        v.f0 = v.tf0;
                    }
                }
                // this note's LFO: its rate follows the key, its depth the velocity
                let ratek = if l_retrig { 2f64.powf(l_ratekey * (f64::from(v.note) - 60.0) / 12.0) } else { 1.0 };
                let vs = (1.0 - l_amtvel) + l_amtvel * v.vel;
                let (depth_v, depth_bv) = (l_depth * vs, l_depth_b * vs);
                let mut lmul = 1.0;
                let mut lshape = 0.0;
                let mut gain_b = 1.0;
                let mut fm_gain = 1.0;
                if lfo_on {
                    let prev = if l_retrig { v.lph } else { free_ph };
                    let ph = if l_retrig { v.lph } else { free_ph };
                    if l_type == 6 {
                        v.nacc += NOISE_HZ / rate;
                        if v.nacc >= 1.0 {
                            v.nacc -= 1.0;
                            rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                            let w = f64::from(rng >> 8) / f64::from(1u32 << 23) - 1.0;
                            v.lnz += (1.0 - (-std::f64::consts::TAU * l_hz * ratek / NOISE_HZ).exp()) * (w - v.lnz);
                        }
                    }
                    let shape = match l_type {
                        0 => (std::f64::consts::TAU * ph).sin(),
                        1 => {
                            if ph < 0.5 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                        2 => {
                            if ph < 0.5 {
                                1.0 - 4.0 * ph
                            } else {
                                4.0 * ph - 3.0
                            }
                        }
                        3 => 2.0 * ph - 1.0,
                        4 => 1.0 - 2.0 * ph,
                        6 => v.lnz,
                        _ => v.lval,
                    };
                    lshape = shape;
                    gain_b = (1.0 + depth_bv * shape).max(0.0);
                    if l_dest_b == 9 {
                        fm_gain = gain_b;
                    }
                    lmul = 2f64.powf(depth_v * shape);
                    let next = ph + l_dph * ratek;
                    let nph = next - next.floor();
                    if l_type == 5 && nph < prev {
                        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        v.lval = f64::from(rng >> 8) / f64::from(1u32 << 23) - 1.0;
                    }
                    if l_retrig {
                        v.lph = nph;
                    }
                }
                let mut heard = 0.0;
                for op in (0..OPS).rev() {
                    let s = &mut v.ops[op];
                    match s.stage {
                        Stage::Attack => {
                            s.env += (1.2 - s.env) * a[op][0];
                            if s.env >= 1.0 {
                                s.env = 1.0;
                                s.stage = Stage::Decay;
                            }
                        }
                        Stage::Decay => s.env += (sustain[op] - s.env) * a[op][1],
                        Stage::Release => s.env -= s.env * a[op][2],
                        Stage::Idle => {}
                    }
                    let mut pm = 0.0;
                    for (k, o) in v.ops.iter().enumerate().take(OPS) {
                        if alg.modulators[op] >> k & 1 == 1 {
                            pm += index * o.out * fm_gain;
                        }
                    }
                    let s = &mut v.ops[op];
                    let fbk = if l_dest_b == 7 && op >= 1 { (fbf[op] + depth_bv * lshape).max(0.0) * fb_max } else { fb[op] };
                    pm += fbk * 0.5 * (v.fb[op][0] + v.fb[op][1]);
                    let inc = if fixed[op] > 0.0 { fixed[op] * if l_dest_b == 8 { 2f64.powf(L_FIXED_B_OCTAVES * depth_bv * lshape) } else { 1.0 } / rate } else { v.f0 * bend * ratio[op] * if l_dest >> op & 1 == 1 { lmul } else { 1.0 } / rate };
                    s.ph += inc;
                    s.ph -= s.ph.floor();
                    // velocity: linear, from (1 - sensitivity) at velocity 0 to 1 at 127 (OPERATOR 0.7 at its default of 50%: half at 0, measured); key scaling: higher notes quieter
                    let vel = 1.0 - vsens[op] * (1.0 - v.vel);
                    let key = 1.0 - kscale[op] * ((v.note as f64 - 36.0) / 91.0).clamp(0.0, 1.0) * 0.75;
                    let key2 = 2f64.powf(kscale2[op] * (v.note as f64 - 60.0) / 12.0);
                    let amp = level[op] * s.env * vel.max(0.05) * key * key2 * if l_dest_b == 1 + op as i32 { gain_b } else { 1.0 };
                    s.out = if wave[op] == 0 {
                        (TAU * (s.ph) + pm).sin() * amp
                    } else {
                        // a table lookup at the phase (cycles) plus the modulation, linearly interpolated
                        let x = (s.ph + pm / TAU).rem_euclid(1.0) * TABLE as f64;
                        let (i, f) = ((x as usize).min(TABLE - 1), x - (x as usize).min(TABLE - 1) as f64);
                        let t = &self.waves[wave[op]];
                        (t[i] as f64 + (t[i + 1] - t[i]) as f64 * f) * amp
                    };
                    v.fb[op][1] = v.fb[op][0];
                    v.fb[op][0] = s.out;
                    if alg.carriers >> op & 1 == 1 {
                        heard += s.out;
                    }
                }
                // a voice ends once every heard operator has released to silence
                if !v.held && (0..OPS).all(|k| alg.carriers >> k & 1 == 0 || v.ops[k].env < 1e-4) {
                    v.active = false;
                }
                if f_on && n & 7 == 0 && (f_env_on || (lfo_on && (l_filter || (10..=12).contains(&l_dest_b)))) {
                    let mut oct = if lfo_on && l_filter { L_FILTER_OCTAVES * depth_v * lshape } else { 0.0 };
                    if f_env_on {
                        let dt = 8.0 / rate;
                        let tv = (-1.4 * fe_tv * v.vel).exp();
                        let (fe_att, fe_scale, fe_rel_tau) = (fe_att * tv, fe_scale * tv, fe_rel_tau * tv);
                        if v.held {
                            v.fe_t += dt;
                            // a linear rise to the peak over the attack, then the fall (fitted to the sound: `fm4fenv`)
                            let fall = |t: f64| (2.2 * (-t / (0.017 * fe_scale)).exp() + 2.9 * (-t / (0.06 * fe_scale)).exp() + 3.4 * (-t / (0.17 * fe_scale)).exp()) / F_ENV_OCTAVES;
                            let unit = fe_sus + (1.0 - fe_sus) * fall(0.0); // the level at Peak 100 %
                            v.fe_val = fe_peak
                                * if v.fe_t < fe_att { unit * (fe_init + (1.0 - fe_init) * v.fe_t / fe_att) } else { fe_sus + (1.0 - fe_sus) * fall(if fe_beat > 0.0 { (v.fe_t - fe_att) % fe_beat } else { v.fe_t - fe_att }) };
                            v.fe_r0 = v.fe_val;
                        } else {
                            v.fe_tr += dt;
                            let end = fe_end * fe_peak * (fe_sus + (1.0 - fe_sus) * (2.2 + 2.9 + 3.4) / F_ENV_OCTAVES);
                            v.fe_val = end + (v.fe_r0 - end) * (-v.fe_tr / fe_rel_tau).exp();
                        }
                        oct += F_ENV_OCTAVES * f_env_amt * v.fe_val;
                    }
                    let mut q = f_q;
                    if lfo_on && l_dest_b == 10 {
                        oct += L_FILTER_B_OCTAVES * depth_bv * lshape;
                    } else if lfo_on && l_dest_b >= 11 {
                        // Filter Q and Filter Res act alike: the resonance goes up by amount x value (to 1.0, where the filter nearly rings)
                        q = 0.5 / (1.0 - 0.97 * (f_res + depth_bv * lshape).clamp(0.0, 1.0));
                    }
                    fco[vi] = biquad(f_kind, (fkey[vi] * 2f64.powf(oct)).clamp(5.0, rate * 0.45), q, rate, f_morph);
                }
                if f_on {
                    if f_sat {
                        let u = heard * f_drive;
                        heard = u / (1.0 + u.abs());
                    }
                    let c = &fco[vi];
                    for z in v.fz.iter_mut().take(f_stages) {
                        let y = c[0] * heard + z[0];
                        z[0] = c[1] * heard - c[3] * y + z[1];
                        z[1] = c[2] * heard - c[4] * y;
                        heard = y;
                    }
                }
                if shaper > 0 {
                    let u = heard * shp_gain;
                    let wet = match shaper {
                        1 => {
                            let u = u * 0.96;
                            u / (1.0 + u.abs())
                        }
                        2 => u.clamp(-1.0, 1.0),
                        3 => (std::f64::consts::FRAC_PI_2 * u).sin(),
                        _ => (u.clamp(-1.0, 1.0) * 8.0).round() / 8.0,
                    };
                    heard += shp_wet * (wet - heard);
                }
                acc += heard * if l_dest_b == 20 { gain_b } else { 1.0 };
            }
            *o = (acc * poly * master * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
            if lfo_on && !l_retrig {
                free_ph += l_dph;
                free_ph -= free_ph.floor();
            }
        }
        self.lfo_ph = free_ph;
        self.rng = rng;
    }

    fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(x: &[i16]) -> u16 {
        x.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0)
    }

    fn crossings(x: &[i16]) -> usize {
        x.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count()
    }

    /// A patch of one audible sine: algorithm 8 (all alone), only OP1 open.
    fn sine() -> Fm4 {
        let mut e = Fm4::new(44_100.0);
        e.set(P_ALGORITHM, 10);
        e.set(op_param(0, O_ATTACK), 1);
        e.set(op_param(0, O_SUSTAIN), 99);
        e
    }

    #[test]
    fn a_fresh_patch_is_one_sine_at_the_right_pitch() {
        let mut e = sine();
        e.note_on(69, 127); // 440 Hz
        let mut buf = vec![0i16; 44_100];
        e.render(&mut buf);
        assert!(peak(&buf) > 1000);
        let n = crossings(&buf[22_050..]);
        assert!((218..=222).contains(&n), "{n} crossings in 0.5 s");
    }

    #[test]
    fn ratio_fine_and_detune_move_the_pitch() {
        let at = |c: i32, fine: i32, cents: i32| {
            let mut e = sine();
            e.set(op_param(0, O_RATIO), c);
            e.set(op_param(0, O_FINE), fine);
            e.set(op_param(0, O_DETUNE), cents);
            e.note_on(57, 127); // 220 Hz
            let mut buf = vec![0i16; 44_100];
            e.render(&mut buf);
            crossings(&buf[22_050..]) as f64 * 2.0
        };
        assert!((at(1, 0, 0) - 220.0).abs() < 6.0);
        assert!((at(2, 0, 0) - 440.0).abs() < 8.0);
        assert!((at(0, 0, 0) - 110.0).abs() < 6.0, "ratio 0 is one half");
        assert!((at(1, 50, 0) - 330.0).abs() < 8.0, "fine 50% is a fifth up");
        assert!(at(1, 0, 50) > at(1, 0, 0));
    }

    #[test]
    fn a_modulator_adds_harmonics_and_more_level_adds_more() {
        // the sum of absolute sample-to-sample steps grows as the spectrum brightens (at a fixed carrier level)
        let bright = |mod_level: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, 0); // 4 > 3 > 2 > 1
            for op in 0..3 {
                e.set(op_param(op, O_LEVEL), 0);
            }
            e.set(op_param(0, O_LEVEL), 99);
            e.set(op_param(1, O_LEVEL), mod_level);
            e.set(op_param(1, O_RATIO), 3);
            e.set(op_param(1, O_SUSTAIN), 99);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.note_on(48, 127);
            let mut buf = vec![0i16; 8820];
            e.render(&mut buf);
            buf[4000..].windows(2).map(|w| (w[1] as f64 - w[0] as f64).abs()).sum::<f64>()
        };
        let none = bright(0);
        let some = bright(80);
        let lots = bright(99);
        assert!(some > none * 1.2 && lots > some, "{none} {some} {lots}");
    }

    #[test]
    fn every_algorithm_sounds_and_the_algorithms_differ() {
        let mut shapes = Vec::new();
        for alg in 0..11 {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, alg);
            for op in 0..OPS {
                e.set(op_param(op, O_LEVEL), 90);
                e.set(op_param(op, O_RATIO), (op as i32 + 1) * 2 - 1);
                e.set(op_param(op, O_SUSTAIN), 90);
            }
            e.note_on(52, 100);
            let mut buf = vec![0i16; 6000];
            e.render(&mut buf);
            assert!(peak(&buf) > 300, "algorithm {} is silent", alg + 1);
            shapes.push(buf);
        }
        for i in 0..11 {
            for j in i + 1..11 {
                assert_ne!(shapes[i], shapes[j], "algorithms {} and {} sound the same", i + 1, j + 1);
            }
        }
    }

    #[test]
    fn feedback_changes_the_tone_and_stays_bounded() {
        let run = |fb: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, 10);
            e.set(op_param(3, O_LEVEL), 99);
            e.set(op_param(0, O_LEVEL), 0);
            e.set(op_param(3, O_FEEDBACK), fb);
            e.note_on(48, 127);
            let mut buf = vec![0i16; 8820];
            e.render(&mut buf);
            buf
        };
        let (clean, fed) = (run(0), run(100));
        assert_ne!(clean, fed);
        let step = |b: &[i16]| b[2000..].windows(2).map(|w| (w[1] as f64 - w[0] as f64).abs()).sum::<f64>();
        assert!(step(&fed) > step(&clean));
    }

    #[test]
    fn plays_releases_frees_and_goes_silent() {
        let mut e = sine();
        let mut buf = vec![0i16; 4410];
        e.note_on(60, 100);
        e.render(&mut buf);
        assert_eq!(e.sounding(), 1);
        e.note_off(60);
        for _ in 0..30 {
            e.render(&mut buf);
        }
        assert_eq!(e.sounding(), 0);
        e.render(&mut buf);
        assert_eq!(peak(&buf), 0);
    }

    #[test]
    fn velocity_sensitivity_and_the_envelope_shape() {
        let level = |vel: u8, sens: i32| {
            let mut e = sine();
            e.set(op_param(0, O_VELOCITY), sens);
            e.note_on(60, vel);
            let mut buf = vec![0i16; 4410];
            e.render(&mut buf);
            peak(&buf)
        };
        assert!(level(20, 100) < level(127, 100) / 2);
        assert!((level(1, 50) as f64 / level(127, 50) as f64 - 0.5).abs() < 0.02, "the default: half at velocity 1");
        let flat = (level(20, 0) as f64 - level(127, 0) as f64).abs() / level(127, 0) as f64;
        assert!(flat < 0.05, "no sensitivity: velocity does nothing ({flat})");
        // a slow attack starts quiet; a decay to a low sustain falls
        let mut e = sine();
        e.set(op_param(0, O_ATTACK), 400);
        e.note_on(60, 127);
        let mut buf = vec![0i16; 22_050];
        e.render(&mut buf);
        assert!(peak(&buf[..1000]) < peak(&buf[18_000..]) / 3);
        let mut d = sine();
        d.set(op_param(0, O_SUSTAIN), 10);
        d.set(op_param(0, O_DECAY), 100);
        d.note_on(60, 127);
        let mut b = vec![0i16; 22_050];
        d.render(&mut b);
        assert!(peak(&b[18_000..]) < peak(&b[..2000]) / 3);
    }

    #[test]
    fn nine_notes_steal_params_clamp_and_the_list_is_whole() {
        let mut e = sine();
        for n in 40..52 {
            e.note_on(n, 127);
        }
        assert!(e.sounding() <= VOICES);
        let mut buf = vec![0i16; 2048];
        e.render(&mut buf);
        e.pitch_bend(0);
        e.render(&mut buf);
        e.pitch_bend(16383);
        e.render(&mut buf);
        e.all_off();
        assert!(e.set(P_LEVEL, 9999) && e.get(P_LEVEL) == Some(100));
        assert!(e.set(op_param(3, O_DETUNE), -999) && e.get(op_param(3, O_DETUNE)) == Some(-50));
        assert!(!e.set(P_COUNT as u16, 1) && e.get(7).is_none() && e.get(P_COUNT as u16).is_none());
        let fresh = Fm4::new(44_100.0);
        let ps = fresh.params();
        assert_eq!(ps.len(), 7 + OPS * OP_PARAMS as usize + OPS * EXT_PARAMS as usize + FILT_PARAMS as usize + LFO_PARAMS as usize + F2_PARAMS as usize);
        assert!(ps.iter().all(|p| p.min <= p.default && p.default <= p.max && fresh.get(p.id) == Some(p.default)));
        assert_eq!(ps.iter().find(|p| p.id == op_param(2, O_LEVEL)).map(|p| p.name.as_str()), Some("OP3 LEVEL"));
    }

    /// The amplitude of a sine at `f` Hz in `x` (a single-frequency Fourier sum over whole periods' worth, Hann-weighted).
    fn tone(x: &[i16], f: f64) -> f64 {
        let n = x.len();
        let (mut c, mut s) = (0.0, 0.0);
        for (i, &v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let a = std::f64::consts::TAU * f * i as f64 / 44_100.0;
            c += v as f64 / 32768.0 * w * a.cos();
            s += v as f64 / 32768.0 * w * a.sin();
        }
        c.hypot(s) / (n as f64 / 4.0)
    }

    /// The numbers measured on OPERATOR 0.7 in the emulator (docs/PORTING.md, 2026-11-18): algorithm 1, note 57 at velocity 127: A alone
    /// 0.0484 peak (-26.3 dBFS), a modulator at full level a whole cycle (6.28 radians) of phase, linear in the level's amplitude, amplitude linear in velocity down to half at velocity 0, and a 399 ms release that falls 16 dB every 100 ms.
    fn operator() -> Fm4 {
        let mut e = Fm4::new(44_100.0);
        e.set(P_ALGORITHM, 0);
        e.set(P_LEVEL, 16);
        e.set(P_INDEX, 628);
        for op in 0..OPS {
            e.set(op_param(op, O_ATTACK), 1);
            e.set(op_param(op, O_DECAY), 998);
            e.set(op_param(op, O_SUSTAIN), 99);
            e.set(op_param(op, O_RELEASE), 399);
            e.set(op_param(op, O_VELOCITY), 50);
            e.set(op_param(op, O_LEVEL), 0);
        }
        e.set(op_param(0, O_LEVEL), 99);
        e
    }

    /// OPERATOR 0.7's waveforms, measured in the emulator (2026-11-19, `fm4fit`, A alone at 0 dB, note 57): a saw of N harmonics has them at 1/k and nothing above N,
    /// a square of N has the first N odd ones at 1/k, and every wave peaks at the same level as the sine (0.0484), so the fundamental sits at 0.0335 (Saw 3), 0.0316
    /// (Saw 4), 0.0298 (Saw 6), 0.0518 (Square 3) and 0.0520 (Square 4) of full scale.
    #[test]
    fn fitted_to_operator_the_waveforms() {
        // (wave, [(harmonic, dB under the fundamental, or None for absent)], fundamental amplitude)
        type Case<'a> = (i32, &'a [(usize, Option<f64>)], f64);
        let cases: [Case; 5] = [
            (3, &[(2, Some(-6.1)), (3, Some(-9.7)), (4, None)], 0.0335),
            (4, &[(4, Some(-12.3)), (5, None)], 0.0316),
            (5, &[(5, Some(-14.4)), (6, Some(-16.2)), (7, None)], 0.0298),
            (11, &[(2, None), (3, Some(-9.7)), (5, Some(-14.4)), (7, None)], 0.0518),
            (12, &[(7, Some(-17.7)), (9, None)], 0.0520),
        ];
        for (w, harmonics, fund) in cases {
            let mut e = operator();
            e.set(ext_param(0, E_WAVE), w);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let x = &b[10_000..10_000 + 22_050];
            assert!((peak(&b[10_000..]) as f64 / 32768.0 - 0.0484).abs() < 0.0015, "wave {w} peak");
            let f1 = tone(x, 220.0);
            assert!((f1 - fund).abs() < 0.0015, "wave {w} fundamental {f1} against {fund}");
            for (h, db) in harmonics {
                let rel = 20.0 * (tone(x, 220.0 * *h as f64) / f1).max(1e-9).log10();
                match db {
                    Some(d) => assert!((rel - d).abs() < 0.8, "wave {w} harmonic {h}: {rel:.1} dB, measured {d}"),
                    None => assert!(rel < -50.0, "wave {w} harmonic {h} should be absent, got {rel:.1}"),
                }
            }
        }
        // the ideal waves and the 4-bit sine are in the table and sound; an out-of-range wave is clamped
        let mut e = operator();
        assert!(e.set(ext_param(3, E_WAVE), 99) && e.get(ext_param(3, E_WAVE)) == Some(18) && e.get(P_COUNT as u16).is_none());
        for w in [1, 2, 10, 18] {
            let mut e = operator();
            e.set(ext_param(0, E_WAVE), w);
            e.note_on(57, 127);
            let mut b = vec![0i16; 22_050];
            e.render(&mut b);
            assert!(peak(&b[5_000..]) > 1000, "wave {w} silent");
        }
    }

    /// The numbers measured on OPERATOR 0.7 (2026-11-19, `fm4fit`): Key scaling pivots on note 60 and doubles the level an octave at +100 % (notes 24 to 96 at +100 %
    /// peaked 0.0061 0.0122 0.0243 0.0484 0.0966 0.1929 0.3847; -100 % mirrored), +73 % gave 0.0292 at note 48 and 0.0802 at 72; Fixed plays Freq x Multi whatever
    /// the note (100 Hz for notes 57 and 69); Phase 66 % starts a sine at -0.83 and falling.
    #[test]
    fn fitted_to_operator_key_scaling_fixed_and_phase() {
        let peak_at = |note: u8, key: i32| {
            let mut e = operator();
            e.set(ext_param(0, E_KEYSCALE), key);
            e.note_on(note, 127);
            let mut b = vec![0i16; 22_050];
            e.render(&mut b);
            peak(&b[8_000..]) as f64 / 32768.0
        };
        for (note, want) in [(24, 0.0061), (48, 0.0243), (60, 0.0484), (84, 0.1929), (96, 0.3847)] {
            assert!((peak_at(note, 100) - want).abs() < want * 0.03, "key +100 note {note}");
            assert!((peak_at(120 - note, -100) - want).abs() < want * 0.03, "key -100 note {}", 120 - note);
        }
        assert!((peak_at(72, 73) - 0.0802).abs() < 0.0025 && (peak_at(48, 73) - 0.0292).abs() < 0.001);
        for note in [57, 69] {
            let mut e = operator();
            e.set(ext_param(0, E_FIXED), 1);
            e.note_on(note, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let x = &b[10_000..10_000 + 22_050];
            assert!(tone(x, 100.0) > 0.04 && tone(x, 200.0) < 0.002, "fixed note {note}");
        }
        let mut e = operator();
        e.set(ext_param(0, E_FIXED), 1);
        e.set(ext_param(0, E_FIXMULTI), 4); // x10
        e.set(ext_param(0, E_FIXFREQ), 127);
        e.note_on(57, 127);
        let mut b = vec![0i16; 44_100];
        e.render(&mut b);
        assert!(tone(&b[10_000..10_000 + 22_050], 1270.0) > 0.04);
        let mut e = operator();
        e.set(ext_param(0, E_PHASE), 66);
        e.note_on(57, 127);
        let mut b = vec![0i16; 2_000];
        e.render(&mut b);
        assert!(b[30] < b[10] && b[10] < 0, "phase 66 starts negative and falling: {} {}", b[10], b[30]);
    }

    /// OPERATOR 0.7's filter (2026-11-19, `fm4fit`; operator A on Saw D, note 45 or 57, Freq<Key at its default +100, Res 0.27): the harmonics' levels agree with two
    /// cascaded two-pole stages (one for 12 dB) whose corner is the displayed Freq times `2^((note - 60) / 12)` and whose Q is 0.65 there, 0.5 at Res 0.
    #[test]
    fn fitted_to_operator_the_filter() {
        let rel = |wave_filter: &dyn Fn(&mut Fm4), note: u8, f0: f64, ks: [usize; 3]| {
            let mut e = operator();
            e.set(ext_param(0, E_WAVE), 10);
            e.set(FILT_BASE + F_ON, 1);
            wave_filter(&mut e);
            e.note_on(note, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let x = &b[8_000..8_000 + 22_050];
            let a: Vec<f64> = ks.iter().map(|&k| 20.0 * (tone(x, f0 * k as f64) * k as f64).log10()).collect();
            a // each harmonic's gain against the saw's own 1/k
        };
        // low-pass 24 dB, display 670 Hz, note 45 (measured gain differences, harmonic 2, 3, 4 against 1: -3.5, -9.9, -17.1)
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 670); }, 45, 110.0, [1, 2, 4]);
        assert!(((g[1] - g[0]) + 3.5).abs() < 1.5 && ((g[2] - g[0]) + 17.1).abs() < 2.0, "lp24 {g:?}");
        // low-pass 12 dB (display about 1.22k at note 45): harmonic 2, 4, 8 against 1: -0.4, -4.6, -10.3 measured
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 1220); e.set(FILT_BASE + F_SLOPE, 0); }, 45, 110.0, [1, 2, 8]);
        assert!(((g[1] - g[0]) + 0.4).abs() < 1.5 && ((g[2] - g[0]) + 10.3).abs() < 2.0, "lp12 {g:?}");
        // high-pass 24 dB, display 670, note 57: harmonic 4 and 6 against 2: +10.3 and +11.8 dB
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 670); e.set(FILT_BASE + F_TYPE, 1); }, 57, 220.0, [2, 4, 6]);
        assert!(((g[1] - g[0]) - 10.3).abs() < 2.0 && ((g[2] - g[0]) - 11.8).abs() < 2.0, "hp24 {g:?}");
        // the notch cuts the harmonics near its corner: 2 and 3 are 20 dB and 35 dB under (note 57)
        let g = rel(&|e| { e.set(FILT_BASE + F_FREQ, 670); e.set(FILT_BASE + F_TYPE, 3); }, 57, 220.0, [1, 2, 3]);
        assert!(g[0] - g[1] > 8.0 && g[0] - g[2] > 12.0, "notch {g:?}");
        // off by default: the first column of the saw is untouched
        let mut e = operator();
        e.set(ext_param(0, E_WAVE), 10);
        e.note_on(45, 127);
        let mut b = vec![0i16; 44_100];
        e.render(&mut b);
        let x = &b[8_000..8_000 + 22_050];
        assert!((20.0 * (tone(x, 220.0) / tone(x, 110.0)).log10() + 6.0).abs() < 0.8);
    }

    /// OPERATOR shows its sustain in decibels: with the dB law 99 is 0 dB and each step is 1 dB down.
    #[test]
    fn sustain_in_decibels() {
        let level = |v: i32| {
            let mut e = operator();
            e.set(P_SUSTAIN_LAW, 1);
            e.set(op_param(0, O_SUSTAIN), v);
            e.set(op_param(0, O_DECAY), 5);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            peak(&b[30_000..]) as f64 / 32768.0
        };
        let top = level(99);
        for down in [6, 20, 40] {
            let d = 20.0 * (level(99 - down) / top).log10();
            assert!((d + down as f64).abs() < 0.5, "{down} steps gave {d:.2} dB");
        }
    }

    #[test]
    fn fitted_to_operator_a_single_carrier() {
        let mut e = operator();
        e.note_on(57, 127);
        let mut b = vec![0i16; 44_100];
        e.render(&mut b);
        let pk = peak(&b[10_000..]) as f64 / 32768.0;
        assert!((pk / 0.0484 - 1.0).abs() < 0.05, "A alone peaks at {pk}, OPERATOR at 0.0484");
        let mut q = operator();
        q.note_on(57, 1);
        let mut c = vec![0i16; 44_100];
        q.render(&mut c);
        let half = peak(&c[10_000..]) as f64 / peak(&b[10_000..]) as f64;
        assert!((half - 0.5).abs() < 0.02, "velocity 1 is {half} of velocity 127, OPERATOR 0.504");
    }

    #[test]
    fn fitted_to_operator_the_modulation_index() {
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0);
        let spectrum = |e: &mut Fm4| {
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 32_768];
            let h1 = tone(seg, f0);
            (h1, (2..=8).map(|h| 20.0 * (tone(seg, f0 * h as f64) / h1).log10()).collect::<Vec<f64>>())
        };
        // B (OP2) into A at full level and full sustain: OPERATOR's fundamental is 0.0245 of A's 0.0484, harmonics 2 to 8 at these dB under it
        let mut e = operator();
        e.set(op_param(1, O_LEVEL), 99);
        let (h1, db) = spectrum(&mut e);
        assert!((h1 / 0.0245 - 1.0).abs() < 0.08, "the fundamental is {h1}, OPERATOR 0.0245");
        for (h, want) in [(2, -8.9), (3, 1.4), (4, -2.3), (6, -0.2), (7, -8.7), (8, -9.8)] {
            assert!((db[h - 2] - want).abs() < 1.2, "harmonic {h} at {:.1} dB, OPERATOR {want}", db[h - 2]);
        }
        // a small index, from the level scan: B at -54 dB with its sustain at -24 dB (levels 27 and 6) had harmonic 2 at -67.7 dB
        let mut q = operator();
        q.set(op_param(1, O_LEVEL), 27);
        q.set(op_param(1, O_SUSTAIN), 6);
        let (_, small) = spectrum(&mut q);
        assert!((small[0] + 67.7).abs() < 2.5, "harmonic 2 at {:.1} dB, OPERATOR -67.7", small[0]);
    }

    /// OPERATOR's self-feedback (2026-11-18, operators B, C and D; A showed none): alone, at 13 %, 61 % and 100 %, its fundamental was 0.977, 0.64 and 0.452 of the
    /// feedback-free level, with harmonics 2 and 3 at -14.2/-25.0, -6.9/-11.0 and -6.5/-10.4 dB: the mean of the last two outputs times 3.1 radians at 100 %.
    #[test]
    fn fitted_to_operator_the_self_feedback() {
        // OPERATOR sounds 146.8 cents flat here (a 48 kHz engine at 44.1 kHz, docs/PORTING.md): bend this engine the same, so a feedback loop of one or two samples spans as many
        // samples of the period as it does there
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0) * 44_100.0 / 48_000.0;
        let run = |pct: i32| {
            let mut e = operator();
            e.set(P_ALGORITHM, 10);
            e.set(op_param(0, O_LEVEL), 0);
            e.set(op_param(1, O_LEVEL), 99);
            e.set(op_param(1, O_FEEDBACK), pct);
            e.set(P_FEEDBACK, 310);
            e.pitch_bend(2180);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 32_768];
            let h1 = tone(seg, f0);
            (h1, 20.0 * (tone(seg, f0 * 2.0) / h1).log10(), 20.0 * (tone(seg, f0 * 3.0) / h1).log10())
        };
        let clean = run(0).0;
        for (pct, rel, h2, h3) in [(13, 0.977, -14.2, -25.0), (61, 0.64, -6.9, -11.0), (100, 0.452, -6.5, -10.4)] {
            let (h1, a, b) = run(pct);
            assert!((h1 / clean / rel - 1.0).abs() < 0.06, "{pct} %: the fundamental is {:.3} of clean, OPERATOR {rel}", h1 / clean);
            // at 100 % harmonic 2 is 2 dB higher here than there (-4.5 against -6.5; harmonic 3 and the fundamental agree): the top of OPERATOR's knob is not quite this law
            let slack = if pct == 100 { 2.3 } else { 1.0 };
            assert!((a - h2).abs() < slack && (b - h3).abs() < 1.5, "{pct} %: harmonics 2 and 3 at {a:.1} and {b:.1}, OPERATOR {h2} and {h3}");
        }
    }

    #[test]
    fn fitted_to_operator_the_release_slope() {
        let mut e = operator();
        e.note_on(57, 127);
        let mut b = vec![0i16; 22_050];
        e.render(&mut b);
        e.note_off(57);
        let rms = |x: &[i16]| 20.0 * ((x.iter().map(|&v| (v as f64 / 32768.0).powi(2)).sum::<f64>() / x.len() as f64).sqrt()).log10();
        let mut tail = vec![0i16; 4410 * 4];
        e.render(&mut tail);
        let (a, c) = (rms(&tail[..4410]), rms(&tail[4410..8820]));
        assert!((a - c - 16.0).abs() < 2.0, "{a:.1} then {c:.1}: OPERATOR falls 16 dB in 100 ms");
    }

    /// OPERATOR's algorithms 2 to 11 (not 1: see the docs), each with all four operators at 0 dB and full sustain, note 57, as measured in the emulator: the fundamental's
    /// amplitude and harmonics 2 to 4 in dB under it. Algorithms 3 and 4 (and 7 and 9) are the same shape with the letters swapped, so they sound alike when
    /// the operators are equal, and the test cannot tell them apart; their diagrams do.
    #[test]
    fn fitted_to_operator_the_algorithms() {
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0) * 44_100.0 / 48_000.0;
        let table: [(usize, f64, [f64; 3]); 9] = [
            (2, 0.0185, [-24.2, -4.0, -16.3]),
            (3, 0.0172, [-9.0, 1.6, -1.5]),
            (5, 0.0214, [-15.6, -3.0, -4.4]),
            (6, 0.0130, [-19.0, 1.2, -11.5]),
            (7, 0.0490, [-8.9, 1.4, -2.3]),
            (8, 0.0733, [-26.3, -13.4, -13.9]),
            (9, 0.0735, [-8.9, 1.4, -2.3]),
            (10, 0.1212, [-22.8, -12.5, -16.1]),
            (11, 0.1933, [-120.0, -120.0, -120.0]),
        ];
        for (alg, h1_want, db_want) in table {
            let mut e = operator();
            e.set(P_ALGORITHM, alg as i32 - 1);
            for op in 0..OPS {
                e.set(op_param(op, O_LEVEL), 99);
            }
            e.pitch_bend(2180);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 32_768];
            let h1 = tone(seg, f0);
            assert!((h1 / h1_want - 1.0).abs() < 0.08, "algorithm {alg}: fundamental {h1:.4}, OPERATOR {h1_want}");
            for (k, want) in db_want.iter().enumerate() {
                let got = 20.0 * (tone(seg, f0 * (k + 2) as f64) / h1).log10();
                if *want > -60.0 {
                    let tol = if *want < -20.0 { 4.5 } else { 2.5 }; // a deep harmonic moves more with a small error in a modulator
                    assert!((got - want).abs() < tol, "algorithm {alg}: harmonic {} at {got:.1} dB, OPERATOR {want}", k + 2);
                } else {
                    assert!(got < -60.0, "algorithm {alg}: harmonic {} at {got:.1} dB, OPERATOR has none", k + 2);
                }
            }
        }
    }

    /// Mighty Arp 1.5 (2026-11-18, measured in the emulator through its arpeggiator with chance 97 %, one octave, gate 98 %, the effects at 0 and the other operators
    /// silent): its algorithms 1 to 4, 5, 6, 7 and 8 are OPERATOR's 1 to 4, 7, 9, 10 and 11; the level is linear in amplitude (100 % gave 0.0708 of full scale, 50 % half); and
    /// a modulator's index is about 5.4 radians at 100 % (OPERATOR's was 6.28). Algorithm 5 (OP2 into OP1, OP4 into OP3), OP1 at 100 %, OP2 at 5, 10, 15, 25 %: harmonics 2 and 3 under the fundamental.
    #[test]
    fn fitted_to_mighty_arp_the_linear_level_and_the_index() {
        let f0 = 440.0 * 2f64.powf((57.0 - 69.0) / 12.0);
        for (l2, h2, h3) in [(5, -15.9, -40.6), (10, -9.4, -28.4), (15, -5.1, -20.5), (25, 2.1, -8.7)] {
            let mut e = operator();
            e.set(P_LEVEL_LAW, 1);
            e.set(P_LEVEL, 24);
            e.set(P_INDEX, 540); // about 1.75 pi: a little under OPERATOR's whole cycle, and a little compressive towards 100 % (best fit 520 to 620)
            e.set(P_ALGORITHM, 6); // OPERATOR's 7 is Mighty Arp's 5
            for op in 0..OPS {
                e.set(op_param(op, O_LEVEL), 0);
                e.set(op_param(op, O_VELOCITY), 0);
            }
            e.set(op_param(0, O_LEVEL), 100);
            e.set(op_param(1, O_LEVEL), l2);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let seg = &b[10_000..10_000 + 8192];
            let h1 = tone(seg, f0);
            let db = |h: f64| 20.0 * (tone(seg, f0 * h) / h1).log10();
            assert!((db(2.0) - h2).abs() < 1.8 && (db(3.0) - h3).abs() < 3.5, "OP2 at {l2} %: harmonics 2 and 3 at {:.1} and {:.1}, Mighty Arp {h2} and {h3}", db(2.0), db(3.0));
        }
        let carrier = |level: i32| {
            let mut c = operator();
            c.set(P_LEVEL_LAW, 1);
            c.set(P_LEVEL, 24);
            c.set(P_ALGORITHM, 10);
            for op in 0..OPS {
                c.set(op_param(op, O_LEVEL), 0);
                c.set(op_param(op, O_VELOCITY), 0);
            }
            c.set(op_param(0, O_LEVEL), level);
            c.note_on(57, 127);
            let mut b = vec![0i16; 22_050];
            c.render(&mut b);
            peak(&b[10_000..]) as f64 / 32768.0
        };
        let full = carrier(100);
        assert!((full / 0.0708 - 1.0).abs() < 0.06, "one carrier at 100 % peaks at {full}, Mighty Arp 0.0708");
        assert!((carrier(50) / full - 0.5).abs() < 0.03, "50 % is half");
    }

    /// OPERATOR 0.7's ratio (measured by the zero crossings of operator A alone, note 57): the coarse knob steps 0.5, 1, 2, 3, 4, 5, 6, 7 ... to 48 (a click each), and the
    /// fine value adds thousandths (coarse 1 and fine 26 played 1.026 times the pitch, fine 146 played 1.146; the screen read "1.026" and "1.146").
    #[test]
    fn fitted_to_operator_the_ratio_table() {
        let freq = |coarse: i32, fine: i32| {
            let mut e = sine();
            e.set(P_RATIO_LAW, 1);
            e.set(op_param(0, O_RATIO), coarse);
            e.set(op_param(0, O_FINE), fine);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.note_on(57, 127);
            let mut b = vec![0i16; 44_100];
            e.render(&mut b);
            let z = &b[8000..];
            z.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count() as f64 * 44_100.0 / z.len() as f64 / 220.0
        };
        for (coarse, ratio) in [(0, 0.5), (1, 1.0), (2, 2.0), (3, 3.0), (7, 7.0), (20, 20.0)] {
            assert!((freq(coarse, 0) / ratio - 1.0).abs() < 0.02, "coarse {coarse}: {:.3}", freq(coarse, 0));
        }
        assert!((freq(1, 26) - 1.026).abs() < 0.006 && (freq(1, 146) - 1.146).abs() < 0.006 && (freq(18, 248) - 18.248).abs() < 0.12, "fine adds thousandths");
        // the generic law is still a percentage of the ratio
        let mut g = sine();
        g.set(op_param(0, O_RATIO), 2);
        g.set(op_param(0, O_FINE), 50);
        g.note_on(57, 127);
        let mut b = vec![0i16; 44_100];
        g.render(&mut b);
        let z = &b[8000..];
        let f = z.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count() as f64 * 44_100.0 / z.len() as f64 / 220.0;
        assert!((f - 3.0).abs() < 0.04, "coarse 2 fine 50 % is 3x: {f}");
    }

    #[test]
    fn glide_slides_from_the_last_note_to_the_next() {
        let play = |ms: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_ALGORITHM, 7);
            e.glide(ms);
            let mut buf = vec![0i16; 4410];
            e.note_on(57, 100);
            e.render(&mut buf);
            e.note_off(57);
            e.render(&mut buf);
            e.note_on(69, 100);
            let mut out = vec![0i16; 22_050];
            e.render(&mut out);
            out
        };
        let none = play(0);
        let g = play(300);
        let f = |x: &[i16], lo: usize, hi: usize| {
            let seg = &x[lo..hi];
            let z = seg.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
            z as f64 / 2.0 * 44_100.0 / seg.len() as f64
        };
        let (n_hz, early, late) = (f(&none, 2048, 8192), f(&g, 1024, 3072), f(&g, 14_000, 22_050));
        assert!((n_hz / early) > 1.2, "glide starts below the target: {n_hz} vs {early}");
        assert!((late / n_hz - 1.0).abs() < 0.03, "and arrives: {late} vs {n_hz}");
        // a huge value is clamped, not trusted
        let mut e = Fm4::new(44_100.0);
        e.glide(i32::MAX);
        e.note_on(81, 100);
        e.note_off(81);
        e.note_on(45, 100);
        let mut out = vec![0i16; 2048];
        e.render(&mut out);
        assert!(out.iter().any(|&v| v != 0));
    }

    /// OPERATOR 0.7's LFO (2026-11-20, `fm4fit` and its LFO page): default Sine, 6.32 Hz, Amount 10 %, on pitch; 10 % swung the pitch +-120 cents and 69 % +-827
    /// (12 cents a percent, an octave at 100 %); a square starts high and switches at half a cycle, a saw up starts low, a saw down starts high.
    #[test]
    fn fitted_to_operator_the_lfo() {
        // cents of each cycle's frequency, from the zero crossings of a clean sine on OP1
        let cents = |amount: i32, ty: i32, hz100: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_TYPE, ty);
            e.set(LFO_BASE + L_RATE, hz100);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let mut ups = vec![];
            for i in 1..x.len() {
                if x[i - 1] < 0 && x[i] >= 0 {
                    ups.push(i as f64 - 1.0 + f64::from(-x[i - 1]) / f64::from(x[i] - x[i - 1]));
                }
            }
            ups.windows(2).map(|w| (1200.0 * (44_100.0 / (w[1] - w[0]) / 220.0).log2(), (w[0] + w[1]) / 2.0 / 44_100.0)).collect::<Vec<_>>()
        };
        let swing = |c: &[(f64, f64)]| (c.iter().map(|v| v.0).fold(f64::MIN, f64::max), c.iter().map(|v| v.0).fold(f64::MAX, f64::min));
        let c = cents(10, 0, 632);
        let (hi, lo) = swing(&c);
        assert!((hi - 120.0).abs() < 10.0 && (lo + 120.0).abs() < 10.0, "10 percent: {hi} {lo}");
        let c = cents(69, 0, 632);
        let (hi, lo) = swing(&c);
        assert!((hi - 828.0).abs() < 60.0 && (lo + 828.0).abs() < 60.0, "69 percent: {hi} {lo}");
        // the rate: ten cycles take a second and a half at 6.32 Hz
        let c = cents(10, 0, 632);
        let mean = c.iter().map(|v| v.0).sum::<f64>() / c.len() as f64;
        let ups: Vec<f64> = (1..c.len()).filter(|&k| c[k - 1].0 - mean < 0.0 && c[k].0 - mean >= 0.0).map(|k| c[k].1).collect();
        let hz = (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0]);
        assert!((hz - 6.32).abs() < 0.3, "rate {hz}");
        // the shapes' first half cycle: square high, saw up low (rising), saw down high
        let first = |ty: i32| cents(10, ty, 632).iter().filter(|v| v.1 < 0.05).map(|v| v.0).sum::<f64>();
        assert!(first(1) > 100.0 && first(3) < -50.0 && first(4) > 50.0, "{} {} {}", first(1), first(3), first(4));
        // off by default: the pitch stays put
        let mut e = Fm4::new(44_100.0);
        e.note_on(57, 127);
        let mut x = vec![0i16; 8_000];
        e.render(&mut x);
        assert!(x.iter().any(|&v| v != 0));
        // random types stay bounded and the free-running one renders
        for (ty, retrig) in [(5, 1), (6, 1), (6, 0), (0, 0)] {
            let mut e = Fm4::new(44_100.0);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_TYPE, ty);
            e.set(LFO_BASE + L_RETRIG, retrig);
            e.set(LFO_BASE + L_AMOUNT, 100);
            e.note_on(60, 100);
            e.note_on(64, 100);
            let mut x = vec![0i16; 20_000];
            e.render(&mut x);
            assert!(x.iter().any(|&v| v != 0));
        }
    }

    /// OPERATOR 0.7's LFO on the filter (2026-11-20; operator A as Saw D, a 24 dB low-pass at 846 Hz, Freq<Key +100, Res 0.27, only the filter a target): the fitted cutoff
    /// swung 537 to 800 Hz about 655 (the displayed corner 712 Hz at the note, times the engine factor) at Amount 10 %, so **plus and minus 0.285 octave, 2.85 octaves at 100 %**,
    /// a sine geometrically about its own value; Amount A at 34 % gave 0.35 of that swing.
    #[test]
    fn fitted_to_operator_the_lfo_on_the_filter() {
        let swing = |amount: i32, amount_a: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_WAVE), 10); // Saw D
            e.set(FILT_BASE + F_ON, 1);
            e.set(FILT_BASE + F_FREQ, 846);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_AMOUNT_A, amount_a);
            e.set(LFO_BASE + L_DEST, L_FILTER_BIT); // the filter only
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100 + 3000];
            e.render(&mut x);
            let x: Vec<f64> = x[3000..].iter().map(|&v| f64::from(v) / 32768.0).collect();
            let f0 = 220.0;
            let q = 0.5 / (1.0 - 0.97 * 0.27);
            let h = |f: f64, fc: f64| {
                let xx = f / fc;
                1.0 / ((1.0 - xx * xx).powi(2) + (xx / q).powi(2))
            };
            let mut fcs = vec![];
            for a in (0..x.len() - 1764).step_by(882) {
                let db: Vec<f64> = (1..=10)
                    .map(|k| {
                        let w = std::f64::consts::TAU * k as f64 * f0 / 44_100.0;
                        let (mut c, mut s) = (0.0, 0.0);
                        for i in 0..1764 {
                            let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 1764.0).cos();
                            c += x[a + i] * win * (w * i as f64).cos();
                            s += x[a + i] * win * (w * i as f64).sin();
                        }
                        20.0 * (c.hypot(s) / 441.0 * k as f64).max(1e-9).log10()
                    })
                    .collect();
                let mut best = (f64::MAX, 0.0);
                for g in 0..200 {
                    let fc = 200.0 * 2f64.powf(g as f64 / 40.0);
                    let m: Vec<f64> = (1..=10).map(|k| 20.0 * h(k as f64 * f0, fc).log10()).collect();
                    let off = db.iter().zip(&m).map(|(d, m)| d - m).sum::<f64>() / 10.0;
                    let e: f64 = db.iter().zip(&m).map(|(d, m)| (d - m - off).powi(2)).sum();
                    if e < best.0 {
                        best = (e, fc);
                    }
                }
                fcs.push(best.1);
            }
            let (lo, hi) = (fcs.iter().cloned().fold(f64::MAX, f64::min), fcs.iter().cloned().fold(0.0, f64::max));
            ((hi / lo).log2(), (hi * lo).sqrt())
        };
        let (oct, centre) = swing(10, 100);
        assert!((oct - 0.57).abs() < 0.15, "10 percent swung {oct} octaves");
        assert!((centre / 712.0 - 1.0).abs() < 0.12, "about {centre} Hz");
        let (oct34, _) = swing(10, 34);
        assert!((oct34 / oct - 0.35).abs() < 0.15, "Amount A at 34 percent: {oct34} against {oct}");
        let (oct0, _) = swing(0, 100);
        assert!(oct0 < 0.1, "no amount, no swing: {oct0}");
    }

    /// OPERATOR 0.7's LFO Dest B on Osc Vol A (2026-11-20, one sine carrier, LFO amount 10, 69 and 100 %, Amount B +100 %): the peak went 0.0436 to 0.0533, 0.0156 to
    /// 0.0818 and 0 to 0.0968 of a clean 0.0484, a gain of 1 plus amount times the sine; Volume gives the same for a lone carrier. Dest B on Osc Vol A leaves the other operators alone.
    #[test]
    fn fitted_to_operator_lfo_dest_b_on_osc_volume() {
        let range = |target: i32, amount: i32, amount_b: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_DEST, 0); // no pitch targets
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_DEST_B, target);
            e.set(LFO_BASE + L_AMOUNT_B, amount_b);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let peaks: Vec<f64> = x[4410..].chunks(441).map(|c| f64::from(peak(c)) / 32768.0).collect();
            (peaks.iter().cloned().fold(f64::MAX, f64::min), peaks.iter().cloned().fold(0.0, f64::max))
        };
        let (clean_lo, clean_hi) = range(0, 100, 100);
        assert!((clean_hi / clean_lo - 1.0).abs() < 0.03, "target Off leaves the level alone");
        let one = clean_hi;
        for (amount, lo, hi) in [(10, 0.9, 1.1), (69, 0.31, 1.69), (100, 0.0, 2.0)] {
            let (a, b) = range(1, amount, 100);
            assert!((a / one - lo).abs() < 0.06 && (b / one - hi).abs() < 0.06, "amount {amount}: {} to {}", a / one, b / one);
        }
        // Volume on a lone carrier is the same; another operator's Osc Vol does nothing to it; Amount B scales
        let (a, b) = range(20, 69, 100);
        assert!((a / one - 0.31).abs() < 0.06 && (b / one - 1.69).abs() < 0.06);
        let (a, b) = range(3, 100, 100);
        assert!((a / b - 1.0).abs() < 0.03, "Osc Vol C leaves operator A alone");
        let (a, b) = range(1, 100, 50);
        assert!((a / one - 0.5).abs() < 0.06 && (b / one - 1.5).abs() < 0.06, "Amount B at 50 percent");
    }

    /// OPERATOR 0.7's LFO Dest B on FM Drive (the fitted index of B into A swung 0.52 to 2.64 about 1.58 at 69 %, 0.30 to 2.84 at 100 %: the same 1 + amount x value as
    /// Osc Vol) and on Feedback (a lone B in algorithm 11 with no base feedback sounded feedback only on the positive half of the LFO, clamped at zero below it).
    #[test]
    fn fitted_to_operator_lfo_dest_b_on_fm_drive_and_feedback() {
        let build = |target: i32, amount: i32, fb: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(P_ALGORITHM, 0);
            e.set(P_INDEX, 628);
            for op in 0..OPS {
                for (field, v) in [(O_ATTACK, 1), (O_DECAY, 998), (O_SUSTAIN, 99), (O_RELEASE, 399), (O_LEVEL, 99), (O_FEEDBACK, 0)] {
                    e.set(op_param(op, field), v);
                }
            }
            for op in 2..OPS {
                e.set(op_param(op, O_LEVEL), 0);
            }
            e.set(op_param(1, O_FEEDBACK), fb);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_DEST, 0);
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_DEST_B, target);
            e.note_on(57, 127);
            e
        };
        // FM Drive: B into A at -12 dB (index 1.58 rad), the fitted index per 20 ms window against a phase-modulation table
        let table: Vec<(f64, Vec<f64>)> = (0..=160)
            .map(|b| {
                let beta = f64::from(b) * 0.05;
                let n = 256;
                let y: Vec<f64> = (0..n).map(|t| (std::f64::consts::TAU * t as f64 / n as f64 + beta * (std::f64::consts::TAU * t as f64 / n as f64).sin()).sin()).collect();
                let h = (1..=7)
                    .map(|k| {
                        let (mut c, mut s) = (0.0, 0.0);
                        for (t, v) in y.iter().enumerate() {
                            let a = std::f64::consts::TAU * (k * t) as f64 / n as f64;
                            c += v * a.cos();
                            s += v * a.sin();
                        }
                        2.0 * c.hypot(s) / n as f64
                    })
                    .collect();
                (beta, h)
            })
            .collect();
        let betas = |amount: i32, target: i32| {
            let mut e = build(target, amount, 0);
            e.set(op_param(1, O_LEVEL), 83);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let x: Vec<f64> = x[4410..].iter().map(|&v| f64::from(v) / 32768.0 / 0.0484).collect();
            let mut out = vec![];
            for a in (0..x.len() - 882).step_by(441) {
                let amp: Vec<f64> = (1..=7)
                    .map(|k| {
                        let w = std::f64::consts::TAU * f64::from(k) * 220.0 / 44_100.0;
                        let (mut c, mut s) = (0.0, 0.0);
                        for i in 0..882 {
                            let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 882.0).cos();
                            c += x[a + i] * win * (w * i as f64).cos();
                            s += x[a + i] * win * (w * i as f64).sin();
                        }
                        c.hypot(s) / 220.5
                    })
                    .collect();
                let best = table.iter().map(|(b, h)| (amp.iter().zip(h).map(|(a, h)| (a - h).powi(2)).sum::<f64>(), *b)).fold((f64::MAX, 0.0), |m, v| if v.0 < m.0 { v } else { m });
                out.push(best.1);
            }
            (out.iter().cloned().fold(f64::MAX, f64::min), out.iter().cloned().fold(0.0, f64::max))
        };
        let (lo, hi) = betas(0, 0);
        assert!((lo - 1.58).abs() < 0.25 && (hi - 1.58).abs() < 0.25, "no target: steady at 1.58, got {lo} {hi}");
        let (lo, hi) = betas(69, 9);
        assert!(lo < 0.9 && hi > 2.2, "FM Drive at 69 percent swings the index (OPERATOR: 0.52 to 2.64): {lo} {hi}");
        let (lo10, hi10) = betas(10, 9);
        assert!(lo10 > 1.3 && hi10 < 1.9 && hi10 - lo10 > 0.1, "and at 10 percent only a little (1.44 to 1.70): {lo10} {hi10}");
        // Feedback: B alone (algorithm 11), base 0 and the LFO at 100 %: harmonics only while the LFO is positive
        let mut e = build(7, 100, 0);
        e.set(P_ALGORITHM, 10);
        e.set(op_param(0, O_LEVEL), 0);
        let mut x = vec![0i16; 44_100];
        e.render(&mut x);
        let x: Vec<f64> = x[4410..].iter().map(|&v| f64::from(v) / 32768.0).collect();
        let dist = |a: usize| {
            // how far a 10 ms window is from a pure sine: the harmonics' share
            let w1 = std::f64::consts::TAU * 220.0 / 44_100.0;
            let (mut c, mut s, mut tot) = (0.0, 0.0, 0.0);
            for i in 0..441 {
                c += x[a + i] * (w1 * i as f64).cos();
                s += x[a + i] * (w1 * i as f64).sin();
                tot += x[a + i] * x[a + i];
            }
            let fund = (c * c + s * s) * 2.0 / 441.0;
            1.0 - fund / tot.max(1e-12)
        };
        let shares: Vec<f64> = (0..x.len() - 441).step_by(441).map(dist).collect();
        let shares: Vec<f64> = shares.iter().map(|&v| if v < 0.05 { 0.0 } else { v }).collect();
        let hi = shares.iter().cloned().fold(0.0, f64::max);
        let quiet = shares.iter().filter(|&&v| v == 0.0).count() as f64 / shares.len() as f64;
        assert!(hi > 0.1, "feedback shows at the LFO's peaks: {hi}");
        assert!(quiet > 0.3 && quiet < 0.8, "and not on the negative half: {quiet} of the windows are clean");
        // with Dest B off the same patch is a clean sine throughout
        let mut e = build(0, 100, 0);
        e.set(P_ALGORITHM, 10);
        e.set(op_param(0, O_LEVEL), 0);
        let mut y = vec![0i16; 8820];
        e.render(&mut y);
        assert!(y.iter().any(|&v| v != 0));
    }

    /// OPERATOR 0.7's LFO Rate<Key and Amt<Vel (2026-11-20, pitch targets): Rate<Key +100 % gave LFO rates of 2.44, 4.88 and 9.78 Hz on notes 45, 57 and 69 (a doubling an
    /// octave about note 60); Amt<Vel +100 % gave swings of 115, 60 and 30 cents at velocities 127, 64 and 32 (the depth times velocity / 127).
    #[test]
    fn fitted_to_operator_the_lfo_rate_key_and_amount_velocity() {
        let run = |note: u8, vel: u8, rate_key: i32, amt_vel: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_RATE, 580);
            e.set(LFO_BASE + L_RATE_KEY, rate_key);
            e.set(LFO_BASE + L_AMT_VEL, amt_vel);
            e.note_on(note, vel);
            let mut x = vec![0i16; 88_200];
            e.render(&mut x);
            let f_nom = 440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0);
            let mut ups = vec![];
            for i in 1..x.len() {
                if x[i - 1] < 0 && x[i] >= 0 {
                    ups.push(i as f64 - 1.0 + f64::from(-x[i - 1]) / f64::from(x[i] - x[i - 1]));
                }
            }
            let c: Vec<(f64, f64)> = ups.windows(2).map(|w| (1200.0 * (44_100.0 / (w[1] - w[0]) / f_nom).log2(), (w[0] + w[1]) / 2.0 / 44_100.0)).filter(|v| v.1 > 0.1).collect();
            let mean = c.iter().map(|v| v.0).sum::<f64>() / c.len() as f64;
            let swing = c.iter().map(|v| v.0 - mean).fold(0.0, f64::max);
            let up: Vec<f64> = (1..c.len()).filter(|&k| c[k - 1].0 < mean && c[k].0 >= mean).map(|k| c[k].1).collect();
            ((up.len() - 1) as f64 / (up[up.len() - 1] - up[0]), swing)
        };
        for (note, hz) in [(45, 5.8 * 2f64.powf(-1.25)), (57, 5.8 * 2f64.powf(-0.25)), (69, 5.8 * 2f64.powf(0.75))] {
            let (rate, _) = run(note, 127, 100, 0);
            assert!((rate / hz - 1.0).abs() < 0.08, "note {note}: {rate} Hz, wanted about {hz}");
        }
        let (flat, _) = run(45, 127, 0, 0);
        assert!((flat - 5.8).abs() < 0.4, "no Rate<Key: {flat}");
        let (_, s127) = run(57, 127, 0, 100);
        let (_, s64) = run(57, 64, 0, 100);
        let (_, s32) = run(57, 32, 0, 100);
        assert!((s64 / s127 - 0.5).abs() < 0.07 && (s32 / s127 - 0.25).abs() < 0.07, "{s127} {s64} {s32}");
        let (_, flat64) = run(57, 64, 0, 0);
        let (_, flat127) = run(57, 127, 0, 0);
        assert!((flat64 / flat127 - 1.0).abs() < 0.07, "Amt<Vel at 0 leaves the depth alone");
    }

    /// OPERATOR 0.7's LFO Range (2026-11-20): Sync at 1/8T and 120 beats a minute ran at 5.52 Hz, which is 6.0 Hz times the engine factor, and the Synced menu is a ladder of
    /// note lengths; High range is only a higher rate (sidebands 6.0 kHz either side of the carrier for the 6.54 kHz setting).
    #[test]
    fn fitted_to_operator_the_lfo_range() {
        let rate_of = |sync: i32, div: i32, tempo: i32, hz100: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_SYNC, sync);
            e.set(LFO_BASE + L_SYNC_DIV, div);
            e.set(LFO_BASE + L_TEMPO, tempo);
            e.set(LFO_BASE + L_RATE, hz100);
            e.note_on(57, 127);
            let mut x = vec![0i16; 441_000];
            e.render(&mut x);
            let mut ups = vec![];
            for i in 1..x.len() {
                if x[i - 1] < 0 && x[i] >= 0 {
                    ups.push(i as f64 - 1.0 + f64::from(-x[i - 1]) / f64::from(x[i] - x[i - 1]));
                }
            }
            let c: Vec<(f64, f64)> = ups.windows(2).map(|w| (44_100.0 / (w[1] - w[0]), (w[0] + w[1]) / 2.0 / 44_100.0)).filter(|v| v.1 > 0.1).collect();
            let mean = c.iter().map(|v| v.0).sum::<f64>() / c.len() as f64;
            let up: Vec<f64> = (1..c.len()).filter(|&k| c[k - 1].0 < mean && c[k].0 >= mean).map(|k| c[k].1).collect();
            (up.len() - 1) as f64 / (up[up.len() - 1] - up[0])
        };
        let r = rate_of(1, 3, 120, 632);
        assert!((r - 6.0).abs() < 0.3, "1/8T at 120: {r}");
        let r = rate_of(1, 7, 120, 632);
        assert!((r - 2.0).abs() < 0.15, "1/4 at 120 is 2 Hz: {r}");
        let r = rate_of(1, 3, 60, 632);
        assert!((r - 3.0).abs() < 0.2, "1/8T at 60: {r}");
        let r = rate_of(1, 12, 120, 632);
        assert!((r - 0.5).abs() < 0.1, "a bar at 120 is 0.5 Hz: {r}");
        assert!((rate_of(0, 3, 120, 632) - 6.32).abs() < 0.3, "free: the knob's rate");
        // High range: an audio-rate LFO puts sidebands at the carrier plus and minus its rate
        let mut e = Fm4::new(44_100.0);
        e.set(P_LEVEL, 16);
        e.set(op_param(0, O_ATTACK), 1);
        e.set(op_param(0, O_SUSTAIN), 99);
        e.set(LFO_BASE + L_ON, 1);
        e.set(LFO_BASE + L_AMOUNT, 100);
        e.set(LFO_BASE + L_RATE, 600_000); // 6 kHz
        e.note_on(93, 127); // 1760 Hz, so a 6 kHz LFO's sidebands are strong enough to see
        let mut x = vec![0i16; 22_050];
        e.render(&mut x);
        let amp = |f: f64| {
            let w = std::f64::consts::TAU * f / 44_100.0;
            let (mut c, mut s) = (0.0, 0.0);
            for (i, &v) in x[2000..18_000].iter().enumerate() {
                c += f64::from(v) * (w * i as f64).cos();
                s += f64::from(v) * (w * i as f64).sin();
            }
            c.hypot(s)
        };
        assert!(amp(6000.0 - 1760.0) > 0.3 * amp(1760.0), "a sideband at the rate minus the carrier");
    }

    /// OPERATOR 0.7's S&H and Noise LFO types (2026-11-20, pitch at 10 %, +-120 cents): S&H held a value per cycle spread across -112 to 113 cents; Noise had a spread of 15.1
    /// cents with an autocorrelation of 0.48 at 20 ms at 5.8 Hz, 33.3 cents (0.46 at 5 ms on a 404 Hz carrier) at 27.5 Hz and 5.1 cents (0.60 at 100 ms) at 0.77 Hz.
    #[test]
    fn fitted_to_operator_sample_and_hold_and_noise() {
        // the pitch in cents on a 5 ms grid
        let trace = |ty: i32, hz100: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_TYPE, ty);
            e.set(LFO_BASE + L_RATE, hz100);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100 * 10];
            e.render(&mut x);
            let mut ups = vec![];
            for i in 1..x.len() {
                if x[i - 1] < 0 && x[i] >= 0 {
                    ups.push(i as f64 - 1.0 + f64::from(-x[i - 1]) / f64::from(x[i] - x[i - 1]));
                }
            }
            let c: Vec<(f64, f64)> = ups.windows(2).map(|w| (1200.0 * (44_100.0 / (w[1] - w[0]) / 220.0).log2(), (w[0] + w[1]) / 2.0 / 44_100.0)).filter(|v| v.1 > 0.2).collect();
            let mut g = vec![];
            let mut k = 0;
            let mut t = 0.2;
            while t < 9.8 {
                while k + 1 < c.len() && c[k].1 < t {
                    k += 1;
                }
                g.push(c[k].0);
                t += 0.005;
            }
            g
        };
        let stat = |g: &[f64], lag: usize| {
            let m = g.iter().sum::<f64>() / g.len() as f64;
            let var = g.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / g.len() as f64;
            let ac = (0..g.len() - lag).map(|i| (g[i] - m) * (g[i + lag] - m)).sum::<f64>() / (g.len() - lag) as f64 / var;
            (var.sqrt(), ac)
        };
        let g = trace(5, 632);
        let (std, _) = stat(&g, 1);
        assert!((std - 69.0).abs() < 18.0, "S&H spread {std}");
        assert!(g.iter().all(|v| v.abs() < 130.0), "within the 120 cents");
        // Noise at the default rate
        let (std, ac20) = stat(&trace(6, 632), 4);
        assert!((std - 15.5).abs() < 5.0, "Noise spread {std}");
        assert!(ac20 > 0.3 && ac20 < 0.65, "Noise correlation at 20 ms {ac20}");
        // a faster rate is wider and shorter, a slower one narrower and longer (the spread goes as the square root of the rate)
        let (fast, _) = stat(&trace(6, 3000), 1);
        let (slow, ac100) = stat(&trace(6, 77), 20);
        assert!((fast / std - 2.2).abs() < 0.6, "30 Hz against 6.3 Hz: {}", fast / std);
        assert!((slow / std - 0.33).abs() < 0.12, "0.77 Hz against 6.3 Hz: {}", slow / std);
        assert!(ac100 > 0.4, "Noise at 0.77 Hz is still correlated at 100 ms: {ac100}");
    }

    /// OPERATOR 0.7's LFO Dest B on the filter (2026-11-20; a Saw D and a 24 dB low-pass at 846 Hz): Filter Freq at 10 % swung the fitted cutoff 501 to 843 Hz (0.75 octave, wider
    /// than Dest A's 0.57), and Filter Q and Filter Res (the same behaviour) rang the filter up by some 40 dB at the LFO's peaks, harmonic 3 rising from -41 to +1 dB.
    #[test]
    fn fitted_to_operator_lfo_dest_b_on_the_filter() {
        let spectrum = |target: i32, amount: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_WAVE), 10);
            e.set(FILT_BASE + F_ON, 1);
            e.set(FILT_BASE + F_FREQ, 846);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_DEST, 0);
            e.set(LFO_BASE + L_AMOUNT, amount);
            e.set(LFO_BASE + L_DEST_B, target);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let x: Vec<f64> = x[4410..].iter().map(|&v| f64::from(v) / 32768.0).collect();
            // the third harmonic's level (dB) in each 20 ms window
            (0..x.len() - 882)
                .step_by(441)
                .map(|a| {
                    let w = std::f64::consts::TAU * 3.0 * 220.0 / 44_100.0;
                    let (mut c, mut s) = (0.0, 0.0);
                    for i in 0..882 {
                        let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 882.0).cos();
                        c += x[a + i] * win * (w * i as f64).cos();
                        s += x[a + i] * win * (w * i as f64).sin();
                    }
                    20.0 * (c.hypot(s) / 220.5).max(1e-9).log10()
                })
                .collect::<Vec<f64>>()
        };
        let range = |v: &[f64]| (v.iter().cloned().fold(f64::MAX, f64::min), v.iter().cloned().fold(f64::MIN, f64::max));
        let (lo, hi) = range(&spectrum(0, 100));
        assert!(hi - lo < 1.0, "Off: steady {lo} {hi}");
        // Res and Q at 100 %: harmonic 3 (near the cutoff) rings far above its resting level
        for target in [11, 12] {
            let (lo, hi) = range(&spectrum(target, 100));
            assert!(hi - lo > 25.0, "target {target}: harmonic 3 swung only {} dB", hi - lo);
        }
        // Freq at 10 %: the cutoff moves a little, so the third harmonic moves a few dB
        let (lo, hi) = range(&spectrum(10, 10));
        assert!(hi - lo > 2.0 && hi - lo < 15.0, "Filter Freq at 10 percent: {} dB", hi - lo);
        let (lo2, hi2) = range(&spectrum(10, 100));
        assert!(hi2 - lo2 > 15.0, "and a lot at 100 percent");
    }

    /// OPERATOR 0.7's LFO Dest B on Fixed Freq (2026-11-20; a fixed operator A, Amount 10 %): the tone swung 69.7 to 120.9 Hz about 91.7 and 231.5 to 403 about 305.5, a ratio of 1.32
    /// either way on both, plus and minus 0.40 octave, so 4 octaves at 100 %. A non-fixed operator is not touched.
    #[test]
    fn fitted_to_operator_lfo_dest_b_on_fixed_freq() {
        let freqs = |fixed: i32, target: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_FIXED), fixed);
            e.set(ext_param(0, E_FIXFREQ), 100);
            e.set(LFO_BASE + L_ON, 1);
            e.set(LFO_BASE + L_DEST, 0);
            e.set(LFO_BASE + L_DEST_B, target);
            e.note_on(57, 127);
            let mut x = vec![0i16; 66_150];
            e.render(&mut x);
            let mut ups = vec![];
            for i in 1..x.len() {
                if x[i - 1] < 0 && x[i] >= 0 {
                    ups.push(i as f64 - 1.0 + f64::from(-x[i - 1]) / f64::from(x[i] - x[i - 1]));
                }
            }
            let f: Vec<f64> = ups.windows(2).filter(|w| w[0] > 4410.0).map(|w| 44_100.0 / (w[1] - w[0])).collect();
            (f.iter().cloned().fold(f64::MAX, f64::min), f.iter().cloned().fold(0.0, f64::max))
        };
        let (lo, hi) = freqs(1, 8);
        assert!((hi / 100.0 - 1.32).abs() < 0.06 && (100.0 / lo - 1.32).abs() < 0.06, "{lo} {hi}");
        let (lo, hi) = freqs(1, 0);
        assert!(hi / lo < 1.03, "Off: steady");
        let (lo, hi) = freqs(0, 8); // a tracking operator ignores it (note 57 is 220 Hz)
        assert!(hi / lo < 1.03 && (lo - 220.0).abs() < 4.0, "{lo} {hi}");
    }

    /// OPERATOR 0.7's filter Morph (2026-11-20; Saw D on operator A, a 24 dB filter at 846 Hz, Res 0.27, harmonics' levels in dB against a 1/k saw): at 25 % it is a band-pass of peak
    /// gain Q (harmonic 3 at -16.5 dB), at 50 % a high-pass (harmonic 1 at -45, the plateau at -5), at 75 % LP plus HP, a notch on harmonic 3 (-45 dB), and 0 and 100 % are the
    /// low-pass (harmonic 1 at -4.4). The mix is linear between the keyframes.
    #[test]
    fn fitted_to_operator_the_filter_morph() {
        let profile = |kind: i32, morph: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_WAVE), 10);
            e.set(FILT_BASE + F_ON, 1);
            e.set(FILT_BASE + F_FREQ, 846);
            e.set(FILT_BASE + F_TYPE, kind);
            e.set(F2_BASE + F2_MORPH, morph);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let x: Vec<f64> = x[13_230..].iter().map(|&v| f64::from(v) / 32768.0).collect();
            (1..=8)
                .map(|k| {
                    let w = std::f64::consts::TAU * k as f64 * 220.0 / 44_100.0;
                    let (mut c, mut s) = (0.0, 0.0);
                    for (i, v) in x.iter().enumerate() {
                        let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
                        c += v * win * (w * i as f64).cos();
                        s += v * win * (w * i as f64).sin();
                    }
                    20.0 * (c.hypot(s) / (x.len() as f64 / 4.0) * k as f64 / 0.0484).max(1e-9).log10()
                })
                .collect::<Vec<f64>>()
        };
        let lp = profile(0, 0);
        let near = |a: &[f64], b: &[f64], tol: f64| a.iter().zip(b).all(|(x, y)| (x - y).abs() < tol);
        assert!(near(&profile(4, 0), &lp, 0.2) && near(&profile(4, 100), &lp, 0.2), "Morph at 0 and 100 percent is the low-pass");
        assert!(near(&profile(4, 25), &profile(2, 0), 0.3), "25 percent is the band-pass");
        assert!(near(&profile(4, 50), &profile(1, 0), 0.3), "50 percent is the high-pass");
        assert!(near(&profile(4, 75), &profile(3, 0), 0.3), "75 percent is the notch");
        // the measured shapes: band-pass peaks near harmonic 3 about 16 dB under a 1/k saw, the notch bites harmonic 3
        let bp = profile(2, 0);
        assert!(bp[2] > bp[0] + 4.0 && bp[2] > bp[7] + 4.0 && (bp[2] + 11.8).abs() < 3.0, "band-pass {bp:?}");
        let nt = profile(3, 0);
        assert!(nt[2] < nt[1] - 15.0 && nt[2] < nt[3] - 15.0, "notch {nt:?}");
        // between the keyframes it moves: 10 percent is between the low-pass and the band-pass
        let m10 = profile(4, 10);
        assert!(m10[0] < lp[0] - 1.0 && m10[0] > bp[0] + 1.0, "{m10:?}");
    }

    /// OPERATOR 0.7's filter circuits and Drive (2026-11-20): a sine through the filter wide open with a non-Clean circuit had its fundamental at -5.3 dB, 3rd harmonic at -19.5 and 5th at
    /// -30.8 dB re the fundamental at Drive 0, and (+6.8 dB) -2.1, -15.8 and -25.0, (+15) +0.1, -12.8 and -20.0, (+23) +1.2, -11.1 and -17.0; Clean did nothing with Drive. A Saw D through
    /// the 846 Hz filter ran 4 to 7 dB lower on every harmonic (-3.9, -6.7, -5.6, -6.2, -5.9 ...), the shape of the clean filter. OSR, MS2, SMP and PRD measured the same.
    #[test]
    fn fitted_to_operator_the_filter_circuits_and_drive() {
        let sine = |circuit: i32, drive10: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(FILT_BASE + F_ON, 1);
            e.set(F2_BASE + F2_CIRCUIT, circuit);
            e.set(F2_BASE + F2_DRIVE, drive10);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let x: Vec<f64> = x[13_230..].iter().map(|&v| f64::from(v) / 32768.0 / 0.0484).collect();
            let amp = |k: f64| {
                let w = std::f64::consts::TAU * k * 220.0 / 44_100.0;
                let (mut c, mut s) = (0.0, 0.0);
                for (i, v) in x.iter().enumerate() {
                    let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
                    c += v * win * (w * i as f64).cos();
                    s += v * win * (w * i as f64).sin();
                }
                c.hypot(s) / (x.len() as f64 / 4.0)
            };
            let f = amp(1.0);
            (20.0 * f.log10(), 20.0 * (amp(3.0) / f).log10(), 20.0 * (amp(5.0) / f).log10(), 20.0 * (amp(2.0) / f).max(1e-9).log10())
        };
        for (drive, fund, h3, h5) in [(0, -5.3, -19.5, -30.8), (68, -2.1, -15.8, -25.0), (150, 0.1, -12.8, -20.0), (230, 1.2, -11.1, -17.0)] {
            let (f, a3, a5, a2) = sine(4, drive);
            assert!((f - fund).abs() < 0.5 && (a3 - h3).abs() < 0.7 && (a5 - h5).abs() < 1.0, "drive {drive}: {f} {a3} {a5}");
            assert!(a2 < -60.0, "odd harmonics only: {a2}");
        }
        let clean = sine(0, 230);
        assert!(clean.0.abs() < 0.3 && clean.1 < -60.0, "Clean ignores Drive: {clean:?}");
        let (f, ..) = sine(1, 0);
        assert!((f - sine(3, 0).0).abs() < 1e-9, "the four saturating circuits are one");
        // a Saw D through the 846 Hz filter: every harmonic about 6 dB under the clean one
        let saw = |circuit: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_WAVE), 10);
            e.set(FILT_BASE + F_ON, 1);
            e.set(FILT_BASE + F_FREQ, 846);
            e.set(F2_BASE + F2_CIRCUIT, circuit);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let x: Vec<f64> = x[13_230..].iter().map(|&v| f64::from(v) / 32768.0).collect();
            (1..=8)
                .map(|k| {
                    let w = std::f64::consts::TAU * k as f64 * 220.0 / 44_100.0;
                    let (mut c, mut s) = (0.0, 0.0);
                    for (i, v) in x.iter().enumerate() {
                        let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
                        c += v * win * (w * i as f64).cos();
                        s += v * win * (w * i as f64).sin();
                    }
                    20.0 * (c.hypot(s) / (x.len() as f64 / 4.0) * k as f64 / 0.0484).max(1e-9).log10()
                })
                .collect::<Vec<f64>>()
        };
        let (clean, circ) = (saw(0), saw(4));
        for k in 1..8 {
            let d = circ[k] - clean[k];
            assert!((d + 6.0).abs() < 1.0, "harmonic {}: {d} dB against the clean filter", k + 1);
        }
        assert!((circ[0] - clean[0] + 3.9).abs() < 0.8, "harmonic 1: {}", circ[0] - clean[0]);
    }

    /// OPERATOR 0.7's Shaper (2026-11-20; a sine through the open filter at Shp Drive 0.5, 4, 8 and 12 dB, and a Saw D through the 846 Hz filter at +6 dB): Hard clips at plus and minus
    /// 1 (fund 1.042, 3rd -36.8 dB at +0.5 dB; 1.183, -15.6 at +4; 1.259, -10.4 at +12), Sine folds (1.134, -18.3 at 0 dB; -21 dB fundamental at +8), Soft is the circuit's curve, 4 Bit
    /// quantises to 17 levels; the saw's harmonics above the cutoff are filled in, so the stage follows the filter.
    #[test]
    fn fitted_to_operator_the_shaper() {
        let sine = |shaper: i32, drive10: i32, wet: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(F2_BASE + F2_SHAPER, shaper);
            e.set(F2_BASE + F2_SHP_DRIVE, drive10);
            e.set(F2_BASE + F2_DRYWET, wet);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let x: Vec<f64> = x[13_230..].iter().map(|&v| f64::from(v) / 32768.0 / 0.0484).collect();
            let amp = |k: f64| {
                let w = std::f64::consts::TAU * k * 220.0 / 44_100.0;
                let (mut c, mut s) = (0.0, 0.0);
                for (i, v) in x.iter().enumerate() {
                    let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
                    c += v * win * (w * i as f64).cos();
                    s += v * win * (w * i as f64).sin();
                }
                c.hypot(s) / (x.len() as f64 / 4.0)
            };
            let f = amp(1.0);
            (20.0 * f.log10(), 20.0 * (amp(3.0) / f).max(1e-9).log10())
        };
        // Hard: the clip level is 1.0 (nothing at Drive 0, the fitted clip of a = 1.06, 1.58, 2.5, 4)
        let (f, h3) = sine(2, 0, 100);
        assert!(f.abs() < 0.15 && h3 < -60.0, "Hard at 0 dB leaves a full sine alone: {f} {h3}");
        for (drive, f_db, h3_db) in [(5, 0.36, -36.8), (40, 1.46, -15.6), (120, 2.0, -10.4)] {
            let (f, h3) = sine(2, drive, 100);
            assert!((f - f_db).abs() < 0.4 && (h3 - h3_db).abs() < 1.5, "Hard {drive}: {f} {h3}");
        }
        // Sine: a full sine folds to 1.134 with the 3rd at -18.3 dB, and at +8 dB the fundamental all but vanishes
        let (f, h3) = sine(3, 0, 100);
        assert!((f - 1.1).abs() < 0.3 && (h3 + 18.3).abs() < 1.0, "Sine: {f} {h3}");
        let (f, _) = sine(3, 80, 100);
        assert!(f < -12.0, "Sine at +8 dB: {f}");
        // Soft is the circuit's curve: -5.3 dB with the 3rd at -19.5
        let (f, h3) = sine(1, 0, 100);
        assert!((f + 5.3).abs() < 0.4 && (h3 + 19.5).abs() < 1.0, "Soft: {f} {h3}");
        // 4 Bit at Drive 0 is nearly clean (the 3rd at -47 dB)
        let (f, h3) = sine(4, 0, 100);
        assert!(f.abs() < 0.3 && h3 < -35.0, "4 Bit: {f} {h3}");
        // Dry/Wet: 0 is the dry sine, 50 between
        let (f0, h0) = sine(2, 120, 0);
        assert!(f0.abs() < 0.15 && h0 < -60.0, "Dry: {f0} {h0}");
        let (_, h50) = sine(2, 120, 50);
        assert!(h50 < -10.4 - 3.0 && h50 > -40.0, "half wet: {h50}");
        // the stage follows the filter: a clipped Saw D keeps harmonics far above the cutoff, a clean one does not
        let saw = |shaper: i32| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_WAVE), 10);
            e.set(FILT_BASE + F_ON, 1);
            e.set(FILT_BASE + F_FREQ, 846);
            e.set(F2_BASE + F2_SHAPER, shaper);
            e.set(F2_BASE + F2_SHP_DRIVE, 60);
            e.note_on(57, 127);
            let mut x = vec![0i16; 44_100];
            e.render(&mut x);
            let x: Vec<f64> = x[13_230..].iter().map(|&v| f64::from(v) / 32768.0).collect();
            let w = std::f64::consts::TAU * 12.0 * 220.0 / 44_100.0;
            let (mut c, mut s) = (0.0, 0.0);
            for (i, v) in x.iter().enumerate() {
                let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
                c += v * win * (w * i as f64).cos();
                s += v * win * (w * i as f64).sin();
            }
            20.0 * (c.hypot(s) / (x.len() as f64 / 4.0) * 12.0 / 0.0484).max(1e-9).log10()
        };
        assert!(saw(2) > saw(0) + 20.0, "Hard fills in the 12th harmonic: {} against {}", saw(2), saw(0));
    }

    /// OPERATOR 0.7's filter Envelope and Freq<Vel (2026-11-20; Saw D, a 24 dB filter at 846 Hz, the default filter envelope Attack 0.1 ms, Decay 598 ms, Sustain 0, Release 50 ms). The
    /// fitted cutoff (about 650 Hz at rest) at Envelope 50 % read 3805, 2111, 1442, 1171, 1002, 919, 800, 721 and 673 Hz at 20, 60, 100, 140, 180, 220, 300, 420 and 540 ms, and at 25 %
    /// 1806, 1192, 968, 872, 814, 773, 721, 684 and 661 Hz: the same octaves halved. Freq<Vel +100 % at velocity 32, 64 and 100 read 1300, 2600 and 5770 Hz; +50 % at 64 and 127 read 1300 and 2555 Hz.
    #[test]
    fn fitted_to_operator_the_filter_envelope_and_freq_vel() {
        let q = 0.5 / (1.0 - 0.97 * 0.27);
        let cutoffs_off = |setup: &dyn Fn(&mut Fm4), vel: u8, starts: &[usize], off: usize| {
            let mut e = Fm4::new(44_100.0);
            e.set(P_LEVEL, 16);
            e.set(op_param(0, O_ATTACK), 1);
            e.set(op_param(0, O_SUSTAIN), 99);
            e.set(ext_param(0, E_WAVE), 10);
            e.set(FILT_BASE + F_ON, 1);
            e.set(FILT_BASE + F_FREQ, 846);
            setup(&mut e);
            e.note_on(57, vel);
            let mut x = vec![0i16; 44_100];
            if off < x.len() {
                let (a, b) = x.split_at_mut(off);
                e.render(a);
                e.note_off(57);
                e.render(b);
            } else {
                e.render(&mut x);
            }
            let x: Vec<f64> = x.iter().map(|&v| f64::from(v) / 32768.0).collect();
            let h = |f: f64, fc: f64| {
                let xx = f / fc;
                1.0 / ((1.0 - xx * xx).powi(2) + (xx / q).powi(2))
            };
            starts
                .iter()
                .map(|&a| {
                    let db: Vec<f64> = (1..=10)
                        .map(|k| {
                            let w = std::f64::consts::TAU * k as f64 * 220.0 / 44_100.0;
                            let (mut c, mut s) = (0.0, 0.0);
                            for i in 0..1764 {
                                let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 1764.0).cos();
                                c += x[a + i] * win * (w * i as f64).cos();
                                s += x[a + i] * win * (w * i as f64).sin();
                            }
                            20.0 * (c.hypot(s) / 441.0 * k as f64).max(1e-9).log10()
                        })
                        .collect();
                    let mut best = (f64::MAX, 0.0);
                    for g in 0..220 {
                        let fc = 200.0 * 2f64.powf(g as f64 / 40.0);
                        let m: Vec<f64> = (1..=10).map(|k| 20.0 * h(k as f64 * 220.0, fc).log10()).collect();
                        let off = db.iter().zip(&m).map(|(d, m)| d - m).sum::<f64>() / 10.0;
                        let e: f64 = db.iter().zip(&m).map(|(d, m)| (d - m - off).powi(2)).sum();
                        if e < best.0 {
                            best = (e, fc);
                        }
                    }
                    best.1
                })
                .collect::<Vec<f64>>()
        };
        let cutoffs = |setup: &dyn Fn(&mut Fm4), vel: u8, starts: &[usize]| cutoffs_off(setup, vel, starts, usize::MAX);
        // the instrument's own corner is 0.9173 of the displayed one (the engine factor): fm4's is the displayed 712 Hz
        let rest = 846.0 * 2f64.powf(-0.25);
        let starts = [0usize, 2646, 4410, 6174, 7938, 13_230, 23_814];
        let want50 = [3805.0, 2111.0, 1442.0, 1171.0, 1002.0, 800.0, 673.0];
        let got = cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 50);
        }, 127, &starts);
        for ((g, w), a) in got.iter().zip(want50).zip(starts) {
            let want = rest + (w - 652.6) * rest / 652.6 * 1.0;
            let want = want.max(rest);
            assert!((g / want - 1.0).abs() < 0.22, "Envelope 50 percent at sample {a}: {g} Hz, wanted about {want}");
        }
        let got25 = cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 25);
        }, 127, &starts[..4]);
        assert!(got25[0] > rest * 2.0 && got25[0] < got[0] * 0.75, "25 percent: {got25:?} against {got:?}");
        let off = cutoffs(&|_| {}, 127, &[0, 13_230]);
        assert!((off[0] / rest - 1.0).abs() < 0.1 && (off[1] / rest - 1.0).abs() < 0.1, "Envelope 0 leaves the cutoff: {off:?}");
        // after the release the cutoff goes back: a held note at the end of the decay is at rest
        let tail = cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 50);
        }, 127, &[30_000]);
        assert!((tail[0] / rest - 1.0).abs() < 0.12, "end of the decay: {tail:?}");
        // Freq<Vel
        let at = |fv: i32, vel: u8| cutoffs(&|e| {
            e.set(F2_BASE + F2_FREQ_VEL, fv);
        }, vel, &[13_230])[0];
        let (a32, a64, a127) = (at(100, 32), at(100, 64), at(0, 127));
        assert!((a32 / a127 - 2.0).abs() < 0.3 && (a64 / a127 - 4.0).abs() < 0.6, "Freq<Vel +100: {a32} {a64} against {a127}");
        let (b64, b127) = (at(50, 64), at(50, 127));
        assert!((b64 / a127 - 2.0).abs() < 0.3 && (b127 / a127 - 4.0).abs() < 0.8, "Freq<Vel +50: {b64} {b127}");
        assert!((at(100, 1) / a127 - 1.0).abs() < 0.1, "none at velocity 0");
        // the other envelope times (fitted from the sound, `core/examples/fm4fenv.rs`): the sustain is a plateau of 9 octaves a unit at 100 %, the attack a linear rise to the peak, the release a fall that takes longer for longer times
        let plateau = |sus: i32| cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 50);
            e.set(F2_BASE + F2_ENV_SUSTAIN, sus);
        }, 127, &[40_000])[0];
        let p10 = (plateau(10) / rest).log2();
        let p33 = (plateau(33) / rest).log2();
        assert!((p10 - 0.45).abs() < 0.15 && (p33 - 1.47).abs() < 0.2, "sustain plateaus {p10} and {p33} octaves, wanted 0.45 and 1.47");
        let rise = cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 25);
            e.set(F2_BASE + F2_ENV_ATTACK, 1350);
        }, 127, &[882, 2646, 5200, 8820, 13_230]);
        assert!(rise[0] < rise[1] && rise[1] < rise[2], "a 135 ms attack rises: {rise:?}");
        assert!(rise[2] > rest * 3.0 && rise[4] < rise[2] * 0.7, "and then falls: {rise:?}");
        // a note let go at sample 30_000 and read 20 ms later (a window starting at 30_000): 0.30, 0.38, 0.50 and 0.70 octave above the rest for 41, 50, 74 and 128 ms
        let after = |rel: i32| cutoffs_off(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 50);
            e.set(F2_BASE + F2_ENV_SUSTAIN, 40);
            e.set(F2_BASE + F2_ENV_RELEASE, rel);
        }, 127, &[30_000], 30_000)[0];
        // the Levels page: Peak 0 removes the envelope, Initial 100 % with a 135 ms attack holds the peak from the strike, End 100 % lifts the cutoff once the note is let go
        let no_peak = cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 25);
            e.set(F2_BASE + F2_ENV_PEAK, 0);
        }, 127, &[0, 13_230]);
        assert!(no_peak.iter().all(|c| (c / rest - 1.0).abs() < 0.1), "Peak 0: {no_peak:?}");
        let flat = cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 25);
            e.set(F2_BASE + F2_ENV_ATTACK, 1350);
            e.set(F2_BASE + F2_ENV_INITIAL, 100);
        }, 127, &[0, 2646, 4410, 13_230]);
        assert!(flat[0] > rest * 3.0 && (flat[1] / flat[0] - 1.0).abs() < 0.25 && flat[3] < flat[0] * 0.5, "Initial 100: {flat:?}");
        let end = |pct: i32| cutoffs_off(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 25);
            e.set(F2_BASE + F2_ENV_END, pct);
        }, 127, &[30_000], 30_000)[0];
        assert!(end(100) > end(0) * 1.8 && end(0) < rest * 1.3, "End: {} against {}", end(100), end(0));
        // Time<Vel +100 %: the decay runs 0.25 times as long at velocity 127 and 0.7 at 32 (the cutoff 20 and 50 ms in, 25 % amount: 1.2 and 0.5 octave against 2.0 and 1.6 without)
        let tv = |pct: i32, vel: u8| cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 25);
            e.set(F2_BASE + F2_ENV_TIMEVEL, pct);
        }, vel, &[2646])[0];
        assert!(tv(100, 127) < tv(0, 127) * 0.8 && tv(100, 32) < tv(0, 32) * 0.95 && tv(100, 127) < tv(100, 32), "Time<Vel: {} {} {} {}", tv(100, 127), tv(0, 127), tv(100, 32), tv(0, 32));
        assert!((tv(0, 32) / tv(0, 127) - 1.0).abs() < 0.02, "no Time<Vel, no velocity effect");
        // Beat: struck again every Repeat (1/8T at 120 is 1/6 s here): the cutoff rises again after it had fallen
        let beat = cutoffs(&|e| {
            e.set(F2_BASE + F2_ENV_AMOUNT, 25);
            e.set(F2_BASE + F2_ENV_MODE, 2);
        }, 127, &[0, 5800, 8000]); // the second window ends just before the restart at 7350, the third starts just after
        assert!(beat[1] < beat[0] * 0.6 && beat[2] > beat[1] * 1.5, "Beat restarts the envelope: {beat:?}");
        let o: Vec<f64> = [41, 50, 74, 128].iter().map(|&r| (after(r) / rest).log2()).collect();
        assert!(o[0] < o[1] && o[1] < o[2] && o[2] < o[3], "a longer release holds the cutoff up longer: {o:?}");
        for (g, w) in o.iter().zip([0.30, 0.38, 0.50, 0.70]) {
            assert!((g - w).abs() < 0.2, "release: {o:?}");
        }
    }
}
