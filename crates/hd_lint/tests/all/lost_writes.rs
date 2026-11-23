use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf};
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages_with_analysis};
use hd_lint::{ANALYZER, Policy};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn lint(source: &str) -> DiagBuf {
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
    let output = build_packages_with_analysis(
        &host,
        "app",
        &Packages::default(),
        &Goal::Analyze,
        Some(&ANALYZER),
    );
    assert!(!output.diags.has_errors(), "{}", output.render());
    Policy::default().apply(&output.analysis_diags)
}

#[test]
fn every_direct_change_to_an_unobserved_parameter_copy_is_reported_once() {
    let source = r#"
struct Box:
    value: i32

impl Box:
    fn set(&mut self, value: i32) -> void:
        self.value = value

    fn discard_change(mut self) -> void:
        self.value = +1

fn assign(mut value: i32) -> void:
    value = +1

fn write_field(mut value: Box) -> void:
    value.value = +1

fn push(mut values: List[i32]) -> void:
    values.push(+1)

fn push_nested(mut values: List[List[i32]]) -> void:
    values[0].push(+1)

fn store_element(mut values: List[i32]) -> void:
    values[0] = +1

fn store_map(mut values: Map[string, i32]) -> void:
    values["one"] = +1

fn call_mut_receiver(mut value: Box) -> void:
    value.set(+1)
"#;
    let findings = lint(source);
    assert_eq!(
        findings.code,
        [Code::LostWrite; 8],
        "{}",
        findings.render_json()
    );
    let messages: Vec<_> = (0..findings.len())
        .map(|index| findings.get_text(findings.message[index]))
        .collect();
    assert!(messages[0].contains("write to `self`"), "{messages:?}");
    assert!(
        messages[1..]
            .iter()
            .all(|message| message.contains("parameter"))
    );
}

#[test]
fn a_deferred_read_observes_the_changed_copy() {
    let source = r"
use std.ops.defer

fn defer_read(mut values: List[i32]) -> void:
    defer(fn() -> void: _ := values.len())
    values.push(+1)
";
    let findings = lint(source);
    assert!(
        !findings.code.contains(&Code::LostWrite),
        "{}",
        findings.render_json()
    );
}

#[test]
fn a_loop_mutation_without_another_use_is_still_lost() {
    let source = r"
fn push_each(mut values: List[i32], items: List[i32]) -> void:
    for item in items:
        values.push(item)
";
    let findings = lint(source);
    assert_eq!(
        findings.code,
        [Code::LostWrite],
        "{}",
        findings.render_json()
    );
}
