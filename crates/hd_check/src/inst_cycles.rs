//! Generic call cycles whose type arguments grow (spec 04
//! `types.generic.polymorphic-recursion`), found at check time from the
//! calls a module's bodies write, as Go's instantiation cycle check.
//!
//! The graph's nodes are type parameters, each as one function's body sees
//! it: a parameter of an implementation is a node per method. A call in
//! the body of `f` whose callee `g` is fixed at check time passes a type
//! argument `A` for each parameter `Q` of `g`: every parameter `P` of `f`
//! (its own or its implementation's) that occurs in `A` gives an edge
//! `P -> Q`, which grows when `A` is not `P` itself. A growing edge whose
//! ends lie in one strongly connected component sits on a cycle: the
//! instantiations along it never end, whatever the layout of `P`.
//!
//! A callee is fixed when the call names a function or method, a generic
//! local function, or a trait method whose implementation the checker
//! selected. A call through a bound or a trait value has no fixed callee,
//! and adds no edge: such a cycle stays the instantiation depth limit's
//! (`types.generic.instantiation-depth.error`). A parameter inside a
//! projection (`T.Item`) adds no edge either, since the projection's
//! size depends on the implementation. The graph covers one module's
//! bodies; a cycle through several files of a folder is left to the limit.

use hd_base::{DefId, HashMap, HashSet, NodeIdx};
use hd_diag::{Code, DiagBuf};
use hd_resolve::ItemData;
use hd_tir::Body;
use hd_tir::ir::{Callee, ChoiceKind, Tag};
use hd_types::{ParamRef, Ty, TyData, Types};

use crate::body::BodyCx;

/// A node: a type parameter as one function's body sees it. A parameter
/// of an implementation is a node per method, since each method is its
/// own instantiation.
type Node = (DefId, ParamRef);

/// One edge `from -> to`, made by the argument `arg` of a call at `at`
/// in the body of `caller` to `callee`.
struct Edge {
    from: Node,
    to: Node,
    grows: bool,
    caller: DefId,
    callee: DefId,
    arg: Ty,
    at: NodeIdx,
}

/// Reports each call that passes a growing type argument around a generic
/// call cycle of `bodies`, once per call. Error: `instantiation-too-deep`.
pub fn growing_cycles(cx: &BodyCx<'_>, bodies: &[Body], diags: &mut DiagBuf) {
    let pool = cx.names.pool.types();
    let mut edges = Vec::new();
    for b in bodies {
        call_edges(cx, pool, b, &mut edges);
    }
    if !edges.iter().any(|e| e.grows) {
        return;
    }
    let mut ids: HashMap<Node, usize> = HashMap::default();
    for e in &edges {
        for p in [e.from, e.to] {
            let n = ids.len();
            ids.entry(p).or_insert(n);
        }
    }
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    for (k, e) in edges.iter().enumerate() {
        succ[ids[&e.from]].push(k);
    }
    let comp = components(&succ, &edges, &ids);
    // A call the row solve already stopped at (a private row's key that
    // grows the same way) keeps that one report.
    let reported: Vec<hd_base::Span> = (0..diags.len())
        .filter(|&i| diags.code[i] == Code::InstantiationTooDeep)
        .map(|i| diags.primary[i])
        .collect();
    let mut seen: HashSet<NodeIdx> = HashSet::default();
    let mut found: Vec<(hd_base::Span, &Edge)> = Vec::new();
    for e in &edges {
        if e.grows && comp[ids[&e.from]] == comp[ids[&e.to]] && seen.insert(e.at) {
            let span = cx.src.span(cx.src.parse.tree.node(e.at));
            if !reported.contains(&span) {
                found.push((span, e));
            }
        }
    }
    found.sort_by_key(|(span, _)| span.lo);
    for (span, e) in found {
        let mut cycle = vec![
            cx.names.display_name(e.caller),
            cx.names.display_name(e.callee),
        ];
        for k in path(&succ, &edges, &ids, e.to, e.from) {
            cycle.push(cx.names.display_name(edges[k].callee));
        }
        let show = |t: Ty| hd_resolve::show_ty_in(&cx.names, pool, t);
        let msg = format!(
            "this call passes `{}` for `{}`, a larger type that holds `{}`, around the generic call cycle `{}`: its instantiations grow without end",
            show(strip_mut(pool, e.arg)),
            show(pool_param(cx, e.to.1)),
            show(pool_param(cx, e.from.1)),
            cycle.join(" -> "),
        );
        diags.error(Code::InstantiationTooDeep, span, &msg);
    }
}

