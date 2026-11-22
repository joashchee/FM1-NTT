use std::io::{BufRead, Write};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = std::fs::File::open(&a[1]).unwrap();
    let mut o = std::io::BufWriter::new(std::fs::File::create(&a[2]).unwrap());
    for line in std::io::BufReader::new(f).lines() {
        let h = line.unwrap();
        let b: Vec<u8> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect();
        match fm1_isa::decode_bytes(&b, 0x1000) {
            Ok(i) => writeln!(o, "{}\t{}\t{}", h, i.len(), i.syntax()).unwrap(),
            Err(_) => writeln!(o, "{}\t0\t-", h).unwrap(),
        }
    }
}
