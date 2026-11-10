//! Counting (`crates/hd_wasm/DESIGN.md` §23.16): every place that
//! holds a counted reference owns one count, and the ownership passes
//! (`docs/compiler/tir-v2.md`, "Stage 2") decided every count of a body
//! ([`Em::owned`]); this module lowers their decisions to count glue and
//! decides none itself. A `Read` copies, moves or borrows as its mode
//! says, a `Drop` releases (and zeroes) a value or a local, a value moves
//! into the one use that consumes it ([`Em::take_from`]), and the one exit
//! releases the sub-body's exit set. A parameter is taken as
//! `layout::Pass` says (§23.25), so a value parameter arrives with its
//! count and a `&T` one is never counted.
//!
//! What the backend still adds is glue for a lowering or an instance the
//! generic body cannot see: an argument a callee instance owns but the
//! passes lent takes a count (`push_args`), a parameter the caller lends
//! takes one on entry when the body owns it (its exit set lists it), one
//! the passes left unowned that an instance receives owned is released at
//! the exit, and a cancelled suspension point releases the values the
//! passes' unwind set lists for it, before the exit releases the exit set
//! ([`Em::release_unwound`]). A `borrow` read takes no count, and a local
//! a borrow is stored into holds none ([`Em::prepare_own_loans`]).

use hd_base::SliceSort as _;
use hd_base::StageResult;
use hd_tir::ir::count::{At, Info, Part};
use hd_tir::ir::{IntrinsicOp, NONE, Ref, Tag};
use hd_tir::own::Reason;
use hd_types::Ty;

use super::Em;
use super::refs::bare_local;
use super::value::Src;
use crate::layout::{Pass, Walk, coll_kind, coll_leaves};
use crate::module::Val;
use crate::rt::Helper;

