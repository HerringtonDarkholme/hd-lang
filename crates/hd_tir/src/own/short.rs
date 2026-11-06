//! Short loans (`crates/hd_wasm_linear/DESIGN.md` §23.20): a reference into
//! a collection whose every holder ends before anything could write or
//! drop that collection lives on the collection's own count. It takes no
//! count of the buffer, and a shared one makes the buffer neither unique
//! nor pinned.
//!
//! An **origin** is a reference-returning `map_probe` intrinsic, a
//! `ptr_ref` intrinsic, or a shared `Ref` of an element of a
//! local's list, or of a read of all of one. Its **holders** are the values
//! and locals its reference flows into: a variant built around it, a
//! block's tail its owner takes, a `break` value's target, a coercion
//! that keeps its values, a local stored once from it, and a read of a
//! holder's part that is a reference again. Every use of a holder must
//! lend: a match on it, a read through its reference (whose value the
//! read copies, or borrows for no longer than the holder lives), a write
//! through a `get_or_insert` reference, or its `Drop`. Exactly one `Drop`
//! ends the loan, in the same loop as the origin.
//!
//! Between the origin and that `Drop`, every statement that runs leaves
//! the collection alone: no store but into a local that is not on the
//! collection's path, no drop or move of that path, no `&mut`, no
//! uniqueness check or pin, no call that may write (`OwnCx::may_write`),
//! no intrinsic but a read, no return and no jump out. Anything else keeps
//! the counted reference.
//!
//! A loan that a `match` on a holder only decides with ends with the
//! decision (the match-decision walk): then only the statements up to
//! the match and the decision must leave the collection alone, and every
//! holder `Drop` in the origin's loop, one per path, ends the loan.

//! Short-loan marks for an owned body, after instance and storage layout
//! facts are available. The emitter supplies those facts and consumes the
//! marks; this pass alone follows holders and checks intervening writes.

use crate::ir::{
    Body, Callee, IntrinsicOp, NONE, Proj, ReadMode, Root, Tag, Topology, local_flags, ref_word,
    unique_word,
};
use hd_base::HashSet;
use hd_types::Ty;

/// A resolved call's existing lowering kind, independent of loan eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    MapProbe,
    PointerRef,
    Open,
    Other,
}

/// Instance, signature and storage facts read by the late TIR pass.
/// These queries make no decision about whether a loan is short.
pub trait Context {
    fn call_kind(&self, instruction: u32) -> CallKind;
    fn value_type(&self, value: u32) -> Ty;
    fn local_type(&self, local: u32) -> Ty;
    fn mutable_ref(&self, ty: Ty) -> bool;
    fn ref_target(&self, ty: Ty) -> Option<(Ty, bool)>;
    fn is_map(&self, ty: Ty) -> bool;
    fn element(&self, ty: Ty) -> Option<Ty>;
    fn counted(&self, ty: Ty) -> bool;
    fn needs_drop(&self, ty: Ty) -> bool;
    fn indirect_local(&self, local: usize) -> bool;
    fn promoted(&self, value: u32) -> bool;
    fn loan_value(&self, value: u32) -> bool;
    fn may_write(&self, callee: &Callee) -> bool;
}

/// A proven short loan. Its holders use their collection's count until
/// its ends; the listed checks serve only its shared element reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub origin: u32,
    pub values: Vec<u32>,
    pub locals: Vec<usize>,
    pub ends: Vec<u32>,
    pub skips: Vec<u32>,
}

/// Decide every short loan, in origin order, from the owned TIR and the
/// already resolved layout facts. This changes no read mode or drop.
#[must_use]
pub fn mark(b: &Body, topo: &Topology, cx: &dyn Context) -> Vec<Mark> {
    Pass {
        b,
        topo,
        cx,
        loans: HashSet::default(),
    }
    .run()
}

struct Pass<'a> {
    b: &'a Body,
    topo: &'a Topology,
    cx: &'a dyn Context,
    loans: HashSet<u32>,
}

/// The uses of each value and each local, for [`mark`].
struct Uses {
    of_value: Vec<Vec<u32>>,
    of_local: Vec<Vec<u32>>,
    stores: Vec<u32>,
    captured: Vec<bool>,
}

