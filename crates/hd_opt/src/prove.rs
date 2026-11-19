//! Bounds-check elimination over owned TIR (step 6 of
//! `docs/future-work/BACKEND_NEUTRAL_TIR.md`, #2175). Each element step of
//! a place is checked until a proof here clears its check
//! ([`hd_tir::ir::place::ELEM_UNCHECKED`]); the backend writes the check
//! only where the bit is clear. Go's `prove.go` folds its explicit
//! `IsInBounds` values the same way, from induction variables it finds
//! per loop (`findIndVar`), as the rising proof here does.
//!
//! It runs after the ownership passes, whose hoisted `IsUnique` may
//! replace a buffer, so a proof sees the final reads and writes.
//!
//! The proofs:
//! - an element of a private local list that a call already received in
//!   the same straight-line run, at the same index
//!   ([`ElementChecks::repeats_bounds`]);
//! - per loop, a rising index from 0 below an unchanged list's length
//!   ([`Lists::rising`]), a falling index below an index an earlier
//!   access checked ([`Lists::falling`]), and, in release, a masked
//!   index `y & (xs.len() - 1)` behind a test that the length is above
//!   zero ([`Lists::masked`]).
//!
//! A list keeps its length where the hoisting pass's region row says so
//! (`Body::stable_lists`). Cost: one pass over the body for its stores,
//! the dominance and loop forest once per body with a loop
//! ([`Nest`]), and per loop the cheap tests, then one walk of its body
//! only once they pass.

use hd_base::SliceSort as _;
use hd_tir::ir::{
    Body, Callee, Coercion, IntrinsicOp, NONE, Nest, PlaceRoot, PrimOp, Proj, Ref, Step, Tag,
    Topology, local_flags,
};
use hd_tir::own::ElementChecks;
use hd_types::{Prim, Ty};

/// The type and call facts the proofs read, which the backend's layout
/// decides: the driver answers them from `hd_layout` and the call record.
pub trait Cx {
    /// Whether `t` is a `List`.
    fn list(&self, t: Ty) -> bool;
    /// Whether the ownership passes track a value of `t`.
    fn tracked(&self, t: Ty) -> bool;
    /// Whether `t` is a tuple.
    fn tuple(&self, t: Ty) -> bool;
    /// The type reference type `t` refers to, and whether it is fat.
    fn ref_target(&self, t: Ty) -> Option<(Ty, bool)>;
    /// The type of struct, tuple or newtype field `k` of `t`; `None`
    /// proves nothing behind a field.
    fn field(&self, _t: Ty, _k: u32) -> Option<Ty> {
        None
    }
    /// The intrinsic `callee` names, if it names one.
    fn intrinsic(&self, callee: &Callee) -> Option<String>;
}

/// Clears the bounds check of each element access `b` proves in range.
/// `release` enables the masked proof. A sub-body with an await point is
/// left alone: a resume enters its blocks in the middle, where the
/// dominance the proofs read does not hold. One with none runs straight
/// through, even as a suspending function's (an entry `main!` that never
/// awaits runs as a plain function).
pub fn run(b: &mut Body, cx: &dyn Cx, release: bool) {
    let checks = ElementChecks::new(b);
    for i in 0..b.len() {
        if checks.repeats_bounds(i) {
            b.uncheck_elem(i, 0);
        }
    }
    let mut proven = Vec::new();
    if b.tags.contains(&Tag::Loop) {
        let topo = Topology::new(b);
        let nest = Nest::new(b, &topo);
        let sets = Sets::new(b);
        scans(2 * b.len());
        let mut awaits = vec![false; b.sub_root.len()];
        for i in 0..b.len() {
            if matches!(
                b.tags[i],
                Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
            ) {
                awaits[topo.sub[i] as usize] = true;
            }
        }
        for l in (0..b.len()).filter(|&l| b.tags[l] == Tag::Loop) {
            let sub = topo.sub[l];
            if awaits[sub as usize] {
                continue;
            }
            let cell = |local: usize| {
                b.storage
                    .binary_search_by(|&(s, x, _)| (s.idx(), x.idx()).cmp(&(sub as usize, local)))
                    .is_ok()
            };
            let lists = Lists {
                b,
                topo: &topo,
                nest: &nest,
                cx,
                cell: &cell,
                sets: &sets,
            };
            proven.extend(lists.proofs(hd_tir::ir::u32_of(l), release));
        }
    }
    // Two proofs may clear one access; each record is copied once.
    proven.sort_stable();
    proven.dedup();
    for i in proven {
        // The first element step: `[x [%i] ..]`, `[$l * .xs [%i] ..]`.
        if let Some(k) = b
            .decode_place(b.data[i][0])
            .and_then(|p| p.proj.iter().position(|s| matches!(s, Proj::Elem(_))))
        {
            b.uncheck_elem(i, k);
        }
    }
}

