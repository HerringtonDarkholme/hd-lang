//! Reached top-level bindings (`fn.crossing.global`,
//! `fn.crossing.global.send`): a crossing closure reaches, directly or
//! through a callee, only immutable top-level bindings, and for `Send`
//! only `Send` ones. The closure's own reads and calls are read from its
//! TIR when its body finishes ([`root_facts`]); the module's call graph is
//! the one `module.init.definite` walks ([`crate::init::InitFacts`]), and
//! each node's verdict is pushed to its callers once ([`check`]).
//!
//! gap: #2288 — a callee of another module, or of a dependency, is not
//! followed: the module's own call graph is all a body check sees.

use hd_base::{DefId, HashMap, HashSet, NodeIdx};
use hd_diag::{Code, DiagBuf};
use hd_tir::Callee;
use hd_tir::ir::{Body, PlaceRoot, Tag, Topology};

use crate::body::BodyCx;
use crate::init::{CallNode, InitFacts};

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

/// Reports each crossing root that reaches a mutable top-level binding of
/// its module, or, for `Send`, one whose type is not `Send`. Cost: the
/// module's facts and its call edges once, then each root's own facts.
pub fn check(cx: &BodyCx<'_>, diags: &mut DiagBuf) {
    let init = cx.init.borrow();
    if init.crossing_roots.is_empty() {
        return;
    }
    let send_trait = cx.names.known.send;
    let mut decls_local = crate::value::InlineDecls::default();
    let decls = cx.decls(&mut decls_local);
    let pool = cx.names.pool.types();
    // Each own binding's verdict: 1 when it is reassignable, 2 when its
    // type is not `Send`. Runtime modules' storage is the instance's own
    // (`fn.crossing.global.runtime`).
    let bad: HashMap<DefId, u8> = init
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
            (g.def, u8::from(!g.short) | (u8::from(!send) << 1))
        })
        .filter(|&(_, v)| v != 0)
        .collect();
    // The call graph reversed, dispatch keys expanded once.
    let mut callers: HashMap<CallNode, Vec<CallNode>> = HashMap::default();
    let mut keys: HashSet<DefId> = HashSet::default();
    for (d, f) in &init.facts {
        for c in f.callees() {
            callers.entry(c).or_default().push(CallNode::Body(*d));
        }
        keys.extend(f.dispatch.iter().copied());
    }
    for r in &init.crossing_roots {
        keys.extend(r.facts.dispatch.iter().copied());
    }
    let reached = init.dispatch_reach(cx, &keys);
    for (k, ms) in &reached {
        for m in ms {
            callers
                .entry(CallNode::Body(*m))
                .or_default()
                .push(CallNode::Dispatch(*k));
        }
    }
    // Each node's verdict, pushed to its callers once per new bit.
    let mut flags: HashMap<CallNode, u8> = HashMap::default();
    let mut work = Vec::new();
    for (d, f) in &init.facts {
        let v = f
            .reads
            .iter()
            .filter_map(|g| bad.get(g))
            .fold(0, |a, b| a | b);
        if v != 0 {
            flags.insert(CallNode::Body(*d), v);
            work.push(CallNode::Body(*d));
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
    for r in &init.crossing_roots {
        crate::work::scan_work(1);
        let v = (r.facts.reads.iter().filter_map(|g| bad.get(g)))
            .chain(r.facts.callees().filter_map(|c| flags.get(&c)))
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
        let node = cx.src.parse.tree.node(r.node);
        diags.error(Code::CrossingTopLevelBinding, cx.src.span(node), msg);
    }
}