/// What [`Pass::short_loan`] finds for one origin.
struct Found {
    values: Vec<u32>,
    locals: Vec<usize>,
    ends: Vec<u32>,
}

impl Pass<'_> {
    fn run(&mut self) -> Vec<Mark> {
        let origins: Vec<_> = (0..self.b.len())
            .filter_map(|i| self.loan_origin(u32::try_from(i).expect("insts")))
            .collect();
        if origins.is_empty() {
            return Vec::new();
        }
        let uses = self.loan_uses();
        let mut marks = Vec::new();
        for (origin, mutable, skips) in origins {
            let Some(found) = self.short_loan(&uses, origin, mutable) else {
                continue;
            };
            self.loans.extend(found.values.iter().copied());
            marks.push(Mark {
                origin,
                values: found.values,
                locals: found.locals,
                ends: found.ends,
                skips,
            });
        }
        marks
    }

    /// Whether this resolved instruction can start a short loan.
    fn loan_origin(&self, x: u32) -> Option<(u32, bool, Vec<u32>)> {
        let i = x as usize;
        match self.b.tags[i] {
            Tag::Call if self.cx.call_kind(x) == CallKind::MapProbe => {
                let args = self.b.record(self.b.data[i][1]);
                let map = *args.first()?;
                let shared = !self.cx.mutable_ref(self.cx.value_type(map));
                match args.len().saturating_sub(3) {
                    2 if shared => Some((x, false, Vec::new())),
                    3 => Some((x, true, Vec::new())),
                    _ => None,
                }
            }
            Tag::Call if self.cx.call_kind(x) == CallKind::PointerRef => {
                Some((x, false, Vec::new()))
            }
            Tag::Ref => Some((x, false, self.shared_elem_ref(x)?)),
            _ => None,
        }
    }

    /// For a shared `Ref` of an element of a local's list, `[$l [%i]]` or
    /// `[*$l [%i]]`: the `IsUnique`s and `Pin`s of the list that stand
    /// right before it in its block, which insertion put there for it.
    fn shared_elem_ref(&self, x: u32) -> Option<Vec<u32>> {
        let i = x as usize;
        let w = self.b.data[i][1];
        let marks = ref_word::MUT | ref_word::TAKES | ref_word::LENT | ref_word::FRAME;
        if w & marks != 0
            || self.cx.promoted(x)
            || self.cx.mutable_ref(self.cx.value_type(x))
            || self.cx.call_kind(x) == CallKind::Open
        {
            return None;
        }
        let p = self.b.view(self.b.data[i][0]);
        // A list that a read of all of its local gives, `[%r [%i]]` after
        // `%r = Read borrow [$l]`, is the local's.
        let (l, read) = match p.root {
            Root::Local(l) => (l, NONE),
            Root::Value(r) => (self.whole_local_read(r)?, r),
            _ => return None,
        };
        let steps: Vec<Proj> = p.steps(self.b).collect();
        let lt = self.cx.local_type(l);
        let list = match steps.as_slice() {
            [Proj::Elem(_)] => lt,
            [Proj::Deref, Proj::Elem(_)] => self.cx.ref_target(lt).filter(|t| !t.1)?.0,
            _ => return None,
        };
        if self.cx.indirect_local(l as usize)
            || self.cx.is_map(list)
            || self.cx.element(list).is_none()
        {
            return None;
        }
        let blk = self.topo.blk[i];
        if blk == NONE || self.b.tags[blk as usize] != Tag::Block {
            return None;
        }
        let members = self.b.record(self.b.data[blk as usize][0]);
        let mut skips = Vec::new();
        for &m in members[..self.topo.pos[i] as usize].iter().rev() {
            if m == read {
                continue;
            }
            let k = m as usize;
            let tag = self.b.tags[k];
            let ours = matches!(tag, Tag::IsUnique)
                && self.b.view(self.b.data[k][0]).root == Root::Local(l)
                && !(tag == Tag::IsUnique && self.b.data[k][1] == unique_word::HOISTED);
            if !ours {
                break;
            }
            skips.push(m);
        }
        (!skips.is_empty()).then_some(skips)
    }

    /// The local that value `r` reads all of without moving it, `[$l]`.
    fn whole_local_read(&self, r: u32) -> Option<u32> {
        let k = r as usize;
        if crate::ir::Ref(r).as_inst().is_none()
            || self.b.tags.get(k) != Some(&Tag::Read)
            || self.b.data[k][1] == ReadMode::Move as u32
        {
            return None;
        }
        let p = self.b.view(self.b.data[k][0]);
        match p.root {
            Root::Local(l) if p.whole() => Some(l),
            _ => None,
        }
    }

    /// Every instruction's uses of each value and of each local, the
    /// stores into each local, and which locals a closure captures.
    fn loan_uses(&self) -> Uses {
        let (n, locals) = (self.b.len(), self.b.local_ty.len());
        let mut u = Uses {
            of_value: vec![Vec::new(); n],
            of_local: vec![Vec::new(); locals],
            stores: vec![0; locals],
            captured: vec![false; locals],
        };
        let mut ops = Vec::new();
        for j in 0..n {
            let x = u32::try_from(j).expect("insts");
            crate::own::operands(self.b, j, &mut ops);
            for op in &ops {
                let uses = &mut u.of_value[op.value as usize];
                if uses.last() != Some(&x) {
                    uses.push(x);
                }
            }
            if matches!(
                self.b.tags[j],
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::LoanEnd | Tag::IsUnique
            ) && let Some(l) = self.b.view(self.b.data[j][0]).local()
            {
                u.of_local[l as usize].push(x);
                if self.b.tags[j] == Tag::Store {
                    u.stores[l as usize] += 1;
                }
            }
        }
        for l in &self.b.cap_local {
            if let Some(c) = u.captured.get_mut(l.idx()) {
                *c = true;
            }
        }
        u
    }

    /// The holders of origin `o` and the `Drop` that ends its loan, when
    /// the loan is short (see the module).
    fn short_loan(&self, u: &Uses, o: u32, mutable: bool) -> Option<Found> {
        let mut found = Found {
            values: Vec::new(),
            locals: Vec::new(),
            ends: Vec::new(),
        };
        let mut ends = Vec::new();
        let mut own = HashSet::default();
        let mut work = vec![o];
        let mut seen_locals = HashSet::default();
        while let Some(v) = work.pop() {
            if found.values.contains(&v) {
                continue;
            }
            found.values.push(v);
            // A borrow read has no count a call could take.
            let dropped = self.b.is_borrow(v)
                || u.of_value[v as usize]
                    .iter()
                    .any(|&j| matches!(self.b.tags[j as usize], Tag::Drop | Tag::LoanEnd));
            for &j in &u.of_value[v as usize] {
                let next = self.holder_use(v, j, mutable, dropped, &mut ends, &mut own)?;
                match next {
                    Next::Value(w) => work.push(w),
                    Next::Local(l) => {
                        if seen_locals.insert(l) {
                            found.locals.push(l);
                            self.local_holder(u, l, mutable, &mut work, &mut ends, &mut own)?;
                        }
                    }
                    Next::Lends => {}
                }
            }
        }
        let roots = self.loan_roots(o);
        if let [end] = ends[..]
            && self.topo.loop_of[o as usize] == self.topo.loop_of[end as usize]
            && self.quiet_span(o, end, &roots, &own)
        {
            found.ends = vec![end];
            return Some(found);
        }
        if self.decided_in_match(u, o, &found, &ends, &roots, &own) {
            found.ends = ends;
            return Some(found);
        }
        None
    }

    /// Whether every statement from origin `o` to instruction `end`
    /// leaves the collection alone, but for the holders' own uses.
    fn quiet_span(
        &self,
        o: u32,
        end: u32,
        roots: &(HashSet<u32>, HashSet<u32>),
        own: &HashSet<u32>,
    ) -> bool {
        let Some(statements) = span(self.b, self.topo, o, end) else {
            return false;
        };
        statements.into_iter().all(|m| {
            let first = self.topo.first[m as usize];
            (first..=m)
                .all(|j| j == o || own.contains(&j) || self.leaves_alone(j, roots, first, end))
        })
    }

    /// A loan that a `match` on a holder only decides with: every use of
    /// a holder but a `Drop` runs before the match or in its decision
    /// (the tag tests, guards and bindings, before any arm), and no
    /// value the decision makes, nor a borrow a binding holds, reaches an
    /// arm. The loan ends when the decision does, so only the statements
    /// up to the match and the decision must leave the collection alone;
    /// the holders' `Drop`s, after it or before an arm's `return`, count
    /// nothing. Each must be in the origin's loop.
    fn decided_in_match(
        &self,
        u: &Uses,
        o: u32,
        found: &Found,
        ends: &[u32],
        roots: &(HashSet<u32>, HashSet<u32>),
        own: &HashSet<u32>,
    ) -> bool {
        let lp = self.topo.loop_of[o as usize];
        if ends.is_empty() || ends.iter().any(|&e| self.topo.loop_of[e as usize] != lp) {
            return false;
        }
        let mut matches = found.values.iter().flat_map(|&v| {
            u.of_value[v as usize].iter().copied().filter(move |&j| {
                self.b.tags[j as usize] == Tag::Match && self.b.data[j as usize][0] == v
            })
        });
        let (Some(m), None) = (matches.next(), matches.next()) else {
            return false;
        };
        let Some(&decision) = self.b.record(self.b.data[m as usize][1]).first() else {
            return false;
        };
        if decision == NONE || self.b.tags[decision as usize] != Tag::Block {
            return false;
        }
        let lo = self.topo.first[decision as usize];
        let inside = |j: u32| (lo..=decision).contains(&j);
        // The statements before the match, without its own blocks, which
        // its block lists right before it.
        let Some(mut before) = span(self.b, self.topo, o, m) else {
            return false;
        };
        before.retain(|&s| self.topo.owner[s as usize] != m);
        let ahead = |j: u32| {
            before
                .iter()
                .any(|&s| (self.topo.first[s as usize]..=s).contains(&j))
        };
        let dropped = |j: u32| matches!(self.b.tags[j as usize], Tag::Drop | Tag::LoanEnd);
        // Every use of a holder but a `Drop` is the match, before it or
        // in its decision.
        let uses = found
            .values
            .iter()
            .flat_map(|&v| u.of_value[v as usize].iter())
            .chain(found.locals.iter().flat_map(|&l| u.of_local[l].iter()));
        for &j in uses {
            if !(j == m || dropped(j) || ahead(j) || inside(j)) {
                return false;
            }
        }
        // A borrow through a holder lends the collection for as long as
        // it lives: only one the decision makes is checked to end there.
        if own
            .iter()
            .any(|&j| self.b.is_borrow(j) && !found.values.contains(&j) && !inside(j))
        {
            return false;
        }
        // Nothing the decision makes reaches an arm: no value but in a
        // `Drop`, and no borrow through a binding.
        for j in lo..=decision {
            let k = j as usize;
            if u.of_value[k]
                .iter()
                .any(|&w| !(inside(w) || w == m || dropped(w)))
            {
                return false;
            }
            if self.b.tags[k] == Tag::Store
                && let w = self.b.data[k][1]
                && (self.b.loan_read(w).is_some()
                    || (crate::ir::Ref(w).as_inst().is_some()
                        && (self.loans.contains(&w) || self.cx.loan_value(w))))
            {
                return false;
            }
        }
        let quiet_before = before.iter().all(|&s| {
            let first = self.topo.first[s as usize];
            (first..=s).all(|j| j == o || own.contains(&j) || self.leaves_alone(j, roots, first, m))
        });
        quiet_before
            && (lo..=decision)
                .all(|j| own.contains(&j) || self.leaves_alone(j, roots, lo, decision))
    }

    /// How use `j` of holder `v` goes on: to the holder it makes, into a
    /// local that holds the loan, or no further; `None` when it may keep
    /// the reference. `dropped`: `v` itself has a `Drop`, or is a borrow
    /// read, so a call only lends it.
    fn holder_use(
        &self,
        v: u32,
        j: u32,
        mutable: bool,
        dropped: bool,
        ends: &mut Vec<u32>,
        own: &mut HashSet<u32>,
    ) -> Option<Next> {
        let k = j as usize;
        let p = self.b.view(self.b.data[k][0]);
        let next = match self.b.tags[k] {
            Tag::Drop | Tag::LoanEnd if p.root == Root::Value(v) && p.whole() => {
                ends.push(j);
                Next::Lends
            }
            Tag::Match | Tag::SwitchTag if self.b.data[k][0] == v => Next::Lends,
            Tag::NewVariant => {
                let parts = self.b.record(self.b.data[k][1]);
                let alone = parts
                    .iter()
                    .all(|&q| q == v || !self.cx.counted(self.cx.value_type(q)));
                if !alone {
                    return None;
                }
                Next::Value(j)
            }
            Tag::Coerce if self.b.same_value_coercion(k) => Next::Value(j),
            Tag::Block => {
                let owner = self.topo.owner[k];
                if owner == NONE || !takes_tail(self.b, owner, j) {
                    return None;
                }
                Next::Value(owner)
            }
            // A `break` to a block gives its value to the block's owner.
            Tag::Break if self.b.data[k][1] == v => {
                let t = *self.b.label_inst.get(self.b.data[k][0] as usize)?;
                if self.b.tags[t as usize] != Tag::Block {
                    return None;
                }
                let owner = self.topo.owner[t as usize];
                if owner == NONE || !takes_tail(self.b, owner, t) {
                    return None;
                }
                Next::Value(owner)
            }
            Tag::Store if self.b.data[k][1] == v && p.whole() => {
                let Root::Local(l) = p.root else {
                    return None;
                };
                Next::Local(l as usize)
            }
            Tag::Read | Tag::Store if p.root == Root::Value(v) => {
                return self.through_holder(j, mutable, own);
            }
            Tag::Call if dropped && !self.cx.counted(self.cx.value_type(j)) => Next::Lends,
            _ => return None,
        };
        if self.b.tags[k] != Tag::Call {
            own.insert(j);
        }
        Some(next)
    }

    /// A read or store `j` rooted at a holder: through its reference, a
    /// copy or a borrow of the referent (or, for `get_or_insert`, a write
    /// to it); of the holder itself, a part that is a holder too.
    fn through_holder(&self, j: u32, mutable: bool, own: &mut HashSet<u32>) -> Option<Next> {
        let k = j as usize;
        let p = self.b.view(self.b.data[k][0]);
        let deref = p.steps(self.b).any(|s| s == Proj::Deref);
        let next = match self.b.tags[k] {
            Tag::Read if deref && self.b.data[k][1] != ReadMode::Move as u32 => Next::Lends,
            Tag::Read if !deref && self.cx.counted(self.cx.value_type(j)) => Next::Value(j),
            Tag::Read if !deref => Next::Lends,
            Tag::Store if deref && mutable => Next::Lends,
            _ => return None,
        };
        own.insert(j);
        Some(next)
    }

    /// Local `l`, stored once from a holder: it must be no parameter, cell,
    /// frame slot or capture, nor in the exit set, and each use of it lend
    /// as a holder's would.
    fn local_holder(
        &self,
        u: &Uses,
        l: usize,
        mutable: bool,
        work: &mut Vec<u32>,
        ends: &mut Vec<u32>,
        own: &mut HashSet<u32>,
    ) -> Option<()> {
        let alone = u.stores[l] == 1
            && !u.captured[l]
            && self.b.local_flags[l] & local_flags::PARAM == 0
            && !self.cx.indirect_local(l)
            && !self.b.exit_drops.iter().any(|&(_, x)| x.idx() == l);
        if !alone {
            return None;
        }
        for &j in &u.of_local[l] {
            let k = j as usize;
            let p = self.b.view(self.b.data[k][0]);
            match self.b.tags[k] {
                Tag::Store if p.whole() => {}
                Tag::Drop | Tag::LoanEnd if p.whole() => ends.push(j),
                Tag::Read | Tag::Store => match self.through_holder(j, mutable, own)? {
                    Next::Value(w) => work.push(w),
                    Next::Local(_) | Next::Lends => {}
                },
                _ => return None,
            }
            own.insert(j);
        }
        Some(())
    }

    /// The locals and values the collection of origin `o` is reached
    /// through: from the map operand of a probe, or the list local of an
    /// element reference, back through coercions, reads, references and
    /// locals stored once.
    fn loan_roots(&self, o: u32) -> (HashSet<u32>, HashSet<u32>) {
        let mut roots = (HashSet::default(), HashSet::default());
        let k = o as usize;
        if self.b.tags[k] == Tag::Call {
            if let Some(&map) = self.b.record(self.b.data[k][1]).first() {
                self.root_value(map, &mut roots, 0);
            }
        } else {
            let p = self.b.view(self.b.data[k][0]);
            self.root_place(p.root, &mut roots, 0);
        }
        roots
    }

    fn root_value(&self, v: u32, roots: &mut (HashSet<u32>, HashSet<u32>), depth: u32) {
        if depth > 16 || crate::ir::Ref(v).as_inst().is_none() || !roots.1.insert(v) {
            return;
        }
        let k = v as usize;
        match self.b.tags[k] {
            Tag::Coerce => self.root_value(self.b.data[k][0], roots, depth + 1),
            Tag::Read | Tag::Ref => {
                let root = self.b.view(self.b.data[k][0]).root;
                self.root_place(root, roots, depth + 1);
            }
            _ => {}
        }
    }

    fn root_place(&self, root: Root, roots: &mut (HashSet<u32>, HashSet<u32>), depth: u32) {
        match root {
            Root::Value(v) => self.root_value(v, roots, depth),
            Root::Local(l) => {
                if !roots.0.insert(l) {
                    return;
                }
                // A local stored once (an inlined body's `self`) stands
                // for what was stored.
                let mut stored = (0..self.b.len()).filter(|&j| {
                    self.b.tags[j] == Tag::Store
                        && self.b.view(self.b.data[j][0]).local() == Some(l)
                });
                if let (Some(j), None) = (stored.next(), stored.next()) {
                    self.root_value(self.b.data[j][1], roots, depth + 1);
                }
            }
            _ => {}
        }
    }

    /// Whether instruction `j`, which runs during a loan of the collection
    /// reached through `roots` that ends at `end`, leaves the collection
    /// alone. A jump must land at or after `first`, the first instruction
    /// of the statement that holds it, and no later than `end`.
    fn leaves_alone(
        &self,
        j: u32,
        roots: &(HashSet<u32>, HashSet<u32>),
        first: u32,
        end: u32,
    ) -> bool {
        let k = j as usize;
        let on_path = |root: Root| match root {
            Root::Local(l) => roots.0.contains(&l),
            Root::Value(v) => roots.1.contains(&v),
            Root::Const(_) => false,
            _ => true,
        };
        let p = self.b.view(self.b.data[k][0]);
        match self.b.tags[k] {
            Tag::Prim
            | Tag::Is
            | Tag::Coerce
            | Tag::NewStruct
            | Tag::CopyStruct
            | Tag::NewVariant
            | Tag::NewTuple
            | Tag::NewList
            | Tag::NewMap
            | Tag::ItemRef
            | Tag::RefIdentity
            | Tag::Block
            | Tag::If
            | Tag::And
            | Tag::Or
            | Tag::Match
            | Tag::SwitchTag
            | Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::SwitchStr
            | Tag::ToArm
            | Tag::Unreachable => true,
            Tag::Read => self.b.data[k][1] != ReadMode::Move as u32 || !on_path(p.root),
            // A reference that takes its local's count empties the local.
            Tag::Ref => {
                let w = self.b.data[k][1];
                w & ref_word::MUT == 0 && (w & ref_word::TAKES == 0 || !on_path(p.root))
            }
            Tag::Store => {
                !on_path(p.root)
                    && matches!(p.root, Root::Local(_))
                    && p.steps(self.b)
                        .all(|s| matches!(s, Proj::Field(_) | Proj::Variant(_)))
            }
            Tag::LoanEnd => true,
            Tag::Drop => {
                !on_path(p.root) || self.drops_only_a_ref(p.root) || self.drops_nothing(p.root)
            }
            Tag::Scope => {
                let suites = self.b.data[k][1];
                suites == NONE || self.b.record(suites).is_empty()
            }
            Tag::Break | Tag::Continue => self
                .b
                .label_inst
                .get(self.b.data[k][0] as usize)
                .is_some_and(|&t| t >= first && t <= end),
            Tag::Call => Callee::from_words(self.b.record(self.b.data[k][0]))
                .is_some_and(|callee| !self.cx.may_write(&callee)),
            Tag::Intrinsic => matches!(
                IntrinsicOp::from_u32(self.b.data[k][0]),
                Some(
                    IntrinsicOp::ListLen
                        | IntrinsicOp::MapLen
                        | IntrinsicOp::StrIndex
                        | IntrinsicOp::StrConcat
                        | IntrinsicOp::StrEq
                )
            ),
            _ => false,
        }
    }

    /// Whether dropping value `root` releases nothing: a value with no
    /// counted leaf and no `std.rt.Drop`, as the `Ptr` a pointer
    /// reference's origin reads.
    fn drops_nothing(&self, root: Root) -> bool {
        matches!(root, Root::Value(v) if !self.cx.needs_drop(self.cx.value_type(v)))
    }

    /// Whether dropping `root` leaves its referent's local holding it: a
    /// `Ref` of a local that takes no count of the local's.
    fn drops_only_a_ref(&self, root: Root) -> bool {
        let Root::Value(mut v) = root else {
            return false;
        };
        for _ in 0..16 {
            let k = v as usize;
            match self.b.tags.get(k) {
                Some(Tag::Coerce) if self.b.same_value_coercion(k) => v = self.b.data[k][0],
                Some(Tag::Ref) => {
                    return self.b.data[k][1] & ref_word::TAKES == 0
                        && matches!(self.b.view(self.b.data[k][0]).root, Root::Local(_));
                }
                _ => return false,
            }
        }
        false
    }
}

