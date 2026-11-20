//! Whole-program pointer-loan decisions, after instance and layout resolution.
//!
//! The checker places each loan as `LoanBegin`/`LoanClose` statements; this
//! link-time pass reads each loan's range from them and keeps the loans an
//! access in range may meet. The context supplies program facts; this pass
//! alone computes source-deduplicated accesses, transitive call effects,
//! trait payload types and kept loan sites.

use crate::ir::{Body, NONE, Place, PlaceRoot, Proj, Tag, loan_word};
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

/// One reachable trait conversion and its vtable's trait/supertrait order.
pub struct Conversion {
    pub from: Ty,
    pub traits: Vec<DefId>,
}

/// Read-only whole-program facts. None of the four answer tables is an input.
pub trait Context {
    fn order(&self) -> &[InstId];
    fn body(&self, id: InstId) -> Option<&Body>;
    /// Original-order instructions belonging to this instance's sub-body.
    fn instructions(&self, id: InstId) -> &[usize];
    fn source(&self, id: InstId) -> Option<(DefId, TyList, u16)>;
    fn reference(&self, ty: Ty) -> Option<Reference>;
    fn strip(&self, ty: Ty) -> Ty;
    fn place_crossings(&self, id: InstId, place: &Place) -> Option<Vec<(usize, Ty)>>;
    fn type_crossings(&self, root: Ty, steps: &[Proj]) -> Vec<(usize, Ty)>;
    fn call(&self, id: InstId, instruction: u32) -> Option<Call>;
    /// Only a `Call` or `CallValue` has call-loan arguments. Its access
    /// row contains only callee effects, never a direct place access.
    #[allow(clippy::option_option)]
    fn receiver(&self, id: InstId, instruction: usize) -> Option<Option<bool>>;
    fn params(&self, id: InstId, instruction: u32) -> Vec<Ty>;
    fn dyn_receiver(&self, id: InstId, value: usize) -> Option<DefId>;
    fn param_locals(&self, id: InstId) -> Vec<usize>;
    fn conversions(&self) -> Vec<Conversion>;
}

/// Link-time answers read by the emitter. No loan policy lives in the reader.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Decisions {
    pub kept: HashSet<LoanSite>,
    /// Per instance and access instruction, the objects its place crosses
    /// (`Context::place_crossings`), where it crosses any: the accesses
    /// the backend checks.
    pub crossed: HashMap<(InstId, u32), Vec<(usize, Ty)>>,
    pub lent: HashMap<Ty, bool>,
    pub dyn_types: HashMap<DefId, Vec<Ty>>,
}

/// One instruction's own accesses through a counted pointer.
#[derive(Default, Clone)]
struct Acc {
    w: Vec<Ty>,
    r: Vec<Ty>,
    any: bool,
    callees: Vec<InstId>,
    crossed: Vec<(usize, Ty)>,
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

/// Sorted positions for each direct effect and for infrequent callees.
/// Frequent callees keep one shared summary and one list of call positions,
/// so a wide summary is not copied into every call site.
#[derive(Default)]
struct EffectIndex<'a> {
    writes: HashMap<Ty, Vec<u32>>,
    reads: HashMap<Ty, Vec<u32>>,
    unknown: Vec<u32>,
    frequent: Vec<(&'a Eff, Vec<u32>)>,
    #[cfg(test)]
    indexed: usize,
    #[cfg(test)]
    probes: std::cell::Cell<usize>,
}

impl<'a> EffectIndex<'a> {
    fn new(accesses: &HashMap<u32, Acc>, summaries: &'a HashMap<InstId, Eff>) -> Self {
        let mut index = Self::default();
        let mut calls: HashMap<InstId, Vec<u32>> = HashMap::default();
        let mut positions: Vec<_> = accesses.keys().copied().collect();
        positions.sort_stable();
        for position in positions {
            let acc = &accesses[&position];
            #[cfg(test)]
            {
                index.indexed += 1 + acc.callees.len();
            }
            if acc.any
                || acc
                    .callees
                    .iter()
                    .any(|callee| summaries.get(callee).is_none_or(|effect| effect.any))
            {
                index.unknown.push(position);
                continue;
            }
            for &ty in &acc.w {
                index.writes.entry(ty).or_default().push(position);
                #[cfg(test)]
                {
                    index.indexed += 1;
                }
            }
            for &ty in &acc.r {
                index.reads.entry(ty).or_default().push(position);
                #[cfg(test)]
                {
                    index.indexed += 1;
                }
            }
            for &callee in &acc.callees {
                let sites = calls.entry(callee).or_default();
                if sites.last() != Some(&position) {
                    sites.push(position);
                }
            }
        }
        // At most sqrt(edges) distinct frequent callees per query. For
        // the rest, each summary entry expands into at most sqrt(edges)
        // positions, independent of how many loans are queried.
        let limit = calls.values().map(Vec::len).sum::<usize>().isqrt();
        for (callee, positions) in calls {
            let effect = &summaries[&callee];
            if positions.len() > limit {
                index.frequent.push((effect, positions));
                continue;
            }
            for &ty in &effect.w {
                index.writes.entry(ty).or_default().extend(&positions);
                #[cfg(test)]
                {
                    index.indexed += positions.len();
                }
            }
            for &ty in &effect.r {
                index.reads.entry(ty).or_default().extend(&positions);
                #[cfg(test)]
                {
                    index.indexed += positions.len();
                }
            }
        }
        for positions in index.writes.values_mut().chain(index.reads.values_mut()) {
            positions.sort_stable();
            positions.dedup();
        }
        index
    }

