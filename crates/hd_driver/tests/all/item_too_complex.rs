//! A body that runs out of fuel is one `item-too-complex` on its item
//! (spec 07 `fn.limit.complexity.error`), not an internal "not
//! implemented". The limit is implementation-defined, so no conformance
//! fixture pins it: these tests set a small budget through the test-only
//! `Packages::body_fuel` hook. The diagnostics reported before stay, and
//! an omitted result type becomes poison, so callers see no cascade. A
//! module's top level reports it on the module, a test case on its
//! registration, and a fact list on the decorated item. A header goal (a
//! signature or field bound) that runs out reports it on its item too,
//! never counting the goal as proved.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A budget that a few dozen expressions exhaust, and `main` never does.
const FUEL: u64 = 64;

/// Each diagnostic's code and 1-based line in `main.hd`, built as a
/// program.
fn diags(main: &str, fuel: Option<u64>) -> Vec<(Code, usize)> {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    diags_for(main, fuel, &goal)
}

/// Each diagnostic's code and 1-based line in `main.hd`, built for `goal`.
fn diags_for(main: &str, fuel: Option<u64>, goal: &Goal) -> Vec<(Code, usize)> {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        body_fuel: fuel,
        ..Packages::default()
    };
    let out = build_packages(&host, "app", &packages, goal);
    (0..out.diags.code.len())
        .map(|i| {
            let lo = out.diags.primary[i].lo as usize;
            (out.diags.code[i], main[..lo].matches('\n').count() + 1)
        })
        .collect()
}

/// `1 + 1 + ...` with `n` terms.
fn long_sum(n: usize) -> String {
    vec!["1"; n].join(" + ")
}

#[test]
fn a_long_body_is_too_complex_on_its_item() {
    let main = format!(
        "fn big() -> i32:\n    {}\n\npub fn main() -> void:\n    pass\n",
        long_sum(100)
    );
    assert_eq!(diags(&main, Some(FUEL)), vec![(Code::ItemTooComplex, 1)]);
    // The default budget checks the same program cleanly.
    assert_eq!(diags(&main, None), vec![]);
}

#[test]
fn earlier_diagnostics_stay() {
    let main = format!(
        "fn big() -> i32:\n    let x: i32 = true\n    {}\n\npub fn main() -> void:\n    pass\n",
        long_sum(100)
    );
    assert_eq!(
        diags(&main, Some(FUEL)),
        vec![(Code::TypeMismatch, 2), (Code::ItemTooComplex, 1)]
    );
}

#[test]
fn an_omitted_result_is_poison_for_callers() {
    let main = format!(
        "fn big():\n    {}\n\nfn caller() -> i32:\n    big() + 1\n\npub fn main() -> void:\n    _ := caller()\n",
        long_sum(100)
    );
    assert_eq!(diags(&main, Some(FUEL)), vec![(Code::ItemTooComplex, 1)]);
}

#[test]
fn a_long_default_is_too_complex() {
    let main = format!(
        "fn f(x: i32 = {}) -> i32:\n    x\n\npub fn main() -> void:\n    _ := f()\n",
        long_sum(100)
    );
    assert_eq!(diags(&main, Some(FUEL)), vec![(Code::ItemTooComplex, 1)]);
}

#[test]
fn a_long_top_level_is_too_complex_on_the_module() {
    let main = format!(
        "fn small() -> i32:\n    1\n\nx := {}\n\npub fn main() -> void:\n    _ := x\n    _ := small()\n",
        long_sum(100)
    );
    assert_eq!(diags(&main, Some(FUEL)), vec![(Code::ItemTooComplex, 1)]);
    assert_eq!(diags(&main, None), vec![]);
}

const TESTS: Goal = Goal::Tests {
    module: None,
    filter: None,
};

#[test]
fn a_long_test_case_is_too_complex_on_that_test() {
    let main = format!(
        "use std.testing.assert\n\ntests:\n    it(\"small\"):\n        assert(true, reason=\"holds\")\n\n    it(\"big\"):\n        _ := {}\n",
        long_sum(100)
    );
    assert_eq!(
        diags_for(&main, Some(FUEL), &TESTS),
        vec![(Code::ItemTooComplex, 7)]
    );
    assert_eq!(diags_for(&main, None, &TESTS), vec![]);
}

#[test]
fn a_long_registration_is_too_complex_on_that_test() {
    let main = format!(
        "tests:\n    use std.testing.it_each\n\n    it_each(\"rows\", [1, 2], body=fn!(n: i32):\n        _ := n + {}\n    )\n",
        long_sum(100)
    );
    assert_eq!(
        diags_for(&main, Some(FUEL), &TESTS),
        vec![(Code::ItemTooComplex, 4)]
    );
    assert_eq!(diags_for(&main, None, &TESTS), vec![]);
}

#[test]
fn a_long_function_fact_is_too_complex_on_the_function() {
    let main = format!(
        "fn tag(n: i32) -> i32:\n    n\n\n@tag({})\nfn tagged() -> void:\n    pass\n\npub fn main() -> void:\n    tagged()\n",
        long_sum(100)
    );
    assert_eq!(diags(&main, Some(FUEL)), vec![(Code::ItemTooComplex, 4)]);
    assert_eq!(diags(&main, None), vec![]);
}

#[test]
fn a_long_declaration_fact_is_too_complex_on_the_declaration() {
    let main = format!(
        "fn tag(n: i32) -> i32:\n    n\n\n@tag({})\ndata Tagged:\n    n: i32\n\npub fn main() -> void:\n    _ := Tagged {{ n: 1 }}\n",
        long_sum(100)
    );
    assert_eq!(diags(&main, Some(FUEL)), vec![(Code::ItemTooComplex, 4)]);
    assert_eq!(diags(&main, None), vec![]);
}

/// A bound `Marker` proved through `n` nested `Box`es: about two steps
/// each, so 40 exhaust `FUEL` while staying under the depth limit of 64.
fn marker_prelude() -> &'static str {
    "pub trait Marker\n\npub data Box[T]:\n    value: T\n\nimpl Marker for i32\n\nimpl[T < Marker] Marker for Box[T]\n\npub data Need[T < Marker]:\n    value: T\n\n"
}

/// `Box[Box[...i32]]` with `n` boxes.
fn boxes(n: usize) -> String {
    format!("{}i32{}", "Box[".repeat(n), "]".repeat(n))
}

/// Line 13 is the first line after `marker_prelude`.
const AFTER_PRELUDE: usize = 13;

#[test]
fn a_long_signature_bound_is_too_complex_on_its_function() {
    for vis in ["", "pub "] {
        let main = format!(
            "{}{vis}fn deep(value: Need[{}]) -> void:\n    pass\n\npub fn main() -> void:\n    pass\n",
            marker_prelude(),
            boxes(40)
        );
        assert_eq!(
            diags(&main, Some(FUEL)),
            vec![(Code::ItemTooComplex, AFTER_PRELUDE)],
            "{vis}fn"
        );
        assert_eq!(diags(&main, None), vec![], "{vis}fn");
    }
}

#[test]
fn a_long_field_bound_is_too_complex_on_its_data() {
    for vis in ["", "pub "] {
        let main = format!(
            "{}{vis}data Deep:\n    first: Need[{}]\n    second: Need[{}]\n\npub fn main() -> void:\n    pass\n",
            marker_prelude(),
            boxes(40),
            boxes(41)
        );
        assert_eq!(
            diags(&main, Some(FUEL)),
            vec![(Code::ItemTooComplex, AFTER_PRELUDE)],
            "{vis}data"
        );
        assert_eq!(diags(&main, None), vec![], "{vis}data");
    }
}
