//! A reader for `fm1tastik.json` (docs/FM1TASTIK-JSON.md, draft 0.1): the file an author keeps in a firmware's repository, read by the catalog and by this app's
//! FILES panel. **Untrusted input** (rule 5): the file is capped at 64 KiB, the JSON reader has a depth, string, array and object limit and never panics, every field
//! is checked against the format's limits, URLs must be https on a public host, and anything that does not fit is an `Error` that says what. Unknown keys are ignored
//! (the format grows), an unknown `categories` value reads as `other`, an unknown major version is refused with the reason.
//! No dependency: a small bounded JSON reader is written here (rule 4).

use crate::{Error, Result};

/// The format's size cap for the file.
pub const MAX_FILE: usize = 64 * 1024;
const MAX_DEPTH: usize = 12;
const MAX_STRING: usize = 4096;
const MAX_ITEMS: usize = 256;

fn bad<T>(why: impl Into<String>) -> Result<T> {
    Err(Error::BadImage(format!("catalog: {}", why.into())))
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------
// JSON

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<()> {
        if self.b.get(self.at) == Some(&c) {
            self.at += 1;
            Ok(())
        } else {
            bad(format!("json: expected {:?} at byte {}", c as char, self.at))
        }
    }

    fn lit(&mut self, word: &str, v: Json) -> Result<Json> {
        if self.b[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(v)
        } else {
            bad(format!("json: unexpected text at byte {}", self.at))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json> {
        if depth > MAX_DEPTH {
            return bad("json: nested too deeply");
        }
        self.ws();
        match self.b.get(self.at) {
            None => bad("json: ends early"),
            Some(b'{') => {
                self.at += 1;
                let mut o: Vec<(String, Json)> = Vec::new();
                self.ws();
                if self.b.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Json::Obj(o));
                }
                loop {
                    if o.len() >= MAX_ITEMS {
                        return bad("json: an object has too many keys");
                    }
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.eat(b':')?;
                    let v = self.value(depth + 1)?;
                    o.push((k, v));
                    self.ws();
                    match self.b.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Obj(o));
                        }
                        _ => return bad(format!("json: expected , or }} at byte {}", self.at)),
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut a = Vec::new();
                self.ws();
                if self.b.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::Arr(a));
                }
                loop {
                    if a.len() >= MAX_ITEMS {
                        return bad("json: an array has too many items");
                    }
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Arr(a));
                        }
                        _ => return bad(format!("json: expected , or ] at byte {}", self.at)),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'n') => self.lit("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => bad(format!("json: unexpected byte at {}", self.at)),
        }
    }

    fn number(&mut self) -> Result<Json> {
        let start = self.at;
        while matches!(self.b.get(self.at), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
            self.at += 1;
        }
        let s = std::str::from_utf8(&self.b[start..self.at]).unwrap_or("");
        match s.parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Json::Num(n)),
            _ => bad(format!("json: a bad number at byte {start}")),
        }
    }

    fn hex4(&mut self) -> Result<u32> {
        let h = self.b.get(self.at..self.at + 4).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u32::from_str_radix(h, 16).ok());
        match h {
            Some(v) => {
                self.at += 4;
                Ok(v)
            }
            None => bad(format!("json: a bad \\u escape at byte {}", self.at)),
        }
    }

    fn string(&mut self) -> Result<String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.at) else { return bad("json: a string is not closed") };
            self.at += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.b.get(self.at) else { return bad("json: a string is not closed") };
                    self.at += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if self.b.get(self.at..self.at + 2) != Some(b"\\u") {
                                    return bad("json: half a surrogate pair");
                                }
                                self.at += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return bad("json: half a surrogate pair");
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            match char::from_u32(cp) {
                                Some(c) => c,
                                None => return bad("json: a lone surrogate"),
                            }
                        }
                        _ => return bad(format!("json: a bad escape at byte {}", self.at)),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                0..=0x1F => return bad("json: a control character in a string"),
                c => out.push(c),
            }
            if out.len() > MAX_STRING {
                return bad("json: a string is too long");
            }
        }
        match String::from_utf8(out) {
            Ok(s) => Ok(s),
            Err(_) => bad("json: a string is not UTF-8"),
        }
    }
}

