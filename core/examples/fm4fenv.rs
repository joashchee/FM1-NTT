//! `fm4fenv AMOUNT SUSTAIN_PCT DECAY_MS RELEASE_MS HOLD OUT.f32 [ATTACK_TENTHS_MS]`: fm4 set up like OPERATOR's filter-envelope runs (operator A a Saw D, the 24 dB filter at 846 Hz, note 57 at velocity 127),
//! held for HOLD seconds and released, 1.5 s more, as mono f32 for the fitting scripts (docs/PORTING.md).
use fm1ntt::{engine::Engine, fm4::*};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n = |i: usize, d: f64| a.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
    let (amt, sus, dec, rel, hold) = (n(1, 50.0), n(2, 0.0), n(3, 598.0), n(4, 50.0), n(5, 1.0));
    let mut e = Fm4::new(44_100.0);
    e.set(P_LEVEL, 16);
    e.set(op_param(0, O_ATTACK), 1);
    e.set(op_param(0, O_SUSTAIN), 99);
    e.set(ext_param(0, E_WAVE), 10);
    e.set(FILT_BASE + F_ON, 1);
    e.set(FILT_BASE + F_FREQ, 846);
    e.set(F2_BASE + F2_ENV_AMOUNT, amt as i32);
    e.set(F2_BASE + F2_ENV_SUSTAIN, sus as i32);
    e.set(F2_BASE + F2_ENV_DECAY, dec as i32);
    e.set(F2_BASE + F2_ENV_RELEASE, rel as i32);
    if let Some(att) = a.get(7).and_then(|v| v.parse::<i32>().ok()) {
        e.set(F2_BASE + F2_ENV_ATTACK, att); // tenths of a millisecond
    }
    e.pitch_bend(2180); // OPERATOR sounds 146.8 cents flat here
    e.note_on(57, 127);
    let mut out = Vec::new();
    let mut blk = vec![0i16; 441];
    for i in 0..((hold + 1.5) * 100.0) as usize {
        if i == (hold * 100.0) as usize {
            e.note_off(57);
        }
        e.render(&mut blk);
        out.extend(blk.iter().flat_map(|&v| (f32::from(v) / 32768.0).to_le_bytes()));
    }
    std::fs::write(a.get(6).map_or("fm4fenv.f32", |s| s.as_str()), out).unwrap();
}