    #[cfg_attr(
        not(test),
        allow(clippy::unused_self, reason = "counts position probes in tests")
    )]
    fn hits(&self, positions: &[u32], from: u32, to: u32, excluded: &[u32]) -> bool {
        let start = positions.partition_point(|&position| {
            #[cfg(test)]
            self.probes.set(self.probes.get() + 1);
            position < from
        });
        for &position in &positions[start..] {
            #[cfg(test)]
            self.probes.set(self.probes.get() + 1);
            if position >= to {
                return false;
            }
            if !excluded.contains(&position) {
                return true;
            }
        }
        false
    }

    fn meets(&self, from: u32, to: u32, excluded: &[u32], ty: Ty, exclusive: bool) -> bool {
        self.hits(&self.unknown, from, to, excluded)
            || self
                .writes
                .get(&ty)
                .is_some_and(|p| self.hits(p, from, to, excluded))
            || exclusive
                && self
                    .reads
                    .get(&ty)
                    .is_some_and(|p| self.hits(p, from, to, excluded))
            || self.frequent.iter().any(|(effect, positions)| {
                #[cfg(test)]
                self.probes.set(self.probes.get() + 1);
                effect.meets(ty, exclusive) && self.hits(positions, from, to, excluded)
            })
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
    for &i in cx.instructions(id) {
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
                    Tag::Ref => mutable(cx, b.ty[i]),
                    _ => false,
                };
                let last = crossed.len().wrapping_sub(1);
                let acc = out.entry(iu).or_default();
                acc.crossed.clone_from(&crossed);
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
                let Some(v) = b.direct_value(a) else {
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
            if !acc.crossed.is_empty() {
                out.crossed.insert((*id, j), acc.crossed.clone());
            }
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
    // Whether the program accesses `t` through a `*T` beyond `own`'s.
    let program = |t: Ty, excl: bool, own: Option<&Acc>| -> bool {
        let less = |list: Option<&Vec<Ty>>| usize::from(list.is_some_and(|l| l.contains(&t)));
        let w = count_w.get(&t).copied().unwrap_or(0) as usize - less(own.map(|a| &a.w));
        let r = count_r.get(&t).copied().unwrap_or(0) as usize - less(own.map(|a| &a.r));
        w > 0 || (excl && r > 0)
    };
    let mut kept: Vec<(LoanSite, Ty, bool)> = Vec::new();
    let mut topos: HashMap<*const Body, Box<crate::ir::Topology>> = HashMap::default();
    for id in &ids {
        let b = cx.body(*id).expect("body");
        let n = u32::try_from(b.tags.len()).expect("insts");
        let effects = std::cell::OnceCell::new();
        let meets = |from, to, excluded: &[u32], ty, exclusive| {
            effects
                .get_or_init(|| EffectIndex::new(&accs[id], &sums))
                .meets(from, to, excluded, ty, exclusive)
        };
        // Each loan's range: from its site, or from the start of a loop
        // whose next round it may outlive (a close past the loop), to its
        // last `LoanClose`, or the body's end with none. Cost: one pass over
        // the instance's instructions, then each begin's loops.
        // An instance with no loan needs no structure: a closure's sub-body
        // instance reads the whole body's, built once per body.
        if !cx
            .instructions(*id)
            .iter()
            .any(|&i| b.tags[i] == Tag::LoanBegin)
        {
            continue;
        }
        let topo = &**topos
            .entry(std::ptr::from_ref(b))
            .or_insert_with(|| Box::new(crate::ir::Topology::new(b)));
        let mut last_close: HashMap<u32, u32> = HashMap::default();
        // A two-phase site's loan becomes exclusive at its call.
        let mut activated: HashSet<u32> = HashSet::default();
        for &i in cx.instructions(*id) {
            if b.tags[i] == Tag::LoanClose {
                let end = last_close.entry(b.data[i][0]).or_insert(0);
                *end = (*end).max(u32::try_from(i).expect("insts") + 1);
            }
            if b.tags[i] == Tag::LoanBegin && b.data[i][1] & loan_word::ACTIVATE != 0 {
                activated.insert(b.data[i][0]);
            }
        }
        for &i in cx.instructions(*id) {
            if b.tags[i] != Tag::LoanBegin {
                continue;
            }
            let [v, word] = b.data[i];
            if word & loan_word::ACTIVATE != 0 {
                continue;
            }
            let iu = crate::ir::u32_of(i);
            let hi = last_close.get(&iu).copied().unwrap_or(n);
            let mut lo = v;
            let mut l = topo.loop_of[v as usize];
            while l != NONE {
                if hi > l {
                    lo = lo.min(topo.first[l as usize]);
                }
                l = topo.loop_of[l as usize];
            }
            let excl = word & loan_word::EXCL != 0 || activated.contains(&v);
            let own = accs[id].get(&v);
            if word & loan_word::INNER != 0 {
                // The object a call's returned reference points into, past
                // the argument's own: the callee reached it through a pointer.
                let callee = match accs[id].get(&v).map(|x| &x.callees[..]) {
                    Some([callee]) => Some(*callee),
                    _ => None,
                };
                let inner: Vec<Ty> = (callee.into_iter())
                    .flat_map(|c| {
                        (0..call_args(b, v as usize).len())
                            .flat_map(move |k| returned_inner(cx, c, k))
                    })
                    .collect();
                if inner
                    .iter()
                    .any(|&t| program(t, excl, None) && meets(lo, hi, &[v], t, excl))
                {
                    for &t in &inner {
                        kept.push(((*id, v, u32::MAX - 1), t, excl));
                    }
                }
                continue;
            }
            ref_sites(cx, b, *id, v, |site, object, x| {
                if program(object, x, own) && meets(lo, hi, &[v], object, x) {
                    kept.push((site, object, x));
                }
            });
            if b.tags[v as usize] != Tag::Ref {
                // A lent `*T` (read, or forwarded to its value): its whole
                // object, or a `*dyn` receiver's payload types.
                let whole = match cx.reference(b.ty[v as usize]) {
                    Some(Reference {
                        target,
                        whole: true,
                        fat: false,
                        ..
                    }) => vec![target],
                    _ => cx
                        .dyn_receiver(*id, v as usize)
                        .and_then(|tr| out.dyn_types.get(&tr).cloned())
                        .unwrap_or_default(),
                };
                for t in whole {
                    if program(t, excl, own) && meets(lo, hi, &[v], t, excl) {
                        kept.push(((*id, v, u32::MAX), t, excl));
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
    let value = b.ty[v];
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

/// A reference's or a lent pointer read's crossed objects. Only a `&mut`
/// reference's last one takes an exclusive loan: a read only reads the
/// pointer slot it crosses last.
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
    let exclusive = b.tags[v as usize] == Tag::Ref && mutable(cx, b.ty[v as usize]);
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
    let param = b.local_ty[l];
    let counted = cx.reference(param).is_some_and(|r| r.whole && !r.fat);
    let mut out = Vec::new();
    for &i in cx.instructions(callee) {
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

fn call_args(b: &Body, i: usize) -> Vec<u32> {
    b.call_args(i)
        .iter()
        .copied()
        .filter(|&v| v < crate::ir::u32_of(i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_index_matches_a_scan_with_excluded_sources() {
        let repeated = InstId::from_raw(1);
        let rare = InstId::from_raw(2);
        let missing = InstId::from_raw(3);
        let first = Ty::I32;
        let second = Ty::BOOL;
        let summaries: HashMap<_, _> = [
            (
                repeated,
                Eff {
                    w: [first].into_iter().collect(),
                    ..Eff::default()
                },
            ),
            (
                rare,
                Eff {
                    r: [second].into_iter().collect(),
                    ..Eff::default()
                },
            ),
        ]
        .into_iter()
        .collect();
        let accesses: HashMap<_, _> = [
            (
                0,
                Acc {
                    callees: vec![repeated],
                    ..Acc::default()
                },
            ),
            (
                1,
                Acc {
                    r: vec![first],
                    ..Acc::default()
                },
            ),
            (
                2,
                Acc {
                    callees: vec![rare],
                    ..Acc::default()
                },
            ),
            (
                3,
                Acc {
                    callees: vec![repeated],
                    ..Acc::default()
                },
            ),
            (
                4,
                Acc {
                    callees: vec![missing],
                    ..Acc::default()
                },
            ),
            (
                5,
                Acc {
                    any: true,
                    ..Acc::default()
                },
            ),
            (
                6,
                Acc {
                    callees: vec![repeated],
                    w: vec![second],
                    ..Acc::default()
                },
            ),
        ]
        .into_iter()
        .collect();
        let index = EffectIndex::new(&accesses, &summaries);
        for from in 0..=7 {
            for to in from..=7 {
                for excluded in [vec![], vec![0], vec![3, 6], vec![4, 5]] {
                    for ty in [first, second, Ty::STRING] {
                        for exclusive in [false, true] {
                            let expected = accesses.iter().any(|(&i, acc)| {
                                (from..to).contains(&i)
                                    && !excluded.contains(&i)
                                    && (acc.any
                                        || acc.w.contains(&ty)
                                        || exclusive && acc.r.contains(&ty)
                                        || acc.callees.iter().any(|callee| {
                                            summaries
                                                .get(callee)
                                                .is_none_or(|effect| effect.meets(ty, exclusive))
                                        }))
                            });
                            assert_eq!(index.meets(from, to, &excluded, ty, exclusive), expected);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn wide_calls_share_one_deep_effect_summary() {
        let width = 1000u32;
        let callee = InstId::from_raw(1);
        let accesses: HashMap<_, _> = (0..width)
            .map(|position| {
                (
                    position,
                    Acc {
                        callees: vec![callee],
                        ..Acc::default()
                    },
                )
            })
            .collect();
        for depth in [1, 20] {
            let types = (100..100 + depth).map(Ty);
            let mut summary = Eff::default();
            for ty in types.clone() {
                let mut combined = Eff {
                    w: [ty].into_iter().collect(),
                    ..Eff::default()
                };
                combined.add(&summary);
                summary = combined;
            }
            let summaries = [(callee, summary)].into_iter().collect();
            let index = EffectIndex::new(&accesses, &summaries);
            assert_eq!(index.frequent.len(), 1);
            assert!(index.writes.is_empty(), "do not expand a repeated summary");
            for (i, ty) in (0..width).zip(types.cycle()) {
                assert!(index.meets(i, i + 1, &[], ty, false));
                assert!(!index.meets(i, i + 1, &[i], ty, true));
            }
            let probes = index.probes.get();
            assert_eq!(index.indexed, 2 * width as usize);
            let per_query = width.ilog2() as usize + 5;
            assert!(probes <= 2 * width as usize * per_query);
            eprintln!(
                "width {width}, effect depth {depth}: {} indexing visits, {probes} probes",
                index.indexed
            );
        }
    }
}
