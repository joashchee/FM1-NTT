//! Fuzzes the readers a person can hand a file or a buffer to (rule 5: untrusted input is an `Err`, never a panic, a hang or unbounded memory):
//! the catalog JSON, WAV and IMA ADPCM, the `.fm1i` instrument package (parsed, built into an instrument and played), the UI description, the
//! firmware extractor and port identifier (on mutated packages and random bytes), and flash restore. Each target builds its input from a seed, so a
//! failing seed replays; a case that takes over two seconds is listed as slow (a hang is not interrupted: this only sees it afterwards).
//!   CARGO_PROFILE_RELEASE_PANIC=unwind cargo run --release --example fuzzreaders -- [CASES] [FIRST_SEED] [TARGET]
//! TARGET is one of catalog wav adpcm package ui extract flash, or all. Reads `../bundled-firmware/` samples when present, writes nothing.
use fm1ntt::{catalog, extract, machine::Machine, package::Package, port, sampler};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use Relaxed as R;
use std::alloc::{GlobalAlloc, Layout, System};
use std::time::Instant;

/// Counts live heap bytes and their peak (std only), so a case that asks for
/// a huge buffer shows up: rule 5 says bounded memory, not only no panic.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), R) + l.size();
            PEAK.fetch_max(now, R);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        LIVE.fetch_sub(l.size(), R);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = System.realloc(p, l, n);
        if !q.is_null() {
            let now = if n >= l.size() { LIVE.fetch_add(n - l.size(), R) + (n - l.size()) } else { LIVE.fetch_sub(l.size() - n, R) - (l.size() - n) };
            PEAK.fetch_max(now, R);
        }
        q
    }
}
#[global_allocator]
static A: Counting = Counting;
/// A case may hold at most this much heap above what it started with.
const MEM_LIMIT: usize = 256 << 20;

/// How many cases got past the first refusal: a fuzz that only meets early errors tests little.
static WAV_OK: AtomicUsize = AtomicUsize::new(0);
static PKG_OK: AtomicUsize = AtomicUsize::new(0);
static INST_OK: AtomicUsize = AtomicUsize::new(0);
static UI_OK: AtomicUsize = AtomicUsize::new(0);
static EXTRACT_OK: AtomicUsize = AtomicUsize::new(0);
static FLASH_OK: AtomicUsize = AtomicUsize::new(0);
static CAT_OK: AtomicUsize = AtomicUsize::new(0);

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03 | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            self.next() as usize % n
        }
    }
}

/// Mutates bytes: flips, overwritten runs, deleted and duplicated spans, truncation, a few interesting values.
fn mutate(src: &[u8], r: &mut Rng) -> Vec<u8> {
    let mut v = src.to_vec();
    for _ in 0..1 + r.below(6) {
        if v.is_empty() {
            break;
        }
        let at = r.below(v.len());
        match r.below(7) {
            0 => v[at] ^= 1 << r.below(8),
            1 => v[at] = r.next() as u8,
            2 => {
                let n = 1 + r.below(16);
                for i in 0..n {
                    if let Some(b) = v.get_mut(at + i) {
                        *b = [0x00, 0xFF, 0x7F, 0x80, b'"', b'{', b'}', b'[', b']', b'0', b'9', b'-', b'\n', b','][r.below(14)];
                    }
                }
            }
            3 => {
                let n = (1 + r.below(32)).min(v.len() - at);
                v.drain(at..at + n);
            }
            4 => {
                let n = (1 + r.below(64)).min(v.len() - at);
                let chunk = v[at..at + n].to_vec();
                v.splice(at..at, chunk);
            }
            5 => v.truncate(at),
            _ => {
                let w = [0u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF, 0x1000_0000][r.below(6)];
                for (i, b) in w.to_le_bytes().iter().enumerate() {
                    if let Some(d) = v.get_mut(at + i) {
                        *d = *b;
                    }
                }
            }
        }
    }
    v
}

