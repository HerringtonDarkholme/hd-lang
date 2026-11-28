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
//! size depends on the implementation. Each module contributes a compact
//! summary; the driver checks the joined graph of its unit's modules.

use hd_base::SliceSort as _;
use hd_base::{DefId, Hash128, HashMap, HashSet, NodeIdx, Span};
use hd_diag::{Code, DiagBuf};
use hd_resolve::ItemData;
use hd_tir::Body;
use hd_tir::ir::{Callee, ChoiceKind, Tag};
use hd_types::{ParamRef, Ty, TyData, Types};

use crate::body::BodyCx;

/// A node: a type parameter as one function's body sees it. A parameter
/// of an implementation is a node per method, since each method is its
/// own instantiation. The fields are the function path hash, parameter
/// owner path hash, and parameter index, stable across cache reads.
pub type Node = (Hash128, Hash128, u16);

/// A fixed-callee call edge, independent of process-local type/definition IDs.
/// Only growing edges carry a diagnostic prefix and caller display name.
#[derive(Clone)]
pub struct Edge {
    pub from: Node,
    pub to: Node,
    pub grows: bool,
    pub caller: String,
    pub callee: String,
    pub message: String,
    pub span: Span,
}

/// Collects a module's fixed-callee edges while its checked TIR is available.
pub fn call_edges(cx: &BodyCx<'_>, bodies: &[Body]) -> Vec<Edge> {
    let pool = cx.names.pool.types();
    let mut edges = Vec::new();
    let mut memo = ParamsMemo::default();
    for b in bodies {
        body_edges(cx, pool, b, &mut edges, &mut memo);
    }
    edges
}

/// The parameters of the type arguments [`params_in`] already walked, by
/// type id, each as a sorted, deduplicated span of `found`. A type's
/// parameters are a function of its interned content, so one module's
/// calls share the memo.
#[derive(Default)]
struct ParamsMemo {
    spans: HashMap<Ty, (u32, u32)>,
    found: Vec<ParamRef>,
}

/// Reports growing cycles in joined module summaries, once per call span.
/// Error: `instantiation-too-deep`.
pub fn growing_cycles(edges: &[&Edge], diags: &mut DiagBuf) {
    if !edges.iter().any(|e| e.grows) {
        return;
    }
    let mut ids: HashMap<Node, usize> = HashMap::default();
    for e in edges {
        for p in [e.from, e.to] {
            let n = ids.len();
            ids.entry(p).or_insert(n);
        }
    }
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    for (k, e) in edges.iter().enumerate() {
        succ[ids[&e.from]].push(k);
    }
    let comp = components(&succ, edges, &ids);
    // A call the row solve already stopped at (a private row's key that
    // grows the same way) keeps that one report.
    let reported: Vec<hd_base::Span> = (0..diags.len())
        .filter(|&i| diags.code[i] == Code::InstantiationTooDeep)
        .map(|i| diags.primary[i])
        .collect();
    let mut seen = HashSet::default();
    let mut found: Vec<(hd_base::Span, &Edge)> = Vec::new();
    for e in edges {
        if e.grows
            && comp[ids[&e.from]] == comp[ids[&e.to]]
            && seen.insert((e.span.file, e.span.lo, e.span.hi))
        {
            let span = e.span;
            if !reported.contains(&span) {
                found.push((span, e));
            }
        }
    }
    found.sort_stable_by_key(|(span, _)| (span.file.raw(), span.lo, span.hi));
    for (span, e) in found {
        let mut cycle = vec![e.caller.as_str(), e.callee.as_str()];
        for k in path(&succ, edges, &ids, e.to, e.from) {
            cycle.push(edges[k].callee.as_str());
        }
        let msg = format!(
            "{} `{}`: its instantiations grow without end",
            e.message,
            cycle.join(" -> "),
        );
        diags.error(Code::InstantiationTooDeep, span, &msg);
    }
}

fn pool_param(cx: &BodyCx<'_>, p: ParamRef) -> Ty {
    cx.names.pool.intern_ty(&TyData::Param(p))
}

