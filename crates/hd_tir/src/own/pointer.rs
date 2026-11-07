//! Whole-program pointer-loan decisions, after instance and layout resolution.
//!
//! This is a link-time result table, not loan begin/end statements in TIR.
//! The context supplies program facts; this pass alone computes source-deduplicated
//! accesses, transitive call effects, trait payload types and kept loan sites.

use crate::ir::{Body, Place, PlaceRoot, Proj, Ref, Tag, Word, slots};
use hd_base::{DefId, HashMap, HashSet, InstId, NodeIdx, SliceSort as _};
use hd_types::{Ty, TyList};

/// An instruction's instance, its loan origin, and the crossed dereference.
/// `u32::MAX` denotes a whole pointer; `u32::MAX - 1` a result's inner object.
pub type LoanSite = (InstId, u32, u32);
type Source = (DefId, TyList, u16, NodeIdx, u32);

/// Resolved reference layout, with the declaration's mutability.
#[derive(Clone, Copy)]
pub struct Reference {
    pub target: Ty,
    pub whole: bool,
    pub fat: bool,
    pub mutable: bool,
}

/// Resolved call edges. A missing edge has no ordinary TIR body.
pub struct Call {
    pub default: bool,
    pub callees: Option<Vec<Option<InstId>>>,
}

/// A holder's structural source, before deciding whether to take a loan.
#[derive(Clone, Copy)]
pub enum Binding {
    Reference(u32),
    CallArgument(u32),
    Other,
}

/// The store, holder slot and source of a reference binding.
pub type Holder = (usize, (usize, usize), Binding);

/// One reachable trait conversion and its vtable's trait/supertrait order.
pub struct Conversion {
    pub from: Ty,
    pub traits: Vec<DefId>,
}

/// Read-only whole-program facts. None of the four answer tables is an input.
pub trait Context {
    fn order(&self) -> &[InstId];
    fn body(&self, id: InstId) -> Option<&Body>;
    fn source(&self, id: InstId) -> Option<(DefId, TyList, u16)>;
    fn substitute(&self, id: InstId, ty: Ty) -> Ty;
    fn reference(&self, ty: Ty) -> Option<Reference>;
    fn strip(&self, ty: Ty) -> Ty;
    fn place_crossings(&self, id: InstId, place: &Place) -> Option<Vec<(usize, Ty)>>;
    fn type_crossings(&self, root: Ty, steps: &[Proj]) -> Vec<(usize, Ty)>;
    fn call(&self, id: InstId, instruction: u32) -> Option<Call>;
    #[allow(clippy::option_option)]
    fn receiver(&self, id: InstId, instruction: usize) -> Option<Option<bool>>;
    fn params(&self, id: InstId, instruction: u32) -> Vec<Ty>;
    fn dyn_receiver(&self, id: InstId, value: usize) -> Option<DefId>;
    fn bindings(&self, id: InstId) -> Vec<Holder>;
    fn held_mutable(&self, id: InstId, held: (usize, usize)) -> bool;
    fn carried_refs(&self, id: InstId, value: usize) -> Vec<u32>;
    fn param_locals(&self, id: InstId) -> Vec<usize>;
    fn conversions(&self) -> Vec<Conversion>;
}

/// Link-time answers read by the emitter. No loan policy lives in the reader.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Decisions {
    pub kept: HashSet<LoanSite>,
    pub lent: HashMap<Ty, bool>,
    pub result_inner: HashSet<(InstId, u32)>,
    pub dyn_types: HashMap<DefId, Vec<Ty>>,
}