/// A list place as a `Body::stable_lists` row names it: a local and a
/// path of fields and dereferences from it (`[$l]`, `[*$l]`,
/// `[*$l .xs]`, #2187).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRoot {
    pub local: u32,
    pub path: Vec<Proj>,
}

impl ListRoot {
    /// The list at `path` from `local`, which holds fields and
    /// dereferences only.
    fn at(local: u32, path: Vec<Proj>) -> Option<Self> {
        path.iter()
            .all(|s| matches!(s, Proj::Field(_) | Proj::Deref))
            .then_some(ListRoot { local, path })
    }

    /// A local's own list, or the list the reference it holds refers to.
    #[must_use]
    pub fn of_local(local: u32, indirect: bool) -> Self {
        let path = if indirect {
            vec![Proj::Deref]
        } else {
            Vec::new()
        };
        ListRoot { local, path }
    }

    /// Whether the list is behind the reference the local holds, for
    /// those two forms only.
    #[must_use]
    pub fn indirect(&self) -> Option<bool> {
        match self.path.as_slice() {
            [] => Some(false),
            [Proj::Deref] => Some(true),
            _ => None,
        }
    }
}

/// Each local's stores, the locals a `Ref` names, and the element
/// accesses by the local their index reads (built on first use). One pass
/// over the body. Cost: O(instructions).
pub struct Sets {
    /// Each local's stores, with the value of a store into all of it
    /// (`NONE` for a field's).
    pub sets: Vec<Vec<(u32, u32)>>,
    referenced: Vec<bool>,
    index_reads: std::sync::OnceLock<(Vec<u32>, Vec<u32>)>,
}

impl Sets {
    #[must_use]
    pub fn new(b: &Body) -> Self {
        let locals = b.local_ty.len();
        let mut sets = vec![Vec::new(); locals];
        let mut referenced = vec![false; locals];
        for i in 0..b.len() {
            if let Some((local, whole)) = b.stored_local(i) {
                let value = if whole { b.data[i][1] } else { NONE };
                sets[local.idx()].push((hd_tir::ir::u32_of(i), value));
            }
            if b.tags[i] == Tag::Ref
                && let Some(p) = b.decode_place(b.data[i][0])
                && let PlaceRoot::Local(l) = p.root
                && p.proj.iter().all(|s| matches!(s, Proj::Field(_)))
            {
                referenced[l.idx()] = true;
            }
        }
        Sets {
            sets,
            referenced,
            index_reads: std::sync::OnceLock::new(),
        }
    }
}

/// One body's shapes for the list proofs: its blocks, dominance and loop
/// forest, stores, which locals live in storage, and the type facts.
pub struct Lists<'a> {
    pub b: &'a Body,
    pub topo: &'a Topology,
    pub nest: &'a Nest,
    pub cx: &'a dyn Cx,
    /// Whether a local lives in a cell or slot, where a call may reach it.
    pub cell: &'a dyn Fn(usize) -> bool,
    pub sets: &'a Sets,
}

/// Each comparison's negation, on `usize` operands.
const NEGATION: [(PrimOp, PrimOp); 6] = [
    (PrimOp::Lt, PrimOp::Ge),
    (PrimOp::Le, PrimOp::Gt),
    (PrimOp::Gt, PrimOp::Le),
    (PrimOp::Ge, PrimOp::Lt),
    (PrimOp::Eq, PrimOp::Ne),
    (PrimOp::Ne, PrimOp::Eq),
];

fn usize_ty() -> Ty {
    Ty::prim(Prim::Usize)
}

/// The local that value `r` reads when it is a plain local read.
#[must_use]
pub fn read_local(b: &Body, r: u32) -> Option<usize> {
    let i = Ref(r).as_inst()?.0 as usize;
    (i < b.len())
        .then(|| b.local_read(i))
        .flatten()
        .map(hd_base::LocalId::idx)
}

/// The list and index of `Read` or `Store` `i` whose place's first
/// element step indexes a list a local roots through fields and
/// dereferences (`[$l [%i] ..]`, `[$l * [%i] ..]`, `[$l * .xs [%i] ..]`).
#[must_use]
pub fn rooted_elem(b: &Body, i: usize) -> Option<(ListRoot, u32)> {
    if !matches!(b.tags[i], Tag::Read | Tag::Store) {
        return None;
    }
    let mut p = b.decode_place(b.data[i][0])?;
    let PlaceRoot::Local(l) = p.root else {
        return None;
    };
    let (k, ix) = p.proj.iter().enumerate().find_map(|(k, s)| match *s {
        Proj::Elem(ix) => Some((k, ix)),
        _ => None,
    })?;
    p.proj.truncate(k);
    Some((ListRoot::at(l.raw(), p.proj)?, ix.0))
}

