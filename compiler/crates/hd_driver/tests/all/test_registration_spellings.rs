//! A test registration call is known by declaration identity, not by
//! spelling (`module.testing.reg.identity`): a renamed import, a qualified
//! call and the prelude's `it` all register test cases, and an ordinary
//! function does not.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
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

const RENAMED: &str = "\
use std.testing.{assert, it as check, it_each as each, it_prop as property}

tests:
    check(\"adds\"):
        assert(1 + 1 == 2, reason=\"small sums\")

    each(\"doubles\", [1, 2], body=fn!(value: i32): assert(value * 2 > value, reason=\"grows\"))

    property(\"squares\", prop=fn!(value: i8): assert(true, reason=\"always\"))
";

#[test]
fn renamed_imports_register_cases() {
    let out = run(&[("m.hd", RENAMED)]);
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(names(&out), ["adds", "doubles", "squares"]);
}

const QUALIFIED: &str = "\
use std.testing
use std.testing.assert

tests:
    testing.it(\"adds\"):
        assert(1 + 1 == 2, reason=\"small sums\")

    testing.it_each(\"halves\", [2, 4], body=fn!(value: i32): assert(value > 0, reason=\"positive\"))

    testing.it_prop_with(\"picks\", gen=fn(choices: mut testing.Choices) -> i32: choices.int(1, 3),
                         prop=fn!(value: i32): assert(value > 0, reason=\"positive\"))
";

#[test]
fn qualified_calls_register_cases() {
    let out = run(&[("m.hd", QUALIFIED)]);
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(names(&out), ["adds", "halves", "picks"]);
}

const HELPERS: &str = "\
pub fn note(name: string, body: fn!() -> void) -> void: pass
";

const ORDINARY: &str = "\
use pkg.helpers.note as check

tests:
    check(\"adds\"):
        pass
";

#[test]
fn an_ordinary_function_does_not_register() {
    let out = run(&[("helpers.hd", HELPERS), ("m.hd", ORDINARY)]);
    let text = out.render();
    assert!(text.contains("invalid-test-statement"), "{text}");
    assert!(names(&out).is_empty(), "{text}");
}

const MISPLACED: &str = "\
use std.testing.{it as check, it_each as each}

fn helper() -> void:
    each(\"rows\", [1, 2], body=fn!(value: i32): pass)

tests:
    check(\"adds\"):
        pass
";

#[test]
fn a_renamed_call_outside_test_position_is_misplaced() {
    let out = run(&[("m.hd", MISPLACED)]);
    let text = out.render();
    assert!(text.contains("misplaced-test-case"), "{text}");
}
