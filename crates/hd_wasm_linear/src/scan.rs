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

/// The code of `Helper::BytesIndex`, `(at, len, from, byte) -> index`: the
/// offset of the first byte equal to `byte` at or after `from` among the
/// `len` bytes at address `at`, else `len`.
#[must_use]
pub fn index_code() -> Vec<u8> {
    let (at, len, i, byte) = (0, 1, 2, 3);
    let (mask, cur, needle) = (4, 5, 6);
    let mut f = Function::new([(2, ValType::I32), (1, ValType::V128)]);
    let mut s = f.instructions();
    // With 16 bytes on the stack: the lanes that equal `byte`, as a bit
    // mask.
    let lanes = |s: &mut wasm_encoder::InstructionSink<'_>| {
        s.local_get(needle).i8x16_eq().i8x16_bitmask();
    };
    // Nothing at or after `len`.
    s.local_get(i)
        .local_get(len)
        .i32_ge_u()
        .if_(BlockType::Empty);
    s.local_get(len).return_().end();
    s.local_get(byte).i8x16_splat().local_set(needle);
    // 16 bytes at a time while 16 remain.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(len)
        .local_get(i)
        .i32_sub()
        .i32_const(16)
        .i32_lt_u()
        .br_if(1);
    s.local_get(at).local_get(i).i32_add().v128_load(V128);
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
        .v128_load(V128);
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
    s.local_get(byte).i32_const(0xff).i32_and().local_set(byte);
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(i).local_get(len).i32_ge_u().br_if(1);
    s.local_get(at)
        .local_get(i)
        .i32_add()
        .i32_load8_u(U8)
        .local_set(cur);
    s.local_get(cur)
        .local_get(byte)
        .i32_eq()
        .if_(BlockType::Empty);
    s.local_get(i).return_().end();
    s.local_get(i).i32_const(1).i32_add().local_set(i).br(0);
    s.end().end();
    s.local_get(len).end();
    f.into_raw_body()
}

/// The code of `Helper::BytesMismatch`, `(left, left_len, right, right_len)
/// -> index`: the offset of the first byte where the two views differ,
/// else the shorter length.
#[must_use]
pub fn mismatch_code() -> Vec<u8> {
    let (left, left_len, right, right_len) = (0, 1, 2, 3);
    let (i, n, mask) = (4, 5, 6);
    let mut f = Function::new([(3, ValType::I32)]);
    let mut s = f.instructions();
    // The lanes of the 16 bytes at `left + off` and `right + off` that
    // differ, as a bit mask.
    let lanes = |s: &mut wasm_encoder::InstructionSink<'_>,
                 off: &dyn Fn(&mut wasm_encoder::InstructionSink<'_>)| {
        s.local_get(left);
        off(s);
        s.i32_add().v128_load(V128).local_get(right);
        off(s);
        s.i32_add()
            .v128_load(V128)
            .i8x16_eq()
            .i8x16_bitmask()
            .i32_const(0xffff)
            .i32_xor();
    };
    // `n`, the shorter length.
    s.local_get(left_len)
        .local_get(right_len)
        .local_get(left_len)
        .local_get(right_len)
        .i32_lt_u()
        .select()
        .local_set(n);
    // 16 bytes at a time while 16 remain.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(n)
        .local_get(i)
        .i32_sub()
        .i32_const(16)
        .i32_lt_u()
        .br_if(1);
    lanes(&mut s, &|s| {
        s.local_get(i);
    });
    s.local_tee(mask).if_(BlockType::Empty);
    s.local_get(i)
        .local_get(mask)
        .i32_ctz()
        .i32_add()
        .return_()
        .end();
    s.local_get(i).i32_const(16).i32_add().local_set(i).br(0);
    s.end().end();
    // Nothing left.
    s.local_get(i).local_get(n).i32_ge_u().if_(BlockType::Empty);
    s.local_get(n).return_().end();
    // Fewer than 16 remain, and at least one.
    s.local_get(n)
        .i32_const(16)
        .i32_ge_u()
        .if_(BlockType::Empty);
    // The last 16 bytes of the shorter length; the lanes before `i` shift
    // out.
    lanes(&mut s, &|s| {
        s.local_get(n).i32_const(16).i32_sub();
    });
    s.local_get(i)
        .i32_const(16)
        .i32_add()
        .local_get(n)
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
    s.local_get(n).return_().end();
    // Under 16 bytes: one byte at a time.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(i).local_get(n).i32_ge_u().br_if(1);
    s.local_get(left)
        .local_get(i)
        .i32_add()
        .i32_load8_u(U8)
        .local_get(right)
        .local_get(i)
        .i32_add()
        .i32_load8_u(U8)
        .i32_ne()
        .if_(BlockType::Empty);
    s.local_get(i).return_().end();
    s.local_get(i).i32_const(1).i32_add().local_set(i).br(0);
    s.end().end();
    s.local_get(n).end();
    f.into_raw_body()
}