fn pool_param(cx: &BodyCx<'_>, p: ParamRef) -> Ty {
    cx.names.pool.intern_ty(&TyData::Param(p))
}

fn strip_mut(pool: Types<'_>, t: Ty) -> Ty {
    match pool.get(t) {
        TyData::Mut(i) => i,
        _ => t,
    }
}

/// The edges of each call in `b` whose callee is fixed.
fn call_edges(cx: &BodyCx<'_>, pool: Types<'_>, b: &Body, out: &mut Vec<Edge>) {
    for (i, tag) in b.tags.iter().enumerate() {
        let local = *tag == Tag::Closure && b.is_instantiation(i);
        if !local && !matches!(tag, Tag::Call | Tag::Await) {
            continue;
        }
        let at = b.syn.get(i).copied().unwrap_or(NodeIdx::NONE);
        if at == NodeIdx::NONE {
            continue;
        }
        let (callee, slots) = if local {
            local_slots(pool, b, i)
        } else {
            let Some(c) = Callee::from_words(b.record(b.data[i][0])) else {
                continue;
            };
            match c {
                Callee::Item { def, targs } => {
                    let args = pool.list_items(targs);
                    let owner = parent(cx, def);
                    let n = owner.map_or(0, |(_, n)| n);
                    let slots: Vec<(ParamRef, Ty)> = args
                        .iter()
                        .enumerate()
                        .filter_map(|(k, &a)| {
                            let p = match owner {
                                Some((o, _)) if k < n => ParamRef {
                                    owner: o,
                                    index: u16::try_from(k).ok()?,
                                },
                                _ => ParamRef {
                                    owner: def,
                                    index: u16::try_from(k - n).ok()?,
                                },
                            };
                            Some((p, a))
                        })
                        .collect();
                    (def, slots)
                }
                Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty,
                    targs,
                    choice: (ChoiceKind::Impl, imp),
                } => {
                    let imp = DefId::from_raw(imp);
                    match impl_slots(cx, pool, trait_, method, imp, self_ty, targs) {
                        Some(x) => x,
                        None => continue,
                    }
                }
                Callee::TraitMethod { .. } => continue,
            }
        };
        for (to, arg) in slots {
            let whole = strip_mut(pool, arg);
            let mut params = Vec::new();
            params_in(pool, arg, &mut params);
            params.sort_by_key(|p| (p.owner.raw(), p.index));
            params.dedup();
            for p in params {
                let grows = !matches!(pool.get(whole), TyData::Param(q) if q == p);
                out.push(Edge {
                    from: (b.item, p),
                    to: (callee, to),
                    grows,
                    caller: b.item,
                    callee,
                    arg,
                    at,
                });
            }
        }
    }
}

/// The parameter slots of a use of a generic local function, `Closure`
/// instruction `i`: its own parameters are the body's, numbered from the
/// record's first index (`crate::local::LocalGeneric`).
fn local_slots(pool: Types<'_>, b: &Body, i: usize) -> (DefId, Vec<(ParamRef, Ty)>) {
    let rec = b.record(b.data[i][1]);
    let first = rec.get(2).copied().unwrap_or(0);
    let targs = rec
        .get(3)
        .map_or(&[][..], |&l| pool.list_items(hd_types::TyList(l)));
    let slots = targs
        .iter()
        .zip(first..)
        .filter_map(|(&a, k)| {
            let index = u16::try_from(k).ok()?;
            Some((
                ParamRef {
                    owner: b.item,
                    index,
                },
                a,
            ))
        })
        .collect();
    (b.item, slots)
}

