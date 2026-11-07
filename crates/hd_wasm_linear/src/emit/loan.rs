//! Emission of TIR short-loan marks. Eligibility, holder propagation and
//! intervening-write checks live in [`hd_tir::own::short`]. The late pass
//! reads resolved call/type/storage facts through its context; this file
//! installs its marks and releases representation temporaries at their ends.

use super::Em;
use hd_mono::Target;
use hd_tir::ir::Callee;
use hd_types::Ty;

/// A short loan, by its origin ([`Em::short_loans`]).
#[derive(Default)]
pub(super) struct ShortLoan {
    /// The values made while emitting the origin that the loan must keep
    /// until its end: copies of open values read through views, each
    /// behind its flag.
    /// A map probe's `owned` flag and table, and the map's type: when the
    /// map's reference is a view, the probe read a copy of the table, which
    /// the end releases.
    pub(super) table: Option<(u32, u32, Ty)>,
}

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
            for end in mark.ends {
                self.short_ends.insert(end, mark.origin);
            }
            self.short_loans.insert(mark.origin, ShortLoan::default());
        }
    }

    /// A short loan's end, `Drop` `i`: releases what its origin's emission
    /// made that the loan kept alive.
    pub(super) fn end_short_loan(&mut self, i: u32) -> hd_base::StageResult<()> {
        let Some(o) = self.short_ends.get(&i).copied() else {
            return Ok(());
        };
        let Some(loan) = self.short_loans.get_mut(&o) else {
            return Ok(());
        };
        // Each path runs one of the loan's ends.
        let table = loan.table;
        if let Some((owned, m, t)) = table {
            self.f.get(owned);
            self.f.op(crate::op::Op::If(wasm_encoder::BlockType::Empty));
            self.release_locals(&[m], t)?;
            self.f.zero(owned);
            self.f.op(crate::op::Op::End);
        }
        Ok(())
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
    fn counted(&self, ty: Ty) -> bool {
        Em::counted(self, ty).is_some()
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
