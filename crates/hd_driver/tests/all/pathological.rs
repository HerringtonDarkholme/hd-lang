//! Generated complexity guards for #1531. The counter belongs to the stage
//! under test. Sizes include the historical stack and retry regressions.

use std::fmt::Write as _;

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

// Checker: #1529, lexical depth must not multiply omitted-row body checks.
/// The issue's `gen.py nest N`, kept here so the test is self-contained.
fn nested(depth: usize) -> String {
    let mut out = String::from("pub fn main() -> void $ Console:\n");
    for i in 0..depth {
        let indent = "    ".repeat(i + 1);
        let _ = write!(
            out,
            "{indent}fn g{i}(k: i32) -> void:\n{indent}    if k > 0:\n{indent}        g{i}(k - 1)\n"
        );
    }
    let _ = writeln!(out, "{}println(\"deep $k\")", "    ".repeat(depth + 1));
    for i in (1..depth).rev() {
        let _ = writeln!(out, "{}g{i}(1)", "    ".repeat(i + 1));
    }
    out.push_str("    g0(1)\n");
    out
}

fn wide(width: usize) -> String {
    let mut out = String::from("pub fn main() -> void $ Console:\n");
    for i in 0..width {
        let _ = write!(
            out,
            "    fn g{i}(k: i32) -> void:\n        if k > 0:\n            g{i}(k - 1)\n        println(k)\n    g{i}(0)\n"
        );
    }
    out
}

fn compile(source: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn check_local_row_work(source: &str, bodies: usize) {
    let (out, counts) = hd_check::work::count_local_body_checks(|| compile(source, &Goal::Analyze));
    assert!(out.diags.is_empty(), "{}", out.render());
    let counts: Vec<_> = counts
        .iter()
        .filter(|c| c.owner.starts_with("app/"))
        .collect();
    assert_eq!(counts.len(), bodies);
    for c in &counts {
        assert!(
            c.checks <= 2,
            "{} {:?}: {} body checks",
            c.owner,
            c.at,
            c.checks
        );
    }
    assert_eq!(counts.iter().map(|c| c.checks).sum::<usize>(), 2 * bodies);
}

#[test]
fn nested_recursive_bodies_are_checked_twice_independent_of_depth() {
    for depth in [5, 20, 80] {
        check_local_row_work(&nested(depth), depth);
    }
}

#[test]
fn recursive_body_checks_grow_linearly_with_width() {
    for width in [100, 1000] {
        check_local_row_work(&wide(width), width);
    }
}

#[test]
fn generic_ancestor_capture_dependencies_do_not_retry_subtrees() {
    for depth in [5, 20, 80] {
        let mut source = nested(depth);
        for i in (0..depth).rev() {
            source = source.replace(&format!("fn g{i}(k:"), &format!("fn g{i}[T{i}](k:"));
            source = source.replace(&format!("g{i}("), &format!("g{i}::[i32]("));
            if i > 0 {
                let header = format!("{}fn g{i}[T{i}](k: i32) -> void:\n", "    ".repeat(i + 1));
                let use_parent = format!("{}g{}::[i32](0)\n", "    ".repeat(i + 2), i - 1);
                source = source.replace(&header, &format!("{header}{use_parent}"));
            }
        }
        check_local_row_work(&source, depth);
    }
}

// Checker: #1530, cyclic function-value rows use an explicit worklist.
/// The issue's `gen.py chain N`, copied here without an external script.
fn ring(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        let _ = writeln!(out, "fn h{i}(k: i32) -> void:");
        if i == n - 1 {
            let _ = writeln!(out, "    println(\"h{i} $k\")");
        }
        let next = (i + 1) % n;
        let _ = write!(
            out,
            "    if k > 0:\n        next := h{next}\n        next(k - 1)\n\n"
        );
    }
    out.push_str("pub fn main() -> void $ Console:\n    h0(3)\n");
    out
}

#[test]
fn row_signature_rings_have_linear_body_work_and_constant_active_depth() {
    for n in [100, 400, 1000, 2000] {
        let (out, counts) =
            hd_check::work::count_item_body_checks(|| compile(&ring(n), &Goal::Analyze));
        assert!(out.diags.is_empty(), "ring {n}: {}", out.render());
        let counts: Vec<_> = counts
            .iter()
            .filter(|c| c.owner.starts_with("app/"))
            .collect();
        assert_eq!(
            counts.len(),
            n + 1,
            "ring {n}: one record per source function"
        );
        for c in &counts {
            assert!(
                c.checks <= 2,
                "ring {n}: {} checked {} times",
                c.owner,
                c.checks
            );
            assert!(
                c.max_active_depth <= 1,
                "ring {n}: {} at depth {}",
                c.owner,
                c.max_active_depth
            );
        }
        assert_eq!(
            counts.iter().map(|c| c.checks).sum::<usize>(),
            2 * n + 1,
            "ring {n}"
        );
    }
}

// Checker: #1774, a chain of omitted results checks callees first from
// any entry, so its body checks do not nest.
/// `g_0` calls `g_1` and so on, each without a result type; `g_n` reads
/// the top-level `z` when `reads_top`. The chain is entered from a
/// top-level statement, or from `main`.
fn omitted_result_chain(n: usize, from_main: bool, reads_top: bool) -> String {
    let mut s = String::from("let z: i32 = 1\n");
    for i in 0..n {
        let _ = writeln!(s, "fn g_{i}(x: i32): g_{}(x)", i + 1);
    }
    let _ = writeln!(
        s,
        "fn g_{n}(x: i32): x{}",
        if reads_top { " + z" } else { "" }
    );
    if from_main {
        s.push_str("pub fn main() -> void $ Console:\n    println(g_0(1))\n");
    } else {
        s.push_str("y := g_0(+1)\nprintln(y)\n");
    }
    s
}

#[test]
fn omitted_result_chains_check_once_per_function_at_depth_one() {
    for (from_main, reads_top) in [(false, false), (false, true), (true, false)] {
        // 500 overflowed from a top-level statement; 10x and 20x that.
        for n in [500, 5000, 10_000] {
            let (out, counts) = hd_check::work::count_item_body_checks(|| {
                compile(
                    &omitted_result_chain(n, from_main, reads_top),
                    &Goal::Analyze,
                )
            });
            assert!(!out.diags.has_errors(), "chain {n}: {}", out.render());
            let chain: Vec<_> = counts.iter().filter(|c| c.owner.contains("/g_")).collect();
            assert_eq!(chain.len(), n + 1, "chain {n}: one record per function");
            for c in &chain {
                assert_eq!(
                    c.checks, 1,
                    "chain {n}: {} checked {} times",
                    c.owner, c.checks
                );
                assert_eq!(
                    c.max_active_depth, 1,
                    "chain {n}: {} checked at depth {}",
                    c.owner, c.max_active_depth
                );
            }
        }
    }
}

// Checker: #1824, a body never checks another inside its own check: one
// that reads a pending callable waits for it on an explicit stack. A cycle
// of omitted results, and a chain whose tail reads a top-level binding
// declared after the statement that calls it, report their one error with
// each body checked at most twice, at depth one.
/// `g_0` calls `g_1` and so on, and the last calls `g_0`.
fn omitted_result_cycle(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = writeln!(s, "fn g_{i}(x: i32): g_{}(x)", (i + 1) % n);
    }
    s.push_str("pub fn main() -> void $ Console:\n    println(g_0(1))\n");
    s
}

/// A chain of `n` called from a statement before `z` is declared; its
/// tail reads `z`, so the chain waits for that statement's call.
fn omitted_result_chain_reading_later_top(n: usize) -> String {
    let mut s = String::from("y := g_0(+1)\nprintln(y)\nlet z: i32 = 1\n");
    for i in 0..n {
        let _ = writeln!(s, "fn g_{i}(x: i32): g_{}(x)", i + 1);
    }
    let _ = writeln!(s, "fn g_{n}(x: i32): x + z");
    s
}

/// A valid chain that waits: its tail names `z`, an unannotated top-level
/// binding declared after the statement that calls `g_0`, so no body of
/// the chain is checked before that call. The tail's `z` is its own
/// local, so each body waits for its callee once and the program checks.
fn omitted_result_chain_naming_later_top(n: usize) -> String {
    let mut s = String::from("y := g_0(+1)\nprintln(y)\nz := 1\n");
    for i in 0..n {
        let _ = writeln!(s, "fn g_{i}(x: i32): g_{}(x)", i + 1);
    }
    let _ = writeln!(s, "fn g_{n}(x: i32):\n    z := x + 1\n    z");
    s
}

#[test]
fn waiting_bodies_check_at_most_twice_at_depth_one() {
    type Shape = (fn(usize) -> String, usize, &'static [&'static str]);
    let shapes: [Shape; 3] = [
        (
            omitted_result_cycle,
            0,
            &["recursive-function-needs-result-type"],
        ),
        (
            omitted_result_chain_reading_later_top,
            1,
            &["top-level-read-before-initialization"],
        ),
        (omitted_result_chain_naming_later_top, 1, &[]),
    ];
    for (generate, extra, code) in shapes {
        // 10,000 aborted on the native stack before the explicit one.
        for n in [1000, 10_000] {
            let (out, counts) =
                hd_check::work::count_item_body_checks(|| compile(&generate(n), &Goal::Analyze));
            let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
            assert_eq!(codes, code, "{code:?} {n}: {}", out.render());
            let chain: Vec<_> = counts.iter().filter(|c| c.owner.contains("/g_")).collect();
            assert_eq!(
                chain.len(),
                n + extra,
                "{code:?} {n}: one record per function"
            );
            for c in &chain {
                assert!(
                    c.checks <= 2 && c.max_active_depth == 1,
                    "{code:?} {n}: {} checked {} times, at depth {}",
                    c.owner,
                    c.checks,
                    c.max_active_depth
                );
            }
            // The valid chain really waits: each body but the tail is
            // dropped once for its pending callee and checked again.
            if code.is_empty() {
                let checks: usize = chain.iter().map(|c| c.checks).sum();
                assert_eq!(checks, 2 * n + 1, "{n}: the chain's body checks");
            }
        }
    }
}

// Checker: #1776, each alias of a chain checks its own right-hand side,
// and the walk keeps its own stack. Past `TYPE_DEPTH` levels a chain is
// `type-too-large` (`types.limit.type-size.error`), and the work stays
// linear and the stack bounded up to 20,000 aliases.
/// `type A1 = List[A0]`, `type A2 = List[A1]`, and so on, read by `f`;
/// each `pub` when `public`.
fn alias_chain(n: u32, public: bool) -> String {
    let vis = if public { "pub " } else { "" };
    let mut s = format!("{vis}type A0 = i32\n");
    for i in 1..=n {
        let _ = writeln!(s, "{vis}type A{i} = List[A{}]", i - 1);
    }
    let _ = writeln!(s, "{vis}fn f(x: A{n}) -> A{n}: x");
    s
}

/// #1984's program: `type A0[S < Source] = S::Item`, then `type A{i}[S <
/// Source] = (A{i-1}[S], A{i-1}[S])`, and a function taking and returning
/// `A{n}[S]`. The source grows linearly in `n`; the type doubles.
fn param_alias_chain(n: u32) -> String {
    let mut s = String::from("trait Source:\n    type Item\n\ntype A0[S < Source] = S::Item\n");
    for i in 1..=n {
        let _ = writeln!(s, "type A{i}[S < Source] = (A{}[S], A{}[S])", i - 1, i - 1);
    }
    let _ = writeln!(s, "fn id[S < Source](s: S, v: A{n}[S]) -> A{n}[S]: v");
    s
}

// Resolver and checker: #1984, a chain of generic aliases doubles its
// type at each level. An alias names its parameters by position, so
// `A{i}[S]` inside `A{i+1}`'s body is `A{i}`'s body itself, with no walk,
// and `A{i+1}` interns one new node: the pool holds the chain's `n`
// nodes, not `n²/2` copies with each alias's own parameter. Each walk
// (the slot paths, the type nodes substituted, the checker's projection
// walks and its header check, kept across aliases with the same bounds)
// enters each distinct node once overall: 60 levels did not finish in
// 60 s; 200 levels took 20,907 header types, 70x the 20 levels'.
#[test]
fn a_generic_alias_chain_walks_each_distinct_node_once() {
    on_test_stack(|| {
        let work = |n| {
            let ((((out, headers), walked), folded), substituted) =
                hd_types::work::count_type_nodes(|| {
                    hd_resolve::work::count_slot_path_nodes(|| {
                        hd_check::work::count_slot_counts(|| {
                            hd_check::work::count_header_types(|| {
                                compile(&param_alias_chain(n), &Goal::Analyze)
                            })
                        })
                    })
                });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            [folded, walked, headers, substituted]
        };
        work(1);
        let (small, large) = (work(20), work(200));
        for (i, what) in [
            "slot path nodes",
            "projection walk nodes",
            "header types",
            "type nodes",
        ]
        .into_iter()
        .enumerate()
        {
            let (a, b) = (small[i], large[i]);
            assert!(
                a > 0 && b <= a * 10 * 5 / 4,
                "{what} for 20 and 200 levels: {a} and {b}"
            );
        }
    });
}

// Solver: a concrete projection's normal form is memoized per (part,
// binding depth) within a query, so #1984's chain instantiated at a
// concrete type, `A{n}[Src]`, whose `Src::Item` leaves the pool shares
// along 2^n paths, normalizes each distinct part once. Unmemoized, 24
// levels took 11.6 s and 200 did not finish.
#[test]
fn a_concrete_alias_chain_normalizes_each_shared_part_once() {
    on_test_stack(|| {
        let work = |n| {
            let src = param_alias_chain(n).replace(
                "fn id[",
                "struct Src: pass\n\nimpl Source for Src:\n    type Item = i32\n\nfn id[",
            );
            let src = format!("{src}fn concrete(x: A{n}[Src]) -> void: pass\n");
            let (out, nodes) = hd_types::work::count_type_nodes(|| compile(&src, &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            nodes
        };
        work(1);
        let (small, large) = (work(20), work(200));
        assert!(
            small > 0 && large <= small * 10 * 5 / 4,
            "type nodes for 20 and 200 levels: {small} and {large}"
        );
    });
}

// Resolver and checker: #2060, a bounded alias's uses are kept by the
// item that writes them, each distinct (alias, arguments) once, and
// stage B asks their bounds there: #1984's chain with one use whose
// argument misses the bound reports that use once, and the uses asked
// and the header types grow with the chain, not with its doubled type.
#[test]
fn a_bounded_alias_chain_asks_each_use_once() {
    on_test_stack(|| {
        let work = |n| {
            let src = format!(
                "{}fn bad(x: A{n}[i32]) -> void: pass\n",
                param_alias_chain(n)
            );
            let ((out, headers), uses) = hd_check::work::count_alias_uses(|| {
                hd_check::work::count_header_types(|| compile(&src, &Goal::Analyze))
            });
            let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
            assert_eq!(codes, ["unsatisfied-trait-bound"], "{n}: {}", out.render());
            [uses, headers]
        };
        work(1);
        let (small, large) = (work(20), work(200));
        for (i, what) in ["alias uses", "header types"].into_iter().enumerate() {
            let (a, b) = (small[i], large[i]);
            assert!(
                a > 0 && b <= a * 10 * 5 / 4,
                "{what} for 20 and 200 levels: {a} and {b}"
            );
        }
    });
}

/// The header types an alias chain of `n` checks, and its codes.
fn alias_chain_work(n: u32) -> (u64, Vec<String>) {
    alias_chain_work_of(n, false)
}

fn alias_chain_work_of(n: u32, public: bool) -> (u64, Vec<String>) {
    let (out, types) =
        hd_check::work::count_header_types(|| compile(&alias_chain(n, public), &Goal::Analyze));
    let mut codes: Vec<String> = out
        .diags
        .code
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect();
    codes.dedup();
    (types, codes)
}

#[test]
fn alias_chain_header_types_grow_linearly() {
    // The first build also checks the standard library's items.
    alias_chain_work(1);
    // 20,000 aborted after 33 s; 2,000 took 0.61 s.
    let (small, small_codes) = alias_chain_work(2000);
    let (large, large_codes) = alias_chain_work(20_000);
    assert_eq!(small_codes, ["type-too-large"]);
    assert_eq!(large_codes, ["type-too-large"]);
    assert!(
        small > 0 && large <= 10 * small + 100,
        "header types for 2,000 and 20,000 aliases: {small} and {large}",
    );
}

// Resolver: #2135, the alias and supertrait components are one iterative
// walk (`hd_base::scc`). Declared top first, a 20,000-alias chain is a
// 20,000-deep walk from its first item: it checks on a 256 KiB stack.
#[test]
fn a_top_first_alias_chain_checks_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(|| {
            let n = 20_000;
            let mut src = String::new();
            for i in (1..=n).rev() {
                let _ = writeln!(src, "type A{i} = List[A{}]", i - 1);
            }
            let _ = writeln!(src, "type A0 = i32\nfn f(x: A{n}) -> A{n}: x");
            let out = compile(&src, &Goal::Analyze);
            let mut codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
            codes.dedup();
            assert_eq!(codes, ["type-too-large"], "{}", out.render());
        })
        .expect("thread")
        .join()
        .expect("no stack overflow");
}

// Checker: #2161, the type walks a value's conversion takes keep their own
// stacks: a value that weakens `*mut i32` to `*i32` under 990 levels of
// `List` checks on a 96 KiB stack, through the variance walk
// (`Ck::variance_fits`, `readonly_reference_fits`, `perm_fits`), the
// nested rows (`Ck::solve_nested_rows`) and the escape check
// (`local_refs::escape_in`). A program 5 levels deep needs the same stack.
#[test]
fn a_variance_at_the_type_depth_checks_on_a_small_stack() {
    let n = 990;
    let mut src = String::from("type A0 = *mut i32\ntype B0 = *i32\n");
    for i in 1..=n {
        let _ = writeln!(
            src,
            "type A{i} = List[A{}]\ntype B{i} = List[B{}]",
            i - 1,
            i - 1
        );
    }
    let _ = writeln!(src, "fn f(x: A{n}) -> B{n}: x");
    std::thread::Builder::new()
        .stack_size(96 << 10)
        .spawn(move || {
            let out = compile(&src, &Goal::Analyze);
            assert!(out.diags.code.is_empty(), "{}", out.render());
        })
        .expect("thread")
        .join()
        .expect("no stack overflow");
}

// Checker: #1825, a public alias chain's interface and signature walks
// (`InterfaceCheck`, `ItemCheck::private_in`) keep each type's answer
// across the items of a pass, with their own stacks, so each alias costs
// its own type, not its expansion again: 20,000 took 19 s.
#[test]
fn public_alias_chain_header_types_grow_linearly() {
    alias_chain_work_of(1, true);
    let (small, small_codes) = alias_chain_work_of(2000, true);
    let (large, large_codes) = alias_chain_work_of(20_000, true);
    assert_eq!(small_codes, ["type-too-large"]);
    assert_eq!(large_codes, ["type-too-large"]);
    assert!(
        small > 0 && large <= 10 * small + 100,
        "header types for 2,000 and 20,000 public aliases: {small} and {large}",
    );
}

#[test]
fn an_alias_chain_at_the_type_depth_checks_and_one_more_is_too_large() {
    use hd_base::limits::TYPE_DEPTH;
    assert_eq!(alias_chain_work(TYPE_DEPTH).1, Vec::<String>::new());
    assert_eq!(alias_chain_work(TYPE_DEPTH + 1).1, ["type-too-large"]);
}

// Checker: #1828, the value rule (`recursive-value-type`) decides each
// declaration once per run (`hd_check::value::InlineDecls`, #2011), so a
// chain of structs held inline costs its length, not its length per
// struct.
/// `struct C0: x: i32`, `struct C1: next: C0`, and so on.
fn struct_chain(n: u32) -> String {
    let mut s = String::from("struct C0:\n    x: i32\n");
    for i in 1..n {
        let _ = writeln!(s, "\nstruct C{i}:\n    next: C{}", i - 1);
    }
    s
}

/// The types the value rule enters for a chain of `n` structs.
fn struct_chain_work(n: u32) -> u64 {
    let (out, types) =
        hd_check::work::count_held_types(|| compile(&struct_chain(n), &Goal::Analyze));
    assert!(out.diags.is_empty(), "chain {n}: {}", out.render());
    types
}

#[test]
fn struct_chain_containment_grows_linearly() {
    // The first build also checks the standard library's items.
    struct_chain_work(1);
    // Each struct's walk went down the whole chain before the walk kept
    // its finite types across the pass: n² / 2 types.
    let small = struct_chain_work(2000);
    let large = struct_chain_work(20_000);
    assert!(
        small >= 2000 && large <= 10 * small + 100,
        "types entered for 2,000 and 20,000 chained structs: {small} and {large}",
    );
}

// Checker: #2008, which declarations hold themselves inline
// (`hd_check::value::holds_itself`) is decided once per declaration per
// run, and every body reads the run's decisions: a chain of generic
// structs, each instance used in its own body, costs its length, not its
// length per body that asks.
/// `struct C0[T]: x: T`, `struct C1[T]: next: C0[T]`, and so on, with
/// `fn use{i}()` holding a `C{i}[i32]?`.
fn generic_chain(n: u32) -> String {
    let mut s = String::from("struct C0[T]:\n    x: T\n");
    for i in 1..n {
        let _ = writeln!(s, "\nstruct C{i}[T]:\n    next: C{}[T]", i - 1);
    }
    for i in 0..n {
        let _ = writeln!(s, "\nfn use{i}():\n    let v: C{i}[i32]? = .None");
    }
    s
}

#[test]
fn generic_chain_used_by_every_body_grows_linearly() {
    let work = |n| {
        let (out, types) =
            hd_check::work::count_held_types(|| compile(&generic_chain(n), &Goal::Analyze));
        assert!(!out.diags.has_errors(), "chain {n}: {}", out.render());
        types
    };
    work(1);
    let (small, large) = (work(500), work(5000));
    assert!(
        small >= 500 && large <= 10 * small + 100,
        "types entered for 500 and 5,000 chained generic structs: {small} and {large}",
    );
}

// Checker: #2008, a body that holds an instance of a declaration that
// holds itself with growing arguments (`struct G[T]: x: G[(T, T)]`) does
// not walk the instance's fields: they never end, and the walk spent the
// body's whole fuel, then reported `item-too-complex` beside the
// declaration's `recursive-value-type`.
fn growing_users(n: usize) -> String {
    let mut s = String::from("struct G[T]:\n    x: G[(T, T)]\n");
    for i in 0..n {
        let _ = writeln!(
            s,
            "\nfn use{i}():\n    let v: G[i32]? = .None\n    let w = v\n    let u = v"
        );
    }
    s
}

#[test]
fn bodies_holding_a_growing_instance_cost_one_walk_each() {
    let work = |n| {
        let (out, checks) =
            hd_check::work::count_item_body_checks(|| compile(&growing_users(n), &Goal::Analyze));
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            codes,
            ["recursive-value-type"],
            "{n} bodies: {}",
            out.render()
        );
        let spent: Vec<u64> = checks
            .iter()
            .filter(|c| c.owner.starts_with("app/"))
            .map(|c| c.fuel_spent)
            .collect();
        assert_eq!(spent.len(), n);
        spent.into_iter().max().unwrap_or(0)
    };
    // Each body's fuel is its own few steps, whatever the number of
    // bodies; the walk of `G[i32]` alone spent the default fuel before.
    let (small, large) = (work(10), work(100));
    assert!(
        small < 200 && large <= small,
        "most fuel one body spent with 10 and 100 bodies: {small} and {large}",
    );
}

// Solver: #2013, a generic call whose argument is an instance of a
// declaration that holds itself (`keep(h)` with `h: G[i32]?`) asks `Copy`
// of it, and the row's walk of the instance's fields never ended. The row
// stops at such an instance (`hd_check::value::SolverDecls`), as the
// ownership walk does: the proof costs a few types once per run.
/// `struct G[T]` with two fields that grow its argument, `struct H`
/// holding `G[i32]`, and `n` bodies passing a `G[i32]?` and an `H?` to a
/// generic function.
fn growing_generic_calls(n: usize) -> String {
    let mut s = String::from(
        "struct G[T]:\n    x: G[(T, T)]\n    y: G[(T, bool)]\n\nstruct H:\n    g: G[i32]\n\
         \nfn keep[T](t: T) -> T:\n    t\n",
    );
    for i in 0..n {
        let _ = writeln!(
            s,
            "\nfn use{i}():\n    let g: G[i32]? = .None\n    let h: H? = .None\n    \
             let u = keep(g)\n    let v = keep(h)"
        );
    }
    s
}

#[test]
fn generic_calls_on_a_growing_instance_prove_copy_once_per_run() {
    let work = |n| {
        let (out, nodes) =
            hd_types::work::count_type_nodes(|| compile(&growing_generic_calls(n), &Goal::Analyze));
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            codes,
            ["recursive-value-type"],
            "{n} bodies: {}",
            out.render()
        );
        nodes
    };
    work(1);
    // The run's memo keeps each closed proof, so the bodies after the
    // first add nothing: the walk entered `G[i32]`'s fields without end.
    let (small, large) = (work(100), work(1000));
    assert!(
        small > 0 && large <= small + 100,
        "type nodes for 100 and 1,000 bodies with generic calls: {small} and {large}",
    );
}

