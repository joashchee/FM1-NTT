//! The Rust port of msfa (core/src/fm6.rs) against the vendored C++ compiled as it is (core/tests/msfa/oracle.cc,
//! written to golden.txt by scripts/msfa-oracle.sh): the tables, and 96 notes of 60 blocks each, bit for bit.

use fm1ntt::fm6::*;
use std::collections::HashMap;

fn fnv(mut h: u64, words: impl IntoIterator<Item = i32>) -> u64 {
    for w in words {
        for b in w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}
const FNV0: u64 = 0xcbf29ce484222325;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap()).collect()
}

#[test]
fn matches_the_c_plus_plus_reference_bit_for_bit() {
    let golden = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/msfa/golden.txt")).unwrap();
    let t = Tables::new(44100.0).unwrap();
    let mut voices: HashMap<usize, [u8; 128]> = HashMap::new();
    let (mut tables, mut plays) = (0, 0);
    for line in golden.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[0] {
            "table" => {
                let want = u64::from_str_radix(f[2], 16).unwrap();
                let got = match f[1] {
                    "sin" => fnv(FNV0, t.sin_table().iter().copied()),
                    "exp2" => fnv(FNV0, t.exp2_table().iter().copied()),
                    "freq" => fnv(FNV0, t.freq_table().iter().copied()),
                    x => panic!("unknown table {x}"),
                };
                assert_eq!(got, want, "the {} table differs from the reference (this host's libm?)", f[1]);
                tables += 1;
            }
            "voice" => {
                let b = hex(f[2]);
                voices.insert(f[1].parse().unwrap(), b.try_into().unwrap());
            }
            "play" => {
                let vi: usize = f[1].parse().unwrap();
                let (note, vel, bend): (i32, i32, i32) = (f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap());
                let want_hash = u64::from_str_radix(f[5], 16).unwrap();
                let want_sum: i64 = f[6].parse().unwrap();
                let want_first: Vec<i32> = f[7..11].iter().map(|x| x.parse().unwrap()).collect();
                let patch = unpack_patch(&voices[&vi]);
                let mut lfo = Lfo::new();
                lfo.reset(&t, lfo_params(&patch));
                lfo.keydown();
                let mut v = Voice::new(&t, &patch, note, vel);
                let (mut h, mut sum, mut first) = (FNV0, 0i64, vec![]);
                for blk in 0..60 {
                    if blk == 30 {
                        v.key_up();
                    }
                    let mut buf = [0i32; N];
                    let (lv, ld) = (lfo.getsample(&t), lfo.getdelay());
                    v.compute(&t, &mut buf, lv, ld, bend);
                    h = fnv(h, buf);
                    sum += buf.iter().map(|&x| (x as i64).abs()).sum::<i64>();
                    if blk == 5 {
                        first = buf[32..36].to_vec();
                    }
                }
                let what = format!("voice {vi} (algorithm {vi}), note {note}, velocity {vel}, bend {bend}");
                assert_eq!(first, want_first, "{what}: first samples");
                assert_eq!(sum, want_sum, "{what}: sum of |samples|");
                assert_eq!(h, want_hash, "{what}: hash of 60 blocks");
                assert!(sum > 0, "{what}: silent");
                plays += 1;
            }
            _ => {}
        }
    }
    assert_eq!((tables, plays), (3, 96), "the golden file is incomplete");
}
