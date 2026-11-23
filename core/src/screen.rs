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
    (n > 0).then(|| (Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }, n))
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

/// What `detect` calls a thing on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// a run of glyph-sized marks on one line (`glyphs` of them, `size` times the 5 x 7 font)
    Text { glyphs: usize, size: usize },
    /// a filled rectangle at least three times as wide as high (a bar, the fill of a value row)
    Bar,
    /// a filled rectangle at least three times as high as wide (a meter, a fader's track)
    Meter,
    /// a filled disc
    Disc,
    /// equal small cells on a lattice (`cols` x `rows`, some may be unlit)
    Grid { cols: usize, rows: usize },
    /// any other block of ink (a frame, a picture)
    Box,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detected {
    pub shape: Shape,
    pub rect: Rect,
    /// the main colour of the thing (the most common among its marks)
    pub color: u32,
}

const GLYPH_MAX_W: usize = 18;
const GLYPH_MAX_H: usize = 24;

/// Finds the things a frame is made of, from its single-colour blocks (`blobs`): **discs** (a roughly square block that is three quarters filled), **bars** and **meters**
/// (solid blocks three times longer than they are thick, wide or tall), **grids** (eight or more equal cells on a lattice), **text** (glyph-sized marks along one line, joined
/// across a gap of about a cell), and other **boxes**. The thresholds are for the FM-1's 5 x 7 (and scaled) fonts and are not fitted to any firmware: a flat colour
/// block is judged by shape alone, so a one-cell-thick glyph such as `I` or `-` inside a word is part of the line, and a lone one is a bar. At most 256 things, top to bottom.
pub fn detect(frame: &[u32], bg: u32) -> Vec<Detected> {
    let total = frame.len().min(W * H);
    let bgc = rgb(bg);
    let ink = |i: usize| rgb(frame[i]) != bgc;
    // single-colour components with their pixels (4-neighbour), for the shapes
    let mut label = vec![u32::MAX; total];
    let mut comps: Vec<(Blob, Vec<usize>)> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..total {
        if label[start] != u32::MAX || !ink(start) {
            continue;
        }
        let c = rgb(frame[start]);
        let id = comps.len() as u32;
        let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0, 0);
        let mut px = Vec::new();
        stack.push(start);
        label[start] = id;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % W, i / W);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            px.push(i);
            let mut visit = |j: usize| {
                if label[j] == u32::MAX && rgb(frame[j]) == c {
                    label[j] = id;
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
        comps.push((Blob { rect: Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }, pixels: px.len(), color: c }, px));
        if comps.len() > 20_000 {
            break;
        }
    }
    let mut out: Vec<Detected> = Vec::new();
    let mut taken = vec![false; total];
    fn claim(taken: &mut [bool], px: &[usize]) {
        for &i in px {
            taken[i] = true;
        }
    }
    // solid, elongated or round things first
    for (b, px) in comps.iter().filter(|(b, _)| b.pixels >= 2) {
        let (w, h) = (b.rect.w, b.rect.h);
        let fill = b.pixels as f64 / b.rect.area() as f64;
        let shape = if w >= 9 && h >= 9 && w.abs_diff(h) <= (w / 8).max(2) && (0.55..=0.88).contains(&fill) {
            Some(Shape::Disc)
        } else if fill >= 0.9 && w >= 3 * h && h <= 14 && w > GLYPH_MAX_W {
            Some(Shape::Bar)
        } else if fill >= 0.9 && h >= 3 * w && w <= 14 && h > GLYPH_MAX_H {
            Some(Shape::Meter)
        } else if w > GLYPH_MAX_W || h > GLYPH_MAX_H {
            Some(Shape::Box)
        } else {
            None
        };
        if let Some(shape) = shape {
            claim(&mut taken, px);
            out.push(Detected { shape, rect: b.rect, color: b.color });
        }
    }
    // a lattice of equal cells (single-colour solid cells that are not text-shaped)
    let mut by_size: std::collections::HashMap<(usize, usize), Vec<usize>> = std::collections::HashMap::new();
    for (i, (b, px)) in comps.iter().enumerate() {
        let fill = b.pixels as f64 / b.rect.area() as f64;
        if !px.iter().any(|&p| taken[p]) && b.rect.w >= 3 && b.rect.h >= 3 && fill >= 0.9 && b.rect.w <= GLYPH_MAX_W && b.rect.h <= GLYPH_MAX_H {
            by_size.entry((b.rect.w, b.rect.h)).or_default().push(i);
        }
    }
    for ((w, h), idx) in by_size {
        // solid cells 5 : 7 are the model's text (`ui::rasterize` draws glyphs as such cells), not a lattice
        let aspect = w as f64 / h as f64;
        if idx.len() < 8 || (0.65..=0.78).contains(&aspect) {
            continue;
        }
        let mut xs: Vec<usize> = idx.iter().map(|&i| comps[i].0.rect.x).collect();
        let mut ys: Vec<usize> = idx.iter().map(|&i| comps[i].0.rect.y).collect();
        xs.sort_unstable();
        xs.dedup();
        ys.sort_unstable();
        ys.dedup();
        // the cells sit on a lattice when a few distinct columns and rows hold most of them
        if xs.len() * ys.len() > idx.len() * 2 || xs.len() > 64 || ys.len() > 64 || (ys.len() == 1 && xs.len() < 8) {
            continue;
        }
        let (x0, y0) = (xs[0], ys[0]);
        let (x1, y1) = (xs[xs.len() - 1] + w, ys[ys.len() - 1] + h);
        let colour = comps[idx[0]].0.color;
        for &i in &idx {
            claim(&mut taken, &comps[i].1);
        }
        out.push(Detected { shape: Shape::Grid { cols: xs.len(), rows: ys.len() }, rect: Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, color: colour });
    }
    // text: what ink is left, as 8-connected marks of any colour (a glyph with anti-aliasing is several colours), glyph-sized, along one line
    let mut seen = vec![false; total];
    let mut marks: Vec<(Rect, u32)> = Vec::new();
    for start in 0..total {
        if seen[start] || taken[start] || !ink(start) {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0, 0);
        let mut cols: Vec<u32> = Vec::new();
        stack.push(start);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % W, i / W);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            cols.push(rgb(frame[i]));
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if (0..W as i32).contains(&nx) && (0..H as i32).contains(&ny) {
                        let j = ny as usize * W + nx as usize;
                        if j < total && !seen[j] && !taken[j] && ink(j) {
                            seen[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        if cols.len() < 2 {
            continue;
        }
        cols.sort_unstable();
        let (mut best, mut run) = ((0usize, cols[0]), 0usize);
        for k in 0..cols.len() {
            run = if k > 0 && cols[k] == cols[k - 1] { run + 1 } else { 1 };
            if run > best.0 {
                best = (run, cols[k]);
            }
        }
        marks.push((Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }, best.1));
    }
    marks.sort_by_key(|m| (m.0.y, m.0.x));
    let mut lines: Vec<(Rect, usize, Vec<u32>)> = Vec::new();
    for (r, colour) in marks {
        if r.w > GLYPH_MAX_W || r.h > GLYPH_MAX_H {
            out.push(Detected { shape: Shape::Box, rect: r, color: colour });
            continue;
        }
        let slot = lines.iter_mut().find(|(lr, _, _)| {
            let (top, bot) = (lr.y.max(r.y), (lr.y + lr.h).min(r.y + r.h));
            let overlap = bot.saturating_sub(top);
            let gap = r.x as i64 - (lr.x + lr.w) as i64;
            overlap * 2 >= r.h.min(lr.h) && gap >= -1 && gap <= (lr.h.max(r.h) as i64).max(8)
        });
        match slot {
            Some((lr, n, cols)) => {
                let (x1, y1) = ((lr.x + lr.w).max(r.x + r.w), (lr.y + lr.h).max(r.y + r.h));
                lr.x = lr.x.min(r.x);
                lr.y = lr.y.min(r.y);
                lr.w = x1 - lr.x;
                lr.h = y1 - lr.y;
                *n += 1;
                cols.push(colour);
            }
            None => lines.push((r, 1, vec![colour])),
        }
    }
    for (r, n, cols) in lines {
        let mut best = (0usize, cols[0]);
        for &c in &cols {
            let k = cols.iter().filter(|&&d| d == c).count();
            if k > best.0 {
                best = (k, c);
            }
        }
        let size = ((r.h as f64 / 7.0).round() as usize).clamp(1, 16);
        out.push(Detected { shape: Shape::Text { glyphs: n, size }, rect: r, color: best.1 });
    }
    out.sort_by_key(|d| (d.rect.y, d.rect.x));
    out.truncate(256);
    out
}

