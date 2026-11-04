//! Pointer-loan elision (model A of `docs/future-work/POINTER_LOANS.md`,
//! phase 11). A loan only matters when an access through another `*T` can
//! meet it, so a loan whose live range can make no such access is never
//! taken, and a type no kept loan holds needs no access check
//! ([`Module::lent`]).
//!
//! **Summaries.** Each instance's accesses through a `*T`, by the type of
//! the object they reach: the types it writes (a store, an `IsUnique`, a
//! `&mut`, a `*T` lent as `&mut`) and reads (a read, a `&`, a `*T` lent as
//! `&`, and every `*T` crossed on the way to another), with its callees'.
//! A call of a function value or a trait value, an erased body's open
//! call, a suspension point and an instance with no body of its own count
//! as accessing everything.
//!
//! **Per reference.** A shared loan on an object of type `T` is kept when
//! its live range may write a `T` through a `*T`; an exclusive one when it
//! may read or write one. A holder's live range is its whole function with
//! its callees; a call loan's is the instructions between its argument and
//! the call, with the callee's summary. The loan's own `Ref` or argument
//! is left out of its range.
//!
//! **Per type.** A loan on a type that no instruction of the program
//! writes through a `*T` (for an exclusive one, reads or writes), the
//! loan's own instruction aside, is never kept, whatever its range calls.
//! An erased body that accesses an open type through a `*T` counts as
//! accessing every type.

use hd_base::{HashMap, HashSet, InstId};
use hd_mono::{Sub, Target, TargetKind, subst};
use hd_tir::ir::{Body, PlaceRoot, Tag};
use hd_types::{Ty, TyData};

use super::borrow::{Roots, call_params, crossings, place_crossings, pointer_lent, root_ty};
use super::excl::{Bind, binds, call_args, call_info, direct_value};
use crate::Input;
use crate::layout::{Kind, kind, ref_mutable};
use crate::module::Module;

/// The key of a kept loan: the instance, the instruction that takes it (a
/// `Ref`, or the read of a `*T` lent as a reference), and the index of
/// the dereference in its place it lies at (`u32::MAX` for a lent `*T`).
pub(crate) type LoanSite = (InstId, u32, u32);

/// One instruction's own accesses through a `*T`.
#[derive(Default, Clone)]
struct Acc {
    w: Vec<Ty>,
    r: Vec<Ty>,
    any: bool,
    /// An access through a `*T` of an open type, which may be any type.
    open: bool,
    callees: Vec<InstId>,
}

/// What a range may access through a `*T`.
#[derive(Default, Clone, PartialEq, Eq)]
struct Eff {
    w: HashSet<Ty>,
    r: HashSet<Ty>,
    any: bool,
}

impl Eff {
    fn add(&mut self, other: &Eff) -> bool {
        let before = (self.w.len(), self.r.len(), self.any);
        self.w.extend(other.w.iter().copied());
        self.r.extend(other.r.iter().copied());
        self.any |= other.any;
        before != (self.w.len(), self.r.len(), self.any)
    }

    /// Whether a loan on `t`, exclusive when `excl`, may meet an access.
    fn meets(&self, t: Ty, excl: bool) -> bool {
        self.any || self.w.contains(&t) || (excl && self.r.contains(&t))
    }
}

/// The instances a call target names.
fn callees(m: &Module, t: &Target) -> Option<Vec<hd_base::Hash128>> {
    let direct = |c: &hd_mono::CallTarget| match c.kind {
        TargetKind::Instance => Some(vec![c.key]),
        TargetKind::Intrinsic(_) | TargetKind::Builtin { .. } => Some(Vec::new()),
    };
    let _ = m;
    match t {
        Target::Call(c) | Target::Default { call: c, .. } | Target::MapInline(c) => direct(c),
        Target::MapKey { hash, eq } => {
            let mut out = direct(hash)?;
            out.extend(direct(eq)?);
            Some(out)
        }
        Target::KnownClosure(k) => Some(vec![*k]),
        Target::Dyn | Target::Open(_) => None,
        _ => Some(Vec::new()),
    }
}

