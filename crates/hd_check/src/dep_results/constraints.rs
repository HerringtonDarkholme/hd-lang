//! Outlives constraints for places, stores, constructions, control flow,
//! calls and closures, and the checked edges into universal regions
//! (`docs/future-work/LIFETIME_SOLVER.md`): [`solve`] generates them for
//! one body, solves them and reads its diagnostics off the solution.

use hd_base::{HashMap, HashSet, NodeIdx, SliceSort as _};
use hd_tir::ir::{IntrinsicOp, PlaceRoot, Proj, Role, Tag, Word, slots};
use hd_types::{Ty, TyData};

use super::{
    ANY, ANY_FN, BOX_MSG, DepSig, ELEM_MSG, FIELD_MSG, FN_PARAM_MSG, FnDest, HOST_MSG, Inputs,
    Layout, NO_SOURCE, Origin, PROVIDER_MSG, STATIC_MSG, STORE_FN_MSG, Set, Verdicts, arg_ty,
    capture_slot, closure_allows, intrinsic_ops, local_ref, referent, referent_fn, returned,
    scoped_uses, site, source_arg, w,
};
use crate::local_refs::holds_local_ref;
use crate::regions::{Category, Constraints, Edge, Loan, Region, RegionTable, Solution};

/// A value's or a place's region variables, one per slot of its type.
type View = (Vec<Region>, Ty);

/// A view's top slots, and the slots behind them.
type Split = (Vec<Region>, Vec<Region>);

/// What a place lies in, and its slots ([`Gen::place`]).
type Lies = (Option<Result<Origin, u32>>, Option<View>);

struct Gen<'s, 'a> {
    s: &'s Inputs<'a>,
    t: RegionTable,
    c: Constraints,
    /// Each loan's origin: loan `l` is `loans[l]`, placeholders first.
    loans: Vec<Origin>,
    ids: HashMap<Origin, Loan>,
    /// The key of instruction `i` is `locals + i`.
    locals: u32,
    at: u32,
    /// `'static`, which a captured function value's destination needs.
    stat: Region,
    /// Each checked edge's diagnostic node, by its regions and instruction.
    sites: HashMap<(Region, Region, u32), NodeIdx>,
    /// The hidden local of each `&e` on a value, by its `Ref`.
    hidden: HashMap<u32, View>,
    /// The universal region of a returned slot's lifetimes, of the
    /// function's result (`None`) or of closure `sub`'s.
    universal: HashMap<(Option<usize>, Vec<u32>), Region>,
}

/// What the result checks need: the function's map with what each
/// returned top slot may depend on ([`super::result_slots`]), the body's
/// root, each closure's map, and the values the scope check skips.
pub(super) struct Returns<'x> {
    pub(super) own: Option<(&'x DepSig, &'x [Option<Vec<u32>>])>,
    pub(super) root: u32,
    pub(super) closures: &'x [(usize, DepSig)],
    pub(super) skip: &'x HashSet<u32>,
}

/// Whether reference type `t`, under any `mut`, is `&mut`.
fn is_mut(pool: hd_types::Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Ref { mutable, .. } => mutable,
        TyData::Mut(i) => is_mut(pool, i),
        _ => false,
    }
}

/// Whether slot `k` of `l` lies behind a `&mut`, and so is invariant.
fn under_mut(pool: hd_types::Types<'_>, l: &Layout, k: usize) -> bool {
    (0..k.min(l.slots.len())).any(|j| l.slots[j].end > k && is_mut(pool, l.slots[j].ty))
}

impl Gen<'_, '_> {
    fn loan(&mut self, o: Origin) -> Loan {
        *self.ids.entry(o).or_insert_with(|| {
            self.loans.push(o);
            w(self.loans.len() - 1)
        })
    }

    fn seed(&mut self, r: Region, o: Origin) {
        let l = self.loan(o);
        self.c.seed(r, l);
    }

    fn flow(&mut self, a: Region, b: Region) {
        self.c.outlives(a, b, self.at, Category::Flow);
    }

    fn val(&self, v: u32) -> Option<View> {
        let t = self.s.ty(v)?;
        Some((self.t.slots(self.locals + v).collect(), t))
    }

    fn local(&self, l: u32) -> View {
        (self.t.slots(l).collect(), self.s.b.local_ty[l as usize])
    }

    /// How many regions a value of type `t` has: one per slot, or one for
    /// a trait value with none.
    fn width(&self, t: Ty) -> u32 {
        let l = self.s.ls.of(t);
        w(l.slots.len().max(usize::from(l.width > 0)))
    }

    /// A view's top slots and the slots behind them.
    fn split(&self, v: &View) -> Split {
        let l = self.s.ls.of(v.1);
        let (t, i): (Vec<usize>, _) =
            (0..v.0.len()).partition(|&k| l.slots.get(k).map_or(k == 0, |s| s.top));
        let at = |ks: Vec<usize>| ks.into_iter().map(|k| v.0[k]).collect();
        (at(t), at(i))
    }

    /// Every top slot of `a` ⊑ every top slot of `b` through one hub, and
    /// every slot behind them through another.
    fn hubs(&mut self, a: &Split, b: &Split) {
        for (from, to) in [(&a.0, &b.0), (&a.1, &b.1)] {
            if from.is_empty() || to.is_empty() {
                continue;
            }
            let h = self.t.fresh(1).start;
            let edges = from
                .iter()
                .map(|&f| (f, h))
                .chain(to.iter().map(|&t| (h, t)));
            edges.for_each(|(x, y)| self.c.outlives(x, y, self.at, Category::Flow));
        }
    }

