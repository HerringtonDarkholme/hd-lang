//! Emission of TIR short-loan marks. Eligibility, holder propagation and
//! intervening-write checks live in [`hd_tir::own::short`]. The late pass
//! reads resolved call/type/storage facts through its context; this file
//! installs the marks; there are no representation temporaries to release.

use super::Em;
use hd_mono::Target;
use hd_tir::ir::Callee;
use hd_types::Ty;

impl Em<'_, '_, '_> {
    /// Read the late TIR pass's marks; no loan eligibility is decided here.
    pub(super) fn prepare_short_loans(&mut self, marks: Vec<hd_tir::own::short::Mark>) {
        for mark in marks {
            for value in mark.values {
                self.loans.insert(value);
                self.short_holders.insert(value);
            }
            for local in mark.locals {
                self.loan_locals.entry(local).or_insert(mark.origin);
            }
            self.short_skips.extend(mark.skips);
            self.short_origins.insert(mark.origin);
        }
    }
}

impl hd_tir::own::short::Context for Em<'_, '_, '_> {
    fn call_kind(&self, instruction: u32) -> hd_tir::own::short::CallKind {
        use hd_tir::own::short::CallKind;
        match self.calls.get(&instruction) {
            Some(Target::MapKey { .. } | Target::MapInline(_)) => CallKind::MapProbe,
            Some(Target::Call(call)) if matches!(&call.kind, hd_mono::TargetKind::Intrinsic(key) if key == "ptr_ref") => {
                CallKind::PointerRef
            }
            _ => CallKind::Other,
        }
    }
    fn value_type(&self, value: u32) -> Ty {
        self.ty_of(value)
    }
    fn local_type(&self, local: u32) -> Ty {
        self.sub(self.b.local_ty[local as usize])
    }
    fn mutable_ref(&self, ty: Ty) -> bool {
        crate::layout::ref_mutable(self.pool(), self.env(), ty)
    }
    fn ref_target(&self, ty: Ty) -> Option<(Ty, bool)> {
        Em::ref_target(self, ty)
    }
    fn is_map(&self, ty: Ty) -> bool {
        Em::is_map(self, ty)
    }
    fn element(&self, ty: Ty) -> Option<Ty> {
        self.elem(ty).ok().map(|(element, _)| element)
    }
    // The ownership passes' notion: a thin local reference is counted
    // there, though it counts nothing ([`crate::layout::walk`]).
    fn counted(&self, ty: Ty) -> bool {
        self.tracked(ty)
    }
    fn needs_drop(&self, ty: Ty) -> bool {
        Em::needs_drop(self, ty)
    }
    fn indirect_local(&self, local: usize) -> bool {
        self.is_cell(local) || self.slots.contains_key(&local)
    }
    fn promoted(&self, value: u32) -> bool {
        self.promoted.contains(&value)
    }
    fn loan_value(&self, value: u32) -> bool {
        self.loans.contains(&value)
    }
    fn may_write(&self, callee: &Callee) -> bool {
        self.input.own_cx.may_write(callee)
    }
}