fn random_bytes(r: &mut Rng, max: usize) -> Vec<u8> {
    (0..r.below(max + 1)).map(|_| r.next() as u8).collect()
}

const GOOD_JSON: &str = r#"{
  "format": "fm1tastik/1", "claim": { "code": "k7Q2-M9XV-4HNE" }, "listed": true, "x-private": [1, 2, {"a": null}],
  "contact": { "issues": "https://github.com/example/zither/issues", "email": "maker@example.org", "discord": "https://discord.gg/xxxxxxx" },
  "firmware": [ {
    "id": "zither", "name": "ZITHER", "summary": "A plucked-string synth with a four-track sequencer.",
    "description": "Karplus-Strong strings.", "author": { "name": "A. Maker", "url": "https://example.org" },
    "homepage": "https://example.org/zither", "source": "https://github.com/example/zither",
    "categories": ["synth", "sequencer", "weird"], "tags": ["plucked", "poly"],
    "hardware": { "fm1": "any", "flash": ["P25Q80"] },
    "licence": { "spdx": "GPL-3.0-only AND MIT", "files": ["LICENSE", "LICENSES/MIT.txt"],
                 "third_party": [ { "what": "preset names", "rights": "granted", "note": "by the author" } ], "notes": "x" },
    "needs": { "assets": false }, "unknown_key": 1,
    "releases": [ { "version": "1.2.0", "date": "2026-11-10", "url": "https://github.com/example/zither/releases/download/v1.2.0/ZITHER.fwsc",
      "sha256": "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824", "size": 5, "notes": "n", "deprecated": false } ],
    "catalog": { "thumbnail": false }
  } ] }"#;

fn catalog_case(seed: u64) {
    let mut r = Rng::new(seed);
    let input = match r.below(5) {
        0 => random_bytes(&mut r, 400),
        1 => {
            // deep nesting, then a mutated good file
            let d = 1 + r.below(2000);
            let mut v = "[".repeat(d).into_bytes();
            v.extend(b"1");
            v.extend("]".repeat(r.below(d + 3)).into_bytes());
            v
        }
        _ => mutate(GOOD_JSON.as_bytes(), &mut r),
    };
    CAT_OK.fetch_add(usize::from(catalog::Catalog::parse(&input).is_ok()), Relaxed);
    let _ = catalog::parse_json(&input);
}