/// The list root value `r` reads: a local's list, or the list a
/// reference it holds or reads refers to.
#[must_use]
pub fn list_root(b: &Body, cx: &dyn Cx, r: u32) -> Option<ListRoot> {
    let i = Ref(r).as_inst()?.0 as usize;
    if !cx.list(b.ty[i]) {
        return None;
    }
    match b.tags[i] {
        Tag::Read if let Some(l) = b.local_read(i) => Some(ListRoot::of_local(l.raw(), false)),
        // A field, element or payload of a value, not a read through a
        // reference.
        Tag::Read
            if b.read_step(i)
                .is_some_and(|(_, s)| !matches!(s, Step::DerefField(_))) =>
        {
            None
        }
        // A list a local roots through fields and dereferences
        // (`[*$l]`, `[$s .xs]`, `[*$l .xs]`), or one a reference value
        // read from a local refers to (`[*%r .xs]`).
        Tag::Read => {
            let p = b.decode_place(b.data[i][0])?;
            let local = match p.root {
                PlaceRoot::Local(l) => l.raw(),
                PlaceRoot::Value(v) if p.proj.first() == Some(&Proj::Deref) => {
                    hd_tir::ir::u32_of(read_local(b, v.0)?)
                }
                _ => return None,
            };
            ListRoot::at(local, p.proj)
        }
        Tag::Coerce => list_root(b, cx, b.data[i][0]),
        _ => None,
    }
}

