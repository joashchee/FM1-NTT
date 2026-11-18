//! `soundbench [--blocks N] [--rate HZ]`: what fm6, the sampler and the kit cost per 64-sample block (fm6's block, `fm6::N`) on this machine,
//! against the block's own time (64 / rate). Every figure is the median of five runs of N blocks with the voices held; nothing is written.
//! Not a firmware benchmark: `fm1ntt --bench` is that (docs/BENCHMARKS.md). Run it with `--release`.
use fm1ntt::{engine::Engine, fm6, fx, sampler};
use std::time::Instant;

/// A held, sustaining 6-operator voice in the DX7 packed layout, with the given algorithm (0 to 31).
fn voice(algorithm: u8) -> [u8; 128] {
    let mut v = [0u8; 128];
    for op in 0..6 {
        let o = op * 17;
        v[o..o + 4].copy_from_slice(&[90, 80, 60, 50]);      // rates
        v[o + 4..o + 8].copy_from_slice(&[99, 90, 80, 0]);   // levels: sustain at 80
        v[o + 8] = 39;                                         // break point
        v[o + 12] = 7 << 3;                                    // detune 7 (centre)
        v[o + 14] = 99;                                        // output level
        v[o + 15] = 2;                                         // coarse 1
    }
    v[102..106].copy_from_slice(&[99, 99, 99, 99]);            // pitch EG rates
    v[106..110].copy_from_slice(&[50, 50, 50, 50]);            // pitch EG levels (centre)
    v[110] = algorithm;
    v[111] = 7;                                                // feedback
    v[117] = 24;                                               // transpose
    v[118..128].copy_from_slice(b"BENCH     ");
    v
}

fn median(mut x: Vec<f64>) -> f64 {
    x.sort_by(|a, b| a.partial_cmp(b).unwrap());
    x[x.len() / 2]
}

/// Median over five runs of `blocks` calls of `f`, in nanoseconds a call.
fn time(blocks: usize, mut f: impl FnMut()) -> f64 {
    for _ in 0..200 {
        f();
    }
    median(
        (0..5)
            .map(|_| {
                let t = Instant::now();
                for _ in 0..blocks {
                    f();
                }
                t.elapsed().as_secs_f64() * 1e9 / blocks as f64
            })
            .collect(),
    )
}

fn tone(rate: u32, secs: usize) -> sampler::Sample {
    let mut s = 12345u32;
    let data: Vec<i16> = (0..rate as usize * secs)
        .map(|i| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            ((((i as f64) * 0.05).sin() * 9000.0) as i32 + ((s >> 20) as i32 - 2048)) as i16
        })
        .collect();
    sampler::Sample::new(rate, data)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |k: &str, d: f64| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(d);
    let blocks = arg("--blocks", 20000.0) as usize;
    let rate = arg("--rate", 48000.0);
    let budget = fm6::N as f64 / rate * 1e9;
    println!("soundbench: block {} samples at {} Hz = {:.0} ns budget; {} blocks x 5 runs, median", fm6::N, rate, budget, blocks);
    println!("{:<34} {:>10} {:>10} {:>9}", "case", "ns/block", "of budget", "ns/voice");
    let row = |name: &str, ns: f64, voices: usize| {
        println!("{:<34} {:>10.0} {:>9.2}% {:>9.0}", name, ns, ns / budget * 100.0, ns / voices.max(1) as f64);
    };

    // fm6: one block per call (render_block), voices held
    for (alg, label) in [(21u8, "alg 22 (3 carriers)"), (31, "alg 32 (6 carriers)"), (0, "alg 1 (2 carriers)")] {
        for n in [0usize, 1, 6, 12] {
            let mut s = fm6::Synth::new(rate, &voice(alg)).expect("synth");
            for k in 0..n {
                s.note_on(48 + 3 * k as u8, 100);
            }
            let live = s.live_voices();
            let ns = time(blocks, || {
                std::hint::black_box(s.render_block());
            });
            row(&format!("fm6 {label}, {n} held"), ns, if n == 0 { 1 } else { live });
        }
    }
    {
        let mut s = fm6::Synth::new(rate, &voice(21)).expect("synth");
        for k in 0..12 {
            s.note_on(48 + 3 * k as u8, 100);
        }
        s.set_tuning(fm6::Tuning::Stock);
        s.set_output(fm6::Output::Stock { pot: 900 });
        let ns = time(blocks, || {
            std::hint::black_box(s.render_block());
        });
        row("fm6 alg 22 Stock tuning, 12 held", ns, 12);
    }

    // sampler: one 64-sample render per call
    for (interp, label) in [(sampler::Interp::Nearest, "nearest"), (sampler::Interp::Linear, "linear"), (sampler::Interp::Cubic, "cubic")] {
        for n in [1usize, 8, 16] {
            let smp = tone(22050, 2).with_loop(1000, 40000);
            let mut s = sampler::Sampler::new(smp, rate, 60).expect("sampler");
            s.set_interp(interp);
            s.set_voices(n);
            for k in 0..n {
                s.note_on(50 + 2 * k as u8, 100);
            }
            let mut out = [0i16; fm6::N];
            let live = s.sounding();
            let ns = time(blocks, || {
                s.render(&mut out);
                std::hint::black_box(&out);
            });
            row(&format!("sampler {label}, {n} held (sounding {live})"), ns, live);
        }
    }

    // kit: eight drums, all sounding, with the limiter on (the X0X setting)
    for n in [1usize, 4, 8] {
        let mut k = sampler::Kit::new(rate).expect("kit");
        for d in 0..n {
            k.add(36 + d as u8, tone(22050, 160), &[]).expect("add");
        }
        k.set_ceiling(Some(0.4), 0.05);
        for d in 0..n {
            k.note_on(36 + d as u8, 110);
        }
        let mut out = [0i16; fm6::N];
        let live = k.sounding();
        let ns = time(blocks, || {
            k.render(&mut out);
            std::hint::black_box(&out);
        });
        let still = k.sounding();
        row(&format!("kit, {n} drums (sounding {live} -> {still} after)"), ns, live);
    }
    // analog: eight held notes, and with the effect chain (chorus, delay, reverb, limiter) over twelve fm6-sized blocks of work
    for n in [1usize, 4, 8] {
        let mut e = fm1ntt::analog::Analog::new(rate);
        for k in 0..n {
            e.note_on(40 + 3 * k as u8, 100);
        }
        let mut out = [0i16; fm6::N];
        let live = e.sounding();
        let ns = time(blocks, || {
            e.render(&mut out);
            std::hint::black_box(&out);
        });
        row(&format!("analog, {n} held (sounding {live})"), ns, live);
    }
    for name in ["limiter", "delay", "lowpass", "chorus", "reverb"] {
        let mut f = fx::make(name, rate).expect("fx");
        let mut out = [0i16; fm6::N];
        for (i, o) in out.iter_mut().enumerate() {
            *o = ((i as f64 * 0.2).sin() * 8000.0) as i16;
        }
        let ns = time(blocks, || {
            f.process(&mut out);
            std::hint::black_box(&out);
        });
        row(&format!("effect: {name}"), ns, 1);
    }
}
