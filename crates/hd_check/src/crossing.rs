//! Reached top-level bindings (`fn.crossing.global`,
//! `fn.crossing.global.send`): a crossing closure reaches, directly or
//! through a callee, only immutable top-level bindings, and for `Send`
//! only `Send` ones. The closure's own reads and calls are read from its
//! TIR when its body finishes ([`root_facts`]). Each module exports its
//! part of the package's call graph, the facts `module.init.definite`
//! walks ([`crate::init::InitFacts`]), as a [`Reach`] ([`summary`]); the
//! driver joins the package's parts, cached modules' included ([`join`]).
//!
//! gap: #2288 — a callee in a dependency, std included, is not followed:
//! a dependency's summaries are not inputs to the package's check.

use hd_base::SliceSort as _;
use hd_base::{DefId, Hash128, HashMap, HashSet, NodeIdx, Span};
use hd_diag::{Code, DiagBuf};
use hd_tir::Callee;
use hd_tir::ir::{Body, PlaceRoot, Tag, Topology};

use crate::body::BodyCx;
use crate::init::InitFacts;

/// A closure written for a crossing parameter: where it is, whether its
/// marker is `Send`, and what its own body reads and calls.
pub(crate) struct Root {
    pub node: NodeIdx,
    pub send: bool,
    pub facts: InitFacts,
}

/// The top-level bindings closure `i` of `b` reads, and the functions it
/// calls, nested closures included. Cost: the body's instructions once,
/// plus one parent step per closure for each instruction's sub-body.
pub(crate) fn root_facts(b: &Body, topo: &Topology, i: usize) -> InitFacts {
    let root = b.data[i][0];
    // Each sub-body's parent sub-body, from the closures that make them.
    let mut parent: HashMap<u32, u32> = HashMap::default();
    for x in 0..b.len() {
        if b.tags[x] == Tag::Closure && !b.is_instantiation(x) {
            parent.insert(b.data[x][0], topo.sub[x]);
        }
    }
    let mut inside: HashMap<u32, bool> = HashMap::default();
    let mut within = |mut s: u32| -> bool {
        let mut path = Vec::new();
        let found = loop {
            if s == root {
                break true;
            }
            if let Some(&v) = inside.get(&s) {
                break v;
            }
            path.push(s);
            match parent.get(&s) {
                Some(&p) if p != s => s = p,
                _ => break false,
            }
        };
        for p in path {
            inside.insert(p, found);
        }
        found
    };
    let mut facts = InitFacts::default();
    for x in 0..b.len() {
        if !within(topo.sub[x]) {
            continue;
        }
        let tag = b.tags[x];
        if matches!(tag, Tag::Read | Tag::Ref | Tag::Store)
            && let Some(p) = b.decode_place(b.data[x][0])
            && let PlaceRoot::Global(def, _) = p.root
        {
            facts.reads.insert(def);
        }
        if matches!(tag, Tag::Call | Tag::Await)
            && let Some(callee) = Callee::from_words(b.record(b.data[x][0]))
        {
            match callee {
                Callee::Item { def, .. } => {
                    facts.calls.insert(def);
                }
                Callee::TraitMethod { method, .. } => {
                    facts.calls.insert(method);
                    facts.dispatch.insert(method);
                }
            }
        }
    }
    facts
}

/// One module's part of the package's reach graph (`fn.crossing.global`):
/// its bindings a crossing closure may not reach, its bodies' reads and
/// calls within the package, and its crossing closures. The driver joins
/// every module's part ([`join`]), as it joins `inst_cycles`' call edges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reach {
    /// Each such binding by path hash: 1 when it is reassignable, 2 when
    /// its type is not `Send`.
    pub bad: Vec<(Hash128, u8)>,
    /// Each body that reads a binding or calls a body of the package: its
    /// path hash, those bindings and those callees; and each dispatch key
    /// its bodies call, once, with the module's methods it reaches.
    pub bodies: Vec<(Hash128, Vec<Hash128>, Vec<Hash128>)>,
    pub roots: Vec<ReachRoot>,
}

/// A crossing closure's own reads and calls, and where to report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReachRoot {
    pub reads: Vec<Hash128>,
    pub callees: Vec<Hash128>,
    pub send: bool,
    pub span: Span,
}

