//! The C face of the `fm1tastik.json` reader (`catalog.rs`, docs/FM1TASTIK-JSON.md): parse a file the host fetched or was given, read the entries it may show,
//! and check a downloaded firmware against a release's hash. Every function tolerates null, reads only what its length says, and never unwinds.
//! Parsing is refused over 64 KiB (the format's limit); a failure says why in `fm1ntt_error`. **The app has no network fetch yet**: the host supplies the bytes.

use crate::capi::set_error;
use crate::catalog::{matches_release, Catalog, Entry, Release};
use crate::package::Tier;
use std::ffi::{c_char, c_int};

pub struct CatalogHandle(Catalog);

/// Reads a `fm1tastik.json` (UTF-8, at most 64 KiB). Null on failure, with the reason in `fm1ntt_error`.
///
/// # Safety
/// `data` points at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_parse(data: *const u8, len: usize) -> *mut CatalogHandle {
    if data.is_null() {
        set_error(Some("no catalog file".into()));
        return std::ptr::null_mut();
    }
    match Catalog::parse(std::slice::from_raw_parts(data, len.min(1 << 20))) {
        Ok(c) => {
            set_error(None);
            Box::into_raw(Box::new(CatalogHandle(c)))
        }
        Err(e) => {
            set_error(Some(e.to_string()));
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `h` is null or from `fm1ntt_catalog_parse`, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_free(h: *mut CatalogHandle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// How many entries the catalog may show (0 when the author set `listed: false`).
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_count(h: *const CatalogHandle) -> usize {
    h.as_ref().map_or(0, |h| h.0.visible().len())
}

fn entry(h: *const CatalogHandle, i: usize) -> Option<&'static Entry> {
    // SAFETY: the handle outlives the call; the borrow is only used inside the caller's call
    unsafe { h.as_ref() }.and_then(|h| h.0.visible().get(i))
}

fn release(h: *const CatalogHandle, i: usize, r: usize) -> Option<&'static Release> {
    entry(h, i).and_then(|e| e.releases.get(r))
}

unsafe fn put(s: &str, out: *mut c_char, cap: usize) -> usize {
    if !out.is_null() && cap > 0 {
        let n = s.len().min(cap - 1);
        std::ptr::copy_nonoverlapping(s.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    s.len()
}

/// An entry's text in `out` (NUL-terminated, cut to `cap`); the full length, 0 for a bad index. `which`: 0 id, 1 name, 2 summary, 3 description, 4 author,
/// 5 homepage, 6 source, 7 SPDX licence expression, 8 categories (comma-separated), 9 tags (comma-separated).
///
/// # Safety
/// `out` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_text(h: *const CatalogHandle, i: usize, which: c_int, out: *mut c_char, cap: usize) -> usize {
    let Some(e) = entry(h, i) else { return 0 };
    match which {
        0 => put(&e.id, out, cap),
        1 => put(&e.name, out, cap),
        2 => put(&e.summary, out, cap),
        3 => put(&e.description, out, cap),
        4 => put(&e.author, out, cap),
        5 => put(&e.homepage, out, cap),
        6 => put(&e.source, out, cap),
        7 => put(&e.spdx, out, cap),
        8 => put(&e.categories.join(","), out, cap),
        9 => put(&e.tags.join(","), out, cap),
        _ => 0,
    }
}

/// The licence's tier on the allowlist: 0 permissive, 1 copyleft (the Pack's), -1 not on the allowlist (never bundled; the author's words, not a verdict on the files).
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_tier(h: *const CatalogHandle, i: usize) -> c_int {
    match entry(h, i).map(|e| e.tier()) {
        Some(Ok(Tier::Permissive)) => 0,
        Some(Ok(Tier::Copyleft)) => 1,
        _ => -1,
    }
}

/// How many releases an entry lists, newest first.
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_release_count(h: *const CatalogHandle, i: usize) -> usize {
    entry(h, i).map_or(0, |e| e.releases.len())
}

/// The index of the newest release that is not deprecated, or -1.
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_latest(h: *const CatalogHandle, i: usize) -> c_int {
    entry(h, i).and_then(|e| e.releases.iter().position(|r| !r.deprecated)).map_or(-1, |p| p as c_int)
}

/// A release's text; the full length, 0 for a bad index. `which`: 0 version, 1 date, 2 download url, 3 page, 4 preview url, 5 SHA-256 (lowercase hex), 6 format, 7 notes.
///
/// # Safety
/// `out` is null or has room for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_release_text(h: *const CatalogHandle, i: usize, r: usize, which: c_int, out: *mut c_char, cap: usize) -> usize {
    let Some(r) = release(h, i, r) else { return 0 };
    match which {
        0 => put(&r.version, out, cap),
        1 => put(&r.date, out, cap),
        2 => put(&r.url, out, cap),
        3 => put(&r.page, out, cap),
        4 => put(&r.preview_url, out, cap),
        5 => put(&r.sha256, out, cap),
        6 => put(&r.format, out, cap),
        7 => put(&r.notes, out, cap),
        _ => 0,
    }
}

/// A release's size in bytes (0 when the author gave none), or 0 for a bad index.
///
/// # Safety
/// `h` is null or live.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_release_size(h: *const CatalogHandle, i: usize, r: usize) -> u64 {
    release(h, i, r).map_or(0, |r| r.size)
}