/// One instruction's own accesses through a counted pointer.
#[derive(Default, Clone)]
struct Acc {
    w: Vec<Ty>,
    r: Vec<Ty>,
    any: bool,
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

/// The accesses of each instruction, from TIR operations and resolved facts.
fn accesses(
    cx: &impl Context,
    types: &HashMap<DefId, Vec<Ty>>,
    id: InstId,
    b: &Body,
) -> HashMap<u32, Acc> {
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
                let Some(crossed) = cx.place_crossings(id, &p) else {
                    continue;
                };
                let write = match b.tags[i] {
                    Tag::Store | Tag::IsUnique => true,
                    Tag::Ref => mutable(cx, cx.substitute(id, b.ty[i])),
                    _ => false,
                };
                let last = crossed.len().wrapping_sub(1);
                let acc = out.entry(iu).or_default();
                for (n, (_, object)) in crossed.into_iter().enumerate() {
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
        let target = cx.call(id, iu);
        let is_call = matches!(
            b.tags[i],
            Tag::Call | Tag::CallDyn | Tag::CallValue | Tag::DefaultCall
        );
        if is_call || target.as_ref().is_some_and(|c| c.default) {
            match target.and_then(|c| c.callees) {
                Some(edges) => {
                    for edge in edges {
                        match edge {
                            Some(c) => out.entry(iu).or_default().callees.push(c),
                            None => out.entry(iu).or_default().any = true,
                        }
                    }
                }
                None => out.entry(iu).or_default().any = true,
            }
        }
        // A pointer lent to a local-reference parameter is accessed at its read.
        if let Some(receiver) = cx.receiver(id, i) {
            let params = cx.params(id, iu);
            for (k, a) in call_args(b, i).into_iter().enumerate() {
                let Some(v) = direct_value(b, a) else {
                    continue;
                };
                if b.tags[v] != Tag::Read {
                    continue;
                }
                for (mutable, target) in
                    lent_objects(cx, types, id, b, params.get(k), v, k, receiver)
                {
                    let acc = out.entry(u32::try_from(v).expect("insts")).or_default();
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

/// Compute the complete link-time loan table, preserving content/source order.
#[must_use]
pub fn decide(cx: &impl Context) -> Decisions {
    let mut out = Decisions {
        dyn_types: dyn_types(cx),
        ..Decisions::default()
    };
    // Each body's accesses, and the program's per-type counts.
    let mut accs: HashMap<InstId, HashMap<u32, Acc>> = HashMap::default();
    // An access is counted once per source instruction: the copies of a
    // body (its exclusive copy, #1118) run the same instruction, which
    // counts as the loan's own in each of them, as a recursive run of it
    // does. Its key is the instance's item, type arguments and sub-body,
    // and the instruction's source node with its rank among the body's
    // accesses at that node.
    let mut seen: HashSet<(Ty, bool, Source)> = HashSet::default();
    let mut count_w: HashMap<Ty, u32> = HashMap::default();
    let mut count_r: HashMap<Ty, u32> = HashMap::default();
    for id in cx.order() {
        let Some(b) = cx.body(*id) else {
            continue;
        };
        let Some((item, args, sub_body)) = cx.source(*id) else {
            continue;
        };
        let a = accesses(cx, &out.dyn_types, *id, b);
        let mut at: Vec<u32> = a.keys().copied().collect();
        at.sort_stable();
        let mut rank: HashMap<hd_base::NodeIdx, u32> = HashMap::default();
        for j in at {
            let acc = &a[&j];
            let node = b.syn[j as usize];
            let k = rank.entry(node).or_default();
            let source = (item, args, sub_body, node, *k);
            *k += 1;
            for x in &acc.w {
                if seen.insert((*x, true, source)) {
                    *count_w.entry(*x).or_default() += 1;
                }
            }
            for x in &acc.r {
                if seen.insert((*x, false, source)) {
                    *count_r.entry(*x).or_default() += 1;
                }
            }
        }
        accs.insert(*id, a);
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
    let ids: Vec<InstId> = cx
        .order()
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
        let less = |list: Option<&Vec<Ty>>| usize::from(list.is_some_and(|l| l.contains(&t)));
        let w = count_w.get(&t).copied().unwrap_or(0) as usize - less(own.map(|a| &a.w));
        let r = count_r.get(&t).copied().unwrap_or(0) as usize - less(own.map(|a| &a.r));
        w > 0 || (excl && r > 0)
    };
    let mut kept: Vec<(LoanSite, Ty, bool)> = Vec::new();
    for id in &ids {
        let b = cx.body(*id).expect("body");
        let sub = |ty| cx.substitute(*id, ty);
        let n = u32::try_from(b.tags.len()).expect("insts");
        // A holder's loans: its whole function.
        for (_, _, bind) in cx.bindings(*id) {
            let Binding::Reference(a) = bind else {
                continue;
            };
            let Some(v) = direct_value(b, a).filter(|&v| b.tags[v] == Tag::Ref) else {
                continue;
            };
            let vu = u32::try_from(v).expect("insts");
            let e = range(*id, 0, n, &[vu]);
            ref_sites(cx, b, *id, vu, |site, object, excl| {
                if program(object, excl, accs[id].get(&vu)) && e.meets(object, excl) {
                    kept.push((site, object, excl));
                }
            });
        }
        // A call's result's loans: its whole function (phase 5).
        for (i, held, bind) in cx.bindings(*id) {
            let Binding::CallArgument(a) = bind else {
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
            let excl =
                [c, s].iter().any(|&x| mutable(cx, sub(b.ty[x]))) || cx.held_mutable(*id, held);
            let mut heads: Vec<(u32, Ty)> = Vec::new();
            match b.tags[v] {
                Tag::Ref => {
                    let Some(p) = b.decode_place(b.data[v][0]) else {
                        continue;
                    };
                    let Some(crossed) = cx.place_crossings(*id, &p) else {
                        continue;
                    };
                    for (k, t) in crossed {
                        heads.push((u32::try_from(k).expect("steps"), t));
                    }
                }
                Tag::Read => {
                    if let Some(Reference {
                        target,
                        whole: true,
                        fat: false,
                        ..
                    }) = cx.reference(sub(b.ty[v]))
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
                (Some(k), Some([callee])) => returned_inner(cx, *callee, k),
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
                out.result_inner.insert((*id, iu));
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
            let Some(receiver) = cx.receiver(*id, i) else {
                continue;
            };
            let params = cx.params(*id, iu);
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
                        ref_sites(cx, b, *id, vu, |site, object, excl| {
                            if program(object, excl, accs[id].get(&vu)) && e.meets(object, excl) {
                                kept.push((site, object, excl));
                            }
                        });
                    }
                    Tag::Read => {
                        let lent =
                            lent_objects(cx, &out.dyn_types, *id, b, params.get(k), v, k, receiver);
                        for (mutable, target) in lent {
                            if program(target, mutable, accs[id].get(&vu))
                                && e.meets(target, mutable)
                            {
                                kept.push(((*id, vu, u32::MAX), target, mutable));
                            }
                        }
                    }
                    _ => {
                        // A `Ref` the argument carries through calls'
                        // results: its loan runs from it to this call.
                        for r in cx.carried_refs(*id, v) {
                            let mut e = range(*id, r + 1, iu, &[r]);
                            e.add(&callee);
                            ref_sites(cx, b, *id, r, |site, object, excl| {
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
        out.kept.insert(site);
        *out.lent.entry(object).or_insert(false) |= excl;
    }
    out
}

fn mutable(cx: &impl Context, ty: Ty) -> bool {
    cx.reference(ty).is_some_and(|r| r.mutable)
}

/// Objects a whole-pointer argument lends as a local reference, including dyn
/// receiver payloads. The trait payload table is computed by this pass.
#[allow(clippy::too_many_arguments)]
fn lent_objects(
    cx: &impl Context,
    types: &HashMap<DefId, Vec<Ty>>,
    id: InstId,
    b: &Body,
    param: Option<&Ty>,
    v: usize,
    k: usize,
    receiver: Option<bool>,
) -> Vec<(bool, Ty)> {
    let value = cx.substitute(id, b.ty[v]);
    if let Some(Reference {
        target,
        whole: true,
        fat: false,
        ..
    }) = cx.reference(value)
    {
        if let (0, Some(mutable)) = (k, receiver) {
            return vec![(mutable, target)];
        }
        if let Some(Reference {
            whole: false,
            fat: false,
            mutable,
            ..
        }) = param.and_then(|t| cx.reference(*t))
        {
            return vec![(mutable, target)];
        }
    }
    match (k, receiver, cx.dyn_receiver(id, v)) {
        (0, Some(mutable), Some(tr)) => types
            .get(&tr)
            .into_iter()
            .flatten()
            .map(|&t| (mutable, t))
            .collect(),
        _ => Vec::new(),
    }
}

/// Class-hierarchy payload types, preserving conversion/parent encounter order.
fn dyn_types(cx: &impl Context) -> HashMap<DefId, Vec<Ty>> {
    let mut out: HashMap<DefId, Vec<Ty>> = HashMap::default();
    for conversion in cx.conversions() {
        let object = cx
            .reference(conversion.from)
            .map_or(conversion.from, |r| r.target);
        let object = cx.strip(object);
        for tr in conversion.traits {
            let types = out.entry(tr).or_default();
            if !types.contains(&object) {
                types.push(object);
            }
        }
    }
    out
}

/// A reference's crossed objects; only the last can take an exclusive loan.
fn ref_sites(
    cx: &impl Context,
    b: &Body,
    id: InstId,
    v: u32,
    mut f: impl FnMut(LoanSite, Ty, bool),
) {
    let Some(p) = b.decode_place(b.data[v as usize][0]) else {
        return;
    };
    let Some(crossed) = cx.place_crossings(id, &p) else {
        return;
    };
    let exclusive = mutable(cx, cx.substitute(id, b.ty[v as usize]));
    let last = crossed.len().wrapping_sub(1);
    for (n, (k, object)) in crossed.into_iter().enumerate() {
        f(
            (id, v, u32::try_from(k).expect("steps")),
            object,
            exclusive && n == last,
        );
    }
}

/// Objects reached past an argument's own counted object by its returned refs.
fn returned_inner(cx: &impl Context, callee: InstId, k: usize) -> Vec<Ty> {
    let Some(b) = cx.body(callee) else {
        return Vec::new();
    };
    let Some(&l) = cx.param_locals(callee).get(k) else {
        return Vec::new();
    };
    let param = cx.substitute(callee, b.local_ty[l]);
    let counted = cx.reference(param).is_some_and(|r| r.whole && !r.fat);
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
        let inner: Vec<Ty> = cx
            .type_crossings(param, &p.proj)
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

/// The instruction behind representation-preserving coercions.
fn direct_value(b: &Body, a: u32) -> Option<usize> {
    let mut v = Ref(a).as_inst()?.0 as usize;
    while b.same_value_coercion(v) {
        v = Ref(b.data[v][0]).as_inst()?.0 as usize;
    }
    Some(v)
}

fn call_args(b: &Body, i: usize) -> Vec<u32> {
    if b.tags[i] == Tag::CallValue {
        return b.record(b.data[i][1]).to_vec();
    }
    let mut args = Vec::new();
    slots(b, i, |_, v, word| {
        if matches!(word, Word::Value(_)) && v < u32::try_from(i).expect("insts") {
            args.push(v);
        }
    });
    args
}