/// The item that owns `def`'s leading type parameters, and how many it
/// has: a method's implementation, data type or enum, or its trait,
/// whose `Self` is parameter 0.
fn parent(cx: &BodyCx<'_>, def: DefId) -> Option<(DefId, usize)> {
    let ItemData::Method { owner, .. } = &cx.lookup.item(def)?.data else {
        return None;
    };
    let o = cx.lookup.item(*owner)?;
    match &o.data {
        ItemData::Impl { .. } | ItemData::Data(_) | ItemData::Enum { .. } => {
            Some((*owner, o.generics.len()))
        }
        ItemData::Trait(_) => Some((*owner, 1 + o.generics.len())),
        _ => None,
    }
}

/// The selected implementation's method of a trait method call, and its
/// parameter slots: the implementation's parameters that its head fixes
/// from the call's receiver and trait arguments, then the method's own.
fn impl_slots(
    cx: &BodyCx<'_>,
    pool: Types<'_>,
    trait_: DefId,
    method: DefId,
    imp: DefId,
    self_ty: Ty,
    targs: hd_types::TyList,
) -> Option<(DefId, Vec<(ParamRef, Ty)>)> {
    let name = cx.lookup.item(method)?.name;
    let it = cx.lookup.item(imp)?;
    let ItemData::Impl {
        self_ty: head,
        trait_args,
        methods,
        ..
    } = &it.data
    else {
        return None;
    };
    let m = methods.iter().find(|(n, _)| *n == name)?.1;
    let n_trait = cx.lookup.item(trait_).map_or(0, |t| t.generics.len());
    let args = pool.list_items(targs);
    let mut fixed: Vec<Option<Ty>> = vec![None; it.generics.len()];
    head_match(pool, imp, *head, self_ty, &mut fixed);
    for (&x, &y) in pool.list_items(*trait_args).iter().zip(args) {
        head_match(pool, imp, x, y, &mut fixed);
    }
    let mut slots = Vec::new();
    for (k, t) in fixed.into_iter().enumerate() {
        if let Some(t) = t {
            let index = u16::try_from(k).ok()?;
            slots.push((ParamRef { owner: imp, index }, t));
        }
    }
    for (k, &a) in args.iter().enumerate().skip(n_trait) {
        let index = u16::try_from(k - n_trait).ok()?;
        slots.push((ParamRef { owner: m, index }, a));
    }
    Some((m, slots))
}

/// Binds the parameters of `owner` in `pattern` to the parts of `t`
/// they stand for.
fn head_match(pool: Types<'_>, owner: DefId, pattern: Ty, t: Ty, out: &mut Vec<Option<Ty>>) {
    let (pattern, t) = (strip_mut(pool, pattern), strip_mut(pool, t));
    match (pool.get(pattern), pool.get(t)) {
        (TyData::Param(p), _) if p.owner == owner => {
            if let Some(slot) = out.get_mut(usize::from(p.index)) {
                slot.get_or_insert(t);
            }
        }
        (TyData::Adt { args: a, .. }, TyData::Adt { args: b, .. })
        | (TyData::TraitValue { args: a, .. }, TyData::TraitValue { args: b, .. })
        | (TyData::Tuple { elems: a, .. }, TyData::Tuple { elems: b, .. }) => {
            for (&x, &y) in pool.list_items(a).iter().zip(pool.list_items(b)) {
                head_match(pool, owner, x, y, out);
            }
        }
        (TyData::Option(x), TyData::Option(y)) => head_match(pool, owner, x, y, out),
        (
            TyData::Fn {
                params: a,
                result: r,
                ..
            },
            TyData::Fn {
                params: b,
                result: s,
                ..
            },
        ) => {
            for (&x, &y) in pool.list_items(a).iter().zip(pool.list_items(b)) {
                head_match(pool, owner, x, y, out);
            }
            head_match(pool, owner, r, s, out);
        }
        _ => {}
    }
}