// Checker: #2011, the value rule decides each declaration from its
// written fields (`hd_check::value::InlineDecls`), never from its
// instances: `k` fields that grow a struct's argument `k` ways give `k^d`
// distinct instances `d` levels down, and the header walked them to the
// type depth limit.
/// `struct G[T]` with `k` fields `f{i}: G[(T, W{i})]`, a struct holding
/// `G[i32]`, and a body holding one.
fn growing_fields(k: usize) -> String {
    let mut s = String::new();
    for i in 0..k {
        let _ = writeln!(s, "struct W{i}:\n    x: i32\n");
    }
    s.push_str("struct G[T]:\n");
    for i in 0..k {
        let _ = writeln!(s, "    f{i}: G[(T, W{i})]");
    }
    s.push_str("\nstruct H:\n    g: G[i32]\n\nfn main():\n    let h: H? = .None\n");
    s
}

/// The types the value rule enters for `source`, which has one
/// `recursive-value-type`.
fn one_recursive_value_work(source: &str) -> u64 {
    let (out, types) = hd_check::work::count_held_types(|| compile(source, &Goal::Analyze));
    let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
    assert_eq!(codes, ["recursive-value-type"], "{}", out.render());
    types
}

#[test]
fn growing_fields_cost_their_count_not_their_instances() {
    one_recursive_value_work(&growing_fields(1));
    let work: Vec<u64> = (2..=4)
        .map(|k| one_recursive_value_work(&growing_fields(k)))
        .collect();
    // Each field adds its struct `W{i}` and a few types of its own.
    assert!(
        work.windows(2).all(|w| w[1] > w[0] && w[1] <= w[0] + 16),
        "types entered for 2, 3 and 4 growing fields: {work:?}",
    );
}

/// A ring of `n` structs, `G{i}[T]` holding `G{i+1}[(T, T)]` and
/// `G{i+1}[(T, bool)]`, the last holding `G0`'s: one group.
fn growing_ring(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let next = (i + 1) % n;
        let _ = writeln!(
            s,
            "struct G{i}[T]:\n    x: G{next}[(T, T)]\n    y: G{next}[(T, bool)]\n"
        );
    }
    s
}

#[test]
fn a_growing_group_costs_its_size() {
    one_recursive_value_work(&growing_ring(1));
    let (small, large) = (
        one_recursive_value_work(&growing_ring(200)),
        one_recursive_value_work(&growing_ring(2000)),
    );
    assert!(
        small >= 200 && large <= 10 * small + 100,
        "types entered for groups of 200 and 2,000 growing structs: {small} and {large}",
    );
}

// Checker: #1830, the value rule decides each declaration once per run by
// its component (`hd_check::value::InlineDecls`, #2011), so a ring of n
// structs, a chain of n structs into a two-struct ring, and a comb of n
// rings hung on a chain each enter their size in types, not a re-walk of
// the chain per struct. Each source grows linearly in n.
/// `struct C{i}: next: C{i+1}`, the last holding `C0`: one ring of `n`.
fn struct_ring(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = writeln!(s, "struct C{i}:\n    next: C{}\n", (i + 1) % n);
    }
    s
}

/// A chain `C{i}: next: C{i+1}` of `n` structs whose last holds `A`, and
/// the ring `A: b: B`, `B: a: A`.
fn chain_into_ring(n: usize) -> String {
    let mut s = String::from("struct A:\n    b: B\n\nstruct B:\n    a: A\n\n");
    for i in 0..n {
        let next = if i + 1 == n {
            "A".to_owned()
        } else {
            format!("C{}", i + 1)
        };
        let _ = writeln!(s, "struct C{i}:\n    next: {next}\n");
    }
    s
}

/// A chain `C{i}: next: C{i+1}, ring: A{i}` of `n` structs, each also
/// holding its own ring `A{i}: b: B{i}`, `B{i}: a: A{i}`.
fn ring_comb(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = writeln!(
            s,
            "struct A{i}:\n    b: B{i}\n\nstruct B{i}:\n    a: A{i}\n"
        );
        if i + 1 == n {
            let _ = writeln!(s, "struct C{i}:\n    ring: A{i}\n");
        } else {
            let _ = writeln!(s, "struct C{i}:\n    next: C{}\n    ring: A{i}\n", i + 1);
        }
    }
    s
}

/// The types the value rule enters for `source`, which reports
/// `recursive-value-type` `reports` times.
fn recursive_values_work(source: &str, reports: usize) -> u64 {
    let (out, types) = hd_check::work::count_held_types(|| compile(source, &Goal::Analyze));
    let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        codes,
        vec!["recursive-value-type"; reports],
        "{}",
        out.render()
    );
    types
}

#[test]
fn rings_below_chains_cost_their_size() {
    on_small_stack(rings_below_chains_cost_their_size_on_this_stack);
}

fn rings_below_chains_cost_their_size_on_this_stack() {
    recursive_values_work(&struct_ring(2), 1);
    // Whether each struct of the shape hangs a ring of its own: the comb
    // reports each ring.
    for (name, generate, per_struct) in [
        ("ring", struct_ring as fn(usize) -> String, false),
        ("chain into a ring", chain_into_ring, false),
        ("comb of rings", ring_comb, true),
    ] {
        let reports = |n: usize| if per_struct { n } else { 1 };
        let small = recursive_values_work(&generate(200), reports(200));
        let large = recursive_values_work(&generate(2_000), reports(2_000));
        assert!(
            small >= 200 && large <= 10 * small * 5 / 4,
            "types entered for the {name} of 200 and 2,000: {small} and {large}",
        );
    }
}

/// Checker: fuel spent entering expressions and solving body obligations.
fn check_fuel(generate: fn(usize) -> String, small: usize, large: usize) {
    let work = |n| {
        let (out, checks) =
            hd_check::work::count_item_body_checks(|| compile(&generate(n), &Goal::Analyze));
        assert!(!out.diags.has_errors(), "size {n}: {}", out.render());
        checks
            .iter()
            .filter(|c| c.owner.starts_with("app/"))
            .map(|c| c.fuel_spent)
            .sum::<u64>()
    };
    let (before, after) = (work(small), work(large));
    assert!(
        before > 0 && after > before && after * small as u64 <= 5 * before * large as u64 / 4,
        "checker fuel for {small} and {large}: {before} and {after}",
    );
}

/// A function with `n` parameters, each bounded by a trait whose
/// associated type has the trait as its bound again.
fn self_recursive_bounds(n: usize) -> String {
    let mut s = String::from(
        "trait Walk:\n    type Next < Walk\n    fn step(self) -> Self::Next\n\nfn run[",
    );
    for i in 0..n {
        let _ = write!(s, "{}T{i} < Walk", if i == 0 { "" } else { ", " });
    }
    s.push_str("](value: T0) -> T0::Next:\n    value.step()\n");
    s
}

/// `trait.assoc.bound-use` is elaborated per projection a body names, so
/// building the environments costs their declared bounds: four clauses
/// per bound today (the bound and `Copy`, in the header and the body),
/// never one per projection depth (eager elaboration to depth 16 made 64).
#[test]
fn self_recursive_bound_environment_clauses() {
    let work = |n| {
        let (out, clauses) = hd_check::work::count_env_clauses(|| {
            compile(&self_recursive_bounds(n), &Goal::Analyze)
        });
        assert!(!out.diags.has_errors(), "size {n}: {}", out.render());
        clauses
    };
    // The first build also checks the standard library's items.
    work(1);
    let (small, large) = (work(50), work(200));
    assert!(
        large > small && large - small <= 8 * 150,
        "environment clauses for 50 and 200 bounds: {small} and {large}",
    );
}

/// A local generic header of `n` parameter pairs, each `T{i}` bounded by
/// `Marker[A{i}]`, or `n` local generic functions nested in each other,
/// each with one pair.
fn local_generic_headers(n: usize, nested: bool) -> String {
    let mut s = String::from("trait Marker[A]:\n    pass\n\nfn inspect() -> void:\n");
    if nested {
        for i in 0..n {
            let indent = "    ".repeat(i + 1);
            let _ = writeln!(
                s,
                "{indent}fn g{i}[A{i}, T{i} < Marker[A{i}]](value: T{i}) -> void:"
            );
        }
        for i in (0..n).rev() {
            let _ = writeln!(s, "{}pass\n", "    ".repeat(i + 2));
        }
    } else {
        s.push_str("    fn choose[");
        for i in 0..n {
            let _ = write!(
                s,
                "{}A{i}, T{i} < Marker[A{i}]",
                if i == 0 { "" } else { ", " }
            );
        }
        s.push_str("](value: T0) -> void:\n        pass\n\n");
    }
    s.push_str("    pass\n");
    s
}

/// #2257: a local generic header keys its environment as its bounds come
/// in, linking each clause once: environment-key work grows with the
/// bounds, never once per bound over the bounds before it.
#[test]
fn local_generic_header_environment_keys_grow_linearly() {
    let work = |n, nested| {
        let ((out, clauses), links) = hd_types::work::count_env_links(|| {
            hd_check::work::count_env_clauses(|| {
                compile(&local_generic_headers(n, nested), &Goal::Analyze)
            })
        });
        assert!(!out.diags.has_errors(), "size {n}: {}", out.render());
        (clauses, links)
    };
    // The first build also checks the standard library's items.
    work(1, false);
    for (nested, small, large) in [(false, 100, 1000), (true, 10, 20)] {
        let ((c0, l0), (c1, l1)) = (work(small, nested), work(large, nested));
        assert!(
            l1 > l0 && l1 * small as u64 <= 5 * l0 * large as u64 / 4,
            "key links for {small} and {large}: {l0} and {l1}",
        );
        assert!(
            c1 * small as u64 <= 5 * c0 * large as u64 / 4,
            "clauses for {small} and {large}: {c0} and {c1}",
        );
    }
}

/// A function value copied through `depth` bindings, then passed `width`
/// times for a parameter whose function type writes a lifetime and
/// returned at the result's.
fn fn_value_uses(width: usize, depth: usize) -> String {
    let mut s = String::from(
        "fn loose['a](value: &'a i32) -> void:\n    pass\n\nlet shared: i32 = 5\n\n\
         fn apply['a](f: fn(&'a i32) -> void, value: &'a i32) -> void:\n    f(value)\n\n\
         fn run['a]() -> fn(&'a i32) -> void:\n    g0 := loose\n",
    );
    for i in 1..=depth {
        let _ = writeln!(s, "    g{i} := g{}", i - 1);
    }
    for _ in 0..width {
        let _ = writeln!(s, "    apply(g{depth}, &shared)");
    }
    let _ = writeln!(s, "    g{depth}");
    s
}

/// #2258: what a function value needs at its inputs is traced once per
/// local part, however many arguments and results read it: the work grows
/// with the copies, not with the uses times the copies.
#[test]
fn function_value_input_needs_are_traced_once_per_local() {
    let work = |width, depth| {
        let (out, traced) = hd_check::work::count_fn_input_traces(|| {
            compile(&fn_value_uses(width, depth), &Goal::Analyze)
        });
        assert!(
            !out.diags.has_errors(),
            "size {width}x{depth}: {}",
            out.render()
        );
        traced
    };
    work(1, 1);
    let (narrow, wide) = (work(100, 20), work(1000, 20));
    // Each use after the first reads the local's trace.
    assert!(
        narrow > 0 && wide <= narrow + 4,
        "traced parts at widths 100 and 1000: {narrow} and {wide}",
    );
    // A build of a program seen before is reused, so these are new.
    let (shallow, deep) = (work(500, 10), work(500, 20));
    assert!(
        deep > shallow && deep <= 2 * shallow + 8,
        "traced parts at depths 10 and 20: {shallow} and {deep}",
    );
}

/// An implementation assigning `width` associated types, each bounded by
/// a trait an impl gives only for a static argument, at a proof `depth`
/// of wrappers deep: the header walks each assignment's proof once.
fn header_outlives(width: usize, depth: usize) -> String {
    let mut s = String::from(
        "trait Saved:\n    pass\n\nstruct Leaf[T]:\n    value: T\n\n\
         struct Wrapped[T]:\n    value: T\n\n\
         impl[T < 'static] Saved for Leaf[T]:\n    pass\n\n\
         impl[T < Saved] Saved for Wrapped[T]:\n    pass\n\ntrait Shelf:\n",
    );
    for i in 0..width {
        let _ = writeln!(s, "    type I{i} < Saved");
    }
    let mut assigned = String::from("Leaf[&'static i32]");
    for _ in 0..depth {
        assigned = format!("Wrapped[{assigned}]");
    }
    s.push_str("\nimpl Shelf for i32:\n");
    for i in 0..width {
        let _ = writeln!(s, "    type I{i} = {assigned}");
    }
    s
}

/// #2261-#2263: the header's outlives walk costs one step per proof node
/// of each checked bound: linear in the bounds and in the proof depth.
#[test]
fn header_outlives_walk_grows_linearly() {
    let work = |width, depth| {
        let (out, scans) = hd_check::work::count_scan_work(|| {
            compile(&header_outlives(width, depth), &Goal::Analyze)
        });
        assert!(
            !out.diags.has_errors(),
            "size {width}x{depth}: {}",
            out.render()
        );
        scans
    };
    work(1, 1);
    let (narrow, wide) = (work(100, 1), work(1000, 1));
    assert!(
        wide > narrow && wide * 100 <= 5 * narrow * 1000 / 4,
        "header walk at widths 100 and 1000: {narrow} and {wide}",
    );
    let (shallow, deep) = (work(10, 10), work(10, 20));
    assert!(
        deep > shallow && deep * 10 <= 5 * shallow * 20 / 4,
        "header walk at depths 10 and 20: {shallow} and {deep}",
    );
}

fn nested_if(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> i32:\n    let total = value\n");
    for i in 0..n {
        let indent = "    ".repeat(i + 1);
        let _ = writeln!(s, "{indent}if total > {i}:\n{indent}    total -= 1");
    }
    s.push_str("    total\n");
    s
}

fn nested_match(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> i32:\n");
    for i in 0..n {
        let indent = "    ".repeat(i * 2 + 1);
        let _ = writeln!(
            s,
            "{indent}match value:\n{indent}    0 => 0\n{indent}    _ =>"
        );
    }
    let _ = writeln!(s, "{}value", "    ".repeat(2 * n + 1));
    s
}

fn nested_closures(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> i32:\n");
    for i in 0..n {
        let indent = "    ".repeat(i + 1);
        let _ = writeln!(s, "{indent}step{i} := fn() -> i32:");
    }
    let _ = writeln!(s, "{}value", "    ".repeat(n + 1));
    for i in (0..n).rev() {
        let _ = writeln!(s, "{}step{i}()", "    ".repeat(i + 1));
    }
    s
}

fn nested_functions(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> i32:\n");
    for i in 0..n {
        let indent = "    ".repeat(i + 1);
        let _ = writeln!(s, "{indent}fn step{i}(value: i32) -> i32:");
    }
    let _ = writeln!(s, "{}value", "    ".repeat(n + 1));
    for i in (0..n).rev() {
        let _ = writeln!(s, "{}step{i}(value)", "    ".repeat(i + 1));
    }
    s
}

fn parens(n: usize) -> String {
    format!(
        "fn run(value: i32) -> i32:\n    {}value{}\n",
        "(".repeat(n),
        " + 1)".repeat(n)
    )
}

fn sum(n: usize) -> String {
    format!(
        "fn run(value: i32) -> i32:\n    value{}\n",
        " + value".repeat(n - 1)
    )
}

fn list(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> List[i32]:\n    [\n");
    for _ in 0..n {
        s.push_str("        value,\n");
    }
    s.push_str("    ]\n");
    s
}

fn statements(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> i32:\n    let total = value\n");
    for i in 0..n / 4 {
        let _ = writeln!(s, "    item{i} := value + {i}");
        let _ = writeln!(
            s,
            "    total += item{i}\n    total += value\n    total -= value"
        );
    }
    s.push_str("    total\n");
    s
}

fn interpolation(n: usize) -> String {
    format!(
        "fn run(value: i32) -> string:\n    \"{}\"\n\npub fn main() -> void $ Console:\n    println(run(1))\n",
        "part ${value} ".repeat(n)
    )
}

fn inferred_chain(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = writeln!(s, "fn step{i}(): step{}()\n", i + 1);
    }
    let _ = writeln!(s, "fn step{n}():\n    println(1)\n    1");
    s
}

fn generic_calls(n: usize) -> String {
    let mut s = String::from(
        "fn keep[T](value: T) -> T: value\n\nfn run(value: i32) -> i32:\n    let total = value\n",
    );
    for _ in 0..n {
        s.push_str("    total = keep(total)\n");
    }
    s.push_str("    total\n");
    s
}

fn deferred_questions(n: usize) -> String {
    let mut s = String::from(
        "trait Build:\n    fn make(value: i32) -> Self\n\nimpl Build for Result[i32, string]:\n    fn make(value: i32) -> Result[i32, string]: .Ok(value)\n\nfn make[T < Build](value: i32) -> T: T::make(value)\n\nfn run(value: i32):\n    let total = value\n",
    );
    for _ in 0..n {
        s.push_str("    total = make(total)?\n");
    }
    s.push_str("    let final: Result[i32, string] = .Ok(total)\n    final\n");
    s
}

#[test]
fn if_nesting_checker_fuel() {
    check_fuel(nested_if, 18, 72);
}
#[test]
fn match_nesting_checker_fuel() {
    check_fuel(nested_match, 10, 40);
}
#[test]
fn closure_nesting_checker_fuel() {
    check_fuel(nested_closures, 10, 40);
}
#[test]
fn local_function_nesting_checker_fuel() {
    check_fuel(nested_functions, 10, 40);
}
#[test]
fn parenthesized_expression_checker_fuel() {
    check_fuel(parens, 36, 144);
}
#[test]
fn left_associative_expression_checker_fuel() {
    check_fuel(sum, 2500, 10_000);
}
#[test]
fn list_literal_checker_fuel() {
    check_fuel(list, 5000, 20_000);
}
#[test]
fn statement_and_local_checker_fuel() {
    check_fuel(statements, 5000, 20_000);
}
#[test]
fn interpolation_emitted_operators() -> Result<(), wasmparser::BinaryReaderError> {
    check_operators(interpolation, 1250, 5000, 0)?;
    Ok(())
}
#[test]
fn inferred_result_and_row_chain_checker_fuel() {
    check_fuel(inferred_chain, 250, 1000);
}
#[test]
fn generic_call_chain_checker_fuel() {
    check_fuel(generic_calls, 250, 1000);
}
#[test]
fn deferred_question_chain_checker_fuel() {
    check_fuel(deferred_questions, 250, 1000);
}

fn nested_loop_blocks(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> i32:\n    let total = value\n");
    for i in 0..n {
        let indent = "    ".repeat(i + 1);
        let _ = writeln!(s, "{indent}while total > {i}:\n{indent}    total -= 1");
    }
    s.push_str("    total\n");
    s
}

fn many_fields(n: usize) -> String {
    let mut s = String::from("struct Record:\n");
    for i in 0..n {
        let _ = writeln!(s, "    f{i}: i32");
    }
    s.push_str("\nfn run(value: i32) -> Record:\n    Record {\n");
    for i in 0..n {
        let _ = writeln!(s, "        f{i}: value,");
    }
    s.push_str("    }\n");
    s
}

fn enum_match(n: usize) -> String {
    let mut s = String::from("enum Code:\n");
    for i in 0..n {
        let _ = writeln!(s, "    V{i}");
    }
    s.push_str("\nfn run(code: Code) -> i32:\n    match code:\n");
    for i in 0..n {
        let _ = writeln!(s, "        .V{i} => {}", (i * 37) % 101);
    }
    s.push_str("\npub fn main() -> void $ Console:\n    println(run(Code.V0))\n");
    s
}

fn generic_instances(n: usize) -> String {
    format!(
        "struct Wrap[T]:\n    inner: T\n\nfn wrap[T](value: T) -> Wrap[T]:\n    Wrap {{ inner: value }}\n\nfn run(value: i32) -> i32:\n    nested := {}value{}\n    nested{}\n\npub fn main() -> void $ Console:\n    println(run(1))\n",
        "wrap(".repeat(n),
        ")".repeat(n),
        ".inner".repeat(n),
    )
}

fn moves_and_borrows(n: usize) -> String {
    let mut s = String::from(
        "fn keep(value: List[i32]) -> List[i32]: value\n\nfn size(value: &List[i32]) -> usize: value.len()\n\nfn run(seed: i32) -> usize:\n    let total: usize = 0\n",
    );
    for i in 0..n {
        let _ = writeln!(
            s,
            "    a{i} := [seed]\n    b{i} := keep(a{i})\n    total += size(&b{i})"
        );
    }
    s.push_str("    total\n\npub fn main() -> void $ Console:\n    println(run(1))\n");
    s
}

fn mutable_field_path(n: usize) -> String {
    let mut s = String::from("struct S0:\n    item: i32\n");
    for i in 1..=n {
        let _ = writeln!(s, "\nstruct S{i}:\n    child: S{}", i - 1);
    }
    let path = ".child".repeat(n);
    let _ = write!(
        s,
        "\nfn write(value: &mut S{n}) -> void:\n    value{path}.item += 1\n\npub fn main() -> void $ Console:\n    let mut root = ",
    );
    for i in (1..=n).rev() {
        let _ = write!(s, "S{i} {{ child: ");
    }
    let _ = writeln!(
        s,
        "S0 {{ item: 0 }}{}\n    write(&mut root)\n    println(root{path}.item)",
        " }".repeat(n),
    );
    s
}

fn captures(n: usize) -> String {
    let mut s = String::from("fn run(value: i32) -> i32:\n");
    for i in 0..n {
        let _ = writeln!(s, "    c{i} := value + {i}");
    }
    s.push_str("    step := fn() -> i32:\n        let total: i32 = 0\n");
    for i in 0..n {
        let _ = writeln!(s, "        total += c{i}");
    }
    s.push_str(
        "        total\n    step()\n\npub fn main() -> void $ Console:\n    println(run(1))\n",
    );
    s
}

/// A generated scale program: its name, its generator, and its small size.
type ScaleProgram<'a> = (&'a str, &'a dyn Fn(usize) -> String, usize);

/// The emitted module of `source`, built for its `main`: its size in
/// bytes and its function count, one per instance the build kept.
fn built_size(source: &str) -> (usize, usize) {
    let out = compile(
        source,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.unwrap_or_default();
    let mut functions = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        if let Ok(wasmparser::Payload::CodeSectionEntry(_)) = payload {
            functions += 1;
        }
    }
    (wasm.len(), functions)
}

/// #1800 draft 56: the scale programs the checker guards also build, and
/// their emitted bytes and instances grow at most linearly with their size
/// (a quarter's slack over the size ratio; std's runtime is a constant).
#[test]
fn scale_programs_build_linearly() {
    let programs: [ScaleProgram; 4] = [
        ("nested", &nested, 10),
        ("wide", &wide, 50),
        ("ring", &ring, 50),
        (
            "omitted-result chain",
            &|n| omitted_result_chain(n, true, false),
            50,
        ),
    ];
    for (name, generate, small) in programs {
        let large = 2 * small;
        let ((b0, f0), (b1, f1)) = (built_size(&generate(small)), built_size(&generate(large)));
        assert!(
            b1 > b0 && b1 * small <= 5 * b0 * large / 4,
            "{name}: {b0} and {b1} bytes at {small} and {large}",
        );
        assert!(
            f1 * small <= 5 * f0 * large / 4,
            "{name}: {f0} and {f1} functions at {small} and {large}",
        );
    }
}

/// Backend: count Wasm operators and, separately, the width of switch tables.
fn operator_counts(source: &str) -> Result<(usize, u32), wasmparser::BinaryReaderError> {
    let out = compile(
        source,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.wasm.is_some(), "{}", out.render());
    let mut ops = 0;
    let mut widest_table = 0;
    for payload in wasmparser::Parser::new(0).parse_all(out.wasm.as_deref().unwrap_or_default()) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload? {
            let mut reader = body.get_operators_reader()?;
            while !reader.eof() {
                let op = reader.read()?;
                ops += 1;
                if let wasmparser::Operator::BrTable { targets } = op {
                    widest_table = widest_table.max(targets.len());
                }
            }
        }
    }
    Ok((ops, widest_table))
}

fn check_operators(
    generate: fn(usize) -> String,
    small: usize,
    large: usize,
    min_table: u32,
) -> Result<(), wasmparser::BinaryReaderError> {
    let work = |n| operator_counts(&generate(n));
    let ((before, _), (after, table)) = (work(small)?, work(large)?);
    assert!(
        before > 0 && after > before && after * small <= 5 * before * large / 4,
        "emitted operators for {small} and {large}: {before} and {after}",
    );
    assert!(
        table >= min_table,
        "expected table of {min_table} arms, got {table}"
    );
    Ok(())
}

#[test]
fn block_nesting_checker_fuel() {
    check_fuel(nested_loop_blocks, 18, 72);
}

#[test]
fn wide_struct_checker_fuel() {
    check_fuel(many_fields, 500, 2000);
}

#[test]
fn wide_enum_exhaustiveness_checker_fuel() {
    check_fuel(enum_match, 1250, 5000);
}

// Checker: #1832, a match's reachability and coverage are one usefulness
// pass, so a flat match whose arms share a head constructor costs its
// rows times its columns. Asking per arm against the earlier arms cost
// (3n² + 29n + 62) / 2 for the tuple match, and 1,670 arms passed
// `MATCH_WORK`.
/// `n` arms `(0, k, _)` over `(i32, i32, i32)`, and a final `_`.
fn flat_tuple_match(n: usize) -> String {
    let mut s = String::from("fn run(a: i32, b: i32, c: i32) -> i32:\n    match (a, b, c):\n");
    for k in 0..n {
        let _ = writeln!(s, "        (0, {k}, _) => {k}");
    }
    let _ = writeln!(s, "        _ => {n}");
    s
}

