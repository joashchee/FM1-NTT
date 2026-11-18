//! A small set of original voices for the fm6 engine, written here (MIT), so the SOUND preview and Solo play something when the loaded firmware has no DX7 banks
//! the extractor can find (the bundled Haunted FM-1 firmwares have none), or when no firmware is loaded at all. They are ordinary packed DX7 voices
//! (128 bytes, `fm6::pack_patch`), built from the usual FM recipes (an electric piano is carriers with a tine modulator at 14 times, and so on); **none is copied from
//! a DX7 cartridge or a firmware**, and none has been judged by ear: the tests only check that each sounds, stays under clipping and is the same every time.

use crate::fm6::pack_patch;

/// One operator, in the DX7's terms.
struct Op {
    rates: [u8; 4],
    levels: [u8; 4],
    out: u8,
    coarse: u8,
    fine: u8,
    detune: u8,
    vel: u8,
}

const OFF: Op = Op { rates: [99, 99, 99, 99], levels: [0, 0, 0, 0], out: 0, coarse: 1, fine: 0, detune: 7, vel: 0 };

fn op(rates: [u8; 4], levels: [u8; 4], out: u8, coarse: u8, fine: u8, detune: u8, vel: u8) -> Op {
    Op { rates, levels, out, coarse, fine, detune, vel }
}

/// `ops` are operators 1 to 6, `alg` the algorithm as printed on a DX7 (1 to 32).
fn voice(name: &str, alg: u8, feedback: u8, transpose: u8, vibrato: Option<(u8, u8)>, ops: [Op; 6]) -> [u8; 128] {
    let mut p = [0u8; 156];
    for (i, o) in ops.iter().enumerate() {
        let b = (5 - i) * 21; // operator 6 comes first in a voice
        p[b..b + 4].copy_from_slice(&o.rates);
        p[b + 4..b + 8].copy_from_slice(&o.levels);
        p[b + 8] = 39; // break point
        p[b + 14] = 0; // amplitude modulation sensitivity
        p[b + 15] = o.vel;
        p[b + 16] = o.out;
        p[b + 18] = o.coarse;
        p[b + 19] = o.fine;
        p[b + 20] = o.detune;
    }
    p[126..130].copy_from_slice(&[99, 99, 99, 99]);
    p[130..134].copy_from_slice(&[50, 50, 50, 50]);
    p[134] = alg - 1;
    p[135] = feedback;
    p[137] = 35; // LFO speed
    if let Some((depth, sens)) = vibrato {
        p[139] = depth;
        p[143] = sens;
    }
    p[141] = 1; // LFO key sync
    p[144] = transpose;
    let mut n = [b' '; 10];
    for (d, c) in n.iter_mut().zip(name.bytes()) {
        *d = c;
    }
    p[145..155].copy_from_slice(&n);
    p[155] = 0x3f;
    pack_patch(&p)
}

/// The number of voices `voices` returns.
pub const COUNT: usize = 8;

