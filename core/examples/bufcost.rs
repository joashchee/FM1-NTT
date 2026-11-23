//! How even is the cost of one audio buffer? Renders a firmware's audio in buffers of `frames` and prints the host time per call
//! against the buffer's own period: the mean, the percentiles and the worst, and how many calls ran no instruction.
//! A firmware that makes its audio in big half-buffers would show a high p99/mean here (ROADMAP.md section 1, "Even pacing").
//!   cargo run --release --example bufcost -- FIRMWARE.fwsc [jit|nojit] [frames] [chord]
//! Idle firmware, or with `chord` four note keys (a major seventh, keys 0, 4, 7, 11) held through the 20 s; measures this machine, not an R36S. The firmware is read as data and nothing is written.
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let Some(path) = a.get(1) else {
        eprintln!("usage: bufcost FIRMWARE.fwsc [jit|nojit] [frames] [chord]");
        return;
    };
    let pkg = std::fs::read(path).expect("cannot read the firmware");
    let jit = a.get(2).is_some_and(|s| s == "jit");
    let frames: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(512).clamp(32, 8192);
    let mut m = fm1ntt::machine::Machine::from_package(&pkg).expect("not a firmware package");
    m.set_jit(jit);
    let mut out = vec![0i16; frames * 2];
    for _ in 0..(44100 * 4 / frames) {
        m.render(&mut out, u32::MAX); // four seconds to boot
    }
    let chord = a.get(4).is_some_and(|s| s == "chord");
    if chord {
        for k in [0u8, 4, 7, 11] {
            m.key(14 + k, true);
        }
    }
    let n = 44100 * 20 / frames;
    let (mut us, mut idle) = (Vec::with_capacity(n), 0usize);
    for _ in 0..n {
        let before = m.instructions();
        let t = Instant::now();
        m.render(&mut out, u32::MAX);
        us.push(t.elapsed().as_secs_f64() * 1e6);
        idle += usize::from(m.instructions() == before);
    }
    let period = frames as f64 / 44100.0 * 1e6;
    let mean = us.iter().sum::<f64>() / n as f64;
    us.sort_by(f64::total_cmp);
    let pc = |p: f64| us[((us.len() - 1) as f64 * p) as usize];
    println!(
        "jit={jit} chord={chord} frames={frames} period={period:.0}us calls={n} mean={mean:.0}us ({:.1}% of period) p50={:.0} p90={:.0} p99={:.0} max={:.0}us (p99 {:.2}x mean, max {:.1}x) no-instruction calls {:.0}% halted={}",
        100.0 * mean / period,
        pc(0.5),
        pc(0.9),
        pc(0.99),
        us[us.len() - 1],
        pc(0.99) / mean,
        us[us.len() - 1] / mean,
        100.0 * idle as f64 / n as f64,
        m.halted()
    );
}