/// `n` arms `Point { x: 0, y: k }` over a struct, and a final `_`.
fn flat_struct_match(n: usize) -> String {
    let mut s = String::from(
        "struct Point:\n    x: i32\n    y: i32\n    z: i32\n\nfn run(p: Point) -> i32:\n    match p:\n",
    );
    for k in 0..n {
        let _ = writeln!(s, "        Point {{ x: 0, y: {k} }} => {k}");
    }
    let _ = writeln!(s, "        _ => {n}");
    s
}

/// Runs `run` on a thread with a test thread's default stack, 2 MiB.
fn on_test_stack(run: impl FnOnce() + Send + 'static) {
    on_stack(2 << 20, run);
}

/// A work test at a tenth of the size its issue reported runs on a stack
/// about a tenth of a test thread's, so a native call per item still
/// overflows where the full size did on [`on_test_stack`]. Its counter
/// tells linear from quadratic at either size: a scan of every item per
/// item is still about n / 10 times the linear work at the larger size.
fn on_small_stack(run: impl FnOnce() + Send + 'static) {
    on_stack(256 << 10, run);
}

fn on_stack(bytes: usize, run: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(run)
        .expect("thread")
        .join()
        .expect("no stack overflow");
}

#[test]
fn flat_tuple_match_usefulness_checker_fuel() {
    on_test_stack(|| check_fuel(flat_tuple_match, 1000, 10_000));
}

#[test]
fn flat_struct_match_usefulness_checker_fuel() {
    on_test_stack(|| check_fuel(flat_struct_match, 1000, 10_000));
}

// Checker: #1845, a match's decision tree is built from a work stack, and
// a literal column is partitioned by value once, so a flat literal match
// costs its arms, not n² / 2, and nests no native call per arm. Before,
// 3,000 literal arms overflowed a test thread's stack and 50,000 aborted
// `hd check`.
/// `n` integer literal arms, and a final `_`.
fn literal_match(n: usize) -> String {
    let mut s = String::from("fn run(a: i32) -> i32:\n    match a:\n");
    for k in 0..n {
        let _ = writeln!(s, "        {k} => {}", k % 7);
    }
    let _ = writeln!(s, "        _ => {n}");
    s
}

/// `n` string literal arms, and a final `_`.
fn string_match(n: usize) -> String {
    let mut s = String::from("fn run(a: string) -> i32:\n    match a:\n");
    for k in 0..n {
        let _ = writeln!(s, "        \"k{k}\" => {}", k % 7);
    }
    let _ = writeln!(s, "        _ => {n}");
    s
}

/// `n` negative float literal arms, and a final `_`.
fn negative_float_match(n: usize) -> String {
    let mut s = String::from("fn run(a: f64) -> i32:\n    match a:\n");
    for k in 0..n {
        let _ = writeln!(s, "        -{k}.5 => {}", k % 7);
    }
    let _ = writeln!(s, "        _ => {n}");
    s
}

/// `n` guarded arms, each a leaf whose fail side is the arms below.
fn guarded_match(n: usize) -> String {
    let mut s = String::from("fn run(a: i32, g: bool) -> i32:\n    match a:\n");
    for k in 0..n {
        let _ = writeln!(s, "        _ if g => {k}");
    }
    let _ = writeln!(s, "        _ => {n}");
    s
}

/// `n` arms alternating a literal and a range, which tests one row each.
fn literal_range_match(n: usize) -> String {
    let mut s = String::from("fn run(a: i32) -> i32:\n    match a:\n");
    for k in 0..n / 2 {
        let _ = writeln!(s, "        {} => {k}", 3 * k);
        let _ = writeln!(s, "        {}..={} => {k}", 3 * k + 1, 3 * k + 2);
    }
    let _ = writeln!(s, "        _ => {n}");
    s
}

/// The decision rows a build of `generate(n)` looks at. Its function is
/// renamed, so no other test's build of the same source answers it from
/// the store.
fn decision_rows(generate: fn(usize) -> String, n: usize) -> u64 {
    let source = generate(n).replace("fn run(", "fn decided(");
    let (out, rows) = hd_check::work::count_decision_rows(|| compile(&source, &Goal::Analyze));
    assert!(!out.diags.has_errors(), "size {n}: {}", out.render());
    rows
}

/// The decision rows of `generate` at each size grow linearly from the
/// first, on a test thread's stack.
fn decision_rows_grow_linearly(generate: fn(usize) -> String, sizes: &'static [usize]) {
    on_test_stack(move || {
        // The first build also checks the standard library's bodies.
        decision_rows(generate, 1);
        let base = decision_rows(generate, sizes[0]);
        for &n in &sizes[1..] {
            let rows = decision_rows(generate, n);
            let scale = (n / sizes[0]) as u64;
            assert!(
                base > 0 && rows <= scale * base * 5 / 4,
                "decision rows for {} and {n} arms: {base} and {rows}",
                sizes[0],
            );
        }
    });
}

#[test]
fn literal_match_decisions_grow_linearly() {
    decision_rows_grow_linearly(literal_match, &[1000, 10_000, 50_000]);
}

#[test]
fn string_match_decisions_grow_linearly() {
    decision_rows_grow_linearly(string_match, &[1000, 10_000, 50_000]);
}

#[test]
fn tuple_and_struct_match_decisions_grow_linearly() {
    decision_rows_grow_linearly(flat_tuple_match, &[1000, 10_000]);
    decision_rows_grow_linearly(flat_struct_match, &[1000, 10_000]);
}

#[test]
fn guarded_match_decisions_grow_linearly() {
    decision_rows_grow_linearly(guarded_match, &[1000, 10_000, 50_000]);
}

// Checker: #1849, an integer column's values and ranges are split once,
// so alternating them costs n rows, not n² / 4.
#[test]
fn literal_and_range_match_decisions_grow_linearly() {
    decision_rows_grow_linearly(literal_range_match, &[1000, 10_000]);
}

// #1813: exhaustive enum decisions are flat tables; finishing the last
// variant does not rebuild the earlier cases or recurse through them.
#[test]
fn exhaustive_enum_switch_work_is_linear_in_width_and_depth() {
    fn source(width: usize, depth: usize) -> String {
        let mut s = String::from("enum Wide:\n");
        for k in 0..width {
            let _ = writeln!(s, "    V{k}");
        }
        let _ = writeln!(s, "\nfn run{width}_{depth}(value: Wide) -> usize:");
        for level in 0..depth {
            let indent = "    ".repeat(2 * level + 1);
            let _ = writeln!(s, "{indent}match value:");
            for k in 0..width - 1 {
                let _ = writeln!(s, "{indent}    .V{k} => {k}");
            }
            let _ = writeln!(s, "{indent}    .V{} =>", width - 1);
        }
        let _ = writeln!(s, "{}0", "    ".repeat(2 * depth + 1));
        s
    }
    on_test_stack(|| {
        decision_rows(literal_match, 1); // Standard library warm-up.
        let mut measured = Vec::new();
        for (width, depth) in [(1000, 1), (10_000, 1), (1000, 20)] {
            let source = source(width, depth);
            let (out, work) =
                hd_check::work::count_decision_rows(|| compile(&source, &Goal::Analyze));
            assert!(
                !out.diags.has_errors(),
                "{width} x {depth}: {}",
                out.render()
            );
            assert!(
                work > 0 && work <= (width * depth * 10) as u64,
                "{width} x {depth}: {work}"
            );
            measured.push(work);
        }
        assert!(measured[1] <= measured[0] * 11);
        assert!(measured[2] <= measured[0] * 21);
    });
}

/// Declarations far past the generic parameter limit: a trait with
/// 65,536 type parameters (with its `Self`, past every `u16` number), a
/// function with 70,000 lifetimes, and a struct whose field names its
/// 70,000th type parameter.
fn far_past_generic_limit(n: usize) -> String {
    let list = |f: &dyn Fn(usize) -> String, n: usize| (0..n).map(f).collect::<Vec<_>>().join(", ");
    let types = list(&|i| format!("T{i}"), n);
    let lifetimes = list(&|i| format!("'a{i}"), n);
    format!(
        "trait Big[{}]:\n    fn get(self) -> i32\n\nfn far[{lifetimes}](x: &'a{} i32) -> i32:\n    1\n\nstruct Wide[{types}]:\n    last: T{}\n",
        list(&|i| format!("T{i}"), 65_536),
        n - 1,
        n - 1,
    )
}

// Resolver: #1854, a declaration's generic parameters past
// `hd_base::limits::GENERIC_PARAMS` are one `too-many-generic-parameters`
// each, never a silent shared number or an arithmetic overflow.
#[test]
fn generic_parameters_far_past_the_limit_report_once_per_declaration() {
    on_test_stack(|| {
        let out = compile(&far_past_generic_limit(70_000), &Goal::Analyze);
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            codes,
            ["too-many-generic-parameters"; 3],
            "{}",
            out.render()
        );
    });
}

/// A struct of `n` type parameters, each the type of one field.
fn struct_of_parameters(n: usize) -> String {
    let params = (0..n)
        .map(|i| format!("T{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = format!("struct Wide[{params}]:\n");
    for i in 0..n {
        let _ = writeln!(out, "    f{i}: T{i}");
    }
    out
}

/// The generic-scope probes header lowering makes for
/// `struct_of_parameters(n)`, and the diagnostic codes.
fn scope_probes(n: usize) -> (u64, Vec<String>) {
    let (out, probes) =
        hd_resolve::work::count_scope_probes(|| compile(&struct_of_parameters(n), &Goal::Analyze));
    let codes = out
        .diags
        .code
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect();
    (probes, codes)
}

// Resolver: #1857, a use of a generic parameter finds it by name in O(1),
// not by a scan of the parameters in scope: n parameters each used once,
// at the limit and far past it, where the rejected names still bind.
#[test]
fn generic_parameter_uses_probe_the_scope_linearly() {
    on_test_stack(|| {
        let (at_limit, codes) = scope_probes(1024);
        assert!(codes.is_empty(), "{codes:?}");
        for n in [2_000, 70_000] {
            let (probes, codes) = scope_probes(n);
            assert_eq!(codes, ["too-many-generic-parameters"], "{n} parameters");
            let scale = (n / 1024 + 1) as u64;
            assert!(
                at_limit > 0 && probes <= scale * at_limit * 5 / 4,
                "scope probes for 1,024 and {n} parameters: {at_limit} and {probes}",
            );
        }
    });
}

// Resolver: #1874, a local declaration finds a name among the parameters
// around it with one hash probe per use, after hashing them once: a scan
// of every enclosing parameter per use cost 1,024 string compares each.
/// A local struct inside a method with `params` type parameters, whose
/// `uses` fields name an unknown type, const or trait in turn.
fn outer_unknown_names(params: usize, uses: usize) -> String {
    let list: Vec<String> = (0..params).map(|i| format!("T{i}")).collect();
    let mut s = format!(
        "struct Fixed[N < const i32]:\n    v: i32\n\nstruct Host:\n    v: i32\n\nimpl Host:\n    fn big[{}](&self) -> i32:\n        struct Local:\n",
        list.join(", ")
    );
    for i in 0..uses {
        let ty = match i % 3 {
            0 => format!("U{i}"),
            1 => format!("Fixed[C{i}]"),
            _ => format!("fn(&dyn K{i}) -> void"),
        };
        let _ = writeln!(s, "            f{i}: {ty}");
    }
    s.push_str("        1\n");
    s
}

#[test]
fn unknown_names_in_a_local_declaration_probe_enclosing_parameters_once_each() {
    let work = |uses: usize| {
        let (out, probes) = hd_resolve::work::count_outer_probes(|| {
            compile(&outer_unknown_names(1024, uses), &Goal::Analyze)
        });
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(codes.len(), uses, "{}", out.render());
        assert!(
            codes
                .iter()
                .all(|c| ["unknown-type", "unknown-name", "unknown-trait"].contains(c)),
            "{}",
            out.render()
        );
        probes
    };
    // Each pass that lowers `Local` hashes the 1,024 parameters and `Self`
    // once, then probes once per use: 3 passes, 3,075 + 4 per use
    // measured. A scan per use would cost 1,024 per use.
    for uses in [1000, 10_000] {
        let probes = work(uses);
        assert!(
            probes <= 4 * (1025 + uses as u64),
            "{uses} unknown names among 1,024 enclosing parameters: {probes} probes",
        );
    }
}

// Checker: #1852, a negative float pattern is read as its value, so its
// column is split by value: n rows, not n² / 2.
#[test]
fn negative_float_match_decisions_grow_linearly() {
    decision_rows_grow_linearly(negative_float_match, &[1000, 10_000]);
}

#[test]
fn nested_generic_instantiation_emitted_operators() -> Result<(), wasmparser::BinaryReaderError> {
    check_operators(generic_instances, 5, 20, 0)
}

#[test]
fn ownership_moves_and_borrows_emitted_operators() -> Result<(), wasmparser::BinaryReaderError> {
    check_operators(moves_and_borrows, 100, 400, 0)
}

#[test]
fn mutable_field_path_emitted_operators() -> Result<(), wasmparser::BinaryReaderError> {
    let (short, _) = operator_counts(&mutable_field_path(5))?;
    let (long, _) = operator_counts(&mutable_field_path(20))?;
    // Field offsets are resolved at compile time, so the depth adds no Wasm.
    assert_eq!(short, long);
    Ok(())
}

#[test]
fn closure_capture_width_emitted_operators() -> Result<(), wasmparser::BinaryReaderError> {
    check_operators(captures, 250, 1000, 0)
}

#[test]
fn wide_match_dispatch_emitted_operators() -> Result<(), wasmparser::BinaryReaderError> {
    // The last exhaustive variant is the default, outside the indexed entries.
    check_operators(enum_match, 500, 2000, 1999)
}

#[test]
fn trait_impl_width_coherence_pairs() {
    let (small, large) = (
        crate::coherence_scale::pairs(500),
        crate::coherence_scale::pairs(2000),
    );
    assert!(
        large <= small + 1500,
        "coherence pairs for 500 and 2000: {small} and {large}"
    );
}

fn cross_statement_literals(n: usize) -> String {
    let mut s =
        String::from("fn consume(value: i32) -> void: pass\n\nfn run() -> void $ Console:\n");
    for i in 0..n {
        let _ = writeln!(s, "    v{i} := {i}\n    println(v{i})\n    consume(v{i})");
    }
    s
}

#[test]
fn cross_statement_literal_class_checker_fuel() {
    check_fuel(cross_statement_literals, 250, 1000);
}

/// Syntax: count each emitted interpolation part. Both sizes also type-check.
#[test]
fn interpolation_width_checks() {
    for width in [1250, 5000] {
        let source = interpolation(width);
        let out = compile(&source, &Goal::Analyze);
        assert!(!out.diags.has_errors(), "size {width}: {}", out.render());
        let parse = hd_syntax::parse(source.as_bytes());
        assert!(parse.is_ok(), "{:?}", parse.diagnostic_codes());
        let parts = parse
            .tokens
            .kind
            .iter()
            .filter(|kind| {
                matches!(
                    kind,
                    hd_syntax::TokenKind::StrMid | hd_syntax::TokenKind::StrTail
                )
            })
            .count();
        assert_eq!(parts, width);
    }
}

/// Syntax: the full expression parses before the #1678 checker failure.
#[test]
fn flat_addition_parses_with_linear_token_count() {
    let work = |n| {
        let parsed = hd_syntax::parse(sum(n).as_bytes());
        assert!(parsed.is_ok(), "{:?}", parsed.diagnostic_codes());
        parsed.tokens.len()
    };
    assert_eq!(work(10_000) - work(2500), 2 * (10_000 - 2500));
}

// Checker: #1777 and #1834, a projection past `PROJECTION_DEPTH` bindings
// (`trait.assoc.projection.depth`). `impl[T < Walk] Walk for S[T]: type
// Out = T::Out` takes one binding per `S`, so `S^70[A]::Out` needs 71, and
// the proof that `S^70[A]` implements `Walk` is past its own depth too
// (`trait.bound.depth.limit`). Each use reports one
// `trait-resolution-depth`: the first of the two it meets.
/// The chain's declarations, then `rest`, where `$S` stands for `S^70[A]`
/// and `$64` for `S^64[A]`.
fn projection_chain(rest: &str) -> String {
    let deep = |n| (0..n).fold(String::from("A"), |t, _| format!("S[{t}]"));
    let head = "trait Walk:\n    type Out\n\nstruct A:\n    x: i32\n\nimpl Walk for A:\n    type Out = i32\n\nstruct S[T]:\n    x: i32\n\nimpl[T < Walk] Walk for S[T]:\n    type Out = T::Out\n\nstruct W[T < Walk]:\n    v: T::Out\n\n";
    let rest = rest.replace("$S", &deep(70)).replace("$64", &deep(64));
    format!("{head}{rest}")
}

/// Each error's line and message, in content order.
fn errors_of(source: &str) -> Vec<(usize, String)> {
    let out = compile(source, &Goal::Analyze);
    (out.diags.content_order().into_iter())
        .filter(|&i| out.diags.severity[i] == hd_diag::Severity::Error)
        .map(|i| {
            let (_, line, _) = out.locate(out.diags.primary[i]);
            (line, out.diags.get_text(out.diags.message[i]).to_owned())
        })
        .collect()
}

#[test]
fn a_projection_past_the_binding_limit_is_reported_once_where_each_type_is_used() {
    // A message cuts a type below `SHOWN_TYPE_DEPTH` constructors (#1861):
    // the projection is one of them.
    let cut = |levels: usize| format!("{}...{}", "S[".repeat(levels), "]".repeat(levels));
    let depth = hd_resolve::iface::SHOWN_TYPE_DEPTH as usize;
    let s70 = cut(depth);
    let past = format!(
        "normalizing `{}::Out` needs more than 64 nested associated-type bindings",
        cut(depth - 1)
    );
    let proof = |deep: &str| {
        format!("the proof that {deep} implements Walk needs more than 64 nested bounds")
    };
    // A header type: the projection-cycle walk of a parameter's type
    // meets the projection before the bound's proof.
    let header = errors_of(&projection_chain("fn h(x: W[$S]) -> void:\n    pass\n"));
    assert_eq!(header, [(19, past.clone())]);
    // A body value of the parameter's type, or the result's: the header
    // reported it.
    let body = errors_of(&projection_chain(
        "fn h(x: *W[$S]) -> void:\n    y := x\n    pass\n",
    ));
    assert_eq!(body, [(19, past.clone())]);
    // A body value of a type another item's header holds: one per item.
    let call = errors_of(&projection_chain(
        "fn mk() -> *W[$S]:\n    panic(\"\")\n\nfn h() -> void:\n    y := mk()\n    pass\n",
    ));
    assert_eq!(call, [(19, past.clone()), (23, past.clone())]);
    // A supertrait's binding that the implementation must match, after
    // the supertrait's proof.
    let sup = errors_of(&projection_chain(
        "trait Sub < Walk[Out = i32]:\n    fn m(&self) -> i32\n\nimpl Sub for $S:\n    fn m(&self) -> i32:\n        0\n",
    ));
    assert_eq!(sup, [(22, proof(&s70))]);
    // A bound whose trait argument projects from the first: the first
    // bound's proof, and no blame on the second.
    let goal = errors_of(&projection_chain(
        "trait Layer[X]:\n    fn m(&self) -> i32\n\nfn f[I < Walk, O < Layer[I::Out]](i: I, o: O) -> i32:\n    0\n\nfn g(i: $S, o: i32) -> i32:\n    f(i, o)\n",
    ));
    assert_eq!(goal, [(26, proof(&s70))]);
    // A call's result projected from the type whose proof failed is no
    // type to compare.
    let s64 = cut(depth);
    let result = errors_of(&projection_chain(
        "fn g[U < Walk](u: U) -> U::Out:\n    panic(\"\")\n\nfn h(x: $64) -> i32:\n    g(x)\n",
    ));
    assert_eq!(result, [(23, proof(&s64))]);
}

// Driver: #1846, a diagnostic's line and column come from one line index
// per file, built on first use, so locating d diagnostics in a file of n
// bytes costs n + d log n, not n · d. Review F13: one file with 16,000
// syntax errors took 13.6 s.
/// `n` functions whose result has the wrong type: an error each.
fn wrong_results(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = writeln!(s, "fn f{i}() -> i32: \"a\"");
    }
    s
}

#[test]
fn locating_many_diagnostics_indexes_their_file_once() {
    for n in [2000, 20_000] {
        let source = wrong_results(n);
        let out = compile(&source, &Goal::Analyze);
        let (located, indexed) = hd_driver::count_indexed_bytes(|| out.render_located());
        assert_eq!(indexed, source.len() as u64, "{n} errors");
        let mismatches: Vec<&str> = located
            .lines()
            .filter(|l| l.contains("type-mismatch"))
            .collect();
        assert_eq!(mismatches.len(), n);
        let last = format!("main.hd:{n}:");
        assert!(
            mismatches.last().is_some_and(|l| l.contains(&last)),
            "{:?}",
            mismatches.last()
        );
    }
}

// Checker: #1850, a `@track_caller` call site's record locates the call
// and each argument through its file's one line index, however many
// tracked calls the file holds.
/// One body of `n` tracked calls, one per line, whose callee reads the
/// argument texts.
fn tracked_calls(n: usize) -> String {
    let mut s = String::from(
        "use std.meta.Location\n\n@track_caller\nfn at(x: i32) -> i32:\n    if Location::caller_args().is_empty(): 0 else: x\n\npub fn run() -> i32:\n    let s = 0\n",
    );
    for _ in 0..n {
        s.push_str("    s = at(s + 1)\n");
    }
    s.push_str("    s\n");
    s
}

#[test]
fn tracked_call_records_index_their_file_once() {
    for n in [2000, 20_000] {
        let source = tracked_calls(n);
        let (out, indexed) = hd_driver::count_indexed_bytes(|| compile(&source, &Goal::Analyze));
        assert!(out.diags.is_empty(), "{}", out.render());
        assert_eq!(indexed, source.len() as u64, "{n} tracked calls");
    }
}

/// Builds and runs the literal decision chain on a test thread's 2 MiB
/// stack. 10,000 arms there leave under 210 bytes per arm, so a native
/// call per arm overflows; 50,000 arms on a 32 MiB worker stack left 670
/// and took 4x longer. Control continuation counts include target lookup
/// and branch compaction.
fn emitted_literal_decisions(generate: fn(usize) -> String, strings: bool) {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let program = |n: usize| {
                let mut source = generate(n);
                let argument = if strings {
                    format!("\"k{}\"", n / 2)
                } else {
                    (n / 2).to_string()
                };
                let _ = write!(
                    source,
                    "\npub fn main() -> void $ Console:\n    println(run({argument}))\n"
                );
                source
            };
            let goal = Goal::Program {
                entry: "main".into(),
            };
            let initial = compile(&program(1), &goal);
            assert!(!initial.diags.has_errors(), "{}", initial.render());
            let mut base = 0;
            for n in [1000, 10_000] {
                let (out, work) = hd_wasm::work::count_control(|| compile(&program(n), &goal));
                assert!(!out.diags.has_errors(), "size {n}: {}", out.render());
                let wasm = out.wasm.as_deref().expect("the program links");
                let ran = crate::release_profile_run::run_wasm(
                    if strings {
                        "deep-string-match"
                    } else {
                        "deep-integer-match"
                    },
                    wasm,
                );
                assert!(
                    ran.status.success(),
                    "size {n}: {}",
                    String::from_utf8_lossy(&ran.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&ran.stdout),
                    format!("{}\n", (n / 2) % 7)
                );
                assert!(
                    ran.stderr.is_empty(),
                    "{}",
                    String::from_utf8_lossy(&ran.stderr)
                );
                if n == 1000 {
                    base = work;
                }
                assert!(
                    base > 0 && work <= (n / 1000) as u64 * base * 5 / 4,
                    "control work for 1000 and {n} arms: {base}, {work}"
                );
            }
        })
        .expect("worker-stack thread")
        .join()
        .expect("build and run without overflowing a test thread's stack");
}

#[test]
fn literal_match_builds_and_runs_ten_thousand_arms_on_a_small_stack() {
    emitted_literal_decisions(literal_match, false);
}

#[test]
fn string_match_builds_and_runs_ten_thousand_arms_on_a_small_stack() {
    emitted_literal_decisions(string_match, true);
}

// Diagnostics: #1861, a type a message names is cut at
// `SHOWN_TYPE_BYTES` and `SHOWN_TYPE_DEPTH`, so neither the message nor
// the time to print it grows with the type's expanded size. An 18-level
// doubled tuple of strings printed 5.2 MB, and its printing time doubled
// with each level.
/// `x0 := "a"`, `x1 := (x0, x0)`, ... and `x{levels}` bound to an `i32`.
fn doubled_tuple(levels: usize) -> String {
    let mut s = String::from("fn main() -> void:\n    x0 := \"a\"\n");
    for i in 1..=levels {
        let _ = writeln!(s, "    x{i} := (x{}, x{})", i - 1, i - 1);
    }
    let _ = writeln!(s, "    let y: i32 = x{levels}");
    s
}

