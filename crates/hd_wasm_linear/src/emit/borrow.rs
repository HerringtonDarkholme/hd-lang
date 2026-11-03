//! Access checks on an object's borrow state (model A of
//! `docs/future-work/POINTER_LOANS.md`, phase 2 of #1336). Every counted
//! object keeps its loans in descriptor bits 16 to 21 (§23.4): a write
//! or a `&mut` through a `*T` needs the whole state clear, and any other
//! access through one, a read or a `&`, needs the exclusive bit clear. A
//! conflict panics `borrow-conflict`. The object a place's last `*T`
//! reaches takes the access's kind; each `*T` before it is read to reach
//! the next, so its object takes a read check.
//!
//! A type none of whose objects any loan may hold needs no check
//! ([`Module::lent`]), so a place that crosses no `*T`, or only `*T`s to
//! such types, emits nothing. Passing a pointer is no access at run time.
//! Nothing sets a loan yet, so no check fires; the function's own loans
//! through the same pointer, which a later phase subtracts, are none.

use hd_base::StageResult;
use hd_tir::ir::{PlaceRoot, Proj, Tag};
use hd_types::Ty;
use wasm_encoder::BlockType;

use super::Em;
use crate::layout::{Kind, ref_mutable};
use crate::module::{HEAP, LOAN_EXCL, LOAN_STATE, Module, mem};
use crate::op::Op as I;
use crate::rt::Helper;

/// Records that objects of type `object` may be lent, exclusively when
/// `excl`: a write through a `*T` to one checks its borrow state, and a
/// read checks it where the loan may be exclusive.
pub(super) fn lend(m: &mut Module, object: Ty, excl: bool) {
    *m.lent.entry(object).or_insert(false) |= excl;
}

impl Em<'_, '_, '_> {
    /// Before instruction `i`: the borrow-state checks of the access it
    /// makes through each `*T` on its place. A `Store` or an `IsUnique`
    /// writes the place, a `Ref` lends it as its mutability says, and a
    /// `Read` reads it.
    // This hook sits on the recursive instruction-emission path. Outlining
    // keeps the 1,000-arm flat-chain regression within its stack bound.
    #[inline(never)]
    pub(super) fn borrow_access(&mut self, i: u32) -> StageResult<()> {
        if self.m.lent.is_empty() {
            return Ok(());
        }
        let write = match self.b.tags[i as usize] {
            Tag::Store | Tag::IsUnique => true,
            Tag::Ref => ref_mutable(self.pool(), self.env(), self.ty(i)),
            Tag::Read => false,
            _ => return Ok(()),
        };
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return Ok(());
        };
        if !matches!(
            p.root,
            PlaceRoot::Local(_) | PlaceRoot::Value(_) | PlaceRoot::Global(..)
        ) {
            return Ok(());
        }
        // Each dereference of a `*T`, with the type of the object it reaches.
        let root = self.at_root_ty(p.root);
        let mut crossed = Vec::new();
        for (k, step) in p.proj.iter().enumerate() {
            if *step != Proj::Deref {
                continue;
            }
            if let Kind::Ref {
                target,
                fat: false,
                whole: true,
            } = self.kind(self.walk_ty(root, &p.proj[..k])?)
            {
                crossed.push((k, target));
            }
        }
        let last = crossed.len().wrapping_sub(1);
        for (n, (k, object)) in crossed.into_iter().enumerate() {
            let writes = write && n == last;
            let mask = match self.m.lent.get(&object) {
                Some(_) if writes => LOAN_STATE,
                Some(true) => LOAN_EXCL,
                _ => continue,
            };
            let mut fresh = Vec::new();
            let (at, _) = self.walk_place(i, &p, k, None, &mut fresh)?;
            let o = self.at_i32(&at)?;
            self.release_fresh(&fresh)?;
            self.borrow_check(o, mask, object, writes);
        }
        Ok(())
    }

    /// Panics `borrow-conflict` when the borrow state of the object whose
    /// payload address is in local `o` has a bit of `mask` set.
    fn borrow_check(&mut self, o: u32, mask: u32, object: Ty, writes: bool) {
        self.f.get(o);
        self.f.u32(crate::module::HEADER - 4);
        self.f.op(I::I32Sub);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.u32(mask);
        self.f.op(I::I32And);
        self.f.op(I::If(BlockType::Empty));
        let access = if writes { "written" } else { "read" };
        self.f.helper(Helper::Panic(format!(
            "borrow-conflict: a `{}` a live reference borrows was {access} through another `*T`",
            self.pool().display(object)
        )));
        self.f.op(I::End);
    }
}
