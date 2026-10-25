//! Byte scans over a string's bytes in SIMD128 (#428,
//! docs/future-work/PERFORMANCE_ROADMAP.md §3.9). Each std primitive keeps
//! an hd reference body that defines its result; the helper here is the
//! body the linear backend links instead, only when a program reaches it.
//!
//! Each scan reads 16 bytes at a time with `v128.load` while 16 or more
//! remain, then the last 16 bytes of the string once, overlapping what it
//! already read, with the bits below the start shifted out. A string
//! shorter than 16 bytes is read byte by byte. No load reaches past the
//! string's last byte or before its view's first.

use crate::module::HEAP;
use wasm_encoder::{BlockType, Function, MemArg, ValType};

/// A 16-byte load from the heap, which takes any alignment.
const V128: MemArg = MemArg {
    offset: 0,
    align: 0,
    memory_index: HEAP,
};

/// A one-byte load from the heap.
const U8: MemArg = MemArg {
    offset: 0,
    align: 0,
    memory_index: HEAP,
};

/// The code of `Helper::BytesIndexJson`, `(at, len, from) -> index`: the
/// offset of the first byte at or after `from` among the `len` bytes at
/// address `at` that JSON must escape (below 0x20, `"` or `\`), else
/// `len`.
#[must_use]
pub fn json_index_code() -> Vec<u8> {
    let (at, len, i) = (0, 1, 2);
    let (mask, byte, v) = (3, 4, 5);
    let mut f = Function::new([(2, ValType::I32), (1, ValType::V128)]);
    let mut s = f.instructions();
    // The 16 lanes of `v` that need an escape, as a bit mask. `b ^ 2` is
    // below 0x21 exactly for a control byte or `"` (0x22 ^ 2 is 0x20);
    // `\` is its own compare.
    let lanes = |s: &mut wasm_encoder::InstructionSink<'_>| {
        s.local_get(v)
            .v128_const(i128::from_le_bytes([2; 16]))
            .v128_xor()
            .v128_const(i128::from_le_bytes([0x21; 16]))
            .i8x16_lt_u()
            .local_get(v)
            .v128_const(i128::from_le_bytes([b'\\'; 16]))
            .i8x16_eq()
            .v128_or()
            .i8x16_bitmask();
    };
    // Nothing at or after `len`.
    s.local_get(i)
        .local_get(len)
        .i32_ge_u()
        .if_(BlockType::Empty);
    s.local_get(len).return_().end();
    // 16 bytes at a time while 16 remain.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(len)
        .local_get(i)
        .i32_sub()
        .i32_const(16)
        .i32_lt_u()
        .br_if(1);
    s.local_get(at)
        .local_get(i)
        .i32_add()
        .v128_load(V128)
        .local_set(v);
    lanes(&mut s);
    s.local_tee(mask).if_(BlockType::Empty);
    s.local_get(i)
        .local_get(mask)
        .i32_ctz()
        .i32_add()
        .return_()
        .end();
    s.local_get(i).i32_const(16).i32_add().local_set(i).br(0);
    s.end().end();
    // Fewer than 16 remain, and at least one.
    s.local_get(len)
        .i32_const(16)
        .i32_ge_u()
        .if_(BlockType::Empty);
    // The string's last 16 bytes; the lanes before `i` shift out.
    s.local_get(at)
        .local_get(len)
        .i32_add()
        .i32_const(16)
        .i32_sub()
        .v128_load(V128)
        .local_set(v);
    lanes(&mut s);
    s.local_get(i)
        .i32_const(16)
        .i32_add()
        .local_get(len)
        .i32_sub()
        .i32_shr_u()
        .local_tee(mask)
        .if_(BlockType::Empty);
    s.local_get(i)
        .local_get(mask)
        .i32_ctz()
        .i32_add()
        .return_()
        .end();
    s.local_get(len).return_().end();
    // A string of fewer than 16 bytes: one byte at a time.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(i).local_get(len).i32_ge_u().br_if(1);
    s.local_get(at)
        .local_get(i)
        .i32_add()
        .i32_load8_u(U8)
        .local_tee(byte)
        .i32_const(0x20)
        .i32_lt_u()
        .local_get(byte)
        .i32_const(i32::from(b'"'))
        .i32_eq()
        .i32_or()
        .local_get(byte)
        .i32_const(i32::from(b'\\'))
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    s.local_get(i).return_().end();
    s.local_get(i).i32_const(1).i32_add().local_set(i).br(0);
    s.end().end();
    s.local_get(len).end();
    f.into_raw_body()
}