/// Parses one JSON document. Bounded: depth, string length, items in an array or object, and (by the caller) size.
pub fn parse_json(bytes: &[u8]) -> Result<Json> {
    let mut r = Reader { b: bytes, at: 0 };
    let v = r.value(0)?;
    r.ws();
    if r.at != bytes.len() {
        return bad(format!("json: text after the document at byte {}", r.at));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------------------
// The catalog

pub const CATEGORIES: [&str; 10] = ["synth", "sequencer", "groovebox", "sampler", "drum", "utility", "midi", "toy", "game", "other"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub date: String,
    pub url: String,
    pub page: String,
    pub preview_url: String,
    /// 64 lowercase hex digits
    pub sha256: String,
    pub size: u64,
    pub format: String,
    pub notes: String,
    pub deprecated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThirdParty {
    pub what: String,
    /// `granted`, `reserved` or `unknown`
    pub rights: String,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub author: String,
    pub homepage: String,
    pub source: String,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub spdx: String,
    pub licence_files: Vec<String>,
    pub third_party: Vec<ThirdParty>,
    pub needs_assets: bool,
    /// the author asked a catalog not to keep a screenshot / not to run it in a browser
    pub thumbnail: bool,
    pub preview: bool,
    pub releases: Vec<Release>,
}

impl Entry {
    /// The licence's tier on the allowlist (`CLAUDE.md` rule 2), or an `Error` saying which term is not on it. The author's words, not a verdict on the files.
    pub fn tier(&self) -> Result<crate::package::Tier> {
        crate::package::tier(&self.spdx)
    }

    /// The newest release that is not deprecated (the file lists newest first), if any.
    pub fn latest(&self) -> Option<&Release> {
        self.releases.iter().find(|r| !r.deprecated)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    /// `false` asks every catalog not to list these firmwares: a reader must honour it
    pub listed: bool,
    /// a one-time claim code, present only while a claim is being proved; the reader never keeps it
    pub claim: Option<String>,
    pub issues: String,
    pub entries: Vec<Entry>,
}

fn text(o: &Json, key: &str, min: usize, max: usize, required: bool) -> Result<String> {
    match o.get(key) {
        None | Some(Json::Null) if !required => Ok(String::new()),
        None | Some(Json::Null) => bad(format!("{key} is required")),
        Some(Json::Str(s)) => {
            let n = s.chars().count();
            if n < min || n > max {
                return bad(format!("{key} must be {min} to {max} characters, not {n}"));
            }
            if s.chars().any(|c| c.is_control() && c != '\n') {
                return bad(format!("{key} has a control character"));
            }
            Ok(s.clone())
        }
        Some(_) => bad(format!("{key} must be a string")),
    }
}

fn flag(o: &Json, key: &str, default: bool) -> Result<bool> {
    match o.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(Json::Bool(b)) => Ok(*b),
        Some(_) => bad(format!("{key} must be true or false")),
    }
}

fn list<'a>(o: &'a Json, key: &str, min: usize, max: usize) -> Result<&'a [Json]> {
    match o.get(key) {
        None | Some(Json::Null) if min == 0 => Ok(&[]),
        None | Some(Json::Null) => bad(format!("{key} is required")),
        Some(Json::Arr(a)) if a.len() >= min && a.len() <= max => Ok(a),
        Some(Json::Arr(a)) => bad(format!("{key} must have {min} to {max} items, not {}", a.len())),
        Some(_) => bad(format!("{key} must be a list")),
    }
}

fn slug(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether the host of an https URL is a public name: no userinfo, no port games, and not localhost, a private, loopback or link-local address, or a bare name.
/// A reader that fetches a person's URL must not be steered at its own network (docs/FM1TASTIK-JSON.md, "How a reader must behave").
pub fn public_https(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let auth = &rest[..end];
    if auth.is_empty() || auth.contains('@') || url.chars().any(|c| c.is_control() || c == ' ' || c == '\\') {
        return false;
    }
    if auth.starts_with('[') {
        return false; // an IPv6 literal: never public by name
    }
    let (host, port) = match auth.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (auth, None),
    };
    if let Some(p) = port {
        if p != "443" {
            return false;
        }
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') || host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".lan") {
        return false;
    }
    if !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
        return false;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        // a dotted number (or one with fewer than four parts, which parsers read as an address too): only a plain public IPv4
        if parts.len() != 4 {
            return false;
        }
        let o: Vec<u16> = parts.iter().filter_map(|p| p.parse().ok()).collect();
        if o.len() != 4 || o.iter().any(|&x| x > 255) {
            return false;
        }
        let private = o[0] == 0 || o[0] == 10 || o[0] == 127 || (o[0] == 169 && o[1] == 254) || (o[0] == 172 && (16..32).contains(&o[1])) || (o[0] == 192 && o[1] == 168) || (o[0] == 100 && (64..128).contains(&o[1])) || o[0] >= 224;
        return !private;
    }
    parts.iter().all(|p| !p.is_empty() && !p.starts_with('-') && !p.ends_with('-'))
}

