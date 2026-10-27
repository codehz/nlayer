//! Entropy for the wasm engine comes from the page.
//!
//! Native builds use getrandom's OS backend. The wasm32 target selects
//! `getrandom_backend = "custom"` (`.cargo/config.toml`); this hook is what
//! that backend calls. The page provides `__nlayer_random_fill` on the `env`
//! import module and fills the buffer from `crypto.getRandomValues`, at most
//! 65536 bytes per call. A non-zero return is "no randomness".
//!
//! Extracted from `nsh-js`'s `standard/random.rs`.

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn __nlayer_random_fill(ptr: *mut u8, len: usize) -> i32;
}

// getrandom 0.3 and 0.4 share this symbol. Only success is relied on: a
// failure from the page becomes [`getrandom::Error::UNSUPPORTED`].
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    ptr: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    // SAFETY: getrandom passes a writable range of exactly `len` bytes. The
    // page fills it synchronously before returning.
    if unsafe { __nlayer_random_fill(ptr, len) } == 0 {
        Ok(())
    } else {
        Err(getrandom::Error::UNSUPPORTED)
    }
}