/// The code of `Helper::Utf8ValidLen`, `(list) -> len`: the length of the
/// longest prefix of a `List[u8]`'s elements that is well-formed UTF-8 and
/// ends at a scalar boundary, so the element count when all are valid.
///
/// ASCII passes 64 bytes per step while 64 remain (four `v128.load`s
/// or-ed, one `i8x16.bitmask`), then 16. In a block with a byte of 128 or
/// more, the bytes before it are ASCII, so the scan jumps to it and runs
/// the scalar decoder of std's reference body from there: up to the next
/// ASCII byte, or to the end once fewer than 16 bytes remain. Every return
/// is the offset of a sequence's first byte, as in the reference, and no
/// load reaches past the last element.
#[must_use]
pub fn utf8_valid_len_code() -> Vec<u8> {
    let list = 0;
    let (at, len, i, mask, lead, point, byte) = (1, 2, 3, 4, 5, 6, 7);
    let mut f = Function::new([(7, ValType::I32)]);
    let mut s = f.instructions();
    let ascii_at = |s: &mut wasm_encoder::InstructionSink<'_>, off: u64| {
        s.local_get(at).local_get(i).i32_add().v128_load(MemArg {
            offset: off,
            ..V128
        });
    };
    // Return `i` when the value on the stack is not 0.
    let fail_if = |s: &mut wasm_encoder::InstructionSink<'_>| {
        s.if_(BlockType::Empty).local_get(i).return_().end();
    };
    s.local_get(list)
        .i32_load(MemArg {
            offset: crate::rt::LIST_LEN,
            align: 2,
            memory_index: HEAP,
        })
        .local_set(len);
    s.local_get(list)
        .i32_const(crate::rt::LIST_ELEMS.cast_signed())
        .i32_add()
        .local_set(at);
    s.loop_(BlockType::Empty);
    // 64 ASCII bytes at a time while 64 remain: the high bits of four
    // blocks or-ed together.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(len)
        .local_get(i)
        .i32_sub()
        .i32_const(64)
        .i32_lt_u()
        .br_if(1);
    ascii_at(&mut s, 0);
    ascii_at(&mut s, 16);
    s.v128_or();
    ascii_at(&mut s, 32);
    s.v128_or();
    ascii_at(&mut s, 48);
    s.v128_or().i8x16_bitmask().br_if(1);
    s.local_get(i).i32_const(64).i32_add().local_set(i).br(0);
    s.end().end();
    // Then 16 at a time while 16 remain; in a block that is not all
    // ASCII, the bytes before the first one that is not are.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(len)
        .local_get(i)
        .i32_sub()
        .i32_const(16)
        .i32_lt_u()
        .br_if(1);
    ascii_at(&mut s, 0);
    s.i8x16_bitmask().local_tee(mask).if_(BlockType::Empty);
    s.local_get(i)
        .local_get(mask)
        .i32_ctz()
        .i32_add()
        .local_set(i)
        .br(2)
        .end();
    s.local_get(i).i32_const(16).i32_add().local_set(i).br(0);
    s.end().end();
    // The scalar decoder, from a scalar boundary: to the end once fewer
    // than 16 bytes remain, else up to the next ASCII byte.
    s.loop_(BlockType::Empty);
    s.local_get(i)
        .local_get(len)
        .i32_ge_u()
        .if_(BlockType::Empty);
    s.local_get(len).return_().end();
    s.local_get(at)
        .local_get(i)
        .i32_add()
        .i32_load8_u(U8)
        .local_tee(lead)
        .i32_const(0x80)
        .i32_lt_u()
        .if_(BlockType::Empty);
    s.local_get(i).i32_const(1).i32_add().local_set(i);
    s.local_get(len)
        .local_get(i)
        .i32_sub()
        .i32_const(16)
        .i32_ge_u()
        .br_if(2);
    s.else_();
    // Return `i` unless `n` bytes from `i` are within `len`.
    let fits = |s: &mut wasm_encoder::InstructionSink<'_>, n: i32| {
        s.local_get(i)
            .i32_const(n)
            .i32_add()
            .local_get(len)
            .i32_gt_u();
        fail_if(s);
    };
    // Continuation byte `n` into `point`, returning `i` unless it is
    // 10xxxxxx.
    let next = |s: &mut wasm_encoder::InstructionSink<'_>, n: u64| {
        s.local_get(at)
            .local_get(i)
            .i32_add()
            .i32_load8_u(MemArg { offset: n, ..U8 })
            .local_tee(byte)
            .i32_const(0xc0)
            .i32_and()
            .i32_const(0x80)
            .i32_ne()
            .if_(BlockType::Empty)
            .local_get(i)
            .return_()
            .end();
        s.local_get(point)
            .i32_const(6)
            .i32_shl()
            .local_get(byte)
            .i32_const(0x3f)
            .i32_and()
            .i32_or()
            .local_set(point);
    };
    s.local_get(lead)
        .i32_const(0xe0)
        .i32_lt_u()
        .if_(BlockType::Empty);
    // Two bytes: C0 and C1 lead nothing, and C2 and up never give an
    // overlong scalar.
    s.local_get(lead).i32_const(0xc2).i32_lt_u();
    fail_if(&mut s);
    fits(&mut s, 2);
    next(&mut s, 1);
    s.local_get(i).i32_const(2).i32_add().local_set(i);
    s.else_();
    s.local_get(lead)
        .i32_const(0xf0)
        .i32_lt_u()
        .if_(BlockType::Empty);
    // Three bytes: neither overlong nor a surrogate.
    fits(&mut s, 3);
    s.local_get(lead).i32_const(0x0f).i32_and().local_set(point);
    next(&mut s, 1);
    next(&mut s, 2);
    s.local_get(point).i32_const(0x800).i32_lt_u();
    fail_if(&mut s);
    s.local_get(point)
        .i32_const(0xd800)
        .i32_sub()
        .i32_const(0x800)
        .i32_lt_u();
    fail_if(&mut s);
    s.local_get(i).i32_const(3).i32_add().local_set(i);
    s.else_();
    // Four bytes: F5 to FF lead nothing; neither overlong nor past
    // U+10FFFF.
    s.local_get(lead).i32_const(0xf4).i32_gt_u();
    fail_if(&mut s);
    fits(&mut s, 4);
    s.local_get(lead).i32_const(0x07).i32_and().local_set(point);
    next(&mut s, 1);
    next(&mut s, 2);
    next(&mut s, 3);
    s.local_get(point)
        .i32_const(0x1_0000)
        .i32_sub()
        .i32_const(0x10_0000)
        .i32_ge_u();
    fail_if(&mut s);
    s.local_get(i).i32_const(4).i32_add().local_set(i);
    s.end().end();
    s.end();
    s.br(0);
    s.end();
    s.end();
    s.local_get(len).end();
    f.into_raw_body()
}