/// The module's [`Reach`]. A call through a dispatch key reaches the
/// key's default and the module's implementations of it. Cost: the
/// module's facts once, and one probe per dispatch key.
#[must_use]
pub fn summary(cx: &BodyCx<'_>) -> Reach {
    let init = cx.init.borrow();
    let send_trait = cx.names.known.send;
    let mut decls_local = crate::value::InlineDecls::default();
    let decls = cx.decls(&mut decls_local);
    let pool = cx.names.pool.types();
    let hash = |d: DefId| cx.names.path_hash(d);
    // Runtime modules' storage is the instance's own
    // (`fn.crossing.global.runtime`).
    let mut bad: Vec<(Hash128, u8)> = init
        .globals
        .values()
        .filter(|g| {
            let path = cx.names.path(g.def);
            !path
                .rsplit_once('.')
                .is_some_and(|(module, _)| hd_resolve::runtime_module(module))
        })
        .map(|g| {
            let send = send_trait == DefId::NONE || hd_types::may_send(pool, &decls, g.ty);
            (hash(g.def), u8::from(!g.short) | (u8::from(!send) << 1))
        })
        .filter(|&(_, v)| v != 0)
        .collect();
    bad.sort_stable();
    let package = cx
        .lookup
        .own
        .first()
        .map(|it| cx.names.package_node(it.def));
    let ours = |d: &DefId| Some(cx.names.package_node(*d)) == package;
    let mut keys: HashSet<DefId> = HashSet::default();
    for f in (init.facts.values()).chain(init.crossing_roots.iter().map(|r| &r.facts)) {
        keys.extend(f.dispatch.iter().copied());
    }
    let reached = init.dispatch_reach(cx, &keys);
    let edges = |f: &InitFacts| -> (Vec<Hash128>, Vec<Hash128>) {
        let mut reads: Vec<Hash128> = f
            .reads
            .iter()
            .filter(|d| ours(d))
            .map(|d| hash(*d))
            .collect();
        // A dispatch key is a node of its own, listed once below with its
        // methods, so a body's facts do not grow with the module's impls.
        let mut calls: Vec<Hash128> = (f.calls.iter().filter(|d| ours(d)).copied())
            .chain(
                (f.dispatch.iter())
                    .filter(|&&k| reached.get(&k).is_some_and(|m| !m.is_empty()))
                    .copied(),
            )
            .map(hash)
            .collect();
        reads.sort_stable();
        calls.sort_stable();
        calls.dedup();
        (reads, calls)
    };
    let mut bodies: Vec<(Hash128, Vec<Hash128>, Vec<Hash128>)> = (init.facts.iter())
        .map(|(d, f)| {
            let (reads, calls) = edges(f);
            (hash(*d), reads, calls)
        })
        .filter(|(_, r, c)| !r.is_empty() || !c.is_empty())
        .collect();
    // Each dispatch key once, with the module's methods it reaches.
    bodies.extend(
        reached
            .iter()
            .filter(|(_, ms)| !ms.is_empty())
            .map(|(k, ms)| {
                let mut calls: Vec<Hash128> = ms.iter().map(|m| hash(*m)).collect();
                calls.sort_stable();
                (hash(*k), Vec::new(), calls)
            }),
    );
    bodies.sort_stable();
    let roots = (init.crossing_roots.iter())
        .map(|r| {
            let (reads, callees) = edges(&r.facts);
            ReachRoot {
                reads,
                callees,
                send: r.send,
                span: cx.src.span(cx.src.parse.tree.node(r.node)),
            }
        })
        .collect();
    Reach { bad, bodies, roots }
}

/// Reports each crossing closure of the joined `parts` that reaches a
/// reassignable top-level binding of the package, or, for `Send`, one
/// whose type is not `Send`, directly or through calls across modules.
/// Each body's verdict is pushed to its callers once per new bit.
/// Cost: nothing past the parts when no part has a crossing closure;
/// else the parts' bindings, bodies and call edges once.
pub fn join(parts: &[&Reach], diags: &mut DiagBuf) {
    if parts.iter().all(|p| p.roots.is_empty()) {
        return;
    }
    let bad: HashMap<Hash128, u8> = parts.iter().flat_map(|p| p.bad.iter().copied()).collect();
    let mut callers: HashMap<Hash128, Vec<Hash128>> = HashMap::default();
    let mut flags: HashMap<Hash128, u8> = HashMap::default();
    let mut work = Vec::new();
    for (d, reads, calls) in parts.iter().flat_map(|p| &p.bodies) {
        for c in calls {
            crate::work::scan_work(1);
            callers.entry(*c).or_default().push(*d);
        }
        let v = reads
            .iter()
            .filter_map(|g| bad.get(g))
            .fold(0, |a, b| a | b);
        if v != 0 {
            *flags.entry(*d).or_default() |= v;
            work.push(*d);
        }
    }
    while let Some(d) = work.pop() {
        let v = flags[&d];
        for c in callers.get(&d).into_iter().flatten() {
            crate::work::scan_work(1);
            let old = flags.get(c).copied().unwrap_or(0);
            if old | v != old {
                flags.insert(*c, old | v);
                work.push(*c);
            }
        }
    }
    for r in parts.iter().flat_map(|p| &p.roots) {
        crate::work::scan_work(1);
        let v = (r.reads.iter().filter_map(|g| bad.get(g)))
            .chain(r.callees.iter().filter_map(|c| flags.get(c)))
            .fold(0, |a, b| a | b);
        let mask = if r.send { 3 } else { 1 };
        if v & mask == 0 {
            continue;
        }
        let msg = if v & 1 != 0 {
            "this crossing closure reaches a top-level `let` binding, directly or through a callee; another domain would change only its own copy, so bind it with `:=` or pass the value in"
        } else {
            "this crossing closure reaches a top-level binding whose type is not `Send`, directly or through a callee; another thread must not share it"
        };
        diags.error(Code::CrossingTopLevelBinding, r.span, msg);
    }
}
