//! Counting (`crates/hd_wasm_linear/DESIGN.md` §23.16): every place that
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
//! generic body cannot see: a consumer that cannot move a count in takes
//! one of its own and the value is released after it
//! ([`Em::release_untaken`]), an argument a callee instance owns but the
//! passes lent takes a count (`push_args`), a parameter the caller lends
//! takes one on entry when the body owns it (its exit set lists it), one
//! the passes left unowned that an instance receives owned is released at
//! the exit, and a cancelled suspension point releases the values the
//! passes' unwind set lists for it, before the exit releases the exit set
//! ([`Em::release_unwound`]). A `borrow` read takes no count, and a local
//! a borrow is stored into holds none ([`Em::prepare_own_loans`]).
//!
//! An erased body's thunk runs one instruction of such a body as glue of
//! its own (`Em::owned` is false there): its operands and captures arrive
//! with counts, its reads copy, and its exit releases every place it holds.

use hd_base::SliceSort as _;
use hd_base::StageResult;
use hd_tir::ir::{NONE, Ref, Tag};
use hd_types::Ty;

use super::Em;
use super::value::Src;
use crate::layout::{Walk, coll_kind, coll_leaves, walk};
use crate::module::Val;
use crate::rt::Helper;

impl Em<'_, '_, '_> {
    /// An owned body's `borrow` reads and binding locals
    /// (`docs/compiler/tir-v2.md`, "Borrow"): a borrow takes no count, so
    /// it joins [`Em::loans`], and its read lowers
    /// as a copy's would with no count. One that an erased body's thunk
    /// makes is that thunk's fresh value, with its count, which the
    /// read's `Drop` (the pass keeps it) releases. One through a reference
    /// takes no count on a physical place; a view's getter makes a fresh
    /// value there, which a flag ([`Em::view_loans`]) marks for the read's
    /// `Drop`. A local the passes store a borrow into holds no count
    /// ([`Em::loan_locals`]): its stores count
    /// nothing and release nothing.
    pub(super) fn prepare_own_loans(&mut self) {
        use hd_tir::ir::place;
        for i in 0..self.b.len() {
            let [a, w] = self.b.data[i];
            let x = u32::try_from(i).expect("insts");
            match self.b.tags[i] {
                Tag::Read if self.b.is_borrow(x) => {
                    if matches!(self.calls.get(&x), Some(hd_mono::Target::Open(_))) {
                        continue;
                    }
                    self.loans.insert(x);
                    if a & place::BARE_LOCAL != 0 {
                        continue;
                    }
                    // A place rooted at a local that reads its referent
                    // through a reference after other steps
                    // (`[$l .r *]`, `Em::place_read`).
                    if let Some(p) = self.b.decode_place(a)
                        && Self::rooted(&p)
                    {
                        if let Ok(Some((_, Some((_, false))))) = self.last_through(&p) {
                            let flag = self.f.local(crate::module::Val::I32);
                            self.view_loans.insert(x, flag);
                        }
                        continue;
                    }
                    // `[*%r ...]` through a reference that may be a view.
                    let start = match self.b.extra.get(a as usize + 1..a as usize + 4) {
                        Some(&[place::ROOT_VALUE, r, step])
                            if step & place::PROJ_KIND == place::PROJ_DEREF =>
                        {
                            Some(self.ty_of(r))
                        }
                        Some(&[place::ROOT_LOCAL, l, step])
                            if step & place::PROJ_KIND == place::PROJ_DEREF =>
                        {
                            Some(self.sub(self.b.local_ty[l as usize]))
                        }
                        _ => None,
                    };
                    if start
                        .and_then(|t| self.ref_target(t))
                        .is_some_and(|(_, fat)| !fat)
                    {
                        let flag = self.f.local(crate::module::Val::I32);
                        self.view_loans.insert(x, flag);
                    }
                }
                // A reference to all of a local in a slot (§23.38) takes
                // no count: a count of a slot's address does nothing. One
                // that takes the local at its last access releases the
                // slot's value at its `Drop`.
                Tag::Ref
                    if super::shape::ref_local(self.b, i)
                        .is_some_and(|l| self.slots.contains_key(&l))
                        && self.b.decode_place(a).is_some_and(|p| p.proj.is_empty()) =>
                {
                    self.loans.insert(x);
                    if w & hd_tir::ir::ref_word::TAKES != 0
                        && let Some(l) = super::shape::ref_local(self.b, i)
                    {
                        self.slot_takes.insert(x, l);
                    }
                }
                // A reference into what a lent parameter refers to
                // (`ref_word::LENT`) takes no count where the referent is
                // physical; a view's projection makes one, which the flag
                // marks for the reference's `Drop`, as for a read.
                Tag::Ref
                    if w & hd_tir::ir::ref_word::LENT != 0
                        && !hd_mono::is_open(self.pool(), self.ty(x))
                        && !matches!(self.calls.get(&x), Some(hd_mono::Target::Open(_)))
                        && self.ref_target(self.ty(x)).is_some_and(|(_, fat)| !fat) =>
                {
                    self.loans.insert(x);
                    let flag = self.f.local(crate::module::Val::I32);
                    self.view_loans.insert(x, flag);
                }
                Tag::Store if self.b.is_borrow(w) => {
                    let l = if a & place::BARE_LOCAL != 0 {
                        Some(a & !place::BARE_LOCAL)
                    } else {
                        match self.b.extra.get(a as usize..a as usize + 3) {
                            Some(&[2, place::ROOT_LOCAL, l]) => Some(l),
                            _ => None,
                        }
                    };
                    if let Some(l) = l {
                        self.loan_locals.insert(l as usize, w);
                    }
                }
                _ => {}
            }
        }
    }

    /// Release the owned fallback of a view-backed loan, leaving a physical
    /// loan uncounted. The flag is cleared so loop and exit drops agree.
    fn release_view_loan(&mut self, i: u32, ls: &[u32]) -> StageResult<()> {
        let flag = self.view_loans[&i];
        self.f.get(flag);
        self.f.op(crate::op::Op::If(wasm_encoder::BlockType::Empty));
        self.release_locals(ls, self.ty(i))?;
        self.f.op(crate::op::Op::End);
        self.f.zero(flag);
        Ok(())
    }

    /// An instruction may run again in a loop. Its previous view temporary
    /// is independent of the caller-owned parameter backing the new read.
    pub(super) fn begin_view_loan(&mut self, i: u32) -> StageResult<Option<u32>> {
        let Some(&flag) = self.view_loans.get(&i) else {
            return Ok(None);
        };
        if self.loops > 0 {
            let ls = self.slot(i)?;
            self.release_view_loan(i, &ls)?;
        }
        Ok(Some(flag))
    }

    /// The counted references of a `t` value, when the program counts.
    pub(super) fn counted(&self, t: Ty) -> Option<Walk> {
        self.input.runtime?;
        walk(self.pool(), self.env(), t)
    }

    /// Whether a new object of type `t` (a data value, a self-recursive
    /// enum's cell, a list's or a map's) takes part in cycle collection
    /// (`crate::cycle`).
    pub(super) fn cyclic_object(&self, t: Ty) -> bool {
        self.input.runtime.is_some() && crate::cycle::object(self.pool(), self.env(), t)
    }

    /// Whether a new object holding values of `parts`, which only an open
    /// value can hold, takes part in cycle collection (`crate::cycle`).
    pub(super) fn cyclic_holding(&self, parts: &[Ty]) -> bool {
        self.input.runtime.is_some() && crate::cycle::holding(self.pool(), self.env(), parts)
    }

    /// The values of a `t` value that a count of it takes through the copy
    /// glue (`rt:coll_copy`, `types.coll.value`): a list's or a map's own,
    /// through a `mut self` receiver's view too, and each list or map in a
    /// struct's fields at any depth. A struct's `mut` view, a provider's,
    /// shares its fields.
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
    /// collection is pinned or exposed (`ref.elem.copy`), so `ls` must hold
    /// a value no count of its own keeps, which the count makes a copy.
    pub(super) fn count_locals(&mut self, ls: &[u32], t: Ty, retain: bool) -> StageResult<()> {
        let start = self.f.body.len();
        let result = self.count_locals_inner(ls, t, retain);
        if result.is_ok() && self.f.origins.is_some() {
            let subject = crate::views::count_subject(self.pool(), self.env(), t);
            self.f.note_count(
                start,
                u32::try_from(self.b.tags.len()).expect("TIR instructions"),
                if retain {
                    crate::views::CountAction::Retain
                } else {
                    crate::views::CountAction::Release
                },
                &subject,
            );
        }
        result
    }

    fn count_locals_inner(&mut self, ls: &[u32], t: Ty, retain: bool) -> StageResult<()> {
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
        if let Some(k) = w.single() {
            self.f.get(ls[k as usize]);
            if matches!(w, Walk::Known(_)) {
                self.f.helper(if retain {
                    Helper::RetainKnown
                } else {
                    Helper::ReleaseKnown
                });
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
    /// references the audit at exit releases (`crates/hd_wasm_linear/DESIGN.md`
    /// §23.24).
    pub(super) fn note_stored(
        &mut self,
        h: hd_base::Hash128,
        t: Ty,
        slot: Option<(u32, Vec<crate::layout::Leaf>)>,
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
            (true, Some(r)) => self.f.call(crate::module::Sym::Inst(r.retain)),
            (false, Some(_)) => self.f.helper(Helper::ReleaseRef),
            (_, None) => self.f.op(crate::op::Op::Drop),
        }
    }

    /// One count more or less on TIR local `l`'s value in `ls`: its
    /// cell's, when it lives in one.
    pub(super) fn count_local(&mut self, l: usize, ls: &[u32], retain: bool) -> StageResult<()> {
        if self.is_cell(l) {
            // A slot has one holder, its local (`closure::slot_locals`).
            if self.slots.contains_key(&l) {
                if retain {
                    return crate::unsupported("a second count of a local's slot");
                }
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
        if r == NONE || Ref(r).as_inst().is_none() || self.counted(self.ty_of(r)).is_none() {
            return Ok(Src::Ref(r));
        }
        let Some(ls) = self.vals.get(&r).cloned() else {
            return crate::unsupported("a value its instruction did not compute");
        };
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
    /// zeroed, before its instruction runs again (a thunk runs no loop).
    pub(super) fn store_as(&mut self, i: u32, owned: bool) -> StageResult<()> {
        let ls = self.slot(i)?;
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        let t = self.ty(i);
        if !owned && self.counted(t).is_some() && !self.loans.contains(&i) {
            self.retain_locals(&ls, t)?;
        }
        Ok(())
    }

    /// Stores a value that arrives with its own count as instruction `i`'s.
    pub(super) fn store_owned(&mut self, i: u32) -> StageResult<()> {
        self.store_as(i, true)
    }

    /// Whether operand `r` moves its count into the instruction being
    /// emitted, which consumes it ([`Em::take_from`]): then no count is
    /// taken for the consumer, and `r`'s locals are zeroed once the
    /// instruction has read them ([`Em::settle`]). A thunk moves nothing.
    pub(super) fn take(&mut self, r: u32) -> bool {
        if !self.owned {
            return false;
        }
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
        for l in self.vals.get(&r).cloned().unwrap_or_default() {
            self.f.zero(l);
        }
    }

    /// Releases value `x`'s count, when it holds a counted value, and
    /// zeroes its locals.
    pub(super) fn release_value(&mut self, x: u32) -> StageResult<()> {
        // A loan of a slot that took its local (`Em::slot_takes`): the
        // local's release, after the reference's last use.
        if let Some(&l) = self.slot_takes.get(&x) {
            return self.release_local(l);
        }
        let Some(ls) = self.vals.get(&x).cloned() else {
            return Ok(());
        };
        let t = self.ty(x);
        if self.counted(t).is_none() {
            return Ok(());
        }
        if self.view_loans.contains_key(&x) {
            self.release_view_loan(x, &ls)?;
        } else if !self.loans.contains(&x) {
            self.release_locals(&ls, t)?;
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
        if !self.is_cell(l) && self.counted(self.sub(self.b.local_ty[l])).is_none() {
            return Ok(());
        }
        if !self.loan_locals.contains_key(&l) {
            self.count_local(l, &ls, false)?;
        }
        for x in ls {
            self.f.zero(x);
        }
        Ok(())
    }

    /// Whether the function holds a counted reference anywhere: a
    /// parameter, a local or an instruction's value. Only then does it
    /// count, and its returns go through its exit.
    pub(super) fn counts_anything(&self) -> bool {
        if self.input.runtime.is_none() {
            return false;
        }
        let b = self.b;
        !self.cells.is_empty()
            || (0..b.local_ty.len()).any(|l| self.counted(self.sub(b.local_ty[l])).is_some())
            || (0..b.ty.len()).any(|i| {
                self.counted(self.ty(u32::try_from(i).expect("insts")))
                    .is_some()
            })
    }

    /// Releases what the function owns at its one exit: its sub-body's
    /// exit set, or in a thunk every counted instruction value and TIR
    /// local, a capture's too.
    ///
    /// For development tools, each place's release comes from where the
    /// place began (`exit_origins`), so a statement's excerpt shows
    /// the releases at the function's exit of what it bound; the rest of
    /// the exit comes from the function's root.
    pub(super) fn release_all(&mut self) -> StageResult<()> {
        let outer = self.f.origin_at;
        self.f.origin_at = self.f.origin_root;
        let result = self.release_all_inner();
        self.f.origin_at = outer;
        result
    }

    /// The origin of each TIR local's exit release, when origins are
    /// tracked: the first `Store` into the whole local with a source
    /// position that binds it. A parameter has none, and its release
    /// stays at the root.
    fn exit_origins(&self) -> hd_base::HashMap<usize, u32> {
        let mut out = hd_base::HashMap::default();
        if !self.m.track_origins {
            return out;
        }
        for i in 0..self.b.len() {
            let x = u32::try_from(i).expect("insts");
            if let Some((l, true)) = self.b.stored_local(i)
                && self.anchored(x)
            {
                out.entry(l.idx()).or_insert(x);
            }
        }
        out
    }

    fn release_all_inner(&mut self) -> StageResult<()> {
        let root = self.f.origin_root;
        if self.owned {
            // The exit set: the locals that may still hold a value here.
            let b = self.b;
            for &(s, l) in &b.exit_drops {
                if s.raw() != u32::from(self.sub_id) || self.borrowed.contains(&l.idx()) {
                    continue;
                }
                if let Some(ls) = self.locals.get(&l.idx()).cloned() {
                    self.count_local(l.idx(), &ls, false)?;
                }
            }
            for l in self.unowned_params.clone() {
                if let Some(ls) = self.locals.get(&l).cloned() {
                    self.count_local(l, &ls, false)?;
                }
            }
            for tmp in self.temps.clone() {
                self.f.get(tmp);
                self.f.helper(Helper::ReleaseRef);
            }
            return Ok(());
        }
        let mut vals: Vec<(u32, Vec<u32>)> =
            self.vals.iter().map(|(k, v)| (*k, v.clone())).collect();
        vals.sort_stable();
        for (i, ls) in vals {
            self.f.origin_at = if self.anchored(i) { Some(i) } else { root };
            if self.view_loans.contains_key(&i) {
                self.release_view_loan(i, &ls)?;
            } else if !self.loans.contains(&i) {
                self.release_locals(&ls, self.ty(i))?;
            }
        }
        let mut locals: Vec<(usize, Vec<u32>)> =
            self.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
        locals.sort_stable();
        let bound = self.exit_origins();
        for (l, ls) in locals {
            self.f.origin_at = bound.get(&l).copied().or(root);
            if !self.borrowed.contains(&l) && !self.loan_locals.contains_key(&l) {
                self.count_local(l, &ls, false)?;
            }
        }
        self.f.origin_at = root;
        for tmp in self.temps.clone() {
            self.f.get(tmp);
            self.f.helper(Helper::ReleaseRef);
        }
        Ok(())
    }
}

impl Em<'_, '_, '_> {
    /// The consumer of each value of an owned body, by the passes' rule
    /// (`hd_tir::own::consumers`).
    pub(super) fn own_consumers(&mut self) {
        hd_tir::own::consumers(self.b, &self.topo, self.input.own_cx, &mut self.consumer);
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
    /// say. The reads join [`Em::loans`].
    pub(super) fn instance_loans(&mut self, plocals: &[usize]) -> hd_base::HashSet<usize> {
        use hd_tir::ir::place;
        let b = self.b;
        let mut out = hd_base::HashSet::default();
        for (k, &l) in plocals.iter().enumerate() {
            let t = self.sub(b.local_ty[l]);
            if !matches!(
                self.pass_at(self.lends, k, t),
                crate::layout::Pass::Borrowed
            ) || self.is_cell(l)
                || b.cap_local.iter().any(|c| c.idx() == l)
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
                    if tag != Tag::Read && b.local_in_place(w).is_some_and(|x| x.idx() == l) {
                        fits = false;
                    }
                    continue;
                }
                if (w & !place::BARE_LOCAL) as usize != l {
                    continue;
                }
                if tag != Tag::Read {
                    fits = false;
                    continue;
                }
                let x = u32::try_from(i).expect("insts");
                let user = self.consumer.get(i).copied().unwrap_or(NONE);
                if user == NONE || self.borrowed_arg(user, x) {
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

    /// Whether call `c` passes value `r` as an argument its callee borrows
    /// at this instance (`layout::Pass::Borrowed`).
    fn borrowed_arg(&self, c: u32, r: u32) -> bool {
        if self.b.tags.get(c as usize) != Some(&Tag::Call) {
            return false;
        }
        let Some(hd_mono::Target::Call(ct)) = self.calls.get(&c) else {
            return false;
        };
        if !matches!(ct.kind, hd_mono::TargetKind::Instance) {
            return false;
        }
        let mut args = self.rec(self.b.data[c as usize][1]);
        args.truncate(args.len().saturating_sub(3));
        let Some(params) = self.param_tys(ct.key, ct.item, ct.args) else {
            return false;
        };
        if params.len() != args.len() || args.iter().filter(|&&a| a == r).count() != 1 {
            return false;
        }
        let k = args.iter().position(|&a| a == r).expect("argument");
        let lends = self.env().lends(ct.item);
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
            || self.counted(self.ty_of(r)).is_none()
            || self.moving.contains(&(user, r))
        {
            return false;
        }
        self.moving.push((user, r));
        true
    }

    /// After owned instruction `i`: each value it consumes whose count its
    /// lowering did not move in (it took a count of its own, or lent the
    /// value to a callee that borrows it) is released and zeroed. Not after
    /// an instruction that never completes (a `Return`, a `Break`, a call
    /// that panics), where no code runs: a jump's value moved out with it
    /// ([`Em::take`] then zeroed it, so a release would only be dead code).
    pub(super) fn release_untaken(&mut self, i: u32) -> StageResult<()> {
        if self.ty(i) == Ty::NEVER {
            return Ok(());
        }
        let mut ops = std::mem::take(&mut self.own_ops);
        hd_tir::own::operands(self.b, i as usize, &mut ops);
        let mut result = Ok(());
        for op in &ops {
            if matches!(
                op.role,
                hd_tir::own::Role::Dropped | hd_tir::own::Role::Tail
            ) {
                continue;
            }
            let v = op.value;
            if self.consumer.get(v as usize) == Some(&i) && !self.moving.contains(&(i, v)) {
                result = self.release_value(v);
                if result.is_err() {
                    break;
                }
            }
        }
        self.own_ops = ops;
        result
    }

    /// `Drop [$l]` or `Drop [%v]` `i`: releases the place's value and
    /// zeroes it, so a later drop of it releases nothing.
    pub(super) fn own_drop(&mut self, i: u32) -> StageResult<()> {
        let word = self.b.data[i as usize][0];
        if word & hd_tir::ir::place::BARE_LOCAL != 0 {
            let l = (word & !hd_tir::ir::place::BARE_LOCAL) as usize;
            return self.release_local(l);
        }
        match self.b.decode_place(word) {
            Some(hd_tir::ir::Place {
                root: hd_tir::ir::PlaceRoot::Value(r),
                proj,
            }) if proj.is_empty() && r.as_inst().is_some() => {
                if self.counting && self.own_drop_rest(i, r.0)? {
                    return Ok(());
                }
                self.release_value(r.0)
            }
            // The rest of a base the move pass moved a part out of
            // (`docs/compiler/tir-v2.md`, "Move" rule 3).
            Some(p) => self.own_drop_part(&p),
            None => crate::unsupported("a malformed place"),
        }
    }

    /// `IsUnique` `i`: the list's or map's buffer made the place's own
    /// (`rt:list_unique`, `rt:map_unique`), and stored there.
    pub(super) fn own_unique(&mut self, i: u32) -> StageResult<()> {
        use hd_tir::ir::unique_word::{COVERED, HOISTED};
        match self.b.data[i as usize][1] {
            // It runs at the loop that follows (`Em::hoisted_uniques`).
            HOISTED => {
                self.hoisted.push(i);
                return Ok(());
            }
            // The hoisted one before the loop covers it unless a reference
            // view's getter may make its buffer fresh.
            COVERED => {
                // The shape the link folds where no view is reachable.
                self.f.helper(Helper::RefViewsAbsent);
                self.f.op(crate::op::Op::If(wasm_encoder::BlockType::Empty));
                self.f.op(crate::op::Op::Else);
                self.unique_at(i)?;
                self.f.op(crate::op::Op::End);
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
        // A map's type has its key and value types; a list's, one.
        if let Ok((_, _, e)) = self.entry(t) {
            self.f.helper(Helper::MapUnique { entry: e });
        } else {
            let (_, e) = self.elem(t)?;
            self.f.helper(Helper::ListUnique { elem: e });
        }
        let Some(at) = at else {
            self.f.set(h);
            return self.put_buffer(word, h).map(|()| h);
        };
        // A place in memory takes a buffer the check replaced.
        let nb = self.f.local(crate::module::Val::I32);
        self.f.op(crate::op::Op::LocalTee(nb));
        self.f.get(h);
        self.f.op(crate::op::Op::I32Ne);
        self.f.op(crate::op::Op::If(wasm_encoder::BlockType::Empty));
        self.put_at(&at, nb)?;
        self.f.op(crate::op::Op::End);
        self.f.get(nb);
        self.f.set(h);
        Ok(h)
    }

    /// The `copy` or `borrow` read of `IsUnique` `i`'s place, through a
    /// step, that stands right after it in its block, outside an erased
    /// body's thunk: the change's operand, which takes the buffer the
    /// check left ([`Em::read_walked`]).
    fn read_after(&self, i: u32) -> Option<u32> {
        use hd_tir::ir::ReadMode::{Borrow, Copy};
        let blk = *self.topo.blk.get(i as usize).filter(|&&blk| blk != NONE)?;
        let pos = self.topo.pos[i as usize];
        let r = *self
            .rec(self.b.data[blk as usize][0])
            .get(pos as usize + 1)?;
        let [a, mode] = self.b.data[r as usize];
        let read = self.b.tags[r as usize] == Tag::Read
            && [Copy as u32, Borrow as u32].contains(&mode)
            && !matches!(self.calls.get(&r), Some(hd_mono::Target::Open(_)));
        let place = self
            .b
            .decode_place(a)
            .filter(|p| read && !p.proj.is_empty())?;
        (Some(place) == self.b.decode_place(self.b.data[i as usize][0])).then_some(r)
    }

    /// `Read` `i` of the place whose buffer the `IsUnique` right before it
    /// left in local `h`: one walk for both. A view's loan holds nothing.
    pub(super) fn read_walked(&mut self, i: u32, h: u32) -> StageResult<()> {
        self.begin_view_loan(i)?;
        self.f.get(h);
        self.store(i)
    }

    /// `Pin` `i`: the list's place becomes its buffer's one holder while
    /// element references hold counts of the buffer, each its own
    /// (§23.20).
    pub(super) fn own_pin(&mut self, i: u32) -> StageResult<()> {
        let (h, t, _) = self.place_header(self.b.data[i as usize][0])?;
        if self.input.runtime.is_none() {
            return Ok(());
        }
        self.pin_owner(h, t)
    }

    /// `Read [$l]` `i` of an owned body, as its mode says: a copy takes a
    /// count of its own; a move takes the local's and empties it; a
    /// borrow takes none and is no place's to release.
    pub(super) fn own_local_read(&mut self, i: u32, l: usize) -> StageResult<()> {
        use hd_tir::ir::ReadMode;
        let mode = ReadMode::from_u32(self.b.data[i as usize][1]);
        if self.is_cell(l) {
            return match mode {
                Some(ReadMode::Copy) => self.cell_get(i, l),
                Some(ReadMode::Borrow) => {
                    self.loans.insert(i);
                    self.cell_get(i, l)
                }
                // The move pass moves a celled local's count by its
                // reference (`docs/compiler/tir-v2.md`, "Move" rule 4).
                _ => crate::unsupported("a `move` read of a local in a cell"),
            };
        }
        let ls = self.local(u32::try_from(l).expect("locals"))?;
        for x in &ls {
            self.f.get(*x);
        }
        match mode {
            Some(ReadMode::Copy) => self.store(i),
            Some(ReadMode::Move) => {
                self.store_owned(i)?;
                if self.counted(self.ty(i)).is_some() {
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
            _ => crate::unsupported("a read the ownership passes left undecided"),
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
        for v in vals {
            self.release_value(v)?;
        }
        Ok(())
    }
}
