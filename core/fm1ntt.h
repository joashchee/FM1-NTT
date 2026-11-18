/* FM1-NTT core's C API (core/src/capi.rs). MIT. Every function tolerates NULL.
 * The matrix: ids 0..13 are buttons (FM1_BUTTON_NAMES), ids 14..40 are the 27 note keys F3..G5; encoders 0..6; one pot. */
#ifndef FM1NTT_H
#define FM1NTT_H
#include <stddef.h>
#include <stdint.h>

#define FM1_SCREEN_W 240
#define FM1_SCREEN_H 240
#define FM1_BUTTONS 14
#define FM1_KEY_BASE 14
#define FM1_KEYS 27
#define FM1_ENCODERS 7
#define FM1_SAMPLE_RATE 44100
#define FM1_ENTRY_FELUCCA 0x02000120u

typedef struct Handle fm1ntt_machine;

const char *fm1ntt_version(void);
fm1ntt_machine *fm1ntt_new(void);
void fm1ntt_free(fm1ntt_machine *m);
int fm1ntt_load_package(fm1ntt_machine *m, const uint8_t *bytes, size_t len);               /* .fwsc; 0 ok, -1 see fm1ntt_error */
int fm1ntt_load_raw(fm1ntt_machine *m, const uint8_t *bytes, size_t len, uint32_t entry);   /* raw XIP image; 0 ok, -1 */
int fm1ntt_loaded(const fm1ntt_machine *m);
size_t fm1ntt_render(fm1ntt_machine *m, int16_t *out, size_t frames, uint32_t max_insns);   /* stereo i16; returns real frames */
void fm1ntt_screen(fm1ntt_machine *m, uint32_t *out);                                       /* 240x240 words, 0x00RRGGBB */
uint32_t fm1ntt_lcd_revision(fm1ntt_machine *m);
void fm1ntt_key(fm1ntt_machine *m, int id, int down);                                       /* matrix id 0..40 */
void fm1ntt_encoder(fm1ntt_machine *m, int enc, int clicks);                                /* + clockwise */
void fm1ntt_pot(fm1ntt_machine *m, int value);                                              /* 0..1023 */
void fm1ntt_midi_in(fm1ntt_machine *m, int byte);
int fm1ntt_halted(fm1ntt_machine *m);
uint64_t fm1ntt_keys_down(fm1ntt_machine *m);                                               /* bit n = id n */
uint64_t fm1ntt_ms(fm1ntt_machine *m);                                                      /* emulated ms since boot */
void fm1ntt_jit(fm1ntt_machine *m, int on);                                                 /* off by default */
void fm1ntt_pace(fm1ntt_machine *m, int level);                                             /* main-loop pacing 0 (off, exact) to 3; off by default */
int fm1ntt_pace_level(fm1ntt_machine *m);
const char *fm1ntt_error(void);                                                             /* last failure, "" when none */

