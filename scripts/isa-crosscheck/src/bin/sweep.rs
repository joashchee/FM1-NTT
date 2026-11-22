use fm1ntt::machine::{Machine, Region};
// Usage: cargo run --release --bin sweep -- FIRMWARE.fwsc... > sweep.tsv  (then sweep_side.py in the container, see docs/ISA-CROSSCHECK.md)
// For each firmware: linear sweep of the XIP image from 0x120 with fm1-isa. A failure that follows at least RUN good decodes in a row is "apparent code":
// print its window so pi32v2-isa can say what it is. Data decodes in long runs only by chance (about two words in three decode).
const RUN: u32 = 32;
fn main() {
    for path in std::env::args().skip(1) {
        let pkg = std::fs::read(&path).unwrap();
        let m = Machine::from_package(&pkg).unwrap();
        let (_, n) = m.region(Region::Xip);
        let mut img = vec![0u8; n];
        m.read_mem(Region::Xip, 0, &mut img);
        let name = path.rsplit('/').next().unwrap();
        let (mut at, mut run, mut ok, mut fails, mut hits) = (0x120usize, 0u32, 0usize, 0usize, 0usize);
        while at + 6 <= img.len() {
            match fm1_isa::decode_bytes(&img[at..at + 6], 0) {
                Ok(i) => { at += i.len() as usize; run += 1; ok += 1; }
                Err(_) => {
                    fails += 1;
                    if run >= RUN { hits += 1; println!("HIT\t{name}\t{at:#x}\t{}", img[at..at + 6].iter().map(|b| format!("{b:02x}")).collect::<String>()); }
                    at += 2; run = 0;
                }
            }
        }
        println!("SUM\t{name}\txip {n} bytes\tdecoded {ok}\tfailures {fails}\tfailures after >= {RUN} good decodes: {hits}");
    }
}