/// The list value whose element `Store` `i` writes (`[%xs [%i] ..]`).
fn elem_store_list(b: &Body, i: usize) -> Option<u32> {
    if b.tags[i] != Tag::Store {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match (p.root, p.proj.first()) {
        (PlaceRoot::Value(r), Some(Proj::Elem(_))) => Some(r.0),
        _ => None,
    }
}

/// The index of element access `x` in any form [`Lists::elem_access`]
/// reads, from the body alone.
fn access_index(b: &Body, x: usize) -> Option<u32> {
    if let Some((_, index)) = b.element_read(x) {
        return Some(index);
    }
    if elem_store_list(b, x).is_some() {
        return match b.decode_place(b.data[x][0])?.proj.first() {
            Some(&Proj::Elem(index)) => Some(index.0),
            _ => None,
        };
    }
    rooted_elem(b, x).map(|(_, index)| index)
}

impl Lists<'_> {
    /// The loop's proven element accesses: the rising and falling proofs
    /// for a canonical loop test, and in release the masked one.
    fn proofs(&self, l: u32, release: bool) -> Vec<usize> {
        let mut out = Vec::new();
        if let Some((test, false)) = self.loop_guard(l) {
            out.extend(self.rising(l, test));
            out.extend(self.falling(l, test));
        }
        if release {
            out.extend(self.masked(l));
        }
        out.into_iter().map(|x| x as usize).collect()
    }

    fn dominates(&self, d: u32, j: u32) -> bool {
        self.nest.dominates(self.topo, d, j)
    }

    fn in_loop(&self, i: u32, l: u32) -> bool {
        self.nest.in_loop(i, l)
    }

    fn loop_body(&self, l: u32) -> Vec<u32> {
        let out = self.nest.loop_body(l);
        scans(out.len());
        out
    }

    fn ty_of(&self, r: u32) -> Ty {
        if r == NONE {
            return Ty::VOID;
        }
        match Ref(r).as_inst() {
            Some(i) => self.b.ty[i.0 as usize],
            None => self.b.consts[(r & !Ref::CONST_BIT) as usize].0,
        }
    }

    /// The value of `usize` constant `r`.
    fn const_usize(&self, r: u32) -> Option<u64> {
        if r == NONE || Ref(r).as_inst().is_some() {
            return None;
        }
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        (ty == usize_ty()).then_some(bits & 0xffff_ffff)
    }

    fn is_op(&self, r: u32, tag: Tag, op: u32) -> bool {
        Ref(r).as_inst().is_some()
            && self.b.tags[r as usize] == tag
            && self.b.data[r as usize][0] == op
    }

    fn list_root(&self, r: u32) -> Option<ListRoot> {
        list_root(self.b, self.cx, r)
    }

    /// Whether read `i` projects a struct or newtype field of a value.
    fn field_read(&self, i: usize) -> bool {
        matches!(self.b.read_step(i), Some((base, Step::Field(_))) if !self.cx.tuple(self.ty_of(base)))
    }

    fn scalar_loop_test(&self, i: u32, budget: &mut usize) -> bool {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let b = self.b;
        let [a, w] = b.data[i as usize];
        if self.cx.tracked(b.ty[i as usize]) {
            // The list a test measures (`while i < xs.len()`, #2186).
            return b.tags[i as usize] == Tag::Read && self.list_root(i).is_some();
        }
        match b.tags[i as usize] {
            Tag::Read if let Some(l) = b.local_read(i as usize) => !(self.cell)(l.idx()),
            Tag::Read => self.field_read(i as usize),
            Tag::Prim | Tag::Coerce => true,
            Tag::Intrinsic => IntrinsicOp::from_u32(a) == Some(IntrinsicOp::ListLen),
            Tag::And | Tag::Or => self.scalar_loop_test(w, budget),
            Tag::Block => b
                .record(a)
                .iter()
                .filter(|&&x| b.tags[x as usize] != Tag::Block)
                .all(|&x| self.scalar_loop_test(x, budget)),
            _ => false,
        }
    }

    /// A canonical loop test, before its body: the `If` after a scalar
    /// prefix one of whose branches only breaks out of the loop, and
    /// whether that branch is the first.
    #[must_use]
    pub fn loop_guard(&self, l: u32) -> Option<(u32, bool)> {
        let (test, prefix, [first, second]) = hd_tir::own::loop_test(self.b, l)?;
        let mut budget = self.b.len();
        let scalar = hd_tir::own::loop_statements(self.b, l)
            .take(prefix)
            .all(|x| self.scalar_loop_test(x, &mut budget));
        (first != second && scalar).then_some((test, first))
    }

    /// Follow immutable scalar aliases and fields of a just-built struct.
    /// A changed or addressable binding never supplies a fact.
    fn source(&self, mut r: u32) -> u32 {
        let b = self.b;
        for _ in 0..32 {
            let Some(i) = Ref(r).as_inst() else { break };
            let i = i.0 as usize;
            let [a, w] = b.data[i];
            match b.tags[i] {
                Tag::Read
                    if let Some(l) = read_local(b, r)
                        && !(self.cell)(l)
                        && b.local_flags[l]
                            & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED)
                            == 0 =>
                {
                    let [(set, value)] = self.sets.sets[l].as_slice() else {
                        break;
                    };
                    if !self.dominates(*set, r) {
                        break;
                    }
                    r = *value;
                }
                Tag::Coerce if b.record(w).first() == Some(&(Coercion::Identity as u32)) => r = a,
                Tag::Read
                    if self.field_read(i)
                        && let Some((from, Step::Field(k))) = b.read_step(i) =>
                {
                    let Some(bi) = Ref(self.source(from)).as_inst() else {
                        break;
                    };
                    if b.tags[bi.0 as usize] != Tag::NewStruct {
                        break;
                    }
                    let Some(&value) = b.record(b.data[bi.0 as usize][1]).get(k as usize) else {
                        break;
                    };
                    r = value;
                }
                _ => break,
            }
        }
        r
    }

    /// The list and `usize` index of element access `x`: an element read
    /// or store of a list value (`[%xs [%i]]`), or of a local's list or
    /// the list its reference refers to ([`rooted_elem`]).
    fn elem_access(&self, x: usize) -> Option<(ListRoot, u32)> {
        let b = self.b;
        let (root, index) = if let Some((list, index)) = b.element_read(x) {
            (self.list_root(list)?, index)
        } else if let Some(list) = elem_store_list(b, x) {
            (self.list_root(list)?, access_index(b, x)?)
        } else {
            let (root, index) = rooted_elem(b, x)?;
            // A map's entry is an element step too.
            let mut ty = b.local_ty[root.local as usize];
            for step in &root.path {
                ty = match *step {
                    Proj::Field(k) => self.cx.field(ty, k)?,
                    _ => self.cx.ref_target(ty)?.0,
                };
            }
            if !self.cx.list(ty) {
                return None;
            }
            (root, index)
        };
        (self.ty_of(index) == usize_ty()).then_some((root, index))
    }

    /// Exact `usize` induction from zero to an unchanged list's length,
    /// `while i < n` or `while i < xs.len()`, with `i = i + 1` in the loop.
    fn rising(&self, l: u32, test: u32) -> Vec<u32> {
        let b = self.b;
        let cond = b.data[test as usize][0];
        // Where an `&&` test holds, so does its first operand.
        let mut lt = cond;
        while Ref(lt).as_inst().is_some() && b.tags[lt as usize] == Tag::And {
            lt = b.data[lt as usize][0];
        }
        if !self.is_op(lt, Tag::Prim, PrimOp::Lt as u32) {
            return Vec::new();
        }
        let [cur, end] = *b.record(b.data[lt as usize][1]) else {
            return Vec::new();
        };
        let Some(current) = read_local(b, cur) else {
            return Vec::new();
        };
        let bound = read_local(b, end);
        if self.ty_of(cur) != usize_ty()
            || self.ty_of(end) != usize_ty()
            || [Some(current), bound]
                .into_iter()
                .flatten()
                .any(|x| (self.cell)(x) || b.local_flags[x] & local_flags::CAPTURED_ASSIGNED != 0)
        {
            return Vec::new();
        }
        let Some([init, step]) = init_and_step(&self.sets.sets[current], |x| self.in_loop(x, l))
        else {
            return Vec::new();
        };
        if self.in_loop(init.0, l)
            || !self.in_loop(step.0, l)
            || !self.dominates(init.0, l)
            || self.const_usize(self.source(init.1)) != Some(0)
            || !self.is_op(step.1, Tag::Prim, PrimOp::Add as u32)
        {
            return Vec::new();
        }
        let [before, one] = *b.record(b.data[step.1 as usize][1]) else {
            return Vec::new();
        };
        if read_local(b, before) != Some(current) || self.const_usize(one) != Some(1) {
            return Vec::new();
        }
        // The bound: a local set once before the loop from a length read
        // (`for i in 0..n`), or a length the test reads on each iteration
        // (`while i < xs.len()`, #2186), which the loop keeps as the list's.
        let len = match bound {
            Some(bound) => {
                let [(bound_set, value)] = self.sets.sets[bound].as_slice() else {
                    return Vec::new();
                };
                if !self.dominates(*bound_set, l) {
                    return Vec::new();
                }
                self.source(*value)
            }
            None => self.source(end),
        };
        if !self.is_op(len, Tag::Intrinsic, IntrinsicOp::ListLen as u32)
            || !(self.dominates(len, l) || bound.is_none() && self.dominates(len, lt))
        {
            return Vec::new();
        }
        let Some(root) = b
            .record(b.data[len as usize][1])
            .first()
            .and_then(|&list| self.list_root(list))
        else {
            return Vec::new();
        };
        if !self.keeps_length(&root, l, len) {
            return Vec::new();
        }
        self.loop_body(l)
            .into_iter()
            .filter(|&x| {
                // A read or a store: the same place at the same index.
                let Some((list, index)) = self.elem_access(x as usize) else {
                    return false;
                };
                if list != root {
                    return false;
                }
                let index = self.source(index);
                read_local(b, index) == Some(current)
                    && self.dominates(cond, index)
                    && self.dominates(index, step.0)
                    && self.dominates(index, x)
            })
            .collect()
    }

    /// Whether `usize` local `l` changes only by its own stores.
    fn plain_index_local(&self, l: usize) -> bool {
        !(self.cell)(l)
            && self.b.local_flags[l] & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED) == 0
            && !self.sets.referenced[l]
    }

    /// Whether instruction `x` runs only once loop test `test`, an `If`
    /// whose second arm leaves the loop, went on: in its first arm or
    /// after it.
    #[must_use]
    pub fn after_test(&self, test: u32, x: u32) -> bool {
        let go_on = self
            .b
            .record(self.b.data[test as usize][1])
            .first()
            .copied();
        self.dominates(test, x) || go_on.is_some_and(|arm| self.nest.child_above(arm, x).is_some())
    }

    /// Falling indices: in loop `l`, whose `test` goes on while `v > 0`
    /// (or `v != 0`), a `usize` local `v` that the loop changes only by
    /// one `v = v - 1` after the test, and that is set once before the
    /// loop from a local `u` that a dominating access of list `xs` used as
    /// its index, unchanged since, never grows past that index. While
    /// `xs` keeps its length, from that access through the loop, `xs[v]`
    /// anywhere in the loop and `xs[v - 1]` read after the test and before
    /// the decrement are within bounds.
    fn falling(&self, l: u32, test: u32) -> Vec<u32> {
        let b = self.b;
        let cond = b.data[test as usize][0];
        if Ref(cond).as_inst().is_none()
            || b.tags[cond as usize] != Tag::Prim
            || ![PrimOp::Gt as u32, PrimOp::Ne as u32].contains(&b.data[cond as usize][0])
        {
            return Vec::new();
        }
        let [cur, zero] = *b.record(b.data[cond as usize][1]) else {
            return Vec::new();
        };
        let Some(v) = read_local(b, cur) else {
            return Vec::new();
        };
        if self.ty_of(cur) != usize_ty()
            || self.const_usize(zero) != Some(0)
            || !self.plain_index_local(v)
        {
            return Vec::new();
        }
        let Some([init, dec]) = init_and_step(&self.sets.sets[v], |x| self.in_loop(x, l)) else {
            return Vec::new();
        };
        // The decrement runs once per iteration at most, after the test.
        if self.in_loop(init.0, l)
            || !self.dominates(init.0, l)
            || self.topo.loop_of[dec.0 as usize] != l
            || !self.is_op(dec.1, Tag::Prim, PrimOp::Sub as u32)
        {
            return Vec::new();
        }
        let [before, one] = *b.record(b.data[dec.1 as usize][1]) else {
            return Vec::new();
        };
        if read_local(b, before) != Some(v)
            || self.const_usize(one) != Some(1)
            || self.topo.loop_of[before as usize] != l
            || !self.after_test(test, before)
        {
            return Vec::new();
        }
        // The value `v` enters with: local `u`, read where it is set.
        let Some(u) = read_local(b, init.1) else {
            return Vec::new();
        };
        if self.ty_of(init.1) != usize_ty() || !self.plain_index_local(u) {
            return Vec::new();
        }
        // The lists an access checked that value against: its index reads
        // `u`, which no store changes between that read and `v`'s.
        let (at, reads) = self.sets.index_reads.get_or_init(|| {
            let mut edges = Vec::new();
            for x in 0..b.len() {
                if let Some(local) = access_index(b, x).and_then(|r| read_local(b, r)) {
                    edges.push((hd_tir::ir::u32_of(local), hd_tir::ir::u32_of(x)));
                }
            }
            scans(b.len());
            csr(b.local_ty.len(), &edges)
        });
        let mut roots: Vec<ListRoot> = Vec::new();
        for &x in &reads[at[u] as usize..at[u + 1] as usize] {
            let Some((root, index)) = self.elem_access(x as usize) else {
                continue;
            };
            if roots.contains(&root)
                || read_local(b, index) != Some(u)
                || !self.dominates(x, init.1)
            {
                continue;
            }
            // Every statement of the index read's block through the one
            // that holds the store of `v`: a path leaving them reaches
            // that store again only through the read.
            let blk = self.topo.blk[index as usize];
            let Some(holder) = self.nest.child_above(blk, init.0) else {
                continue;
            };
            let from = self.topo.pos[index as usize] as usize;
            let to = self.topo.pos[holder as usize] as usize;
            let between = self.nest.statements(b, blk, from, to);
            scans(between.len());
            if between.iter().any(|&y| {
                b.stored_local(y as usize)
                    .is_some_and(|(s, _)| s.idx() == u)
            }) {
                continue;
            }
            if self.keeps_length(&root, l, x) {
                roots.push(root);
            }
        }
        if roots.is_empty() {
            return Vec::new();
        }
        self.loop_body(l)
            .into_iter()
            .filter(|&y| {
                let Some((root, index)) = self.elem_access(y as usize) else {
                    return false;
                };
                if !roots.contains(&root) {
                    return false;
                }
                // A read of `v` in the loop, which never exceeds its entry
                // value.
                let index = self.source(index);
                if read_local(b, index) == Some(v) {
                    return self.in_loop(index, l);
                }
                // `v - 1` where the test found `v` above 0.
                if !self.is_op(index, Tag::Prim, PrimOp::Sub as u32) {
                    return false;
                }
                let [r, one] = *b.record(b.data[index as usize][1]) else {
                    return false;
                };
                read_local(b, r) == Some(v)
                    && self.const_usize(one) == Some(1)
                    && self.after_test(test, r)
                    && self.dominates(r, dec.0)
            })
            .collect()
    }

    /// Masked indices (#1713): an access `xs[y & (n - 1)]` among loop
    /// `l`'s own instructions, the `&`'s operands in either order, where
    /// `n` is `xs.len()` and the `&` runs only once a test showed a length
    /// of `xs` above zero ([`Lists::positive_operand`]). Unsigned,
    /// `y & m <= m`, and with the length above zero `n - 1` does not wrap.
    /// `xs` keeps its length from each of the two length reads through
    /// the loop. Cost: the loop's own instructions, and a walk of the
    /// enclosing blocks of each masked access.
    fn masked(&self, l: u32) -> Vec<u32> {
        let own = self.nest.own(l);
        scans(own.len());
        if !own
            .iter()
            .any(|&x| self.is_op(x, Tag::Prim, PrimOp::BitAnd as u32))
        {
            return Vec::new();
        }
        own.iter()
            .copied()
            .filter(|&x| {
                let Some((root, index)) = self.elem_access(x as usize) else {
                    return false;
                };
                let Some((n, and)) = self.masked_len(&root, index) else {
                    return false;
                };
                let Some(t) = self.positive_len(&root, and) else {
                    return false;
                };
                let keeps = |d: u32| self.dominates(d, x) && self.keeps_length(&root, l, d);
                keeps(n) && keeps(t)
            })
            .collect()
    }

    /// The `xs.len()` read of list `root` that value `r` stands for.
    fn len_of(&self, root: &ListRoot, r: u32) -> Option<u32> {
        let n = self.source(r);
        (self.is_op(n, Tag::Intrinsic, IntrinsicOp::ListLen as u32)
            && self
                .b
                .record(self.b.data[n as usize][1])
                .first()
                .is_some_and(|&list| self.list_root(list).as_ref() == Some(root)))
        .then_some(n)
    }

    /// The length read `n` and the `&` of index `index` when it is
    /// `y & (n - 1)` in either order, `n` a length of list `root`.
    fn masked_len(&self, root: &ListRoot, index: u32) -> Option<(u32, u32)> {
        let and = self.source(index);
        if !self.is_op(and, Tag::Prim, PrimOp::BitAnd as u32) {
            return None;
        }
        let mask = |m: u32| self.len_of(root, self.minus_one_of(self.source(m))?);
        let [y, z] = *self.b.record(self.b.data[and as usize][1]) else {
            return None;
        };
        Some((mask(y).or_else(|| mask(z))?, and))
    }

    /// A length read of list `root` that a test showed above zero before
    /// `at` runs: a test whose arm holds `at`, or an earlier statement of
    /// a block around `at` whose other arm never falls through.
    fn positive_len(&self, root: &ListRoot, at: u32) -> Option<u32> {
        let b = self.b;
        let shown = |test: u32, outcome: bool| {
            let c = self.source(b.data[test as usize][0]);
            self.positive_operand(c, outcome)
                .and_then(|n| self.len_of(root, n))
        };
        for (blk, pos) in Nest::block_path(self.topo, b, at)? {
            let owner = self.topo.owner[blk as usize];
            if owner != NONE && b.tags[owner as usize] == Tag::If {
                let arms = b.record(b.data[owner as usize][1]);
                if let Some(n) = shown(owner, arms.first() == Some(&blk)) {
                    return Some(n);
                }
            }
            for &s in &b.record(b.data[blk as usize][0])[..pos] {
                if b.tags[s as usize] != Tag::If {
                    continue;
                }
                let arms = b.record(b.data[s as usize][1]);
                for (k, outcome) in [(1, true), (0, false)] {
                    if let Some(&other) = arms.get(k)
                        && other != NONE
                        && self.arm_exit(other, s).is_some()
                        && let Some(n) = shown(s, outcome)
                    {
                        return Some(n);
                    }
                }
            }
        }
        None
    }

    /// Whether list `root` keeps its length from `from`, which dominates
    /// loop `l` or runs in it, through the loop: the hoisting pass's row
    /// for the list at the loop (`Body::stable_lists`), whose region, when
    /// `from` runs before the loop, starts before `from` and ends after
    /// the statement of `from`'s block that holds the loop. A frame cell's
    /// list has no row. O(log rows + the loop's rows + the block depth).
    fn keeps_length(&self, root: &ListRoot, l: u32, from: u32) -> bool {
        let Some([first, after, _]) =
            stable_rows(self.b, l).find_map(|(r, w)| (r == *root).then_some(w))
        else {
            return false;
        };
        if self.in_loop(from, l) {
            return true;
        }
        self.nest
            .child_above(self.topo.blk[from as usize], l)
            .is_some_and(|holder| (first == NONE || first < from) && holder < after)
    }

    /// The operand that `Prim` condition `c` shows above zero when it
    /// comes out `outcome`, on `usize`: `a < n` puts `n` above `a`, so
    /// above zero, and `n != 0` and `n >= 1` do directly; a failed
    /// comparison is its negation, as is `!c`.
    #[must_use]
    pub fn positive_operand(&self, c: u32, outcome: bool) -> Option<u32> {
        let b = self.b;
        if Ref(c).as_inst().is_none() || b.tags[c as usize] != Tag::Prim {
            return None;
        }
        let op = PrimOp::from_u32(b.data[c as usize][0])?;
        let [x, y] = *b.record(b.data[c as usize][1]) else {
            // `!c` comes out the other way: of the one-operand operations
            // only `!` gives a `bool`.
            let &[inner] = b.record(b.data[c as usize][1]).first_chunk::<1>()?;
            return (op == PrimOp::Not).then(|| self.positive_operand(inner, !outcome))?;
        };
        if self.ty_of(x) != usize_ty() {
            return None;
        }
        // On `usize` operands every `bool` operation is a comparison.
        let op = if outcome {
            op
        } else {
            NEGATION.iter().find(|(o, _)| *o == op)?.1
        };
        let at_least_one = |r: u32| self.const_usize(r).is_some_and(|k| k >= 1);
        match op {
            PrimOp::Lt => Some(y),
            PrimOp::Gt => Some(x),
            PrimOp::Le if at_least_one(x) => Some(y),
            PrimOp::Ge if at_least_one(y) => Some(x),
            PrimOp::Ne if self.const_usize(y) == Some(0) => Some(x),
            PrimOp::Ne if self.const_usize(x) == Some(0) => Some(y),
            PrimOp::Eq if at_least_one(y) => Some(x),
            PrimOp::Eq if at_least_one(x) => Some(y),
            _ => None,
        }
    }

    /// How arm `arm` of `If` `x` ends when it never ends normally: its
    /// last statement returns, panics, traps, or breaks or continues to a
    /// label past `x`, and no break inside it ends at the arm or at `x`.
    /// `Some(true)` when nothing in it leaves for a label outside it, so
    /// the run ends there; `Some(false)` when it may go on past the block
    /// that holds `x`. `None` when it may fall through.
    #[must_use]
    pub fn arm_exit(&self, arm: u32, x: u32) -> Option<bool> {
        let b = self.b;
        let last = *(b.tags[arm as usize] == Tag::Block)
            .then(|| b.record(b.data[arm as usize][0]))?
            .iter()
            .rev()
            .find(|&&s| b.tags[s as usize] != Tag::Block)?;
        let first = self.topo.first[arm as usize];
        let target = |i: u32| b.label_inst[b.data[i as usize][0] as usize];
        let ends = match b.tags[last as usize] {
            Tag::Return | Tag::Unreachable => true,
            Tag::Break | Tag::Continue if target(last) != arm && target(last) != x => false,
            Tag::Call if self.calls_panic(last) => true,
            _ => return None,
        };
        let (mut inside, mut past) = (true, true);
        for i in first..arm {
            if matches!(b.tags[i as usize], Tag::Break | Tag::Continue) {
                let t = target(i);
                past &= t != arm && t != x;
                inside &= (first..arm).contains(&t);
            }
        }
        past.then_some(ends && inside)
    }

    /// The intrinsic call `s` makes, if it calls one.
    fn intrinsic(&self, s: u32) -> Option<String> {
        let b = self.b;
        (b.tags[s as usize] == Tag::Call)
            .then(|| Callee::from_words(b.record(b.data[s as usize][0])))
            .flatten()
            .and_then(|c| self.cx.intrinsic(&c))
    }

    /// Whether instruction `s` calls the panic intrinsic.
    #[must_use]
    pub fn calls_panic(&self, s: u32) -> bool {
        self.intrinsic(s).as_deref() == Some("panic")
    }

    /// The `x` of instruction `v` when it is `x - 1`, or
    /// `x.wrapping_sub(1)`, which is the same subtraction in a release
    /// build.
    #[must_use]
    pub fn minus_one_of(&self, v: u32) -> Option<u32> {
        let b = self.b;
        let i = Ref(v).as_inst()?.0 as usize;
        let [r, one] = match b.tags[i] {
            Tag::Prim if b.data[i][0] == PrimOp::Sub as u32 => {
                *b.record(b.data[i][1]).first_chunk::<2>()?
            }
            // A call's record ends with three words past its arguments.
            Tag::Call if self.intrinsic(v).as_deref() == Some("int_wrapping_sub") => {
                *b.record(b.data[i][1]).first_chunk::<2>()?
            }
            _ => return None,
        };
        (self.const_usize(one) == Some(1)).then_some(r)
    }
}