/// A small valid WAV: PCM16 or IMA ADPCM, mono or stereo, with an optional loop chunk.
fn wav(r: &mut Rng) -> Vec<u8> {
    let adpcm = r.below(2) == 0;
    let ch = 1 + r.below(2) as u16;
    let rate = [8000u32, 22050, 44100, 48000][r.below(4)];
    let n = 64 + r.below(512);
    let samples: Vec<i16> = (0..n * ch as usize).map(|i| ((i as f64 * 0.1).sin() * 12000.0) as i16).collect();
    let (fmt_tag, align, data) = if adpcm {
        (0x11u16, 256 * ch, sampler::encode_ima_blocks(&samples[..samples.len() / ch as usize], 256))
    } else {
        (1u16, 2 * ch, samples.iter().flat_map(|s| s.to_le_bytes()).collect())
    };
    let mut fmt = Vec::new();
    fmt.extend(fmt_tag.to_le_bytes());
    fmt.extend(ch.to_le_bytes());
    fmt.extend(rate.to_le_bytes());
    fmt.extend((rate * u32::from(align)).to_le_bytes());
    fmt.extend(align.to_le_bytes());
    fmt.extend(if adpcm { 4u16 } else { 16u16 }.to_le_bytes());
    let mut body = b"WAVE".to_vec();
    for (id, d) in [(&b"fmt "[..], fmt), (&b"data"[..], data)] {
        body.extend(id);
        body.extend((d.len() as u32).to_le_bytes());
        body.extend(&d);
        if d.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut v = b"RIFF".to_vec();
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(body);
    v
}

fn wav_case(seed: u64) {
    let mut r = Rng::new(seed);
    let input = if r.below(6) == 0 { random_bytes(&mut r, 300) } else { mutate(&wav(&mut r), &mut r) };
    if let Ok(s) = sampler::parse_wav(&input) {
        WAV_OK.fetch_add(1, Relaxed);
        if let Ok(mut sm) = sampler::Sampler::new(s, 44100.0, r.below(128) as u8) {
            sm.note_on(r.below(128) as u8, r.below(128) as u8);
            let mut out = vec![0i16; 256];
            for _ in 0..4 {
                sm.render(&mut out);
            }
            sm.note_off(60);
            sm.render(&mut out);
        }
    }
}

fn adpcm_case(seed: u64) {
    let mut r = Rng::new(seed);
    let data = random_bytes(&mut r, 2048);
    let _ = sampler::decode_ima_blocks(&data, [0, 1, 3, 4, 5, 8, 100, 256, 512, 1024, 2048, 4096][r.below(12)]);
    let _ = sampler::decode_ima_blocks(&data, r.below(5000));
}

fn package_text() -> Option<String> {
    std::fs::read_to_string("../bundled-firmware/instruments/factory-voices.fm1i").ok()
}

fn package_case(seed: u64, base: &str) {
    let mut r = Rng::new(seed);
    let mut text = if r.below(8) == 0 {
        String::from_utf8_lossy(&random_bytes(&mut r, 300)).into_owned()
    } else {
        // line-level: delete, duplicate, shuffle, mutate numbers or words in a line; then byte-level on the whole
        let mut lines: Vec<String> = base.lines().map(str::to_string).collect();
        for _ in 0..r.below(5) {
            if lines.is_empty() {
                break;
            }
            let i = r.below(lines.len());
            match r.below(4) {
                0 => {
                    lines.remove(i);
                }
                1 => lines.insert(i, lines[i].clone()),
                2 => {
                    let j = r.below(lines.len());
                    lines.swap(i, j);
                }
                _ => {
                    let w = ["-1", "999999999", "0", "nan", "1e30", "", "{", "\"", "fm6", "fm4", "sampler", "analog", "drum", "kit", "255", "256", "4294967296"][r.below(17)];
                    let at = lines[i].find(" = ").map_or(0, |p| p + 3);
                    lines[i].truncate(at);
                    lines[i].push_str(w);
                }
            }
        }
        lines.join("\n")
    };
    if r.below(2) == 0 {
        text = String::from_utf8_lossy(&mutate(text.as_bytes(), &mut r)).into_owned();
    }
    let Ok(pkg) = Package::parse(&text) else { return };
    PKG_OK.fetch_add(1, Relaxed);
    let _ = Package::parse(&pkg.to_text()); // the round trip
    let wavs = std::cell::RefCell::new(Rng::new(seed ^ 0x55));
    let resolver = |_: &str| -> Option<Vec<u8>> {
        let mut w = wavs.borrow_mut();
        match w.below(3) {
            0 => None,
            1 => Some(wav(&mut w)),
            _ => Some(random_bytes(&mut w, 600)),
        }
    };
    let Ok(mut inst) = fm1ntt::instrument::Instrument::from_package(&pkg, 44100.0, &resolver) else { return };
    INST_OK.fetch_add(1, Relaxed);
    let mut out = vec![0i16; 512];
    inst.note_on(r.below(128) as u8, r.below(128) as u8);
    inst.midi_bytes(&[0xB0, r.below(128) as u8, r.below(128) as u8]);
    inst.midi_bytes(&[0xC0, r.below(128) as u8]);
    inst.midi_bytes(&random_bytes(&mut r, 12));
    inst.render(&mut out);
    for p in inst.params().into_iter().take(40) {
        let _ = inst.get_param(p.id);
        let v = [p.min, p.max, p.min.wrapping_sub(1), p.max.wrapping_add(1), i32::MIN, i32::MAX, 0][r.below(7)];
        let _ = inst.set_param(p.id, v);
    }
    for i in 0..inst.fx_count().min(8) {
        for p in inst.fx_params(i).into_iter().take(20) {
            let _ = inst.set_fx_param(i, p.id, [p.min, p.max, i32::MIN, i32::MAX][r.below(4)]);
        }
        let _ = inst.move_fx(i, r.below(12));
    }
    for g in inst.global_params().into_iter().take(20) {
        let _ = inst.set_global(g.id, [g.min, g.max, i32::MIN, i32::MAX][r.below(4)]);
    }
    let _ = inst.set_bank(r.below(8));
    inst.render(&mut out);
    inst.note_on(r.below(128) as u8, 127);
    inst.render(&mut out);
}

/// The directives from the module's own grammar (the text after `ui = `), a valid file for the mutations to start from.
const UI_GOOD: [&str; 13] = [
    "theme bg=#1a0a12 fg=#f5e6e8 dim=#8a6a72 accent=#ff7f66",
    "color drums=#ff6b5a bass=#a78bfa",
    "page home \"HOME\"",
    "page song \"SONG\"",
    "page mixer \"MIXER\"",
    "text home 10 20 \"SLOW BLOOM\" size=2 color=fg bind=state:title",
    "value song 8 40 \"TEMPO\" bind=param:12 min=40 max=200 unit=\"BPM\" bar=1",
    "fader mixer 20 30 h=100 bind=param:20 color=drums label=\"DRUMS\"",
    "meter mixer 20 140 w=8 h=40 bind=state:level0 color=drums",
    "grid song 30 40 w=160 h=100 cols=32 rows=7 bind=state:pattern0 color=accent",
    "disc home 20 120 r=60 bind=state:beat color=accent",
    "bar home 8 200 w=120 bind=state:progress color=accent",
    "key HOME page=home",
];

fn ui_case(seed: u64, _base: &str) {
    let mut r = Rng::new(seed);
    let mut lines: Vec<String> = UI_GOOD.iter().map(|s| s.to_string()).collect();
    if r.below(2) == 0 {
        lines.push("hint song \"K1-K4 CHANGE  HOME MAIN\"".into());
        lines.push("enc KNOB1 page=song bind=param:12 step=1".into());
    }
    // most cases change only one line, so the rest still parses and the drawing code is reached
    let ls: Vec<String> = if r.below(4) == 0 {
        String::from_utf8_lossy(&mutate(lines.join("\n").as_bytes(), &mut r)).lines().map(str::to_string).collect()
    } else {
        let i = r.below(lines.len());
        lines[i] = String::from_utf8_lossy(&mutate(lines[i].as_bytes(), &mut r)).replace('\n', " ");
        lines
    };
    if let Ok(ui) = fm1ntt::ui::Ui::parse(&ls) {
        UI_OK.fetch_add(1, Relaxed);
        let state = std::cell::Cell::new(seed | 1);
        let value = |_: &_| {
            let v = state.get().wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            state.set(v);
            Some((v >> 33) as i32 - (1 << 30))
        };
        for page in 0..4 {
            let _ = ui.draw(page, &value, &|_| Some("v".to_string()));
        }
    }
}

fn extract_case(seed: u64, base: &[u8]) {
    let mut r = Rng::new(seed);
    let bytes = match r.below(4) {
        0 => random_bytes(&mut r, 5000),
        1 => mutate(&base[..base.len().min(2000)], &mut r),
        _ => mutate(base, &mut r),
    };
    let min = 1 + r.below(8);
    EXTRACT_OK.fetch_add(usize::from(extract::report(&bytes, min).is_ok()), Relaxed);
    let _ = extract::voices(&bytes, min);
    let _ = extract::app_image(&bytes);
    let _ = port::identify(&bytes);
    let _ = extract::find_strings(&bytes, 1 + r.below(10));
    let _ = extract::find_banks(&bytes, "x", min);
    let _ = extract::find_unpacked(&bytes, min, [1, 2, 4, 16, 64, 128][r.below(6)]);
}

fn flash_case(seed: u64) {
    let mut r = Rng::new(seed);
    let Ok(mut m) = Machine::from_raw(vec![0x11; 4096], 0x0200_0120) else { return };
    let n = match r.below(4) {
        0 => r.below(2 << 20),
        1 => (1 << 20) + r.below(9) - 4,
        2 => r.below(8192),
        _ => 1 << 20,
    };
    let bytes: Vec<u8> = (0..n).map(|_| if r.below(3) == 0 { r.next() as u8 } else { 0xFF }).collect();
    FLASH_OK.fetch_add(usize::from(m.restore_flash(&bytes).is_ok()), Relaxed);
    let mut out = vec![0i16; 128];
    m.render(&mut out, 20_000);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cases: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let first: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let which = a.get(3).map_or("all", String::as_str);
    let last = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    {
        let last = last.clone();
        std::panic::set_hook(Box::new(move |info| {
            let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
            *last.lock().unwrap() = format!("{loc}: {msg}");
        }));
    }
    let pkg_text = package_text().unwrap_or_default();
    let fw = std::fs::read("../bundled-firmware/haunted-fm1/untrue-0.1.fwsc").unwrap_or_default();
    let mut total_panics = 0;
    for target in ["catalog", "wav", "adpcm", "package", "ui", "extract", "flash"] {
        if which != "all" && which != target {
            continue;
        }
        if (target == "package" || target == "ui") && pkg_text.is_empty() || target == "extract" && fw.is_empty() {
            println!("{target}: skipped (the bundled sample is not here)");
            continue;
        }
        let mut found: std::collections::BTreeMap<String, (u64, usize)> = Default::default();
        let (mut slow, mut worst) = (0, (0.0f64, 0u64));
        let (mut mem_worst, mut mem_over) = ((0usize, 0u64), 0usize);
        for seed in first..first + cases {
            let base_live = LIVE.load(R);
            PEAK.store(base_live, R);
            let t = Instant::now();
            let res = catch_unwind(AssertUnwindSafe(|| match target {
                "catalog" => catalog_case(seed),
                "wav" => wav_case(seed),
                "adpcm" => adpcm_case(seed),
                "package" => package_case(seed, &pkg_text),
                "ui" => ui_case(seed, &pkg_text),
                "extract" => extract_case(seed, &fw),
                _ => flash_case(seed),
            }));
            let el = t.elapsed().as_secs_f64();
            if el > worst.0 {
                worst = (el, seed);
            }
            slow += usize::from(el > 2.0);
            let used = PEAK.load(R).saturating_sub(base_live);
            if used > mem_worst.0 {
                mem_worst = (used, seed);
            }
            mem_over += usize::from(used > MEM_LIMIT);
            if res.is_err() {
                let why = last.lock().unwrap().clone();
                found.entry(why).or_insert((seed, 0)).1 += 1;
            }
        }
        total_panics += found.len();
        let deep = match target {
            "catalog" => format!("{} accepted", CAT_OK.load(Relaxed)),
            "wav" => format!("{} parsed as WAV and played", WAV_OK.load(Relaxed)),
            "package" => format!("{} parsed, {} built into an instrument", PKG_OK.load(Relaxed), INST_OK.load(Relaxed)),
            "ui" => format!("{} parsed and drawn", UI_OK.load(Relaxed)),
            "extract" => format!("{} reported", EXTRACT_OK.load(Relaxed)),
            "flash" => format!("{} restored", FLASH_OK.load(Relaxed)),
            _ => String::new(),
        };
        println!("{target:8} {cases} cases from seed {first}: {} distinct panic site(s); {deep}; slowest {:.3}s (seed {}); over 2 s: {slow}; peak heap {:.1} MiB (seed {}); over {} MiB: {mem_over}", found.len(), worst.0, worst.1, mem_worst.0 as f64 / 1048576.0, mem_worst.1, MEM_LIMIT >> 20);
        total_panics += mem_over;
        for (why, (seed, n)) in &found {
            println!("    {n:5} x  {why}   (first: seed {seed})");
        }
    }
    println!("{}", if total_panics == 0 { "no panic" } else { "PANICS FOUND" });
}