    /// `a`'s slots ⊑ `b`'s, and back in each slot under a `&mut` of `b`,
    /// or in all of them with `inv`. Where the two do not line up, or with
    /// `merge`, every top slot flows through one hub and every slot behind
    /// them through another (`ref.dep.origin.position.union`).
    fn relate(&mut self, a: &View, b: &View, inv: bool, merge: bool) {
        let (pool, la, lb) = (self.s.pool, self.s.ls.of(a.1), self.s.ls.of(b.1));
        if !merge && a.0.len() == b.0.len() && la.tops == lb.tops {
            for (k, (&x, &y)) in a.0.iter().zip(&b.0).enumerate() {
                if inv || under_mut(pool, &lb, k) {
                    self.c.equal(x, y, self.at);
                } else {
                    self.flow(x, y);
                }
            }
            return;
        }
        let (sa, sb) = (self.split(a), self.split(b));
        self.hubs(&sa, &sb);
        if inv {
            self.hubs(&sb, &sa);
        }
    }

    /// Field `ty` of `v` by its field slot map `at`: the instance's slots,
    /// and a fresh one where the map names none, which a hub reads from
    /// the instance (`dir.0`) or stores into it (`dir.1`).
    fn field(&mut self, v: &View, ty: Ty, at: Option<Vec<usize>>, dir: (bool, bool)) -> View {
        let (lc, n) = (self.s.ls.of(ty), self.width(ty) as usize);
        // A top slot that is no one slot of the instance takes all of them.
        let at = at.filter(|at| at.len() == n && lc.tops.iter().all(|&k| at[k] < v.0.len()));
        let mut fresh: Split = Default::default();
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            if let Some(&r) = at.as_ref().and_then(|at| v.0.get(at[k])) {
                out.push(r);
                continue;
            }
            let r = self.t.fresh(1).start;
            let top = lc.slots.get(k).is_none_or(|s| s.top);
            (if top { &mut fresh.0 } else { &mut fresh.1 }).push(r);
            out.push(r);
        }
        let parent = self.split(v);
        if dir.0 {
            self.hubs(&parent, &fresh);
        }
        if dir.1 {
            self.hubs(&fresh, &parent);
        }
        (out, ty)
    }

    /// The place `proj` reaches from `v`: a field by its field slot map, a
    /// dereference the reference's slots after its first. An element, a
    /// trait value or a counted reference stops at what it is reached from.
    fn project(&mut self, mut v: View, proj: &[Proj], dir: (bool, bool)) -> View {
        let (pool, mut variant) = (self.s.pool, None);
        for p in proj {
            match *p {
                Proj::Variant(k) => variant = Some(k),
                Proj::Field(j) => match self.s.ls.field_slots(self.s.cx, v.1, variant.take(), j) {
                    Some((child, at)) => v = self.field(&v, child, at, dir),
                    None => break,
                },
                Proj::Deref if local_ref(pool, v.1) == Some((false, false)) => {
                    v = (v.0[1.min(v.0.len())..].to_vec(), referent(pool, v.1));
                }
                _ => break,
            }
        }
        v
    }

    /// The value an element place's collection was read from, through
    /// coercions: that place (`ref.dep.origin.element`).
    fn read_of(&self, mut v: u32) -> Option<u32> {
        let b = self.s.b;
        while self.s.ty(v).is_some() && b.tags[v as usize] == Tag::Coerce {
            v = b.data[v as usize][0];
        }
        (self.s.ty(v).is_some() && b.tags[v as usize] == Tag::Read).then_some(v)
    }

    /// What the place `word` of instruction `i` lies in: a loan, or a
    /// reference value it reborrows from (`ref.dep.origin.place`).
    fn lies(&self, i: u32, word: u32) -> Option<Result<Origin, u32>> {
        let b = self.s.b;
        let p = b.decode_place(word)?;
        let (first, read) = (p.proj.first().copied(), |v: u32| b.data[v as usize][0]);
        match p.root {
            PlaceRoot::Local(l) => Some(Ok(Origin::Local(l.raw()))),
            PlaceRoot::Value(v) if matches!(first, Some(Proj::Elem(_))) => {
                match self.read_of(v.0) {
                    Some(r) => self.lies(r, read(r)),
                    None => Some(Ok(Origin::Hidden(i))),
                }
            }
            PlaceRoot::Value(v) if first == Some(Proj::Deref) => {
                match self.s.ty(v.0).and_then(|t| local_ref(self.s.pool, t)) {
                    Some((false, _)) => Some(Err(v.0)),
                    _ if b.tags.get(v.0 as usize) == Some(&Tag::Read) => self.lies(v.0, read(v.0)),
                    _ => Some(Ok(Origin::Hidden(v.0))),
                }
            }
            PlaceRoot::Value(_) => Some(Ok(Origin::Hidden(i))),
            PlaceRoot::Global(d, _) => Some(Ok(Origin::Global(d.raw()))),
            _ => None,
        }
    }

    /// The place `word`: what it lies in ([`Gen::lies`]), and its slots
    /// past its first `limit` projections none (`ref.dep.origin.place`),
    /// none for a top-level binding's and through a counted reference.
    fn place(&mut self, i: u32, word: u32, limit: usize, dir: (bool, bool)) -> Lies {
        let b = self.s.b;
        let Some(p) = b.decode_place(word) else {
            return (None, None);
        };
        let first = p.proj.first().copied();
        let root = match p.root {
            PlaceRoot::Local(l) => Some(self.local(l.raw())),
            PlaceRoot::Value(v) if matches!(first, Some(Proj::Elem(_))) => {
                match self.read_of(v.0) {
                    Some(r) => self.place(r, b.data[r as usize][0], usize::MAX, dir).1,
                    None => self.val(v.0),
                }
            }
            PlaceRoot::Value(v) if first == Some(Proj::Deref) => {
                let kind = self.s.ty(v.0).and_then(|t| local_ref(self.s.pool, t));
                if matches!(kind, Some((false, _))) {
                    self.val(v.0)
                } else {
                    None
                }
            }
            PlaceRoot::Value(v) => self.val(v.0),
            _ => None,
        };
        let proj = &p.proj[..limit.min(p.proj.len())];
        (self.lies(i, word), root.map(|r| self.project(r, proj, dir)))
    }

    /// Puts what a place lies in into region `r`.
    fn lies_in(&mut self, at: Option<Result<Origin, u32>>, r: Region) {
        match at {
            Some(Ok(o)) => self.seed(r, o),
            Some(Err(v)) => {
                let from = self.val(v).expect("a reference").0[0];
                self.flow(from, r);
            }
            None => {}
        }
    }

    /// Stores `value` into the place `word`: through a reference, into
    /// the whole referent (`ref.dep.origin.position.union`).
    fn store(&mut self, word: u32, value: &View, merge: bool) {
        let p = self.s.b.decode_place(word);
        let deref = p.and_then(|p| p.proj.iter().position(|&x| x == Proj::Deref));
        let limit = deref.map_or(usize::MAX, |k| k + 1);
        if let (_, Some(p)) = self.place(self.at, word, limit, (false, true)) {
            self.relate(value, &p, false, merge);
        }
    }

    /// Seeds parameter `l`'s slots with its placeholders, but for a
    /// function type written inside another type (`ref.generic.outlives.written`).
    fn param(&mut self, l: u32) {
        let (pool, ls) = (self.s.pool, self.s.ls);
        let (regions, pt) = (self.local(l).0, ls.of(self.s.params[l as usize]));
        if pt.slots.len() != regions.len() {
            // Each slot takes all of them, as `fit` does.
            for (&r, k) in regions
                .iter()
                .flat_map(|r| (0..pt.slots.len().max(1)).map(move |k| (r, k)))
            {
                self.seed(r, Origin::Param(l, w(k)));
            }
            return;
        }
        for (k, &r) in regions.iter().enumerate() {
            let s = pt.slots[k];
            let hidden = match (0..k).rev().find(|&j| pt.slots[j].end > k) {
                None => s.written,
                Some(j) => s.fun && !capture_slot(pool, referent(pool, pt.slots[j].ty)),
            };
            if !hidden {
                self.seed(r, Origin::Param(l, w(k)));
            }
        }
    }

    /// Every region of `from` ⊑ every region of `to`, through one hub.
    fn each_into(&mut self, from: &[Region], to: &[Region]) {
        self.hubs(&(from.to_vec(), vec![]), &(to.to_vec(), vec![]));
    }

    /// Call argument `a` as a value of its source type `at`: an address
    /// the call takes for a receiver, with its place's slots behind it
    /// (`fn.method.auto-address`).
    fn arg(&mut self, a: u32, at: Ty) -> View {
        let v = self.val(a).expect("an argument");
        if at == v.1 {
            return v;
        }
        let (_, p) = self.place(a, self.s.b.data[a as usize][0], usize::MAX, (true, false));
        let behind = p.map(|p| p.0).unwrap_or_default();
        ([&v.0[..1], &behind].concat(), at)
    }

    /// Puts what a counted reference argument `a` is lent from into `r`
    /// (`ref.dep.origin.lend`).
    fn lend(&mut self, a: u32, r: Region) {
        let mut v = a as usize;
        let b = self.s.b;
        while b.tags[v] == Tag::Coerce && self.s.ty(b.data[v][0]).is_some() {
            v = b.data[v][0] as usize;
        }
        let at = match b.tags[v] {
            Tag::Read => self.place(w(v), b.data[v][0], 0, (false, false)).0,
            _ => Some(Ok(Origin::Hidden(w(v)))),
        };
        self.lies_in(at, r);
    }

    /// A call by its map `sig` (`ref.dep.call`): one fresh variable per
    /// callee lifetime and type-parameter slot, each argument slot ⊑ its
    /// variable, equal under a `&mut` (`ref.generic.store`), and the slots
    /// behind a slot ⊑ its variable too, since the callee may return them.
    /// With `result`, each result slot takes its lifetime's variable. At
    /// a call that may store through a `&mut` argument, what flows into a
    /// variable is checked as a store into the caller's place
    /// (`ref.lifetime.store.call`).
    fn call(&mut self, sig: &DepSig, me: &View, result: bool) {
        let (b, pool, ls, iu) = (self.s.b, self.s.pool, self.s.ls, self.at as usize);
        let cat = if self.s.stores.contains_key(&self.at) {
            Category::CallerPlace
        } else {
            Category::Flow
        };
        let mut vars: HashMap<u32, Region> = HashMap::default();
        let mut var = |t: &mut RegionTable, id| *vars.entry(id).or_insert_with(|| t.fresh(1).start);
        let any = result && sig.outs.contains(&Some(ANY));
        let any_fn = result && sig.outs.contains(&Some(ANY_FN));
        for (p, (pt, ids)) in sig.params.iter().enumerate() {
            let Some((a, at)) = source_arg(b, iu, p).and_then(|a| Some((a, arg_ty(pool, b, a)?)))
            else {
                continue;
            };
            if b.tags[a as usize] != Tag::Ref && !ls.holds(at) {
                let mut ids = ids.clone();
                ids.extend(any.then_some(ANY));
                ids.dedup();
                let h = self.t.fresh(1).start;
                self.lend(a, h);
                let rs: Vec<Region> = ids.into_iter().map(|id| var(&mut self.t, id)).collect();
                self.flow_vars(&[h], &rs, cat);
                continue;
            }
            let v = self.arg(a, at);
            let lay = ls.of(at);
            if any {
                let r = var(&mut self.t, ANY);
                self.each_into(&v.0, &[r]);
            }
            if any_fn {
                let r = var(&mut self.t, ANY_FN);
                for (k, _) in lay.slots.iter().enumerate().filter(|(_, x)| x.fun) {
                    self.flow(v.0[k], r);
                }
            }
            if v.0.len() != ids.len() || lay.tops != ls.of(*pt).tops {
                // `ref.generic.call.unaligned`: each lifetime takes it all.
                let rs: Vec<Region> = ids.iter().map(|&id| var(&mut self.t, id)).collect();
                self.flow_vars(&v.0, &rs, cat);
                continue;
            }
            for (k, &id) in ids.iter().enumerate() {
                let r = var(&mut self.t, id);
                if under_mut(pool, &lay, k) {
                    self.c.equal(v.0[k], r, self.at);
                } else {
                    self.flow_vars(&[v.0[k]], &[r], cat);
                }
                for j in k + 1..lay.slots[k].end {
                    self.flow_vars(&[v.0[j]], &[r], cat);
                }
            }
        }
        if !result {
            return;
        }
        let (have, want) = (ls.of(me.1), ls.of(sig.ret));
        if have.tops.len() == want.tops.len() && !have.tops.is_empty() {
            for (k, &to) in me.0.iter().enumerate() {
                if let Some(&Some(id)) = sig.outs.get(k) {
                    let r = var(&mut self.t, id);
                    self.flow(r, to);
                }
            }
        } else {
            let rs: Vec<Region> = sig
                .all_ids()
                .into_iter()
                .map(|id| var(&mut self.t, id))
                .collect();
            self.each_into(&rs, &me.0);
        }
    }

    /// Every region of `from` ⊑ every variable of `rs`, through one hub
    /// where `from` is several, by edges of category `cat`.
    fn flow_vars(&mut self, from: &[Region], rs: &[Region], cat: Category) {
        let h = match from {
            &[f] => f,
            _ if cat == Category::Flow => return self.each_into(from, rs),
            _ => {
                let h = self.t.fresh(1).start;
                for &f in from {
                    self.flow(f, h);
                }
                h
            }
        };
        let at = self.s.b.syn[self.at as usize];
        for &r in rs {
            match cat {
                Category::Flow => self.flow(h, r),
                _ => self.checked(h, r, cat, at),
            }
        }
    }

    /// A checked edge `from ⊑ to` at the current instruction, reported
    /// at node `at`.
    fn checked(&mut self, from: Region, to: Region, cat: Category, at: NodeIdx) {
        self.c.outlives(from, to, self.at, cat);
        self.sites.insert((from, to, self.at), at);
    }

    /// Where a diagnostic about value `v` of the current instruction
    /// points: at `v`, or at the instruction for a constant.
    fn node(&self, v: u32) -> NodeIdx {
        let b = self.s.b;
        let here = b.syn[self.at as usize];
        let at = (self.s.ty(v).is_some()).then(|| b.syn.get(v as usize).copied());
        at.flatten().filter(|n| *n != NodeIdx::NONE).unwrap_or(here)
    }

    /// The capture slots at the top of view `v`.
    fn fun_tops(&self, v: &View) -> Vec<Region> {
        let l = self.s.ls.of(v.1);
        let fun = |k: usize| l.slots.get(k).is_some_and(|s| s.top && s.fun);
        (v.0.iter().enumerate())
            .filter(|&(k, _)| fun(k))
            .map(|(_, &r)| r)
            .collect()
    }

    /// A store into the place `word` the caller may own: each top slot of
    /// `value` ⊑ the top slot of the place it lands in, checked against
    /// that slot's lifetime (`ref.lifetime.store.field`); with `merge` or
    /// where the two do not line up, every top slot ⊑ each of them. A
    /// captured function value stored into a top-level binding ⊑ `'static`.
    fn caller_store(&mut self, word: u32, value: &View, merge: bool) {
        let (b, ls) = (self.s.b, self.s.ls);
        let at = b.syn[self.at as usize];
        let Some(p) = b.decode_place(word) else {
            return;
        };
        if let PlaceRoot::Global(..) = p.root {
            for r in self.fun_tops(value) {
                self.checked(r, self.stat, Category::CallerPlace, at);
            }
            return;
        }
        if p.proj.first() != Some(&Proj::Deref) {
            return;
        }
        let (_, Some(place)) = self.place(self.at, word, usize::MAX, (false, false)) else {
            return;
        };
        let (from, to) = (self.split(value).0, self.split(&place).0);
        if !merge && from.len() == to.len() && ls.of(value.1).tops.len() == from.len() {
            for (&x, &y) in from.iter().zip(&to) {
                self.checked(x, y, Category::CallerPlace, at);
            }
        } else {
            self.flow_vars(&from, &to, Category::CallerPlace);
        }
    }

    /// Each destination that keeps a captured function value
    /// (`fn.capture.local-store`): its top capture slots ⊑ `'static`, by
    /// the destination's category. A call's parameter that may only use or
    /// pass on its argument (`FnDest::Scoped`) checks nothing.
    fn escapes(&mut self, i: u32) {
        let (b, s, iu) = (self.s.b, self.s, i as usize);
        let mut kept: Vec<(u32, Category)> = Vec::new();
        let mut parts = |pick: &dyn Fn(Word) -> bool, cat| {
            slots(b, iu, |_, v, word| {
                if pick(word) {
                    kept.push((v, cat));
                }
            });
        };
        let consume = |w: Word| matches!(w, Word::Value(Role::Consume));
        match b.tags[iu] {
            Tag::Ref
                if s.boxes.contains(&i)
                    && matches!(s.pool.get(b.ty[iu]), TyData::Ref { counted: true, .. }) =>
            {
                if let (_, Some(p)) = self.place(i, b.data[iu][0], usize::MAX, (false, false)) {
                    for r in self.fun_tops(&p) {
                        self.checked(r, self.stat, Category::Box, b.syn[iu]);
                    }
                }
            }
            Tag::Store => {
                let [place, v] = b.data[iu];
                let local = b
                    .decode_place(place)
                    .is_some_and(|p| matches!(p.root, PlaceRoot::Local(_)) && p.proj.is_empty());
                if !local {
                    kept.push((v, Category::FieldCapture));
                }
            }
            Tag::NewStruct
            | Tag::CopyStruct
            | Tag::NewVariant
            | Tag::NewTuple
            | Tag::NewList
            | Tag::NewMap => {
                let outlives = s.outlives_fields(iu);
                parts(&consume, Category::FieldCapture);
                kept.retain(|(v, _)| !outlives.contains(v));
            }
            Tag::Intrinsic => parts(&consume, Category::FieldCapture),
            Tag::With | Tag::ContextNew => parts(&|w| w == Word::Provider, Category::Provider),
            Tag::CallHost => parts(&|w| matches!(w, Word::Value(_)), Category::HostCall),
            Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue => self.call_escapes(i),
            _ => {}
        }
        for (v, cat) in kept {
            let at = self.node(v);
            for r in self.val(v).map(|v| self.fun_tops(&v)).unwrap_or_default() {
                self.checked(r, self.stat, cat, at);
            }
        }
    }

    /// Each capture slot of call `i`'s arguments by where its parameter
    /// keeps it: a type parameter bounded by `'static`, one that only
    /// forwards parameters' captures, or a function type written inside
    /// another type.
    fn call_escapes(&mut self, i: u32) {
        let (b, pool, ls) = (self.s.b, self.s.pool, self.s.ls);
        for (p, (t, dests)) in self.s.call_params(i as usize).into_iter().enumerate() {
            let Some((a, vt)) =
                source_arg(b, i as usize, p).and_then(|a| Some((a, arg_ty(pool, b, a)?)))
            else {
                continue;
            };
            if dests.is_empty() || !ls.holds(vt) {
                continue;
            }
            let v = self.arg(a, vt);
            let at = self.node(a);
            let hub = (v.0.len() != ls.of(t).slots.len()).then(|| {
                let h = self.t.fresh(1).start;
                v.0.iter().for_each(|&r| self.flow(r, h));
                h
            });
            for (s, dest) in dests {
                let cat = match dest {
                    FnDest::Scoped => continue,
                    FnDest::Static => Category::StaticParam,
                    FnDest::Forward => Category::Forward,
                    FnDest::Kept => Category::FieldCapture,
                };
                let from = hub.unwrap_or(v.0[s]);
                self.checked(from, self.stat, cat, at);
            }
        }
    }

    /// Returned value `v`'s top slots ⊑ the universal region of the
    /// lifetimes of each result slot, checked (`ref.dep.return`).
    fn returns(&mut self, v: u32, sub: Option<usize>, slots: &[Option<Vec<u32>>]) {
        let Some(view) = self.val(v) else { return };
        let mut from = self.split(&view).0;
        if from.len() != slots.len() {
            let h = self.t.fresh(1).start;
            for &r in &from {
                self.c.outlives(r, h, v, Category::Flow);
            }
            from = vec![h; slots.len()];
        }
        for (&f, ids) in from.iter().zip(slots) {
            let Some(ids) = ids else { continue };
            let t = &mut self.t;
            let u = *(self.universal.entry((sub, ids.clone()))).or_insert_with(|| t.fresh(1).start);
            self.c.outlives(f, u, v, Category::Result);
        }
    }

    /// The local reference a trait value reference is made from, through
    /// its block and the coercion to the trait value: the `&dyn` takes it
    /// as a coercion would (`trait.dyn.ref.convert`).
    fn dyn_source(&self, mut v: u32) -> Option<View> {
        let b = self.s.b;
        while self.s.ty(v).is_some() {
            let [x, y] = b.data[v as usize];
            match b.tags[v as usize] {
                Tag::Block => v = y,
                Tag::Coerce if local_ref(self.s.pool, self.s.ty(x)?) == Some((false, false)) => {
                    return self.val(x);
                }
                Tag::Coerce => v = x,
                _ => return None,
            }
        }
        None
    }

    fn step(&mut self, i: u32) {
        let (b, pool) = (self.s.b, self.s.pool);
        let iu = i as usize;
        let [a, d] = b.data[iu];
        let Some(me) = self.val(i) else { return };
        if b.tags[iu] == Tag::RefDyn
            && let Some(v) = self.dyn_source(a)
        {
            return self.relate(&v, &me, false, false);
        }
        match b.tags[iu] {
            Tag::Store => {
                if let Some(v) = self.val(d) {
                    self.store(a, &v, false);
                    self.caller_store(a, &v, false);
                }
            }
            Tag::Intrinsic => {
                let ops = intrinsic_ops(b, iu);
                let keeps = matches!(
                    IntrinsicOp::from_u32(a),
                    Some(IntrinsicOp::ListPush | IntrinsicOp::MapSet)
                );
                if let Some((&coll, kept)) = ops.split_first()
                    && keeps
                    && self.s.ty(coll).is_some_and(|t| holds_local_ref(pool, t))
                    && b.tags[coll as usize] == Tag::Read
                {
                    for &v in kept {
                        if let Some(v) = self.val(v).filter(|v| holds_local_ref(pool, v.1)) {
                            self.store(b.data[coll as usize][0], &v, true);
                            self.caller_store(b.data[coll as usize][0], &v, true);
                        }
                    }
                }
                // A collection intrinsic's result holds what its operands hold.
                for &v in ops {
                    if let Some(v) = self.val(v) {
                        self.relate(&v, &me, false, true);
                    }
                }
            }
            Tag::Break => {
                if let (Some(v), Some(&t)) = (self.val(d), b.label_inst.get(a as usize)) {
                    let to = self.val(t).expect("a label's instruction");
                    self.relate(&v, &to, false, false);
                }
            }
            Tag::Ref if !me.0.is_empty() => {
                let inv = is_mut(pool, me.1);
                let (at, place) = self.place(i, a, usize::MAX, (true, inv));
                self.lies_in(at, me.0[0]);
                let inner = (me.0[1..].to_vec(), referent(pool, me.1));
                // A `&mut` of a local's or a reference's part has that
                // part's own slots, by the field slot maps
                // (`ref.lifetime.store.field`).
                let own = place.as_ref().filter(|p| {
                    inv && matches!(at, Some(Ok(Origin::Local(_)) | Err(_)))
                        && referent_fn(pool, p.1) == referent_fn(pool, inner.1)
                });
                if let Some(p) = own {
                    return self.relate(p, &inner, true, false);
                }
                // Otherwise the referent holds what its storage holds, each
                // slot the union of all of them through a hub where the
                // place is a part of it, even one with as many slots.
                let held = match at {
                    Some(Ok(Origin::Local(l))) => Some(self.local(l)),
                    Some(Err(v)) => self
                        .val(v)
                        .map(|v| self.project(v, &[Proj::Deref], (true, inv))),
                    // The hidden local of `&e` on a value.
                    Some(Ok(Origin::Hidden(h))) if h == i => place.map(|place| {
                        let h = (self.t.fresh(self.width(inner.1)).collect(), inner.1);
                        self.relate(&place, &h, false, false);
                        self.hidden.insert(i, h.clone());
                        h
                    }),
                    _ => None,
                };
                if let Some(held) = held {
                    let part = referent_fn(pool, held.1) != referent_fn(pool, inner.1);
                    self.relate(&held, &inner, inv, part);
                }
            }
            Tag::Read => match self.place(i, a, usize::MAX, (true, false)) {
                (_, Some(p)) => self.relate(&p, &me, false, false),
                // A trait value through a counted reference has the origins
                // of the place that reference was read from.
                (at, None)
                    if super::is_dyn(pool, me.1) && !matches!(at, Some(Ok(Origin::Global(_)))) =>
                {
                    me.0.iter().for_each(|&r| self.lies_in(at, r));
                }
                _ => {}
            },
            Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue => {
                let s = self.s;
                if let Some(sig) = s.deps.get(&i).or_else(|| s.stores.get(&i)) {
                    self.call(sig, &me, s.deps.contains_key(&i));
                }
            }
            // `fn.capture.local-origins`: what each captured local holds.
            Tag::Closure if !b.is_instantiation(iu) => {
                if let [start, len, ..] = *b.record(d) {
                    for c in start..start.saturating_add(len) {
                        let Some(&l) = b.cap_local.get(c as usize) else {
                            continue;
                        };
                        if !self.s.reported.contains(&l) {
                            let held = self.split(&self.local(l.raw())).0;
                            self.each_into(&held, &me.0);
                        }
                    }
                }
            }
            Tag::Coerce => {
                if let Some(v) = self.val(a) {
                    self.relate(&v, &me, false, false);
                }
            }
            // A block's, a branch's and a loop's value, and a construction's
            // parts, each by its field slot map (`ref.dep.origin.parts`).
            tag => {
                let mut parts = Vec::new();
                slots(b, iu, |_, v, word| {
                    if matches!(word, Word::Block | Word::Value(Role::Tail | Role::Consume)) {
                        parts.push(v);
                    }
                });
                let cons = matches!(tag, Tag::NewTuple | Tag::NewStruct | Tag::NewVariant);
                let (rec, variant) = (
                    if cons { b.record(d) } else { &[] },
                    (tag == Tag::NewVariant).then_some(a),
                );
                for v in parts {
                    let j = rec.iter().position(|&x| x == v);
                    let field =
                        j.and_then(|j| self.s.ls.field_slots(self.s.cx, me.1, variant, w(j)));
                    let to = match field {
                        Some((ty, at)) => self.field(&me, ty, at, (false, true)),
                        None => me.clone(),
                    };
                    if let Some(v) = self.val(v) {
                        self.relate(&v, &to, false, false);
                    }
                }
            }
        }
    }
}

