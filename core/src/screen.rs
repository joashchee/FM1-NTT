//! Reading a firmware's screen (docs/PORTING-UI.md, the porter's OBSERVE-SCREEN and VERIFY-UI steps): small, dependency-free picture tools over the FM-1's
//! 240 x 240 frame (0xXXRRGGBB), used by `examples/uiport.rs` and by tests.
//!
//! Nothing here knows any firmware. It finds what a frame is made of: its background and palette, the blocks of ink on it, the bands of text, what changed
//! between two frames; it compares a frame with a package's drawn layout (`ui::rasterize`); and it writes a PNG so a person can look.

pub const W: usize = 240;
pub const H: usize = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub fn area(&self) -> usize {
        self.w * self.h
    }
}

fn rgb(p: u32) -> u32 {
    p & 0x00ff_ffff
}

/// A hash of the frame (FNV-1a over the pixels), to tell one screen from another.
pub fn hash(frame: &[u32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &p in frame {
        h = (h ^ u64::from(rgb(p))).wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// The colours of the frame, most used first, at most `n`: `(colour, pixel count)`.
pub fn palette(frame: &[u32], n: usize) -> Vec<(u32, usize)> {
    let mut v: Vec<(u32, usize)> = Vec::new();
    let mut sorted: Vec<u32> = frame.iter().map(|&p| rgb(p)).collect();
    sorted.sort_unstable();
    for c in sorted {
        match v.last_mut() {
            Some(l) if l.0 == c => l.1 += 1,
            _ => v.push((c, 1)),
        }
    }
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// The background: the most used colour.
pub fn background(frame: &[u32]) -> u32 {
    palette(frame, 1).first().map_or(0, |c| c.0)
}

/// The box around every pixel that differs between two frames, and how many do; `None` when they are the same.
pub fn diff(a: &[u32], b: &[u32]) -> Option<(Rect, usize)> {
    let (mut x0, mut y0, mut x1, mut y1, mut n) = (W, H, 0, 0, 0);
    for i in 0..a.len().min(b.len()).min(W * H) {
        if rgb(a[i]) != rgb(b[i]) {
            let (x, y) = (i % W, i / W);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            n += 1;
        }
    }
    (n > 0).then_some((Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }, n))
}

/// A block of connected ink (4-neighbour) of one colour: its box, pixel count and colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blob {
    pub rect: Rect,
    pub pixels: usize,
    pub color: u32,
}

/// The connected single-colour blocks that are not `bg`, at least `min` pixels, biggest first, at most `limit` of them. A flood fill with its own stack: bounded by
/// the frame, no recursion.
pub fn blobs(frame: &[u32], bg: u32, min: usize, limit: usize) -> Vec<Blob> {
    let total = frame.len().min(W * H);
    let mut seen = vec![false; total];
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..total {
        if seen[start] || rgb(frame[start]) == rgb(bg) {
            continue;
        }
        let c = rgb(frame[start]);
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (W, H, 0, 0, 0);
        stack.push(start);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % W, i / W);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            n += 1;
            let mut visit = |j: usize| {
                if !seen[j] && rgb(frame[j]) == c {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < W && i + 1 < total {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - W);
            }
            if i + W < total {
                visit(i + W);
            }
        }
        if n >= min {
            out.push(Blob { rect: Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }, pixels: n, color: c });
        }
    }
    out.sort_by(|a, b| b.pixels.cmp(&a.pixels).then(a.rect.y.cmp(&b.rect.y)).then(a.rect.x.cmp(&b.rect.x)));
    out.truncate(limit);
    out
}

/// The horizontal bands that hold ink (rows with at least one non-`bg` pixel, joined across gaps of up to `gap` empty rows): `(first row, last row)`. Text lines
/// and widgets stack in bands, so this is where a layout's rows are.
pub fn bands(frame: &[u32], bg: u32, gap: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut empty = 0;
    for y in 0..H.min(frame.len() / W) {
        let ink = (0..W).any(|x| rgb(frame[y * W + x]) != rgb(bg));
        if ink {
            match out.last_mut() {
                Some(l) if empty <= gap && l.1 + 1 + empty >= y => l.1 = y,
                _ => out.push((y, y)),
            }
            empty = 0;
        } else {
            empty += 1;
        }
    }
    out
}

