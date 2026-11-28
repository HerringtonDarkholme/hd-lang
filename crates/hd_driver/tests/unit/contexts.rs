//! Algebra and structural-sharing costs of visible-unit contexts.
use std::collections::BTreeSet;

use hd_driver::{testing::UnitSet, work::count_context_work};

fn set(values: impl IntoIterator<Item = u32>) -> UnitSet {
    values
        .into_iter()
        .fold(UnitSet::default(), |s, v| s.union(&UnitSet::singleton(v)))
}

#[test]
fn shared_unit_sets_merge_dense_sparse_and_overlapping_dependencies() {
    let mut seed = 17u32;
    let mut sets = Vec::new();
    for n in 0..40 {
        let mut expected = BTreeSet::new();
        for i in 0..n * 7 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            expected.insert(if n % 2 == 0 { seed } else { i });
        }
        expected.extend([0, 63, 64, 127, u32::MAX, 1 << 31]);
        sets.push((set(expected.iter().copied()), expected));
    }
    for (a, ae) in &sets {
        let reversed = set(ae.iter().rev().copied());
        assert_eq!(
            a.iter().collect::<Vec<_>>(),
            reversed.iter().collect::<Vec<_>>()
        );
        assert_ne!(
            a.iter().collect::<Vec<_>>(),
            UnitSet::default().iter().collect::<Vec<_>>()
        );
        for (b, be) in &sets {
            let expected: Vec<_> = ae.union(be).copied().collect();
            assert_eq!(a.union(b).iter().collect::<Vec<_>>(), expected);
            assert_eq!(b.union(a).iter().collect::<Vec<_>>(), expected);
        }
        assert_eq!(
            a.iter().collect::<Vec<_>>(),
            ae.iter().copied().collect::<Vec<_>>()
        );
    }
}

#[test]
fn extending_contexts_costs_the_added_units_not_copied_closures() {
    let measure = |width: u32, depth: u32| {
        let ((), work) = count_context_work(|| {
            let mut previous = UnitSet::default();
            let mut retained = Vec::new();
            for level in 0..depth {
                let mut next = previous.clone();
                for item in 0..width {
                    next = next.union(&UnitSet::singleton(level * width + item));
                }
                retained.push(next.clone());
                previous = next;
            }
            // Rejoining an identical ancestor must be a pointer hit, not
            // a walk of its members. Retained contexts remain immutable.
            for s in &retained {
                let _ = s.union(s);
            }
        });
        work
    };
    // Unit keys still occupy at most32 bits of the common128-bit trie.
    let path_bound = 2 * u64::from(u32::BITS + 1);
    for depth in [1, 20] {
        for width in [100, 1000] {
            let work = measure(width, depth);
            assert!(
                work.tree > 0
                    && work.tree <= path_bound * u64::from(width * depth) + u64::from(depth),
                "{work:?}"
            );
            assert_eq!(work.items, 0);
        }
    }
}

#[test]
fn dependency_context_folds_are_linear_at_width_1000_and_depth_20() {
    use hd_base::UnitId;
    use hd_project::ModuleGraph;
    let measure = |width: u32, depth: u32| {
        let mut graph = ModuleGraph::default();
        let mut previous = None;
        let mut next = 0u32;
        for _ in 0..depth {
            let start = next;
            for _ in 0..width {
                let id = UnitId::from_raw(next);
                next += 1;
                graph.uses.push(previous.into_iter().collect());
                graph.order.push(id);
            }
            let id = UnitId::from_raw(next);
            next += 1;
            graph
                .uses
                .push((start..start + width).map(UnitId::from_raw).collect());
            graph.order.push(id);
            previous = Some(id);
        }
        let (contexts, work) = count_context_work(|| hd_driver::testing::unit_contexts(&graph));
        assert_eq!(
            contexts.last().map(|s| s.iter().count()),
            Some(graph.uses.len())
        );
        // Counts above exclude this one intentional inspection. Building
        // contexts must never enumerate the dependencies' unit members.
        assert_eq!(work.items, 0);
        let edges = graph.uses.iter().map(Vec::len).sum::<usize>();
        assert!(
            work.graph <= 2 * (graph.uses.len() + edges) as u64,
            "{work:?}"
        );
        work
    };
    let path_bound = 2 * u64::from(u32::BITS - 6 + 1);
    for depth in [1, 20] {
        for width in [100, 1000] {
            let work = measure(width, depth);
            // One insertion and at most two dependency edges per node;
            // each shared-parent union follows at most the u32 key path.
            let vertices = (width + 1) * depth;
            assert!(
                work.tree <= path_bound * u64::from(3 * vertices),
                "{work:?}"
            );
        }
    }
}