/// Generates the body's constraints and checked edges, solves them, and
/// returns the diagnostics the solution gives.
pub(super) fn solve(s: &Inputs<'_>, r: &Returns<'_>) -> Verdicts {
    let b = s.b;
    let mut g = Gen {
        s,
        t: RegionTable::default(),
        c: Constraints::default(),
        loans: Vec::new(),
        ids: HashMap::default(),
        locals: w(b.local_ty.len()),
        at: 0,
        stat: 0,
        sites: HashMap::default(),
        hidden: HashMap::default(),
        universal: HashMap::default(),
    };
    let widths: Vec<u32> = (b.local_ty.iter().map(|&t| g.width(t)))
        .chain((0..b.tags.len()).map(|i| g.width(b.ty[i]).max(u32::from(b.tags[i] == Tag::Ref))))
        .collect();
    for n in widths {
        g.t.push(n);
    }
    g.stat = g.t.fresh(1).start;
    for l in 0..g.locals {
        if b.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM != 0 {
            g.param(l);
        }
    }
    for i in 0..w(b.tags.len()) {
        g.at = i;
        g.step(i);
        g.escapes(i);
    }
    if let Some((_, slots)) = r.own {
        for v in returned(b, r.root) {
            g.returns(v, None, slots);
        }
    }
    for (sub, dep) in r.closures {
        let slots: Vec<_> = (dep.result_ids(s.ls, dep.ret).into_iter().map(Some)).collect();
        for v in returned(b, b.sub_root[*sub]) {
            g.returns(v, Some(*sub), &slots);
        }
    }
    let sol = g.c.solve(g.t.len());
    g.verdicts(&sol, r)
}

