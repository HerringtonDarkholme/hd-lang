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
//!
//! An owned body (`docs/compiler/tir-v2.md`, "Stage 2"; [`Em::owned`])
//! went through the ownership passes, which decided every count: its
//! `Read`s copy, move or borrow as their mode says, its `Drop`s release
//! (and zero) a value or a local, a value moves into the one use that
//! consumes it ([`Em::take_from`]), and its exit releases its exit set.
//! The glue stays the same; only where it comes from changes. What the
//! backend still adds is glue for a lowering or an instance the generic
//! body cannot see: a consumer that cannot move a count in takes one of its
//! own and the value is released after it ([`Em::release_untaken`]), an
//! argument a callee instance owns but the passes lent takes a count
//! (`push_args`), a parameter the caller lends takes one on entry when the
//! body owns it (its exit set lists it), one the passes left unowned that
//! an instance receives owned is released at the exit, and a cancelled
//! suspension point releases the values the passes' unwind set lists for
//! it, before the exit releases the exit set ([`Em::release_unwound`]). A
//! `borrow` read takes no count, and a local a borrow is stored into holds
//! none ([`Em::prepare_own_loans`]).

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
                Tag::Read if let Some(l) = em.b.local_read(i as usize) => {
                    let l = l.idx();
                    if em.is_cell(l) || !matches!(em.kind(em.sub(em.b.local_ty[l])), Kind::Value) {
                        return false;
                    }
                    em.moves
                        .loan_locals
                        .get(&l)
                        .is_none_or(|v| valid(em, *v, depth + 1))
                }
                Tag::Read if let Some((base, _)) = em.projection(i as usize) => {
                    matches!(em.kind(em.ty_of(base)), Kind::Value) && valid(em, base, depth + 1)
                }
                Tag::Read if let Some((list, _)) = em.b.element_read(i as usize) => {
                    em.moves.loans.contains(&i) && valid(em, list, depth + 1)
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
                        Some(IntrinsicOp::MapSlotEntry) => {
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
        // Span loans (`live::span_loans`): a cell can change in any call,
        // an open value's thunk may make a fresh box, and a borrowed
        // binding's own lifetime ends at its last access, not the span's.
        let span: Vec<u32> = self
            .moves
            .span
            .iter()
            .copied()
            .filter(|&i| {
                let l = self.b.local_read(i as usize).expect("a local read").idx();
                !self.is_cell(l)
                    && !self.moves.loan_locals.contains_key(&l)
                    && !hd_mono::is_open(self.pool(), self.ty(i))
                    && !matches!(self.calls.get(&i), Some(hd_mono::Target::Open(_)))
            })
            .collect();
        self.moves.loans.extend(span);
    }

    pub(super) fn prepare_parameter_loans(&mut self) {
        // The checker introduces single-assignment reference temporaries
        // for method receivers. They borrow the same caller-owned pointer.
        let mut roots = self.borrowed.clone();
        let mut sets: hd_base::HashMap<usize, Vec<u32>> = hd_base::HashMap::default();
        for i in 0..self.b.len() {
            if let Some((l, v)) = super::live::local_store(self.b, i) {
                sets.entry(l).or_default().push(v);
            } else if let Some((l, _)) = super::live::local_part_store(self.b, i) {
                // A field store binds no source of its own.
                sets.entry(l).or_default().push(NONE);
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
                    || !self
                        .b
                        .local_read(*source as usize)
                        .is_some_and(|r| roots.contains(&r.idx()))
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
                // A list header read through a place or a field; a plain
                // local or global read, a list element, a tuple element and
                // a payload are no such read.
                if *tag != Tag::Read
                    || self.b.local_read(i).is_some()
                    || self.b.global_read(i).is_some()
                    || self.b.element_read(i).is_some()
                    || self
                        .projection(i)
                        .is_some_and(|(_, p)| p != super::live::Projected::Field)
                {
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
        // The roots that hold references, whose referents a place may
        // dereference; a data value's fields are no such place.
        let refs = roots
            .iter()
            .copied()
            .filter(|&l| matches!(self.kind(self.sub(self.b.local_ty[l])), Kind::Ref { .. }))
            .collect();
        let mut reads: Vec<u32> =
            super::live::parameter_loans(self.b, &roots, &refs, &headers, &self.projs)
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
            if let Some((super::live::Through::Value(source), _)) = self.read_through(i as usize)
                && !self.moves.loans.contains(&source)
            {
                continue;
            }
            // A field of a borrowed read borrows only where the read loads
            // that one field ([`Em::projected_field`]): a view's branch
            // then owns the field ([`Em::own_view_field`]), and the
            // physical branch reads it in place.
            if let Some((base, super::live::Projected::Field)) = self.projection(i as usize)
                && (!self.moves.loans.contains(&base)
                    || !matches!(self.projected_field(base), Ok(Some((f, _))) if f == i))
            {
                continue;
            }
            if matches!(self.b.tags[i as usize], Tag::Ref)
                && self.ref_target(self.ty(i)).is_some_and(|(_, fat)| fat)
            {
                continue;
            }
            self.moves.loans.insert(i);
            if self.b.local_read(i as usize).is_none() {
                let flag = self.f.local(crate::module::Val::I32);
                self.view_loans.insert(i, flag);
            }
        }
        // Match spans have a separate, typed plan: `Moves::span` still
        // contains only plain local reads. Validate physical projection
        // before treating a field or its pattern bindings as loans. A
        // field is a field read that a borrowed whole read loads alone, or a
        // `Read [*%r .f]` through a borrowed reference value, which loads
        // only its field.
        let fields: Vec<u32> = (0..self.b.len())
            .filter_map(|i| {
                let x = u32::try_from(i).expect("insts");
                let projected = match self.b.tags[i] {
                    Tag::Read
                        if let Some((base, super::live::Projected::Field)) = self.projection(i) =>
                    {
                        Ref(base).as_inst().is_some()
                            && matches!(self.read_through(base as usize), Some((_, true)))
                            && self.moves.loans.contains(&base)
                            && matches!(self.projected_field(base), Ok(Some((f, _))) if f == x)
                    }
                    Tag::Read => match self.read_through(i) {
                        Some((super::live::Through::Value(source), false)) => {
                            self.moves.loans.contains(&source)
                                && !hd_mono::is_open(self.pool(), self.ty_of(source))
                                && self
                                    .ref_target(self.ty_of(source))
                                    .is_some_and(|(_, fat)| !fat)
                                && !self.calls.contains_key(&x)
                        }
                        _ => false,
                    },
                    _ => false,
                };
                (projected
                    && !hd_mono::is_open(self.pool(), self.ty(x))
                    && coll_leaves(self.pool(), self.env(), self.ty(x)).is_empty())
                .then_some(x)
            })
            .collect();
        for span in super::live::field_spans(self.b, &fields, &roots, &self.projs) {
            let field = span.field;
            if span
                .reads
                .iter()
                .any(|i| hd_mono::is_open(self.pool(), self.ty(*i)) || self.calls.contains_key(i))
                || span.locals.keys().any(|l| {
                    self.is_cell(*l) || hd_mono::is_open(self.pool(), self.sub(self.b.local_ty[*l]))
                })
            {
                continue;
            }
            self.moves.loans.extend(span.reads);
            self.moves.loan_locals.extend(span.locals);
            self.view_loans
                .entry(field)
                .or_insert_with(|| self.f.local(crate::module::Val::I32));
        }
    }

    /// An owned body's `borrow` reads and binding locals
    /// (`docs/compiler/tir-v2.md`, "Borrow"): a borrow takes no count, so
    /// it joins [`Moves::loans`](super::live::Moves), and its read lowers
    /// as a copy's would with no count. One that an erased body's thunk
    /// makes is that thunk's fresh value, with its count, which the
    /// read's `Drop` (the pass keeps it) releases. One through a reference
    /// takes no count on a physical place; a view's getter makes a fresh
    /// value there, which a flag ([`Em::view_loans`]) marks for the read's
    /// `Drop`. A local the passes store a borrow into holds no count
    /// ([`Moves::loan_locals`](super::live::Moves)): its stores count
    /// nothing and release nothing.
    pub(super) fn prepare_own_loans(&mut self) {
        use hd_tir::ir::{ReadMode, place};
        for i in 0..self.b.len() {
            let [a, w] = self.b.data[i];
            let x = u32::try_from(i).expect("insts");
            let borrow = |r: u32| {
                Ref(r).as_inst().is_some()
                    && (r as usize) < self.b.len()
                    && self.b.tags[r as usize] == Tag::Read
                    && self.b.data[r as usize][1] == ReadMode::Borrow as u32
            };
            match self.b.tags[i] {
                Tag::Read if borrow(x) => {
                    if matches!(self.calls.get(&x), Some(hd_mono::Target::Open(_))) {
                        continue;
                    }
                    self.moves.loans.insert(x);
                    if a & place::BARE_LOCAL != 0 {
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
                Tag::Store if borrow(w) => {
                    let l = if a & place::BARE_LOCAL != 0 {
                        Some(a & !place::BARE_LOCAL)
                    } else {
                        match self.b.extra.get(a as usize..a as usize + 3) {
                            Some(&[2, place::ROOT_LOCAL, l]) => Some(l),
                            _ => None,
                        }
                    };
                    if let Some(l) = l {
                        self.moves.loan_locals.insert(l as usize, w);
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
        // An owned body's value is consumed or dropped, and zeroed, before
        // its instruction runs again.
        let old: Vec<u32> = if self.loops > 0 && !self.owned {
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
        if self.owned {
            let Some(&u) = self.emitting.last() else {
                return false;
            };
            return self.take_from(r, u);
        }
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

    /// Whether `Read [$l]` `i` of local `l` is the local's last access and
    /// takes its count (`live::Moves::last_access`): the local's release
    /// after that statement goes, and the caller zeroes its locals. A
    /// borrowed parameter has no count to give, and a cell is shared.
    pub(super) fn take_local(&mut self, i: u32, l: usize) -> bool {
        if self.owned
            || !self.counting
            || self.borrowed.contains(&l)
            || self.is_cell(l)
            || self.moves.loans.contains(&i)
            || self.moves.loan_locals.contains_key(&l)
        {
            return false;
        }
        let Some(&(of, at)) = self.moves.last_access.get(&i) else {
            return false;
        };
        if of != l || self.counted(self.sub(self.b.local_ty[l])).is_none() {
            return false;
        }
        self.skip.insert((at, super::live::Drop::Local(l)));
        true
    }

    /// Whether `Ref` `i`, the address of cell local `l`'s cell with no
    /// field path, is the local's last access and takes the cell's count
    /// (`live::Moves::last_access`, `crates/hd_wasm_linear/DESIGN.md`
    /// §23.20): the reference stores the address as owned, the caller
    /// zeroes the local, and the local's release after that statement goes.
    ///
    /// The local owns one count of its cell, from the declaration that made
    /// it or from the parameter moved into it, and the reference to the
    /// cell's payload is counted through the same object. At the last
    /// access nothing in the body reaches the local again until its
    /// declaration runs again, which makes a new cell (a loop around the
    /// reference and not the declaration makes the loop statement the
    /// last access, not the reference), and the planned release is the
    /// only other point that drops the local's count. So the count moves
    /// as a plain local read's does at its last access ([`Em::take_local`]).
    ///
    /// Kept out, each taking a count of its own as before:
    /// - a cell a closure shares (a capture in this body, or this closure
    ///   body's shared capture): the cell's lifetime is the closures' as
    ///   much as the local's;
    /// - a suspending body, whose frame saves the cell's local at each
    ///   point and whose resume enters blocks in the middle;
    /// - a parameter the caller lends, and a reference that borrows
    ///   (`Moves::loans`), which hold no count to move.
    pub(super) fn take_cell(&mut self, i: u32, l: usize) -> bool {
        if self.owned
            || !self.counting
            || self.susp.is_some()
            || !self.is_cell(l)
            || self.borrowed.contains(&l)
            || self.moves.loans.contains(&i)
            || self.moves.loan_locals.contains_key(&l)
            || self.shares_cell(l)
        {
            return false;
        }
        let Some(&(of, at)) = self.moves.last_access.get(&i) else {
            return false;
        };
        if of != l {
            return false;
        }
        self.skip.insert((at, super::live::Drop::Local(l)));
        true
    }

    /// Whether a closure shares local `l`'s cell: a closure of this body
    /// captures it shared, or `l` is this closure body's or thunk's shared
    /// capture, which the prologue received.
    fn shares_cell(&self, l: usize) -> bool {
        let param =
            self.b.local_flags.get(l).copied().unwrap_or(0) & hd_tir::ir::local_flags::PARAM != 0;
        (self.entry_cells.contains(&l) && !param)
            || (0..self.b.len()).any(|ci| {
                self.b.tags[ci] == Tag::Closure
                    && super::closure::captures(self.b, ci)
                        .iter()
                        .any(|&(c, shared)| shared && c == l)
            })
    }

    /// Whether block tail `r`, used only as its block's value
    /// (`live::Moves::tails`), moves its count into that value; the
    /// caller zeroes it once read ([`Em::settle_value`]).
    pub(super) fn take_tail(&self, r: u32) -> bool {
        !self.owned
            && self.counting
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
        if !self.counting || self.owned {
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
    pub(super) fn release_drops(&mut self, ds: &[super::live::Drop]) -> StageResult<()> {
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
        // An owned body's jumps have their `Drop`s before them.
        if !self.counting || self.owned {
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
            if let Some((l, _)) = super::live::local_store(self.b, i)
                && self.anchored(x)
            {
                out.entry(l).or_insert(x);
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
                if s.raw() != u32::from(self.sub_id) {
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

impl Em<'_, '_, '_> {
    /// The consumer of each value of an owned body, by the passes' rule
    /// (`hd_tir::own::consumers`).
    pub(super) fn own_consumers(&mut self) -> StageResult<()> {
        let Some(cx) = self.input.own_cx else {
            return crate::unsupported("an owned body without the passes' facts");
        };
        let mut out = std::mem::take(&mut self.consumer);
        hd_tir::own::consumers(self.b, cx, &mut out);
        self.consumer = out;
        Ok(())
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

    /// Releases value `r`'s count, and zeroes it.
    pub(super) fn release_value(&mut self, r: u32) -> StageResult<()> {
        self.release_drops(&[super::live::Drop::Val(r)])
    }

    /// `Drop [$l]` or `Drop [%v]` `i`: releases the place's value and
    /// zeroes it, so a later drop of it releases nothing.
    pub(super) fn own_drop(&mut self, i: u32) -> StageResult<()> {
        let word = self.b.data[i as usize][0];
        if word & hd_tir::ir::place::BARE_LOCAL != 0 {
            let l = (word & !hd_tir::ir::place::BARE_LOCAL) as usize;
            return self.release_drops(&[super::live::Drop::Local(l)]);
        }
        match self.b.decode_place(word) {
            Some(hd_tir::ir::Place {
                root: hd_tir::ir::PlaceRoot::Value(r),
                proj,
            }) if proj.is_empty() && r.as_inst().is_some() => {
                self.release_drops(&[super::live::Drop::Val(r.0)])
            }
            // The rest of a base the move pass moved a part out of
            // (`docs/compiler/tir-v2.md`, "Move" rule 3).
            Some(p) => self.own_drop_part(&p),
            None => crate::unsupported("a malformed place"),
        }
    }

    /// `IsUnique` `i`: the list's or map's buffer made the place's own
    /// (`rt:list_unique`, `rt:map_unique`).
    pub(super) fn own_unique(&mut self, i: u32) -> StageResult<()> {
        let (h, t) = self.place_header(self.b.data[i as usize][0])?;
        if self.input.runtime.is_none() {
            return Ok(());
        }
        // A map's type has its key and value types; a list's, one.
        if let Ok((_, _, e)) = self.entry(t) {
            self.f.get(h);
            self.f.helper(Helper::MapUnique { entry: e });
            return Ok(());
        }
        let (_, e) = self.elem(t)?;
        self.f.get(h);
        self.f.helper(Helper::ListUnique {
            elem: e,
            push: false,
        });
        Ok(())
    }

    /// `Pin` `i`: the list's header becomes its buffer's one owner while
    /// element references hold counts of the buffer: `ListPin` without
    /// the count of its own, which the reference holds (§23.20).
    pub(super) fn own_pin(&mut self, i: u32) -> StageResult<()> {
        let (h, t) = self.place_header(self.b.data[i as usize][0])?;
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
                    self.moves.loans.insert(i);
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
                self.moves.loans.insert(i);
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
        vals.sort_unstable();
        vals.dedup();
        for v in vals {
            self.release_drops(&[super::live::Drop::Val(v)])?;
        }
        Ok(())
    }
}