/// A bare type or row parameter represented as one generic slot.
fn bare_param(pool: Types<'_>, t: Ty) -> Option<ParamRef> {
    match pool.get(t) {
        TyData::Param(p) => Some(p),
        TyData::Row(row) => {
            let row = pool.row_data(row);
            match (row.keys.as_slice(), row.params.as_slice()) {
                ([], [p]) => Some(ParamRef {
                    owner: p.owner,
                    index: p.index,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The edges of each call in `b` whose callee is fixed.
fn body_edges(
    cx: &BodyCx<'_>,
    pool: Types<'_>,
    b: &Body,
    out: &mut Vec<Edge>,
    memo: &mut ParamsMemo,
) {
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
            let whole = arg;
            let params = params_in(pool, arg, memo);
            for p in params {
                let grows = bare_param(pool, whole) != Some(p);
                let node = |owner, p: ParamRef| {
                    (
                        cx.names.path_hash(owner),
                        cx.names.path_hash(p.owner),
                        p.index,
                    )
                };
                let show = |t| hd_resolve::show_ty_in(&cx.names, pool, t);
                out.push(Edge {
                    from: node(b.item, p),
                    to: node(callee, to),
                    grows,
                    caller: if grows { cx.names.display_name(b.item).to_owned() } else { String::new() },
                    callee: cx.names.display_name(callee).to_owned(),
                    message: if grows {
                        format!(
                            "this call passes `{}` for `{}`, a larger type that holds `{}`, around the generic call cycle",
                            show(whole), show(pool_param(cx, to)), show(pool_param(cx, p)),
                        )
                    } else { String::new() },
                    span: cx.src.span(cx.src.parse.tree.node(at)),
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
/// has: a method's implementation, struct or enum, or its trait,
/// whose `Self` is parameter 0.
fn parent(cx: &BodyCx<'_>, def: DefId) -> Option<(DefId, usize)> {
    let ItemData::Method { owner, .. } = &cx.lookup.item(def)?.data else {
        return None;
    };
    let o = cx.lookup.item(*owner)?;
    match &o.data {
        ItemData::Impl { .. } | ItemData::Struct(_, _) | ItemData::Enum { .. } => {
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
    let heads = [&[*head], pool.list_items(*trait_args)].concat();
    let n = heads.len().min(1 + args.len());
    let goals = [&[self_ty], &args[..n - 1]].concat();
    let mut fixed = Vec::new();
    hd_types::solver::bind_head(pool, imp, &heads[..n], &goals, &mut fixed);
    fixed.truncate(it.generics.len());
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

/// The type parameters that occur in `t`, outside projections, sorted
/// and deduplicated. A work stack over each distinct node once (by type
/// id): a part shared by many slots, as in `(T, T)` nested n deep (2^n
/// leaves, n + 1 nodes), is walked once (#1886). A part with no parameter
/// is skipped by its pool flag, and a part an earlier call's argument
/// was is answered by `memo` (#1912): `wrap(wrap(...x))` passes `W^i[T]`
/// at level i, whose part `W^(i-1)[T]` the call below passed, so each
/// call walks one new node. Cost: the new nodes that hold a parameter,
/// plus the sort of the parameters found.
fn params_in(pool: Types<'_>, t: Ty, memo: &mut ParamsMemo) -> Vec<ParamRef> {
    if let Some(&(from, to)) = memo.spans.get(&t) {
        return memo.found[from as usize..to as usize].to_vec();
    }
    let mut out = Vec::new();
    let mut seen: HashSet<Ty> = HashSet::default();
    let mut stack = vec![t];
    let row_params = |r: hd_types::RowId, out: &mut Vec<ParamRef>, stack: &mut Vec<Ty>| {
        let row = pool.row_data(r);
        out.extend(row.params.iter().map(|p| ParamRef {
            owner: p.owner,
            index: p.index,
        }));
        stack.extend(row.keys.iter().copied());
    };
    while let Some(t) = stack.pop() {
        if !pool.has_param(t) || !seen.insert(t) {
            continue;
        }
        if let Some(&(from, to)) = memo.spans.get(&t) {
            out.extend_from_slice(&memo.found[from as usize..to as usize]);
            continue;
        }
        hd_types::work::type_node();
        match pool.get(t) {
            TyData::Param(p) => out.push(p),
            TyData::Adt(_, args) => stack.extend_from_slice(pool.list_items(args)),
            TyData::Opaque { args, bound, .. } => {
                stack.extend_from_slice(pool.list_items(args));
                stack.push(bound);
            }
            TyData::Closure {
                args,
                sig,
                captures,
                ..
            } => {
                stack.extend_from_slice(pool.list_items(args));
                stack.push(sig);
                stack.extend_from_slice(pool.list_items(captures));
            }

            TyData::Ref { inner, .. } => stack.push(inner),
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                stack.extend_from_slice(pool.list_items(params));
                stack.push(result);
                row_params(row, &mut out, &mut stack);
            }
            TyData::TraitValue { args, bindings, .. } => {
                stack.extend_from_slice(pool.list_items(args));
                stack.extend(bindings.iter().map(|&(_, b)| b));
            }
            TyData::Row(r) | TyData::Context(r) => row_params(r, &mut out, &mut stack),
            TyData::Assoc { .. }
            | TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_) => {}
        }
    }
    out.sort_stable_by_key(|p| (p.owner.raw(), p.index));
    out.dedup();
    let from = u32::try_from(memo.found.len()).unwrap_or(u32::MAX);
    memo.found.extend_from_slice(&out);
    let to = u32::try_from(memo.found.len()).unwrap_or(u32::MAX);
    memo.spans.insert(t, (from, to));
    out
}

/// Each node's strongly connected component ([`hd_base::scc`]).
fn components(succ: &[Vec<usize>], edges: &[&Edge], ids: &HashMap<Node, usize>) -> Vec<u32> {
    let sccs = hd_base::scc::sccs(succ.len(), |v, out| {
        out.extend(
            succ[v as usize]
                .iter()
                .map(|&e| hd_base::scc::node(ids[&edges[e].to])),
        );
    });
    sccs.comp
}

/// The edges of a shortest path from `from` to `to`, empty when they are
/// the same node.
fn path(
    succ: &[Vec<usize>],
    edges: &[&Edge],
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