/// A proof that `Box[Box[...[i32]...]]`, `depth` deep, implements a
/// trait implemented for `Box[T]` given `T`'s proof.
fn deep_proof(depth: usize) -> String {
    format!(
        "trait Marker\nstruct Box[T]:\n    value: T\nimpl Marker for i32\nimpl[T < Marker] Marker for Box[T]\nfn need[T < Marker](value: T) -> void:\n    pass\nfn deep(value: {}i32{}) -> void:\n    need(value)\n",
        "Box[".repeat(depth),
        "]".repeat(depth)
    )
}

/// The one diagnostic of `source`, which has `code`.
fn only_message(source: &str, code: &str) -> String {
    let out = compile(source, &Goal::Analyze);
    let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
    assert_eq!(codes, [code], "{}", out.render());
    out.diags.get_text(out.diags.message[0]).to_owned()
}

/// Bytes a message naming two cut types stays under.
const SHORT_MESSAGE: usize = 2 * hd_resolve::iface::SHOWN_TYPE_BYTES + 80;

#[test]
fn a_mismatch_on_a_doubled_tuple_prints_a_short_type() {
    // 22 levels expand to 4 million strings, 29 MB uncut.
    for levels in [18, 22] {
        let message = only_message(&doubled_tuple(levels), "type-mismatch");
        assert!(
            message.len() <= SHORT_MESSAGE && message.contains("found ((((((((..., ...)"),
            "{levels} levels: {} bytes: {message}",
            message.len()
        );
    }
}

// Checker: #1870, the dependent-result checks' slot layout walk
// (`hd_check::dep_results::Layout::walk`) enters a part with no slots once
// per check, however often the pool shares it. It pushed one step per
// leaf of the expanded type: 60 levels did not finish in 2 minutes.
#[test]
fn a_doubled_tuple_of_strings_lays_out_in_linear_steps() {
    let steps = |levels: usize| {
        let (out, steps) =
            hd_check::work::count_layout_steps(|| compile(&doubled_tuple(levels), &Goal::Analyze));
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            codes,
            ["type-mismatch"],
            "{levels} levels: {}",
            out.render()
        );
        steps
    };
    // One step per level, and a few for the bindings' other types.
    for levels in [20, 60, 200] {
        let n = usize::try_from(steps(levels)).expect("steps");
        assert!(
            levels <= n && n <= levels + 10,
            "{levels} levels: {n} steps"
        );
    }
}

/// #1887's program: a tuple of references doubled `levels` times, the
/// source linear in `levels`.
fn doubled_references(levels: usize) -> String {
    let mut s = String::from(
        "struct Point:\n    x: i32\nfn main():\n    p := Point { x: 1 }\n    x0 := &p\n",
    );
    for i in 1..=levels {
        let _ = writeln!(s, "    x{i} := (x{}, x{})", i - 1, i - 1);
    }
    let _ = writeln!(s, "    let y: i32 = x{levels}");
    s
}

// Checker: #1887, a tuple of references doubled 20 times laid out a
// million origin slots (`Layout::new`) and did not check in a minute.
// `types.limit.references` reports the first value past 1,024 slots, the
// body's dependent-result checks skip it, and the count enters each
// distinct type once.
#[test]
fn a_doubled_tuple_of_references_reports_the_limit_in_linear_steps() {
    let steps = |levels: usize| {
        let ((out, counted), laid) = hd_check::work::count_layout_steps(|| {
            hd_check::work::count_slot_counts(|| {
                compile(&doubled_references(levels), &Goal::Analyze)
            })
        });
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            codes,
            ["type-mismatch", "type-too-large"],
            "{levels} levels: {}",
            out.render()
        );
        counted + laid
    };
    // A warm-up build, so std's one-time checks are not counted.
    steps(19);
    let (small, large) = (steps(20), steps(200));
    assert!(
        small >= 20 && large <= 10 * small,
        "20 levels: {small} steps, 200 levels: {large}"
    );
}

#[test]
fn a_proof_past_the_depth_limit_prints_a_short_type() {
    for depth in [70, 150] {
        let message = only_message(&deep_proof(depth), "trait-resolution-depth");
        assert!(
            message.len() <= SHORT_MESSAGE
                && message.contains("Box[Box[Box[Box[Box[Box[Box[Box[...]]]]]]]]"),
            "depth {depth}: {} bytes: {message}",
            message.len()
        );
    }
}

// Checker: #1839, a settle solves the omitted rows that reach only
// checked callables by searching from what it changed, not by a fixpoint
// over every waiting row: in a ring of direct calls each check settles
// with every earlier member's row waiting, which cost W^4 (a ring of 400
// took 3.2 billion steps).
/// `h{i}` calls `h{i+1}`, the last calls `h0`, each with an omitted row.
fn row_ring(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let next = (i + 1) % n;
        let _ = writeln!(
            s,
            "fn h{i}(k: i32) -> void:\n    if k > 0:\n        h{next}(k - 1)\n"
        );
    }
    s.push_str("pub fn main() -> void $ Console:\n    h0(3)\n    println(\"x\")\n");
    s
}

/// `hub` calls each `w{i}`, and each calls `hub`: the hub's row waits on
/// every member until the last is checked.
fn row_hub(n: usize) -> String {
    let mut s = String::from("fn hub(k: i32) -> void:\n    if k > 0:\n");
    for i in 0..n {
        let _ = writeln!(s, "        w{i}(k - 1)");
    }
    for i in 0..n {
        let _ = writeln!(
            s,
            "fn w{i}(k: i32) -> void:\n    if k > 0:\n        hub(k - 1)\n"
        );
    }
    s.push_str("pub fn main() -> void $ Console:\n    hub(3)\n    println(\"x\")\n");
    s
}

/// A generated program of a size.
type Generator = fn(usize) -> String;