/// Where a holder's use leads ([`Pass::holder_use`]).
enum Next {
    Value(u32),
    Local(usize),
    Lends,
}

/// Whether `owner` takes block `blk`'s tail as its value: an `If`'s or a
/// `Match`'s arm, or a cleanup scope's body.
fn takes_tail(b: &crate::ir::Body, owner: u32, blk: u32) -> bool {
    let [a, w] = b.data[owner as usize];
    match b.tags[owner as usize] {
        Tag::If => true,
        Tag::Match => b.record(w).first() != Some(&blk),
        Tag::Scope => a == blk,
        _ => false,
    }
}

/// The statements that run from origin `o` to `end`: those after `o` in
/// each block around it, up to the block that holds `end`, and there those
/// after the one around `o` up to `end`. `None` when `end` is in no block
/// around `o`.
fn span(b: &crate::ir::Body, topo: &Topology, o: u32, end: u32) -> Option<Vec<u32>> {
    let h = topo.blk[end as usize];
    let mut out = Vec::new();
    let mut x = o;
    for _ in 0..b.len() {
        if b.tags[x as usize] == Tag::Block {
            x = topo.owner[x as usize];
            if x == NONE {
                return None;
            }
            continue;
        }
        let p = topo.blk[x as usize];
        if p == NONE || b.tags[p as usize] != Tag::Block {
            return None;
        }
        let members = b.record(b.data[p as usize][0]);
        let from = topo.pos[x as usize] as usize + 1;
        if p == h {
            let to = topo.pos[end as usize] as usize;
            if to < from {
                return None;
            }
            out.extend_from_slice(&members[from..to]);
            return Some(out);
        }
        out.extend_from_slice(members.get(from..)?);
        x = p;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Place, PlaceRoot, Providers, Ref, TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::TyList;

    const REFERENCE: Ty = Ty(1_000_000);
    const LIST: Ty = Ty(1_000_001);

    struct Facts<'a>(&'a Body);

    impl Context for Facts<'_> {
        fn call_kind(&self, instruction: u32) -> CallKind {
            if self.0.tags[instruction as usize] == Tag::Call
                && matches!(Callee::from_words(self.0.record(self.0.data[instruction as usize][0])), Some(Callee::Item { def, .. }) if def == DefId::from_raw(7))
            {
                CallKind::PointerRef
            } else {
                CallKind::Other
            }
        }
        fn value_type(&self, value: u32) -> Ty {
            Ref(value).as_inst().map_or_else(
                || self.0.consts[(value & !Ref::CONST_BIT) as usize].0,
                |inst| self.0.ty[inst.0 as usize],
            )
        }
        fn local_type(&self, local: u32) -> Ty {
            self.0.local_ty[local as usize]
        }
        fn mutable_ref(&self, _: Ty) -> bool {
            false
        }
        fn ref_target(&self, _: Ty) -> Option<(Ty, bool)> {
            None
        }
        fn is_map(&self, _: Ty) -> bool {
            false
        }
        fn element(&self, ty: Ty) -> Option<Ty> {
            (ty == LIST).then_some(Ty::I32)
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == REFERENCE || ty == LIST
        }
        fn needs_drop(&self, ty: Ty) -> bool {
            self.counted(ty)
        }
        fn indirect_local(&self, _: usize) -> bool {
            false
        }
        fn promoted(&self, _: u32) -> bool {
            false
        }
        fn loan_value(&self, _: u32) -> bool {
            false
        }
        fn may_write(&self, callee: &Callee) -> bool {
            matches!(callee, Callee::Item { def, .. } if *def == DefId::from_raw(8))
        }
    }

    fn call(b: &mut TirBuilder, def: u32, args: &[Ref], ty: Ty) -> Ref {
        b.call(
            &Callee::Item {
                def: DefId::from_raw(def),
                targs: TyList::EMPTY,
            },
            args,
            Providers::None,
            ty,
            NodeIdx::from_raw(1),
        )
    }

    fn pointer_loan(intervenes: u32) -> (Body, u32, u32) {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        let local = b.local(Ty::I32, Symbol::from_raw(0), local_flags::PARAM, syn);
        let pointer = b.get(local, Ty::I32, syn);
        let origin = call(&mut b, 7, &[pointer], REFERENCE);
        match intervenes {
            1 => {
                call(&mut b, 8, &[], Ty::VOID);
            }
            2 => {
                call(&mut b, 9, &[origin], REFERENCE);
            }
            _ => {}
        }
        let place = b.place(&Place {
            root: PlaceRoot::Value(origin),
            proj: vec![],
        });
        let end = b.emit(Tag::LoanEnd, place, NONE, Ty::VOID, syn);
        let root = b.close_block(block, None, Ty::VOID, syn);
        (
            b.finish_tolerating(root, &[], &[22])
                .expect("owned TIR shape"),
            origin.0,
            end.0,
        )
    }

    #[test]
    fn a_pointer_reference_with_a_quiet_lifetime_gets_a_mark() {
        let (body, origin, end) = pointer_loan(0);
        let marks = mark(&body, &Topology::new(&body), &Facts(&body));
        assert_eq!(
            marks,
            vec![Mark {
                origin,
                values: vec![origin],
                locals: vec![],
                ends: vec![end],
                skips: vec![]
            }]
        );
    }

    #[test]
    fn a_possible_write_or_an_escaping_holder_prevents_a_mark() {
        for intervenes in [1, 2] {
            let (body, _, _) = pointer_loan(intervenes);
            assert!(mark(&body, &Topology::new(&body), &Facts(&body)).is_empty());
        }
    }

    #[test]
    fn a_shared_element_reference_marks_only_its_own_adjacent_check() {
        for hoisted in [false, true] {
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let syn = NodeIdx::from_raw(1);
            let block = b.open_block();
            let local = b.local(LIST, Symbol::from_raw(0), local_flags::PARAM, syn);
            let index = b.local(Ty::I32, Symbol::from_raw(1), local_flags::PARAM, syn);
            let index = b.get(index, Ty::I32, syn);
            let whole = b.place(&Place::local(local));
            let check = b.emit(
                Tag::IsUnique,
                whole,
                if hoisted { unique_word::HOISTED } else { 0 },
                Ty::VOID,
                syn,
            );
            let element = b.place(&Place {
                root: PlaceRoot::Local(local),
                proj: vec![Proj::Elem(index)],
            });
            let origin = b.emit(Tag::Ref, element, 0, REFERENCE, syn);
            let result = b.place(&Place {
                root: PlaceRoot::Value(origin),
                proj: vec![],
            });
            let end = b.emit(Tag::Drop, result, NONE, Ty::VOID, syn);
            let root = b.close_block(block, None, Ty::VOID, syn);
            let body = b
                .finish_tolerating(root, &[], &[22])
                .expect("owned TIR shape");
            let marks = mark(&body, &Topology::new(&body), &Facts(&body));
            if hoisted {
                assert!(marks.is_empty());
            } else {
                assert_eq!(
                    marks,
                    vec![Mark {
                        origin: origin.0,
                        values: vec![origin.0],
                        locals: vec![],
                        ends: vec![end.0],
                        skips: vec![check.0]
                    }]
                );
            }
        }
    }
}