/// A URL or nothing (empty is "not given"); a URL must be https on a public host.
fn url(o: &Json, key: &str, required: bool) -> Result<String> {
    let s = text(o, key, 1, 512, required)?;
    if !s.is_empty() && !public_https(&s) {
        return bad(format!("{key} must be an https URL on a public host"));
    }
    Ok(s)
}

/// Whether text looks like a webhook or a token, which never belongs in a public file.
fn secretish(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("/api/webhooks/") || l.contains("hooks.slack.com") || l.contains("webhook") || l.contains("token=") || l.contains("secret")
}

fn date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) && {
        let (m, d) = (&s[5..7], &s[8..10]);
        ("01"..="12").contains(&m) && ("01"..="31").contains(&d)
    }
}

fn release(r: &Json) -> Result<Release> {
    let sha = text(r, "sha256", 64, 64, true)?.to_ascii_lowercase();
    if !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return bad("sha256 must be 64 hex digits");
    }
    let d = text(r, "date", 0, 10, false)?;
    if !d.is_empty() && !date(&d) {
        return bad("date must be YYYY-MM-DD");
    }
    let size = match r.get("size") {
        None | Some(Json::Null) => 0,
        Some(Json::Num(n)) if *n >= 0.0 && n.fract() == 0.0 && *n <= 1e12 => *n as u64,
        Some(_) => return bad("size must be a whole number of bytes"),
    };
    let format = text(r, "format", 0, 24, false)?;
    Ok(Release {
        version: text(r, "version", 1, 48, true)?,
        date: d,
        url: url(r, "url", true)?,
        page: url(r, "page", false)?,
        preview_url: url(r, "preview_url", false)?,
        sha256: sha,
        size,
        format: if format.is_empty() { "fwsc".into() } else { format },
        notes: text(r, "notes", 0, 500, false)?,
        deprecated: flag(r, "deprecated", false)?,
    })
}

fn entry(f: &Json) -> Result<Entry> {
    let id = text(f, "id", 1, 48, true)?;
    if !slug(&id, 48) {
        return bad(format!("id {id:?} must be lowercase letters, digits and hyphens"));
    }
    let wrap = |e: Error| match e {
        Error::BadImage(w) => Error::BadImage(format!("{w} (firmware {id})")),
    };
    (|| {
        let mut categories: Vec<String> = Vec::new();
        for c in list(f, "categories", 0, 3)? {
            let Json::Str(s) = c else { return bad("a category must be a string") };
            let s = if CATEGORIES.contains(&s.as_str()) { s.clone() } else { "other".to_string() };
            if !categories.contains(&s) {
                categories.push(s);
            }
        }
        let mut tags = Vec::new();
        for t in list(f, "tags", 0, 10)? {
            match t {
                Json::Str(s) if slug(s, 24) => tags.push(s.clone()),
                _ => return bad("a tag must be lowercase letters, digits and hyphens, up to 24"),
            }
        }
        let lic = match f.get("licence") {
            Some(l @ Json::Obj(_)) => l,
            _ => return bad("licence is required"),
        };
        let spdx = text(lic, "spdx", 1, 200, true)?;
        let mut licence_files = Vec::new();
        for p in list(lic, "files", 1, 16)? {
            match p {
                Json::Str(s) if !s.is_empty() && s.len() <= 200 && !s.starts_with('/') && !s.split('/').any(|c| c == "..") && !s.chars().any(|c| c.is_control() || c == '\\') => {
                    licence_files.push(s.clone())
                }
                _ => return bad("a licence file must be a relative path inside the repository"),
            }
        }
        let mut third_party = Vec::new();
        for t in list(lic, "third_party", 0, 32)? {
            let rights = text(t, "rights", 1, 16, true)?;
            if !["granted", "reserved", "unknown"].contains(&rights.as_str()) {
                return bad(format!("third_party rights {rights:?} must be granted, reserved or unknown"));
            }
            third_party.push(ThirdParty { what: text(t, "what", 1, 200, true)?, rights, note: text(t, "note", 0, 500, false)? });
        }
        let mut releases = Vec::new();
        for r in list(f, "releases", 0, 64)? {
            releases.push(release(r)?);
        }
        let needs_assets = match f.get("needs") {
            Some(n @ Json::Obj(_)) => flag(n, "assets", false)?,
            _ => false,
        };
        let (thumbnail, preview) = match f.get("catalog") {
            Some(c @ Json::Obj(_)) => (flag(c, "thumbnail", true)?, flag(c, "preview", true)?),
            _ => (true, true),
        };
        let author = match f.get("author") {
            Some(a @ Json::Obj(_)) => text(a, "name", 0, 64, false)?,
            _ => String::new(),
        };
        Ok(Entry {
            id: id.clone(),
            name: text(f, "name", 1, 64, true)?,
            summary: text(f, "summary", 1, 140, true)?,
            description: text(f, "description", 0, 2000, false)?,
            author,
            homepage: url(f, "homepage", false)?,
            source: url(f, "source", false)?,
            categories,
            tags,
            spdx,
            licence_files,
            third_party,
            needs_assets,
            thumbnail,
            preview,
            releases,
        })
    })()
    .map_err(wrap)
}

