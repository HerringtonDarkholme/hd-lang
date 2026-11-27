//! A C provider of `Reader` whose `!` methods complete from a thread of
//! their own, in Rust with the C ABI; `c_providers.rs` fills the contract
//! hash from the header `hd build` writes, and `__READ__` and
//! `__READ_ABORT__` with `Some(read)` and `Some(abort)`, or `None` for a
//! library that leaves the slot empty. Each call's thread and its abort
//! slot share one lock per call, so no completion follows the abort slot's
//! return (`cli.host.c.token`).
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// The header's `hd_completions` and `struct hd_call`.
#[repr(C)]
pub struct Completions {
    complete_i32: unsafe extern "C" fn(*const Token, i32) -> bool,
    complete_i64: unsafe extern "C" fn(*const Token, i64) -> bool,
    complete_f64: unsafe extern "C" fn(*const Token, f64) -> bool,
    complete_bytes: unsafe extern "C" fn(*const Token, *const u8, usize) -> bool,
    complete_error: unsafe extern "C" fn(*const Token, i32) -> bool,
}

#[repr(C)]
pub struct Token {
    completions: *const Completions,
}

#[repr(C)]
pub struct Reader {
    abi_version: u32,
    contract_hash: [u8; 16],
    create: Option<unsafe extern "C" fn() -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    read: Option<unsafe extern "C" fn(*mut c_void, usize, u32)>,
    read_abort: Option<unsafe extern "C" fn(*mut c_void, usize, bool)>,
    total: unsafe extern "C" fn(*mut c_void, usize, i32),
    total_abort: unsafe extern "C" fn(*mut c_void, usize, bool),
    aborts: unsafe extern "C" fn(*mut c_void) -> i32,
    discarded: unsafe extern "C" fn(*mut c_void) -> i32,
}

/// One call: `true` once it is completed or aborted, and its wake.
type Call = Arc<(Mutex<bool>, Condvar)>;

#[derive(Default)]
struct State {
    calls: Mutex<HashMap<usize, Call>>,
    aborts: Mutex<(i32, i32)>,
}

/// Runs `complete` on the token after `wait`, on a thread of its own,
/// unless the call is aborted first. A completion the host refuses (it
/// returns false) leaves the value with the library.
fn later(this: *mut c_void, token: usize, wait: u32, complete: impl FnOnce(*const Token) -> bool + Send + 'static) {
    let state = unsafe { &*this.cast::<State>() };
    let call: Call = Arc::default();
    state.calls.lock().unwrap().insert(token, Arc::clone(&call));
    std::thread::spawn(move || {
        let (ended, wake) = &*call;
        let ended = ended.lock().unwrap();
        let (mut ended, _) = wake
            .wait_timeout_while(ended, Duration::from_millis(u64::from(wait)), |e| !*e)
            .unwrap();
        if !*ended {
            if !complete(token as *const Token) {
                eprintln!("completion refused");
            }
            *ended = true;
        }
    });
}

fn table(token: *const Token) -> &'static Completions {
    unsafe { &*(*token).completions }
}

unsafe extern "C" fn create() -> *mut c_void {
    Box::into_raw(Box::new(State::default())).cast()
}
unsafe extern "C" fn destroy(this: *mut c_void) {
    drop(unsafe { Box::from_raw(this.cast::<State>()) });
    eprintln!("reader destroyed");
}
unsafe extern "C" fn read(this: *mut c_void, token: usize, wait: u32) {
    later(this, token, wait, move |t| unsafe {
        if wait == 0 {
            (table(t).complete_error)(t, 5)
        } else {
            let bytes = b"hi".to_vec();
            (table(t).complete_bytes)(t, bytes.as_ptr(), bytes.len())
        }
    });
}
unsafe extern "C" fn total(this: *mut c_void, token: usize, n: i32) {
    later(this, token, 1, move |t| unsafe {
        (table(t).complete_i64)(t, i64::from(n) * 10_000_000_000)
    });
}
/// Ends the call of `token`: its thread completes nothing after this.
unsafe extern "C" fn abort(this: *mut c_void, token: usize, completed: bool) {
    let state = unsafe { &*this.cast::<State>() };
    if let Some(call) = state.calls.lock().unwrap().remove(&token) {
        let (ended, wake) = &*call;
        *ended.lock().unwrap() = true;
        wake.notify_all();
    }
    let mut aborts = state.aborts.lock().unwrap();
    aborts.0 += 1;
    aborts.1 += i32::from(completed);
    eprintln!("read aborted");
}
unsafe extern "C" fn aborts(this: *mut c_void) -> i32 {
    unsafe { &*this.cast::<State>() }.aborts.lock().unwrap().0
}
unsafe extern "C" fn discarded(this: *mut c_void) -> i32 {
    unsafe { &*this.cast::<State>() }.aborts.lock().unwrap().1
}

static READER: Reader = Reader {
    abi_version: 1,
    contract_hash: __HASH__,
    create: Some(create),
    destroy: Some(destroy),
    read: __READ__,
    read_abort: __READ_ABORT__,
    total,
    total_abort: abort,
    aborts,
    discarded,
};

#[unsafe(no_mangle)]
pub extern "C" fn hd_reader_Reader() -> *const Reader {
    &READER
}
