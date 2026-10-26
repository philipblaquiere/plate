//! The configuration evaluator compiled to WebAssembly, so Nickel runs in the browser.
//!
//! The ABI is deliberately tiny, so it needs no wasm-bindgen or wasm-pack, just
//! `cargo build --target wasm32-wasip1`. The WASI target (rather than `wasm32-unknown-unknown`)
//! is what lets Nickel resolve `import "other.ncl"`: its import paths must be absolute, and on
//! `wasm32-unknown-unknown` no path is. The browser gives the module a stub WASI with no files
//! (web/src/engine/wasi.ts); imports are found among the files passed in.
//!
//! - `plate_alloc(len) -> ptr` / `plate_free(ptr, len)`: memory for passing strings in.
//! - `plate_evaluate(ptr, len) -> result`: evaluate a workspace, given as UTF-8 JSON
//!   `{ "main": "hotel.ncl", "files": { "hotel.ncl": "...", "plans/room.ncl": "..." } }` (the
//!   main file may `import` the others), or plain Nickel source for a single file.
//! - `plate_schema_nickel() -> result`: the Nickel contract generated from the JSON Schema.
//!
//! A result is a buffer holding a 4-byte little-endian length followed by that many bytes of
//! UTF-8 JSON. The caller frees it with `plate_free(result, 4 + length)`.
//!
//! `plate_evaluate` returns `{ "ok": true, "config": {...}, "sources": [...] }` or
//! `{ "ok": false, "diagnostics": [...] }`. `sources` maps each value of `config` (by JSON
//! pointer) to its text and file, so the editor can write drawing edits back. Diagnostics name
//! the file they're in.

use std::alloc::{alloc, dealloc, Layout};

use serde_json::json;

/// Allocate `len` bytes for the caller to write into.
#[no_mangle]
pub extern "C" fn plate_alloc(len: usize) -> *mut u8 {
    // SAFETY: the layout has a non-zero size and alignment 1.
    unsafe { alloc(Layout::from_size_align(len.max(1), 1).expect("layout")) }
}

/// Free memory from `plate_alloc`, or a result buffer (pass `4 + length`).
///
/// # Safety
/// `ptr` must come from this module's allocator with the same `len`.
#[no_mangle]
pub unsafe extern "C" fn plate_free(ptr: *mut u8, len: usize) {
    dealloc(ptr, Layout::from_size_align(len.max(1), 1).expect("layout"));
}

/// Evaluate Nickel source against the plate schema.
///
/// # Safety
/// `ptr..ptr+len` must be readable memory, e.g. from `plate_alloc`.
#[no_mangle]
pub unsafe extern "C" fn plate_evaluate(ptr: *const u8, len: usize) -> *mut u8 {
    let bytes = std::slice::from_raw_parts(ptr, len);
    let response = match std::str::from_utf8(bytes) {
        Ok(source) => evaluate_json(source),
        Err(_) => {
            json!({ "ok": false, "diagnostics": [{ "severity": "error", "message": "the file is not valid UTF-8" }] })
        }
    };
    result(response.to_string())
}

/// The Nickel contract generated from the JSON Schema (for debugging and docs).
#[no_mangle]
pub extern "C" fn plate_schema_nickel() -> *mut u8 {
    result(match plate_config::compiled_schema() {
        Ok(compiled) => compiled.nickel.clone(),
        Err(e) => format!("# schema error: {e}"),
    })
}

#[derive(serde::Deserialize)]
struct Request {
    main: String,
    files: std::collections::BTreeMap<String, String>,
}

/// The JSON response for a request: a workspace (JSON), or a single file's source.
pub fn evaluate_json(input: &str) -> serde_json::Value {
    let checked = match serde_json::from_str::<Request>(input) {
        Ok(req) => plate_config::check_files(plate_config::Workspace { main: &req.main, files: &req.files }),
        Err(_) => plate_config::check(input),
    };
    match checked {
        Ok((_, evaluation)) => {
            json!({ "ok": true, "config": evaluation.value, "sources": evaluation.sources })
        }
        Err(diagnostics) => json!({ "ok": false, "diagnostics": diagnostics }),
    }
}

fn result(text: String) -> *mut u8 {
    let body = text.into_bytes();
    let ptr = plate_alloc(body.len() + 4);
    // SAFETY: `ptr` points to `4 + body.len()` freshly allocated bytes.
    unsafe {
        std::ptr::copy_nonoverlapping((body.len() as u32).to_le_bytes().as_ptr(), ptr, 4);
        std::ptr::copy_nonoverlapping(body.as_ptr(), ptr.add(4), body.len());
    }
    ptr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_abi() {
        let source = b"{ start = { mode = 'spine, points = [[0, 0]] } }";
        unsafe {
            let input = plate_alloc(source.len());
            std::ptr::copy_nonoverlapping(source.as_ptr(), input, source.len());
            let out = plate_evaluate(input, source.len());
            plate_free(input, source.len());
            let len = u32::from_le_bytes(std::slice::from_raw_parts(out, 4).try_into().unwrap()) as usize;
            let text = std::str::from_utf8(std::slice::from_raw_parts(out.add(4), len)).unwrap().to_owned();
            plate_free(out, len + 4);
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(v["ok"], true);
            assert_eq!(v["config"]["name"], "Untitled plate");
            assert!(v["sources"].as_array().unwrap().iter().any(|s| s["pointer"] == "/start/points/0/0"));
        }
    }
}