impl Em<'_, '_, '_> {
    /// Reads the shared count descriptor for the current function boundary.
    pub(super) fn stated_count(&self, tag: Tag, at: At, part: Part) -> Option<Reason> {
        hd_tir::ir::count::find(self.b, tag, self.b.sub_root[self.sub_id as usize], at, part)
            .map(|c| Reason::from_word(c.reason))
    }

    /// Reads a statement in the target's adjacent operation-count bundle.
    pub(super) fn operation_count(&self, target: u32, tag: Tag, part: Part) -> Option<Reason> {
        hd_tir::ir::count::operation(self.b, &self.topo, target, tag, part)
            .map(|c| Reason::from_word(c.reason))
    }

    /// Mandatory ownership boundary fact; unlike `stated_count`, absence
    /// is not an optional action at these producer-proven sites.
    pub(super) fn required_stated_count(&self, tag: Tag, at: At, part: Part) -> Reason {
        super::required(
            self.stated_count(tag, at, part),
            "ownership must state this boundary count",
        )
    }

    /// Mandatory action in the ownership pass's adjacent operation bundle.
    pub(super) fn required_operation_count(&self, i: u32, tag: Tag, part: Part) -> Reason {
        super::required(
            self.operation_count(i, tag, part),
            "ownership must state this operation count",
        )
    }

    /// A TIR-stated count of an already constructed borrowed result.
    pub(super) fn own_retain(&mut self, value: u32) -> StageResult<()> {
        if self.retained_before_result.remove(&value) {
            return Ok(());
        }
        // ruled out: retained_results verifies Retain immediately after its value;
        // lowering stores that value or records its earlier retain above.
        assert!(
            self.vals.contains_key(&value),
            "retained_results: Retain must follow a constructed or pre-retained value"
        );
        let locals = self.vals[&value].clone();
        self.retain_locals(&locals, self.ty_of(value))
    }

    /// Read the explicit result count, without deciding one from its layout:
    /// a stated retain, or the source's count taken over.
    pub(super) fn has_stated_retain(&self, value: u32) -> bool {
        hd_tir::ir::count::following(self.b, &self.topo, value, Tag::Retain)
            == Some(Info::new(At::Value, Part::Result, 0))
            || hd_tir::own::took_source(self.b, &self.topo, value as usize)
    }

    /// Whether `Store` `i` releases what its place holds.
    pub(super) fn store_releases(&self, i: u32) -> bool {
        hd_tir::ir::count::following(self.b, &self.topo, i, Tag::Release)
            == Some(Info::new(At::Operation, Part::Result, 3))
    }

    /// Whether the store being emitted releases its old value: a `Store` by
    /// its statement, and an instruction that replaces an entry always.
    pub(super) fn releases_old(&self) -> bool {
        self.emitting
            .last()
            .is_none_or(|&i| self.b.tags[i as usize] != Tag::Store || self.store_releases(i))
    }

    /// An owned body's `borrow` reads and binding locals
    /// (`docs/compiler/tir-v2.md`, "Borrow"): a borrow takes no count, so
    /// it joins [`Em::loans`], and its read lowers
    /// as a copy's would with no count. One through a reference takes no
    /// count ([`crate::rt::ref_lend`]). A local the passes store a borrow
    /// into holds no count ([`Em::loan_locals`]): its stores and `Drop`s
    /// count nothing.
    pub(super) fn prepare_own_loans(&mut self) {
        use hd_tir::ir::place;
        self.prepare_string_slice_comparisons();
        for i in 0..self.b.len() {
            let [a, w] = self.b.data[i];
            let x = u32::try_from(i).expect("insts");
            match self.b.tags[i] {
                Tag::Read if self.b.is_borrow(x) => {
                    self.loans.insert(x);
                }
                // A coercion of a borrow keeps its value, with no count.
                Tag::Coerce if self.b.is_borrowed_coercion(x) => {
                    self.loans.insert(x);
                }
                // A borrowed read or lent field reference stores its loan.
                Tag::Store if let Some(w) = self.b.loan_read(w) => {
                    let p = self.b.view(a);
                    let l = p.whole().then(|| p.local()).flatten().map(|l| l as usize);
                    // A borrow of all of another such local holds that
                    // one's read, whose fresh value its kept `Drop`s
                    // release (`hd_tir::own::derive`'s bindings).
                    if let Some(l) = l
                        && self.loans.contains(&w)
                    {
                        let from = self.b.data[w as usize][0];
                        let held = (from & place::BARE_LOCAL != 0)
                            .then(|| self.loan_locals.get(&bare_local(self.b, from)))
                            .flatten()
                            .copied();
                        self.loan_locals.insert(l, held.unwrap_or(w));
                    }
                }
                _ => {}
            }
        }
        // A substring compared immediately borrows its byte range. When
        // the source read is not a move, neither that read nor the view
        // needs a count before the equality instruction consumes it.
        let compared_slices: Vec<u32> = self.string_slice_comparisons.keys().copied().collect();
        for call in compared_slices {
            let mut args = self.rec(self.b.data[call as usize][1]);
            args.truncate(args.len().saturating_sub(3));
            let Some(&text) = args.first() else {
                continue;
            };
            if super::shape::read_local(self.b, text).is_some()
                && self.b.data[text as usize][1] != hd_tir::ir::ReadMode::Move as u32
            {
                self.loans.insert(text);
                self.loans.insert(call);
            }
        }
    }

    /// Build the direct `string.slice` call to string-equality map once
    /// for the body. Count preparation and emission then do constant-time
    /// lookups instead of rescanning the body for every instruction.
    fn prepare_string_slice_comparisons(&mut self) {
        for (equal, tag) in self.b.tags.iter().enumerate() {
            if *tag != Tag::Intrinsic
                || IntrinsicOp::from_u32(self.b.data[equal][0]) != Some(IntrinsicOp::StrEq)
            {
                continue;
            }
            let equal = u32::try_from(equal).expect("insts");
            for slice in self.rec(self.b.data[equal as usize][1]) {
                if self.is_string_slice_call(slice) {
                    self.string_slice_comparisons.insert(slice, equal);
                }
            }
        }
    }

    /// The counted references of a `t` value, when the program counts.
    pub(super) fn counted(&self, t: Ty) -> Option<Walk> {
        self.input.runtime?;
        crate::layout::count_walk(self.pool(), self.env(), t)
    }

    /// Whether the ownership passes track a value of `t` in a program
    /// that counts ([`crate::layout::walk`]): one with a counted
    /// reference, a thin local reference among them, though that counts
    /// nothing ([`Em::counted`]).
    pub(super) fn tracked(&self, t: Ty) -> bool {
        self.input.runtime.is_some() && crate::layout::walk(self.pool(), self.env(), t).is_some()
    }

    /// Whether `t` is a thin local reference (`ref.count.local`) in a
    /// program that counts: a plain address, which counts nothing, though
    /// one may own the object it names ([`Em::owned_refs`]).
    pub(super) fn local_ref(&self, t: Ty) -> bool {
        self.input.runtime.is_some()
            && matches!(
                self.kind(t),
                crate::layout::Kind::Ref {
                    fat: false,
                    whole: false,
                    ..
                }
            )
    }

    /// Whether releasing a value has work: reference counts, a
    /// `std.rt.Drop` hook on an inline value, or one on an inline field.
    /// A thin local reference counts nothing, but its end ends its loans
    /// and releases an object it owns, so it has work too: this is the
    /// ownership passes' walk ([`crate::layout::walk`]), not the count's.
    pub(super) fn needs_drop(&self, t: Ty) -> bool {
        self.tracked(t) || !self.drop_calls(t).is_empty()
    }

    /// Whether a new object of type `t` (a self-recursive enum's cell, a
    /// list's or a map's) takes part in cycle collection
    /// (`crate::cycle`).
    pub(super) fn cyclic_object(&self, t: Ty) -> bool {
        self.input.runtime.is_some() && crate::cycle::object(self.pool(), self.env(), t)
    }

    /// Whether an object's release can reach the same release glue again.
    pub(super) fn recursive_object(&self, t: Ty) -> bool {
        self.input.runtime.is_some() && crate::cycle::recursive_object(self.pool(), self.env(), t)
    }

    /// Whether a new object holding values of `parts`, which only an open
    /// value can hold, takes part in cycle collection (`crate::cycle`).
    pub(super) fn cyclic_holding(&self, parts: &[Ty]) -> bool {
        self.input.runtime.is_some() && crate::cycle::holding(self.pool(), self.env(), parts)
    }

    /// The values of a `t` value that a count of it takes through the copy
    /// glue (`rt:coll_copy`, `types.coll.value`): a list's or a map's own,
    /// and each list or map in a struct's fields at any depth.
    pub(super) fn copied(&self, t: Ty) -> Vec<u32> {
        if coll_kind(self.pool(), self.env(), t).is_some() {
            vec![0]
        } else {
            coll_leaves(self.pool(), self.env(), t)
        }
    }

    /// One count more (`retain`) or less on each counted reference of the
    /// value in locals `ls` of type `t`. A count more of a collection is
    /// its copy glue's result, which replaces it in `ls`: a clone when the
    /// collection is pinned or exposed (the pin's eager copy, until phase 10 of `docs/future-work/POINTER_LOANS.md`), so `ls` must hold
    /// a value no count of its own keeps, which the count makes a copy.
    pub(super) fn count_locals(&mut self, ls: &[u32], t: Ty, retain: bool) -> StageResult<()> {
        let start = self.f.body.len();
        // A `&dyn`'s data count stands by design (#1354).
        let reason = if self.ref_target(t).is_some_and(|(_, fat)| fat) {
            Some(Reason::ByDesign)
        } else {
            self.f.reason
        };
        let was = std::mem::replace(&mut self.f.reason, reason);
        self.assert_reason_fact(t);
        let result = self.count_locals_inner(ls, t, retain);
        self.f.reason = was;
        if result.is_ok() && self.f.origins.is_some() {
            let subject = crate::views::count_subject(self.pool(), self.env(), t);
            self.f.note_count(
                start,
                u32::try_from(self.b.anchor_len()).expect("TIR instructions"),
                if retain {
                    crate::views::CountAction::Retain
                } else {
                    crate::views::CountAction::Release
                },
                &subject,
                reason,
            );
        }
        result
    }

    /// A checked build's test of the named reason on the count of a `t`
    /// value, from a fact of the code writing it: what `Overwritten`
    /// replaces has its store's `Release`, `Cancelled` and a frame's
    /// `ByDesign` only run where a suspending body ends, `ByDesign` otherwise
    /// counts a `&dyn`'s data.
    fn assert_reason_fact(&self, t: Ty) {
        if cfg!(target_arch = "wasm32") || !self.f.names_counts {
            return;
        }
        let at = self.emitting.last().copied();
        let holds = match self.f.reason {
            Some(Reason::Overwritten) => {
                at.is_some_and(|i| self.b.tags[i as usize] != Tag::Store || self.store_releases(i))
            }
            Some(Reason::Cancelled) => self.susp.is_some(),
            Some(Reason::ByDesign) => {
                self.ref_target(t).is_some_and(|(_, fat)| fat) || self.susp.is_some()
            }
            _ => true,
        };
        assert!(
            holds,
            "{}: a count named {} has no such fact at this site",
            self.f.name,
            self.f.reason.expect("a named reason"),
        );
    }

    /// A checked build's test that what a cancellation releases is what the
    /// body says a suspension point may hold: a value in the unwind set, a
    /// local in the exit set of this sub-body.
    pub(super) fn assert_cancelled(&self, root: &super::drop_flags::Root) {
        use super::drop_flags::Root;
        if cfg!(target_arch = "wasm32")
            || !self.f.names_counts
            || self.f.reason != Some(Reason::Cancelled)
        {
            return;
        }
        let holds = match *root {
            Root::Value(v) => self.b.unwind_drops.iter().any(|r| r.1 == v),
            Root::Local(l) => self
                .b
                .exit_drops
                .iter()
                .any(|&(s, x)| s.raw() == u32::from(self.sub_id) && x.idx() == l),
        };
        assert!(
            holds,
            "{}: a cancelled release of what no suspension point holds",
            self.f.name
        );
    }

    /// Runs `f` with `reason` as the reason of the count code it writes
    /// ([`crate::module::Func::reason`]).
    pub(super) fn because<T>(&mut self, reason: Reason, f: impl FnOnce(&mut Self) -> T) -> T {
        let was = self.f.reason.replace(reason);
        let out = f(self);
        self.f.reason = was;
        out
    }

    fn count_locals_inner(&mut self, ls: &[u32], t: Ty, retain: bool) -> StageResult<()> {
        if !retain && self.drop_inline(ls, t)? {
            return Ok(());
        }
        let Some(mut w) = self.counted(t) else {
            return Ok(());
        };
        let colls = if retain { self.copied(t) } else { Vec::new() };
        for &c in &colls {
            self.f.get(ls[c as usize]);
            self.f.helper(Helper::CollCopy);
            self.f.set(ls[c as usize]);
        }
        if !colls.is_empty() {
            match w.without(&colls) {
                Some(rest) => w = rest,
                None => return Ok(()),
            }
        }
        if let (Some(k), Some(addr)) = (w.single(), w.addr()) {
            self.f.get(ls[k as usize]);
            if w.known() {
                if retain {
                    self.f.helper(Helper::RetainKnown);
                } else {
                    crate::rt::release_call(&mut self.f, addr);
                }
            } else {
                self.ref_count(retain);
            }
            return Ok(());
        }
        for l in ls {
            self.f.get(*l);
        }
        let vals = self.vals(t)?;
        self.f.helper(if retain {
            Helper::Retain { vals, walk: w }
        } else {
            Helper::Release { vals, walk: w }
        });
        Ok(())
    }

    /// In checked mode, notes top-level binding `h` of type `t`, in static
    /// slot `slot` when some reference refers to it, as one whose counted
    /// references the audit at exit releases (`crates/hd_wasm/DESIGN.md`
    /// §23.24).
    pub(super) fn note_stored(
        &mut self,
        h: hd_base::Hash128,
        t: Ty,
        slot: Option<(crate::module::Address, Vec<crate::layout::Leaf>)>,
    ) -> StageResult<()> {
        if self.input.runtime.and_then(|r| r.checked).is_none() {
            return Ok(());
        }
        let Some(walk) = self.counted(t) else {
            return Ok(());
        };
        let vals = self.vals(t)?;
        self.m
            .stored
            .insert(h, crate::module::Stored { vals, walk, slot });
        Ok(())
    }

    /// One count more or less on the address on the stack.
    fn ref_count(&mut self, retain: bool) {
        match (retain, self.input.runtime) {
            (true, Some(r)) => self.f.count_call(
                crate::module::Sym::Inst(r.retain),
                crate::views::CountAction::Retain,
            ),
            (false, Some(_)) => self.f.helper(Helper::ReleaseRef),
            (_, None) => self.f.op(crate::op::Op::Drop),
        }
    }

    /// One count more or less on TIR local `l`'s value in `ls`: its
    /// cell's, when it lives in one.
    pub(super) fn count_local(&mut self, l: usize, ls: &[u32], retain: bool) -> StageResult<()> {
        if self.is_cell(l) {
            self.assert_cancelled(&super::drop_flags::Root::Local(l));
            // A slot has one holder, its local (`closure::slot_locals`).
            if self.slots.contains_key(&l) {
                // ruled out: only captures request a retained local; storage::run
                // gives their child-body storage cells, while slots belong to sub 0.
                assert!(
                    !retain,
                    "storage::run and closure::slot_locals: a slot has only its local holder"
                );
                return self.release_cell_of(l, ls[0]);
            }
            self.f.get(ls[0]);
            self.f.helper(if retain {
                Helper::RetainKnown
            } else {
                Helper::ReleaseKnown
            });
            return Ok(());
        }
        let t = self.sub(self.b.local_ty[l]);
        self.count_locals(ls, t, retain)
    }

    pub(super) fn retain_locals(&mut self, ls: &[u32], t: Ty) -> StageResult<()> {
        self.count_locals(ls, t, true)
    }

    pub(super) fn release_locals(&mut self, ls: &[u32], t: Ty) -> StageResult<()> {
        self.count_locals(ls, t, false)
    }

    /// The `t` value in locals `ls`, which keep it, as a copy with counts
    /// of its own for another place: each collection is the copy glue's
    /// result in a local of its own ([`Em::count_locals`]).
    pub(super) fn copy_locals(&mut self, ls: &[u32], t: Ty) -> StageResult<Vec<u32>> {
        let mut out = ls.to_vec();
        for c in self.copied(t) {
            let x = self.f.local(Val::I32);
            self.f.get(ls[c as usize]);
            self.f.set(x);
            out[c as usize] = x;
        }
        self.retain_locals(&out, t)?;
        Ok(out)
    }

    /// Value `r` as a copy with counts of its own, for a place that keeps
    /// it ([`Em::copy_locals`]). A constant is static data, which has no
    /// count.
    pub(super) fn retain_ref(&mut self, r: u32) -> StageResult<Src> {
        if r == NONE
            || Ref(r).as_inst().is_none()
            || !self.tracked(self.ty_of(r))
            || self.short_holders.contains(&r)
        {
            return Ok(Src::Ref(r));
        }
        let ls = self.computed(r);
        Ok(Src::Locals(self.copy_locals(&ls, self.ty_of(r))?))
    }

    /// Operand `r` for the instruction being emitted, which consumes it:
    /// moved in at its only use ([`Em::take`]), else a copy with counts of
    /// its own ([`Em::retain_ref`]).
    pub(super) fn consumed(&mut self, r: u32) -> StageResult<Src> {
        if self.take(r) {
            return Ok(Src::Ref(r));
        }
        self.retain_ref(r)
    }

    /// Stores the values on the stack as instruction `i`'s. Unless `owned`
    /// (a call's result, a new object), the value is a copy and its place
    /// takes a count of its own. The value is consumed or dropped, and
    /// zeroed, before its instruction runs again.
    pub(super) fn store_as(&mut self, i: u32, owned: bool) -> StageResult<()> {
        let ls = self.slot(i)?;
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        let t = self.ty(i);
        // A stated reference copy lowers at its `Retain` (`Em::own_retain`).
        let stated = self.b.tags[i as usize] == Tag::Ref
            && hd_tir::ir::count::following(self.b, &self.topo, i, Tag::Retain)
                == Some(Info::new(At::Value, Part::Result, 0));
        if !stated && !owned && self.tracked(t) && !self.loans.contains(&i) {
            self.retain_locals(&ls, t)?;
        }
        self.mark_drop_root(super::drop_flags::Root::Value(i), t, true);
        Ok(())
    }

    /// Stores a value that arrives with its own count as instruction `i`'s.
    pub(super) fn store_owned(&mut self, i: u32) -> StageResult<()> {
        self.store_as(i, true)
    }

    /// Whether operand `r` moves its count into the instruction being
    /// emitted, which consumes it ([`Em::take_from`]): then no count is
    /// taken for the consumer, and `r`'s locals are zeroed once the
    /// instruction has read them ([`Em::settle`]).
    pub(super) fn take(&mut self, r: u32) -> bool {
        let Some(&u) = self.emitting.last() else {
            return false;
        };
        self.take_from(r, u)
    }

    /// Zeroes the locals of the operands whose counts moved into
    /// instruction `u` ([`Em::take`]).
    pub(super) fn settle(&mut self, u: u32) {
        let (done, rest): (Vec<_>, Vec<_>) = self.moving.drain(..).partition(|m| m.0 == u);
        self.moving = rest;
        for (_, r) in done {
            self.zero_locals_of(r);
        }
    }

    /// Zeroes moved value `r`'s locals now, before a branch leaves the
    /// instruction that took it.
    pub(super) fn settle_value(&mut self, r: u32) {
        self.moving.retain(|m| m.1 != r);
        self.zero_locals_of(r);
    }

    fn zero_locals_of(&mut self, r: u32) {
        // Only a suspending body's cancelled way out reads a moved value's
        // locals again: a cleanup scope's ladder (`release_unwound`)
        // releases the unwind rows of every point in it, and the zero says
        // this one holds nothing. A value is never dropped after its move,
        // so outside a frame nothing else tests it.
        if self.susp.is_some() {
            for l in self.vals.get(&r).cloned().unwrap_or_default() {
                self.f.zero(l);
            }
            self.mark_drop_root(super::drop_flags::Root::Value(r), self.ty_of(r), false);
        }
    }

    /// Releases value `x`'s count, when it holds a counted value, and
    /// zeroes its locals.
    pub(super) fn release_value(&mut self, x: u32) -> StageResult<()> {
        let Some(ls) = self.vals.get(&x).cloned() else {
            return Ok(());
        };
        let t = self.ty(x);
        if !self.needs_drop(t) {
            return Ok(());
        }
        if self.owned_refs.contains(&x) {
            self.f.get(ls[0]);
            self.f.helper(Helper::ReleaseKnown);
        } else if !self.loans.contains(&x) {
            self.release_drop_place(&ls, t, &Self::value_place(x))?;
        }
        for l in ls {
            self.f.zero(l);
        }
        Ok(())
    }

    /// Releases TIR local `l`'s count, when it holds a counted value, and
    /// zeroes its locals.
    pub(super) fn release_local(&mut self, l: usize) -> StageResult<()> {
        if self.borrowed.contains(&l) {
            return Ok(());
        }
        let Some(ls) = self.locals.get(&l).cloned() else {
            return Ok(());
        };
        if !self.is_cell(l) && !self.needs_drop(self.sub(self.b.local_ty[l])) {
            return Ok(());
        }
        self.release_owned_local(l, &ls)?;
        for x in ls {
            self.f.zero(x);
        }
        Ok(())
    }

    /// Releases the count local `l` (Wasm locals `ls`) holds. A binding
    /// local ([`Em::loan_locals`]) holds a borrow, with no count.
    fn release_owned_local(&mut self, l: usize, ls: &[u32]) -> StageResult<()> {
        self.borrow_release(l);
        if self.owned_locals.contains(&l) {
            self.f.get(ls[0]);
            self.f.op(crate::op::Op::If(wasm_encoder::BlockType::Empty));
            self.f.get(ls[0]);
            self.f.helper(Helper::ReleaseKnown);
            self.f.op(crate::op::Op::End);
            return Ok(());
        }
        match self.loan_locals.get(&l) {
            None if !self.is_cell(l) => {
                self.release_drop_place(ls, self.sub(self.b.local_ty[l]), &Self::local_place(l))
            }
            None => self.count_local(l, ls, false),
            Some(_) => Ok(()),
        }
    }

    /// Whether the function holds a counted reference anywhere: a
    /// parameter, a local or an instruction's value. Only then does it
    /// count, and its returns go through its exit.
    pub(super) fn counts_anything(&self) -> bool {
        if self.input.runtime.is_none() {
            return false;
        }
        let b = self.b;
        self.borrow_needs_exit()
            || !self.cells.is_empty()
            || (0..b.local_ty.len()).any(|l| self.needs_drop(self.sub(b.local_ty[l])))
            || (0..b.ty.len()).any(|i| self.needs_drop(self.ty(u32::try_from(i).expect("insts"))))
    }

    /// Releases what the function owns at its one exit: its sub-body's
    /// exit set.
    pub(super) fn release_all(&mut self) -> StageResult<()> {
        let outer = self.f.origin_at;
        self.f.origin_at = self.f.origin_root;
        let was = self.f.reason;
        let result = self.release_all_inner();
        self.f.reason = was;
        self.f.origin_at = outer;
        result
    }

    fn release_all_inner(&mut self) -> StageResult<()> {
        // The exit set: the locals that may still hold a value here.
        self.f.reason = Some(Reason::LastUse);
        let b = self.b;
        let mut released = hd_base::HashSet::default();
        for &(s, l) in &b.exit_drops {
            if s.raw() != u32::from(self.sub_id) || self.borrowed.contains(&l.idx()) {
                continue;
            }
            if let Some(ls) = self.locals.get(&l.idx()).cloned() {
                self.release_owned_local(l.idx(), &ls)?;
                released.insert(l.idx());
            }
        }
        // A value made only of references needs no ownership drop, so
        // the move pass may omit it from `exit_drops`. Its contained
        // run-time loans still end when the value is released here.
        self.borrow_exit();
        for l in self.borrow_locals() {
            if !released.contains(&l) {
                self.borrow_release(l);
            }
        }
        // Exit release descriptors are lowered here on every return, but
        // for a parameter this instance receives lent: the whole-program
        // pass lends it, or its type here is no owned one.
        let params = super::param_locals(self.b);
        let mut exits: Vec<(usize, Reason)> = self
            .b
            .tags
            .iter()
            .enumerate()
            .filter_map(|(i, &tag)| {
                if tag != Tag::Release {
                    return None;
                }
                let c = Info::read(self.b, i)?;
                let Part::Local(local) = c.part else {
                    return None;
                };
                (c.at == At::Exit
                    && self.topo.sub[i] == u32::from(self.sub_id)
                    && (params.iter().position(|&p| p == local as usize)).is_some_and(|k| {
                        let ty = self.sub(self.b.local_ty[local as usize]);
                        matches!(self.pass_at(self.lends, k, ty), Pass::Owned(_))
                    }))
                .then_some((local as usize, Reason::from_word(c.reason)))
            })
            .collect();
        exits.sort_stable_by_key(|x| x.0);
        for (l, reason) in exits {
            self.f.reason = Some(reason);
            if let Some(ls) = self.locals.get(&l).cloned() {
                if self.is_cell(l) {
                    self.count_local(l, &ls, false)?;
                } else {
                    self.release_drop_place(
                        &ls,
                        self.sub(self.b.local_ty[l]),
                        &Self::local_place(l),
                    )?;
                }
            }
        }
        // Saved reference-object slots have their own normal-exit contract.
        for (l, held) in self.frame_refs.clone() {
            let local = super::required(
                u32::try_from(l).ok(),
                "frame_refs must contain u32 TIR parameter IDs from own::referent",
            );
            // ruled out: own::referent states this saved reference's Exit release.
            let reason = self.required_stated_count(Tag::Release, At::Exit, Part::Referent(local));
            self.because(reason, |em| {
                em.f.get(held);
                em.f.helper(Helper::ReleaseRef);
            });
        }
        let nodes = self
            .susp
            .as_ref()
            .map(super::susp::Susp::wakers)
            .unwrap_or_default();
        for (node, i) in nodes {
            self.count_point_ref(i, Part::Waker, node, false);
        }
        Ok(())
    }
}

