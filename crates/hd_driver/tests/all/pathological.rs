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
    check_operators(enum_match, 500, 2000, 2000)
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