/// The code of `Helper::BytesIndexPair`, `(at, len, from, first, last, gap)
/// -> index`: the offset `i` of the first byte at or after `from` among the
/// `len` bytes at address `at` with `i + gap < len`, byte `i` equal to
/// `first` and byte `i + gap` equal to `last`, else `len`.
#[must_use]
pub fn index_pair_code() -> Vec<u8> {
    let (at, len, i, first, last, gap) = (0, 1, 2, 3, 4, 5);
    let (mask, limit, cur) = (6, 7, 8);
    let (first_lanes, last_lanes) = (9, 10);
    let mut f = Function::new([(3, ValType::I32), (2, ValType::V128)]);
    let mut s = f.instructions();
    // With nothing at or after `i` that leaves room for `gap`: no match.
    s.local_get(i)
        .local_get(len)
        .i32_ge_u()
        .if_(BlockType::Empty);
    s.local_get(len).return_().end();
    s.local_get(gap)
        .local_get(len)
        .local_get(i)
        .i32_sub()
        .i32_ge_u()
        .if_(BlockType::Empty);
    s.local_get(len).return_().end();
    // Candidates start before `limit`: `i + gap < len`.
    s.local_get(len).local_get(gap).i32_sub().local_set(limit);
    s.local_get(first).i8x16_splat().local_set(first_lanes);
    s.local_get(last).i8x16_splat().local_set(last_lanes);
    // With the candidate offset `i` (or the last 16 candidates) on the
    // stack: the lanes where both bytes match, as a bit mask. Loads start
    // at `i` and `i + gap` and end by `limit + 15` and `len`.
    let lanes = |s: &mut wasm_encoder::InstructionSink<'_>, base: u32| {
        s.local_get(at)
            .local_get(base)
            .i32_add()
            .v128_load(V128)
            .local_get(first_lanes)
            .i8x16_eq();
        s.local_get(at)
            .local_get(base)
            .i32_add()
            .local_get(gap)
            .i32_add()
            .v128_load(V128)
            .local_get(last_lanes)
            .i8x16_eq();
        s.v128_and().i8x16_bitmask();
    };
    // 16 candidates at a time while 16 remain.
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(limit)
        .local_get(i)
        .i32_sub()
        .i32_const(16)
        .i32_lt_u()
        .br_if(1);
    lanes(&mut s, i);
    s.local_tee(mask).if_(BlockType::Empty);
    s.local_get(i)
        .local_get(mask)
        .i32_ctz()
        .i32_add()
        .return_()
        .end();
    s.local_get(i).i32_const(16).i32_add().local_set(i).br(0);
    s.end().end();
    // Fewer than 16 candidates remain, and at least one.
    s.local_get(limit)
        .i32_const(16)
        .i32_ge_u()
        .if_(BlockType::Empty);
    // The last 16 candidates, from `limit - 16`; the lanes before `i`
    // shift out.
    s.local_get(limit).i32_const(16).i32_sub().local_set(cur);
    lanes(&mut s, cur);
    s.local_get(i)
        .i32_const(16)
        .i32_add()
        .local_get(limit)
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
    // Fewer than 16 candidates in all: one at a time.
    s.local_get(first)
        .i32_const(0xff)
        .i32_and()
        .local_set(first);
    s.local_get(last).i32_const(0xff).i32_and().local_set(last);
    s.block(BlockType::Empty).loop_(BlockType::Empty);
    s.local_get(i).local_get(limit).i32_ge_u().br_if(1);
    s.local_get(at)
        .local_get(i)
        .i32_add()
        .i32_load8_u(U8)
        .local_get(first)
        .i32_eq()
        .if_(BlockType::Empty);
    s.local_get(at)
        .local_get(i)
        .i32_add()
        .local_get(gap)
        .i32_add()
        .i32_load8_u(U8)
        .local_get(last)
        .i32_eq()
        .if_(BlockType::Empty);
    s.local_get(i).return_().end().end();
    s.local_get(i).i32_const(1).i32_add().local_set(i).br(0);
    s.end().end();
    s.local_get(len).end();
    f.into_raw_body()
}
