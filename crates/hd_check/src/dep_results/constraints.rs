//! Outlives constraints for places, stores, constructions, control flow,
//! calls and closures (`docs/future-work/LIFETIME_SOLVER.md`, slices S3
//! and S4), solved beside [`Solve`] in shadow: with `HD_REGION_SHADOW`
//! set, [`shadow`] prints each instruction's top slots where the two
//! disagree.

use std::sync::LazyLock;

use hd_base::{HashMap, SliceSort as _};
use hd_tir::ir::{IntrinsicOp, PlaceRoot, Proj, Role, Tag, Word, slots};
use hd_types::{Ty, TyData};

use super::{
    ANY, ANY_FN, DepSig, Layout, Origin, Set, Solve, arg_ty, capture_slot, intrinsic_ops,
    local_ref, referent, source_arg, w,
};
use crate::local_refs::holds_local_ref;
use crate::regions::{Category, Constraints, Loan, Region, RegionTable};

/// Whether `HD_REGION_SHADOW` asks for the shadow comparison.
pub(super) static SHADOW: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("HD_REGION_SHADOW").is_some());

/// A value's or a place's region variables, one per slot of its type.
type View = (Vec<Region>, Ty);

/// A view's top slots, and the slots behind them.
type Split = (Vec<Region>, Vec<Region>);

/// What a place lies in, and its slots ([`Gen::place`]).
type Lies = (Option<Result<Origin, u32>>, Option<View>);

struct Gen<'s, 'a> {
    s: &'s Solve<'a>,
    t: RegionTable,
    c: Constraints,
    /// Each loan's origin: loan `l` is `loans[l]`, placeholders first.
    loans: Vec<Origin>,
    ids: HashMap<Origin, Loan>,
    /// The key of instruction `i` is `locals + i`.
    locals: u32,
    at: u32,
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

    /// The place `word`: what it lies in, as `Solve::place` finds it (a
    /// loan, or a reference value it reborrows from), and its
    /// slots past its first `limit` projections none
    /// (`ref.dep.origin.place`), none for a top-level binding's and through
    /// a counted reference.
    fn place(&mut self, i: u32, word: u32, limit: usize, dir: (bool, bool)) -> Lies {
        let b = self.s.b;
        let Some(p) = b.decode_place(word) else {
            return (None, None);
        };
        let (first, read) = (p.proj.first().copied(), |v: u32| b.data[v as usize][0]);
        let (at, root) = match p.root {
            PlaceRoot::Local(l) => (Some(Ok(Origin::Local(l.raw()))), Some(self.local(l.raw()))),
            PlaceRoot::Value(v) if matches!(first, Some(Proj::Elem(_))) => {
                match self.read_of(v.0) {
                    Some(r) => self.place(r, read(r), usize::MAX, dir),
                    None => (Some(Ok(Origin::Hidden(i))), self.val(v.0)),
                }
            }
            PlaceRoot::Value(v) if first == Some(Proj::Deref) => {
                match self.s.ty(v.0).and_then(|t| local_ref(self.s.pool, t)) {
                    Some((false, _)) => {
                        let view = self.val(v.0);
                        (view.as_ref().map(|_| Err(v.0)), view)
                    }
                    _ if b.tags.get(v.0 as usize) == Some(&Tag::Read) => {
                        (self.place(v.0, read(v.0), 0, (false, false)).0, None)
                    }
                    _ => (Some(Ok(Origin::Hidden(v.0))), None),
                }
            }
            PlaceRoot::Value(v) => (Some(Ok(Origin::Hidden(i))), self.val(v.0)),
            PlaceRoot::Global(d, _) => (Some(Ok(Origin::Global(d.raw()))), None),
            _ => (None, None),
        };
        let proj = &p.proj[..limit.min(p.proj.len())];
        (at, root.map(|r| self.project(r, proj, dir)))
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
    /// With `result`, each result slot takes its lifetime's variable.
    fn call(&mut self, sig: &DepSig, me: &View, result: bool) {
        let (b, pool, ls, iu) = (self.s.b, self.s.pool, self.s.ls, self.at as usize);
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
                for id in ids {
                    let r = var(&mut self.t, id);
                    self.lend(a, r);
                }
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
                self.each_into(&v.0, &rs);
                continue;
            }
            for (k, &id) in ids.iter().enumerate() {
                let r = var(&mut self.t, id);
                if under_mut(pool, &lay, k) {
                    self.c.equal(v.0[k], r, self.at);
                } else {
                    self.flow(v.0[k], r);
                }
                for j in k + 1..lay.slots[k].end {
                    self.flow(v.0[j], r);
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
                let (at, place) = self.place(i, a, usize::MAX, (true, false));
                self.lies_in(at, me.0[0]);
                let inner = (me.0[1..].to_vec(), referent(pool, me.1));
                // The referent holds what its storage holds, each slot the
                // union of all of them where the place is a part of it.
                let held = match at {
                    Some(Ok(Origin::Local(l))) => Some(self.local(l)),
                    Some(Err(v)) => self
                        .val(v)
                        .map(|v| self.project(v, &[Proj::Deref], (true, inv))),
                    // The hidden local of `&e` on a value.
                    Some(Ok(Origin::Hidden(h))) if h == i => place.map(|place| {
                        let h = (self.t.fresh(self.width(inner.1)).collect(), inner.1);
                        self.relate(&place, &h, false, false);
                        h
                    }),
                    _ => None,
                };
                if let Some(held) = held {
                    self.relate(&held, &inner, inv, false);
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

/// Generates the body's constraints, solves them, and prints each top
/// slot whose loans differ from `s`'s origin set, then one count line.
pub(super) fn shadow(s: &Solve<'_>) {
    let b = s.b;
    let mut g = Gen {
        s,
        t: RegionTable::default(),
        c: Constraints::default(),
        loans: Vec::new(),
        ids: HashMap::default(),
        locals: w(b.local_ty.len()),
        at: 0,
    };
    let widths: Vec<u32> = (b.local_ty.iter().map(|&t| g.width(t)))
        .chain((0..b.tags.len()).map(|i| g.width(b.ty[i]).max(u32::from(b.tags[i] == Tag::Ref))))
        .collect();
    for n in widths {
        g.t.push(n);
    }
    for l in 0..g.locals {
        if b.local_flags[l as usize] & hd_tir::ir::local_flags::PARAM != 0 {
            g.param(l);
        }
    }
    for i in 0..w(b.tags.len()) {
        g.at = i;
        g.step(i);
    }
    let sol = g.c.solve(g.t.len());
    let (mut same, mut differ) = (0, 0);
    for (i, today) in s.val.iter().enumerate() {
        let tops = g.split(&g.val(w(i)).expect("a value")).0;
        for (k, set) in today.iter().enumerate() {
            let r = tops[k];
            let mut new: Set = sol.value(r).iter().map(|l| g.loans[l as usize]).collect();
            new.sort_stable();
            if new == *set {
                same += 1;
            } else {
                differ += 1;
                let (src, at) = (s.cx.src, b.syn[i]);
                let node = (at != hd_base::NodeIdx::NONE).then(|| src.parse.tree.node(at));
                let lo = node.map_or(0, |n| src.parse.tokens.span(src.first(n)).0);
                let line = src.parse.tokens.line_of(lo) + 1;
                let tag = b.tags[i];
                eprintln!(
                    "region-shadow: line {line} %{i} {tag:?} slot {k}: today {set:?} new {new:?}"
                );
            }
        }
    }
    eprintln!("region-shadow-body: {same} same, {differ} differ");
}
