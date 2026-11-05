//! Value movers: exchange ownership without copying or dropping a value.

use hd_base::StageResult;
use hd_types::Ty;
use wasm_encoder::BlockType;

use super::Em;
use crate::layout::Leaf;
use crate::module::{HEAP, Val, mem};
use crate::op::Op as I;
use crate::rt::{self, Elem, Helper, LIST_ELEMS, LIST_LEN, RefLayout};
use crate::unsupported;

struct ListMove {
    reference: u32,
    header: u32,
    view: u32,
    layout: RefLayout,
    elem: Elem,
    ty: Ty,
}

impl Em<'_, '_, '_> {
    fn exchange_leaves(&mut self, left: u32, right: u32, leaves: &[Leaf], offset: u32) {
        let av: Vec<_> = leaves.iter().map(|l| self.f.local(l.val)).collect();
        let bv: Vec<_> = leaves.iter().map(|l| self.f.local(l.val)).collect();
        for (leaf, (&av, &bv)) in leaves.iter().zip(av.iter().zip(&bv)) {
            self.f.get(left);
            self.f.op(leaf.load(offset));
            self.f.set(av);
            self.f.get(right);
            self.f.op(leaf.load(offset));
            self.f.set(bv);
        }
        for (leaf, (&av, &bv)) in leaves.iter().zip(av.iter().zip(&bv)) {
            self.f.get(left);
            self.f.get(bv);
            self.f.op(leaf.store(offset));
            self.f.get(right);
            self.f.get(av);
            self.f.op(leaf.store(offset));
        }
    }

    /// Take a view's header without another count; physical references
    /// keep their header in place until the operation writes it back.
    fn moving_list(&mut self, operand: u32) -> StageResult<ListMove> {
        let Some((ty, false)) = self.ref_target(self.ty_of(operand)) else {
            return unsupported("a list mover without a thin reference");
        };
        let (_, elem) = self.elem(ty)?;
        let layout = self.ref_layout(ty)?;
        let (reference, header, view) = (
            self.f.local(Val::I32),
            self.f.local(Val::I32),
            self.f.local(Val::I32),
        );
        self.comp(operand, 0)?;
        self.f.set(reference);
        self.f.get(reference);
        self.f.helper(Helper::RefIsView);
        self.f.op(I::LocalTee(view));
        self.f.op(I::If(BlockType::Empty));
        let [taken] = rt::ref_take(&mut self.f, reference, &layout)[..] else {
            return unsupported("a list mover with several headers");
        };
        self.f.get(taken);
        self.f.set(header);
        self.f.op(I::Else);
        self.f.get(reference);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.set(header);
        self.f.op(I::End);
        Ok(ListMove {
            reference,
            header,
            view,
            layout,
            elem,
            ty,
        })
    }

    fn unique_moving_list(&mut self, list: &ListMove) {
        self.f.get(list.header);
        let start = self.f.body.len();
        self.f.helper(Helper::ListUnique {
            elem: list.elem.clone(),
        });
        if self.f.origins.is_some() {
            let subject = crate::views::count_subject(self.pool(), self.env(), list.ty);
            self.f.note_count(
                start,
                u32::try_from(self.b.anchor_len()).expect("TIR instructions"),
                crate::views::CountAction::Unique,
                &subject,
                Some(hd_tir::own::Reason::MayBeShared),
            );
        }
        self.f.set(list.header);
    }

    fn restore_moving_list(&mut self, list: &ListMove) {
        self.f.get(list.view);
        self.f.op(I::If(BlockType::Empty));
        rt::ref_store(
            &mut self.f,
            list.reference,
            &[list.header],
            &list.layout,
            true,
        );
        self.f.op(I::Else);
        self.f.get(list.reference);
        self.f.get(list.header);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        self.f.op(I::End);
    }

    /// The caller checks nonemptiness. Shortening the list transfers its
    /// last slot's ownership to the result, without dropping that slot.
    pub(super) fn list_take_last(&mut self, i: u32, operand: u32) -> StageResult<()> {
        let list = self.moving_list(operand)?;
        self.unique_moving_list(&list);
        let index = self.f.local(Val::I32);
        self.f.get(list.header);
        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        self.f.i32(1);
        self.f.op(I::I32Sub);
        self.f.set(index);
        let slot = self.slot_at(list.header, index, list.elem.size);
        let values: Vec<_> = list
            .elem
            .leaves
            .iter()
            .map(|l| self.f.local(l.val))
            .collect();
        for (leaf, &value) in list.elem.leaves.iter().zip(&values) {
            self.f.get(slot);
            self.f.op(leaf.load(LIST_ELEMS));
            self.f.set(value);
        }
        self.f.get(list.header);
        self.f.get(index);
        self.f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
        self.restore_moving_list(&list);
        for value in values {
            self.f.get(value);
        }
        self.store_owned(i)
    }

    /// Bounds-check both indices, then exchange the original slots.
    pub(super) fn list_swap(&mut self, args: &[u32]) -> StageResult<()> {
        let [operand, left, right] = args else {
            return unsupported("list_swap without a list and two indices");
        };
        let list = self.moving_list(*operand)?;
        let a = self.index(list.header, *left)?;
        let b = self.index(list.header, *right)?;
        self.f.get(a);
        self.f.get(b);
        self.f.op(I::I32Ne);
        self.f.op(I::If(BlockType::Empty));
        self.unique_moving_list(&list);
        let a = self.slot_at(list.header, a, list.elem.size);
        let b = self.slot_at(list.header, b, list.elem.size);
        self.exchange_leaves(a, b, &list.elem.leaves, LIST_ELEMS);
        self.f.op(I::End);
        self.restore_moving_list(&list);
        Ok(())
    }

    /// `std.ops.swap`: physical places exchange their leaves directly;
    /// views move their values through their getters and setters.
    pub(super) fn swap_refs(&mut self, args: &[u32]) -> StageResult<()> {
        let [left, right] = args else {
            return unsupported("swap without two operands");
        };
        let Some((target, false)) = self.ref_target(self.ty_of(*left)) else {
            return unsupported("swap of a value without a thin reference");
        };
        if self.ref_target(self.ty_of(*right)) != Some((target, false)) {
            return unsupported("swap of different referent types");
        }
        let value = self.ref_layout(target)?;
        let (a, b) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.comp(*left, 0)?;
        self.f.set(a);
        self.comp(*right, 0)?;
        self.f.set(b);
        // Keep the physical build independent of view boxing and release
        // code, even before general constant-expression folding.
        self.f.helper(Helper::RefViewsAbsent);
        self.f.op(I::If(BlockType::Result(Val::I32.wasm())));
        self.f.i32(0);
        self.f.op(I::Else);
        self.f.get(a);
        self.f.helper(Helper::RefIsView);
        self.f.get(b);
        self.f.helper(Helper::RefIsView);
        self.f.op(I::I32Or);
        self.f.op(I::End);
        self.f.op(I::If(BlockType::Empty));
        let av = rt::ref_take(&mut self.f, a, &value);
        let bv = rt::ref_take(&mut self.f, b, &value);
        rt::ref_store(&mut self.f, a, &bv, &value, true);
        rt::ref_store(&mut self.f, b, &av, &value, true);
        self.f.op(I::Else);
        self.exchange_leaves(a, b, &value.leaves, 0);
        self.f.op(I::End);
        Ok(())
    }
}