/// The `ui = ...` lines for a page of detected things (the porter's draft): text as `text` with `?` for each glyph (the words need text recovery), bars and meters as `bar` and
/// `meter`, discs as `disc`, grids as `grid`; boxes are left out (they are frames, not widgets). `page` must be a page the draft defines; colours are written as hex names the
/// caller defines (`color cNNNNNN=#nnnnnn`), returned beside the lines.
pub fn draft_lines(page: &str, things: &[Detected]) -> (Vec<String>, Vec<String>) {
    let mut colours: Vec<String> = Vec::new();
    let mut lines = Vec::new();
    for d in things {
        let mut name = format!("c{:06x}", d.color);
        if !colours.contains(&name) && colours.len() / 2 >= 56 {
            name = "fg".to_string(); // the model holds 64 colours: past 56 the rest are the foreground
        }
        let r = d.rect;
        let (w, h) = (r.w.min(240), r.h.min(240));
        let line = match d.shape {
            Shape::Text { glyphs, size } => format!("text {page} {} {} \"{}\" size={size} color={name}", r.x, r.y, "?".repeat(glyphs.min(60))),
            Shape::Bar => format!("bar {page} {} {} w={w} h={h} color={name}", r.x, r.y),
            Shape::Meter => format!("meter {page} {} {} w={w} h={h} color={name}", r.x, r.y),
            Shape::Disc => format!("disc {page} {} {} r={} color={name}", r.x + r.w / 2, r.y + r.h / 2, (r.w / 2).clamp(1, 240)),
            Shape::Grid { cols, rows } => format!("grid {page} {} {} w={w} h={h} cols={cols} rows={rows} color={name}", r.x, r.y),
            Shape::Box => continue,
        };
        if name != "fg" && !colours.contains(&name) {
            colours.push(name);
            let c = d.color;
            colours.push(format!("#{c:06x}"));
        }
        lines.push(line);
    }
    let defs = colours.chunks(2).map(|c| format!("color {}={}", c[0], c[1])).collect();
    (defs, lines)
}