/// The built-in voices, always the same eight in the same order.
pub fn voices() -> Vec<[u8; 128]> {
    let sustain = [99, 99, 99, 70];
    let hold = [99, 99, 99, 0];
    vec![
        // a plain sine: one carrier
        voice("SINE", 32, 0, 24, None, [op(sustain, hold, 90, 1, 0, 7, 0), OFF, OFF, OFF, OFF, OFF]),
        // an electric piano: three carriers, each with a modulator; the tine one at 14 times, quick to fade
        voice("E.PIANO", 5, 3, 24, None, [
            op([96, 25, 25, 67], [99, 75, 0, 0], 99, 1, 0, 7, 2),
            op([95, 50, 35, 78], [99, 75, 0, 0], 58, 14, 0, 7, 3),
            op([95, 50, 35, 78], [99, 75, 0, 0], 99, 1, 0, 8, 2),
            op([95, 29, 20, 50], [99, 95, 0, 0], 82, 1, 0, 7, 2),
            OFF,
            OFF,
        ]),
        // a bass: one carrier, a modulator at the same pitch that fades into it
        voice("FM BASS", 5, 0, 12, None, [
            op([99, 55, 30, 70], [99, 90, 80, 0], 99, 1, 0, 7, 1),
            op([99, 60, 40, 70], [99, 70, 55, 0], 78, 1, 0, 7, 2),
            OFF,
            OFF,
            OFF,
            OFF,
        ]),
        // a bell: inharmonic modulators (3.5 and 7) over a long ring
        voice("BELL", 5, 0, 24, None, [
            op([99, 25, 22, 40], [99, 80, 50, 0], 95, 1, 0, 7, 1),
            op([99, 30, 25, 40], [99, 70, 40, 0], 70, 2, 75, 7, 2),
            op([99, 35, 25, 45], [99, 70, 40, 0], 85, 4, 0, 7, 1),
            op([99, 40, 30, 50], [99, 60, 30, 0], 62, 7, 0, 7, 2),
            OFF,
            OFF,
        ]),
        // an organ: six carriers at the drawbar ratios, a flat envelope
        voice("ORGAN", 32, 0, 24, None, [
            op([99, 99, 99, 85], hold, 70, 0, 0, 7, 0),
            op([99, 99, 99, 85], hold, 74, 1, 0, 7, 0),
            op([99, 99, 99, 85], hold, 68, 2, 0, 7, 0),
            op([99, 99, 99, 85], hold, 60, 3, 0, 7, 0),
            op([99, 99, 99, 85], hold, 54, 4, 0, 7, 0),
            op([99, 99, 99, 85], hold, 48, 6, 0, 7, 0),
        ]),
        // a brass: a slow rise, the modulator brighter than the carrier, a little feedback
        voice("BRASS", 5, 4, 24, None, [
            op([68, 60, 50, 60], [99, 92, 88, 0], 99, 1, 0, 7, 1),
            op([62, 55, 50, 60], [99, 86, 80, 0], 80, 1, 0, 7, 3),
            op([68, 60, 50, 60], [99, 92, 88, 0], 96, 1, 0, 9, 1),
            op([62, 55, 50, 60], [99, 86, 80, 0], 78, 1, 0, 5, 3),
            OFF,
            OFF,
        ]),
        // a pluck: the modulator is gone before the carrier
        voice("PLUCK", 5, 0, 24, None, [
            op([99, 45, 30, 60], [99, 70, 0, 0], 99, 1, 0, 7, 2),
            op([99, 70, 50, 60], [99, 45, 0, 0], 74, 3, 0, 7, 3),
            OFF,
            OFF,
            OFF,
            OFF,
        ]),
        // strings: six carriers in detuned pairs, a slow attack, a little vibrato
        voice("STRINGS", 32, 0, 24, Some((14, 3)), [
            op([48, 40, 50, 40], [99, 99, 99, 0], 76, 1, 0, 7, 1),
            op([48, 40, 50, 40], [99, 99, 99, 0], 76, 1, 0, 10, 1),
            op([48, 40, 50, 40], [99, 99, 99, 0], 76, 1, 0, 4, 1),
            op([46, 40, 50, 40], [99, 99, 99, 0], 66, 2, 0, 7, 1),
            op([46, 40, 50, 40], [99, 99, 99, 0], 66, 2, 0, 11, 1),
            op([46, 40, 50, 40], [99, 99, 99, 0], 66, 2, 0, 3, 1),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fm6::{unpack_patch, voice_name, Synth};

    #[test]
    fn there_are_eight_named_voices_and_they_do_not_change() {
        let v = voices();
        assert_eq!(v.len(), COUNT);
        assert_eq!(v, voices());
        let names: Vec<String> = v.iter().map(|b| voice_name(&unpack_patch(b))).collect();
        assert_eq!(names, ["SINE", "E.PIANO", "FM BASS", "BELL", "ORGAN", "BRASS", "PLUCK", "STRINGS"]);
    }

    #[test]
    fn every_voice_sounds_and_none_clips_even_in_a_chord() {
        for (i, v) in voices().iter().enumerate() {
            let mut s = Synth::new(44100.0, v).unwrap();
            let mut buf = vec![0i16; 44100 / 2];
            s.note_on(60, 100);
            s.render(&mut buf);
            let peak = buf.iter().map(|x| (*x as i32).abs()).max().unwrap();
            assert!(peak > 800, "voice {} is nearly silent (peak {})", i, peak);
            assert!(peak < 30000, "voice {} is too loud (peak {})", i, peak);
            for n in [48, 55, 64, 67, 72, 76] {
                s.note_on(n, 110);
            }
            let mut chord = vec![0i16; 44100 / 4];
            s.render(&mut chord);
            let cp = chord.iter().map(|x| (*x as i32).abs()).max().unwrap();
            assert!(cp < 32000, "voice {} clips in a chord (peak {})", i, cp);
        }
    }
}
