//! The design's checking stages beyond bodies (`crates/hd_tir/DESIGN.md`
//! §4.13.1, §4.13.10; `crates/hd_check/DESIGN.md` (trait solver) §5; `crates/hd_sched/DESIGN.md` §6.1):
//! init order; overlap is `hd_resolve::Universe`.

use hd_base::HashMap;
use std::collections::BTreeSet;

use hd_base::DefId;

use crate::init::{CallNode, InitSummary};

/// A node of a cycle's call graph: a body, or a dispatch key of the
/// module at this index.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node {
    Body(DefId),
    Dispatch(usize, DefId),
}

/// One step of a module cycle's initialization: a module of the cycle,
/// by its index in identity order, and one of its statements.
pub type Step = (usize, usize);

/// What `cycle_order` finds for one module cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CycleOrder {
    /// Every statement, in the order the cycle runs them.
    Steps(Vec<Step>),
    /// No remaining statement is ready (`module.init.cycle.stuck`): the
    /// earliest remaining one, and the earliest binding it waits on.
    Stuck { at: Step, waits_on: DefId },
}

/// `InitOrder(F)` (§4.13.10) for one module cycle of several modules
/// (spec 10 "Order Inside A Cycle"). `modules` are the cycle's summaries
/// in module identity order. A statement depends on each binding of the
/// cycle its transitive read set holds, through every module's bodies
/// (`module.init.cycle.dependency`). Each step runs the earliest remaining
/// statement whose dependencies are all initialized
/// (`module.init.cycle.step`), earliest by module identity, then by source
/// position (`module.init.cycle.earliest`).
#[must_use]
pub fn cycle_order(modules: &[&InitSummary]) -> CycleOrder {
    // Statement `k` of the cycle: module `m`'s statement `s` is
    // `base[m] + s`, so `k` order is the earliest order.
    let mut base = Vec::with_capacity(modules.len());
    let mut steps: Vec<Step> = Vec::new();
    for (m, sum) in modules.iter().enumerate() {
        base.push(steps.len());
        steps.extend((0..sum.stmts.len()).map(|s| (m, s)));
    }
    // Each binding of the cycle: its index, then the statement that
    // initializes it.
    let mut binding: HashMap<DefId, usize> = HashMap::default();
    let mut owner: Vec<(usize, DefId)> = Vec::new();
    for (m, sum) in modules.iter().enumerate() {
        for (d, s) in &sum.globals {
            binding.insert(*d, owner.len());
            owner.push((base[m] + *s as usize, *d));
        }
    }
    // The bindings each body reaches, through its calls: each body's set
    // pushed to its callers until nothing grows.
    // A dispatch key reaches the methods of its own module's impls, so
    // it is a node per module.
    let node = |m: usize, c: CallNode| match c {
        CallNode::Body(d) => Node::Body(d),
        CallNode::Dispatch(k) => Node::Dispatch(m, k),
    };
    let mut reach: HashMap<Node, BTreeSet<usize>> = HashMap::default();
    let mut callers: HashMap<Node, Vec<Node>> = HashMap::default();
    for (m, sum) in modules.iter().enumerate() {
        for (d, f) in &sum.fns {
            let set = hd_base::btree_set(f.reads.iter().filter_map(|g| binding.get(g)).copied());
            reach.entry(Node::Body(*d)).or_default().extend(set);
            for c in f.callees() {
                callers.entry(node(m, c)).or_default().push(Node::Body(*d));
            }
        }
        for (k, methods) in &sum.dispatch {
            for d in methods {
                (callers.entry(Node::Body(*d)).or_default()).push(Node::Dispatch(m, *k));
            }
        }
    }
    let mut work: Vec<Node> = reach
        .iter()
        .filter(|(_, s)| !s.is_empty())
        .map(|(d, _)| *d)
        .collect();
    while let Some(d) = work.pop() {
        let set = reach.get(&d).cloned().unwrap_or_default();
        for c in callers.get(&d).into_iter().flatten() {
            let to = reach.entry(*c).or_default();
            let before = to.len();
            to.extend(set.iter().copied());
            if to.len() > before {
                work.push(*c);
            }
        }
    }
    // Each statement's dependencies, as bindings.
    let deps: Vec<BTreeSet<usize>> = modules
        .iter()
        .enumerate()
        .flat_map(|(m, sum)| sum.stmts.iter().map(move |st| (m, st)))
        .map(|(m, st)| {
            let mut set = hd_base::btree_set(
                st.facts
                    .reads
                    .iter()
                    .filter_map(|g| binding.get(g))
                    .copied(),
            );
            for c in st.facts.callees() {
                set.extend(reach.get(&node(m, c)).into_iter().flatten().copied());
            }
            set
        })
        .collect();
    // Ready statements by `k`; a statement's count is its dependencies
    // whose statement has not run.
    let n = steps.len();
    let mut waiting = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (k, set) in deps.iter().enumerate() {
        for g in set {
            waiting[k] += 1;
            dependents[owner[*g].0].push(k);
        }
    }
    let mut ready = hd_base::btree_set((0..n).filter(|k| waiting[*k] == 0));
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while let Some(k) = ready.pop_first() {
        done[k] = true;
        order.push(steps[k]);
        for j in &dependents[k] {
            waiting[*j] -= 1;
            if waiting[*j] == 0 {
                ready.insert(*j);
            }
        }
    }
    if let Some(k) = (0..n).find(|k| !done[*k]) {
        // A binding set is ordered by index: by statement, then as the
        // module's summary lists a statement's bindings.
        let waits_on = deps[k]
            .iter()
            .map(|g| owner[*g])
            .filter(|(s, _)| !done[*s])
            .min_by_key(|(s, _)| *s)
            .map_or(DefId::NONE, |(_, d)| d);
        return CycleOrder::Stuck {
            at: steps[k],
            waits_on,
        };
    }
    CycleOrder::Steps(order)
}

/// The runs of a cycle's steps: each a module and a range of consecutive
/// statements of it that run one after another.
#[must_use]
pub fn runs(steps: &[Step]) -> Vec<(usize, std::ops::Range<usize>)> {
    let mut out: Vec<(usize, std::ops::Range<usize>)> = Vec::new();
    for &(m, s) in steps {
        match out.last_mut() {
            Some((lm, r)) if *lm == m && r.end == s => r.end = s + 1,
            _ => out.push((m, s..s + 1)),
        }
    }
    out
}
