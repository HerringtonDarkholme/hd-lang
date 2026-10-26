//! The playground compiler's wasm exports (`crates/hd_web/DESIGN.md` §3.1).
//!
//! Owner decision 2026-10-12 (`crates/hd_web/DESIGN.md` §6, Q1): these four
//! hand-written exports carry `#[unsafe(no_mangle)]`, and this module is
//! the only place in the crate where `unsafe_code` is allowed. There are no
//! `unsafe` blocks and no raw-pointer dereferences: every buffer is a `Vec`
//! the crate owns, and JS reads and writes it through the exported memory.
//!
//! A compile from JS:
//!
//! 1. `p = hd_input(len)` sizes the request buffer; JS writes `len` request
//!    bytes (the format is in [`crate::wire`]) at `p`.
//! 2. `status = hd_compile()` runs the request: 0 compiled, 1 compiled with
//!    errors, 2 malformed request (or a REPL request that does not fit the
//!    session's state).
//! 3. `hd_output(kind)` and `hd_output_len(kind)` give the address and length
//!    of the response: kind 0 is the JSON text, kind 1 the program's bytes,
//!    or a REPL request's module (length 0 when there is none).
//!
//! An address stays valid until the next call. Memory may grow during any
//! call, so JS takes a fresh view of `memory.buffer` after each one.

use std::cell::RefCell;

use crate::Session;
use crate::wire;

struct State {
    session: Session,
    input: Vec<u8>,
    json: Vec<u8>,
    wasm: Vec<u8>,
}

// The input is borrowed only for each import call; Rust owns the result
// buffer and validates UTF-8 before making its String. No raw pointer is
// dereferenced in Rust. JS obtains fresh memory views after allocation.
#[link(wasm_import_module = "hd_web")]
unsafe extern "C" {
    safe fn nfc_utf8(input: *const u8, input_len: usize, output: *mut u8, capacity: usize)
    -> usize;
}

fn normalize_nfc(text: &str) -> String {
    let len = nfc_utf8(text.as_ptr(), text.len(), std::ptr::null_mut(), 0);
    let mut output = vec![0; len];
    let written = nfc_utf8(text.as_ptr(), text.len(), output.as_mut_ptr(), output.len());
    assert_eq!(
        written, len,
        "the NFC host changed the required buffer length"
    );
    String::from_utf8(output).expect("the NFC host returned invalid UTF-8")
}

impl Default for State {
    fn default() -> Self {
        hd_syntax::set_nfc_normalizer(normalize_nfc);
        Self {
            session: Session::new(),
            input: Vec::new(),
            json: Vec::new(),
            wasm: Vec::new(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Makes the request buffer `len` bytes long (zeroed) and returns its
/// address, for JS to write the request into.
#[unsafe(no_mangle)]
pub extern "C" fn hd_input(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|s| {
        s.input.clear();
        s.input.resize(len, 0);
        s.input.as_mut_ptr()
    })
}

/// Compiles the request buffer; the status is `wire::STATUS_*`.
#[unsafe(no_mangle)]
pub extern "C" fn hd_compile() -> u32 {
    STATE.with_borrow_mut(|s| {
        let State {
            session,
            input,
            json,
            wasm,
        } = s;
        let response = wire::respond(session, input);
        *json = response.json.into_bytes();
        *wasm = response.wasm;
        response.status
    })
}

/// The address of output `kind` (0 JSON, 1 wasm); null for another kind.
#[unsafe(no_mangle)]
pub extern "C" fn hd_output(kind: u32) -> *const u8 {
    STATE.with_borrow(|s| output(s, kind).map_or(std::ptr::null(), <[u8]>::as_ptr))
}

/// The length of output `kind` (0 JSON, 1 wasm); 0 for another kind.
#[unsafe(no_mangle)]
pub extern "C" fn hd_output_len(kind: u32) -> usize {
    STATE.with_borrow(|s| output(s, kind).map_or(0, <[u8]>::len))
}

fn output(s: &State, kind: u32) -> Option<&[u8]> {
    match kind {
        0 => Some(&s.json),
        1 => Some(&s.wasm),
        _ => None,
    }
}
