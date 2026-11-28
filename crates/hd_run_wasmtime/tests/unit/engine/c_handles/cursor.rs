//! A C provider of `Store`, whose `open` returns `Cursor` handles, in Rust
//! with the C ABI; `c_handles.rs` fills the two contract hashes.
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

static DESTROYED: AtomicU32 = AtomicU32::new(0);
static CREATED: AtomicU32 = AtomicU32::new(0);
static CALLS: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
pub struct Store {
    abi_version: u32,
    contract_hash: [u8; 16],
    create: Option<unsafe extern "C" fn() -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    open: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
}

#[repr(C)]
pub struct Cursor {
    abi_version: u32,
    contract_hash: [u8; 16],
    create: Option<unsafe extern "C" fn() -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    next: unsafe extern "C" fn(*mut c_void) -> i32,
}

unsafe extern "C" fn create() -> *mut c_void {
    CREATED.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(Box::new(100i32)).cast()
}
unsafe extern "C" fn destroy_store(this: *mut c_void) {
    drop(unsafe { Box::from_raw(this.cast::<i32>()) });
}
unsafe extern "C" fn open(this: *mut c_void) -> *mut c_void {
    let base = unsafe { &mut *this.cast::<i32>() };
    *base += 100;
    Box::into_raw(Box::new(*base)).cast()
}
unsafe extern "C" fn next(this: *mut c_void) -> i32 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    let n = unsafe { &mut *this.cast::<i32>() };
    *n += 1;
    *n
}
unsafe extern "C" fn destroy_cursor(this: *mut c_void) {
    drop(unsafe { Box::from_raw(this.cast::<i32>()) });
    DESTROYED.fetch_add(1, Ordering::SeqCst);
}

static STORE: Store = Store {
    abi_version: 1,
    contract_hash: __STORE__,
    create: Some(create),
    destroy: Some(destroy_store),
    open,
};

static CURSOR: Cursor = Cursor {
    abi_version: 1,
    contract_hash: __CURSOR__,
    create: None,
    destroy: Some(destroy_cursor),
    next,
};

#[unsafe(no_mangle)]
pub extern "C" fn hd_store_Store() -> *const Store {
    &STORE
}

#[unsafe(no_mangle)]
pub extern "C" fn hd_store_Cursor() -> *const Cursor {
    &CURSOR
}

/// How many cursors were destroyed, and how many `next` calls ran.
#[unsafe(no_mangle)]
pub extern "C" fn counts() -> u64 {
    u64::from(DESTROYED.load(Ordering::SeqCst)) << 32 | u64::from(CALLS.load(Ordering::SeqCst))
}

/// How many providers `create` made.
#[unsafe(no_mangle)]
pub extern "C" fn created() -> u32 {
    CREATED.load(Ordering::SeqCst)
}