impl Gen<'_, '_> {
    /// The origins the top slots of view `v` hold.
    fn origins(&self, sol: &Solution, v: &View) -> Set {
        let tops = self.split(v).0;
        let loans = tops.iter().flat_map(|&r| sol.value(r).iter());
        let mut set: Set = loans.map(|l| self.loans[l as usize]).collect();
        set.sort_stable();
        set.dedup();
        set
    }

    /// Whether a top slot of value `v` holds `o`.
    fn has(&self, sol: &Solution, v: u32, o: Origin) -> bool {
        let (Some(&l), Some(v)) = (self.ids.get(&o), self.val(v)) else {
            return false;
        };
        (self.split(&v).0.iter()).any(|&r| sol.value(r).contains(l))
    }

    /// What instruction `i` uses, for the scope check: its value's
    /// origins, and for a place it reads, borrows or stores through, what
    /// each local or hidden local the place lies in holds, with that local.
    fn uses(&self, sol: &Solution, i: u32) -> Vec<(Option<Origin>, Origin)> {
        let b = self.s.b;
        let val = self.val(i).map(|v| self.origins(sol, &v));
        let mut out: Vec<_> = (val.unwrap_or_default().into_iter())
            .map(|o| (None, o))
            .collect();
        let (tag, word) = (b.tags[i as usize], b.data[i as usize][0]);
        let deref = || (b.decode_place(word)).is_some_and(|p| p.proj.contains(&Proj::Deref));
        if !(matches!(tag, Tag::Read | Tag::Ref) || tag == Tag::Store && deref()) {
            return out;
        }
        let storage = match self.lies(i, word) {
            Some(Ok(o)) => vec![o],
            Some(Err(v)) => self
                .val(v)
                .map(|v| self.origins(sol, &v))
                .unwrap_or_default(),
            None => vec![],
        };
        for st in storage {
            let held = match st {
                Origin::Local(l) => Some(self.local(l)),
                Origin::Hidden(h) => self.hidden.get(&h).cloned(),
                _ => None,
            };
            let held = held.map(|v| self.origins(sol, &v)).unwrap_or_default();
            out.extend(held.into_iter().map(|o| (Some(st), o)));
        }
        out
    }