/// Whether type `t` names a type parameter: an erased body's open type.
fn open(pool: &hd_types::InternPool, t: Ty) -> bool {
    matches!(pool.get(t), TyData::Param(..)) || hd_mono::is_open(pool, t)
}

/// The accesses of each instruction of instance `id`'s body `b`.
fn accesses(input: &Input<'_>, m: &Module, id: InstId, b: &Body) -> HashMap<u32, Acc> {
    let (pool, env) = (input.pool, input.env);
    let t = &input.collected.table;
    let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
    let sub = |ty| subst(pool, env, item, args, ty);
    let calls = &input.collected.calls[id.idx()];
    let shared = super::closure::shared_locals(b);
    let pointers = super::borrow::pointer_locals(pool, env, b, &sub);
    let roots = Roots {
        shared: &shared,
        pointers: &pointers,
        objects: &m.objects,
        env,
    };
    let mut out: HashMap<u32, Acc> = HashMap::default();
    for i in 0..b.tags.len() {
        let iu = u32::try_from(i).expect("insts");
        match b.tags[i] {
            Tag::Store | Tag::IsUnique | Tag::Ref | Tag::Read => {
                let Some(p) = b.decode_place(b.data[i][0]) else {
                    continue;
                };
                if !matches!(
                    p.root,
                    PlaceRoot::Local(_) | PlaceRoot::Value(_) | PlaceRoot::Global(..)
                ) {
                    continue;
                }
                let Some(root) = root_ty(b, &sub, p.root) else {
                    continue;
                };
                let write = match b.tags[i] {
                    Tag::Store | Tag::IsUnique => true,
                    Tag::Ref => ref_mutable(pool, env, sub(b.ty[i])),
                    _ => false,
                };
                let crossed = place_crossings(pool, (b, &sub), &roots, root, &p);
                let last = crossed.len().wrapping_sub(1);
                let acc = out.entry(iu).or_default();
                for (n, (_, object)) in crossed.into_iter().enumerate() {
                    acc.any |= open(pool, object);
                    acc.open |= open(pool, object);
                    if write && n == last {
                        acc.w.push(object);
                    } else {
                        acc.r.push(object);
                    }
                }
            }
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace => {
                out.entry(iu).or_default().any = true;
            }
            _ => {}
        }
        let target = calls.get(&iu);
        let is_call = matches!(
            b.tags[i],
            Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::DefaultCall
        );
        if is_call || target.is_some_and(|x| matches!(x, Target::Default { .. })) {
            match target.map(|x| callees(m, x)) {
                Some(Some(keys)) => {
                    for k in keys {
                        match m.inst_of.get(&k) {
                            Some(&c) if matches!(t.sub[c.idx()], Sub::Body(_)) => {
                                out.entry(iu).or_default().callees.push(c);
                            }
                            _ => out.entry(iu).or_default().any = true,
                        }
                    }
                }
                _ => out.entry(iu).or_default().any = true,
            }
        }
        // A `*T` that a call lends as a local reference is accessed where
        // it is read.
        if let Some(receiver) = call_info(input, b, i, target) {
            let params = call_params(input, m, target);
            for (k, a) in call_args(b, i).into_iter().enumerate() {
                let Some(v) = direct_value(b, a) else {
                    continue;
                };
                if b.tags[v] != Tag::Read {
                    continue;
                }
                let value = sub(b.ty[v]);
                let lent = pointer_lent(pool, env, params.get(k).copied(), value, k, receiver);
                if let (Some((mutable, _)), Kind::Ref { target, .. }) =
                    (lent, kind(pool, env, value))
                {
                    let acc = out.entry(u32::try_from(v).expect("insts")).or_default();
                    acc.any |= open(pool, target);
                    acc.open |= open(pool, target);
                    if mutable {
                        acc.w.push(target);
                    } else {
                        acc.r.push(target);
                    }
                }
            }
        }
    }
    out
}