/// The stable list places the hoisting pass lists for loop `l`
/// (`Body::stable_lists`, sorted by loop), each with its region's bounds
/// and whether the loop keeps its buffer too (else only its length).
/// O(log rows + its rows).
pub fn stable_rows(b: &Body, l: u32) -> impl Iterator<Item = (ListRoot, [u32; 3])> + '_ {
    let rows = &b.stable_lists;
    let from = rows.partition_point(|r| r[0] < l);
    rows[from..]
        .iter()
        .take_while(move |r| r[0] == l)
        .filter_map(|&[_, word, before, after, buffer]| {
            let p = b.decode_place(word)?;
            let PlaceRoot::Local(local) = p.root else {
                return None;
            };
            Some((ListRoot::at(local.raw(), p.proj)?, [before, after, buffer]))
        })
}

/// A local's two stores (`Sets::sets`), the one `in_loop` rejects first:
/// an induction variable's start and step, when it has exactly two.
pub fn init_and_step(
    stores: &[(u32, u32)],
    in_loop: impl FnOnce(u32) -> bool,
) -> Option<[(u32, u32); 2]> {
    let [a, c] = stores else { return None };
    Some(if in_loop(a.0) { [*c, *a] } else { [*a, *c] })
}

thread_local! {
    static SCANS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Counts, on this thread while `run` runs, the instructions the loop
/// proofs list: each body's tree and stores once, and each loop's or run
/// of statements' instructions they ask for.
pub fn count_scans<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let outer = SCANS.with(|w| w.replace(Some(0)));
    let out = run();
    let work = SCANS.with(|w| w.replace(outer)).unwrap_or(0);
    if let Some(outer) = outer {
        SCANS.with(|w| w.set(Some(outer + work)));
    }
    (out, work)
}

fn scans(n: usize) {
    hd_base::work::add(hd_base::work::Counter::LoopScans, n as u64);
    SCANS.with(|w| {
        if let Some(k) = w.get() {
            w.set(Some(k + n as u64));
        }
    });
}

/// Compressed rows of `edges` by source, over `nodes` nodes: node `x`'s
/// targets are `succ[at[x]..at[x + 1]]`. O(nodes + edges).
fn csr(nodes: usize, edges: &[(u32, u32)]) -> (Vec<u32>, Vec<u32>) {
    let mut at = vec![0u32; nodes + 1];
    for &(x, _) in edges {
        at[x as usize + 1] += 1;
    }
    for x in 0..nodes {
        at[x + 1] += at[x];
    }
    let mut fill = at.clone();
    let mut succ = vec![0u32; edges.len()];
    for &(x, y) in edges {
        succ[fill[x as usize] as usize] = y;
        fill[x as usize] += 1;
    }
    (at, succ)
}
