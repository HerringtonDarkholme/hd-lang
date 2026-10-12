//! The body of every registration form may use `TestRunner`, as a plain
//! `it` body does (`module.testing.unit-row.test-runner`,
//! `std-testing.runner.body-property`), and no capability that the test
//! runner does not bind: a registration function takes its body's row.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(text: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("m.hd", text);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(
        &host,
        "app",
        &Goal::Tests {
            module: None,
            filter: None,
        },
    )
}

fn names(out: &Output) -> Vec<&str> {
    out.tests.iter().map(|c| c.name.as_str()).collect()
}

const EVERY_FORM: &str = "\
use std.testing.{Choices, TestRunner, it_each, it_prop, it_prop_with}
use std.time.s

fn pick(regions: List[string]) -> string $ TestRunner:
    regions[$.use(TestRunner).row(regions.len())]

fn small(choices: mut Choices) -> i32:
    choices.int(1, 10)

tests:
    it(\"timed\", timeout=1s):
        _ := pick([\"a\", \"b\"])

    it_each(\"table\", [1, 2], body=fn!(attempt: i32):
        _ := pick([\"a\", \"b\"])
    )

    it_prop(\"property\", prop=fn!(quantity: i32):
        _ := pick([\"a\", \"b\"])
    )

    it_prop_with(\"generated\", gen=small, prop=fn!(quantity: i32):
        _ := pick([\"a\", \"b\"])
    )
";

#[test]
fn every_registration_form_admits_test_runner() {
    let out = run(EVERY_FORM);
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(names(&out), ["timed", "table", "property", "generated"]);
}

const RENAMED: &str = "\
use std.testing.{Choices, TestRunner, it_prop as property, it_prop_with as generated}

tests:
    property(\"renamed property\", prop=fn!(quantity: i32):
        _ := $.use(TestRunner).row(1)
    )

    generated(\"renamed generated\", gen=fn(c: mut Choices) -> i32: 1, prop=fn!(quantity: i32):
        _ := $.use(TestRunner).row(1)
    )
";

#[test]
fn renamed_property_forms_admit_test_runner() {
    let out = run(RENAMED);
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(names(&out), ["renamed property", "renamed generated"]);
}

const OTHER_CAPABILITY: &str = "\
use std.testing.{Choices, PropertyRunner, it_each, it_prop, it_prop_with}

tests:
    it_each(\"table\", [1], body=fn!(value: i32):
        println(value)
    )

    it_prop(\"property\", prop=fn!(value: i32):
        _ := $.use(PropertyRunner)
    )

    it_prop_with(\"generated\", gen=fn(c: mut Choices) -> i32: 1, prop=fn!(value: i32):
        println(value)
    )
";

#[test]
fn a_body_needing_another_capability_is_still_an_error() {
    let out = run(OTHER_CAPABILITY);
    let text = out.render();
    assert_eq!(text.matches("missing-requirement").count(), 3, "{text}");
    assert!(text.contains("PropertyRunner"), "{text}");
    assert!(text.contains("Console"), "{text}");
}
