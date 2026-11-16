//! Resident scalar slots for suspension frames. Follow rustc coroutine
//! lowering's saved-local -> field remapping, without hd's old per-state
//! serialization. Addresses of source cells never point into this area.
//! <https://github.com/rust-lang/rust/blob/main/compiler/rustc_mir_transform/src/coroutine/mod.rs>

use std::collections::BTreeMap;

use crate::module::{Func, HEAP, Ins, Val};
use crate::op::Op as I;
use crate::origins::Remap;
use wasm_encoder::BlockType;

use super::susp::{FRAME, load_op, store_op};

/// A body with no suspension point has no persistent scalar accesses. Its
/// one cold-entry marker restores constructor slots, never a per-state row.
pub(super) fn initial(f: &mut Func, area: &[(u32, u32, Val)], selected: &[bool]) {
    let body = std::mem::take(&mut f.body);
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = Vec::new();
    crate::suspension_work::add(body.len(), 0, 0, 0, 0);
    for (at, ins) in body.into_iter().enumerate() {
        let before = out.len();
        if matches!(ins, Ins::LoadAll) {
            for &(l, offset, value) in area {
                if selected[l as usize] {
                    out.extend([
                        Ins::Op(I::LocalGet(FRAME)),
                        Ins::Op(load_op(value, offset)),
                        Ins::Op(I::LocalSet(l)),
                    ]);
                }
            }
        } else {
            out.push(ins);
        }
        origins.merged(at..at + 1, out.len() - before);
    }
    f.body = out;
    origins.finish(f);
}

/// Rewrite original virtual-local accesses once. The immutable reservation
/// base belongs to `wide::locals_frame`; `base` may switch to heap storage.
/// Layout offsets are task-relative, whereas `base` addresses the scalar
/// region itself (so provisional memory needs no fake task prefix).
#[derive(Clone, Copy)]
pub(super) struct Plan<'a> {
    pub area: &'a [(u32, u32, Val)],
    pub selected: &'a [bool],
    pub start: u32,
    pub size: u32,
}

pub(super) fn resident(f: &mut Func, plan: Plan<'_>, base: u32, stack: u32, stack_offset: u32) {
    let Plan {
        area,
        selected,
        start,
        size,
    } = plan;
    let mut positions = vec![None; selected.len()];
    for &(l, at, v) in area {
        if selected[l as usize] {
            positions[l as usize] = Some((at - start, v));
        }
    }
    let body = std::mem::take(&mut f.body);
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = vec![
        Ins::Op(I::LocalGet(FRAME)),
        Ins::Op(I::If(BlockType::Result(wasm_encoder::ValType::I32))),
        Ins::Op(I::LocalGet(FRAME)),
        Ins::Op(I::I32Const(start.cast_signed())),
        Ins::Op(I::I32Add),
        Ins::Op(I::Else),
        Ins::Op(I::LocalGet(stack)),
        Ins::Op(I::I32Const(stack_offset.cast_signed())),
        Ins::Op(I::I32Add),
        Ins::Op(I::End),
        Ins::Op(I::LocalSet(base)),
        Ins::Op(I::LocalGet(FRAME)),
        Ins::Op(I::I32Eqz),
        Ins::Op(I::If(BlockType::Empty)),
        Ins::Op(I::LocalGet(base)),
        Ins::Op(I::I32Const(0)),
        Ins::Op(I::I32Const((size - start).cast_signed())),
        crate::bulk::fill(HEAP),
    ];
    // Fresh direct calls arrive through the original signature. A cold or
    // resumed call already owns its argument slots; its zero poll arguments
    // must never overwrite them.
    for &(l, at, v) in area {
        if (l as usize) < f.params.len() && selected[l as usize] {
            out.extend([
                Ins::Op(I::LocalGet(base)),
                Ins::Op(I::LocalGet(l)),
                Ins::Op(store_op(v, at - start)),
            ]);
        }
    }
    out.push(Ins::Op(I::End));
    origins.repeat(f.origin_root, out.len());
    let mut temps = BTreeMap::new();
    let mut accesses = 0;
    crate::suspension_work::add(body.len(), 0, positions.iter().flatten().count(), 0, 0);
    for (at, ins) in body.into_iter().enumerate() {
        let before = out.len();
        match ins {
            Ins::Op(I::LocalGet(l)) if positions.get(l as usize).is_some_and(Option::is_some) => {
                if let Some((offset, value)) = positions[l as usize] {
                    accesses += 1;
                    out.push(Ins::Op(I::LocalGet(base)));
                    out.push(Ins::Op(load_op(value, offset)));
                }
            }
            Ins::Op(I::LocalSet(l) | I::LocalTee(l))
                if positions.get(l as usize).is_some_and(Option::is_some) =>
            {
                if let Some((offset, value)) = positions[l as usize] {
                    accesses += 1;
                    let temp = *temps.entry(value).or_insert_with(|| f.local(value));
                    let tee = matches!(ins, Ins::Op(I::LocalTee(_)));
                    out.extend([
                        Ins::Op(if tee {
                            I::LocalTee(temp)
                        } else {
                            I::LocalSet(temp)
                        }),
                        Ins::Op(I::LocalGet(base)),
                        Ins::Op(I::LocalGet(temp)),
                        Ins::Op(store_op(value, offset)),
                    ]);
                }
            }
            Ins::SpillBase => {
                out.push(Ins::Op(I::I32Const(start.cast_signed())));
                out.push(Ins::Op(I::I32Add));
            }
            Ins::LoadAll => {}
            other => out.push(other),
        }
        origins.merged(at..at + 1, out.len() - before);
    }
    crate::suspension_work::add(0, accesses, 0, 0, 0);
    f.body = out;
    origins.finish(f);
}
