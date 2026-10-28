use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf, Severity};
use hd_driver::{
    Executor, Goal, Host, NoClock, Output, Packages, build_packages, build_packages_with_analysis,
};
use hd_lint::{
    ANALYZER, Level, Policy, UNREACHABLE, UNSIGNED_COMPARISON, UNUSED_BINDING, UNUSED_DEBUG,
};
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

fn policy() -> Policy {
    let mut policy = Policy::default();
    policy.set(UNUSED_BINDING, Level::Allow).unwrap();
    policy.set(UNSIGNED_COMPARISON, Level::Allow).unwrap();
    policy
}

fn lint(source: &str, policy: Policy) -> (Output, DiagBuf) {
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

#[test]
fn exact_debug_message_expression_spans_and_utf8_fix_ranges_are_preserved() {
    let source = include_str!("../../../spec/conformance/cli/fix-discarded-debug/src/lib.hd");
    let (out, findings) = lint(source, policy());
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(findings.code, [Code::UnusedDebugText; 3]);
    for (i, start) in [93, 152, 238].into_iter().enumerate() {
        assert_eq!(
            findings.get_text(findings.message[i]),
            "a `debug(...)` statement discards its text; use `dbg(...)` to print it"
        );
        let span = findings.primary[i];
        assert!(source[span.lo as usize..span.hi as usize].starts_with("debug("));
        assert_eq!(findings.fixes[i].len, 1);
        assert_eq!(
            findings.get_text(findings.fix_title[i]),
            "replace discarded `debug` with `dbg`"
        );
        let edit = findings.edits[i];
        assert_eq!((edit.span.lo, edit.span.hi), (start, start + 5));
        assert_eq!(findings.get_text(edit.text), "dbg");
    }
}

#[test]
fn every_statement_after_return_break_continue_and_infinite_loop_is_reported() {
    for source in [
        "fn f() -> i32:\n    return +1\n    debug(+2)\n    +3\n",
        "fn f() -> void:\n    while true:\n        break\n        debug(+2)\n        pass\n",
        "fn f() -> void:\n    while true:\n        continue\n        debug(+2)\n        pass\n",
        "fn f() -> void:\n    while true:\n        pass\n    debug(+2)\n    pass\n",
    ] {
        let (out, findings) = lint(source, policy());
        assert!(out.diags.is_empty(), "{}", out.render());
        assert_eq!(
            findings
                .code
                .iter()
                .filter(|c| **c == Code::UnreachableCode)
                .count(),
            2,
            "{}",
            findings.render_json()
        );
        assert_eq!(
            findings
                .code
                .iter()
                .filter(|c| **c == Code::UnusedDebugText)
                .count(),
            1
        );
        for i in 0..findings.len() {
            if findings.code[i] == Code::UnreachableCode {
                assert_eq!(
                    findings.get_text(findings.message[i]),
                    "this statement is unreachable"
                );
            }
        }
    }
    for source in [
        "fn f() -> void:\n    while true:\n        break\n    pass\n",
        "fn f() -> void:\n    panic(\"stop\")\n    pass\n",
    ] {
        let (_, findings) = lint(source, policy());
        assert!(findings.is_empty(), "{}", findings.render_json());
    }
}

#[test]
fn debug_identity_and_exact_fix_safety_distinguish_source_forms() {
    let source = "use std.format\nfn f() -> void:\n    debug::[i32](+1)\n    debug(value=+2)\n    format.debug(+3)\n    debug((+4,)...)\n    `debug`(+5)\n    _ := debug(+6)\n    let shown = debug(+7)\n    _ := shown\n    pass\nfn returned() -> string:\n    debug(+8)\n";
    let (_, findings) = lint(source, policy());
    assert_eq!(
        findings.code,
        [Code::UnusedDebugText; 5],
        "{}",
        findings.render_json()
    );
    assert!(findings.fix_title.is_empty());
    assert!(findings.edits.is_empty());
    let source = "fn f() -> void:\n    shown := debug::[i32]\n    shown(+1)\n    pass\n";
    let (_, findings) = lint(source, policy());
    assert!(findings.is_empty(), "{}", findings.render_json());
}

#[test]
fn independent_levels_never_suppress_core_discard_or_dead_code_errors() {
    let source = "fn f() -> i32:\n    return +1\n    debug(+2)\n    +3\n";
    for (name, other) in [(UNREACHABLE, UNUSED_DEBUG), (UNUSED_DEBUG, UNREACHABLE)] {
        for (level, expected) in [
            (Level::Allow, None),
            (Level::Warn, Some(Severity::Warning)),
            (Level::Deny, Some(Severity::Error)),
        ] {
            let mut policy = policy();
            policy.set(name, level).unwrap();
            policy.set(other, Level::Allow).unwrap();
            let (_, findings) = lint(source, policy);
            assert!(findings.severity.iter().all(|s| Some(*s) == expected));
            assert_eq!(
                findings.len(),
                if expected.is_none() {
                    0
                } else if name == UNREACHABLE {
                    2
                } else {
                    1
                }
            );
        }
    }
    for (source, code) in [
        (
            "fn f() -> void:\n    return\n    missing\n",
            Code::UnknownName,
        ),
        (
            "fn work!() -> void: pass\nfn f() -> void:\n    work()\n",
            Code::DiscardedMustUseValue,
        ),
        ("fn f() -> void:\n    +1\n", Code::TypeMismatch),
    ] {
        let out = checked(
            source,
            &MemoryStore::default(),
            true,
            Executor::Serial(SerialOrder::Priority),
        );
        assert!(out.diags.code.contains(&code), "{}", out.render());
    }
}

#[test]
fn rejected_trials_rollback_statement_and_discard_facts() {
    let source = "trait Parse[T]:\n    fn parse(f: fn() -> T) -> bool\nstruct Item: pass\nimpl Parse[i32] for Item:\n    fn parse(f: fn() -> i32) -> bool: f() >= +0\nimpl Parse[bool] for Item:\n    fn parse(f: fn() -> bool) -> bool: f()\nfn choose(value: u32) -> bool:\n    Item::parse(fn():\n        debug(value)\n        return value >= 0\n        debug(value)\n        true\n    )\n";
    let (_, findings) = lint(source, policy());
    assert_eq!(findings.len(), 4, "{}", findings.render_json());
}

#[test]
fn inferred_parallel_and_warm_checks_keep_each_site_once_and_normal_caches_quiet() {
    use std::fmt::Write as _;
    let mut source = String::from("fn inferred():\n    debug(+1)\n    return +2\n    +3\n");
    for i in 0..64 {
        writeln!(
            source,
            "fn f_{i}() -> i32:\n    debug(+1)\n    return +2\n    +3"
        )
        .unwrap();
    }
    for executor in [Executor::Serial(SerialOrder::Priority), Executor::Pool(4)] {
        let store = MemoryStore::default();
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty(), "{}", normal.render());
        assert!(normal.analysis_diags.is_empty());
        for _ in 0..2 {
            let out = checked(&source, &store, true, executor);
            assert!(!out.diags.has_errors(), "{}", out.render());
            let findings = policy().apply(&out.analysis_diags);
            assert_eq!(
                findings.len(),
                130,
                "{executor:?}: {}",
                findings.render_json()
            );
        }
        let normal = checked(&source, &store, false, executor);
        assert!(normal.diags.is_empty());
        assert!(normal.counters.modules_checked.is_empty());
    }
}
