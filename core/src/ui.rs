//! The UI model of an instrument package (docs/PORTING-UI.md): what a firmware's screen and controls are, written down as data.
//!
//! A `.fm1i` may carry `ui = ...` lines (format 3). Each line is one directive: a **theme** (named colours), a **page** (a screen), a **widget** on a page (text,
//! value row, fader, meter, grid, disc, bar, hint), or a **control** (a button or encoder that goes to a page or drives a binding). Data, never code: a binding
//! names an engine parameter, a global, an effect parameter or a runtime state name, and a widget is placed in the FM-1's 240 x 240 pixels.
//!
//! `Ui::parse` is bounded (rule 5) and an `Error` on anything else. `Ui::draw` lays a page out as a **display list** (rectangles, circles, text) that a renderer
//! (the host's `pix.h`, a test, the porter's verifier) paints; this module paints nothing and owns no font. `rasterize` makes a display list into pixels with
//! text as solid cells, which is what `screen::coverage_agreement` compares with a captured frame (porter step 5, UI half).
//!
//! The grammar (one directive per `ui = ` line; `"..."` is a text with spaces; `#rrggbb` a colour; a missing value takes its default):
//! ```text
//! ui = theme bg=#1a0a12 fg=#f5e6e8 dim=#8a6a72 accent=#ff7f66     # the four base colours (any other name=#colour is a named colour)
//! ui = color drums=#ff6b5a bass=#a78bfa
//! ui = page home "HOME"
//! ui = text home 10 20 "SLOW BLOOM" size=2 color=fg bind=state:title
//! ui = value song 8 40 "TEMPO" bind=param:12 min=40 max=200 unit="BPM" bar=1
//! ui = fader mixer 20 30 h=100 bind=param:20 color=drums label="DRUMS"
//! ui = meter mixer 20 140 w=8 h=40 bind=state:level0 color=drums
//! ui = grid parts 30 40 w=160 h=100 cols=32 rows=7 bind=state:pattern0 color=accent
//! ui = disc home 20 120 r=60 bind=state:beat color=accent
//! ui = bar home 8 200 w=120 bind=state:progress color=accent
//! ui = hint song "K1-K4 CHANGE  HOME MAIN"
//! ui = key HOME page=home          # control: a button opens a page
//! ui = enc KNOB1 page=song bind=param:12 step=1
//! ```

use crate::{Error, Result};

pub const SCREEN: i32 = 240;
pub const MAX_PAGES: usize = 16;
pub const MAX_WIDGETS: usize = 256;
pub const MAX_CONTROLS: usize = 64;
pub const MAX_COLORS: usize = 64;
pub const MAX_LINES: usize = 1024;
pub const MAX_LINE: usize = 240;
pub const MAX_TEXT: usize = 64;

/// What a widget shows or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    None,
    /// an engine parameter id
    Param(u16),
    /// a global setting id (`globals.rs`)
    Global(u16),
    /// effect `fx`'s parameter `id`
    Fx(u8, u16),
    /// a runtime value the host publishes by name (index into `STATE_NAMES`): the song's title, the beat, a level, a step
    State(u8),
}

/// The state names a host can publish (a closed list, so a package cannot ask for what no host has).
pub const STATE_NAMES: [&str; 12] = [
    "title", "key", "scale", "bpm", "mood", "beat", "bar", "progress", "playing", "level0", "level1", "pattern0",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Value,
    Fader,
    Meter,
    Grid,
    Disc,
    Bar,
    Hint,
}

const KINDS: [(&str, Kind); 8] = [
    ("text", Kind::Text),
    ("value", Kind::Value),
    ("fader", Kind::Fader),
    ("meter", Kind::Meter),
    ("grid", Kind::Grid),
    ("disc", Kind::Disc),
    ("bar", Kind::Bar),
    ("hint", Kind::Hint),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Widget {
    pub kind: Kind,
    pub page: usize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// text, the label of a value row or fader, the hint line
    pub text: String,
    pub bind: Bind,
    /// a colour name (`fg`, `dim`, `accent`, `bg` or one from `color`)
    pub color: String,
    /// text size multiplier (1 is the 5 x 7 font), or a disc's radius, or a grid's columns
    pub size: i32,
    pub min: i32,
    pub max: i32,
    pub unit: String,
    pub bar: bool,
    pub cols: i32,
    pub rows: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    /// a button or an encoder name, as in `devices.rs`
    pub name: String,
    pub encoder: bool,
    /// the page it opens (empty: none)
    pub page: String,
    pub bind: Bind,
    pub step: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Ui {
    /// `[bg, fg, dim, accent]` then the named colours
    pub colors: Vec<(String, u32)>,
    pub pages: Vec<(String, String)>,
    pub widgets: Vec<Widget>,
    pub controls: Vec<Control>,
}

/// One thing to paint. A renderer paints them in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Rect { x: i32, y: i32, w: i32, h: i32, color: u32 },
    /// a filled disc
    Disc { cx: i32, cy: i32, r: i32, color: u32 },
    /// text in the 5 x 7 font scaled by `size`
    Text { x: i32, y: i32, size: i32, color: u32, text: String },
}

fn bad<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::BadImage(format!("ui: {}", m.into())))
}