#[test]
fn waiting_rows_settled_one_by_one_cost_linear_solve_work() {
    let shapes: [(&str, Generator); 2] = [("ring", row_ring), ("hub", row_hub)];
    for (name, generate) in shapes {
        let mut base = 0;
        for n in [1000, 10_000] {
            let (out, work) =
                hd_check::work::count_solve_work(|| compile(&generate(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{name} {n}: {}", out.render());
            if n == 1000 {
                base = work;
            }
            assert!(
                base > 0 && work <= 11 * base,
                "{name}: solve work for 1,000 and {n} rows: {base}, {work}"
            );
        }
    }
}

/// `n` functions, each declaring a local struct `P` and an inherent local
/// impl of it, and calling its method: `n` locals of one name, and `n`
/// local impls each seen by one body.
fn local_types(n: usize) -> String {
    let mut out = String::from("pub fn main() -> void: pass\n");
    for i in 0..n {
        let _ = write!(
            out,
            "fn g{i}() -> i32:\n    struct P:\n        a: i32\n        b: i32\n    impl P:\n        fn get{i}(self) -> i32: self.a\n    p := P {{ a: +1, b: +2 }}\n    p.get{i}()\n"
        );
    }
    out
}

// Resolver and checker: #1878, a local type or impl is found through the
// module's index of locals, by name and byte, not by a filter over every
// local of the module per use; and a body's local impls in scope change
// only where one starts or ends, not by a list of the unseen ones per
// statement.
#[test]
fn local_declarations_are_found_by_one_probe_each() {
    on_small_stack(|| {
        let mut base = 0;
        for n in [200, 3_200] {
            let (out, probes) =
                hd_resolve::work::count_local_probes(|| compile(&local_types(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            if n == 200 {
                base = probes;
            }
            assert!(
                base > 0 && probes <= 16 * base * 5 / 4,
                "local lookups for 200 and {n} functions: {base}, {probes}"
            );
        }
    });
}

/// A local function inside a function of 1,024 type parameters, whose `n`
/// parameters name unknown types, and whose body names `n` unknown const
/// arguments.
fn local_fn_unknown_types(n: usize) -> String {
    let list: Vec<String> = (0..1024).map(|i| format!("T{i}")).collect();
    let params: Vec<String> = (0..n).map(|i| format!("p{i}: U{i}")).collect();
    let mut s = format!(
        "struct Fixed[N < const i32]:\n    v: i32\n\nfn outer[{}]() -> i32:\n    fn inner({}) -> i32:\n",
        list.join(", "),
        params.join(", ")
    );
    for i in 0..n {
        let _ = writeln!(s, "        let f{i}: Fixed[C{i}] = Fixed {{ v: +1 }}");
    }
    s.push_str("        +1\n    +1\n");
    s
}

// Checker: #1877, a name that reaches nothing asks the module's index of
// enclosing parameters, built once, with one probe: a walk down from the
// root scanning each level's children, then a compare with each of the
// 1,024 enclosing parameters, cost that per use.
#[test]
fn unknown_names_in_a_local_function_ask_the_enclosing_index_once_each() {
    on_test_stack(|| {
        for n in [1_000, 10_000] {
            let (out, work) = hd_resolve::work::count_enclosing_work(|| {
                compile(&local_fn_unknown_types(n), &Goal::Analyze)
            });
            let unknown = out
                .diags
                .code
                .iter()
                .filter(|c| matches!(c.as_str(), "unknown-type" | "unknown-name"))
                .count();
            assert!(unknown >= n, "{n}: {}", out.render());
            // The 1,025 names indexed once (`Self` of none, `T0`..`T1023`,
            // `N`), then a few questions per use (5 measured).
            assert!(
                work <= 1_100 + 6 * n as u64,
                "enclosing work for {n} uses: {work}"
            );
        }
    });
}

/// A struct whose `n` fields each name an unknown type, every other one a
/// name std exports (`StringBuilder`), so each asks for an import hint.
fn unknown_field_types(n: usize) -> String {
    let mut s = String::from("pub fn main() -> void: pass\n\nstruct Wide:\n");
    for i in 0..n {
        if i % 2 == 0 {
            let _ = writeln!(s, "    f{i}: Unknown{i}");
        } else {
            let _ = writeln!(s, "    f{i}: StringBuilder");
        }
    }
    s
}

// Resolver and checker: #1876, an unknown name's import hint asks the
// explicit std index by name, built once, with one probe:
// a pass over every export of std per unknown name cost about 175 µs each.
#[test]
fn import_hints_probe_one_explicit_index() {
    on_test_stack(|| {
        for n in [1_000, 10_000] {
            let (out, work) = hd_resolve::work::count_hint_work(|| {
                compile(&unknown_field_types(n), &Goal::Analyze)
            });
            let shown = out.render();
            let hinted = shown.matches("; add `use std.text.StringBuilder`").count();
            assert_eq!(hinted, n / 2, "{n}: {shown}");
            // The explicit std exports are indexed once, followed by a
            // few probes per unknown name (4.5 measured): not std's
            // exports per name.
            assert!(
                work <= 600 + 5 * n as u64,
                "hint work for {n} unknown names: {work}"
            );
        }
    });
}

// #1876: the index answers for a module's last segment too: an unknown
// `json` is std's module, `min` has one home, and `max` has several.
#[test]
fn import_hints_name_a_module_and_several_homes_from_the_index() {
    let out = compile(
        "pub fn main() -> void:\n    a := json\n    b := min\n    c := max\n",
        &Goal::Analyze,
    );
    let shown = out.render();
    assert!(
        shown.contains("`json` is not defined; add `use std.encoding.json`"),
        "{shown}"
    );
    assert!(
        shown.contains("`min` is not defined; add `use std.cmp.min`"),
        "{shown}"
    );
    assert!(
        shown.contains("`max` has several public std homes: `std.cmp`, `std.validate`"),
        "{shown}"
    );
}

/// `n` traits, each with a written implementation and a delegating one
/// that writes its associated function, and a newtype that derives `Eq`
/// for every 100th (`derivations_find_what_they_need_by_one_probe_each`
/// derives for every one).
fn trait_impls(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = write!(
            s,
            "trait T{i}:\n    fn m{i}(self) -> i32\n    fn k{i}() -> i32\n\nstruct S{i}:\n    x: i32\n\nimpl T{i} for S{i}:\n    fn m{i}(self) -> i32:\n        self.x\n    fn k{i}() -> i32:\n        1\n\nstruct D{i}:\n    S{i}\n\nimpl T{i} for D{i} by S{i}:\n    fn k{i}() -> i32:\n        2\n\n"
        );
        if i % 100 == 0 {
            let _ = write!(s, "@derive(Eq)\ntype M{i}(i32)\n\n");
        }
    }
    s
}

// Resolver: #1888, an implementation finds its trait, its part and its
// trait's methods through the unit's index of items, one probe each,
// and checks its methods against the trait's names hashed once: a scan
// of every item of the unit per implementation cost n² / 2.
#[test]
fn trait_impls_find_their_traits_by_one_probe_each() {
    on_small_stack(|| {
        let mut base = 0;
        for n in [200, 2_000] {
            let (out, probes) =
                hd_resolve::work::count_item_probes(|| compile(&trait_impls(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            if n == 200 {
                base = probes;
            }
            assert!(
                base > 0 && probes <= 10 * base * 5 / 4,
                "implementation work for 200 and {n} traits: {base}, {probes}"
            );
        }
    });
}

/// `n` functions with omitted results, each declaring a struct with an
/// inherent method `get` and calling it.
fn same_named_methods(n: usize) -> String {
    let mut s = String::from("pub fn main() -> void $ Console:\n    println(g0())\n\n");
    for i in 0..n {
        let _ = write!(
            s,
            "fn g{i}():\n    struct P:\n        x: i32\n    impl P:\n        fn get(self) -> i32:\n            self.x\n    p := P {{ x: {i} }}\n    p.get()\n\n"
        );
    }
    s
}

// Checker: #1889, the omitted-result plan links a body to one node per
// name it holds, and a name several callables share to each of them
// once: a link from each body to every callable of each name made n
// bodies that each call one of n methods named `get` n² edges. (The
// sizes stay small: same-named inherent members are still compared in
// pairs, a separate quadratic.)
#[test]
fn omitted_result_plan_links_a_shared_name_once() {
    on_test_stack(|| {
        for n in [1_000, 10_000] {
            let (out, edges) = hd_check::work::count_plan_edges(|| {
                compile(&same_named_methods(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            // Each body names `P` and `get` (and `g0` in `main`); `get`
            // joins its n methods once.
            assert!(
                edges <= 4 * n as u64,
                "plan edges for {n} same-named methods: {edges}"
            );
        }
    });
}

/// One function whose suite declares `n` local structs.
fn many_local_structs(n: usize) -> String {
    let mut s =
        String::from("pub fn main() -> void $ Console:\n    println(f())\n\nfn f() -> i32:\n");
    for i in 0..n {
        let _ = writeln!(s, "    struct S{i}:\n        x: i32");
    }
    s.push_str("    1\n");
    s
}

// Resolver: #1890, a local header reads the declarations around it from
// the module's index, built by one walk of the tree: a walk down from
// the top-level declaration, scanning each level's children for the one
// that holds the header, cost one scan of the suite per local.
#[test]
fn local_headers_read_their_enclosing_declarations_from_one_walk() {
    on_test_stack(|| {
        let mut base = 0;
        for n in [1_000, 10_000] {
            let (out, steps) = hd_resolve::work::count_enclosing_steps(|| {
                compile(&many_local_structs(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            if n == 1_000 {
                base = steps;
            }
            assert!(
                base > 0 && steps <= 10 * base * 5 / 4,
                "enclosing steps for 1,000 and {n} local structs: {base}, {steps}"
            );
        }
    });
}

// #2249: growing a suite must not rescan it for every local capture,
// and lexical depth adds only linear capture and diagnostic-context work.
#[test]
fn local_impl_capture_context_work_is_bounded_in_width_and_depth() {
    fn source(width: usize, depth: usize) -> String {
        let mut out =
            String::from("fn outer[T](x: T) -> void:\n    struct Cell:\n        value: T\n");
        for level in 0..depth {
            let indent = "    ".repeat(level + 1);
            let _ = writeln!(out, "{indent}fn level{level}[U{level}]() -> void:");
        }
        let indent = "    ".repeat(depth + 1);
        let _ = writeln!(
            out,
            "{indent}trait Pick:\n{indent}    fn pick(self) -> void\n{indent}impl Cell:\n{indent}    fn visit[V](self) -> void:\n{indent}        struct Child:\n{indent}            value: V\n{indent}        impl Pick for Child:\n{indent}            fn pick(self) -> void:"
        );
        for index in 0..width {
            let _ = writeln!(
                out,
                "{indent}                struct S{index}:\n{indent}                    value: T"
            );
        }
        let _ = writeln!(out, "{indent}                pass\n{indent}        pass");
        for level in (0..=depth).rev() {
            let _ = writeln!(out, "{}pass", "    ".repeat(level + 1));
        }
        out
    }
    on_test_stack(|| {
        let mut shallow = 0;
        for depth in [1, 20] {
            let mut overhead = 0;
            let mut small = 0;
            for width in [0, 100, 1_000] {
                let (out, steps) = hd_resolve::work::count_enclosing_steps(|| {
                    compile(&source(width, depth), &Goal::Analyze)
                });
                assert!(
                    out.diags.is_empty(),
                    "width {width}, depth {depth}: {}",
                    out.render()
                );
                if width == 0 {
                    overhead = steps;
                    continue;
                }
                let work = steps - overhead;
                eprintln!(
                    "local capture work: width={width} depth={depth} steps={steps} incremental={work}"
                );
                if width == 100 {
                    small = work;
                }
                assert!(
                    small > 0 && work <= 11 * small,
                    "width {width}, depth {depth}: {work}, width100 {small}"
                );
                if depth == 1 && width == 1_000 {
                    shallow = work;
                }
                if depth == 20 && width == 1_000 {
                    assert!(
                        shallow > 0 && work <= 21 * shallow,
                        "depth20 {work}, depth1 {shallow}"
                    );
                }
            }
        }
    });
}

/// `n` functions, each declaring a local trait `Ti` with a method `get`,
/// an impl of it for a local struct and the call `p.get()`: `n` local
/// traits declare `get`, and each call sees one of them.
fn local_traits_of_one_method(n: usize) -> String {
    let mut out = String::from("pub fn main() -> void: pass\n");
    for i in 0..n {
        let _ = write!(
            out,
            "fn g{i}() -> i32:\n    struct P:\n        a: i32\n    trait T{i}:\n        fn get(self) -> i32\n    impl T{i} for P:\n        fn get(self) -> i32: self.a\n    p := P {{ a: +1 }}\n    p.get()\n"
        );
    }
    out
}

/// `n` module-level traits `Ti` declaring `get` and `make`, each with a
/// subtrait `Si`, a struct implementing both, and calls through a bound
/// (`x.get()`, `T::make()`) and through a trait value (`v.get()`): calls
/// that do not ask the module's available traits.
fn bounded_traits_of_one_method(n: usize) -> String {
    let mut out = String::from("pub fn main() -> void: pass\n");
    for i in 0..n {
        let _ = write!(
            out,
            "trait T{i}:\n    fn get(self) -> i32\n    fn make() -> Self\ntrait S{i} < T{i}:\n    fn other(self) -> i32\nstruct P{i}:\n    a: i32\nimpl T{i} for P{i}:\n    fn get(self) -> i32: self.a\n    fn make() -> Self: P{i} {{ a: +1 }}\nimpl S{i} for P{i}:\n    fn other(self) -> i32: self.a\nfn h{i}[T < S{i}](x: T) -> i32: x.get()\nfn k{i}[T < T{i}]() -> T: T::make()\nfn d{i}(v: &dyn S{i}) -> i32: v.get()\n"
        );
    }
    out
}

// Checker: #1930, a method call asks the module's index of traits by
// method name for the ones bound where it is (the local ones whose extent
// holds it, then the scope's), not every trait of the program declaring
// the name; a call through a bound or a trait value asks its own traits
// whether they declare the name.
#[test]
fn method_calls_ask_only_the_traits_visible_to_them() {
    on_small_stack(|| {
        for (shape, source) in [
            ("local", local_traits_of_one_method as fn(usize) -> String),
            ("bounded", bounded_traits_of_one_method),
        ] {
            let mut base = 0;
            for n in [100, 1_000] {
                let (out, work) =
                    hd_check::work::count_trait_lookup_work(|| compile(&source(n), &Goal::Analyze));
                assert!(out.diags.is_empty(), "{shape} {n}: {}", out.render());
                if n == 100 {
                    base = work;
                }
                assert!(
                    base > 0 && work <= 10 * base * 5 / 4,
                    "{shape} trait lookups for 100 and {n}: {base}, {work}"
                );
            }
        }
    });
}

/// `n` functions, each declaring a local struct `P` with an inherent
/// `get` and calling it: `n` inherent members of one name, each of its
/// own head.
fn inherent_members_of_one_name(n: usize) -> String {
    let mut out = String::from("pub fn main() -> void: pass\n");
    for i in 0..n {
        let _ = write!(
            out,
            "fn g{i}() -> i32:\n    struct P:\n        a: i32\n    impl P:\n        fn get(self) -> i32: self.a\n    p := P {{ a: +1 }}\n    p.get()\n"
        );
    }
    out
}

// Resolver: #1929, a module's inherent members of one name are compared
// only with the earlier ones whose targets share a head, not pairwise.
// Checker: #1934, a method call on a struct tries only the inherent
// methods of its name whose impl's target names that struct, or names no
// struct, not every method of the name in the module.
#[test]
fn inherent_members_are_compared_by_target_head() {
    on_small_stack(|| {
        let mut base = (0, 0);
        for n in [200, 2_000] {
            let ((out, lookups), pairs) = hd_resolve::work::count_member_pairs(|| {
                hd_check::work::count_trait_lookup_work(|| {
                    compile(&inherent_members_of_one_name(n), &Goal::Analyze)
                })
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            if n == 200 {
                base = (pairs, lookups);
            }
            assert!(
                base.0 > 0 && pairs <= 10 * base.0 * 5 / 4,
                "inherent member comparisons for 200 and {n}: {}, {pairs}",
                base.0
            );
            assert!(
                base.1 > 0 && lookups <= 10 * base.1 * 5 / 4,
                "method lookups for 200 and {n}: {}, {lookups}",
                base.1
            );
        }
    });
}

/// `n` traits, each with an implementation, a newtype and a struct that
/// derive `Eq`, and a struct with a block with no trait that derives
/// `Debug`: `3n` derivations, each finding its template, its heads and
/// its target's block.
fn derived_types(n: usize) -> String {
    let mut s = String::from("use std.meta.Structure\n\n");
    for i in 0..n {
        let _ = write!(
            s,
            "trait T{i}:\n    fn f(self) -> i32\n\nstruct S{i}:\n    a: i32\n\nimpl T{i} for S{i}:\n    fn f(self) -> i32: self.a\n\n@derive(Eq)\ntype M{i}(i32)\n\n@derive(Eq)\nstruct D{i}:\n    a: i32\n\n@derive(Debug)\nstruct B{i}:\n    a: i32\n\nimpl B{i} by Structure:\n    a = []\n\n"
        );
    }
    s
}

// Derivations: #1928, a derivation finds its trait's template through an
// index of the templates by trait, its type's heads and its block with no
// trait through the module's index by item, and a newtype its derived
// impls through an index by target, one probe each, not a scan of every
// item or head per derivation.
#[test]
fn derivations_find_what_they_need_by_one_probe_each() {
    on_small_stack(|| {
        let mut base = 0;
        for n in [100, 1_000] {
            let (out, work) =
                hd_resolve::work::count_derive_work(|| compile(&derived_types(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            if n == 100 {
                base = work;
            }
            assert!(
                base > 0 && work <= 10 * base * 5 / 4,
                "derivation lookups for 100 and {n}: {base}, {work}"
            );
        }
    });
}

/// A body binding `p0 := (+1, +2)`, then `p{i} := (p{i-1}, p{i-1})` for
/// `n` levels (a type of 2^n leaves and n distinct nodes, while its
/// literals are open), or `x0 := ["a"]`, then `x{i} := [x{i-1}]`.
fn doubled_types(doubled: bool, n: usize) -> String {
    let mut s = String::from("pub fn main() -> void:\n");
    if doubled {
        s.push_str("    p0 := (+1, +2)\n");
        for i in 1..=n {
            let _ = writeln!(s, "    p{i} := (p{}, p{})", i - 1, i - 1);
        }
        let _ = writeln!(s, "    _ := p{n}");
    } else {
        s.push_str("    x0 := [\"a\"]\n");
        for i in 1..=n {
            let _ = writeln!(s, "    x{i} := [x{}]", i - 1);
        }
        let _ = writeln!(s, "    _ := x{n}");
    }
    s
}

// Types: #1891, resolving variables, the occurs check and the solver's
// folds walk each distinct node of an interned type once (memoized by
// type id), not the type as a tree. A use resolves only the head, or
// nothing when no projection, opaque type or poison is reachable through
// a binding; a closed resolution is kept across calls until a rollback or
// `rebind` undoes a binding under it, so writeback and the occurs check
// resolve each node once per body. Both a chain of list types and a
// doubled tuple whose literals stay open until writeback cost work
// linear in their depth (the tuple's grew as its square, before that as
// 2^depth).
#[test]
fn type_walks_visit_each_distinct_node_once() {
    on_test_stack(|| {
        for (doubled, small) in [(false, 90), (true, 40)] {
            let mut base = 0;
            for n in [small, small * 10] {
                let (out, nodes) = hd_types::work::count_type_nodes(|| {
                    compile(&doubled_types(doubled, n), &Goal::Analyze)
                });
                assert!(out.diags.is_empty(), "{doubled} {n}: {}", out.render());
                if n == small {
                    base = nodes;
                }
                assert!(
                    base > 0 && nodes <= 10 * base * 5 / 4,
                    "type nodes walked for {small} and {n} levels ({doubled}): {base}, {nodes}"
                );
            }
        }
    });
}

/// A doubled tuple `n` levels deep passed on: through a generic call
/// (`id(x{n})`), or through an alias chain `type A{i} = (A{i-1}, A{i-1})`.
/// Or a pair of `Two[...Two[S::Item]]` (`n` deep), each node holding a
/// projection, both halves one type, passed on from a parameter; or the
/// same pair over a type parameter `T` (`param`).
fn passed_doubled(shape: &str, n: usize) -> String {
    let mut s = String::new();
    match shape {
        "generic" => s.push_str("fn id[T](v: T) -> T: v\n"),
        "alias" => {
            s.push_str("type A0 = string\n");
            for i in 1..=n {
                let _ = writeln!(s, "type A{i} = (A{}, A{})", i - 1, i - 1);
            }
            let _ = writeln!(s, "fn id(v: A{n}) -> A{n}: v");
        }
        _ => {
            s.push_str(
                "trait Source:\n    type Item\n\nstruct P: pass\n\n\
                 impl Source for P:\n    type Item = string\n\n\
                 struct Two[T]:\n    a: T\n    b: T\n\n",
            );
            // `param`: the chain holds a parameter itself, not only in a
            // projection, so each level leaves it for the environment.
            let param = shape == "param";
            let bottom = if param { "T" } else { "S::Item" };
            let (mut w, mut v) = (String::from(bottom), String::from("string"));
            for _ in 0..n {
                w = format!("Two[{w}]");
                v = format!("Two[{v}]");
            }
            if param {
                let _ = writeln!(s, "fn id[T](v: ({w}, {w})) -> ({w}, {w}): v");
                let _ = writeln!(s, "fn pass_on(v: ({v}, {v})) -> void:\n    _ := id(v)");
            } else {
                let _ = writeln!(s, "fn id[S < Source](s: S, v: ({w}, {w})) -> ({w}, {w}): v");
                let _ = writeln!(
                    s,
                    "fn pass_on(v: ({v}, {v})) -> void:\n    _ := id(P {{}}, v)"
                );
            }
            return s;
        }
    }
    s.push_str("pub fn main() -> void:\n    x0 := \"a\"\n");
    for i in 1..=n {
        let _ = writeln!(s, "    x{i} := (x{}, x{})", i - 1, i - 1);
    }
    let _ = writeln!(s, "    _ := id(x{n})");
    s
}

// Checker: #1886, passing a shared type on walks each distinct node once.
// The occurs check behind a generic call's binding is the worklist of
// #1891 (5cde4114c). This commit makes the rest walk each distinct node
// once, by type id: `Ck::normalize_deep` returns a type with no
// projection as it is and memoizes the rest; `Types::subst`, the copy
// row's `copy_parts` and the instantiation-cycle check's `params_in` keep
// the nodes they did. Before, each followed the type as a tree: 2^n nodes
// for n levels (the generic call at 60 levels over 60 s).
#[test]
fn passing_a_doubled_type_on_walks_each_node_once() {
    on_test_stack(|| {
        for shape in ["generic", "alias"] {
            let mut base = 0;
            for n in [20, 200] {
                let (out, nodes) = hd_types::work::count_type_nodes(|| {
                    compile(&passed_doubled(shape, n), &Goal::Analyze)
                });
                assert!(out.diags.is_empty(), "{shape} {n}: {}", out.render());
                if n == 20 {
                    base = nodes;
                }
                // The alias chain holds no variable and no projection, so
                // no counted walk enters it: `normalize_deep` returns it at
                // its gate, where it once rebuilt 2^n nodes.
                assert!(
                    (base > 0 || shape == "alias") && nodes <= 10 * base * 5 / 4,
                    "type nodes walked for 20 and {n} levels ({shape}): {base}, {nodes}"
                );
            }
        }
    });
}

// Types: #1985, the `Copy` row is asked at each level of a `Two` chain from
// the top down. Each walk keeps every node's answer it knows (nothing
// left, or the one list its parts leave), so the levels below hit the
// memo. A projection in every node also makes `normalize_deep` take the
// shared half from its memo and `subst` overflow its array into the map.
#[test]
fn copy_goals_down_a_generic_chain_walk_each_level_once() {
    on_test_stack(|| {
        // The first build of a process also checks what the std needs;
        // count only the chains.
        let _ = compile(&passed_doubled("projection", 2), &Goal::Analyze);
        for shape in ["projection", "param"] {
            let mut base = 0;
            for n in [10, 100] {
                let (out, nodes) = hd_types::work::count_type_nodes(|| {
                    compile(&passed_doubled(shape, n), &Goal::Analyze)
                });
                assert!(out.diags.is_empty(), "{shape} {n}: {}", out.render());
                if n == 10 {
                    base = nodes;
                }
                assert!(
                    base > 0 && nodes <= 10 * base * 5 / 4,
                    "type nodes walked for 10 and {n} levels ({shape}): {base}, {nodes}"
                );
            }
        }
    });
}

/// A value of `Two` doubled `n` levels deep, passed to a `Wire` bound or
/// to a library marker's (`module.marker.structural`).
fn marker_doubled(library: bool, n: usize) -> String {
    let mut s = String::from(
        "use std.meta.{Marker, Wire}\n\npub trait Gpu < Marker\n\nimpl Gpu for string\n\nstruct Two[T]:\n    a: T\n    b: T\n\nfn post[T < Wire](v: T) -> void: pass\n\nfn launch[T < Gpu](v: T) -> void: pass\n\npub fn main() -> void:\n    x0 := \"a\"\n",
    );
    for i in 1..=n {
        let _ = writeln!(s, "    x{i} := Two {{ a: x{}, b: x{} }}", i - 1, i - 1);
    }
    let _ = writeln!(s, "    {}(x{n})", if library { "launch" } else { "post" });
    s
}

// Types: a marker goal walks each distinct node of the asked type once, as
// the `Copy` row does (#1886): `Two` doubled n levels is 2^n leaves.
#[test]
fn marker_goals_walk_each_distinct_node_once() {
    on_test_stack(|| {
        let _ = compile(&marker_doubled(false, 2), &Goal::Analyze);
        for library in [false, true] {
            let mut base = 0;
            for n in [10, 100] {
                let (out, nodes) = hd_types::work::count_type_nodes(|| {
                    compile(&marker_doubled(library, n), &Goal::Analyze)
                });
                assert!(out.diags.is_empty(), "{library} {n}: {}", out.render());
                if n == 10 {
                    base = nodes;
                }
                assert!(
                    base > 0 && nodes <= 10 * base * 5 / 4,
                    "type nodes walked for 10 and {n} levels (library {library}): {base}, {nodes}"
                );
            }
        }
    });
}

/// A chain of `n` functions down to one that reads a top-level `let`,
/// and `n` crossing closures, each calling the head of the chain
/// (`fn.crossing.global`).
fn crossing_chain(n: usize) -> String {
    let mut s = String::from(
        "use std.meta.Wire\n\nfn run(code: fn() -> i32 + Wire) -> i32:\n    code()\n\nlet counter = +0\n\n",
    );
    let _ = writeln!(s, "fn g{n}() -> i32:\n    counter\n");
    for i in (0..n).rev() {
        let _ = writeln!(s, "fn g{i}() -> i32:\n    g{}()\n", i + 1);
    }
    s.push_str("pub fn main() -> void:\n");
    for _ in 0..n {
        s.push_str("    _ := run(fn() -> i32: g0())\n");
    }
    s
}

// Checker: the reached-binding check of crossing closures pushes each
// node's verdict to its callers once, then reads each closure's own facts:
// linear in the module's call edges and closures, not their product.
#[test]
fn crossing_reach_is_linear_in_calls_and_closures() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) =
                hd_check::work::count_scan_work(|| compile(&crossing_chain(n), &Goal::Analyze));
            let errors = out.diags.len();
            assert_eq!(errors, n, "{n}: one error per closure: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 100 and 1,000 crossing closures down a chain: {small} and {large}"
        );
    });
}

/// `n` enums, each with a `Serialize` derivation block of its own.
fn enum_views(n: usize) -> String {
    let mut s = String::from(
        "use std.encoding.Serialize\nuse std.meta.Structure\n\npub fn main() -> void: pass\n",
    );
    for i in 0..n {
        let _ = write!(
            s,
            "\nenum E{i}:\n    A(v: i32)\n\nimpl Serialize for E{i} by Structure:\n    Self += []\n"
        );
    }
    s
}

/// `n` calls each passing `&mut` of an element of one list beside another
/// argument, which asks `ref.excl.args` (#2134).
fn element_loan_calls(n: usize) -> String {
    let mut s = String::from(
        "fn bump(x: &mut i32, by: i32) -> void:\n    *x += by\n\npub fn main() -> void $ Console:\n    let mut xs: List[i32] = [+0, +0, +0]\n",
    );
    for i in 0..n {
        let _ = writeln!(s, "    bump(&mut xs[{}], xs[{}])", i % 3, (i + 1) % 3);
    }
    s.push_str("    println(xs[0])\n");
    s
}

// Checker: #2134, a call's exclusivity check reads the body's first users,
// writes, moves and uses once per body, and each argument's accesses
// through a window of them: it rescanned the whole body once per element
// loan per argument.
#[test]
fn element_loan_arguments_scan_the_body_once() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) =
                hd_check::work::count_scan_work(|| compile(&element_loan_calls(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        // The body's first users once, then per call its arguments'
        // components and their windows of later accesses.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 100 and 1,000 element-loan calls: {small} and {large}"
        );
    });
}

/// `n` calls each passing a reference to one local beside another
/// argument: each is a site loan whose argument region is its own
/// component (#2154).
fn reference_calls(n: usize) -> String {
    let mut s = String::from(
        "fn look(x: &i32, by: i32) -> i32:\n    *x + by\n\npub fn main() -> void $ Console:\n    let a = +1\n    let mut t = +0\n",
    );
    for _ in 0..n {
        s.push_str("    t += look(&a, t)\n");
    }
    s.push_str("    println(t)\n");
    s
}

// Checker: #2154, a solved component's loan set costs its members: the
// dense bitset grew to the highest loan id, so k reference-taking calls
// held about k components of up to k/64 words each.
#[test]
fn reference_calls_read_loan_sets_by_their_members() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) =
                hd_check::work::count_loan_words(|| compile(&reference_calls(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "loan-set words for 100 and 1,000 reference-taking calls: {small} and {large}"
        );
    });
}

/// `n` built-in pushes, each onto an element of one list, whose argument
/// takes `&mut` of another list (`ref.elem.access.live`).
fn element_receiver_pushes(n: usize) -> String {
    let mut s = String::from(
        "fn grow(v: &mut List[i32]) -> i32:\n    v.push(+1)\n    +0\n\npub fn main() -> void $ Console:\n    let mut xs: List[List[i32]] = [[+0], [+0], [+0]]\n    let mut ys: List[i32] = []\n",
    );
    for i in 0..n {
        let _ = writeln!(s, "    xs[{}].push(grow(&mut ys))", i % 3);
    }
    s.push_str("    println(xs[0][0])\n");
    s
}

// Checker: an element receiver's access is checked against the writes
// between the receiver and its call, one window of the body's sorted
// writes per call.
#[test]
fn element_receivers_read_one_window_of_writes_each() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) = hd_check::work::count_scan_work(|| {
                compile(&element_receiver_pushes(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 100 and 1,000 element-receiver pushes: {small} and {large}"
        );
    });
}

// Checker: #1910, the enum tagging pass finds each enum's derivation
// blocks in an index of the heads built once: a scan of every head per
// enum was quadratic in the items.
#[test]
fn enum_tagging_reads_each_enums_blocks_from_one_index() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) =
                hd_check::work::count_scan_work(|| compile(&enum_views(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let small = work(200);
        let large = work(2_000);
        // The heads indexed once, then the enum and its one block: about
        // 4 per enum, where the scan read every head per enum.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 200 and 2,000 enums: {small} and {large}"
        );
    });
}

/// One struct of `n` fields, built with every field and read field by
/// field, and a function that takes it by value: the scale check's
/// `wide-struct` shape.
fn wide_struct(n: usize) -> String {
    let mut s = String::from("struct Wide:\n");
    for i in 0..n {
        let _ = writeln!(s, "    f{i}: i32");
    }
    s.push_str("\nfn total(w: Wide) -> i32:\n    w.f0\n");
    s.push_str("\npub fn main() -> void $ Console:\n    let t = +0\n    w := Wide {\n");
    for i in 0..n {
        let _ = writeln!(s, "        f{i}: +1,");
    }
    s.push_str("    }\n");
    for i in 0..n {
        let _ = writeln!(s, "    t = t + w.f{i}");
    }
    s.push_str("    println(\"${t + total(w)}\")\n");
    s
}

// Checker and emitter: #1926, a struct's fields are found by name in one
// index per struct, a type's copy answer is walked once per pass and per
// run, and a field read takes its place from one layout per struct: each
// read walked or laid out every field of the struct.
#[test]
fn wide_struct_reads_cost_one_probe_per_field() {
    on_test_stack(|| {
        let check = |n: usize| {
            let (out, work) =
                hd_check::work::count_scan_work(|| compile(&wide_struct(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (check(200), check(2_000));
        // Per field: its index entry, its literal and read probes, and one
        // step of the one copy walk.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 200 and 2,000 fields: {small} and {large}"
        );
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |n: usize| {
            let (out, work) =
                hd_layout::work::count_member_layouts(|| compile(&wide_struct(n), &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (build(200), build(2_000));
        // The struct laid out once, then one probe per field read.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "member layouts for 200 and 2,000 fields: {small} and {large}"
        );
    });
}

/// One enum with `n` payload variants, each read by a match arm (#1909).
fn enum_payload_arms(n: usize) -> String {
    let mut s = String::from("enum Big:\n    Zero\n");
    for i in 0..n {
        let _ = writeln!(s, "    V{i}(x: i32)");
    }
    s.push_str("\nfn weigh(b: Big) -> i32:\n    match b:\n        .Zero => +0\n");
    for i in 0..n {
        let _ = writeln!(s, "        .V{i}(x) => x + 1");
    }
    s.push_str("\npub fn main() -> void $ Console:\n    println(weigh(Big.Zero))\n");
    s
}

#[test]
fn enum_arm_payload_layout_work_is_linear() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |n| {
            let (out, work) =
                hd_layout::work::count_member_layouts(|| compile(&enum_payload_arms(n), &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (build(100), build(1_000));
        eprintln!("enum payload layout work for 100 and 1,000 arms: {small} and {large}");
        assert!(
            small > 0 && large <= 10 * small + 100,
            "enum payload layout work for 100 and 1,000 arms: {small} and {large}"
        );
    });
}

/// One string of `n` interpolations of one local, its length read back:
/// the scale check's `interpolation` shape (#1914).
fn interpolated_local(n: usize) -> String {
    format!(
        "pub fn main() -> void $ Console:\n    let t = +0\n    s := \"start{}\"\n    t = i32(s.len())\n    println(\"$t\")\n",
        " $t".repeat(n)
    )
}

/// `n` values, then an else-if chain of `n` arms whose last arm reads all
/// of them: each value is live past every arm but not in it, so a range
/// per local with a hole per arm it skips would be `n²` pieces.
fn else_if_reads_past_arms(n: usize) -> String {
    let mut s = String::from("fn pick(k: i32) -> i32:\n");
    for i in 0..n {
        let _ = writeln!(s, "    v{i} := k * {i} + 1");
    }
    for i in 0..n {
        let head = if i == 0 { "if" } else { "else if" };
        let _ = writeln!(s, "    {head} k == {i}:\n        k + {i}");
    }
    let sum: Vec<String> = (0..n).map(|i| format!("v{i}")).collect();
    let _ = writeln!(s, "    else:\n        {}", sum.join(" + "));
    s.push_str("\npub fn main() -> void $ Console:\n    r := pick(+7)\n    println(\"$r\")\n");
    s
}

// Emitter: #1935 and #1914, the backend's liveness keeps each block's live
// set as a persistent set that shares what the block leaves alone, and
// local coalescing is a linear scan over live ranges with holes: each block
// kept its own copy of every live local, and coalescing built a locals by
// locals conflict matrix, so `n` values live across `n` blocks cost `n²`.
#[test]
fn liveness_and_coalescing_work_is_linear_in_live_values() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |source: String| {
            let (out, work) = hd_wasm::work::count_liveness(|| compile(&source, &goal));
            assert!(!out.diags.has_errors(), "{}", out.render());
            work
        };
        // #1914's smallest size, #1935's largest, and an else-if chain
        // whose `n` arms all skip `n` live values, each with the size 10x
        // from it. A persistent set's insert copies one path of about
        // `log2 n` nodes, so work grows by 10x times the growth of `log2 n`
        // (12.1x, 10.0x and 10.1x measured), within 14x; the quadratic
        // sets grew 100x, and a hole per value per arm passed 8 GB at
        // 2,000 arms.
        for (shape, generate, small) in [
            (
                "interpolation",
                interpolated_local as fn(usize) -> String,
                500,
            ),
            ("wide-struct", wide_struct, 800),
            ("else-if", else_if_reads_past_arms, 200),
        ] {
            // The marker keeps another test's build of the same program,
            // already in the compiler's cache, from answering for this one.
            let marked = |n: usize| generate(n) + "\nfn liveness_only() -> i32:\n    1\n";
            let (a, b) = (build(marked(small)), build(marked(small * 10)));
            assert!(
                a > 0 && b <= a * 14,
                "{shape}: liveness work for {small} and {}: {a} and {b}",
                small * 10
            );
        }
    });
}

/// #1895: keep every trimmed string alive until the final length sum.
fn live_string_temporaries(n: usize) -> String {
    let mut s = String::from("pub fn main() -> void $ Console:\n");
    for i in 0..n {
        let _ = writeln!(s, "    s{i} := \"a{i}\".trim()");
    }
    s.push_str("    total := ");
    for i in 0..n {
        if i != 0 {
            s.push_str(" + ");
        }
        let _ = write!(s, "s{i}.len()");
    }
    s.push_str("\n    println(total)\n");
    s
}

#[test]
fn string_temporary_coalescing_work_is_near_linear() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |n| {
            let (out, work) =
                hd_wasm::work::count_liveness(|| compile(&live_string_temporaries(n), &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            eprintln!("string temporary liveness/coalescing at {n}: {work}");
            work
        };
        let (a, b, c) = (build(1_000), build(4_000), build(10_000));
        // Persistent live sets add a logarithmic path per local, rather
        // than constructing pairwise interference or a dense row per block.
        assert!(a > 0 && b <= a * 6, "1000/4000: {a}/{b}");
        assert!(c <= a * 14, "1000/10000: {a}/{c}");
    });
}

// Emitter: #1914, rule 11's zero facts were a dense bit row of every
// local per waiting block, copied at each forward branch, so `n` locals
// across `n` blocks allocated `n²/64` words. They are persistent sets now,
// shared between blocks and met by intersection.
#[test]
fn zero_fact_work_is_linear_in_interpolations() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |source: String| {
            let (out, work) = hd_wasm::work::count_zero_facts(|| compile(&source, &goal));
            assert!(!out.diags.has_errors(), "{}", out.render());
            work
        };
        // An insert copies a path of about `log2 n` nodes, so 10x the
        // parts grows the work by 10x times the growth of `log2 n` (12.2x
        // measured), within 14x; the dense rows grew with the square.
        let (a, b) = (
            build(interpolated_local(500)),
            build(interpolated_local(5_000)),
        );
        assert!(
            a > 0 && b <= a * 14,
            "zero facts for 500 and 5,000: {a} and {b}"
        );
        // The scale check's `let-statements` shape: `n` locals written in
        // one block. The entry set is made in one pass and a block carries
        // its writes in a dense row, so no write copies a path, and no set
        // is hashed: linear, where an insert per local and a removal per
        // write grew 4.68x in hashes per 4x locals.
        let (a, b) = (build(let_statements(500)), build(let_statements(5_000)));
        assert!(
            a > 0 && b <= a * 10 * 5 / 4,
            "zero facts for 500 and 5,000 `let`s: {a} and {b}"
        );
    });
}

/// The scale check's `let-statements` shape: `n` locals of one body, each
/// set from a running total and then added to it.
fn let_statements(n: usize) -> String {
    let mut s = String::from("pub fn main() -> void $ Console:\n    let t = +0\n");
    for i in 0..n {
        let _ = writeln!(s, "    let v{i} = t + 1");
    }
    for i in 0..n {
        let _ = writeln!(s, "    t = t + v{i}");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// One body of `n` calls, each lending a part of one local to a callee
/// that only reads it: `n` argument promotion sites.
fn promoted_calls(n: usize) -> String {
    let mut s = String::from(
        "use std.meta.inline\n\nstruct Wrap:\n    name: string\n\n@inline(.Never)\nfn label(w: &Wrap) -> usize:\n    w.name.len() + 1\n\npub fn main() -> void $ Console:\n    let w = Wrap { name: \"x\" }\n    let t = +0\n",
    );
    for _ in 0..n {
        s.push_str("    t = t + label(&w)\n");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

// Mono: #1914, argument promotion scanned the whole caller body for each
// call site (`promotable_args`), so `n` calls cost `n²`; the body's
// census is made once now and each argument is one lookup.
#[test]
fn promotion_census_work_is_linear_in_call_sites() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |source: String| {
            let ((out, work), outer) = hd_tir::own::promote::count_census(|| {
                hd_tir::own::promote::count_census(|| compile(&source, &goal))
            });
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert_eq!(outer, work, "nested censuses propagate the same work");
            work
        };
        // The census reads each instruction once and each argument is one
        // lookup: 5,102 and 21,302 measured, the std bodies a constant.
        let (a, b) = (build(promoted_calls(200)), build(promoted_calls(2_000)));
        assert!(
            a > 0 && b <= a * 10,
            "census work for 200 and 2,000 calls: {a} and {b}"
        );
    });
}

/// One function whose row names `n` traits, each declaring a method of
/// one name, and uses each with `$.use`: the scale check's `row-traits`
/// shape, plus a caller under the same keys in reverse order.
fn row_traits(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = write!(s, "trait R{i}:\n    fn r(&self) -> i32\n\n");
    }
    let keys: Vec<String> = (0..n).map(|i| format!("R{i}")).collect();
    let _ = writeln!(s, "fn needs() -> i32 $ Console + {}:", keys.join(" + "));
    s.push_str("    let t = +0\n");
    for i in 0..n {
        let _ = writeln!(s, "    t = t + $.use(R{i}).r()");
    }
    s.push_str("    t\n\n");
    let reversed: Vec<&str> = keys.iter().rev().map(String::as_str).collect();
    let _ = writeln!(
        s,
        "fn caller() -> i32 $ Console + {}:",
        reversed.join(" + ")
    );
    s.push_str("    needs()\n\npub fn main() -> void $ Console:\n    println(\"0\")\n");
    s
}

// Checker: #1921, a row's keys are indexed once per body, by trait and
// supertrait: each `$.use` scanned every key of the row, walking each
// one's supertraits.
#[test]
fn row_keys_cost_one_probe_per_use() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) =
                hd_check::work::count_scan_work(|| compile(&row_traits(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        // Per key: its index entry and walk, and its `$.use` probe.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 100 and 1,000 keys: {small} and {large}"
        );
    });
}

/// `n` types that implement `Disposable`, each disposed by a `using`
/// binding in a function main calls: the form of the scale check's
/// `syntax-items` shape that grew.
fn using_bindings(n: usize) -> String {
    let mut s = String::from("use std.ops.Disposable\n\n");
    for i in 0..n {
        let _ = write!(
            s,
            "struct Res{i}:\n    n: i32\n\nimpl Disposable for Res{i}:\n    fn dispose(&mut self) -> void:\n        pass\n\nfn run{i}(x: i32) -> i32:\n    using res = box(Res{i} {{ n: x }})\n    x\n\n"
        );
    }
    s.push_str("pub fn main() -> void $ Console:\n    let t = +0\n");
    for i in 0..n {
        let _ = writeln!(s, "    t = run{i}(t)");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

// Checker: #1923, a trait method call whose impl is chosen reaches that
// impl's method in the module's initialization facts, and a binding's
// scope is found up its parent links: every `using` reached every
// `dispose` of the module, and every scope question scanned the module's
// items from the root.
#[test]
fn using_bindings_reach_their_own_dispose_and_scope() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) =
                hd_check::work::count_scan_work(|| compile(&using_bindings(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(200), work(2_000));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 200 and 2,000 `using` bindings: {small} and {large}"
        );
    });
}

/// `n` traits that each declare a method of one name, with a defaulted
/// parameter, each implemented by its own struct and called once: the
/// scale check's `type-defaults` shape.
fn same_name_traits(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = write!(
            s,
            "trait Mix{i}[Rhs = Self]:\n    fn mix(&self, other: Rhs) -> i32\n\nstruct P{i}:\n    a: i32\n\nimpl Mix{i} for P{i}:\n    fn mix(&self, other: P{i}) -> i32: self.a + other.a\n\n"
        );
    }
    s.push_str("pub fn main() -> void $ Console:\n    let t = +0\n");
    for i in 0..n {
        let _ = writeln!(
            s,
            "    p{i} := P{i} {{ a: +1 }}\n    t = p{i}.mix(P{i} {{ a: t }})"
        );
    }
    s.push_str("    println(\"$t\")\n");
    s
}

// Checker: #1925, a method call on a receiver of a known head asks only
// the traits with an impl row for that head (each impl table's inverse
// head index), and the compiler-supplied ones: each call solved a goal
// for every available trait declaring the name.
#[test]
fn same_name_trait_methods_ask_the_traits_with_rows_for_the_receiver() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) =
                hd_check::work::count_scan_work(|| compile(&same_name_traits(n), &Goal::Analyze));
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        // Per call: the traits with rows for its receiver (one) and the
        // supplied ones, where every trait declaring `mix` was asked.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 100 and 1,000 traits: {small} and {large}"
        );
    });
}

