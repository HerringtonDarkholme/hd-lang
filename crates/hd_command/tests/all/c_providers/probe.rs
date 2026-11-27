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
    many: unsafe extern "C" fn(*mut c_void, i8, u16, i32, i64, f32, f64, u8, i16, u32, usize, i8, u16, i32, i64, f32, f64, u8, i16, u32, usize, i8, u16, i32, i64, f32, f64, u8, i16, u32, usize, i8, u16, i32, i64, f32, f64, u8, i16, u32, usize) -> f64,
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

/// Forty arguments of mixed types, most of them on the stack: each
/// weighted by its position, so a misplaced one changes the sum.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn many(_: *mut c_void, a0: i8, a1: u16, a2: i32, a3: i64, a4: f32, a5: f64, a6: u8, a7: i16, a8: u32, a9: usize, a10: i8, a11: u16, a12: i32, a13: i64, a14: f32, a15: f64, a16: u8, a17: i16, a18: u32, a19: usize, a20: i8, a21: u16, a22: i32, a23: i64, a24: f32, a25: f64, a26: u8, a27: i16, a28: u32, a29: usize, a30: i8, a31: u16, a32: i32, a33: i64, a34: f32, a35: f64, a36: u8, a37: i16, a38: u32, a39: usize) -> f64 {
    1.0 * a0 as f64 + 2.0 * a1 as f64 + 3.0 * a2 as f64 + 4.0 * a3 as f64 + 5.0 * a4 as f64 + 6.0 * a5 as f64 + 7.0 * a6 as f64 + 8.0 * a7 as f64 + 9.0 * a8 as f64 + 10.0 * a9 as f64 + 11.0 * a10 as f64 + 12.0 * a11 as f64 + 13.0 * a12 as f64 + 14.0 * a13 as f64 + 15.0 * a14 as f64 + 16.0 * a15 as f64 + 17.0 * a16 as f64 + 18.0 * a17 as f64 + 19.0 * a18 as f64 + 20.0 * a19 as f64 + 21.0 * a20 as f64 + 22.0 * a21 as f64 + 23.0 * a22 as f64 + 24.0 * a23 as f64 + 25.0 * a24 as f64 + 26.0 * a25 as f64 + 27.0 * a26 as f64 + 28.0 * a27 as f64 + 29.0 * a28 as f64 + 30.0 * a29 as f64 + 31.0 * a30 as f64 + 32.0 * a31 as f64 + 33.0 * a32 as f64 + 34.0 * a33 as f64 + 35.0 * a34 as f64 + 36.0 * a35 as f64 + 37.0 * a36 as f64 + 38.0 * a37 as f64 + 39.0 * a38 as f64 + 40.0 * a39 as f64
}

static TABLE: Probe = Probe {
    abi_version: __ABI__,
    contract_hash: __HASH__,
    create, destroy, ints, floats, half, flag, narrow, wide, text, sum, fill, count, many,
};

#[unsafe(export_name = "__ENTRY__")]
pub extern "C" fn entry() -> *const Probe {
    &TABLE
}
