//! Constant numeric conversions and comparisons for the existing forward walk.
//! LLVM's `ConstantFoldCastOperand` likewise folds after concrete source and
//! destination types are known. hd has fixed integer widths and two IEEE
//! widths, so every operation below costs O(1), without an evaluator walk.

use hd_tir::ir::PrimOp;
use hd_types::{Prim, Ty};

pub(crate) fn primitive(ty: Ty) -> Option<Prim> {
    Prim::ALL.into_iter().find(|&p| Ty::prim(p) == ty)
}

fn integer_type(p: Prim) -> Option<(u32, bool)> {
    Some(match p {
        Prim::I8 => (8, true),
        Prim::I16 => (16, true),
        Prim::I32 => (32, true),
        Prim::I64 => (64, true),
        Prim::U8 => (8, false),
        Prim::U16 => (16, false),
        Prim::U32 | Prim::Usize => (32, false),
        Prim::U64 => (64, false),
        _ => return None,
    })
}

fn integer(p: Prim, bits: u64) -> Option<u64> {
    let (width, signed) = integer_type(p)?;
    let shift = u64::BITS - width;
    Some(if signed {
        ((bits << shift).cast_signed() >> shift).cast_unsigned()
    } else {
        bits << shift >> shift
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub(crate) fn convert(from: Prim, to: Prim, bits: u64) -> Option<u64> {
    if !(from.is_integer() || from.is_float()) || !(to.is_integer() || to.is_float()) {
        return None;
    }
    if from.is_float() {
        let value = f64::from_bits(bits);
        return Some(match to {
            Prim::I8 => i64::from(value as i8).cast_unsigned(),
            Prim::I16 => i64::from(value as i16).cast_unsigned(),
            Prim::I32 => i64::from(value as i32).cast_unsigned(),
            Prim::I64 => (value as i64).cast_unsigned(),
            Prim::U8 => u64::from(value as u8),
            Prim::U16 => u64::from(value as u16),
            Prim::U32 | Prim::Usize => u64::from(value as u32),
            Prim::U64 => value as u64,
            Prim::F32 => f64::from(value as f32).to_bits(),
            Prim::F64 => bits,
            _ => return None,
        });
    }
    let bits = integer(from, bits)?;
    if to.is_integer() {
        return integer(to, bits);
    }
    // Cast straight to f32, without an intermediate f64 rounding.
    Some(match (from.is_unsigned(), to) {
        (true, Prim::F32) => f64::from(bits as f32).to_bits(),
        (false, Prim::F32) => f64::from(bits.cast_signed() as f32).to_bits(),
        (true, Prim::F64) => (bits as f64).to_bits(),
        (false, Prim::F64) => (bits.cast_signed() as f64).to_bits(),
        _ => return None,
    })
}

fn compare<T: PartialOrd + Copy>(op: PrimOp, left: T, right: T) -> Option<u64> {
    Some(u64::from(match op {
        PrimOp::Eq => left == right,
        PrimOp::Ne => left != right,
        PrimOp::Lt => left < right,
        PrimOp::Le => left <= right,
        PrimOp::Gt => left > right,
        PrimOp::Ge => left >= right,
        _ => return None,
    }))
}

pub(crate) fn comparison(op: PrimOp, p: Prim, left: u64, right: u64) -> Option<u64> {
    if p.is_float() {
        return compare(op, f64::from_bits(left), f64::from_bits(right));
    }
    if p.is_integer() {
        let (left, right) = (integer(p, left)?, integer(p, right)?);
        return if p.is_unsigned() {
            compare(op, left, right)
        } else {
            compare(op, left.cast_signed(), right.cast_signed())
        };
    }
    if matches!(p, Prim::Bool | Prim::Char) {
        return compare(op, left, right);
    }
    None
}

/// Integer bounds are constant results of the existing `int_min`/`int_max`
/// intrinsics; an open result remains a call until its instance closes.
#[must_use]
pub fn integer_bound(ty: Ty, minimum: bool) -> Option<u64> {
    let (width, signed) = integer_type(primitive(ty)?)?;
    integer_bound_bits(width, signed, minimum)
}

/// Shared bound bits for constant folding and emission. Invalid metadata
/// returns no bound instead of shifting beyond the `u64` representation.
#[must_use]
pub fn integer_bound_bits(width: u32, signed: bool, minimum: bool) -> Option<u64> {
    let mask = u64::MAX.checked_shr(u64::BITS.checked_sub(width)?)?;
    Some(match (minimum, signed) {
        (true, true) => u64::MAX.checked_shl(width.checked_sub(1)?)?,
        (true, false) => 0,
        (false, true) => mask >> 1,
        (false, false) => mask,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_canonicalization_and_wrapping_cover_every_width() {
        for (signed, min, max) in [
            (Prim::I8, -128i64, 127i64),
            (Prim::I16, -32768, 32767),
            (Prim::I32, -2_147_483_648, 2_147_483_647),
            (Prim::I64, i64::MIN, i64::MAX),
        ] {
            assert_eq!(
                integer_bound(Ty::prim(signed), true),
                Some(min.cast_unsigned())
            );
            assert_eq!(
                integer_bound(Ty::prim(signed), false),
                Some(max.cast_unsigned())
            );
        }
        assert_eq!(integer_bound_bits(0, false, true), None);
        assert_eq!(integer_bound_bits(65, true, false), None);
        for signed in [Prim::I8, Prim::I16, Prim::I32, Prim::I64] {
            assert_eq!(convert(Prim::U64, signed, u64::MAX), Some(u64::MAX));
            assert_eq!(comparison(PrimOp::Lt, signed, u64::MAX, 0), Some(1));
        }
        for (unsigned, max) in [
            (Prim::U8, 255),
            (Prim::U16, 65535),
            (Prim::U32, 4_294_967_295),
            (Prim::Usize, 4_294_967_295),
            (Prim::U64, u64::MAX),
        ] {
            assert_eq!(convert(Prim::I64, unsigned, u64::MAX), Some(max));
            assert_eq!(comparison(PrimOp::Gt, unsigned, u64::MAX, 0), Some(1));
            assert_eq!(integer_bound(Ty::prim(unsigned), true), Some(0));
            assert_eq!(integer_bound(Ty::prim(unsigned), false), Some(max));
        }
        assert_eq!(convert(Prim::I8, Prim::I64, 255), Some(u64::MAX));
        assert_eq!(convert(Prim::U8, Prim::I64, 255), Some(255));
        assert_eq!(
            convert(Prim::U64, Prim::I8, 128),
            Some((-128i64).cast_unsigned())
        );
    }

    #[test]
    fn floating_casts_saturate_and_preserve_signed_zero_and_rounding() {
        assert_eq!(
            convert(Prim::F64, Prim::I8, f64::INFINITY.to_bits()),
            Some(127)
        );
        assert_eq!(
            convert(Prim::F64, Prim::I8, f64::NEG_INFINITY.to_bits()),
            Some((-128i64).cast_unsigned())
        );
        assert_eq!(
            convert(Prim::F64, Prim::U64, f64::INFINITY.to_bits()),
            Some(u64::MAX)
        );
        assert_eq!(convert(Prim::F64, Prim::I64, f64::NAN.to_bits()), Some(0));
        assert_eq!(convert(Prim::F64, Prim::U32, (-1.5f64).to_bits()), Some(0));
        assert_eq!(
            convert(Prim::F64, Prim::F32, (-0.0f64).to_bits()),
            Some((-0.0f64).to_bits())
        );
        // Just above a binary32 halfway point, lost by an intermediate f64.
        let value = (1u64 << 63) + (1u64 << 39) + 1;
        let rounded = f64::from(f32::from_bits(0x5f00_0001)).to_bits();
        assert_eq!(convert(Prim::U64, Prim::F32, value), Some(rounded));
        assert_eq!(
            comparison(PrimOp::Eq, Prim::F64, f64::NAN.to_bits(), 0),
            Some(0)
        );
        assert_eq!(
            comparison(PrimOp::Ne, Prim::F64, f64::NAN.to_bits(), 0),
            Some(1)
        );
    }
}