/// `n` structs, each holding the one before by value, each with a
/// constructor and a getter that reads the part before: #2030's shape.
fn by_value_struct_chain(n: usize) -> String {
    let mut s = String::from(
        "struct T0:\n    v: i32\n\nfn mk0() -> T0: T0 { v: +1 }\nfn get0(t: T0) -> i32: t.v\n",
    );
    for i in 1..n {
        let p = i - 1;
        let _ = write!(
            s,
            "\nstruct T{i}:\n    inner: T{p}\n    v: i32\n\nfn mk{i}() -> T{i}: T{i} {{ inner: mk{p}(), v: +{i} }}\nfn get{i}(t: T{i}) -> i32: get{p}(t.inner) * 3 + 1\n"
        );
    }
    let last = n - 1;
    let _ = write!(
        s,
        "\npub fn main() -> void $ Console:\n    println(\"${{get{last}(mk{last}())}}\")\n"
    );
    s
}

// Driver and wasm: #2030, outside a link the ownership passes share one
// layout cache per run on each thread, so each type of the chain is laid
// out from its parts once per run: each body made a cache of its own and
// laid out every part of the chain it touched again.
#[test]
fn a_by_value_struct_chain_is_laid_out_once_per_run() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |n: usize| {
            let (out, work) =
                hd_layout::work::count_layout_parts(|| compile(&by_value_struct_chain(n), &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (build(100), build(1_000));
        // Per struct: its layout and walks from its parts, once per
        // query kind, in Collect and in the link.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "layout parts for 100 and 1,000 structs: {small} and {large}"
        );
    });
}

/// A tuple doubled through `n` aliases (`type D1 = (D0, D0)`) over the
/// empty tuple, so its `2^n` parts take no storage, built level by level
/// and passed through a function: a generic one at `D{n}` when `generic`,
/// else a plain one. The source grows linearly in `n`.
fn doubled_through_aliases(n: usize, generic: bool) -> String {
    let mut s = String::from("type D0 = ()\n");
    for i in 1..=n {
        let _ = writeln!(s, "type D{i} = (D{p}, D{p})", p = i - 1);
    }
    s.push_str("\nfn make0() -> D0: ()\n");
    for i in 1..=n {
        let _ = write!(
            s,
            "fn make{i}() -> D{i}:\n    let h = make{p}()\n    (h, h)\n",
            p = i - 1
        );
    }
    if generic {
        s.push_str("\nfn hold[T](x: T) -> T: x\n");
    } else {
        let _ = writeln!(s, "\nfn hold(x: D{n}) -> D{n}: x");
    }
    let _ = write!(
        s,
        "\npub fn main() -> void $ Console:\n    let v = hold(make{n}())\n    let (a, _) = v\n    println(\"held\")\n"
    );
    s
}

// Driver: the ownership passes ask a type's layout through the run's
// memo keyed by type id, each entry made from its parts' entries:
// `holds_ref` and `counted_refs_only` walked every part of a type, so
// depth 24 took 9 s and depth 130 did not end. Collection keeps instance
// arguments under `instantiation-too-deep`'s 32 levels, so the generic
// shape runs at 3 and 30.
#[test]
fn layout_probes_grow_linearly_in_alias_depth() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let probes = |n: usize, generic: bool| {
            let (out, probes) = hd_driver::work::count_layout_probes(|| {
                compile(&doubled_through_aliases(n, generic), &goal)
            });
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            probes
        };
        for (small, large, generic) in [(13, 130, false), (3, 30, true)] {
            // The first build owns std's bodies, which later builds take
            // from the store; depth 1 is then the probes of the program's
            // other bodies, which the depth adds to.
            probes(2, generic);
            let base = probes(1, generic);
            let (a, b) = (probes(small, generic), probes(large, generic));
            let (a, b) = (a.saturating_sub(base), b.saturating_sub(base));
            // Per level past the first, the large depth asks at most twice
            // what the small one does: 44 at 13 and at 130, plain.
            let levels = |n: usize| u64::try_from(n - 1).expect("a depth");
            assert!(
                a > 0 && b * levels(small) <= 2 * a * levels(large),
                "layout probes past depth 1 at depth {small} and {large} \
                 (generic: {generic}): {a} and {b}"
            );
        }
    });
}

/// A list of a tuple doubled through `n` aliases over `i32`, so its
/// type takes 2^(n + 2) bytes, past `types.limit.value-size` from 29 on.
/// The source grows linearly in `n`.
fn list_of_doubled_i32(n: usize) -> String {
    let mut s = String::from("type D0 = i32\n");
    for i in 1..=n {
        let _ = writeln!(s, "type D{i} = (D{p}, D{p})", p = i - 1);
    }
    let _ = write!(
        s,
        "\npub fn main() -> void $ Console:\n    let xs: List[D{n}] = []\n    println(xs.len())\n"
    );
    s
}

// #2191: an ownership walk of scalar-only shared types is empty. Finding
// that fact must not flatten 2^depth scalar leaves to assign walk offsets.
#[test]
fn scalar_doubling_ownership_keeps_layout_work_linear() {
    on_test_stack(|| {
        let measure = |depth: usize| {
            let mut text = String::from("type D0 = i32\n");
            for i in 1..=depth {
                let _ = writeln!(text, "type D{i} = (D{p}, D{p})", p = i - 1);
            }
            let _ = write!(
                text,
                "fn make() -> D{depth}: panic(\"not reached\")\n\n\
                 let mut large: D{depth} = make()\n\n\
                 fn touch(value: &mut D{depth}) -> void: ()\n\n\
                 pub fn main() -> void: touch(&mut large)\n"
            );
            let ((out, leaves), parts) = hd_layout::work::count_layout_parts(|| {
                hd_layout::work::count_layout_leaves(|| {
                    compile(
                        &text,
                        &Goal::Program {
                            entry: "main".into(),
                        },
                    )
                })
            });
            let codes: Vec<_> = out.diags.code.iter().map(|c| c.as_str()).collect();
            assert_eq!(codes, ["type-too-large"], "{}", out.render());
            eprintln!("scalar ownership, depth {depth}: {leaves} leaves, {parts} parts");
            // The size-only layouts copy at most INDIRECT scalar leaves
            // per child, two children per level. A full physical layout
            // of even D28 would copy hundreds of millions instead.
            assert!(leaves <= 2 * hd_layout::INDIRECT as u64 * parts);
            parts
        };
        // Every root is oversized, so codegen must reject it without
        // materializing a valid global's genuinely required scalar ABI.
        // Grow the source by 13 and 26 further shared levels past D29.
        measure(29);
        let base = measure(29);
        let small = measure(42).saturating_sub(base);
        let large = measure(55).saturating_sub(base);
        assert!(
            small > 0 && large <= 2 * small,
            "13/26 added levels: {small}/{large} layout parts"
        );
    });
}

// Wasm: #2111, a type is checked against the value-size limit through its
// shape, its size, alignment and niche without its leaves, made once per
// distinct type from its parts' shapes: the layout copied each part's
// leaves first, so depth 130 laid out 2^28 leaves of its 28th level (6.9
// GB) before its 29th overflowed a `u32` sum. Depth 13 builds; depth 130
// is `type-too-large` after at most as many layouts per level.
#[test]
fn a_type_past_the_value_size_limit_stops_without_its_leaves() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let build = |n: usize| {
            hd_layout::work::count_layout_parts(|| compile(&list_of_doubled_i32(n), &goal))
        };
        // The first build lays out std's types too; depth 1 is then the
        // program's other layouts, which the depth adds to.
        build(1);
        let (_, base) = build(1);
        let (small, a) = build(13);
        assert!(!small.diags.has_errors(), "{}", small.render());
        let (large, b) = build(130);
        let codes: Vec<&str> = large.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(codes, ["type-too-large"], "{}", large.render());
        let (a, b) = (a.saturating_sub(base), b.saturating_sub(base));
        eprintln!("layout parts past depth 1: depth 13 {a}, depth 130 {b}");
        assert!(
            a > 0 && b <= 10 * a,
            "layout parts past depth 1 at depth 13 and 130: {a} and {b}"
        );
    });
}

/// One struct implementing `n` traits, each declaring its own method,
/// and each method called once: #2029's shape.
fn struct_of_many_traits(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = write!(s, "trait T{i}:\n    fn m{i}(&self) -> i32\n\n");
    }
    s.push_str("struct S:\n    v: i32\n\n");
    for i in 0..n {
        let _ = write!(
            s,
            "impl T{i} for S:\n    fn m{i}(&self) -> i32: self.v + {i}\n\n"
        );
    }
    s.push_str("fn run() -> i32:\n    s := S { v: +1 }\n    let total = +0\n");
    for i in 0..n {
        let _ = writeln!(s, "    total = total + s.m{i}()");
    }
    s.push_str("    total\n\npub fn main() -> void $ Console:\n    println(\"${run()}\")\n");
    s
}

// Checker: #2029, a method call finds the traits of its receiver's head
// that declare its name through one index from (head, name), made once
// per head and module, and the call's auto-address asks the same index:
// each call probed every trait the head implements.
#[test]
fn many_trait_impls_find_a_method_by_its_name() {
    on_test_stack(|| {
        let work = |n: usize| {
            let ((out, scan), lookup) = hd_check::work::count_trait_lookup_work(|| {
                hd_check::work::count_scan_work(|| {
                    compile(&struct_of_many_traits(n), &Goal::Analyze)
                })
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            (scan, lookup)
        };
        let (small, large) = (work(100), work(1_000));
        // Per head: one step per trait and per method; per call: one
        // probe and the one trait declaring the name.
        assert!(
            small.0 > 0 && large.0 <= 10 * small.0 + 100,
            "scan work for 100 and 1,000 traits: {} and {}",
            small.0,
            large.0
        );
        assert!(
            small.1 > 0 && large.1 <= 10 * small.1 + 100,
            "trait lookups for 100 and 1,000 traits: {} and {}",
            small.1,
            large.1
        );
    });
}

/// #2029's shape across modules: `traits.hd` declares `n` traits,
/// `impls.hd` a struct implementing each, and each of `n` more modules
/// calls one of its methods.
fn struct_of_many_traits_called_from_modules(n: usize) -> MemorySources {
    let mut src = MemorySources::default();
    let mut traits = String::new();
    for i in 0..n {
        let _ = write!(traits, "pub trait T{i}:\n    fn m{i}(&self) -> i32\n\n");
    }
    src.insert("traits.hd", &traits);
    let mut impls = String::new();
    for i in 0..n {
        let _ = writeln!(impls, "use pkg.traits.T{i}");
    }
    impls.push_str("\npub struct S:\n    pub v: i32\n\n");
    for i in 0..n {
        let _ = write!(
            impls,
            "impl T{i} for S:\n    fn m{i}(&self) -> i32: self.v + {i}\n\n"
        );
    }
    src.insert("impls.hd", &impls);
    for i in 0..n {
        src.insert(
            &format!("c{i}.hd"),
            &format!(
                "use pkg.impls.S\nuse pkg.traits.T{i}\n\npub fn call{i}(s: S) -> i32: s.m{i}()\n"
            ),
        );
    }
    src.insert(
        "main.hd",
        "pub fn main() -> void $ Console:\n    println(\"0\")\n",
    );
    src
}

// Checker: #2029 across modules, each impl table indexes its traits by
// (head key, method name) once per run, and a module asks only the
// tables with rows for the receiver's head: each module that called a
// method on the struct collected and probed all of its traits again.
#[test]
fn many_trait_impls_called_from_many_modules_index_them_once() {
    on_test_stack(|| {
        let work = |n: usize| {
            let src = struct_of_many_traits_called_from_modules(n);
            let store = crate::warm_store();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let ((out, scan), lookup) = hd_check::work::count_trait_lookup_work(|| {
                hd_check::work::count_scan_work(|| build(&host, "app", &Goal::Analyze))
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            assert!(
                out.counters.modules_checked.len() > n,
                "{n}: every caller module is checked"
            );
            (scan, lookup)
        };
        let (small, large) = (work(100), work(1_000));
        assert!(
            small.0 > 0 && large.0 <= 10 * small.0 + 100,
            "scan work for 100 and 1,000 modules: {} and {}",
            small.0,
            large.0
        );
        assert!(
            small.1 > 0 && large.1 <= 10 * small.1 + 100,
            "trait lookups for 100 and 1,000 modules: {} and {}",
            small.1,
            large.1
        );
    });
}

/// One struct of `n` fields, and `n` functions that each hold a tuple of
/// two values of it and compare them: review F15's shape, with the
/// struct inside a body's own tuple type so the body's containment walk
/// reaches its fields.
fn compared_wide_values(n: usize) -> String {
    let mut s = String::from("@derive(Eq)\nstruct Wide:\n");
    for i in 0..n {
        let _ = writeln!(s, "    f{i}: i32");
    }
    for i in 0..n {
        let _ = write!(
            s,
            "\nfn same{i}(a: Wide, b: Wide) -> bool:\n    pair := (a, b)\n    pair._0 == pair._1\n"
        );
    }
    s.push_str("\npub fn main() -> void $ Console:\n    println(\"0\")\n");
    s
}

// Checker: #1894, a closed type found clean by one body's containment
// walk is published for the run's other bodies in that impl universe:
// every body that held the struct walked all its fields again.
#[test]
fn compared_wide_values_walk_the_fields_once_per_run() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) = hd_check::work::count_scan_work(|| {
                compile(&compared_wide_values(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(200), work(2_000));
        // Per field: one step of the one walk; per body: its tuple and
        // one probe of the published struct.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 200 and 2,000 fields and bodies: {small} and {large}"
        );
    });
}

/// A trait and `n` generic structs, each implementing it by calling the
/// method through its parameter's bound: review F11's shape, where each
/// body's init facts listed every impl method of the module.
fn bound_dispatch_impls(n: usize) -> String {
    let mut s = String::from("trait Get:\n    fn get(&self) -> i32\n");
    for i in 0..n {
        let _ = write!(
            s,
            "\nstruct W{i}[A]:\n    v: A\n\nimpl[A < Get] Get for W{i}[A]:\n    fn get(&self) -> i32: self.v.get()\n"
        );
    }
    s.push_str("\npub fn main() -> void $ Console:\n    println(\"0\")\n");
    s
}

// Checker and driver: #1893, a call through a bound names its dispatch
// set by one key in a body's init facts, and the module's summary lists
// each key's methods once: every body listed every impl method, so the
// summary was quadratic, and at 6,000 impls its `check` entry passed the
// disk cache's entry limit and a no-edit rerun checked everything again.
#[test]
fn bound_dispatch_names_its_impls_once_per_module() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) = hd_check::work::count_scan_work(|| {
                compile(&bound_dispatch_impls(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(200), work(2_000));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 200 and 2,000 impls: {small} and {large}"
        );
        let bytes = |n: usize| {
            let mut src = MemorySources::default();
            src.insert("main.hd", &bound_dispatch_impls(n));
            let store = hd_cache::MemoryStore::default();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let cold = build(&host, "app", &Goal::Analyze);
            assert!(cold.diags.is_empty(), "{n}: {}", cold.render());
            let written = store.bytes();
            let warm = build(&host, "app", &Goal::Analyze);
            assert!(
                warm.counters.modules_checked.is_empty(),
                "{n}: a rerun with no edit checked {:?}",
                warm.counters.modules_checked
            );
            assert_eq!(store.bytes(), written, "{n}: a rerun wrote entries");
            written
        };
        let (small, large) = (bytes(200), bytes(2_000));
        assert!(
            large <= 12 * small,
            "cache bytes for 200 and 2,000 impls: {small} and {large}"
        );
    });
}

/// A trait, `n` structs implementing it, and `n` functions each calling
/// it through a trait value: #1936's trait-value form.
fn trait_value_dispatch_impls(n: usize) -> String {
    let mut s = String::from("trait Get:\n    fn get(&self) -> i32\n");
    for i in 0..n {
        let _ = write!(
            s,
            "\nstruct W{i}:\n    v: i32\n\nimpl Get for W{i}:\n    fn get(&self) -> i32: self.v\n\nfn call{i}(g: &dyn Get) -> i32: g.get()\n"
        );
    }
    s.push_str("\npub fn main() -> void $ Console:\n    println(\"0\")\n");
    s
}

// Checker and driver: #1936, a call through a trait value names its
// dispatch set by one key in each body's init facts, as a call through a
// bound does (`bound_dispatch_names_its_impls_once_per_module`): n
// bodies calling through `&dyn Get` with n impls of `Get` in the module
// store n keys, not n^2 methods, and the summary lists the key's methods
// once.
#[test]
fn trait_value_dispatch_names_its_impls_once_per_module() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) = hd_check::work::count_scan_work(|| {
                compile(&trait_value_dispatch_impls(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(200), work(2_000));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 200 and 2,000 impls: {small} and {large}"
        );
        let bytes = |n: usize| {
            let mut src = MemorySources::default();
            src.insert("main.hd", &trait_value_dispatch_impls(n));
            let store = hd_cache::MemoryStore::default();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let out = build(&host, "app", &Goal::Analyze);
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            store.bytes()
        };
        let (small, large) = (bytes(200), bytes(2_000));
        assert!(
            large <= 12 * small,
            "cache bytes for 200 and 2,000 impls: {small} and {large}"
        );
    });
}

/// `x.go(if true: x.go(…) else: +0)` nested `depth` deep under an `i32`
/// binding, with three instantiations of `Go` for `X`: review F9's shape.
fn nested_method_choice(depth: usize) -> String {
    let mut s =
        String::from("trait Go[A]:\n    fn go(&self, a: A) -> A\n\nstruct X:\n    n: i32\n");
    for t in ["i32", "i64", "f64"] {
        let _ = write!(
            s,
            "\nimpl Go[{t}] for X:\n    fn go(&self, a: {t}) -> {t}: a\n"
        );
    }
    let mut e = String::from("+0");
    for _ in 0..depth {
        e = format!("x.go(if true: {e} else: +0)");
    }
    let _ = write!(
        s,
        "\npub fn main() -> void $ Console:\n    x := X {{ n: 1 }}\n    let y: i32 = {e}\n    println(\"$y\")\n"
    );
    s
}

/// `n` instantiations `impl Tr[A_i] for X` and `n` calls `x.go(A_i { })`:
/// review F10's shape.
fn instantiation_per_argument(n: usize) -> String {
    let mut s =
        String::from("trait Tr[A]:\n    fn go(&self, a: A) -> i32\n\nstruct X:\n    n: i32\n");
    for i in 0..n {
        let _ = write!(
            s,
            "\nstruct A{i}:\n    n: i32\n\nimpl Tr[A{i}] for X:\n    fn go(&self, a: A{i}) -> i32: a.n\n"
        );
    }
    s.push_str("\npub fn main() -> void $ Console:\n    x := X { n: 1 }\n    let t = +0\n");
    for i in 0..n {
        let _ = writeln!(s, "    t = t + x.go(A{i} {{ n: 1 }})");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

// Checker: #1892, a method choice drops the candidates whose trait
// argument has another rigid head than the argument inferred before the
// trials or the expected result, by one probe of the candidates' head
// index, which a closed receiver's instantiations keep once per run: a
// nested call under a known expected type tried every candidate per
// level (exponential), and each call tried every instantiation.
#[test]
fn method_choice_narrows_by_argument_and_result_heads() {
    on_test_stack(|| {
        let work = |source: String| {
            let (out, work) = hd_check::work::count_scan_work(|| compile(&source, &Goal::Analyze));
            assert!(out.diags.is_empty(), "{}", out.render());
            work
        };
        let (small, large) = (
            work(nested_method_choice(4)),
            work(nested_method_choice(40)),
        );
        // Per level: three trials at the top, one probe of the index below.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work at depth 4 and 40: {small} and {large}"
        );
        let (small, large) = (
            work(instantiation_per_argument(100)),
            work(instantiation_per_argument(1_000)),
        );
        // Per call: one probe and one candidate; per instantiation: its
        // index entry, once per run.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "scan work for 100 and 1,000 instantiations: {small} and {large}"
        );
    });
}

/// One type parameter bounded by `n` traits, each declaring one method,
/// and one call of each method on a value of it: the scale check's
/// `param-bounds` shape.
fn param_bounds(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = write!(s, "trait B{i}:\n    fn b{i}(&self) -> i32\n\n");
    }
    let bounds: Vec<String> = (0..n).map(|i| format!("B{i}")).collect();
    let _ = writeln!(s, "fn bounded[T < {}](x: T) -> i32:", bounds.join(" & "));
    s.push_str("    let t = +0\n");
    for i in 0..n {
        let _ = writeln!(s, "    t = t + x.b{i}()");
    }
    s.push_str("    t\n\npub fn main() -> void $ Console:\n    println(\"0\")\n");
    s
}

// Checker: #1937, a call on a bounded type finds its method's clause by
// one probe of the body's index of the type's clauses by method name:
// each call walked the clauses in order, asking each one's trait.
#[test]
fn bounded_parameter_calls_find_their_clause_by_one_probe() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) = hd_check::work::count_trait_lookup_work(|| {
                compile(&param_bounds(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        // Per bound: its method name indexed once; per call: one probe.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "trait lookups for 100 and 1,000 bounds: {small} and {large}"
        );
    });
}

/// One type parameter bounded by `n` traits, each declaring one function
/// with no receiver, and one `T::s{i}()` call of each.
fn param_assoc_calls(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let _ = write!(s, "trait S{i}:\n    fn s{i}() -> i32\n\n");
    }
    let bounds: Vec<String> = (0..n).map(|i| format!("S{i}")).collect();
    let _ = writeln!(s, "fn bounded[T < {}]() -> i32:", bounds.join(" & "));
    s.push_str("    let t = +0\n");
    for i in 0..n {
        let _ = writeln!(s, "    t = t + T::s{i}()");
    }
    s.push_str("    t\n\npub fn main() -> void $ Console:\n    println(\"0\")\n");
    s
}

// Checker: #1970, `T::f()` on a type parameter reads the traits of the
// clauses that declare `f` from the body's index of the type's clauses
// by method name: each call asked every bound's trait.
#[test]
fn parameter_associated_calls_find_their_bounds_by_one_probe() {
    on_test_stack(|| {
        let work = |n: usize| {
            let (out, work) = hd_check::work::count_trait_lookup_work(|| {
                compile(&param_assoc_calls(n), &Goal::Analyze)
            });
            assert!(out.diags.is_empty(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (work(100), work(1_000));
        // Per bound: its function name indexed once; per call: one probe
        // and one step for the one declaring trait.
        assert!(
            small > 0 && large <= 10 * small + 100,
            "trait lookups for 100 and 1,000 bounds: {small} and {large}"
        );
    });
}

/// A list type and literal nested `n` deep in one binding: the scale
/// check's `list-type-nest` shape.
fn list_type_nest(n: usize) -> String {
    let ty = format!("{}i32{}", "List[".repeat(n), "]".repeat(n));
    let value = format!("{}t{}", "[".repeat(n), "]".repeat(n));
    format!(
        "pub fn main() -> void $ Console:\n    let t = +0\n    let xs: {ty} = {value}\n    t = i32(xs.len())\n    println(\"$t\")\n"
    )
}

// Collection: #1915, a type's layout hash reads the declarations it
// reaches from one Merkle hash per distinct type, memoized for the
// build: each of a nest's n types walked the whole type below it.
#[test]
fn nested_list_types_hash_each_type_once() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let steps = |n: usize| {
            let (out, steps) =
                hd_mono::work::count_type_steps(|| compile(&list_type_nest(n), &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            steps
        };
        let (small, large) = (steps(15), steps(150));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "type steps at depth 15 and 150: {small} and {large}"
        );
    });
}

/// Inline `if` values nested `n` deep: the scale check's `if-expr-nest`.
fn if_value_nest(n: usize) -> String {
    format!(
        "pub fn main() -> void $ Console:\n    let t = +1\n    t = {}t{}\n    println(\"$t\")\n",
        "if t > 0: (".repeat(n),
        ") else: +0".repeat(n)
    )
}

/// `||` and `&&` values nested `n` deep: the scale check's `bool-nest`.
fn logic_value_nest(n: usize) -> String {
    format!(
        "pub fn main() -> void $ Console:\n    let t = +1\n    b := {}true{}\n    if b:\n        t = t + 1\n    println(\"$t\")\n",
        "(t < 1 || !(t >= 2 && ".repeat(n),
        "))".repeat(n)
    )
}

/// `n` inline `if` values one after another in one body.
fn if_value_run(n: usize) -> String {
    let mut s = String::from("pub fn main() -> void $ Console:\n    let t = +1\n");
    for _ in 0..n {
        s.push_str("    t = if t > 0: t + 1 else: +0\n");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// A generated program, its size n, and what it prints at size k.
type ValueShape = (&'static str, fn(usize) -> String, usize, fn(usize) -> usize);

/// Runs the program `out` linked and checks what it prints. A work test
/// runs only its largest size: the smaller programs repeat the same code
/// fewer times, so their output proves nothing the largest's does not,
/// and each run is a Node process.
fn runs_and_prints(name: &str, out: &Output, printed: &str) {
    let ran = crate::release_profile_run::run_wasm(name, out.wasm.as_deref().expect("links"));
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), printed, "{name}");
}

// Emitter: #1905, #1913, rule 6 of the local cleanup (a value `if` whose
// arms each set one local) rewrote one `if` per pass over the body, and
// rule 20 copied the body for each value `if` it tested, so a body's value
// `if`s, nested or one after another, cost their count times its length.
// Now one pass rewrites every rule 6 `if`, and rule 20 copies only for a
// rewrite, which counts as work. The largest program runs and prints its value.
#[test]
fn value_ifs_rewrite_in_one_pass_over_the_body() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let work = |name: &str, source: String, printed: Option<String>| {
            let (out, work) = hd_wasm::work::count_control(|| compile(&source, &goal));
            assert!(!out.diags.has_errors(), "{name}: {}", out.render());
            if let Some(printed) = printed {
                runs_and_prints(name, &out, &printed);
            }
            work
        };
        let shapes: [ValueShape; 3] = [
            // An entry nests more than two levels: 50 stays below the limit.
            ("if-value-nest", if_value_nest, 5, |_| 1),
            ("logic-value-nest", logic_value_nest, 5, |_| 2),
            ("if-value-run", if_value_run, 100, |n| n + 1),
        ];
        for (name, generate, n, printed) in shapes {
            let zero = work(name, generate(0), None);
            let small = work(name, generate(n), None) - zero;
            let large = work(
                name,
                generate(10 * n),
                Some(format!("{}\n", printed(10 * n))),
            ) - zero;
            assert!(
                small > 0 && large <= 12 * small,
                "{name}: control work over the empty body at {n} and {}: {small} and {large}",
                10 * n
            );
        }
    });
}

