//! A C provider of `Store`, whose `open` returns `Cursor` handles, in Rust
//! with the C ABI; `c_providers.rs` fills the two contract hashes from the
//! header `hd build` writes.
use std::ffi::c_void;

#[repr(C)]
pub struct Store {
    abi_version: u32,
    contract_hash: [u8; 16],
    create: Option<unsafe extern "C" fn() -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    open: unsafe extern "C" fn(*mut c_void, i32) -> *mut c_void,
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
    Box::into_raw(Box::new(0i32)).cast()
}
unsafe extern "C" fn destroy_store(this: *mut c_void) {
    drop(unsafe { Box::from_raw(this.cast::<i32>()) });
    eprintln!("store destroyed");
}
unsafe extern "C" fn open(_: *mut c_void, from: i32) -> *mut c_void {
    Box::into_raw(Box::new(from)).cast()
}
unsafe extern "C" fn next(this: *mut c_void) -> i32 {
    let n = unsafe { &mut *this.cast::<i32>() };
    *n += 1;
    *n
}
unsafe extern "C" fn destroy_cursor(this: *mut c_void) {
    let n = unsafe { Box::from_raw(this.cast::<i32>()) };
    eprintln!("cursor {n} destroyed");
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
