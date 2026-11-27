//! A C provider of `Probe` in Rust, built by `c_providers.rs` with its
//! placeholders filled: the table's ABI version, contract hash and entry
//! symbol.
use std::ffi::c_void;

#[repr(C)]
pub struct Probe {
    abi_version: u32,
    contract_hash: [u8; 16],
    create: unsafe extern "C" fn() -> *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    ints: unsafe extern "C" fn(*mut c_void, i8, i16, i32, i64, u8, u16, u32, u64, usize) -> i64,
    floats: unsafe extern "C" fn(*mut c_void, f64, f64, f64, f64, f64, f64, f64, f64, f32, f64) -> f64,
    half: unsafe extern "C" fn(*mut c_void, f32) -> f32,
    flag: unsafe extern "C" fn(*mut c_void, bool) -> bool,
    narrow: unsafe extern "C" fn(*mut c_void) -> i8,
    wide: unsafe extern "C" fn(*mut c_void) -> u64,
    text: unsafe extern "C" fn(*mut c_void, *const u8, usize) -> usize,
    sum: unsafe extern "C" fn(*mut c_void, *const u8, usize) -> u32,
    fill: unsafe extern "C" fn(*mut c_void, *mut i32, usize, *mut u64, *const f64),
    count: unsafe extern "C" fn(*mut c_void) -> u32,
}

unsafe extern "C" fn create() -> *mut c_void {
    Box::into_raw(Box::new(0u32)).cast()
}
unsafe extern "C" fn destroy(this: *mut c_void) {
    drop(unsafe { Box::from_raw(this.cast::<u32>()) });
    eprintln!("probe destroyed");
}
unsafe extern "C" fn ints(_: *mut c_void, a: i8, b: i16, c: i32, d: i64, e: u8, f: u16, g: u32, h: u64, n: usize) -> i64 {
    i64::from(a) * 100_000_000 + i64::from(b) * 10_000_000 + i64::from(c) * 1_000_000 + d * 100_000
        + i64::from(e) * 10_000 + i64::from(f) * 1_000 + i64::from(g / 1_000_000_000) * 100 + h as i64 * 10 + n as i64
}
unsafe extern "C" fn floats(_: *mut c_void, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, i: f32, j: f64) -> f64 {
    a + b + c + d + e + f + g + h + f64::from(i) * 100.0 + j * 1000.0
}
unsafe extern "C" fn half(_: *mut c_void, x: f32) -> f32 { x / 2.0 }
unsafe extern "C" fn flag(_: *mut c_void, on: bool) -> bool { !on }
unsafe extern "C" fn narrow(_: *mut c_void) -> i8 { -7 }
unsafe extern "C" fn wide(_: *mut c_void) -> u64 { u64::MAX - 1 }
unsafe extern "C" fn text(_: *mut c_void, s: *const u8, len: usize) -> usize {
    let bytes = unsafe { std::slice::from_raw_parts(s, len) };
    std::str::from_utf8(bytes).map_or(0, |t| t.chars().count() * 100 + len)
}
unsafe extern "C" fn sum(_: *mut c_void, xs: *const u8, len: usize) -> u32 {
    unsafe { std::slice::from_raw_parts(xs, len) }.iter().map(|&x| u32::from(x)).sum()
}
unsafe extern "C" fn fill(_: *mut c_void, into: *mut i32, len: usize, at: *mut u64, v: *const f64) {
    let into = unsafe { std::slice::from_raw_parts_mut(into, len) };
    for (k, x) in into.iter_mut().enumerate() {
        *x = k as i32 + 1;
    }
    unsafe { *at += 41 + *v as u64 };
}
unsafe extern "C" fn count(this: *mut c_void) -> u32 {
    let n = unsafe { &mut *this.cast::<u32>() };
    *n += 1;
    *n
}

static TABLE: Probe = Probe {
    abi_version: __ABI__,
    contract_hash: __HASH__,
    create, destroy, ints, floats, half, flag, narrow, wide, text, sum, fill, count,
};

#[unsafe(export_name = "__ENTRY__")]
pub extern "C" fn entry() -> *const Probe {
    &TABLE
}
