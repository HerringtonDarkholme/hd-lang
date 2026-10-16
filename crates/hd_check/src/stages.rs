//! The design's checking stages beyond bodies (`crates/hd_tir/DESIGN.md`
//! §4.13.1, §4.13.10; `crates/hd_check/DESIGN.md` (trait solver) §5; `crates/hd_sched/DESIGN.md` §6.1):
//! init order; overlap is `hd_resolve::Universe`.

use std::collections::{BTreeSet, HashMap};

use hd_base::DefId;

use crate::init::InitSummary;

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
    let mut binding: HashMap<DefId, usize> = HashMap::new();
    let mut owner: Vec<(usize, DefId)> = Vec::new();
    for (m, sum) in modules.iter().enumerate() {
        for (d, s) in &sum.globals {
            binding.insert(*d, owner.len());
            owner.push((base[m] + *s as usize, *d));
        }
    }
    // The bindings each body reaches, through its calls: each body's set
    // pushed to its callers until nothing grows.
    let mut reach: HashMap<DefId, BTreeSet<usize>> = HashMap::new();
    let mut callers: HashMap<DefId, Vec<DefId>> = HashMap::new();
    for sum in modules {
        for (d, f) in &sum.fns {
            let set: BTreeSet<usize> = f
                .reads
                .iter()
                .filter_map(|g| binding.get(g))
                .copied()
                .collect();
            reach.entry(*d).or_default().extend(set);
            for c in &f.calls {
                callers.entry(*c).or_default().push(*d);
            }
        }
    }
    let mut work: Vec<DefId> = reach
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
        .flat_map(|sum| &sum.stmts)
        .map(|st| {
            let mut set: BTreeSet<usize> = st
                .facts
                .reads
                .iter()
                .filter_map(|g| binding.get(g))
                .copied()
                .collect();
            for c in &st.facts.calls {
                set.extend(reach.get(c).into_iter().flatten().copied());
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
    let mut ready: BTreeSet<usize> = (0..n).filter(|k| waiting[*k] == 0).collect();
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

#[cfg(test)]
mod tests {
    use hd_base::DefId;

    use super::{CycleOrder, cycle_order, runs};
    use crate::init::{InitFacts, InitSummary, StmtSummary};

    fn d(n: u32) -> DefId {
        DefId::from_raw(n)
    }

    /// A statement that reads the bindings `reads` and calls `calls`.
    fn stmt(reads: &[u32], calls: &[u32]) -> StmtSummary {
        StmtSummary {
            facts: InitFacts {
                reads: reads.iter().map(|r| d(*r)).collect(),
                calls: calls.iter().map(|c| d(*c)).collect(),
            },
            ..StmtSummary::default()
        }
    }

    /// A module: its statements, its bindings (binding, statement), and
    /// its functions (function, reads, calls).
    fn module(
        stmts: Vec<StmtSummary>,
        globals: &[(u32, u32)],
        fns: &[(u32, &[u32], &[u32])],
    ) -> InitSummary {
        InitSummary {
            stmts,
            globals: globals.iter().map(|(g, s)| (d(*g), *s)).collect(),
            fns: fns
                .iter()
                .map(|(f, r, c)| {
                    (
                        d(*f),
                        InitFacts {
                            reads: r.iter().map(|x| d(*x)).collect(),
                            calls: c.iter().map(|x| d(*x)).collect(),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn a_two_module_cycle_runs_a_statement_after_the_binding_it_reaches() {
        // catalog: `let featured = price_of("tea")`; prices: `let markup = 5`,
        // and `price_of` (20) reads `markup` (10) and calls `base_price` (30).
        let catalog = module(vec![stmt(&[], &[20])], &[(1, 0)], &[(30, &[], &[])]);
        let prices = module(vec![stmt(&[], &[])], &[(10, 0)], &[(20, &[10], &[30])]);
        let order = cycle_order(&[&catalog, &prices]);
        assert_eq!(order, CycleOrder::Steps(vec![(1, 0), (0, 0)]));
        let CycleOrder::Steps(steps) = order else {
            unreachable!()
        };
        assert_eq!(runs(&steps), vec![(1, 0..1), (0, 0..1)]);
    }

    #[test]
    fn independent_statements_run_by_module_identity_then_position() {
        let one = module(vec![stmt(&[], &[]), stmt(&[], &[])], &[], &[]);
        let two = module(vec![stmt(&[], &[])], &[], &[]);
        let order = cycle_order(&[&one, &two]);
        assert_eq!(order, CycleOrder::Steps(vec![(0, 0), (0, 1), (1, 0)]));
    }

    #[test]
    fn a_three_module_cycle_follows_calls_through_every_module() {
        // a's statement calls b.f (20), which calls c.g (30), which reads
        // c's binding (31), set by c's second statement; c's first
        // statement calls a.h (10), which reads nothing.
        let a = module(vec![stmt(&[], &[20])], &[(11, 0)], &[(10, &[], &[])]);
        let b = module(vec![stmt(&[], &[])], &[], &[(20, &[], &[30])]);
        let c = module(
            vec![stmt(&[], &[10]), stmt(&[], &[])],
            &[(31, 1)],
            &[(30, &[31], &[])],
        );
        let order = cycle_order(&[&a, &b, &c]);
        assert_eq!(
            order,
            CycleOrder::Steps(vec![(1, 0), (2, 0), (2, 1), (0, 0)])
        );
    }

    #[test]
    fn statements_interleave_with_the_bindings_they_wait_on() {
        // a: `let x = b.get_y()`, `record("a")`, `let z = x`; b: `let y = 1`,
        // `let w = a.get_z()`.
        let a = module(
            vec![stmt(&[], &[20]), stmt(&[], &[]), stmt(&[1], &[])],
            &[(1, 0), (3, 2)],
            &[(10, &[3], &[])],
        );
        let b = module(
            vec![stmt(&[], &[]), stmt(&[], &[10])],
            &[(21, 0), (22, 1)],
            &[(20, &[21], &[])],
        );
        let CycleOrder::Steps(steps) = cycle_order(&[&a, &b]) else {
            panic!("the cycle has an order")
        };
        // a's second statement is ready first; a's first waits on `y`.
        assert_eq!(steps, vec![(0, 1), (1, 0), (0, 0), (0, 2), (1, 1)]);
        assert_eq!(
            runs(&steps),
            vec![(0, 1..2), (1, 0..1), (0, 0..1), (0, 2..3), (1, 1..2)]
        );
    }

    #[test]
    fn statements_that_wait_on_each_other_are_stuck() {
        // a: `let a = b.read_b()`; b: `let b = a.read_a()`.
        let a = module(vec![stmt(&[], &[20])], &[(1, 0)], &[(10, &[1], &[])]);
        let b = module(vec![stmt(&[], &[10])], &[(2, 0)], &[(20, &[2], &[])]);
        assert_eq!(
            cycle_order(&[&a, &b]),
            CycleOrder::Stuck {
                at: (0, 0),
                waits_on: d(2),
            }
        );
    }
}