    /// Whether slot `j` of parameter `q`, a place the caller owns, may take
    /// `o`: the run of its referent's top slot has one lifetime, and a
    /// result of that lifetime may depend on `o` (`ref.lifetime.store`). A
    /// capture slot takes nothing (`fn.capture.local-store`).
    fn takes(&self, q: u32, j: usize, o: Origin) -> bool {
        let (pool, ls) = (self.s.pool, self.s.ls);
        let Some(own) = self.s.own else { return false };
        let Some((t, ids)) = own.params.get(q as usize) else {
            return false;
        };
        let lay = ls.of(*t);
        let Some(&s0) = lay.tops.iter().rev().find(|&&s0| s0 < j) else {
            return false;
        };
        if lay.slots.get(j).is_none_or(|x| x.fun) {
            return false;
        }
        let k = j - s0 - 1;
        let inner = ls.of(referent(pool, lay.slots[s0].ty));
        let Some((lo, hi)) = inner.runs().find(|&(lo, hi)| lo <= k && k < hi) else {
            return false;
        };
        let Some(lts @ [l, ..]) = ids.get(s0 + 1 + lo..s0 + 1 + hi) else {
            return false;
        };
        lts.iter().all(|x| x == l) && own.allows(ls, o, *l)
    }

    /// The diagnostics of the checked edges whose source holds a loan
    /// their target does not allow: a universal region of the result its
    /// lifetimes' parameter slots, a slot of a place the caller owns what
    /// [`Gen::takes`] allows, `'static` top-level bindings, and a type
    /// parameter that only forwards captures the parameters' too.
    fn verdicts(&self, sol: &Solution, r: &Returns<'_>) -> Verdicts {
        let (b, ls, pool) = (self.s.b, self.s.ls, self.s.pool);
        // Each slot behind a parameter's top `&mut` is a place the caller owns.
        let mut caller: Vec<(Region, u32, usize)> = Vec::new();
        for l in 0..self.locals {
            if b.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM == 0 {
                continue;
            }
            let (lay, rs) = (ls.of(self.s.params[l as usize]), self.local(l).0);
            if lay.slots.len() != rs.len() {
                continue;
            }
            for &s0 in lay
                .tops
                .iter()
                .filter(|&&s0| is_mut(pool, lay.slots[s0].ty))
            {
                caller.extend((s0 + 1..lay.slots[s0].end).map(|j| (rs[j], l, j)));
            }
        }
        let results: HashMap<Region, &(Option<usize>, Vec<u32>)> =
            self.universal.iter().map(|(k, &u)| (u, k)).collect();
        let allowed = |e: &Edge, l: Loan| {
            let o = self.loans[l as usize];
            let global = matches!(o, Origin::Global(_));
            match e.cat {
                Category::Flow => true,
                Category::Result => match (results[&e.to], r.own) {
                    ((None, ids), Some((own, _))) => ids.iter().any(|&id| own.allows(ls, o, id)),
                    ((Some(sub), ids), _) => (r.closures.iter().find(|c| c.0 == *sub))
                        .is_some_and(|(_, dep)| closure_allows(ls, b, *sub, dep, o, ids)),
                    _ => true,
                },
                Category::CallerPlace if e.to == self.stat => global,
                Category::CallerPlace => {
                    // A call may store what the place's own parameter holds
                    // (`ref.generic.store`).
                    let call = !matches!(b.tags[e.inst as usize], Tag::Store | Tag::Intrinsic);
                    (caller.iter().filter(|c| sol.same(e.to, c.0))).all(|&(_, q, j)| {
                        call && matches!(o, Origin::Param(p, _) if p == q) || self.takes(q, j, o)
                    })
                }
                Category::Forward => global || matches!(o, Origin::Param(..)),
                _ => global,
            }
        };
        let has = |v: u32, o: Origin| self.has(sol, v, o);
        let mut v = Verdicts::default();
        let mut returns = HashSet::default();
        for x in sol.violations(&self.c, allowed) {
            let e = x.edge;
            let bad = sol.value(e.from).iter().filter(|&l| !allowed(&e, l));
            let o = bad.map(|l| self.loans[l as usize]).min().expect("a loan");
            let at = self.sites.get(&(e.from, e.to, e.inst)).copied();
            match e.cat {
                Category::Result if returns.insert(e.inst) => {
                    let at = site(b, &has, e.inst, o, 0);
                    match results[&e.to] {
                        (None, ids) => v.results.push((at, o, *ids.first().unwrap_or(&NO_SOURCE))),
                        (Some(_), _) => v.closures.push((at, o)),
                    }
                }
                Category::Result => {}
                Category::CallerPlace => v.stores.extend(at),
                cat => v
                    .capture
                    .extend(at.map(|at| (at, message(cat, b.tags[e.inst as usize])))),
            }
        }
        v.scope = scoped_uses(b, self.s.cx, r.skip, &|i| self.uses(sol, i));
        v
    }
}

/// The message of a captured function value kept at a destination of
/// category `cat` by an instruction of tag `tag`.
fn message(cat: Category, tag: Tag) -> &'static str {
    match (cat, tag) {
        (Category::Box, _) => BOX_MSG,
        (Category::Provider, _) => PROVIDER_MSG,
        (Category::HostCall, _) => HOST_MSG,
        (Category::StaticParam, _) => STATIC_MSG,
        (Category::FieldCapture, Tag::Store) => STORE_FN_MSG,
        (Category::FieldCapture, Tag::Intrinsic) => ELEM_MSG,
        (Category::FieldCapture, Tag::Call | Tag::Await | Tag::CallDyn | Tag::CallValue)
        | (Category::Forward, _) => FN_PARAM_MSG,
        _ => FIELD_MSG,
    }
}