/// The type parameters that occur in `t`, outside projections.
pub(crate) fn params_in(pool: Types<'_>, t: Ty, out: &mut Vec<ParamRef>) {
    match pool.get(t) {
        TyData::Param(p) => out.push(p),
        TyData::Adt { args, .. } => {
            for &a in pool.list_items(args) {
                params_in(pool, a, out);
            }
        }
        TyData::Tuple { elems, rest } => {
            for &a in pool.list_items(elems) {
                params_in(pool, a, out);
            }
            if let Some(r) = rest {
                params_in(pool, r, out);
            }
        }
        TyData::Option(i) | TyData::Mut(i) | TyData::Own(i) | TyData::Borrow { ty: i, .. } => {
            params_in(pool, i, out);
        }
        TyData::Fn {
            params,
            result,
            row,
            ..
        } => {
            for &a in pool.list_items(params) {
                params_in(pool, a, out);
            }
            params_in(pool, result, out);
            for k in pool.row_data(row).keys {
                params_in(pool, k, out);
            }
        }
        TyData::TraitValue { args, bindings, .. } => {
            for &a in pool.list_items(args) {
                params_in(pool, a, out);
            }
            for (_, b) in bindings {
                params_in(pool, b, out);
            }
        }
        TyData::Row(r) | TyData::Context(r) => {
            for k in pool.row_data(r).keys {
                params_in(pool, k, out);
            }
        }
        TyData::Assoc { .. }
        | TyData::Prim(_)
        | TyData::Never
        | TyData::Poison
        | TyData::Infer(_)
        | TyData::Canon(_)
        | TyData::Lifetime(_)
        | TyData::Lifetimes(_) => {}
    }
}

/// Each node's strongly connected component (Tarjan's algorithm, without
/// recursion).
fn components(succ: &[Vec<usize>], edges: &[Edge], ids: &HashMap<Node, usize>) -> Vec<usize> {
    const UNSEEN: usize = usize::MAX;
    let n = succ.len();
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![UNSEEN; n];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut comps = 0;
    for root in 0..n {
        if index[root] != UNSEEN {
            continue;
        }
        // (node, position in its successor list)
        let mut work = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&mut (v, ref mut k)) = work.last_mut() {
            if let Some(&e) = succ[v].get(*k) {
                *k += 1;
                let w = ids[&edges[e].to];
                if index[w] == UNSEEN {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(u, _)) = work.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    comp[w] = comps;
                    if w == v {
                        break;
                    }
                }
                comps += 1;
            }
        }
    }
    comp
}

/// The edges of a shortest path from `from` to `to`, empty when they are
/// the same node.
fn path(
    succ: &[Vec<usize>],
    edges: &[Edge],
    ids: &HashMap<Node, usize>,
    from: Node,
    to: Node,
) -> Vec<usize> {
    let (s, t) = (ids[&from], ids[&to]);
    let mut via: Vec<Option<usize>> = vec![None; succ.len()];
    let mut queue = std::collections::VecDeque::from([s]);
    let mut reached = vec![false; succ.len()];
    reached[s] = true;
    while let Some(v) = queue.pop_front() {
        if v == t {
            break;
        }
        for &e in &succ[v] {
            let w = ids[&edges[e].to];
            if !reached[w] {
                reached[w] = true;
                via[w] = Some(e);
                queue.push_back(w);
            }
        }
    }
    let mut out = Vec::new();
    let mut v = t;
    while v != s {
        let Some(e) = via[v] else { break };
        out.push(e);
        v = ids[&edges[e].from];
    }
    out.reverse();
    out
}