/// The program's loan decisions: the sites whose loans are kept, and the
/// types they hold ([`Module::lent`]).
pub(crate) fn decide(input: &Input<'_>, m: &mut Module) {
    let t = &input.collected.table;
    let (pool, env) = (input.pool, input.env);
    // Each body's accesses, and the program's per-type counts.
    let mut accs: HashMap<InstId, HashMap<u32, Acc>> = HashMap::default();
    let mut count_w: HashMap<Ty, u32> = HashMap::default();
    let mut count_r: HashMap<Ty, u32> = HashMap::default();
    let mut every = false;
    for id in input.order {
        let Some(b) = (input.body)(*id) else {
            continue;
        };
        let erased = matches!(t.sub[id.idx()], Sub::Erased);
        if !matches!(t.sub[id.idx()], Sub::Body(_)) && !erased {
            continue;
        }
        let a = accesses(input, m, *id, b);
        for acc in a.values() {
            for x in &acc.w {
                *count_w.entry(*x).or_default() += 1;
            }
            for x in &acc.r {
                *count_r.entry(*x).or_default() += 1;
            }
            // Only an open access is unknown program-wide: a call's
            // callees are counted at their own bodies.
            every |= acc.open || erased && acc.any && (!acc.w.is_empty() || !acc.r.is_empty());
        }
        if !erased {
            accs.insert(*id, a);
        }
    }
    // Each instance's summary, with its callees', to a fixed point.
    let mut sums: HashMap<InstId, Eff> = HashMap::default();
    for (id, a) in &accs {
        let mut e = Eff::default();
        for acc in a.values() {
            e.w.extend(acc.w.iter().copied());
            e.r.extend(acc.r.iter().copied());
            e.any |= acc.any;
        }
        sums.insert(*id, e);
    }
    let ids: Vec<InstId> = input
        .order
        .iter()
        .copied()
        .filter(|i| accs.contains_key(i))
        .collect();
    loop {
        let mut grew = false;
        for id in &ids {
            let callees: Vec<InstId> = accs[id].values().flat_map(|a| a.callees.clone()).collect();
            let mut e = sums[id].clone();
            for c in callees {
                match sums.get(&c) {
                    Some(ce) => {
                        let ce = ce.clone();
                        grew |= e.add(&ce);
                    }
                    None => grew |= !std::mem::replace(&mut e.any, true),
                }
            }
            sums.insert(*id, e);
        }
        if !grew {
            break;
        }
    }
    // The effects of the instructions of a range, the loan's own aside.
    let range = |id: InstId, from: u32, to: u32, own: &[u32]| -> Eff {
        let mut e = Eff::default();
        for (&j, acc) in &accs[&id] {
            if own.contains(&j) || j < from || j >= to {
                continue;
            }
            e.w.extend(acc.w.iter().copied());
            e.r.extend(acc.r.iter().copied());
            e.any |= acc.any;
            for c in &acc.callees {
                match sums.get(c) {
                    Some(ce) => {
                        e.add(ce);
                    }
                    None => e.any = true,
                }
            }
        }
        e
    };
    // Whether the program accesses `t` through a `*T` beyond `own`'s.
    let program = |t: Ty, excl: bool, own: Option<&Acc>| -> bool {
        let less =
            |list: Option<&Vec<Ty>>| list.map_or(0, |l| l.iter().filter(|x| **x == t).count());
        let w = count_w.get(&t).copied().unwrap_or(0) as usize - less(own.map(|a| &a.w));
        let r = count_r.get(&t).copied().unwrap_or(0) as usize - less(own.map(|a| &a.r));
        every || w > 0 || (excl && r > 0)
    };
    let mut kept: Vec<(LoanSite, Ty, bool)> = Vec::new();
    let objects = m.objects.clone();
    for id in &ids {
        let b = (input.body)(*id).expect("body");
        let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
        let sub = |ty| subst(pool, env, item, args, ty);
        let n = u32::try_from(b.tags.len()).expect("insts");
        let shared = super::closure::shared_locals(b);
        let pointers = super::borrow::pointer_locals(pool, env, b, &sub);
        let roots = Roots {
            shared: &shared,
            pointers: &pointers,
            objects: &objects,
            env,
        };
        // A holder's loans: its whole function.
        for (_, _, bind, _) in binds(pool, env, b, &sub, true) {
            let Bind::Begin(a) = bind else {
                continue;
            };
            let Some(v) = direct_value(b, a).filter(|&v| b.tags[v] == Tag::Ref) else {
                continue;
            };
            let vu = u32::try_from(v).expect("insts");
            let e = range(*id, 0, n, &[vu]);
            ref_sites(input, b, &sub, *id, vu, &roots, |site, object, excl| {
                if program(object, excl, accs[id].get(&vu)) && e.meets(object, excl) {
                    kept.push((site, object, excl));
                }
            });
        }
        // A call's result's loans: its whole function (phase 5).
        for (i, held, bind, _) in binds(pool, env, b, &sub, true) {
            let Bind::Call(a) = bind else {
                continue;
            };
            let iu = u32::try_from(i).expect("insts");
            let call = b.held_args.iter().find(|x| x.1 == a).map(|x| x.0 as usize);
            let (Some(c), Some(v), Some(s)) =
                (call, direct_value(b, a), direct_value(b, b.data[i][1]))
            else {
                continue;
            };
            let (cu, vu) = (
                u32::try_from(c).expect("insts"),
                u32::try_from(v).expect("insts"),
            );
            if !b.held_args.contains(&(cu, a)) {
                continue;
            }
            let excl = [c, s].iter().any(|&x| ref_mutable(pool, env, sub(b.ty[x])))
                || super::excl::held_mutable(pool, env, b, &sub, held);
            let mut heads: Vec<(u32, Ty)> = Vec::new();
            match b.tags[v] {
                Tag::Ref => {
                    let Some(p) = b.decode_place(b.data[v][0]) else {
                        continue;
                    };
                    let Some(root) = root_ty(b, &sub, p.root) else {
                        continue;
                    };
                    for (k, t) in place_crossings(pool, (b, &sub), &roots, root, &p) {
                        heads.push((u32::try_from(k).expect("steps"), t));
                    }
                }
                Tag::Read => {
                    if let Kind::Ref {
                        target,
                        whole: true,
                        fat: false,
                    } = kind(pool, env, sub(b.ty[v]))
                    {
                        heads.push((u32::MAX, target));
                    }
                }
                _ => {}
            }
            if heads.is_empty() {
                continue;
            }
            let k_arg = call_args(b, c).iter().position(|&x| x == a);
            let inner = match (k_arg, accs[id].get(&cu).map(|x| &x.callees[..])) {
                (Some(k), Some([callee])) => returned_inner(input, *callee, k),
                _ => Vec::new(),
            };
            // The call and its argument make the loan; neither is in its range.
            let e = range(*id, 0, n, &[vu, cu]);
            let last = heads.len() - 1;
            for (j, (k, t)) in heads.iter().enumerate() {
                let x = excl && inner.is_empty() && j == last;
                if program(*t, x, accs[id].get(&vu)) && e.meets(*t, x) {
                    kept.push(((*id, iu, *k), *t, x));
                }
            }
            if !inner.is_empty() {
                m.result_inner.insert((*id, iu));
                if inner
                    .iter()
                    .any(|t| program(*t, excl, None) && e.meets(*t, excl))
                {
                    for t in &inner {
                        kept.push(((*id, iu, u32::MAX - 1), *t, excl));
                    }
                }
            }
        }
        // Call loans: from the argument to the call, and the callee.
        for i in 0..b.tags.len() {
            let iu = u32::try_from(i).expect("insts");
            let target = input.collected.calls[id.idx()].get(&iu);
            let Some(receiver) = call_info(input, b, i, target) else {
                continue;
            };
            let params = call_params(input, m, target);
            let mut callee = Eff::default();
            for c in accs[id].get(&iu).map_or(&[][..], |a| &a.callees[..]) {
                match sums.get(c) {
                    Some(ce) => {
                        callee.add(ce);
                    }
                    None => callee.any = true,
                }
            }
            callee.any |= accs[id].get(&iu).is_some_and(|a| a.any);
            for (k, a) in call_args(b, i).into_iter().enumerate() {
                let Some(v) = direct_value(b, a) else {
                    continue;
                };
                let vu = u32::try_from(v).expect("insts");
                let mut e = range(*id, vu + 1, iu, &[vu]);
                e.add(&callee);
                match b.tags[v] {
                    Tag::Ref => {
                        ref_sites(input, b, &sub, *id, vu, &roots, |site, object, excl| {
                            if program(object, excl, accs[id].get(&vu)) && e.meets(object, excl) {
                                kept.push((site, object, excl));
                            }
                        });
                    }
                    Tag::Read => {
                        let value = sub(b.ty[v]);
                        if let (Some((mutable, _)), Kind::Ref { target, .. }) = (
                            pointer_lent(pool, env, params.get(k).copied(), value, k, receiver),
                            kind(pool, env, value),
                        ) && program(target, mutable, accs[id].get(&vu))
                            && e.meets(target, mutable)
                        {
                            kept.push(((*id, vu, u32::MAX), target, mutable));
                        }
                    }
                    _ => {
                        // A `Ref` the argument carries through calls'
                        // results: its loan runs from it to this call.
                        for r in super::excl::carried_refs(pool, env, b, &sub, v) {
                            let mut e = range(*id, r + 1, iu, &[r]);
                            e.add(&callee);
                            ref_sites(input, b, &sub, *id, r, &roots, |site, object, excl| {
                                if program(object, excl, accs[id].get(&r)) && e.meets(object, excl)
                                {
                                    kept.push((site, object, excl));
                                }
                            });
                        }
                    }
                }
            }
        }
    }
    for (site, object, excl) in kept {
        m.kept.insert(site);
        *m.lent.entry(object).or_insert(false) |= excl;
    }
}

