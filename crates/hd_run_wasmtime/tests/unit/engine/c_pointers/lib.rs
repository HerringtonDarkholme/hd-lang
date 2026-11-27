//! A C provider of `Cell` whose `bump(int64_t *at)` and
//! `sum(const double *xs, size_t len)` count their calls, in Rust with the
//! C ABI; `c_pointers.rs` fills the contract hash.
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

static CALLS: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
pub struct Cell {
    abi_version: u32,
    contract_hash: [u8; 16],
    create: Option<unsafe extern "C" fn() -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    bump: unsafe extern "C" fn(*mut c_void, *mut i64) -> i32,
    sum: unsafe extern "C" fn(*mut c_void, *const f64, usize) -> i32,
}

unsafe extern "C" fn create() -> *mut c_void {
    Box::into_raw(Box::new(0u8)).cast()
}
unsafe extern "C" fn destroy(this: *mut c_void) {
    drop(unsafe { Box::from_raw(this.cast::<u8>()) });
}
unsafe extern "C" fn bump(_: *mut c_void, at: *mut i64) -> i32 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    unsafe { *at += 1 };
    0
}
unsafe extern "C" fn sum(_: *mut c_void, xs: *const f64, len: usize) -> i32 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    unsafe { std::slice::from_raw_parts(xs, len) }.iter().sum::<f64>() as i32
}

static CELL: Cell = Cell {
    abi_version: 1,
    contract_hash: __HASH__,
    create: Some(create),
    destroy: Some(destroy),
    bump,
    sum,
};

#[unsafe(no_mangle)]
pub extern "C" fn hd_cell_Cell() -> *const Cell {
    &CELL
}

/// How many slot calls ran.
#[unsafe(no_mangle)]
pub extern "C" fn calls() -> u32 {
    CALLS.load(Ordering::SeqCst)
}
