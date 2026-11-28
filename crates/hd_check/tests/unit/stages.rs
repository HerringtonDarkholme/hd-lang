use hd_base::{DefId, HashSet};
use hd_check::stages::{CycleOrder, cycle_order, runs};

use hd_check::init::{InitFacts, InitSummary, StmtSummary};

fn d(n: u32) -> DefId {
    DefId::from_raw(n)
}

/// A statement that reads the bindings `reads` and calls `calls`.
fn stmt(reads: &[u32], calls: &[u32]) -> StmtSummary {
    StmtSummary {
        facts: InitFacts {
            reads: reads.iter().map(|r| d(*r)).collect(),
            calls: calls.iter().map(|c| d(*c)).collect(),
            ..InitFacts::default()
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
                        ..InitFacts::default()
                    },
                )
            })
            .collect(),
        dispatch: Vec::new(),
    }
}

#[test]
fn a_dispatch_key_reaches_the_methods_of_its_own_module() {
    // prices: `let markup = 5`; method 20 of an impl reads `markup` (10),
    // and function 21 calls through the key 40 that reaches it. catalog
    // names the key 40 too, which reaches no method of its own.
    let mut prices = module(vec![stmt(&[], &[])], &[(10, 0)], &[(20, &[10], &[])]);
    prices.fns.push((
        d(21),
        InitFacts {
            dispatch: [d(40)].into_iter().collect(),
            ..InitFacts::default()
        },
    ));
    prices.dispatch = vec![(d(40), vec![d(20)])];
    let through = |calls: &[u32], key: bool| {
        let mut st = stmt(&[], calls);
        if key {
            st.facts.dispatch.insert(d(40));
        }
        let mut catalog = module(vec![st], &[(1, 0)], &[]);
        catalog.dispatch = vec![(d(40), vec![])];
        cycle_order(&[&catalog, &prices])
    };
    assert_eq!(
        through(&[21], false),
        CycleOrder::Steps(vec![(1, 0), (0, 0)])
    );
    assert_eq!(through(&[], true), CycleOrder::Steps(vec![(0, 0), (1, 0)]));
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

#[test]
fn uncalled_function_graphs_do_no_cycle_read_closure_work() {
    // Ten times as many independent chains, each twenty calls deep.
    // Their leaf reads a real cycle binding, so eager closure is observable.
    for width in [100u32, 1000] {
        let depth = 20u32;
        let mut one = module(vec![stmt(&[], &[])], &[(1, 0)], &[]);
        let two = module(vec![stmt(&[], &[])], &[(2, 0)], &[]);
        for chain in 0..width {
            let first = 1000 + chain * depth;
            for level in 0..depth {
                let leaf = level + 1 == depth;
                one.fns.push((
                    d(first + level),
                    InitFacts {
                        reads: if leaf {
                            [d(2)].into_iter().collect()
                        } else {
                            HashSet::default()
                        },
                        calls: if leaf {
                            HashSet::default()
                        } else {
                            [d(first + level + 1)].into_iter().collect()
                        },
                        ..InitFacts::default()
                    },
                ));
            }
            one.dispatch.push((d(500_000 + chain), vec![d(first)]));
        }
        let (order, scanned) = hd_check::work::count_scan_work(|| cycle_order(&[&one, &two]));
        assert_eq!(order, CycleOrder::Steps(vec![(0, 0), (1, 0)]));
        assert_eq!(scanned, 0, "no function or dispatch summary is visited");

        // Direct binding dependencies still order rootless initializers.
        one.stmts[0].facts.reads.insert(d(2));
        assert_eq!(
            cycle_order(&[&one, &two]),
            CycleOrder::Steps(vec![(1, 0), (0, 0)])
        );
        one.stmts[0].facts.reads.clear();

        // Either kind of root must retain transitive read propagation.
        one.stmts[0].facts.calls.insert(d(1000));
        let (order, scanned) = hd_check::work::count_scan_work(|| cycle_order(&[&one, &two]));
        assert_eq!(order, CycleOrder::Steps(vec![(1, 0), (0, 0)]));
        assert_eq!(scanned, u64::from(width * (depth + 1)));
        one.stmts[0].facts.calls.clear();
        one.stmts[0].facts.dispatch.insert(d(500_000));
        assert_eq!(
            cycle_order(&[&one, &two]),
            CycleOrder::Steps(vec![(1, 0), (0, 0)])
        );
    }
}