/// 1 when `data` is the release's file (its size and SHA-256 agree), else 0: a mismatch is a broken or tampered release, never a new one.
///
/// # Safety
/// `data` points at `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_catalog_matches(h: *const CatalogHandle, i: usize, r: usize, data: *const u8, len: usize) -> c_int {
    match (release(h, i, r), data.is_null()) {
        (Some(r), false) => matches_release(std::slice::from_raw_parts(data, len), r) as c_int,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    const FILE: &str = r#"{ "format": "fm1tastik/1", "contact": { "issues": "https://github.com/example/zither/issues" },
      "firmware": [ { "id": "zither", "name": "ZITHER", "summary": "A plucked-string synth.", "description": "d",
        "author": { "name": "A. Maker" }, "homepage": "https://example.org/zither", "source": "https://github.com/example/zither",
        "categories": ["synth"], "tags": ["plucked"], "licence": { "spdx": "MIT", "files": ["LICENSE"] },
        "releases": [ { "version": "1.2.0", "date": "2026-11-10", "url": "https://github.com/example/zither/releases/download/v1.2.0/ZITHER.fwsc",
          "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824", "size": 5 } ] } ] }"#;

    #[test]
    fn the_c_face_reads_a_catalog_and_checks_a_release() {
        unsafe {
            let h = fm1ntt_catalog_parse(FILE.as_ptr(), FILE.len());
            assert!(!h.is_null());
            assert_eq!(fm1ntt_catalog_count(h), 1);
            let mut b = [0 as c_char; 64];
            assert_eq!(fm1ntt_catalog_text(h, 0, 1, b.as_mut_ptr(), 64), 6);
            assert_eq!(CStr::from_ptr(b.as_ptr()).to_str().unwrap(), "ZITHER");
            assert_eq!(fm1ntt_catalog_text(h, 0, 1, b.as_mut_ptr(), 3), 6, "the full length is returned when cut");
            assert_eq!(CStr::from_ptr(b.as_ptr()).to_str().unwrap(), "ZI");
            assert_eq!(fm1ntt_catalog_text(h, 5, 1, b.as_mut_ptr(), 64), 0, "a bad index");
            assert_eq!((fm1ntt_catalog_tier(h, 0), fm1ntt_catalog_tier(h, 9)), (0, -1));
            assert_eq!((fm1ntt_catalog_release_count(h, 0), fm1ntt_catalog_latest(h, 0), fm1ntt_catalog_release_size(h, 0, 0)), (1, 0, 5));
            assert!(fm1ntt_catalog_release_text(h, 0, 0, 5, b.as_mut_ptr(), 64) == 64);
            assert_eq!(fm1ntt_catalog_matches(h, 0, 0, b"hello".as_ptr(), 5), 1);
            assert_eq!(fm1ntt_catalog_matches(h, 0, 0, b"hellO".as_ptr(), 5), 0);
            assert_eq!(fm1ntt_catalog_matches(h, 0, 0, std::ptr::null(), 5), 0);
            fm1ntt_catalog_free(h);
            // refusals: nothing, junk, and nulls everywhere
            assert!(fm1ntt_catalog_parse(std::ptr::null(), 0).is_null());
            assert!(fm1ntt_catalog_parse(b"{".as_ptr(), 1).is_null());
            assert!(!CStr::from_ptr(crate::capi::fm1ntt_error()).to_bytes().is_empty());
            assert_eq!((fm1ntt_catalog_count(std::ptr::null()), fm1ntt_catalog_tier(std::ptr::null(), 0), fm1ntt_catalog_latest(std::ptr::null(), 0)), (0, -1, -1));
            fm1ntt_catalog_free(std::ptr::null_mut());
        }
    }
}
