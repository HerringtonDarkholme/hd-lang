use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf, Severity};
use hd_driver::{
    Executor, Goal, Host, NoClock, Output, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::{ANALYZER, Level, Policy, REDUNDANT_MUT, UNUSED_BINDING, VARIANT_BINDING};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn checked(source: &str, store: &MemoryStore, collect: bool, executor: Executor) -> Output {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store,
        clock: &NoClock,
        executor,
    };
    if collect {
        build_packages_with_analysis(
            &host,
            "app",
            &Packages::default(),
            &Goal::Analyze,
            Some(&ANALYZER),
        )
    } else {
        build_packages(&host, "app", &Packages::default(), &Goal::Analyze)
    }
}

fn lint(source: &str, mut policy: Policy) -> (Output, DiagBuf) {
    policy.set(UNUSED_BINDING, Level::Allow).unwrap();
    let out = checked(
        source,
        &MemoryStore::default(),
        true,
        Executor::Serial(SerialOrder::Priority),
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let findings = policy.apply(&out.analysis_diags);
    (out, findings)
}

const PAIR: &str = "enum Pair:\n    Values(left: i32, right: i32)\n";

#[test]
fn primitive_and_annotation_messages_and_fix_spans_are_preserved() {
    let source = include_str!("../../../spec/conformance/cli/lint-binding-patterns/src/main.hd");
    let (out, findings) = lint(source, Policy::default());
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(
        findings.code,
        [
            Code::RedundantLetMut,
            Code::RedundantLetMut,
            Code::VariantBindingNameMismatch,
            Code::VariantBindingNameMismatch
        ]
    );
    assert_eq!(
        findings.get_text(findings.message[0]),
        "`let mut` on a primitive is needed only when `&mut` of it is taken; a plain `let` is already reassignable"
    );
    assert_eq!(
        findings.get_text(findings.message[1]),
        "the annotation already grants `mut`, and no `&mut` of this binding is taken; remove the `mut` before the name"
    );
    // Compare the full CLI JSON, including fix ranges, to the frozen old producer separately.
    for i in 0..2 {
        let span = findings.primary[i];
        assert!(source[span.lo as usize..span.hi as usize].starts_with("mut "));
    }
    assert_eq!(
        findings.get_text(findings.message[2]),
        "`right` is also a field of `Values`; the field in this position is `left`"
    );
    assert_eq!(
        findings.get_text(findings.message[3]),
        "`left` is also a field of `Values`; the field in this position is `right`"
    );
}

#[test]
fn mutable_addressing_and_shadowing_keep_binding_identity() {
    let source = "fn f() -> i32:\n    let mut counter = +0\n    if true:\n        let mut counter = +1\n        pointer := &mut counter\n        *pointer += +1\n    counter\n";
    let (_, findings) = lint(source, Policy::default());
    assert_eq!(findings.code, [Code::RedundantLetMut]);
}

#[test]
fn captures_and_nonprimitive_bindings_do_not_get_redundant_mut_advice() {
    let source = "fn f() -> void:\n    let mut count = +0\n    bump := fn() -> void:\n        pointer := &mut count\n        *pointer += +1\n    bump()\n    let mut values: List[i32] = []\n    values.push(+1)\n    let mut pair = (+1, +2)\n    _ := pair\n";
    let (_, findings) = lint(source, Policy::default());
    assert!(findings.is_empty(), "{}", findings.render_json());
}

#[test]
fn correct_fresh_named_and_wildcard_payload_bindings_are_quiet() {
    for pattern in [
        ".Values(left, right)",
        ".Values(a, b)",
        ".Values(left=a, right=b)",
        ".Values(left, _)",
    ] {
        let source = format!(
            "{PAIR}fn f(value: Pair) -> void:\n    match value:\n        {pattern} => pass\n"
        );
        let (_, findings) = lint(&source, Policy::default());
        assert!(findings.is_empty(), "{pattern}: {}", findings.render_json());
    }
}

#[test]
fn policy_levels_are_independent_and_module_errors_cannot_be_suppressed() {
    let source = include_str!("../../../spec/conformance/cli/lint-binding-patterns/src/main.hd");
    for (name, other) in [
        (REDUNDANT_MUT, VARIANT_BINDING),
        (VARIANT_BINDING, REDUNDANT_MUT),
    ] {
        for (level, expected) in [
            (Level::Allow, None),
            (Level::Warn, Some(Severity::Warning)),
            (Level::Deny, Some(Severity::Error)),
        ] {
            let mut policy = Policy::default();
            policy.set(name, level).unwrap();
            policy.set(other, Level::Allow).unwrap();
            let (_, findings) = lint(source, policy);
            assert_eq!(findings.len(), if expected.is_some() { 2 } else { 0 });
            assert!(findings.severity.iter().all(|s| Some(*s) == expected));
        }
    }
    let out = checked(
        "let mut count = +0\n",
        &MemoryStore::default(),
        true,
        Executor::Serial(SerialOrder::Priority),
    );
    assert_eq!(out.diags.code, [Code::MutOnPrimitive]);
    let mut policy = Policy::default();
    policy.set(REDUNDANT_MUT, Level::Allow).unwrap();
    assert!(policy.apply(&out.analysis_diags).is_empty());
}

#[test]
fn rejected_trials_keep_only_selected_mutability_and_pattern_facts() {
    let source = format!(
        "{PAIR}trait Parse[T]:\n    fn parse(f: fn() -> T) -> bool\nstruct Item: pass\nimpl Parse[i32] for Item:\n    fn parse(f: fn() -> i32) -> bool: f() >= +0\nimpl Parse[bool] for Item:\n    fn parse(f: fn() -> bool) -> bool: f()\nfn choose(value: Pair) -> bool:\n    Item::parse(fn():\n        let mut extra = +1\n        extra += +1\n        match value:\n            .Values(right, left) => right + extra >= left\n    )\n"
    );
    let (_, findings) = lint(&source, Policy::default());
    assert_eq!(findings.len(), 3, "{}", findings.render_json());
}

#[test]
fn inferred_parallel_and_cached_bodies_keep_each_site_once() {
    use std::fmt::Write as _;
    let mut source = format!(
        "{PAIR}fn inferred(value: Pair):\n    let mut count = +1\n    count += +1\n    match value:\n        .Values(right, left) => count + left - right\n"
    );
    for i in 0..64 {
        writeln!(source, "fn f_{i}(value: Pair) -> i32:\n    let mut count = +1\n    count += +1\n    match value:\n        .Values(right, left) => count + left - right").unwrap();
    }
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty(), "{}", normal.render());
        assert!(normal.analysis_diags.is_empty());
        for _ in 0..2 {
            let (out, findings) = {
                let out = checked(&source, &store, true, executor);
                let findings = Policy::default().apply(&out.analysis_diags);
                (out, findings)
            };
            assert!(!out.diags.has_errors(), "{}", out.render());
            assert_eq!(findings.len(), 195, "{executor:?}");
        }
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty());
        assert!(normal.counters.modules_checked.is_empty());
    }
}