/// The UI check (porter step 5, UI half): how well page `page` of `model`, drawn and rasterised (text as solid cells), agrees with a captured `frame` of the firmware's own
/// screen, as `(ink overlap, share of the model's ink that sits on ink)` from `coverage_agreement`, and the line a package's `verify-ui` holds. The thresholds are per firmware
/// (the model's text is solid cells and the firmware's is glyphs, so even a right layout scores below 1): this records what was measured and judges nothing.
pub fn verify_ui(model: &crate::ui::Ui, page: usize, frame: &[u32]) -> (f64, f64) {
    let layout = crate::ui::rasterize(&model.draw(page, &|_| None, &|_| None));
    coverage_agreement(frame, &layout, background(frame), model.color("bg"))
}

pub fn verify_ui_line(page_name: &str, score: (f64, f64)) -> String {
    format!("{page_name}: ink overlap {:.2}, {:.2} of the model on ink (text as solid cells; not compared with a unit)", score.0, score.1)
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

    /// A page drawn from a model (`ui::draw`, `ui::rasterize`), read back by `detect`: the text lines, the bar, the meter and the disc are found where the model put them.
    #[test]
    fn what_a_model_draws_is_detected_where_it_was_put() {
        use crate::ui::{rasterize, Bind, Ui};
        let lines: Vec<String> = [
            "theme bg=#101020 fg=#f0f0f0 dim=#808080 accent=#ff8040",
            "page home \"HOME\"",
            "text home 10 10 \"SLOW BLOOM\" size=2 color=accent",
            "text home 10 40 \"TEMPO 120 BPM\" color=fg",
            "bar home 10 80 w=120 h=6 bind=param:1 color=fg",
            "meter home 200 20 w=8 h=60 bind=param:1 color=accent",
            "disc home 60 150 r=25 bind=none color=accent",
        ]
        .iter()
        .map(|l| l.to_string())
        .collect();
        let ui = Ui::parse(&lines).unwrap();
        let ops = ui.draw(0, &|b| (*b == Bind::Param(1)).then_some(99), &|_| None);
        let px = rasterize(&ops);
        let things = detect(&px, 0x101020);
        let texts: Vec<&Detected> = things.iter().filter(|d| matches!(d.shape, Shape::Text { .. })).collect();
        assert_eq!(texts.len(), 2, "{things:?}");
        assert!(matches!(texts[0].shape, Shape::Text { glyphs: 9, size: 2 }) && texts[0].rect.x == 10 && texts[0].rect.y == 10 && texts[0].color == 0xff8040, "{:?}", texts[0]);
        assert!(matches!(texts[1].shape, Shape::Text { glyphs: 11, size: 1 }) && texts[1].rect.x == 10 && texts[1].rect.y == 40, "{:?}", texts[1]);
        assert!(things.iter().any(|d| d.shape == Shape::Bar && d.rect == Rect { x: 10, y: 80, w: 120, h: 6 }), "{things:?}");
        assert!(things.iter().any(|d| d.shape == Shape::Meter && d.rect.x == 200 && d.rect.y == 20 && d.rect.h == 60), "{things:?}");
        assert!(things.iter().any(|d| d.shape == Shape::Disc && d.rect.x == 35 && d.rect.w == 51), "{things:?}");
        // the draft the detections make is a model Ui::parse accepts, with a widget for each
        let (defs, ws) = draft_lines("home", &things);
        let mut draft = vec!["theme bg=#101020 fg=#f0f0f0 dim=#808080 accent=#ff8040".to_string(), "page home \"HOME\"".to_string()];
        draft.extend(defs);
        draft.extend(ws);
        let back = Ui::parse(&draft).unwrap();
        assert_eq!(back.widgets.len(), 6, "two texts, a bar, a meter and the two discs a disc is drawn with (a ring and its dot)");
    }

    #[test]
    fn a_lattice_of_cells_is_a_grid_and_a_lone_mark_is_not() {
        let mut f = frame(0x000000);
        for row in 0..2 {
            for col in 0..16 {
                fill(&mut f, Rect { x: 20 + col * 10, y: 100 + row * 10, w: 8, h: 8 }, if (row + col) % 3 == 0 { 0xff4040 } else { 0x404040 });
            }
        }
        let g = detect(&f, 0);
        // lit and unlit cells are different colours: each colour on its own lattice, and the lit ones are 11 cells of 32
        assert!(g.iter().any(|d| matches!(d.shape, Shape::Grid { .. })), "{g:?}");
        let mut one = frame(0);
        fill(&mut one, Rect { x: 50, y: 50, w: 8, h: 8 }, 0xffffff);
        assert!(!detect(&one, 0).iter().any(|d| matches!(d.shape, Shape::Grid { .. })));
        // an empty frame has nothing, and a frame of noise does not run away
        assert!(detect(&frame(5), 5).is_empty());
        let noise: Vec<u32> = (0..W * H).map(|i| ((i * 2654435761usize) >> 7) as u32 & 0x1f1f1f).collect();
        assert!(detect(&noise, 0).len() <= 256);
    }

    #[test]
    fn a_model_that_draws_the_screen_scores_high_and_a_wrong_one_low() {
        use crate::ui::{rasterize, Ui};
        let lines: Vec<String> = ["theme bg=#101020 fg=#f0f0f0 dim=#808080 accent=#ff8040", "page home \"HOME\"", "text home 10 10 \"SLOW BLOOM\" size=2 color=accent", "bar home 10 80 w=120 h=6 color=fg", "disc home 60 150 r=25 color=accent"]
            .iter()
            .map(|l| l.to_string())
            .collect();
        let ui = Ui::parse(&lines).unwrap();
        let frame = rasterize(&ui.draw(0, &|_| None, &|_| None));
        let (iou, share) = verify_ui(&ui, 0, &frame);
        assert!(iou > 0.99 && share > 0.99, "{iou} {share}");
        let other = Ui::parse(&lines[..2].iter().cloned().chain(["bar home 150 200 w=80 h=20 color=fg".to_string()]).collect::<Vec<_>>()).unwrap();
        let (iou, share) = verify_ui(&other, 0, &frame);
        assert!(iou < 0.05 && share < 0.05, "{iou} {share}");
        assert!(verify_ui_line("home", (0.5, 0.8)).starts_with("home: ink overlap 0.50, 0.80"));
    }
}