impl Em<'_, '_, '_> {
    /// The consumer of each value of an owned body, by the passes' rule
    /// (`hd_tir::own::consumers`), and the body's per-value use facts
    /// ([`Em::used`], [`Em::value_uses`], [`Em::part_only`]) in one pass
    /// over its operands: O(instructions + operands).
    pub(super) fn own_consumers(&mut self) {
        hd_tir::own::consumers(self.b, &self.topo, self.input.own_cx, &mut self.consumer);
        let verify = cfg!(not(target_arch = "wasm32"))
            && self
                .input
                .runtime
                .and_then(|runtime| runtime.checked)
                .is_some();
        if self.m.track_origins || verify {
            hd_tir::own::copy_reasons(
                self.b,
                &self.topo,
                self.input.own_cx,
                &mut self.copy_reasons,
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        if verify {
            let bad = hd_tir::own::check_copy_reasons(
                self.b,
                &self.topo,
                self.input.own_cx,
                &self.copy_reasons,
            );
            assert!(
                bad.is_empty(),
                "the reason verifier rejects a body of item {:?}:\n{}",
                self.b.item,
                bad.join("\n")
            );
        }
        self.used.clear();
        self.used.resize(self.b.len(), false);
        self.value_uses.clear();
        self.value_uses.resize(self.b.len(), 0);
        self.part_only.clear();
        self.part_only.resize(self.b.len(), true);
        let mut ops = std::mem::take(&mut self.own_ops);
        for i in 0..self.b.len() {
            if self.b.tags[i] == Tag::Retain
                && Info::read(self.b, i).is_some_and(|c| c.at == At::Value)
            {
                self.used[self.b.data[i][0] as usize] = true;
            }
            hd_tir::own::operands(self.b, i, &mut ops);
            let part = super::refs::part_use(self.b, i);
            for op in &ops {
                self.used[op.value as usize] = true;
                if op.role != hd_tir::ir::Role::Dropped {
                    self.value_uses[op.value as usize] += 1;
                }
                if part != Some(op.value) {
                    self.part_only[op.value as usize] = false;
                }
            }
        }
        crate::work::use_facts(self.b.len());
        self.own_ops = ops;
    }

    /// In an owned body, at an instance that receives some of the
    /// function's parameters `plocals` borrowed (a type parameter's made a
    /// reference, or a reference the passes own): the parameters that take
    /// no count here, and the reads of them that take none. Such a
    /// parameter is the caller's for the whole call, and the body never
    /// stores, addresses or captures it, so a read of all of it needs no
    /// count of its own where nothing keeps its value: a read the passes
    /// let every use lend, or one that a call's argument consumes which
    /// the callee borrows at this instance too (`hash_of(a)` for `a: &T`).
    /// A parameter the passes own (its sub-body's exit set lists it) takes
    /// none only when every read of it is such a read; one they leave
    /// unowned is borrowed here already, and its other reads copy as they
    /// say. A read a move takes a part of (`Read move [%r as .v .k]`, as a
    /// `match` binds a payload) keeps its count, which the move takes. The
    /// reads join [`Em::loans`]. With `framed`, every parameter is taken
    /// as borrowed: a suspending body with no suspension point runs while
    /// its frame holds a count of each. A framed parameter lends only when
    /// every read of it reads all of it.
    pub(super) fn instance_loans(
        &mut self,
        plocals: &[usize],
        framed: bool,
    ) -> hd_base::HashSet<usize> {
        use hd_tir::ir::place;
        let b = self.b;
        let mut out = hd_base::HashSet::default();
        // The values a move takes a part of, or all of.
        let moved: hd_base::HashSet<u32> = (0..b.len())
            .filter(|&i| {
                b.tags[i] == Tag::Read && b.data[i][1] == hd_tir::ir::ReadMode::Move as u32
            })
            .filter_map(|i| match b.view(b.data[i][0]).root {
                hd_tir::ir::Root::Value(v) => Some(v),
                _ => None,
            })
            .collect();
        for (k, &l) in plocals.iter().enumerate() {
            let t = self.sub(b.local_ty[l]);
            // A framed parameter (a suspending body's with no suspension
            // point) is held by the frame, as a borrowed one by its caller;
            // one holding a collection is not: the frame holds a copy of
            // it, which the body releases.
            if framed && !self.copied(t).is_empty() {
                continue;
            }
            if !(framed
                || matches!(
                    self.pass_at(self.lends, k, t),
                    crate::layout::Pass::Borrowed
                ))
                || self.is_cell(l)
                // A closure copying the parameter into its environment
                // counts its own copy (`Em::closure`), as one moving a lent
                // parameter in does; a shared one moves it into its cell.
                || b
                    .cap_local
                    .iter()
                    .zip(&b.cap_mode)
                    .any(|(c, m)| c.idx() == l && m.shares())
            {
                continue;
            }
            let owned = b
                .exit_drops
                .iter()
                .any(|&(s, x)| s.raw() == u32::from(self.sub_id) && x.idx() == l);
            let mut fits = true;
            let mut loans = Vec::new();
            for i in 0..b.len() {
                let tag = b.tags[i];
                if !matches!(tag, Tag::Read | Tag::Store | Tag::Ref) {
                    continue;
                }
                let w = b.data[i][0];
                if w & place::BARE_LOCAL == 0 {
                    // A framed parameter lends only through whole reads,
                    // the reads this walk follows.
                    if (tag != Tag::Read || framed)
                        && b.local_in_place(w).is_some_and(|x| x.idx() == l)
                    {
                        fits = false;
                    }
                    continue;
                }
                if bare_local(b, w) != l {
                    continue;
                }
                if tag != Tag::Read {
                    fits = false;
                    continue;
                }
                let x = u32::try_from(i).expect("insts");
                let user = self.consumer.get(i).copied().unwrap_or(NONE);
                if !moved.contains(&x) && (user == NONE || self.borrowed_arg(user, x)) {
                    loans.push(x);
                } else {
                    fits = false;
                }
            }
            if owned && !fits {
                continue;
            }
            if owned {
                out.insert(l);
            }
            self.loans.extend(loans);
        }
        out
    }

    /// The copied captures of closure `ci`'s code that its environment
    /// keeps for the call and the code only lends: every naming of one in
    /// this sub-body is a read of all of it whose value no use keeps (or a
    /// call argument its callee borrows here too), and no move takes a part
    /// of such a read. They take no count on entry and none at the exit;
    /// the reads join [`Em::loans`]. A shared capture is a cell, and a
    /// capture holding a collection is not taken (its count is a copy).
    pub(super) fn capture_loans(&mut self, ci: usize) -> hd_base::HashSet<usize> {
        use hd_tir::ir::place;
        let b = self.b;
        let sub = u32::from(self.sub_id);
        let mut out = hd_base::HashSet::default();
        let moved: hd_base::HashSet<u32> = (0..b.len())
            .filter(|&i| {
                b.tags[i] == Tag::Read && b.data[i][1] == hd_tir::ir::ReadMode::Move as u32
            })
            .filter_map(|i| match b.view(b.data[i][0]).root {
                hd_tir::ir::Root::Value(v) => Some(v),
                _ => None,
            })
            .collect();
        for (l, shared) in super::closure::captures(b, ci) {
            let t = self.sub(b.local_ty[l]);
            if shared || self.is_cell(l) || !self.copied(t).is_empty() {
                continue;
            }
            let mut fits = true;
            let mut loans = Vec::new();
            for i in 0..b.len() {
                if self.topo.sub[i] != sub {
                    continue;
                }
                let tag = b.tags[i];
                if tag == Tag::Closure {
                    let rec = b.record(b.data[i][1]);
                    if (rec[0]..rec[0] + rec[1]).any(|c| b.cap_local[c as usize].idx() == l) {
                        fits = false;
                    }
                    continue;
                }
                if !matches!(tag, Tag::Read | Tag::Store | Tag::Ref) {
                    continue;
                }
                let w = b.data[i][0];
                if w & place::BARE_LOCAL == 0 {
                    if b.local_in_place(w).is_some_and(|x| x.idx() == l) {
                        fits = false;
                    }
                    continue;
                }
                if bare_local(b, w) != l {
                    continue;
                }
                if tag != Tag::Read {
                    fits = false;
                    continue;
                }
                let x = u32::try_from(i).expect("insts");
                let user = self.consumer.get(i).copied().unwrap_or(NONE);
                if !moved.contains(&x) && (user == NONE || self.borrowed_arg(user, x)) {
                    loans.push(x);
                } else {
                    fits = false;
                }
            }
            if fits {
                out.insert(l);
                self.loans.extend(loans);
            }
        }
        out
    }

    /// Whether call `c` passes value `r` as an argument its callee borrows
    /// at this instance (`layout::Pass::Borrowed`).
    fn borrowed_arg(&self, c: u32, r: u32) -> bool {
        if self.b.tags.get(c as usize) != Some(&Tag::Call) {
            return false;
        }
        let mut args = self.rec(self.b.data[c as usize][1]);
        args.truncate(args.len().saturating_sub(3));
        if matches!(self.calls.get(&c), Some(hd_mono::Target::Dyn)) {
            // A dynamic call lends every argument to its slot. The slot
            // takes any count its concrete target owns, and a blocking
            // drive keeps the caller live until it returns.
            return args.iter().filter(|&&a| a == r).count() == 1;
        }
        let Some(hd_mono::Target::Call(ct)) = self.calls.get(&c) else {
            return false;
        };
        if !matches!(ct.kind, hd_mono::TargetKind::Instance) {
            return false;
        }
        let Some(params) = self.param_tys(ct.key, ct.item, ct.args) else {
            return false;
        };
        if params.len() != args.len() || args.iter().filter(|&&a| a == r).count() != 1 {
            return false;
        }
        let k = args.iter().position(|&a| a == r).expect("argument");
        let lends = self.env().lends(ct.item, ct.args);
        matches!(
            self.pass_at(lends, k, params[k]),
            crate::layout::Pass::Borrowed
        )
    }

    /// In an owned body: whether value `r`, which `user` consumes, moves
    /// its count into the instruction or block being emitted. It is
    /// zeroed once `user` has read it ([`Em::settle`]).
    pub(super) fn take_from(&mut self, r: u32, user: u32) -> bool {
        if !self.counting
            || r == NONE
            || Ref(r).as_inst().is_none()
            || self.consumer.get(r as usize) != Some(&user)
            || !self.vals.contains_key(&r)
            || !self.needs_drop(self.ty_of(r))
            || self.moving.contains(&(user, r))
        {
            return false;
        }
        self.moving.push((user, r));
        true
    }

    /// `Drop [$l]` or `Drop [%v]` `i`: releases the place's value and
    /// zeroes it, so a later drop of it releases nothing. No pass writes a
    /// `Drop` of a part of a place.
    pub(super) fn own_drop(&mut self, i: u32) -> StageResult<()> {
        let word = self.b.data[i as usize][0];
        if word & hd_tir::ir::place::BARE_LOCAL != 0 {
            let l = bare_local(self.b, word);
            return self.release_local(l);
        }
        // ruled out: own::insert writes only [ROOT_VALUE, instruction] records
        // for non-local Drops; moves leave that whole root Drop in place.
        let value = self.b.record(word)[1];
        if self.counting && self.own_drop_rest(i, value)? {
            return Ok(());
        }
        self.release_value(value)
    }

    /// `LoanEnd` `i`: the holder's reference counts nothing, so only its
    /// locals go, as they do at a `Drop`, and no stale address is left for
    /// a later round or an unwind to test.
    pub(super) fn own_loan_end(&mut self, i: u32) {
        let p = self.b.view(self.b.data[i as usize][0]);
        let (t, ls) = match p.root {
            hd_tir::ir::Root::Local(l) if !self.borrowed.contains(&(l as usize)) => (
                self.sub(self.b.local_ty[l as usize]),
                self.locals.get(&(l as usize)),
            ),
            hd_tir::ir::Root::Value(v) => (self.ty(v), self.vals.get(&v)),
            _ => return,
        };
        if !self.needs_drop(t) {
            return;
        }
        for l in ls.cloned().unwrap_or_default() {
            self.f.zero(l);
        }
    }

    /// `IsUnique` `i`: the list's or map's buffer made the place's own
    /// (`rt:list_unique`, `rt:map_unique`), and stored there.
    pub(super) fn own_unique(&mut self, i: u32) -> StageResult<()> {
        use hd_tir::ir::unique_word::{FRESH, HOISTED, REPEAT};
        match self.b.data[i as usize][1] {
            // Of a buffer nothing else can hold, or one an earlier proof
            // left unique: no check.
            FRESH | REPEAT => return Ok(()),
            // It runs at the loop that follows (`Em::hoisted_uniques`).
            HOISTED => {
                self.hoisted.push(i);
                return Ok(());
            }
            _ => {}
        }
        let h = self.unique_at(i)?;
        self.walked = self.read_after(i).map(|r| (r, h));
        Ok(())
    }

    /// The uniqueness check of `IsUnique` `i`'s place, which stores the
    /// buffer it leaves there, and the local that holds that buffer.
    pub(super) fn unique_at(&mut self, i: u32) -> StageResult<u32> {
        let word = self.b.data[i as usize][0];
        let (h, t, at) = self.place_header(word)?;
        if self.input.runtime.is_none() {
            return Ok(h);
        }
        self.f.get(h);
        let start = self.f.body.len();
        if self.is_map(t) {
            let (_, _, e) = self.entry(t)?;
            self.f.helper(Helper::MapUnique { entry: e });
        } else {
            let (_, e) = self.elem(t)?;
            self.f.helper(Helper::ListUnique { elem: e });
        }
        if self.f.origins.is_some() {
            let subject = crate::views::count_subject(self.pool(), self.env(), t);
            self.f.note_count(
                start,
                u32::try_from(self.b.anchor_len()).expect("TIR instructions"),
                crate::views::CountAction::Unique,
                &subject,
                hd_tir::own::reason(self.b, i as usize),
            );
        }
        self.f.set(h);
        // Collection places are physical slots. Install the returned
        // header at the same address, transferring its existing count.
        if let Some(at) = at {
            self.put_at(&at, h);
        } else {
            self.put_buffer(word, h)?;
        }
        Ok(h)
    }

    /// The `copy` or `borrow` read of `IsUnique` `i`'s place, through a
    /// step, that stands right after it in its block: the change's operand, which takes the buffer the
    /// check left ([`Em::read_walked`]).
    fn read_after(&self, i: u32) -> Option<u32> {
        use hd_tir::ir::ReadMode::{Borrow, Copy};
        let blk = *self.topo.blk.get(i as usize).filter(|&&blk| blk != NONE)?;
        let pos = self.topo.pos[i as usize];
        let r = *self
            .rec(self.b.data[blk as usize][0])
            .get(pos as usize + 1)?;
        let [a, mode] = self.b.data[r as usize];
        let read =
            self.b.tags[r as usize] == Tag::Read && [Copy as u32, Borrow as u32].contains(&mode);
        let place = self
            .b
            .decode_place(a)
            .filter(|p| read && !p.proj.is_empty())?;
        (Some(place) == self.b.decode_place(self.b.data[i as usize][0])).then_some(r)
    }

    /// `Read` `i` of the place whose buffer the `IsUnique` right before it
    /// left in local `h`: one walk for both.
    pub(super) fn read_walked(&mut self, i: u32, h: u32) -> StageResult<()> {
        self.f.get(h);
        self.store(i)
    }

    /// `Read [$l]` `i` of an owned body, as its mode says: a copy takes a
    /// count of its own; a move takes the local's and empties it; a
    /// borrow takes none and is no place's to release. The passes decide
    /// every read's mode (`verify_owned`, "decided"), so a read that is
    /// neither a move nor a borrow is a copy.
    pub(super) fn own_local_read(&mut self, i: u32, l: usize) -> StageResult<()> {
        use hd_tir::ir::ReadMode;
        let mode = ReadMode::from_u32(self.b.data[i as usize][1]);
        if self.is_cell(l) {
            return match mode {
                Some(ReadMode::Borrow) => {
                    self.loans.insert(i);
                    self.cell_get(i, l)
                }
                // A last whole-value read moves the payload out and ends
                // the cell. A last reference instead takes the cell's
                // count (`docs/compiler/tir-v2.md`, "Move" rule 4).
                Some(ReadMode::Move) => self.cell_take(i, l),
                _ => self.cell_get(i, l),
            };
        }
        let ls = self.local(u32::try_from(l).expect("locals"))?;
        for x in &ls {
            self.f.get(*x);
        }
        match mode {
            Some(ReadMode::Move) => {
                self.store_owned(i)?;
                self.mark_drop_root(
                    super::drop_flags::Root::Local(l),
                    self.sub(self.b.local_ty[l]),
                    false,
                );
                if self.needs_drop(self.ty(i)) {
                    for x in ls {
                        self.f.zero(x);
                    }
                }
                Ok(())
            }
            Some(ReadMode::Borrow) => {
                self.loans.insert(i);
                self.store(i)
            }
            _ => self.store(i),
        }
    }

    /// The release on a cancelled suspension point's way out of an owned
    /// body: the cancel edge is no TIR jump, so the values the passes list
    /// for the points it may come from (`Body::unwind_drops`: the point
    /// itself, or every point inside the cleanup scope `from` whose ladder
    /// takes it) are released (and zeroed) here. The exit then releases
    /// the exit set, which holds every local that may hold a value at a
    /// suspension point.
    pub(super) fn release_unwound(&mut self, from: u32) -> StageResult<()> {
        let (Some((lo, hi)), Some(s)) = (self.range(from), self.susp.as_ref()) else {
            return Ok(());
        };
        let rows = &self.b.unwind_drops;
        let mut vals: Vec<u32> = Vec::new();
        for (&p, &k) in &s.states {
            if (lo..=hi).contains(&k) {
                let at = rows.partition_point(|r| r.0 < p);
                vals.extend(rows[at..].iter().take_while(|r| r.0 == p).map(|r| r.1));
            }
        }
        vals.sort_stable();
        vals.dedup();
        self.because(Reason::Cancelled, |em| {
            vals.into_iter().try_for_each(|v| em.release_value(v))
        })
    }
}
