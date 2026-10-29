use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages_with_analysis};
use hd_lint::{ANALYZER, Level, Policy, UNBOUND_DISPOSABLE, UNUSED_BINDING};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn lint(source: &str, policy: Policy) -> DiagBuf {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages_with_analysis(
        &host,
        "app",
        &Packages::default(),
        &Goal::Analyze,
        Some(&ANALYZER),
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    policy.apply(&out.analysis_diags)
}

fn policy() -> Policy {
    let mut policy = Policy::default();
    policy.set(UNUSED_BINDING, Level::Allow).unwrap();
    policy
}

const TYPES: &str = r"
use std.ops.{AsyncDisposable, Disposable}

struct Guard:
    id: i32

impl Disposable for Guard:
    fn dispose(&mut self) -> void:
        pass

struct AsyncGuard:
    id: i32

impl AsyncDisposable for AsyncGuard:
    fn dispose!(&mut self) -> void:
        pass

struct Holder:
    guard: Guard

fn guard() -> Guard:
    Guard { id: +1 }

fn async_guard() -> AsyncGuard:
    AsyncGuard { id: +2 }

fn take(_value: Guard) -> void:
    pass
";

#[test]
fn discarded_and_plain_bound_values_are_errors_by_default() {
    let source = format!(
        "{TYPES}\nfn examples() -> void:\n    guard()\n    _plain := guard()\n    let _async = async_guard()\n"
    );
    let findings = lint(&source, policy());
    assert_eq!(findings.code, [Code::UnboundDisposable; 3]);
    assert!(
        findings
            .severity
            .iter()
            .all(|severity| *severity == Severity::Error)
    );
    for i in 0..findings.len() {
        assert!(
            findings
                .get_text(findings.message[i])
                .contains("without registering cleanup")
        );
    }
}

#[test]
fn cleanup_transfer_return_and_storage_are_quiet() {
    let source = format!(
        "{TYPES}\nfn returned() -> Guard:\n    guard()\n\nfn examples() -> void:\n    using managed = guard()\n    take(guard())\n    _ := Holder {{ guard: guard() }}\n    _ := managed.id\n\nfn async_example!() -> void:\n    using! managed = async_guard()\n    _ := managed.id\n"
    );
    let findings = lint(&source, policy());
    assert!(findings.is_empty(), "{}", findings.render_json());
}

#[test]
fn policy_can_allow_or_warn() {
    let source = format!("{TYPES}\nfn example() -> void:\n    guard()\n    pass\n");
    for (level, expected) in [
        (Level::Allow, None),
        (Level::Warn, Some(Severity::Warning)),
        (Level::Deny, Some(Severity::Error)),
    ] {
        let mut selected = policy();
        selected.set(UNBOUND_DISPOSABLE, level).unwrap();
        assert_eq!(lint(&source, selected).severity.first().copied(), expected);
    }
}
