//! Encode literal aggregates at existing Memory boundaries; no calls are evaluated.
use super::super::Em;
use crate::layout::{Kind, Leaf, by_address};
use crate::module::Ins;
use crate::op::Op;
use hd_base::StageResult;
use hd_tir::ir::{NONE, Ref, Tag};

/// O(literal nodes + encoded bytes); Memory children remain opaque.
/// Runtime operands produce no allocation.
pub(super) fn allocation(em: &mut Em<'_, '_, '_>, root: u32) -> StageResult<Option<u32>> {
    let mut work = vec![(root, 0, true)];
    let mut constants = Vec::new();
    let mut tags = Vec::new();
    while let Some((r, off, whole)) = work.pop() {
        if r != NONE && r & Ref::CONST_BIT != 0 {
            constants.push((r, off));
            continue;
        }
        if r == NONE || !whole && by_address(em.pool(), em.env(), em.ty(r))? {
            return Ok(None);
        }
        let t = em.ty(r);
        let [a, b] = em.b.data[r as usize];
        match em.b.tags[r as usize] {
            Tag::Scope => match em.b.wrapper_value(r as usize) {
                Some(value) => work.push((value, off, whole)),
                None => return Ok(None),
            },
            Tag::Block if b != NONE => work.push((b, off, whole)),
            Tag::NewStruct if em.newtype(t) => {
                // ruled out: a verified newtype constructor has exactly one field.
                work.push((em.b.record(b)[0], off, whole));
            }
            Tag::NewStruct | Tag::NewTuple => {
                if em.b.tags[r as usize] == Tag::NewTuple && a != NONE {
                    return Ok(None);
                }
                let parts = em.member_parts(t)?;
                for (&value, part) in em.b.record(b).iter().zip(&parts.parts).rev() {
                    work.push((value, off + part.offset, false));
                }
            }
            Tag::NewVariant if matches!(em.kind(t), Kind::Value) => {
                let flat = em.enum_flat(t)?;
                if let Some(d) = flat.cell()
                    && let Some(raw) = d.enc.raw(a)
                {
                    tags.push((off, d.leaf, u64::from(raw)));
                }
                for (&value, &at) in em.b.record(b).iter().zip(&flat.offsets[a as usize]).rev() {
                    work.push((value, off + at, false));
                }
            }
            _ => return Ok(None),
        }
    }
    // Allocate only after proving literals; never expand an unknown Memory value.
    let mem = crate::layout::layout(em.pool(), em.env(), em.ty(root))?;
    let mut bytes = vec![0; mem.size as usize];
    let mut pointers = Vec::new();
    for (off, leaf, bits) in tags {
        write(&mut bytes, off, leaf, bits);
    }
    for (r, off) in constants {
        let ty = em.b.consts[(r & !Ref::CONST_BIT) as usize].0;
        let mem = crate::layout::mem(em.pool(), em.env(), ty)?;
        let values = em.konst_ops(r)?;
        for (leaf, ops) in mem.leaves.iter().zip(values) {
            let bits = match ops.as_slice() {
                [Ins::Static(at)] => {
                    pointers.push(off + leaf.offset);
                    u64::from(*at)
                }
                [Ins::Op(Op::I32Const(bits))] => u64::from(bits.cast_unsigned()),
                [Ins::Op(Op::I64Const(bits))] => bits.cast_unsigned(),
                [Ins::Op(Op::F32Const(bits))] => u64::from(bits.bits()),
                [Ins::Op(Op::F64Const(bits))] => bits.bits(),
                _ => return Ok(None),
            };
            write(&mut bytes, off, *leaf, bits);
        }
    }
    Ok(Some(em.m.static_record(&bytes, mem.align, &pointers)))
}

fn write(bytes: &mut [u8], off: u32, leaf: Leaf, bits: u64) {
    let at = (off + leaf.offset) as usize;
    let len = usize::from(leaf.bytes);
    bytes[at..at + len].copy_from_slice(&bits.to_le_bytes()[..len]);
}
