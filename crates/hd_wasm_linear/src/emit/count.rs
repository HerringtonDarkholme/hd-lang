//! Counting (`crates/hd_wasm_linear/DESIGN.md` §23.16, job 8): every place that
//! holds a counted reference owns one count. An instruction's value owns
//! one from its store, a TIR local from its set, a field from its object's
//! construction or its write; a call's result arrives with its own. A
//! function releases every place it owns at its one exit. A store in a
//! loop releases the value the place held from the last iteration.
//!
//! Elision 1 (§23.25, job 17): a parameter is taken as `layout::Pass`
//! says, so a value parameter arrives with its count and a `&T` one is
//! never counted; and a count moves instead of being copied where a value
//! is consumed at its only use, or a local read at its last access
//! ([`Em::take`], [`Em::take_local`], [`Em::take_tail`]). A moved place is
//! zeroed, so the exit, which still releases every place, releases it as
//! nothing.

use hd_base::StageResult;
use hd_tir::ir::{IntrinsicOp, NONE, Ref, Tag};
use hd_types::Ty;

use super::Em;
use crate::layout::{Kind, Walk, coll_leaves, walk};
use crate::rt::Helper;

impl Em<'_, '_, '_> {
    /// Keep a liveness loan only when every step reads the same immutable
    /// representation. A copied collection header or a mutable data/ref
    /// projection needs its own ownership instead.
    pub(super) fn prepare_loans(&mut self) {
        fn valid(em: &Em<'_, '_, '_>, i: u32, depth: usize) -> bool {
            if depth > 32 || i == NONE || Ref(i).as_inst().is_none() {
                return false;
            }
            // An erased instruction's thunk may construct an owned box
            // even for a logical read. Its representation is not a loan.
            if hd_mono::is_open(em.pool(), em.ty(i))
                || matches!(em.calls.get(&i), Some(hd_mono::Target::Open(_)))
            {
                return false;
            }
            let [a, w] = em.b.data[i as usize];
            match em.b.tags[i as usize] {
                Tag::LocalGet => {
                    let l = a as usize;
                    if em.is_cell(l) || !matches!(em.kind(em.sub(em.b.local_ty[l])), Kind::Value) {
                        return false;
                    }
                    em.moves
                        .loan_locals
                        .get(&l)
                        .is_none_or(|v| valid(em, *v, depth + 1))
                }
                Tag::Field | Tag::TupleGet | Tag::Payload | Tag::Unwrap => {
                    matches!(em.kind(em.ty_of(a)), Kind::Value) && valid(em, a, depth + 1)
                }
                Tag::Intrinsic => {
                    let Some(&base) = em.b.record(w).first() else {
                        return false;
                    };
                    match IntrinsicOp::from_u32(a) {
                        Some(IntrinsicOp::ValueCopy) => {
                            coll_leaves(em.pool(), em.env(), em.ty(i)).is_empty()
                                && valid(em, base, depth + 1)
                        }
                        Some(IntrinsicOp::ListIndex | IntrinsicOp::MapSlotEntry) => {
                            em.moves.loans.contains(&i) && valid(em, base, depth + 1)
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        let reads: Vec<u32> = self
            .moves
            .loans
            .iter()
            .copied()
            .filter(|i| valid(self, *i, 0))
            .collect();
        let locals: Vec<(usize, u32)> = self
            .moves
            .loan_locals
            .iter()
            .filter(|(_, v)| valid(self, **v, 0))
            .map(|(l, v)| (*l, *v))
            .collect();
        self.moves.loans = reads.into_iter().collect();
        self.moves.loan_locals = locals.into_iter().collect();
    }

    pub(super) fn prepare_parameter_loans(&mut self) {
        // The checker introduces single-assignment reference temporaries
        // for method receivers. They borrow the same caller-owned pointer.
        let mut roots = self.borrowed.clone();
        let mut sets: hd_base::HashMap<usize, Vec<u32>> = hd_base::HashMap::default();
        for (i, tag) in self.b.tags.iter().enumerate() {
            if *tag == Tag::LocalSet {
                sets.entry(self.b.data[i][0] as usize)
                    .or_default()
                    .push(self.b.data[i][1]);
            }
        }
        loop {
            let mut changed = false;
            let mut bindings: Vec<_> = sets.iter().collect();
            bindings.sort_by_key(|(l, _)| **l);
            for (&l, values) in bindings {
                let [source] = values.as_slice() else {
                    continue;
                };
                if roots.contains(&l)
                    || self.is_cell(l)
                    || self.b.local_flags[l]
                        & (hd_tir::ir::local_flags::MUTATED
                            | hd_tir::ir::local_flags::CAPTURED_ASSIGNED)
                        != 0
                    || *source == NONE
                    || Ref(*source).as_inst().is_none()
                    || self.b.tags[*source as usize] != Tag::LocalGet
                    || !roots.contains(&(self.b.data[*source as usize][0] as usize))
                    || !matches!(self.kind(self.sub(self.b.local_ty[l])), Kind::Ref { .. })
                {
                    continue;
                }
                roots.insert(l);
                self.moves.loan_locals.insert(l, *source);
                changed = true;
            }
            if !changed {
                break;
            }
        }
        let headers = self
            .b
            .tags
            .iter()
            .enumerate()
            .filter_map(|(i, tag)| {
                if *tag != Tag::RefRead {
                    return None;
                }
                let i = u32::try_from(i).expect("insts");
                match self.pool().get(self.strip_mut(self.ty(i))) {
                    hd_types::TyData::Adt { def, .. }
                        if self.env().std_kind(def) == hd_mono::layout::StdKind::List =>
                    {
                        Some(i)
                    }
                    _ => None,
                }
            })
            .collect();
        let mut reads: Vec<u32> = super::live::parameter_loans(self.b, &roots, &headers)
            .into_iter()
            .collect();
        reads.sort_unstable();
        for i in reads {
            // Erased thunks can construct a fresh owned representation.
            if hd_mono::is_open(self.pool(), self.ty(i))
                || matches!(self.calls.get(&i), Some(hd_mono::Target::Open(_)))
            {
                continue;
            }
            if self.b.tags[i as usize] == Tag::RefRead
                && !self.moves.loans.contains(&self.b.data[i as usize][0])
            {
                continue;
            }
            if self.b.tags[i as usize] == Tag::RefPlace
                && self.ref_target(self.ty(i)).is_some_and(|(_, fat)| fat)
            {
                continue;
            }
            self.moves.loans.insert(i);
            if self.b.tags[i as usize] != Tag::LocalGet {
                let flag = self.f.local(crate::module::Val::I32);
                self.view_loans.insert(i, flag);
            }
        }
    }

    /// Release the owned fallback of a view-backed loan, leaving a physical
    /// loan uncounted. The flag is cleared so loop and exit drops agree.
    fn release_view_loan(&mut self, i: u32, ls: &[u32]) -> StageResult<()> {
        let flag = self.view_loans[&i];
        self.f.get(flag);
        self.f.op(wasm_encoder::Instruction::If(
            wasm_encoder::BlockType::Empty,
        ));
        self.release_locals(ls, self.ty(i))?;
        self.f.op(wasm_encoder::Instruction::End);
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

    /// Calls `on` for the value in locals `ls` of type `t`: one count more
    /// (`retain`) or less on each of its counted references.
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
        let Some(w) = self.counted(t) else {
            return Ok(());
        };
        if let Walk::Ref(k) = w {
            self.f.get(ls[k as usize]);
            if crate::layout::block_start(self.pool(), self.env(), t) {
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
            (_, None) => self.f.op(wasm_encoder::Instruction::Drop),
        }
    }

    /// One count more or less on TIR local `l`'s value in `ls`: its
    /// cell's, when it lives in one.
    pub(super) fn count_local(&mut self, l: usize, ls: &[u32], retain: bool) -> StageResult<()> {
        if self.is_cell(l) {
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

    /// One more count on value `r`'s references, for a place that keeps a
    /// copy of it. A constant is static data, which has no count.
    pub(super) fn retain_ref(&mut self, r: u32) -> StageResult<()> {
        if r == NONE || Ref(r).as_inst().is_none() || self.counted(self.ty_of(r)).is_none() {
            return Ok(());
        }
        let Some(ls) = self.vals.get(&r).cloned() else {
            return crate::unsupported("a value its instruction did not compute");
        };
        self.retain_locals(&ls, self.ty_of(r))
    }

    /// Stores the values on the stack as instruction `i`'s. Unless `owned`
    /// (a call's result, a new object), the value is a copy and its place
    /// takes a count of its own. In a loop, the value the place held from
    /// the last iteration is released after the new one is counted.
    pub(super) fn store_as(&mut self, i: u32, owned: bool) -> StageResult<()> {
        let ls = self.slot(i)?;
        let t = self.ty(i);
        if self.counted(t).is_none() || self.moves.loans.contains(&i) {
            for l in ls.iter().rev() {
                self.f.set(*l);
            }
            return Ok(());
        }
        let old: Vec<u32> = if self.loops > 0 {
            let vs = self.vals(t)?;
            let old: Vec<u32> = vs.iter().map(|v| self.f.local(*v)).collect();
            for (l, o) in ls.iter().zip(&old) {
                self.f.get(*l);
                self.f.set(*o);
            }
            old
        } else {
            Vec::new()
        };
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        if !owned {
            self.retain_locals(&ls, t)?;
        }
        if !old.is_empty() {
            self.release_locals(&old, t)?;
        }
        Ok(())
    }

    /// Stores a value that arrives with its own count as instruction `i`'s.
    pub(super) fn store_owned(&mut self, i: u32) -> StageResult<()> {
        self.store_as(i, true)
    }

    /// Whether operand `r` moves its count into the instruction being
    /// emitted, which is its only use, a later statement of its own block
    /// (`live::Moves::sole`): then no count is taken for the consumer, the
    /// release planned after that statement goes, and `r`'s locals are
    /// zeroed once the instruction has read them ([`Em::settle`]).
    pub(super) fn take(&mut self, r: u32) -> bool {
        if !self.counting
            || r == NONE
            || Ref(r).as_inst().is_none()
            || self.moves.loans.contains(&r)
        {
            return false;
        }
        let Some(&(u, at)) = self.moves.sole.get(&r) else {
            return false;
        };
        if self.emitting.last() != Some(&u)
            || !self.vals.contains_key(&r)
            || self.counted(self.ty_of(r)).is_none()
            || self.moving.contains(&(u, r))
        {
            return false;
        }
        self.skip.insert((at, super::live::Drop::Val(r)));
        self.moving.push((u, r));
        true
    }

    /// Whether `LocalGet` `i` of local `l` is the local's last access and
    /// takes its count (`live::Moves::last_get`): the local's release
    /// after that statement goes, and the caller zeroes its locals. A
    /// borrowed parameter has no count to give, and a cell is shared.
    pub(super) fn take_local(&mut self, i: u32, l: usize) -> bool {
        if !self.counting
            || self.borrowed.contains(&l)
            || self.is_cell(l)
            || self.moves.loans.contains(&i)
            || self.moves.loan_locals.contains_key(&l)
        {
            return false;
        }
        let Some(&(of, at)) = self.moves.last_get.get(&i) else {
            return false;
        };
        if of != l || self.counted(self.sub(self.b.local_ty[l])).is_none() {
            return false;
        }
        self.skip.insert((at, super::live::Drop::Local(l)));
        true
    }

    /// Whether block tail `r`, used only as its block's value
    /// (`live::Moves::tails`), moves its count into that value; the
    /// caller zeroes it once read ([`Em::settle_value`]).
    pub(super) fn take_tail(&self, r: u32) -> bool {
        self.counting
            && !self.moves.loans.contains(&r)
            && self.moves.tails.contains(&r)
            && self.vals.contains_key(&r)
            && self.counted(self.ty_of(r)).is_some()
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

    /// Releases what dies after statement `p` of block `blk`
    /// ([`super::live::plan`]), and zeroes its places, which the exit
    /// then releases as nothing.
    pub(super) fn drop_dead(&mut self, blk: u32, p: usize) -> StageResult<()> {
        if !self.counting {
            return Ok(());
        }
        let Some(mut ds) = self.drops.get(&(blk, p)).cloned() else {
            return Ok(());
        };
        // A place whose count moved at this statement holds nothing.
        ds.retain(|d| !self.skip.contains(&((blk, p), *d)));
        let outer = self.f.origin_at;
        self.f.origin_at = self
            .rec(self.b.data[blk as usize][0])
            .get(p)
            .copied()
            .filter(|&i| self.anchored(i))
            .or(outer);
        let result = self.release_drops(&ds);
        self.f.origin_at = outer;
        result
    }

    /// Releases and zeroes each of `ds`' places that holds a counted value.
    fn release_drops(&mut self, ds: &[super::live::Drop]) -> StageResult<()> {
        for &d in ds {
            let ls = match d {
                super::live::Drop::Val(x) => {
                    let Some(ls) = self.vals.get(&x).cloned() else {
                        continue;
                    };
                    let t = self.ty(x);
                    if self.counted(t).is_none() {
                        continue;
                    }
                    if self.view_loans.contains_key(&x) {
                        self.release_view_loan(x, &ls)?;
                    } else if !self.moves.loans.contains(&x) {
                        self.release_locals(&ls, t)?;
                    }
                    ls
                }
                super::live::Drop::Local(l) => {
                    if self.borrowed.contains(&l) {
                        continue;
                    }
                    let Some(ls) = self.locals.get(&l).cloned() else {
                        continue;
                    };
                    if !self.is_cell(l) && self.counted(self.sub(self.b.local_ty[l])).is_none() {
                        continue;
                    }
                    if !self.moves.loan_locals.contains_key(&l) {
                        self.count_local(l, &ls, false)?;
                    }
                    ls
                }
            };
            for l in ls {
                self.f.zero(l);
            }
        }
        Ok(())
    }

    /// Releases, and zeroes, what the blocks entered after `ctl` reached
    /// length `floor` hold: a jump to `floor`'s block leaves them, so their
    /// values and the locals that live in them are dead. A place already
    /// released holds zeros, which release as nothing.
    pub(super) fn drop_exited(&mut self, floor: usize) -> StageResult<()> {
        if !self.counting {
            return Ok(());
        }
        let exited: Vec<u32> = self
            .blocks
            .iter()
            .filter(|(_, at)| *at > floor)
            .map(|(blk, _)| *blk)
            .collect();
        for blk in exited {
            let mut ds: Vec<super::live::Drop> = self
                .rec(self.b.data[blk as usize][0])
                .into_iter()
                .filter(|&x| self.b.tags[x as usize] != hd_tir::ir::Tag::Block)
                .map(super::live::Drop::Val)
                .collect();
            let mut ls: Vec<usize> = self
                .drops
                .iter()
                .filter(|(k, _)| k.0 == blk)
                .flat_map(|(_, v)| v.iter())
                .filter_map(|d| match d {
                    super::live::Drop::Local(l) => Some(*l),
                    super::live::Drop::Val(_) => None,
                })
                .collect();
            ls.sort_unstable();
            ds.extend(ls.into_iter().map(super::live::Drop::Local));
            let outer = self.f.origin_at;
            self.f.origin_at = Some(blk);
            let result = self.release_drops(&ds);
            self.f.origin_at = outer;
            result?;
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

    /// Releases every place the function owns: each counted instruction
    /// value and TIR local, a parameter's too.
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
    /// tracked: the first `LocalSet` with a source position that binds
    /// it. A parameter has none, and its release stays at the root.
    fn exit_origins(&self) -> hd_base::HashMap<usize, u32> {
        let mut out = hd_base::HashMap::default();
        if !self.m.track_origins {
            return out;
        }
        for (i, tag) in self.b.tags.iter().enumerate() {
            let i = u32::try_from(i).expect("insts");
            if *tag == Tag::LocalSet && self.anchored(i) {
                out.entry(self.b.data[i as usize][0] as usize).or_insert(i);
            }
        }
        out
    }

    fn release_all_inner(&mut self) -> StageResult<()> {
        let root = self.f.origin_root;
        let mut vals: Vec<(u32, Vec<u32>)> =
            self.vals.iter().map(|(k, v)| (*k, v.clone())).collect();
        vals.sort_unstable();
        for (i, ls) in vals {
            self.f.origin_at = if self.anchored(i) { Some(i) } else { root };
            if self.view_loans.contains_key(&i) {
                self.release_view_loan(i, &ls)?;
            } else if !self.moves.loans.contains(&i) {
                self.release_locals(&ls, self.ty(i))?;
            }
        }
        let mut locals: Vec<(usize, Vec<u32>)> =
            self.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
        locals.sort_unstable();
        let bound = self.exit_origins();
        for (l, ls) in locals {
            self.f.origin_at = bound.get(&l).copied().or(root);
            if !self.borrowed.contains(&l) && !self.moves.loan_locals.contains_key(&l) {
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