impl Catalog {
    /// Reads a `fm1tastik.json`. Refused: over 64 KiB, not JSON, not an object, a `format` that is not `fm1tastik/1` (an unknown major says so), a firmware or a
    /// release that breaks a limit, a duplicate id, a URL that is not https on a public host, a webhook or token in `contact`.
    pub fn parse(bytes: &[u8]) -> Result<Catalog> {
        if bytes.len() > MAX_FILE {
            return bad(format!("the file is {} bytes, over the {MAX_FILE} limit", bytes.len()));
        }
        let doc = parse_json(bytes)?;
        if !matches!(doc, Json::Obj(_)) {
            return bad("the document must be an object");
        }
        let format = text(&doc, "format", 1, 32, true)?;
        let Some(major) = format.strip_prefix("fm1tastik/") else { return bad(format!("format {format:?} is not fm1tastik/<major>")) };
        match major.split('.').next().and_then(|m| m.parse::<u32>().ok()) {
            Some(1) => {}
            Some(m) => return bad(format!("this reader knows fm1tastik/1, not major version {m}")),
            None => return bad(format!("format {format:?} has no major version")),
        }
        let mut issues = String::new();
        if let Some(c) = doc.get("contact") {
            if !matches!(c, Json::Obj(_) | Json::Null) {
                return bad("contact must be an object");
            }
            if let Json::Obj(o) = c {
                for (k, v) in o {
                    if let Json::Str(s) = v {
                        if secretish(k) || secretish(s) {
                            return bad("contact holds what looks like a webhook or a token: a public file must not");
                        }
                    }
                }
            }
            issues = url(c, "issues", false)?;
            if let Some(Json::Str(d)) = c.get("discord") {
                if !public_https(d) {
                    return bad("discord must be an https URL on a public host");
                }
            }
            text(c, "email", 0, 120, false)?;
        }
        let claim = match doc.get("claim") {
            Some(c @ Json::Obj(_)) => Some(text(c, "code", 12, 64, true)?),
            _ => None,
        };
        let mut entries: Vec<Entry> = Vec::new();
        for f in list(&doc, "firmware", 1, 32)? {
            let e = entry(f)?;
            if entries.iter().any(|x| x.id == e.id) {
                return bad(format!("the id {:?} is used twice", e.id));
            }
            entries.push(e);
        }
        Ok(Catalog { listed: flag(&doc, "listed", true)?, claim, issues, entries })
    }

    /// The entries a catalog may show: none when the author set `listed: false`.
    pub fn visible(&self) -> &[Entry] {
        if self.listed {
            &self.entries
        } else {
            &[]
        }
    }
}