/// Calls `f` with each loan `Ref` `v` takes: its site, the object's type,
/// and whether the loan is exclusive (the last object of a `&mut`).
fn ref_sites(
    input: &Input<'_>,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    id: InstId,
    v: u32,
    roots: &Roots<'_>,
    mut f: impl FnMut(LoanSite, Ty, bool),
) {
    let (pool, env) = (input.pool, input.env);
    let Some(p) = b.decode_place(b.data[v as usize][0]) else {
        return;
    };
    let Some(root) = root_ty(b, sub, p.root) else {
        return;
    };
    let excl = ref_mutable(pool, env, sub(b.ty[v as usize]));
    let crossed = place_crossings(pool, (b, sub), roots, root, &p);
    let last = crossed.len().wrapping_sub(1);
    for (n, (k, object)) in crossed.into_iter().enumerate() {
        f(
            (id, v, u32::try_from(k).expect("steps")),
            object,
            excl && n == last,
        );
    }
}

/// The types of the objects a reference that instance `callee` returns
/// from its parameter `k` may point into, past the argument's own object:
/// the last `*T` each `Ref` rooted at that parameter crosses after it.
fn returned_inner(input: &Input<'_>, callee: InstId, k: usize) -> Vec<Ty> {
    let (pool, env) = (input.pool, input.env);
    let t = &input.collected.table;
    let Some(b) = (input.body)(callee) else {
        return Vec::new();
    };
    let Some(&l) = super::param_locals(b).get(k) else {
        return Vec::new();
    };
    let (item, args) = (t.item[callee.idx()], t.args[callee.idx()]);
    let sub = |ty| subst(pool, env, item, args, ty);
    let param = sub(b.local_ty[l]);
    let counted = matches!(
        kind(pool, env, param),
        Kind::Ref {
            whole: true,
            fat: false,
            ..
        }
    );
    let mut out = Vec::new();
    for i in 0..b.tags.len() {
        if b.tags[i] != Tag::Ref {
            continue;
        }
        let Some(p) = b.decode_place(b.data[i][0]) else {
            continue;
        };
        if !matches!(p.root, PlaceRoot::Local(x) if x.idx() == l) {
            continue;
        }
        let inner: Vec<Ty> = crossings(pool, env, param, &p.proj)
            .into_iter()
            .filter(|&(j, _)| !(counted && j == 0))
            .map(|x| x.1)
            .collect();
        if let Some(&last) = inner.last()
            && !out.contains(&last)
        {
            out.push(last);
        }
    }
    out
}