/* ---- the sandbox panels' views ---- */
#define FM1_REGION_RAM 0
#define FM1_REGION_XIP 1
#define FM1_REGION_FLASH 2
typedef struct { uint32_t pc, sp, flags; uint32_t regs[16]; uint64_t insns; } Fm1Cpu;
int fm1ntt_cpu(fm1ntt_machine *m, int core, Fm1Cpu *out);                                   /* core 0 or 1; 0 when there is none */
int fm1ntt_region(fm1ntt_machine *m, int region, uint32_t *base, size_t *size);              /* its chip address and size */
size_t fm1ntt_read_mem(fm1ntt_machine *m, int region, size_t offset, uint8_t *out, size_t len); /* reads no peripheral */
size_t fm1ntt_console(fm1ntt_machine *m, uint8_t *out, size_t cap);                         /* the last bytes the firmware wrote */
void fm1ntt_midi_stats(fm1ntt_machine *m, uint64_t *queued, uint64_t *dropped);
void fm1ntt_reset(fm1ntt_machine *m);                                                       /* power-on again, same firmware */
size_t fm1ntt_flash_size(void);                                                             /* 1 MiB */
size_t fm1ntt_flash_save(fm1ntt_machine *m, uint8_t *out, size_t cap);                      /* bytes copied, 0 if cap is too small */
int fm1ntt_flash_restore(fm1ntt_machine *m, const uint8_t *bytes, size_t len);              /* resets, then restores; 0 ok, -1 */
int fm1ntt_jit_on(fm1ntt_machine *m);
void fm1ntt_jit_profile(fm1ntt_machine *m, int on);                                        /* count JIT entries and compiled instructions (before the first run; slows it) */
size_t fm1ntt_jit_report(fm1ntt_machine *m, char *out, size_t cap);                         /* one line of JSON about the JIT; its length, 0 if cap is too small */
const char *fm1ntt_board(fm1ntt_machine *m);                                                /* "Fm1", "Smk37", "" for a raw image */
size_t fm1ntt_bench_image(uint8_t *out, size_t cap);                                       /* the benchmark workload: load it with fm1ntt_load_raw at FM1_ENTRY_FELUCCA */
uint32_t fm1ntt_bench_version(void);                                                        /* bump = reports are not comparable */
int fm1ntt_jit_available(void);                                                            /* 1 when the build's CPU supports the native JIT */
uint32_t fm1ntt_clock_mhz(void);                                                            /* 240: instructions a second real time needs */
/* The sound engines (core/src/capi_sound.rs): fm6 (a DX7 voice from the keys) and the sampler. All mono 16-bit; null-safe; a render of more than 1 << 20 frames returns 0. */
typedef struct Fm6Handle fm1ntt_fm6;
typedef struct SamplerHandle fm1ntt_sampler;
typedef struct KitHandle fm1ntt_kit;
size_t fm1ntt_factory_voices(uint8_t *out, size_t max);                                             /* the built-in voices (128 bytes each), at most max; the count */
size_t fm1ntt_package_voices(const uint8_t *pkg, size_t len, uint8_t *out, size_t max);    /* the packed DX7 voices (128 bytes each) in a .fwsc's banks, at most max; the count */
fm1ntt_fm6 *fm1ntt_fm6_new(double sample_rate, const uint8_t *voice128);                    /* a packed DX7 voice (128 bytes) or NULL for a plain sine; NULL on failure */
void fm1ntt_fm6_free(fm1ntt_fm6 *h);
int fm1ntt_fm6_set_voice_sysex(fm1ntt_fm6 *h, const uint8_t *bytes, size_t len);           /* a 163-byte DX7 single-voice dump; 0 ok, -1 */
void fm1ntt_fm6_set_model(fm1ntt_fm6 *h, int output, int pot, int tuning, int dtun);       /* output 0 msfa 1 Felucca 2 stock (pot 0..1023); tuning 0 msfa 1 Felucca (dtun) 2 stock */
void fm1ntt_fm6_note_on(fm1ntt_fm6 *h, int note, int velocity);
void fm1ntt_fm6_note_off(fm1ntt_fm6 *h, int note);
void fm1ntt_fm6_pitch_bend(fm1ntt_fm6 *h, int value);                                      /* 0..16383, 8192 centre */
void fm1ntt_fm6_all_off(fm1ntt_fm6 *h);
size_t fm1ntt_fm6_render(fm1ntt_fm6 *h, int16_t *out, size_t frames);
size_t fm1ntt_fm6_sounding(const fm1ntt_fm6 *h);
fm1ntt_sampler *fm1ntt_sampler_new(const uint8_t *wav, size_t len, double out_rate, int root); /* a WAV (PCM or IMA ADPCM) played from the keys; NULL on failure */
void fm1ntt_sampler_free(fm1ntt_sampler *h);
void fm1ntt_sampler_note_on(fm1ntt_sampler *h, int note, int velocity);
void fm1ntt_sampler_note_off(fm1ntt_sampler *h, int note);
void fm1ntt_sampler_all_off(fm1ntt_sampler *h);
size_t fm1ntt_sampler_render(fm1ntt_sampler *h, int16_t *out, size_t frames);
fm1ntt_kit *fm1ntt_kit_new(double out_rate, double ceiling);                                /* a drum map; ceiling is the peak limit as a fraction of full scale, 0 for none (X0X: 0.4) */
void fm1ntt_kit_free(fm1ntt_kit *h);
int fm1ntt_kit_add_wav(fm1ntt_kit *h, int note, const uint8_t *wav, size_t len, const uint8_t *chokes, size_t n_chokes); /* 0 ok, -1; striking the note cuts the notes in chokes */
void fm1ntt_kit_note_on(fm1ntt_kit *h, int note, int velocity);
void fm1ntt_kit_all_off(fm1ntt_kit *h);
size_t fm1ntt_kit_render(fm1ntt_kit *h, int16_t *out, size_t frames);
#endif