/// Checks bytes a reader fetched against a release's `sha256`: a mismatch is a broken or tampered release (docs/FM1TASTIK-JSON.md), never a new one.
pub fn matches_release(bytes: &[u8], r: &Release) -> bool {
    if r.size != 0 && bytes.len() as u64 != r.size {
        return false;
    }
    let h = crate::package::sha256(bytes);
    let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
    hex == r.sha256
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
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

    fn good_with(from: &str, to: &str) -> Vec<u8> {
        assert!(GOOD.contains(from), "{from}");
        GOOD.replace(from, to).into_bytes()
    }

    #[test]
    fn a_good_file_reads_and_the_rest_is_ignored() {
        let c = Catalog::parse(GOOD.as_bytes()).unwrap();
        assert!(c.listed && c.claim.as_deref() == Some("k7Q2-M9XV-4HNE") && c.issues.ends_with("/issues"));
        let e = &c.entries[0];
        assert_eq!((e.id.as_str(), e.name.as_str(), e.author.as_str()), ("zither", "ZITHER", "A. Maker"));
        assert_eq!(e.categories, ["synth", "sequencer", "other"], "an unknown category reads as other");
        assert!(!e.thumbnail && e.preview && !e.needs_assets);
        assert_eq!(e.licence_files.len(), 2);
        assert_eq!(e.tier().unwrap(), crate::package::Tier::Copyleft);
        let r = e.latest().unwrap();
        assert_eq!((r.version.as_str(), r.size, r.format.as_str()), ("1.2.0", 5, "fwsc"));
        assert_eq!(r.sha256, r.sha256.to_ascii_lowercase(), "the hash is kept lowercase");
        assert!(matches_release(b"hello", r), "SHA-256 of hello");
        assert!(!matches_release(b"hellO", r) && !matches_release(b"hello!", r));
    }

    #[test]
    fn listed_false_hides_everything() {
        let c = Catalog::parse(&good_with("\"listed\": true", "\"listed\": false")).unwrap();
        assert!(c.visible().is_empty() && c.entries.len() == 1);
    }

    #[test]
    fn what_must_be_refused() {
        let refused = |why: &str, b: Vec<u8>| assert!(Catalog::parse(&b).is_err(), "{why} was accepted");
        refused("an unknown major", good_with("fm1tastik/1", "fm1tastik/2"));
        refused("no format", good_with("\"format\": \"fm1tastik/1\",", ""));
        refused("a bad hash length", good_with("2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824", "2cf24d"));
        refused("a non-hex hash", good_with("2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824", &"z".repeat(64)));
        refused("an http url", good_with("https://github.com/example/zither/releases", "http://github.com/example/zither/releases"));
        refused("a webhook in contact", good_with("https://discord.gg/xxxxxxx", "https://discord.com/api/webhooks/123/abc"));
        refused("a token in contact", good_with("maker@example.org", "token=abcdef"));
        refused("a localhost release", good_with("https://github.com/example/zither/releases", "https://localhost/x"));
        refused("an upper-case id", good_with("\"id\": \"zither\"", "\"id\": \"Zither\""));
        refused("an empty summary", good_with("A plucked-string synth with a four-track sequencer.", ""));
        refused("a long summary", good_with("A plucked-string synth with a four-track sequencer.", &"x".repeat(141)));
        refused("no licence", good_with("\"licence\": {", "\"licenc\": {"));
        refused("a licence path out of the repository", good_with("\"LICENSE\"", "\"../../etc/passwd\""));
        refused("an absolute licence path", good_with("\"LICENSE\"", "\"/etc/passwd\""));
        refused("a bad third-party value", good_with("\"granted\"", "\"yes\""));
        refused("a bad date", good_with("2026-11-10", "2026-13-10"));
        refused("four categories", good_with("[\"synth\", \"sequencer\", \"weird\"]", "[\"synth\",\"sequencer\",\"toy\",\"game\"]"));
        refused("a bad tag", good_with("\"plucked\"", "\"Plucked Strings\""));
        refused("a negative size", good_with("\"size\": 5", "\"size\": -5"));
        refused("not an object", b"[1,2]".to_vec());
        refused("empty", Vec::new());
        refused("no firmware", br#"{"format":"fm1tastik/1","firmware":[]}"#.to_vec());
        let mut two = GOOD.to_string();
        let one = &GOOD[GOOD.find("{\n        \"id\"").unwrap()..GOOD.rfind("] }").unwrap()];
        two = two.replace(one, &format!("{one},{one}"));
        refused("a repeated id", two.into_bytes());
        refused("over 64 KiB", format!("{{\"format\":\"fm1tastik/1\",\"x\":\"{}\"}}", " ".repeat(MAX_FILE)).into_bytes());
    }

    #[test]
    fn the_limits_themselves_are_accepted() {
        let name = "n".repeat(64);
        let id = "a".repeat(48);
        let summary = "s".repeat(140);
        let one = |id: &str| format!(r#"{{"id":"{id}","name":"{name}","summary":"{summary}","licence":{{"spdx":"MIT","files":["LICENSE"]}}}}"#);
        let many: Vec<String> = (0..32).map(|k| one(&format!("{}{k:02}", &id[..46]))).collect();
        let doc = format!(r#"{{"format":"fm1tastik/1","firmware":[{}]}}"#, many.join(","));
        let c = Catalog::parse(doc.as_bytes()).unwrap();
        assert_eq!(c.entries.len(), 32);
        let too_many = format!(r#"{{"format":"fm1tastik/1","firmware":[{},{}]}}"#, many.join(","), one("extra"));
        assert!(Catalog::parse(too_many.as_bytes()).is_err(), "33 firmwares");
    }

    #[test]
    fn only_public_https_hosts() {
        for ok in ["https://example.org/x", "https://github.com/a/b", "https://cdn.jsdelivr.net/gh/o/r@v1/f.fwsc", "https://8.8.8.8/x", "https://example.org:443/x"] {
            assert!(public_https(ok), "{ok}");
        }
        for no in [
            "http://example.org",
            "https://localhost/x",
            "https://foo.localhost/",
            "https://127.0.0.1/",
            "https://10.1.2.3/",
            "https://192.168.1.1/",
            "https://172.16.0.1/",
            "https://172.31.255.255/",
            "https://169.254.169.254/latest/meta-data",
            "https://0.0.0.0/",
            "https://100.64.0.1/",
            "https://[::1]/",
            "https://user:pw@example.org/",
            "https://example.org:8443/",
            "https://example.org@evil.example/",
            "https://2130706433/",
            "https://127.1/",
            "https://printer.local/",
            "https://intranet/",
            "https://exa mple.org/",
            "https://example.org\\@evil/",
            "https:///x",
            "ftp://example.org/x",
            "file:///etc/passwd",
            "",
        ] {
            assert!(!public_https(no), "{no:?} was accepted");
        }
    }

    #[test]
    fn json_edges() {
        assert_eq!(parse_json(br#""\u00e9\ud83d\ude00""#).unwrap(), Json::Str("é😀".into()));
        assert!(parse_json(br#""\ud83d""#).is_err() && parse_json(br#""\udc00""#).is_err() && parse_json(br#""\x""#).is_err());
        assert!(parse_json(b"\"a\nb\"").is_err(), "a raw newline in a string");
        assert!(parse_json(b"{\"a\":1,}").is_err() && parse_json(b"[1,]").is_err() && parse_json(b"01x").is_err() && parse_json(b"1 2").is_err());
        assert!(parse_json(b"\xff").is_err() && parse_json(b"\"\xff\"").is_err());
        assert_eq!(parse_json(b" [ 1 , -2.5e1 , true , null ] ").unwrap(), Json::Arr(vec![Json::Num(1.0), Json::Num(-25.0), Json::Bool(true), Json::Null]));
        assert!(parse_json(&[b'['; 40]).is_err(), "deep nesting is refused, not recursed into");
        assert!(parse_json(format!("[{}]", "0,".repeat(300) + "0").as_bytes()).is_err(), "too many items");
        assert!(parse_json(format!("\"{}\"", "a".repeat(MAX_STRING + 1)).as_bytes()).is_err());
        assert!(parse_json(b"1e999").is_err(), "a number that is not finite");
    }

    /// Every cut of a good file, every byte set to a few awkward values, and a deterministic spray of damage: `Ok` or `Err`, never a panic or a hang.
    #[test]
    fn damage_never_panics() {
        let good = GOOD.as_bytes();
        for n in 0..good.len() {
            let _ = Catalog::parse(&good[..n]);
        }
        for i in 0..good.len() {
            for v in [0u8, b'"', b'\\', b'{', b'}', 0xFF, b'9'] {
                let mut d = good.to_vec();
                d[i] = v;
                let _ = Catalog::parse(&d);
            }
        }
        let mut x = 0x2545F4914F6CDD1Du64;
        for _ in 0..3000 {
            let mut d = good.to_vec();
            for _ in 0..1 + (x % 6) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x as usize) % d.len();
                d[at] = (x >> 24) as u8;
            }
            let _ = Catalog::parse(&d);
        }
        for junk in [&b"{{{{{{{{{{{{{{{{"[..], b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[", b"\"\\u", b"{\"a\"", b"-", b"nul", b"{\"format\":"] {
            assert!(Catalog::parse(junk).is_err());
        }
    }
}