/// `for` (or `while`) loops nested `n` deep, each level adding one: the
/// scale check's `for-nest` and `while-nest` shapes.
fn loop_nest(n: usize, kind: &str) -> String {
    let mut s = String::from("pub fn main() -> void $ Console:\n    let t = +0\n");
    for i in 0..n {
        let indent = "    ".repeat(i + 1);
        if kind == "for" {
            let _ = writeln!(s, "{indent}for k{i} in 0..1:");
        } else {
            let _ = writeln!(s, "{indent}while t < {}:", i + 1);
        }
        let _ = writeln!(s, "{indent}    t = t + 1");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// `n` loops one after another, each over a list by its index: the
/// range-index proof lists each loop's instructions and the statements
/// before it.
fn list_loop_run(n: usize) -> String {
    let mut s =
        String::from("pub fn main() -> void $ Console:\n    xs := [1, 2, 3]\n    let t = +0\n");
    for _ in 0..n {
        s.push_str("    for i in 0..xs.len():\n        t = t + xs[i]\n");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// `n` loops one after another below one length read: each loop's
/// region of the list is the hoisting pass's, not a walk of every loop
/// before it.
fn shared_length_loop_run(n: usize) -> String {
    let mut s = String::from(
        "pub fn main() -> void $ Console:\n    xs := [1, 2, 3]\n    m := xs.len()\n    let t = +0\n",
    );
    for _ in 0..n {
        s.push_str("    for i in 0..m:\n        t = t + xs[i]\n");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// `n` loops one after another, each over its own list by its index:
/// each list's aliases are walked once, over what reaches that list
/// (#1982).
fn list_loop_run_distinct(n: usize) -> String {
    let mut s = String::from("pub fn main() -> void $ Console:\n    let t = +0\n");
    for k in 0..n {
        let _ = writeln!(s, "    xs{k} := [1, 2, 3]");
        let _ = writeln!(s, "    for i in 0..xs{k}.len():\n        t = t + xs{k}[i]");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// `n` falling-index loops one after another over one list (#1982): each
/// asks which accesses checked its entry index.
fn falling_loop_run(n: usize) -> String {
    let mut s =
        String::from("pub fn main() -> void $ Console:\n    xs := [1, 2, 3]\n    let t = +0\n");
    for k in 0..n {
        let _ = writeln!(s, "    let top{k}: usize = 2\n    t = t + xs[top{k}]");
        let _ = writeln!(s, "    let v{k}: usize = top{k}\n    while v{k} > 0:");
        let _ = writeln!(s, "        t = t + xs[v{k} - 1]\n        v{k} = v{k} - 1");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// `n` masked-index loops one after another over one list (#1982), each
/// under its own length test.
fn masked_loop_run(n: usize) -> String {
    let mut s = String::from(
        "pub fn main() -> void $ Console:\n    xs := [1, 2, 3]\n    let t = +0\n    let m: usize = 3\n",
    );
    for _ in 0..n {
        s.push_str("    if xs.len() > 0:\n        for i in 0..m:\n");
        s.push_str("            t = t + xs[i & (xs.len() - 1)]\n");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

fn build_at(source: &str, level: hd_driver::OptLevel) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = hd_driver::Packages {
        level,
        ..hd_driver::Packages::default()
    };
    hd_driver::build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

// Emitter: #1916, each loop's list proofs scanned every instruction of
// the body and built each one's block path to ask whether the loop held
// it: cubic in a nest, quadratic in a run of loops. The body's block tree
// is now numbered once in preorder, a loop's instructions are a slice of
// it, and only a proof whose cheap tests pass asks for them. #1982: the
// counter also counts the whole-body pass (the list facts, once per
// body), which each loop repeated, so a run of loops was quadratic. Each
// list's stability is now its region row of the hoisting pass (#2175).
// The proofs run in `hd_opt::prove`, which counts its scans; the emitter
// asks no loop question since #2175 step 6 deleted its guard pass.
#[test]
fn loop_proofs_list_each_loops_instructions_from_one_tree() {
    on_test_stack(|| {
        let work = |name: &str, source: String, level, printed: Option<String>| {
            let (out, work) = hd_opt::prove::count_scans(|| build_at(&source, level));
            assert!(!out.diags.has_errors(), "{name}: {}", out.render());
            if let Some(printed) = printed {
                runs_and_prints(name, &out, &printed);
            }
            work
        };
        let shapes: [ValueShape; 8] = [
            // A level nests two deep: 70 levels stay below the nesting limit.
            ("for-nest", |n| loop_nest(n, "for"), 7, |n| n),
            ("while-nest", |n| loop_nest(n, "while"), 7, |n| n),
            ("list-loop-run", list_loop_run, 100, |n| 6 * n),
            ("distinct-list-loop-run", list_loop_run_distinct, 100, |n| {
                6 * n
            }),
            ("shared-length-loop-run", shared_length_loop_run, 100, |n| {
                6 * n
            }),
            ("param-length-loop-run", param_length_loop_run, 100, |n| {
                3 * n
            }),
            ("falling-loop-run", falling_loop_run, 100, |n| 6 * n),
            ("masked-loop-run", masked_loop_run, 100, |n| 5 * n),
        ];
        for level in [hd_driver::OptLevel::Dev, hd_driver::OptLevel::Optimized] {
            for (name, generate, n, printed) in shapes {
                let run = |k, printed| work(name, generate(k), level, printed);
                // The first build proves the std bodies the run then reuses.
                run(0, None);
                let zero = run(0, None);
                let small = run(n, None) - zero;
                let large = run(10 * n, Some(format!("{}\n", printed(10 * n)))) - zero;
                assert!(
                    small > 0 && large <= 12 * small,
                    "{name} {level:?}: loop scans over the empty body at {n} and {}: {small} and {large}",
                    10 * n
                );
            }
        }
    });
}

/// `n` loops below one length read of a by-value list parameter, each
/// pushing onto another list: the parameter's list keeps its length across
/// the push, a call that may write, so its region is the hoisting pass's,
/// not a walk of every loop before it.
fn param_length_loop_run(n: usize) -> String {
    let mut s = String::from(
        "fn total(xs: List[i64]) -> i64:\n    m := xs.len()\n    let mut out: List[i64] = []\n",
    );
    for _ in 0..n {
        s.push_str("    for i in 0..m:\n        out.push(xs[i])\n");
    }
    s.push_str("    i64(out.len())\n\npub fn main() -> void $ Console:\n    println(total([1i64, 2, 3]))\n");
    s
}

/// A program generator by size.
type Generate = fn(usize) -> String;

/// A tuple type, value and pattern nested `n` deep: the scale check's
/// `tuple-nest`.
fn tuple_nest(n: usize) -> String {
    let ty = format!("{}i32{}", "(i32, ".repeat(n), ")".repeat(n));
    let value = format!("{}t{}", "(t, ".repeat(n), ")".repeat(n));
    let mut pattern = String::from("z");
    for i in (0..n).rev() {
        pattern = format!("(a{i}, {pattern})");
    }
    format!(
        "pub fn main() -> void $ Console:\n    let t = +1\n    let x: {ty} = {value}\n    let {pattern} = x\n    t = z\n    println(\"$t\")\n"
    )
}

/// A tuple type, value and pattern nested `n` deep with two values a
/// level, `(i32, i32, (i32, i32, ...))`: over 16 values from 8 levels on.
fn tuple_pair_nest(n: usize) -> String {
    let ty = format!("{}i32{}", "(i32, i32, ".repeat(n), ")".repeat(n));
    let value = format!("{}t{}", "(t, t, ".repeat(n), ")".repeat(n));
    let mut pattern = String::from("z");
    for i in (0..n).rev() {
        pattern = format!("(a{i}, b{i}, {pattern})");
    }
    format!(
        "pub fn main() -> void $ Console:\n    let t = +1\n    let x: {ty} = {value}\n    let {pattern} = x\n    t = z\n    println(\"$t\")\n"
    )
}

/// An option type, value and pattern nested `n` deep: the scale check's
/// `option-nest`.
fn option_nest(n: usize) -> String {
    let ty = format!("i32{}", "?".repeat(n));
    let value = format!("{}t{}", ".Some(".repeat(n), ")".repeat(n));
    let pattern = format!("{}v{}", ".Some(".repeat(n), ")".repeat(n));
    // At depth 0 the binding pattern matches every value.
    let rest = if n == 0 { "" } else { "        _ => +0\n" };
    format!(
        "pub fn main() -> void $ Console:\n    let t = +0\n    let o: {ty} = {value}\n    t = match o:\n        {pattern} => v\n{rest}    println(\"$t\")\n"
    )
}

// Checker: #1918, #1924, each level of a nested pattern asked whether its
// part's type is copied, and the walk re-walked every member type below
// it, since only the type asked entered the memo: O(depth^2) per body.
// Now a walk that ends clear memoizes every type it visited, and a walk
// stops at a member type answered before.
#[test]
fn nested_pattern_copy_questions_walk_each_type_once() {
    on_test_stack(|| {
        let shapes: [(&str, Generate); 2] =
            [("tuple-nest", tuple_nest), ("option-nest", option_nest)];
        // A level nests one bracket deep: 150 stays below the limit.
        let n = 15;
        for (name, generate) in shapes {
            let work = |k: usize| {
                let (out, work) =
                    hd_check::work::count_scan_work(|| compile(&generate(k), &Goal::Analyze));
                assert!(!out.diags.has_errors(), "{name} {k}: {}", out.render());
                work
            };
            // The first build also checks what the run's memo then keeps.
            work(0);
            let zero = work(0);
            let small = work(n) - zero;
            let large = work(10 * n) - zero;
            assert!(
                small > 0 && large <= 12 * small,
                "{name}: scan work over the empty body at {n} and {}: {small} and {large}",
                10 * n
            );
        }
    });
}

/// A `!Copy` struct held `n` tuples deep through named aliases
/// (`type D{i} = (D{i-1}, i32)`), and a function reading a value of each
/// depth, deepest first: each copy question's answer is a blocker below
/// every level asked before it.
fn blocked_tuple_aliases(n: usize) -> String {
    let mut s = String::from("struct T:\n    n: i32\n\nimpl !Copy for T\n\ntype D0 = (T, i32)\n");
    for i in 1..=n {
        let _ = writeln!(s, "type D{i} = (D{}, i32)", i - 1);
    }
    for i in (0..=n).rev() {
        let _ = write!(s, "\nfn pass{i}(x: D{i}) -> D{i}: x\n");
    }
    s.push_str("\npub fn main() -> void $ Console:\n    println(\"0\")\n");
    s
}

// Checker: #1938, a copy question whose walk found a blocker memoized
// only the type asked, so the levels of a nest holding a `!Copy` type,
// asked from the deepest, each walked down to it again: O(depth^2). Now
// the walk is a post-order over the member types that memoizes every
// visited type's own answer, clear or blocked.
#[test]
fn blocked_copy_questions_walk_each_type_once() {
    on_test_stack(|| {
        let work = |k: usize| {
            let (out, work) = hd_check::work::count_scan_work(|| {
                compile(&blocked_tuple_aliases(k), &Goal::Analyze)
            });
            assert!(!out.diags.has_errors(), "{k}: {}", out.render());
            work
        };
        // The first build also fills what the run's memo then keeps.
        work(0);
        let zero = work(0);
        let small = work(20) - zero;
        let large = work(200) - zero;
        assert!(
            small > 0 && large <= 12 * small,
            "scan work over depth 0 at depth 20 and 200: {small} and {large}"
        );
    });
}

/// A tuple pattern nested `n` deep binding `a{i}` at depth `i`, and a
/// reference to each binding: each reference asks its binding's scope.
fn referenced_pattern_nest(n: usize) -> String {
    let ty = format!("{}i32{}", "(i32, ".repeat(n), ")".repeat(n));
    let value = format!("{}t{}", "(t, ".repeat(n), ")".repeat(n));
    let mut pattern = String::from("z");
    for i in (0..n).rev() {
        pattern = format!("(a{i}, {pattern})");
    }
    let mut s = format!(
        "pub fn main() -> void $ Console:\n    let t = +1\n    let x: {ty} = {value}\n    let {pattern} = x\n"
    );
    for i in 0..n {
        let _ = writeln!(s, "    let r{i} = &a{i}\n    t = t + *r{i}");
    }
    s.push_str("    println(\"$t $z\")\n");
    s
}

// Checker: #1938, each scope question walked from its binding's node up
// to the root, so `n` bindings at depths 1..n of one pattern cost
// O(n^2). Now each node's nearest scope node and loop are memoized for
// the body, and a question climbs only nodes no earlier question did.
#[test]
fn scope_questions_climb_each_node_once() {
    on_test_stack(|| {
        let work = |k: usize| {
            let (out, work) = hd_check::work::count_scan_work(|| {
                compile(&referenced_pattern_nest(k), &Goal::Analyze)
            });
            assert!(!out.diags.has_errors(), "{k}: {}", out.render());
            work
        };
        work(0);
        let zero = work(0);
        let small = work(14) - zero;
        let large = work(140) - zero;
        assert!(
            small > 0 && large <= 12 * small,
            "scan work over depth 0 at depth 14 and 140: {small} and {large}"
        );
    });
}

/// `n` functions, each declaring a local trait `T{i}` with a method
/// `get{i}`, and `name` too when `shared`, and failing one call
/// `q.get()` on an `i32`: #1933's shape. Every other function's trait
/// declares `get` but is not available, and every function adds a
/// method name to the program.
fn failing_local_trait_calls(n: usize, shared: bool) -> String {
    let mut s = String::new();
    for i in 0..n {
        let get = if shared {
            "        fn get(self) -> i32\n"
        } else {
            ""
        };
        let _ = write!(
            s,
            "fn call{i}(q: i32) -> i32:\n    trait T{i}:\n{get}        fn get{i}(self) -> i32\n    q.get()\n\n"
        );
    }
    s
}

// Checker: #1933, each `unknown-method` error asked every trait of the
// program declaring the name whether the receiver implements it, for the
// "add `use`" hint, and ranked every method name of the program for the
// "did you mean" hint: n failing calls cost n^2. Now the hint asks only
// the traits with a row for the receiver's head and ranks only the
// receiver's methods.
#[test]
fn unknown_method_hints_ask_only_the_receivers_methods() {
    on_small_stack(|| {
        for shared in [true, false] {
            let work = |n: usize| {
                let (out, work) = hd_check::work::count_scan_work(|| {
                    compile(&failing_local_trait_calls(n, shared), &Goal::Analyze)
                });
                assert_eq!(out.diags.len(), n, "{n}: {}", out.render());
                work
            };
            work(0);
            let zero = work(0);
            let small = work(100) - zero;
            let large = work(1_000) - zero;
            assert!(
                small > 0 && large <= 12 * small,
                "shared {shared}: scan work over 0 calls at 100 and 1,000: {small} and {large}"
            );
        }
    });
}

/// `k` units of one module each, every module calling `get` on the
/// struct `S` of the root unit's `shared` module: lane 126's measure
/// of the unit slots a module's view scans per receiver head.
fn modules_in_many_units_call_one_type(k: usize) -> MemorySources {
    let mut src = MemorySources::default();
    src.insert(
        "shared.hd",
        "pub trait Get:\n    fn get(&self) -> i32\n\npub struct S:\n    pub v: i32\n\nimpl Get for S:\n    fn get(&self) -> i32: self.v\n",
    );
    for i in 0..k {
        src.insert(
            &format!("d{i}/m.hd"),
            &format!("use pkg.shared.{{S, Get}}\n\npub fn call{i}(s: S) -> i32: s.get()\n"),
        );
    }
    src.insert(
        "main.hd",
        "pub fn main() -> void $ Console:\n    println(\"0\")\n",
    );
    src
}

// Driver and solver: each module's impl view held a slot per unit of
// the program, made per module (`closure_impls`) and scanned once per
// receiver head (`Impls::tables_at`), so k modules in k units cost
// k^2. The view now holds its closure's units alone (`UnitSlots`).
#[test]
fn modules_in_many_units_scan_their_view_once_per_head() {
    on_test_stack(|| {
        let work = |k: usize| {
            let src = modules_in_many_units_call_one_type(k);
            let store = crate::warm_store();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let (out, slots) =
                hd_types::work::count_view_slots(|| build(&host, "app", &Goal::Analyze));
            assert!(out.diags.is_empty(), "{k}: {}", out.render());
            slots
        };
        let (small, large) = (work(100), work(1_000));
        assert!(
            small > 0 && large <= 12 * small,
            "view slots at 100 and 1,000 units: {small} and {large}"
        );
    });
}

// Emitter: #1918, the walk for a release's `Drop` calls entered every
// inline part of each value it released, so each level of an option or
// tuple nest walked the `i` levels below it: O(depth^2). A part that
// holds no `Drop` value is now skipped, by the link's memo of
// `holds_drop`.
//
// The tuple nest holds two values a level, so at both sizes its tuple is
// over the 16 values a local keeps in memory (#2031): one-value levels
// put 13 values at 12 and 121 at 120, comparing a local of leaves with
// one written in place, two code paths and not one at two sizes.
#[test]
fn drop_call_walks_skip_parts_without_a_drop_value() {
    on_test_stack(|| {
        let shapes: [(&str, Generate); 2] = [
            ("tuple-pair-nest", tuple_pair_nest),
            ("option-nest", option_nest),
        ];
        let goal = Goal::Program {
            entry: "main".into(),
        };
        // A level nests one bracket deep: 120 stays below the limit. The
        // sizes differ from the copy test's, whose checker memo a build
        // here would fill.
        let n = 12;
        for (name, generate) in shapes {
            let work = |k: usize| {
                let (out, work) =
                    hd_layout::work::count_member_layouts(|| compile(&generate(k), &goal));
                assert!(!out.diags.has_errors(), "{name} {k}: {}", out.render());
                work
            };
            let zero = work(0);
            let small = work(n) - zero;
            let large = work(10 * n) - zero;
            assert!(
                small > 0 && large <= 12 * small,
                "{name}: drop walk and member layouts over the empty body at {n} and {}: {small} and {large}",
                10 * n
            );
        }
    });
}

/// `n` statements each testing a `&&` value: rule 20's `if` with an
/// empty other arm.
fn and_if_run(n: usize) -> String {
    let mut s = String::from("pub fn main() -> void $ Console:\n    let t = +1\n");
    for _ in 0..n {
        s.push_str("    if t > 0 && t < 5:\n        t = t + 1\n");
    }
    s.push_str("    println(\"$t\")\n");
    s
}

/// `n` statements each testing a `||` value: rule 20's block form.
fn or_if_run(n: usize) -> String {
    and_if_run(n).replace("t > 0 && t < 5", "t > 3 || t > 0")
}

/// `n` statements each testing a negated `||` value: the `if` form with
/// an empty other arm and the value flipped.
fn not_or_if_run(n: usize) -> String {
    and_if_run(n).replace("t > 0 && t < 5", "!(t > 3 || t < 0)")
}

/// `n` statements each testing a `||` value with both arms written.
fn or_else_run(n: usize) -> String {
    and_if_run(n).replace(
        "    if t > 0 && t < 5:\n        t = t + 1\n",
        "    if t > 3 || t > 0:\n        t = t + 1\n    else:\n        t = t - 1\n",
    )
}

/// `n` loops each going on early on a `&&` value: a `br_if` to a loop.
fn and_continue_run(n: usize) -> String {
    and_if_run(n).replace(
        "    if t > 0 && t < 5:\n        t = t + 1\n",
        "    while t < 3:\n        t = t + 1\n        if t > 0 && t < 2:\n            continue\n",
    )
}

/// `n` loops each testing a `&&` value: rule 20's `br_if` forms.
fn and_while_run(n: usize) -> String {
    and_if_run(n).replace("    if t > 0 && t < 5:", "    while t > 0 && t < 3:")
}

// Emitter: #1978, rule 20 (a `&&` or `||` value feeding a branch) rewrote
// one site per pass over the body, so a body of n such conditions cost n
// passes. Now one pass names each branch's target by its label, rewrites
// every site that overlaps no other, and names the depths again.
#[test]
fn branch_feeding_logic_values_rewrite_in_one_pass_over_the_body() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let work = |name: &str, source: String, printed: Option<String>| {
            let (out, work) = hd_wasm::work::count_control(|| compile(&source, &goal));
            assert!(!out.diags.has_errors(), "{name}: {}", out.render());
            if let Some(printed) = printed {
                runs_and_prints(name, &out, &printed);
            }
            work
        };
        let shapes: [ValueShape; 6] = [
            ("and-if-run", and_if_run, 100, |n| (n + 1).min(5)),
            ("not-or-if-run", not_or_if_run, 100, |n| (n + 1).min(4)),
            ("or-else-run", or_else_run, 100, |n| n + 1),
            ("and-continue-run", and_continue_run, 100, |n| {
                if n == 0 { 1 } else { 3 }
            }),
            ("or-if-run", or_if_run, 100, |n| n + 1),
            ("and-while-run", and_while_run, 100, |n| {
                if n == 0 { 1 } else { 3 }
            }),
        ];
        for (name, generate, n, printed) in shapes {
            let zero = work(name, generate(0), None);
            let small = work(name, generate(n), None) - zero;
            let large = work(
                name,
                generate(10 * n),
                Some(format!("{}\n", printed(10 * n))),
            ) - zero;
            assert!(
                small > 0 && large <= 12 * small,
                "{name}: control work over the empty body at {n} and {}: {small} and {large}",
                10 * n
            );
        }
    });
}

/// A generic struct instantiated `n` deep through a generic function, on
/// an integer literal: the scale check's `generic-nest`.
fn generic_nest(n: usize) -> String {
    format!(
        "struct W[T]:\n    v: T\n\nfn wrap[T](x: T) -> W[T]: W {{ v: x }}\n\npub fn main() -> void $ Console:\n    let t = +0\n    w := {}t{}\n    t = w{}\n    println(\"$t\")\n",
        "wrap(".repeat(n),
        ")".repeat(n),
        ".v".repeat(n)
    )
}

/// A generic newtype nested `n` deep by a call per level: each level's
/// value `N^i[i32]` is a newtype over the level below.
fn newtype_nest(n: usize) -> String {
    format!(
        "type N[T](T)\n\nfn wrap[T](x: T) -> N[T]: N(x)\n\npub fn main() -> void $ Console:\n    let t = +0\n    w := {}t{}\n    println(\"${{t}}\")\n    _ := w\n",
        "wrap(".repeat(n),
        ")".repeat(n),
    )
}

/// [`generic_nest`] with nine parameters: each level's type
/// `W^i[?t, ?a, ..., ?h]` holds the level below and the same eight
/// literal variables, nine variables in all, which stay unbound to the
/// body's end.
fn wide_generic_nest(n: usize) -> String {
    let params = "T, A, B, C, D, E, F, G, H";
    let fields = "v: T, a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H";
    let mut s = format!(
        "struct W[{params}]:\n{}\n\nfn wrap[{params}](v: T, a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H) -> W[{params}]:\n    W {{ v: v, a: a, b: b, c: c, d: d, e: e, f: f, g: g, h: h }}\n\npub fn main() -> void $ Console:\n    let t = +0\n",
        fields
            .split(", ")
            .map(|f| format!("    {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    for x in ["a", "b", "c", "d", "e", "f", "g", "h"] {
        let _ = writeln!(s, "    let {x} = +0");
    }
    let _ = writeln!(
        s,
        "    w := {}t{}\n    t = w{}\n    println(\"$t $a $b $c $d $e $f $g $h\")",
        "wrap(".repeat(n),
        ", a, b, c, d, e, f, g, h)".repeat(n),
        ".v".repeat(n)
    );
    s
}

/// [`generic_nest`] inside a generic function, on its parameter: each
/// level's type `W^i[T]` holds the parameter, not a variable.
fn generic_nest_in_generic(n: usize) -> String {
    format!(
        "struct W[T]:\n    v: T\n\nfn wrap[T](x: T) -> W[T]: W {{ v: x }}\n\nfn nest[T](t: T) -> T:\n    w := {}t{}\n    w{}\n\npub fn main() -> void $ Console:\n    t := nest(+0)\n    println(\"$t\")\n",
        "wrap(".repeat(n),
        ")".repeat(n),
        ".v".repeat(n)
    )
}

// Checker: #1991, the canonical-fold memo kept only types with at most
// eight inference variables, so each level of a nest whose types carry
// nine re-folded the whole type below it: O(depth^2) type nodes. #2005,
// `InferTable::resolve`'s open-resolution memo likewise kept only
// results with at most four. Every fold and every resolution is kept now.
#[test]
fn wide_generic_nest_folds_each_level_once() {
    on_test_stack(|| {
        let nodes = |n: usize| {
            let (out, nodes) =
                hd_types::work::count_type_nodes(|| compile(&wide_generic_nest(n), &Goal::Analyze));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            nodes
        };
        // The first build also checks what the run's memo then keeps.
        nodes(0);
        let zero = nodes(0);
        let (small, large) = (nodes(3) - zero, nodes(30) - zero);
        assert!(
            small > 0 && large <= 11 * small,
            "type nodes at depth 3 and 30: {small} and {large}"
        );
    });
}

// Checker: #1912, each level of a generic nest asks `Copy` of its
// argument's type `W^i[X]` (the default bound), and the `Copy` row walked
// the whole type, the goal's canonical fold folded it, the instantiation
// cycle check walked it for parameters, and the call's search for
// variables behind a reference walked it: O(depth^2) type nodes. Now
// each walk stops at a part an earlier goal or call answered.
#[test]
fn generic_nest_checks_walk_each_level_once() {
    on_test_stack(|| {
        let shapes: [(&str, Generate); 2] = [
            ("generic-nest", generic_nest),
            ("generic-nest-in-generic", generic_nest_in_generic),
        ];
        for (name, generate) in shapes {
            let nodes = |n: usize| {
                let (out, nodes) =
                    hd_types::work::count_type_nodes(|| compile(&generate(n), &Goal::Analyze));
                assert!(!out.diags.has_errors(), "{name} {n}: {}", out.render());
                nodes
            };
            // The first build also checks what the run's memo then keeps.
            nodes(0);
            let zero = nodes(0);
            // The sizes differ from the other generic nest tests', whose
            // builds the shared store keeps.
            let (small, large) = (nodes(2) - zero, nodes(20) - zero);
            assert!(
                small > 0 && large <= 12 * small,
                "{name}: type nodes at depth 2 and 20: {small} and {large}"
            );
        }
    });
}

// Ownership and emitter: #1912, each instance of a generic nest's `wrap`
// asked whether its types hold a `Drop` value with a memo of the body's
// own, walking `W^i` to its depth in the instance for level `i`; and the
// static-value proof went in rounds over every function, one level of
// the call chain per round. Both cost O(depth^2). Now the run keeps one
// memo of `Drop` answers, and the proof looks at a function again only
// when a fact it reads was proved.
#[test]
fn generic_nest_builds_walk_each_level_once() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let shapes: [(&str, Generate); 2] = [
            ("generic-nest", generic_nest),
            ("generic-nest-in-generic", generic_nest_in_generic),
        ];
        for (name, generate) in shapes {
            let work = |n: usize| {
                let ((out, looks), nodes) = hd_layout::work::count_member_layouts(|| {
                    hd_wasm::work::count_static_looks(|| compile(&generate(n), &goal))
                });
                assert!(!out.diags.has_errors(), "{name} {n}: {}", out.render());
                (nodes, looks)
            };
            // Depth 4 is the base: a program with no `wrap` call reaches
            // fewer std bodies. Depth 1 warms what the run's caches keep,
            // so each measured program is built for the first time, at
            // sizes no other test builds. 31 stays below the instantiation
            // depth limit of 32, so the levels over the base are 3 and 27.
            work(1);
            let base = work(4);
            let (small, large) = (work(7), work(31));
            let (nodes, looks) = (
                (small.0 - base.0, large.0 - base.0),
                (small.1 - base.1, large.1 - base.1),
            );
            assert!(
                nodes.0 > 0 && nodes.1 <= 11 * nodes.0,
                "{name}: member layouts and Drop answers at depth 7 and 31 over 4: {nodes:?}"
            );
            assert!(
                looks.0 > 0 && looks.1 <= 11 * looks.0,
                "{name}: static-value instructions read at depth 7 and 31 over 4: {looks:?}"
            );
        }
    });
}

// Emitter: #1989, a release of a newtype runs each layer's `Drop`, so the
// drop walk and `holds_drop` step through a value's newtype layers. Each
// level of a newtype nest is a newtype over the level below.
#[test]
fn newtype_nest_drop_walks_are_linear() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let work = |n: usize| {
            let (out, nodes) =
                hd_layout::work::count_member_layouts(|| compile(&newtype_nest(n), &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            nodes
        };
        work(1);
        let base = work(2);
        let (small, large) = (work(5) - base, work(29) - base);
        assert!(
            small > 0 && large <= 11 * small,
            "member layouts and newtype layers at depth 5 and 29 over 2: {small} and {large}"
        );
    });
}

/// `n` struct declarations, each holding the next by value, over a
/// `!Copy` struct, with a function per level that moves a value of that
/// level's type: the ownership passes ask `Copy` of each level. With
/// `newtype`, each level is a newtype over the next instead.
fn declaration_chain(n: usize, newtype: bool) -> String {
    let mut s = String::from(
        "use std.meta.inline\n\nstruct Bottom:\n    id: i32\n\nimpl !Copy for Bottom\n\n",
    );
    for i in 0..n {
        let (next, inner) = if i + 1 < n {
            (format!("S{}", i + 1), format!("make{}()", i + 1))
        } else {
            ("Bottom".to_owned(), "Bottom { id: 1 }".to_owned())
        };
        let (decl, value) = if newtype {
            (format!("type S{i}({next})"), format!("S{i}({inner})"))
        } else {
            (
                format!("struct S{i}:\n    inner: {next}"),
                format!("S{i} {{ inner: {inner} }}"),
            )
        };
        let _ = write!(
            s,
            "{decl}\n\n@inline(.Never)\nfn keep{i}(x: S{i}) -> S{i}:\n    y := x\n    y\n\nfn make{i}() -> S{i}:\n    keep{i}({value})\n\n"
        );
    }
    s.push_str("pub fn main() -> void:\n    _ := make0()\n");
    s
}

// Ownership: #1990, the passes' `Copy` walk kept only the asked type's
// answer and stopped at 32 open types, answering "copies" past it. Each
// level of a declaration chain re-walked the chain below it up to the
// cap. Now one walk memoizes every type it enters for the run.
//
// #2002: a newtype holds its base (`types.newtype.holds`), so the same
// walk steps through each newtype layer to the `!Copy` base, once.
#[test]
fn ownership_copy_walk_enters_each_type_once() {
    for newtype in [false, true] {
        on_test_stack(move || {
            let goal = Goal::Program {
                entry: "main".into(),
            };
            let nodes = |n: usize| {
                let (out, nodes) = hd_types::work::count_type_nodes(|| {
                    compile(&declaration_chain(n, newtype), &goal)
                });
                assert!(!out.diags.has_errors(), "{n}: {}", out.render());
                nodes
            };
            nodes(1);
            let base = nodes(2);
            let (small, large) = (nodes(40) - base, nodes(400) - base);
            assert!(
                small > 0 && large <= 11 * small,
                "newtype {newtype}: type nodes at chain length 40 and 400 over 2: {small} and {large}"
            );
        });
    }
}

// Types: #1912, each level of a generic nest resolved its type
// `W^i[?int]` again, since only closed resolutions were kept across calls
// and the literal's variable stays unbound to the body's end: O(depth^2)
// nodes. Now an open resolution is kept with its few unbound variables,
// valid while they stay unbound roots.
#[test]
fn open_resolutions_walk_each_node_once_while_their_variables_stay_unbound() {
    on_test_stack(|| {
        let nodes = |n: usize| {
            let (out, nodes) =
                hd_types::work::count_resolve_nodes(|| compile(&generic_nest(n), &Goal::Analyze));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            nodes
        };
        // The first build also checks what the run's memo then keeps.
        nodes(0);
        let zero = nodes(0);
        // 30 stays below the instantiation depth limit of 32.
        let (small, large) = (nodes(3) - zero, nodes(30) - zero);
        // About five nodes a level, less a few the zero program also
        // walks: 10x the depth stays under 13x; the old walk took 47x.
        assert!(
            small > 0 && large <= 13 * small,
            "resolved nodes at depth 3 and 30: {small} and {large}"
        );
    });
}

/// #2062's shape: `n` modules in a chain, each with a test module, and
/// a top-level binding in the first, so every test group runs an init.
fn test_module_chain(n: usize) -> MemorySources {
    let mut src = MemorySources::default();
    src.insert("src/m0.hd", "base := +1\n\npub fn step0() -> i32: base\n");
    for i in 1..n {
        let p = i - 1;
        src.insert(
            &format!("src/m{i}.hd"),
            &format!("use pkg.m{p}.step{p}\n\npub fn step{i}() -> i32: step{p}() + 1\n"),
        );
    }
    for i in 0..n {
        src.insert(
            &format!("src/m{i}_test.hd"),
            &format!(
                "use pkg.m{i}.step{i}\nuse std.testing.assert_equal\n\nit(\"chain {i}\"):\n    assert_equal(step{i}(), {}, reason=\"counts up\")\n",
                i + 1
            ),
        );
    }
    src
}

#[test]
fn suspending_test_entry_roots_use_linear_membership_work() {
    on_test_stack(|| {
        let mut costs = Vec::new();
        for width in [100, 1_000] {
            let mut source = String::new();
            for i in 0..width {
                writeln!(source, "it(\"case {i}\"):\n    pass\n").unwrap();
            }
            let mut sources = MemorySources::default();
            sources.insert("main_test.hd", &source);
            let store = crate::warm_store();
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let (out, work) = hd_wasm::work::count_entry_roots(|| {
                build(
                    &host,
                    "app",
                    &Goal::Tests {
                        module: None,
                        filter: None,
                    },
                )
            });
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert_eq!(out.tests.len(), width);
            wasmparser::Validator::new()
                .validate_all(out.wasm.as_deref().unwrap())
                .unwrap();
            eprintln!("{width} suspending cases: {work} entry-root work");
            assert!(work >= width as u64 * 2, "index and queries are counted");
            costs.push(work);
        }
        assert!(costs[1] <= costs[0] * 10);
    });
}

// Wasm: #2062, the constants pass indexes the init keys the entry
// helpers name once and probes the set once per function: it scanned
// every key for each function.
#[test]
fn the_constants_pass_probes_init_keys_once_per_function() {
    on_test_stack(|| {
        let goal = Goal::Tests {
            module: None,
            filter: None,
        };
        let run = |n: usize| {
            let src = test_module_chain(n);
            // Its own store: both counted tests build test_module_chain at 20 and
            // 200, and a shared store's cache hit skips the work they count (main
            // red at 0f73c1cbe, run 43863426728: 0 and 0).
            let store = hd_cache::MemoryStore::default();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let (out, work) = hd_wasm::work::count_init_probes(|| build(&host, "app", &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            assert_eq!(out.tests.len(), n, "{n}: one case per test module");
            work
        };
        let (small, large) = (run(20), run(200));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "init keys read and probed for 20 and 200 test modules: {small} and {large}"
        );
    });
}

// Driver: #2064, the collect step found each extra root by a scan of the
// roots so far, and each init's instance by a scan of every init, so k
// test modules cost k^2 there. Roots and inits are now indexed once and
// each lookup is one probe.
#[test]
fn collect_roots_look_up_each_root_once() {
    on_test_stack(|| {
        let goal = Goal::Tests {
            module: None,
            filter: None,
        };
        let run = |n: usize| {
            let src = test_module_chain(n);
            // Its own store: both counted tests build test_module_chain at 20 and
            // 200, and a shared store's cache hit skips the work they count (main
            // red at 0f73c1cbe, run 43863426728: 0 and 0).
            let store = hd_cache::MemoryStore::default();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let (out, work) = hd_driver::work::count_root_lookups(|| build(&host, "app", &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            assert_eq!(out.tests.len(), n, "{n}: one case per test module");
            work
        };
        let (small, large) = (run(20), run(200));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "collect root lookups for 20 and 200 test modules: {small} and {large}"
        );
    });
}

// Driver and Wasm: #2063, each root module rebuilt the whole use graph's
// edges and components and ordered them again, and the test program
// listed every init each test module reaches, so k test modules cost k
// times the program there and k^2/2 init calls. The global init order is
// now built once per collect, and the program names each init once, in a
// unit graph `hd.test.init` walks.
#[test]
fn the_init_order_is_built_once_per_collect() {
    on_test_stack(|| {
        let goal = Goal::Tests {
            module: None,
            filter: None,
        };
        let run = |n: usize| {
            let src = init_module_chain(n);
            let store = crate::warm_store();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let (out, work) =
                hd_driver::work::count_init_order_steps(|| build(&host, "app", &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            assert_eq!(out.tests.len(), n, "{n}: one case per test module");
            (work, out.wasm.map_or(0, |w| w.len()))
        };
        let ((small, small_size), (large, large_size)) = (run(20), run(200));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "init order steps for 20 and 200 test modules: {small} and {large}"
        );
        assert!(
            small_size > 0 && large_size <= 10 * small_size,
            "test program bytes for 20 and 200 test modules: {small_size} and {large_size}"
        );
    });
}

// Wasm: #2063, `hd.test.init` walks the unit graph with a stack in
// static memory and sorts with a heapsort, so a chain of 2,000 modules
// initializes in one call that nests none.
#[test]
fn a_deep_chain_of_modules_initializes_in_one_flat_call() {
    on_test_stack(|| {
        let n = 2_000;
        let mut main = format!(
            "use pkg.m{last}.step{last}\nuse std.testing.assert_equal\n\ntests:\n    it(\"deep\"):\n        assert_equal(step{last}(), +{n})\n",
            last = n - 1
        );
        main.push('\n');
        let mut p = hd_testkit::stages::Program::new(&main).expect("a program");
        p.files.push((
            "m0.hd".to_owned(),
            "base0 := +1\n\npub fn step0() -> i32: base0\n".to_owned(),
        ));
        for i in 1..n {
            let q = i - 1;
            p.files.push((
                format!("m{i}.hd"),
                format!("use pkg.m{q}.step{q}\n\nbase{i} := step{q}()\n\npub fn step{i}() -> i32: base{i} + 1\n"),
            ));
        }
        let out = hd_testkit::stages::tests(
            &p,
            hd_testkit::stages::Build::parse("tests").expect("a build"),
        );
        assert!(
            out.contains("deep: {\"status\":0,\"trapped\":false"),
            "{}",
            out.lines().rev().take(20).collect::<Vec<_>>().join("\n")
        );
    });
}

/// #2063's chain: `n` modules, each using the one before it and each with
/// a top-level binding, and a test module for each.
fn init_module_chain(n: usize) -> MemorySources {
    let mut src = MemorySources::default();
    src.insert("src/m0.hd", "base0 := +1\n\npub fn step0() -> i32: base0\n");
    for i in 1..n {
        let p = i - 1;
        src.insert(
            &format!("src/m{i}.hd"),
            &format!(
                "use pkg.m{p}.step{p}\n\nbase{i} := step{p}()\n\npub fn step{i}() -> i32: base{i} + 1\n"
            ),
        );
    }
    for i in 0..n {
        src.insert(
            &format!("src/m{i}_test.hd"),
            &format!(
                "use pkg.m{i}.step{i}\nuse std.testing.assert_equal\n\nit(\"chain {i}\"):\n    assert_equal(step{i}(), {}, reason=\"counts up\")\n",
                i + 1
            ),
        );
    }
    src
}

// Driver: a joined REPL build dealt its init order to its inputs by a
// pass over the whole order per input, so n inputs cost n^2 there. One
// pass now deals each init to its module's input.
#[test]
fn a_joined_repl_build_deals_its_inits_in_one_pass() {
    on_test_stack(|| {
        let run = |n: u32| {
            let mut src = MemorySources::named("repl-joined");
            for i in 1..=n {
                src.insert(&hd_project::repl_file(i), &format!("v{i} := +{i}\n"));
            }
            let session = hd_driver::Session::linked();
            let store = crate::warm_store();
            let host = Host {
                render_tir: &[],
                sources: &src,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let packages = hd_driver::Packages {
                session: Some(&session),
                inputs_from: Some(1),
                ..hd_driver::Packages::default()
            };
            let goal = Goal::ReplInput { input: n };
            let (out, work) = hd_driver::work::count_init_order_steps(|| {
                hd_driver::build_packages(&host, "app", &packages, &goal)
            });
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            work
        };
        let (small, large) = (run(20), run(200));
        assert!(
            small > 0 && large <= 10 * small + 100,
            "init order steps for 20 and 200 joined inputs: {small} and {large}"
        );
    });
}

// Checker: #2097, each global collection mutation must probe the existing
// name index instead of scanning every top-level binding.
#[test]
fn global_mutability_questions_use_the_name_index() {
    on_test_stack(|| {
        let work = |n: usize| {
            let mut source = String::new();
            for i in 0..n {
                let _ = writeln!(source, "let mut xs{i}: List[i32] = []");
            }
            source.push_str("\nfn push_all() -> void:\n");
            for i in 0..n {
                let _ = writeln!(source, "    xs{i}.push(+1)");
            }
            source.push_str("    ()\n");
            let (out, work) =
                hd_check::work::count_global_mutability_work(|| compile(&source, &Goal::Analyze));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            work
        };
        let small = work(1_000);
        let large = work(10_000);
        eprintln!("global mutability probes: {small} / {large}");
        assert_eq!(small, 1_000);
        assert_eq!(large, 10 * small, "{small} / {large}");
    });
}

// #2112: every local reference used to start another whole-body solve
// after each 64 owners. Match the scale metric's trait-items shape.
fn trait_item_cells(n: usize) -> String {
    let mut source = String::new();
    for i in 0..n {
        let _ = write!(
            source,
            "trait T{i}:\n    fn m{i}(&self) -> i32\n\nstruct S{i}:\n    a: i32\n\nimpl T{i} for S{i}:\n    fn m{i}(&self) -> i32: self.a\n\n"
        );
    }
    source.push_str("pub fn main() -> void $ Console:\n    let t = +0\n");
    for i in 0..n {
        let _ = write!(
            source,
            "    s{i} := S{i} {{ a: +1 }}\n    t = t + s{i}.m{i}()\n"
        );
    }
    source.push_str("    println(\"$t\")\n");
    source
}

#[test]
fn frame_cell_layout_probes_grow_linearly_with_locals() {
    on_test_stack(|| {
        let goal = Goal::Program {
            entry: "main".into(),
        };
        let probes = |n| {
            let (out, probes) =
                hd_driver::work::count_layout_probes(|| compile(&trait_item_cells(n), &goal));
            assert!(!out.diags.has_errors(), "{n}: {}", out.render());
            probes
        };
        probes(1); // Warm std's bodies, as in the scale metric.
        let (small, large) = (probes(500), probes(2000));
        eprintln!("frame cell layout probes: 500={small}, 2000={large}");
        assert!(
            small > 0 && large <= 5 * small,
            "4x the locals must ask at most 5x the ownership questions: {small} -> {large}"
        );
    });
}

/// A borrowed aggregate's initializer writes directly into its frame slot:
/// increasing its width cannot add a scalar load/store round trip.
#[test]
fn borrowed_memory_local_emission_is_constant_at_two_widths()
-> Result<(), Box<dyn std::error::Error>> {
    use wasmparser::{BinaryReader, Name, NameSectionReader, Parser, Payload, TypeRef};
    let mut counts = Vec::new();
    for width in [32, 320] {
        let mut source = String::from("struct Big:\n");
        for field in 0..width {
            writeln!(source, "    f{field}: i32")?;
        }
        source.push_str("\nfn make(x: i32) -> Big:\n    Big { ");
        for field in 0..width {
            write!(source, "f{field}: x, ")?;
        }
        source.push_str("}\n\nfn borrowed(x: i32) -> string:\n    let value = make(x)\n    \"${value.f0}\"\n\npub fn main() -> void $ Console:\n    println(borrowed(+7))\n");
        let output = compile(
            &source,
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!output.diags.has_errors(), "{}", output.render());
        let wasm = output.wasm.as_deref().ok_or("missing program")?;
        let mut imported = 0;
        let mut bodies = Vec::new();
        let mut target = None;
        for payload in Parser::new(0).parse_all(wasm) {
            match payload? {
                Payload::ImportSection(section) => {
                    for import in section.into_imports() {
                        imported += usize::from(matches!(import?.ty, TypeRef::Func(_)));
                    }
                }
                Payload::CodeSectionEntry(body) => bodies.push(body),
                Payload::CustomSection(section) if section.name() == "name" => {
                    for subsection in NameSectionReader::new(BinaryReader::new(
                        section.data(),
                        section.data_offset(),
                    )) {
                        if let Name::Function(map) = subsection? {
                            for entry in map {
                                let entry = entry?;
                                if entry.name.ends_with("/borrowed#0") {
                                    target = Some(entry.index as usize);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let target = target.ok_or("missing borrowed body")? - imported;
        counts.push(
            bodies[target]
                .get_operators_reader()?
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?
                .len(),
        );
    }
    assert_eq!(
        counts[0], counts[1],
        "borrow width must not expand to leaves: {counts:?}"
    );
    Ok(())
}

// #2250: an input graph shared across many parameters is visited once,
// including when its nested tuple type represents exponentially many leaves.
#[test]
fn const_inference_inputs_are_scanned_once_at_width_thousand_depth_twenty() {
    fn source(
        width: usize,
        depth: usize,
        explicit: bool,
        calls: usize,
        across_bodies: bool,
    ) -> String {
        let mut out =
            String::from("struct Fixed[N < const usize]:\n    value: i32\ntype Layer0[T] = T\n");
        for level in 1..=depth {
            let before = level - 1;
            let _ = writeln!(
                out,
                "type Layer{level}[T] = (Layer{before}[T], Layer{before}[T])"
            );
        }
        let parameters = (0..width)
            .map(|i| format!("p{i}: Layer{depth}[Fixed[N]]"))
            .collect::<Vec<_>>()
            .join(", ");
        let arguments = vec!["value"; width].join(", ");
        let written = if explicit { "::[4]" } else { "" };
        if across_bodies {
            let _ = writeln!(
                out,
                "fn inner[N < const usize]({parameters}) -> void:\n    pass"
            );
            for caller in 0..calls {
                let _ = writeln!(
                    out,
                    "fn outer{caller}(value: Layer{depth}[Fixed[4]]) -> void:\n    inner{written}({arguments})"
                );
            }
        } else {
            let _ = writeln!(out, "fn outer(value: Layer{depth}[Fixed[4]]) -> void:");
            let _ = writeln!(
                out,
                "    fn inner[N < const usize]({parameters}) -> void:\n        pass"
            );
            for _ in 0..calls {
                let _ = writeln!(out, "    inner{written}({arguments})");
            }
        }
        out
    }
    on_test_stack(|| {
        // Warm only std, so the first measurement has the same cache
        // starting point as the others; each subject source is still new.
        let warm = compile("fn warm() -> void:\n    pass\n", &Goal::Analyze);
        assert!(warm.diags.is_empty(), "{}", warm.render());
        let mut shallow = 0;
        for depth in [1, 20] {
            let mut small = 0;
            for width in [100, 1_000] {
                let mut counts = [0; 2];
                for (i, explicit) in [true, false].into_iter().enumerate() {
                    let (out, steps) = hd_check::work::count_scan_work(|| {
                        compile(&source(width, depth, explicit, 1, false), &Goal::Analyze)
                    });
                    assert!(
                        out.diags.is_empty(),
                        "width {width}, depth {depth}: {}",
                        out.render()
                    );
                    counts[i] = steps;
                }
                let work = counts[1].saturating_sub(counts[0]);
                eprintln!(
                    "const input scan: width={width} depth={depth} explicit={} inferred={} incremental={work}",
                    counts[0], counts[1]
                );
                // One root per input, two edges per tuple layer, and fixed
                // call bookkeeping; no per-slot or per-input graph expansion.
                assert!(work > 0 && work <= 4 * width as u64 + 16 * depth as u64 + 128);
                if width == 100 {
                    small = work;
                }
                assert!(work <= 11 * small);
                if width == 1_000 {
                    if depth == 1 {
                        shallow = work;
                    } else {
                        assert!(work <= shallow + 16 * depth as u64 + 128);
                    }
                }
            }
        }
        for across_bodies in [false, true] {
            let mut first_walk = 0;
            for calls in [1, 1_000] {
                let mut counts = [0; 2];
                for (i, explicit) in [true, false].into_iter().enumerate() {
                    let (out, steps) = hd_check::work::count_scan_work(|| {
                        compile(
                            &source(1, 20, explicit, calls, across_bodies),
                            &Goal::Analyze,
                        )
                    });
                    assert!(out.diags.is_empty(), "calls {calls}: {}", out.render());
                    counts[i] = steps;
                }
                let work = counts[1].saturating_sub(counts[0]);
                let walk = work.saturating_sub(calls as u64);
                eprintln!(
                    "const input cache: across_bodies={across_bodies} calls={calls} incremental={work} after_probes={walk}"
                );
                if calls == 1 {
                    first_walk = walk;
                }
                assert!(first_walk > 0);
                assert_eq!(walk, first_walk, "a stable local signature is walked once");
            }
        }
    });
}