fn colour(s: &str) -> Option<u32> {
    let h = s.strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten()
}

/// Splits a line into tokens, keeping `"quoted text"` (and `key="quoted"`) together; the quotes are dropped.
fn tokens(line: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if quoted {
        return bad("an unclosed quote");
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

fn parse_bind(s: &str) -> Result<Bind> {
    let (k, v) = s.split_once(':').unwrap_or((s, ""));
    let num = |t: &str| t.parse::<u16>().or_else(|_| bad(format!("{s:?}: a number was expected")));
    Ok(match k {
        "" | "none" => Bind::None,
        "param" => Bind::Param(num(v)?),
        "global" => Bind::Global(num(v)?),
        "fx" => {
            let (i, id) = v.split_once('.').unwrap_or(("", ""));
            Bind::Fx(i.parse::<u8>().or_else(|_| bad(format!("{s:?}: fx needs I.N")))?, num(id)?)
        }
        "state" => match STATE_NAMES.iter().position(|n| *n == v) {
            Some(i) => Bind::State(i as u8),
            None => return bad(format!("{v:?} is not a state name")),
        },
        _ => return bad(format!("{s:?} is not a binding")),
    })
}

pub fn bind_text(b: &Bind) -> String {
    match b {
        Bind::None => "none".into(),
        Bind::Param(n) => format!("param:{n}"),
        Bind::Global(n) => format!("global:{n}"),
        Bind::Fx(i, n) => format!("fx:{i}.{n}"),
        Bind::State(i) => format!("state:{}", STATE_NAMES[usize::from(*i).min(STATE_NAMES.len() - 1)]),
    }
}

impl Ui {
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty() && self.widgets.is_empty() && self.controls.is_empty() && self.colors.is_empty()
    }

    fn page_index(&self, name: &str) -> Option<usize> {
        self.pages.iter().position(|p| p.0 == name)
    }

    /// The colour called `name` (`bg`, `fg`, `dim`, `accent` or a named one); a default grey when unknown.
    pub fn color(&self, name: &str) -> u32 {
        self.colors.iter().find(|c| c.0 == name).map(|c| c.1).unwrap_or(0x80_80_80)
    }

    /// Parses the directives (the text after `ui = ` on each line). An unknown directive, a page that does not exist, a colour that is not defined, a number
    /// out of the screen or too many of anything is an error.
    pub fn parse(lines: &[String]) -> Result<Ui> {
        if lines.len() > MAX_LINES {
            return bad("too many lines");
        }
        let mut ui = Ui::default();
        for (n, line) in lines.iter().enumerate() {
            let at = n + 1;
            if line.len() > MAX_LINE {
                return bad(format!("directive {at} is longer than {MAX_LINE} characters"));
            }
            let t = tokens(line)?;
            let Some(head) = t.first().map(String::as_str) else { continue };
            let num = |s: &str| s.parse::<i32>().or_else(|_| bad(format!("directive {at}: {s:?} is not a number")));
            match head {
                "theme" | "color" => {
                    for kv in &t[1..] {
                        let Some((k, v)) = kv.split_once('=') else { return bad(format!("directive {at}: {kv:?} is not name=#colour")) };
                        let Some(c) = colour(v) else { return bad(format!("directive {at}: {v:?} is not #rrggbb")) };
                        if k.is_empty() || k.len() > 16 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                            return bad(format!("directive {at}: a colour name is up to 16 letters, digits or _"));
                        }
                        if ui.colors.len() >= MAX_COLORS {
                            return bad("too many colours");
                        }
                        match ui.colors.iter_mut().find(|e| e.0 == k) {
                            Some(e) => e.1 = c,
                            None => ui.colors.push((k.to_string(), c)),
                        }
                    }
                }
                "page" => {
                    let Some(name) = t.get(1) else { return bad(format!("directive {at}: a page needs a name")) };
                    if ui.pages.len() >= MAX_PAGES {
                        return bad("too many pages");
                    }
                    if ui.page_index(name).is_some() {
                        return bad(format!("directive {at}: page {name:?} twice"));
                    }
                    ui.pages.push((name.clone(), t.get(2).cloned().unwrap_or_default().chars().take(MAX_TEXT).collect()));
                }
                "key" | "enc" => {
                    let Some(name) = t.get(1) else { return bad(format!("directive {at}: {head} needs a control name")) };
                    let encoder = head == "enc";
                    let known = if encoder { crate::devices::ENCODER_NAMES.contains(&name.as_str()) } else { crate::devices::BUTTON_NAMES.contains(&name.as_str()) };
                    if !known {
                        return bad(format!("directive {at}: {name:?} is not a {}", if encoder { "encoder" } else { "button" }));
                    }
                    if ui.controls.len() >= MAX_CONTROLS {
                        return bad("too many controls");
                    }
                    let mut c = Control { name: name.clone(), encoder, page: String::new(), bind: Bind::None, step: 1 };
                    for kv in &t[2..] {
                        match kv.split_once('=') {
                            Some(("page", v)) => c.page = v.to_string(),
                            Some(("bind", v)) => c.bind = parse_bind(v)?,
                            Some(("step", v)) => c.step = num(v)?.clamp(-1000, 1000),
                            _ => return bad(format!("directive {at}: {kv:?} is not page=, bind= or step=")),
                        }
                    }
                    ui.controls.push(c);
                }
                _ => {
                    let Some(&(_, kind)) = KINDS.iter().find(|k| k.0 == head) else { return bad(format!("directive {at}: {head:?} is not a directive")) };
                    if ui.widgets.len() >= MAX_WIDGETS {
                        return bad("too many widgets");
                    }
                    let Some(page) = t.get(1).and_then(|p| ui.page_index(p)) else { return bad(format!("directive {at}: the page is not defined (a page line comes first)")) };
                    let mut w = Widget {
                        kind,
                        page,
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                        text: String::new(),
                        bind: Bind::None,
                        color: "fg".into(),
                        size: 1,
                        min: 0,
                        max: 99,
                        unit: String::new(),
                        bar: false,
                        cols: 16,
                        rows: 1,
                    };
                    let mut rest = t[2..].iter().peekable();
                    // positional: x y, then (for text, value and hint) the text
                    if kind != Kind::Hint {
                        for slot in [&mut w.x, &mut w.y] {
                            let Some(v) = rest.next_if(|s| s.parse::<i32>().is_ok()) else { return bad(format!("directive {at}: x and y are expected")) };
                            *slot = num(v)?;
                        }
                    }
                    if matches!(kind, Kind::Text | Kind::Value | Kind::Hint) {
                        if let Some(v) = rest.next_if(|s| !s.contains('=')) {
                            w.text = v.chars().take(MAX_TEXT).collect();
                        }
                    }
                    for kv in rest {
                        let Some((k, v)) = kv.split_once('=') else { return bad(format!("directive {at}: {kv:?} is not key=value")) };
                        match k {
                            "w" => w.w = num(v)?,
                            "h" => w.h = num(v)?,
                            "r" | "size" => w.size = num(v)?,
                            "cols" => w.cols = num(v)?,
                            "rows" => w.rows = num(v)?,
                            "min" => w.min = num(v)?,
                            "max" => w.max = num(v)?,
                            "bar" => w.bar = num(v)? != 0,
                            "unit" => w.unit = v.chars().take(8).collect(),
                            "label" => w.text = v.chars().take(MAX_TEXT).collect(),
                            "color" => w.color = v.to_string(),
                            "bind" => w.bind = parse_bind(v)?,
                            _ => return bad(format!("directive {at}: {k:?} is not a widget setting")),
                        }
                    }
                    if !(0..=SCREEN).contains(&w.x) || !(0..=SCREEN).contains(&w.y) || !(0..=SCREEN).contains(&w.w) || !(0..=SCREEN).contains(&w.h) {
                        return bad(format!("directive {at}: a position or size is off the {SCREEN} x {SCREEN} screen"));
                    }
                    if !(1..=16).contains(&w.size) && kind != Kind::Disc {
                        return bad(format!("directive {at}: size is 1 to 16"));
                    }
                    if kind == Kind::Disc && !(1..=SCREEN).contains(&w.size) {
                        return bad(format!("directive {at}: a disc's radius is 1 to {SCREEN}"));
                    }
                    if !(1..=64).contains(&w.cols) || !(1..=64).contains(&w.rows) || w.min > w.max {
                        return bad(format!("directive {at}: cols and rows are 1 to 64 and min is not above max"));
                    }
                    ui.widgets.push(w);
                }
            }
        }
        // every named colour a widget uses must exist; every page a control opens must exist
        for w in &ui.widgets {
            if !["bg", "fg", "dim", "accent"].contains(&w.color.as_str()) && !ui.colors.iter().any(|c| c.0 == w.color) {
                return bad(format!("colour {:?} is not defined", w.color));
            }
        }
        for c in &ui.controls {
            if !c.page.is_empty() && ui.page_index(&c.page).is_none() {
                return bad(format!("{} opens page {:?}, which is not defined", c.name, c.page));
            }
        }
        Ok(ui)
    }

    /// The directives back as text (the lines after `ui = `), in an order `parse` reads back to an equal `Ui`.
    pub fn to_lines(&self) -> Vec<String> {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "'"));
        let mut l = Vec::new();
        if !self.colors.is_empty() {
            l.push(format!("color {}", self.colors.iter().map(|c| format!("{}=#{:06x}", c.0, c.1)).collect::<Vec<_>>().join(" ")));
        }
        for (name, title) in &self.pages {
            l.push(format!("page {name} {}", q(title)));
        }
        for w in &self.widgets {
            let kind = KINDS.iter().find(|k| k.1 == w.kind).map(|k| k.0).unwrap_or("text");
            let mut s = format!("{kind} {}", self.pages[w.page].0);
            if w.kind != Kind::Hint {
                s += &format!(" {} {}", w.x, w.y);
            }
            if matches!(w.kind, Kind::Text | Kind::Value | Kind::Hint) {
                s += &format!(" {}", q(&w.text));
            } else if !w.text.is_empty() {
                s += &format!(" label={}", q(&w.text));
            }
            s += &format!(" w={} h={} {}={} cols={} rows={} min={} max={} bar={} color={} bind={}", w.w, w.h, if w.kind == Kind::Disc { "r" } else { "size" }, w.size, w.cols, w.rows, w.min, w.max, i32::from(w.bar), w.color, bind_text(&w.bind));
            if !w.unit.is_empty() {
                s += &format!(" unit={}", q(&w.unit));
            }
            l.push(s);
        }
        for c in &self.controls {
            let mut s = format!("{} {}", if c.encoder { "enc" } else { "key" }, c.name);
            if !c.page.is_empty() {
                s += &format!(" page={}", c.page);
            }
            s += &format!(" bind={} step={}", bind_text(&c.bind), c.step);
            l.push(s);
        }
        l
    }

    /// The control that a button or encoder is, if the package gives it a job.
    pub fn control(&self, name: &str, encoder: bool) -> Option<&Control> {
        self.controls.iter().find(|c| c.name == name && c.encoder == encoder)
    }

    /// Page `page` as a display list. `value` gives a binding's current number (`None`: unknown, drawn as `--`); `text_of` a state's text (a title). Bounded: at
    /// most 4096 operations.
    pub fn draw(&self, page: usize, value: &dyn Fn(&Bind) -> Option<i32>, text_of: &dyn Fn(&Bind) -> Option<String>) -> Vec<Op> {
        let mut ops = vec![Op::Rect { x: 0, y: 0, w: SCREEN, h: SCREEN, color: self.color("bg") }];
        for w in self.widgets.iter().filter(|w| w.page == page) {
            if ops.len() > 4000 {
                break;
            }
            let c = self.color(&w.color);
            let (dim, fg) = (self.color("dim"), self.color("fg"));
            let v = value(&w.bind);
            let frac = |v: Option<i32>| v.map_or(0.0, |v| ((v - w.min) as f64 / (w.max - w.min).max(1) as f64).clamp(0.0, 1.0));
            let shown = |v: Option<i32>| v.map_or("--".to_string(), |v| format!("{v}{}", w.unit));
            match w.kind {
                Kind::Text => {
                    let t = text_of(&w.bind).unwrap_or_else(|| w.text.clone());
                    ops.push(Op::Text { x: w.x, y: w.y, size: w.size, color: c, text: t });
                }
                Kind::Hint => ops.push(Op::Text { x: 4, y: SCREEN - 9, size: 1, color: dim, text: w.text.clone() }),
                Kind::Value => {
                    ops.push(Op::Text { x: w.x, y: w.y, size: 1, color: dim, text: w.text.clone() });
                    ops.push(Op::Text { x: w.x, y: w.y + 10, size: w.size, color: fg, text: text_of(&w.bind).unwrap_or_else(|| shown(v)) });
                    if w.bar {
                        let bw = if w.w > 0 { w.w } else { 100 };
                        ops.push(Op::Rect { x: w.x, y: w.y + 10 + 8 * w.size + 2, w: bw, h: 1, color: dim });
                        ops.push(Op::Rect { x: w.x, y: w.y + 10 + 8 * w.size + 2, w: (bw as f64 * frac(v)) as i32, h: 2, color: c });
                    }
                }
                Kind::Fader => {
                    let h = if w.h > 0 { w.h } else { 80 };
                    ops.push(Op::Rect { x: w.x + 3, y: w.y, w: 2, h, color: dim });
                    let fill = (h as f64 * frac(v)) as i32;
                    ops.push(Op::Rect { x: w.x + 3, y: w.y + h - fill, w: 2, h: fill, color: c });
                    ops.push(Op::Rect { x: w.x, y: w.y + h - fill - 1, w: 8, h: 3, color: fg });
                    ops.push(Op::Text { x: w.x, y: w.y + h + 3, size: 1, color: c, text: w.text.clone() });
                }
                Kind::Meter | Kind::Bar => {
                    let (bw, bh) = (if w.w > 0 { w.w } else { 8 }, if w.h > 0 { w.h } else { 4 });
                    ops.push(Op::Rect { x: w.x, y: w.y, w: bw, h: bh, color: dim });
                    if bw >= bh {
                        ops.push(Op::Rect { x: w.x, y: w.y, w: (bw as f64 * frac(v)) as i32, h: bh, color: c });
                    } else {
                        let f = (bh as f64 * frac(v)) as i32;
                        ops.push(Op::Rect { x: w.x, y: w.y + bh - f, w: bw, h: f, color: c });
                    }
                }
                Kind::Grid => {
                    // the cells are drawn as a frame and one cell set per bit of the bound state (a host fills them in; here only the frame and the grid lines)
                    let (gw, gh) = (if w.w > 0 { w.w } else { 160 }, if w.h > 0 { w.h } else { 80 });
                    ops.push(Op::Rect { x: w.x, y: w.y, w: gw, h: 1, color: dim });
                    ops.push(Op::Rect { x: w.x, y: w.y + gh - 1, w: gw, h: 1, color: dim });
                    ops.push(Op::Rect { x: w.x, y: w.y, w: 1, h: gh, color: dim });
                    ops.push(Op::Rect { x: w.x + gw - 1, y: w.y, w: 1, h: gh, color: dim });
                    for r in 1..w.rows {
                        ops.push(Op::Rect { x: w.x, y: w.y + gh * r / w.rows, w: gw, h: 1, color: dim });
                    }
                    if let Some(p) = v {
                        let col = p.rem_euclid(w.cols);
                        ops.push(Op::Rect { x: w.x + gw * col / w.cols, y: w.y, w: (gw / w.cols).max(1), h: gh, color: c });
                    }
                }
                Kind::Disc => {
                    ops.push(Op::Disc { cx: w.x, cy: w.y, r: w.size, color: dim });
                    ops.push(Op::Disc { cx: w.x, cy: w.y, r: (w.size / 3).max(1), color: c });
                }
            }
        }
        ops
    }
}