fn measured_reader_work(count: u32, with_methods: bool) -> (hd_driver::work::ContextWork, u64) {
    measured_reader_work_kind(count, with_methods, false)
}

fn measured_reader_work_kind(
    count: u32,
    with_methods: bool,
    with_arguments: bool,
) -> (hd_driver::work::ContextWork, u64) {
    use hd_driver::{Executor, Goal, Host, NoClock, build};
    use hd_project::MemorySources;
    use hd_sched::SerialOrder;

    let mut sources = MemorySources::default();
    sources.insert(
        "src/main.hd",
        include_str!("../all/programs/context_work/main.hd"),
    );
    if with_arguments {
        sources.insert(
            "src/definitions.hd",
            include_str!("../all/programs/context_work/arg-definitions.hd"),
        );
    }
    for n in 0..count {
        let text = if n == 0 {
            include_str!("../all/programs/context_work/base.hd").to_owned()
        } else {
            include_str!("../all/programs/context_work/next.hd")
                .replace("pkg.previous", &format!("pkg.m{}", n - 1))
        };
        let text = if with_methods {
            text + "\n" + include_str!("../all/programs/context_work/trait.hd")
        } else {
            text
        };
        let text = if with_arguments {
            include_str!("../all/programs/context_work/arg-imports.hd").to_owned()
                + &text
                + "\n"
                + include_str!("../all/programs/context_work/arg-implementation.hd")
        } else {
            text
        };
        sources.insert(&format!("src/m{n}.hd"), &text);
    }
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let ((out, work), methods) = hd_check::work::count_method_context_steps(|| {
        count_context_work(|| build(&host, "app", &Goal::Analyze))
    });
    assert!(!out.diags.has_errors(), "{}", out.render());
    eprintln!(
        "{count} modules, methods={with_methods}, argument impls={with_arguments}: {work:?}, method steps={methods}"
    );
    (work, methods)
}

#[test]
fn context_work_reader_members_grow_linearly() {
    let zero = measured_reader_work(0, false).0;
    let (small, large) = (
        measured_reader_work(100, false).0,
        measured_reader_work(1000, false).0,
    );
    let (small_graph, large_graph) = (small.graph - zero.graph, large.graph - zero.graph);
    assert!(
        small_graph > 0 && large_graph <= 12 * small_graph,
        "{small:?} -> {large:?}"
    );
    let (small_items, large_items) = (small.items - zero.items, large.items - zero.items);
    assert!(
        small_items > 0 && large_items <= 12 * small_items,
        "reader members: {small:?} -> {large:?}"
    );
}