/* The instrument host (core/src/capi_inst.rs, docs/PORTING.md): a package (.fm1i text) played, edited and written back. Null-safe; failures say why in fm1ntt_error. */
typedef struct InstHandle fm1ntt_inst;
typedef int64_t (*fm1ntt_read_fn)(void *user, const char *name, uint8_t *buf, size_t cap);   /* a sample file for the loader: cap 0 asks the size; -1 when unreadable */
fm1ntt_inst *fm1ntt_inst_load(const uint8_t *text, size_t len, double rate, fm1ntt_read_fn read, void *user);   /* NULL when not a package, the licence is off the allowlist, or the engine cannot be built */
fm1ntt_inst *fm1ntt_inst_factory(double rate);                                              /* the built-in voices */
void fm1ntt_inst_free(fm1ntt_inst *h);
int fm1ntt_package_tier(const uint8_t *text, size_t len);                                   /* 0 permissive (MIT app ok), 1 copyleft (the Pack only; never the MIT app or a Mac App Store build), -1 not a package */
size_t fm1ntt_inst_text(const fm1ntt_inst *h, int which, char *out, size_t cap);            /* 0 name, 1 kind, 2 licence, 3 verification note ("" = unverified), 4 voice name (fm6); the full length */
void fm1ntt_inst_midi(fm1ntt_inst *h, const uint8_t *bytes, size_t len);                    /* raw MIDI bytes: notes, controllers through the package's map, bend, program change */
void fm1ntt_inst_note_on(fm1ntt_inst *h, int note, int velocity);
void fm1ntt_inst_note_off(fm1ntt_inst *h, int note);
void fm1ntt_inst_all_off(fm1ntt_inst *h);
size_t fm1ntt_inst_sounding(const fm1ntt_inst *h);
size_t fm1ntt_inst_render_stereo(fm1ntt_inst *h, int16_t *out, size_t frames);              /* interleaved; frames written, 0 for more than 1 << 20 */
size_t fm1ntt_inst_render(fm1ntt_inst *h, int16_t *out, size_t frames);                     /* mono */
size_t fm1ntt_inst_voice_count(const fm1ntt_inst *h);
size_t fm1ntt_inst_program(const fm1ntt_inst *h);
void fm1ntt_inst_set_program(fm1ntt_inst *h, size_t program);
size_t fm1ntt_inst_param_count(const fm1ntt_inst *h);
int fm1ntt_inst_param(const fm1ntt_inst *h, size_t index, int32_t info[4], char *name, size_t cap);   /* info = { id, min, max, default }; 0 ok, -1 out of range */
int32_t fm1ntt_inst_get(const fm1ntt_inst *h, int id);                                      /* INT32_MIN when there is no such parameter */
int fm1ntt_inst_set(fm1ntt_inst *h, int id, int value);                                     /* clamped to the range; 0 ok, -1 */
typedef struct fm1ntt_evq fm1ntt_evq;                                                       /* a lock-free queue of timestamped MIDI, one pushing thread to the audio thread */
fm1ntt_evq *fm1ntt_evq_new(size_t capacity);                                                /* 16 to 65536 messages */
void fm1ntt_evq_free(fm1ntt_evq *q);
int fm1ntt_evq_push(const fm1ntt_evq *q, uint64_t ts_us, const uint8_t *bytes, size_t len);  /* 1 to 3 bytes, stamped on the host's monotonic microsecond clock; 0 queued, -1 refused (counted) */
size_t fm1ntt_evq_len(const fm1ntt_evq *q);
uint64_t fm1ntt_evq_dropped(const fm1ntt_evq *q);
size_t fm1ntt_render_events(fm1ntt_machine *m, const fm1ntt_evq *q, uint64_t now_us, int16_t *out, size_t frames, uint32_t max_insns);   /* fm1ntt_render, with the queue's MIDI bytes put into the jack at their own frames */
size_t fm1ntt_inst_render_events(fm1ntt_inst *h, const fm1ntt_evq *q, uint64_t now_us, int16_t *out, size_t frames);   /* mono; applies each message at its own frame; no lock shared with the pusher */
size_t fm1ntt_inst_arp_count(const fm1ntt_inst *h);                                        /* the arpeggiator's parameters (core/src/arp.rs): the same four calls */
int fm1ntt_inst_arp_param(const fm1ntt_inst *h, size_t index, int32_t info[4], char *name, size_t cap);
int32_t fm1ntt_inst_arp_get(const fm1ntt_inst *h, int id);
int fm1ntt_inst_arp_set(fm1ntt_inst *h, int id, int value);                                 /* off releases the held note */
size_t fm1ntt_inst_to_package(const fm1ntt_inst *h, const char *source, uint8_t *out, size_t cap);   /* the instrument, edits included, as package text; the full length */
void fm1ntt_sha256_hex(const uint8_t *data, size_t len, char out[65]);