/// A display list as pixels (`SCREEN` x `SCREEN`, 0xRRGGBB). Text is drawn as solid cells (6 x 8 per character at size 1): enough to compare where things are.
pub fn rasterize(ops: &[Op]) -> Vec<u32> {
    let n = SCREEN as usize;
    let mut px = vec![0u32; n * n];
    let mut put = |x: i32, y: i32, c: u32| {
        if (0..SCREEN).contains(&x) && (0..SCREEN).contains(&y) {
            px[y as usize * n + x as usize] = c;
        }
    };
    for op in ops {
        match op {
            Op::Rect { x, y, w, h, color } => {
                for yy in *y..(*y + *h).min(SCREEN) {
                    for xx in *x..(*x + *w).min(SCREEN) {
                        put(xx, yy, *color);
                    }
                }
            }
            Op::Disc { cx, cy, r, color } => {
                for yy in (cy - r)..=(cy + r) {
                    for xx in (cx - r)..=(cx + r) {
                        if (xx - cx).pow(2) + (yy - cy).pow(2) <= r * r {
                            put(xx, yy, *color);
                        }
                    }
                }
            }
            Op::Text { x, y, size, color, text } => {
                let (cw, ch) = (6 * size, 8 * size);
                for (i, c) in text.chars().enumerate() {
                    if c == ' ' {
                        continue;
                    }
                    for yy in 0..ch - size {
                        for xx in 0..cw - size {
                            put(x + i as i32 * cw + xx, y + yy, *color);
                        }
                    }
                }
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
    }

    const SAMPLE: &str = r##"
        theme bg=#1a0a12 fg=#f5e6e8 dim=#8a6a72 accent=#ff7f66
        color drums=#ff6b5a
        page home "HOME"
        page song "SONG"
        text home 10 20 "SLOW BLOOM" size=2 color=accent bind=state:title
        disc home 40 150 r=50 bind=state:beat color=accent
        value song 8 40 "TEMPO" bind=param:12 min=40 max=200 unit=BPM bar=1 w=120
        fader song 20 100 h=80 bind=param:20 color=drums label=DRUMS
        grid song 30 40 w=160 h=100 cols=32 rows=7 bind=state:pattern0
        hint song "K1-K4 CHANGE"
        key HOME page=home
        enc KNOB1 page=song bind=param:12 step=2
    "##;

    #[test]
    fn parses_round_trips_and_draws() {
        let ui = Ui::parse(&lines(SAMPLE)).unwrap();
        assert_eq!((ui.pages.len(), ui.widgets.len(), ui.controls.len()), (2, 6, 2));
        assert_eq!(ui.color("accent"), 0xff7f66);
        assert_eq!(ui.control("KNOB1", true).map(|c| c.bind), Some(Bind::Param(12)));
        assert!(ui.control("KNOB1", false).is_none());
        let again = Ui::parse(&ui.to_lines()).unwrap();
        assert_eq!(ui, again);
        // page 1 (song) with the tempo at 120: the value row's bar is a bit under half way
        let ops = ui.draw(1, &|b| (*b == Bind::Param(12)).then_some(120), &|_| None);
        assert!(ops.iter().any(|o| matches!(o, Op::Text { text, .. } if text == "120BPM")));
        assert!(ops.len() > 8 && ops.len() < 100);
        // unknown values draw as dashes, never a panic
        let ops = ui.draw(1, &|_| None, &|_| None);
        assert!(ops.iter().any(|o| matches!(o, Op::Text { text, .. } if text == "--")));
        let px = rasterize(&ops);
        assert_eq!(px.len(), 240 * 240);
        assert_eq!(px[0], 0x1a0a12);
        // the title comes from the host's state text
        let ops = ui.draw(0, &|_| Some(3), &|b| (*b == Bind::State(0)).then(|| "DRIFT".to_string()));
        assert!(ops.iter().any(|o| matches!(o, Op::Text { text, size: 2, .. } if text == "DRIFT")));
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        let bad = [
            "nonsense 1 2",
            "text nowhere 1 1 \"x\"",
            "page a\npage a",
            "page a\ntext a 1 1 \"x\" color=missing",
            "page a\ntext a 999 1 \"x\"",
            "page a\ntext a 1 1 \"x",
            "page a\nvalue a 1 1 \"x\" bind=state:nope",
            "page a\nvalue a 1 1 \"x\" bind=param:x",
            "page a\nvalue a 1 1 \"x\" min=5 max=1",
            "page a\nkey NOPE page=a",
            "page a\nkey HOME page=b",
            "color bg=#12345",
            "page a\ngrid a 1 1 cols=0",
            "page a\ndisc a 5 5 r=0",
        ];
        for b in bad {
            assert!(Ui::parse(&lines(b)).is_err(), "should refuse: {b}");
        }
        let many: Vec<String> = (0..MAX_LINES + 1).map(|_| "color a=#000000".to_string()).collect();
        assert!(Ui::parse(&many).is_err());
        let wide = vec![format!("page a \"{}\"", "x".repeat(MAX_LINE + 1))];
        assert!(Ui::parse(&wide).is_err());
        let widgets: Vec<String> = std::iter::once("page a".to_string()).chain((0..MAX_WIDGETS + 1).map(|_| "bar a 1 1".to_string())).collect();
        assert!(Ui::parse(&widgets).is_err());
        assert!(Ui::parse(&[]).unwrap().is_empty());
    }
}
