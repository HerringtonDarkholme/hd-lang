//! `Helper::RemF64`: floating `%` (`expr.float.remainder-truncated`,
//! `expr.float.remainder-special`), written in Wasm because the instruction
//! set has no remainder. It is C `fmod`: the exact remainder of division
//! truncated toward zero, with the dividend's sign. `a - b * trunc(a / b)`
//! is not that, since the quotient and the product round.
//!
//! The method is shift-and-subtract on the significands, as musl's `fmod`
//! does. Both operands are brought to a 53-bit significand with an
//! unbiased exponent (subnormals are normalized with `clz`). While the
//! dividend's exponent is above the divisor's, the divisor's significand
//! is subtracted when it fits and the difference is doubled; every step is
//! integer arithmetic, so the result is exact. The loop runs at most as
//! many times as the exponents differ, which is 2045 at most.
//!
//! An `f32` `%` widens both operands, calls this, and narrows the result:
//! the remainder of two `f32` values is an `f32` value, so the narrowing
//! is exact.

use crate::asm::Asm;
use crate::{Code, VT};

/// The low 52 bits of a double: its stored fraction.
const FRAC: i64 = (1 << 52) - 1;
/// The implicit leading bit of a normal significand.
const HIDDEN: i64 = 1 << 52;
/// `0xFFE0_0000_0000_0000` as a signed value: a double's bits shifted left
/// by one are above this exactly for a NaN, and at least this for an
/// infinity or a NaN.
const INF2: i64 = -(1 << 53);

/// Sets `e` and `m` from the bits `bits` of a finite nonzero double: `m` is
/// the significand with its leading one at bit 52, and `value = m * 2^(e -
/// 1075)`, so a subnormal gets an exponent below one.
fn normalize(a: &mut Asm, bits: u32, e: u32, m: u32, shift: u32) {
    a.get(bits);
    a.i64(52);
    a.s().i64_shr_u().i32_wrap_i64();
    a.i32(0x7ff);
    a.s().i32_and();
    a.set(e);
    a.get(bits);
    a.i64(FRAC);
    a.s().i64_and();
    a.set(m);
    a.get(e);
    a.s().i32_eqz();
    a.if_();
    // The leading one of the fraction moves up to bit 52.
    a.get(m);
    a.s().i64_clz();
    a.i64(11);
    a.s().i64_sub();
    a.set(shift);
    a.get(m);
    a.get(shift);
    a.s().i64_shl();
    a.set(m);
    a.i32(1);
    a.get(shift);
    a.s().i32_wrap_i64().i32_sub();
    a.set(e);
    a.else_();
    a.get(m);
    a.i64(HIDDEN);
    a.s().i64_or();
    a.set(m);
    a.end();
}

/// Returns the zero of `sign`'s sign: the remainder of a multiple.
fn ret_zero(a: &mut Asm, sign: u32) {
    a.get(sign);
    a.s().f64_reinterpret_i64().return_();
}

/// The code of `RemF64`: `(x: f64, y: f64) -> f64`.
#[must_use]
pub(crate) fn rem_f64_code() -> Code {
    let mut a = Asm::new(vec![VT::F64, VT::F64]);
    let (x, y) = (0, 1);
    let (ux, uy) = (a.local(VT::I64), a.local(VT::I64));
    let (sign, shift) = (a.local(VT::I64), a.local(VT::I64));
    let (mx, my) = (a.local(VT::I64), a.local(VT::I64));
    let (ex, ey) = (a.local(VT::I32), a.local(VT::I32));
    let diff = a.local(VT::I64);
    for (src, dst) in [(x, ux), (y, uy)] {
        a.get(src);
        a.s().i64_reinterpret_f64();
        a.set(dst);
    }
    a.get(ux);
    a.i64(i64::MIN);
    a.s().i64_and();
    a.set(sign);
    // A zero or NaN divisor, or an infinite or NaN dividend, gives NaN.
    a.get(uy);
    a.i64(1);
    a.s().i64_shl().i64_eqz();
    a.get(uy);
    a.i64(1);
    a.s().i64_shl();
    a.i64(INF2);
    a.s().i64_gt_u().i32_or();
    a.get(ux);
    a.i64(1);
    a.s().i64_shl();
    a.i64(INF2);
    a.s().i64_ge_u().i32_or();
    a.if_();
    a.s().f64_const(f64::NAN.into()).return_();
    a.end();
    // A dividend no larger than the divisor in magnitude is its own
    // remainder, or a zero when they are equal.
    a.get(ux);
    a.i64(1);
    a.s().i64_shl();
    a.get(uy);
    a.i64(1);
    a.s().i64_shl();
    a.s().i64_le_u();
    a.if_();
    a.get(ux);
    a.i64(1);
    a.s().i64_shl();
    a.get(uy);
    a.i64(1);
    a.s().i64_shl();
    a.s().i64_eq();
    a.if_();
    ret_zero(&mut a, sign);
    a.end();
    a.get(x);
    a.s().return_();
    a.end();
    normalize(&mut a, ux, ex, mx, shift);
    normalize(&mut a, uy, ey, my, shift);
    // Subtract the divisor's significand where it fits, once per exponent
    // step; the doubling brings in the next bit of the dividend's place.
    let step = |a: &mut Asm| {
        a.get(mx);
        a.get(my);
        a.s().i64_sub();
        a.set(diff);
        a.get(diff);
        a.i64(0);
        a.s().i64_ge_s();
        a.if_();
        a.get(diff);
        a.s().i64_eqz();
        a.if_();
        ret_zero(a, sign);
        a.end();
        a.get(diff);
        a.set(mx);
        a.end();
    };
    a.block();
    a.loop_();
    a.get(ex);
    a.get(ey);
    a.s().i32_gt_s().i32_eqz();
    a.br_if(1);
    step(&mut a);
    a.get(mx);
    a.i64(1);
    a.s().i64_shl();
    a.set(mx);
    a.get(ex);
    a.i32(1);
    a.s().i32_sub();
    a.set(ex);
    a.br(0);
    a.end();
    a.end();
    step(&mut a);
    // The remainder is nonzero here: normalize its significand again.
    a.get(mx);
    a.s().i64_clz();
    a.i64(11);
    a.s().i64_sub();
    a.set(shift);
    a.get(mx);
    a.get(shift);
    a.s().i64_shl();
    a.set(mx);
    a.get(ex);
    a.get(shift);
    a.s().i32_wrap_i64().i32_sub();
    a.set(ex);
    // Scale: a normal result drops the hidden bit and takes the exponent;
    // a subnormal one shifts the exact significand down.
    a.get(ex);
    a.i32(0);
    a.s().i32_gt_s();
    a.if_();
    a.get(mx);
    a.i64(HIDDEN);
    a.s().i64_sub();
    a.get(ex);
    a.s().i64_extend_i32_u();
    a.i64(52);
    a.s().i64_shl().i64_or();
    a.set(mx);
    a.else_();
    a.get(mx);
    a.i32(1);
    a.get(ex);
    a.s().i32_sub().i64_extend_i32_u().i64_shr_u();
    a.set(mx);
    a.end();
    a.get(mx);
    a.get(sign);
    a.s().i64_or().f64_reinterpret_i64();
    a.finish(vec![VT::F64])
}
