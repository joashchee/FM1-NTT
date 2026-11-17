//! The web build of the core: a wasm32 `cdylib` that exports every `fm1ntt_*` function of the C API (`core/fm1ntt.h`) and two more,
//! `fm1ntt_wasm_alloc` and `fm1ntt_wasm_free`, so the page can hand the module a firmware without growing its memory behind the allocator's back.
//!
//! The module has one import, `env.fm1_jit_compile`: the vendored core compiles hot code to small WebAssembly modules and asks its host to
//! instantiate them (`web/public/fm1ntt.js` does). It also needs the function table exported and growable (`.cargo/config.toml`).
//! Firmware is untrusted input: `fm1ntt_load_package` returns an error, never a crash, and every size here is bounded.

pub use fm1ntt::capi::*;

/// The largest block the page may ask for: a firmware package is about 0.6 MiB, so 64 MiB is generous and bounded (CLAUDE.md rule 5).
const MAX_ALLOC: usize = 64 << 20;

/// A zeroed block of `len` bytes (at least 1) for the host to fill; null when `len` is over 64 MiB. Free it with `fm1ntt_wasm_free` and the same `len`.
#[no_mangle]
pub extern "C" fn fm1ntt_wasm_alloc(len: usize) -> *mut u8 {
    if len > MAX_ALLOC {
        return std::ptr::null_mut();
    }
    Box::leak(vec![0u8; len.max(1)].into_boxed_slice()).as_mut_ptr()
}

/// Frees a block from `fm1ntt_wasm_alloc`.
///
/// # Safety
/// `p` came from `fm1ntt_wasm_alloc(len)` with this same `len` and is not used again; null is ignored.
#[no_mangle]
pub unsafe extern "C" fn fm1ntt_wasm_free(p: *mut u8, len: usize) {
    if !p.is_null() && len <= MAX_ALLOC {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len.max(1))));
    }
}
