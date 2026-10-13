//! The playground compiler's wasm exports (playground-wasm.md §3.1).
//!
//! Owner decision 2026-10-12 (playground-wasm.md §6, Q1): these four
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
//!    or a REPL request's module (length 0 when there is none), kind 2 the
//!    program's WebAssembly text (length 0 unless the request's goal was
//!    wat).
//!
//! An address stays valid until the next call. Memory may grow during any
//! call, so JS takes a fresh view of `memory.buffer` after each one.

use std::cell::RefCell;

use crate::Session;
use crate::wire;

#[derive(Default)]
struct State {
    session: Session,
    input: Vec<u8>,
    json: Vec<u8>,
    wasm: Vec<u8>,
    wat: Vec<u8>,
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
            wat,
        } = s;
        let response = wire::respond(session, input);
        *json = response.json.into_bytes();
        *wasm = response.wasm;
        *wat = response.wat.into_bytes();
        response.status
    })
}

/// The address of output `kind` (0 JSON, 1 wasm, 2 WAT); null for another kind.
#[unsafe(no_mangle)]
pub extern "C" fn hd_output(kind: u32) -> *const u8 {
    STATE.with_borrow(|s| output(s, kind).map_or(std::ptr::null(), <[u8]>::as_ptr))
}

/// The length of output `kind` (0 JSON, 1 wasm, 2 WAT); 0 for another kind.
#[unsafe(no_mangle)]
pub extern "C" fn hd_output_len(kind: u32) -> usize {
    STATE.with_borrow(|s| output(s, kind).map_or(0, <[u8]>::len))
}

fn output(s: &State, kind: u32) -> Option<&[u8]> {
    match kind {
        0 => Some(&s.json),
        1 => Some(&s.wasm),
        2 => Some(&s.wat),
        _ => None,
    }
}