/// How well a drawn layout (`ui::rasterize`, 0xRRGGBB) agrees with a captured frame, as the overlap of what is ink in each (not the background colour):
/// intersection over union, 0 to 1, and the share of the layout's ink that sits on ink in the frame. A screen drawn right scores near 1; text drawn as solid
/// cells against real glyphs scores lower, so a layout is judged by thresholds measured per firmware, not by 1.
pub fn coverage_agreement(frame: &[u32], layout: &[u32], bg_frame: u32, bg_layout: u32) -> (f64, f64) {
    let (mut inter, mut uni, mut lay) = (0usize, 0usize, 0usize);
    for i in 0..frame.len().min(layout.len()).min(W * H) {
        let (a, b) = (rgb(frame[i]) != rgb(bg_frame), rgb(layout[i]) != rgb(bg_layout));
        inter += usize::from(a && b);
        uni += usize::from(a || b);
        lay += usize::from(b);
    }
    (if uni == 0 { 1.0 } else { inter as f64 / uni as f64 }, if lay == 0 { 1.0 } else { inter as f64 / lay as f64 })
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// A frame as a PNG (stored deflate blocks, no compression: small enough, and no dependency), scaled up `scale` times.
pub fn png(frame: &[u32], scale: usize) -> Vec<u8> {
    let scale = scale.clamp(1, 8);
    let mut raw = Vec::with_capacity((W * scale * 3 + 1) * H * scale);
    for y in 0..H * scale {
        raw.push(0);
        for x in 0..W * scale {
            let p = frame.get((y / scale) * W + x / scale).copied().unwrap_or(0);
            raw.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        z.extend((c.len() as u16).to_le_bytes());
        z.extend((!(c.len() as u16)).to_le_bytes());
        z.extend(*c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        let mut c = kind.to_vec();
        c.extend(data);
        out.extend(crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend(((W * scale) as u32).to_be_bytes());
    ihdr.extend(((H * scale) as u32).to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bg: u32) -> Vec<u32> {
        vec![bg; W * H]
    }
    fn fill(f: &mut [u32], r: Rect, c: u32) {
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                f[y * W + x] = c;
            }
        }
    }

    #[test]
    fn palette_background_diff_blobs_and_bands() {
        let mut a = frame(0x101010);
        fill(&mut a, Rect { x: 10, y: 20, w: 30, h: 5 }, 0xff0000);
        fill(&mut a, Rect { x: 100, y: 22, w: 4, h: 4 }, 0x00ff00);
        fill(&mut a, Rect { x: 10, y: 100, w: 50, h: 2 }, 0x0000ff);
        assert_eq!(background(&a), 0x101010);
        let p = palette(&a, 3);
        assert_eq!(p[0].0, 0x101010);
        assert_eq!(p[1], (0xff0000, 150));
        let b = blobs(&a, 0x101010, 10, 10);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0], Blob { rect: Rect { x: 10, y: 20, w: 30, h: 5 }, pixels: 150, color: 0xff0000 });
        assert_eq!(blobs(&a, 0x101010, 101, 10).len(), 1);
        assert_eq!(blobs(&a, 0x101010, 1, 2).len(), 2);
        // two rows of ink with a gap: two bands; a wide `gap` joins them
        assert_eq!(bands(&a, 0x101010, 3), vec![(20, 25), (100, 101)]);
        assert_eq!(bands(&a, 0x101010, 100).len(), 1);
        // the same frame: no diff; a changed corner: its box
        assert_eq!(diff(&a, &a), None);
        let mut c = a.clone();
        fill(&mut c, Rect { x: 200, y: 200, w: 3, h: 2 }, 0xffffff);
        assert_eq!(diff(&a, &c), Some((Rect { x: 200, y: 200, w: 3, h: 2 }, 6)));
        assert_ne!(hash(&a), hash(&c));
        assert_eq!(hash(&a), hash(&a.clone()));
        // the alpha byte is ignored
        let d: Vec<u32> = a.iter().map(|p| p | 0xff00_0000).collect();
        assert_eq!(hash(&a), hash(&d));
        // a solid frame has no blobs and no bands
        let s = frame(0x202020);
        assert!(blobs(&s, 0x202020, 1, 5).is_empty() && bands(&s, 0x202020, 2).is_empty());
    }

    #[test]
    fn coverage_agreement_scores_a_matching_layout_high() {
        let mut shot = frame(0x000000);
        fill(&mut shot, Rect { x: 20, y: 20, w: 100, h: 40 }, 0xffffff);
        let mut same = frame(0x111111);
        fill(&mut same, Rect { x: 20, y: 20, w: 100, h: 40 }, 0xff8800);
        assert_eq!(coverage_agreement(&shot, &same, 0x000000, 0x111111), (1.0, 1.0));
        let mut off = frame(0x111111);
        fill(&mut off, Rect { x: 70, y: 20, w: 100, h: 40 }, 0xff8800);
        let (iou, on) = coverage_agreement(&shot, &off, 0x000000, 0x111111);
        assert!((iou - 50.0 * 40.0 / (150.0 * 40.0)).abs() < 1e-9 && (on - 0.5).abs() < 1e-9);
        let empty = frame(0x111111);
        assert_eq!(coverage_agreement(&frame(0), &empty, 0, 0x111111), (1.0, 1.0));
    }

    #[test]
    fn png_has_the_right_signature_and_size() {
        let f = frame(0x336699);
        let p = png(&f, 2);
        assert_eq!(&p[..8], &[0x89, b'P', b'N', b'G', 13, 10, 26, 10]);
        assert_eq!(u32::from_be_bytes([p[16], p[17], p[18], p[19]]), 480);
        assert_eq!(u32::from_be_bytes([p[20], p[21], p[22], p[23]]), 480);
        assert!(p.ends_with(&[0xae, 0x42, 0x60, 0x82]));
    }
}