#[test]
fn shared_contexts_preserve_each_units_exact_reach_in_reverse_query_order() {
    use hd_base::UnitId;
    use hd_project::ModuleGraph;
    // Logical DAG order deliberately differs from unit-ID order. Querying
    // dependents first exercises lazy folding, joins and cached ancestors.
    for round in 0..8u32 {
        let count = 64u32;
        let id = |rank| (rank * 17 + round) % count;
        let mut graph = ModuleGraph {
            uses: vec![Vec::new(); count as usize],
            ..ModuleGraph::default()
        };
        let mut seed = round + 1;
        for rank in 0..count {
            for prior in 0..rank {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if seed % 7 == 0 {
                    graph.uses[id(rank) as usize].push(UnitId::from_raw(id(prior)));
                }
            }
        }
        graph.order = (0..count).rev().map(|r| UnitId::from_raw(id(r))).collect();
        let actual = hd_driver::testing::unit_contexts(&graph);
        for (start, context) in graph.order.iter().zip(actual) {
            let mut expected = BTreeSet::new();
            let mut pending = vec![*start];
            while let Some(unit) = pending.pop() {
                if expected.insert(unit.raw()) {
                    pending.extend_from_slice(&graph.uses[unit.idx()]);
                }
            }
            assert_eq!(
                context.iter().collect::<Vec<_>>(),
                expected.into_iter().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn context_work_with_new_methods_extends_indexes_instead_of_copying_them() {
    let zero = measured_reader_work(0, true).1;
    let (small, large) = (
        measured_reader_work(100, true),
        measured_reader_work(1000, true),
    );
    let (a, b) = (small.1 - zero, large.1 - zero);
    // Every unit adds a trait with the same method name, growing both
    // the declaration map and one candidate bucket. Tenfold input must
    // not rebuild the growing bucket for each inherited context.
    assert!(a > 0 && b <= 20 * a, "method-context steps {a} -> {b}");
}

#[test]
fn context_work_with_argument_owned_impls_shares_candidate_directories() {
    let zero = measured_reader_work_kind(0, false, true);
    let (small, large) = (
        measured_reader_work_kind(100, false, true),
        measured_reader_work_kind(1000, false, true),
    );
    let (a, b) = (small.0.items - zero.0.items, large.0.items - zero.0.items);
    assert!(a > 0 && b <= 12 * a, "reader members {a} -> {b}");
    let (a, b) = (small.1 - zero.1, large.1 - zero.1);
    assert!(
        a > 0 && b <= 20 * a,
        "persistent context-map work {a} -> {b}"
    );
}

fn measured_named_directory_work(count: u32) -> u64 {
    use hd_driver::{Executor, Goal, Host, NoClock, build};
    use hd_project::MemorySources;
    use hd_sched::SerialOrder;
    let mut sources = MemorySources::default();
    sources.insert(
        "src/main.hd",
        include_str!("../all/programs/context_work/main.hd"),
    );
    let mut definitions = String::new();
    for n in 0..count {
        definitions.push_str(
            &include_str!("../all/programs/context_work/named-trait.hd")
                .replace("Pick", &format!("Pick{n}"))
                .replace("pick", &format!("pick{n}")),
        );
    }
    definitions.push_str(include_str!("../all/programs/context_work/named-types.hd"));
    sources.insert("src/definitions.hd", &definitions);
    for n in 0..count {
        let step = if n == 0 {
            include_str!("../all/programs/context_work/base.hd").to_owned()
        } else {
            include_str!("../all/programs/context_work/next.hd")
                .replace("pkg.previous", &format!("pkg.m{}", n - 1))
        };
        let module = include_str!("../all/programs/context_work/arg-imports.hd").to_owned()
            + &step
            + "\n"
            + include_str!("../all/programs/context_work/arg-implementation.hd")
            + "\n"
            + include_str!("../all/programs/context_work/named-call.hd");
        sources.insert(
            &format!("src/m{n}.hd"),
            &module
                .replace("Pick", &format!("Pick{n}"))
                .replace("pick", &format!("pick{n}")),
        );
    }
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let ((out, lookup_work), map_work) = hd_check::work::count_method_context_steps(|| {
        hd_check::work::count_trait_lookup_work(|| build(&host, "app", &Goal::Analyze))
    });
    let work = lookup_work + map_work;
    assert!(!out.diags.has_errors(), "{}", out.render());
    eprintln!("{count} argument-owned trait/method calls: {work} name lookup steps");
    work
}

#[test]
fn context_work_named_argument_directories_do_not_reindex_inherited_traits() {
    let zero = measured_named_directory_work(0);
    let small = measured_named_directory_work(100) - zero;
    let large = measured_named_directory_work(1000) - zero;
    assert!(
        small > 0 && large <= 15 * small,
        "trait-name work {small} -> {large}"
    );
}
