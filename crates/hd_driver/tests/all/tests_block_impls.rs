//! Impls declared in a `tests:` block (`names.tests.module-items`): a test
//! build selects them where its checker proved them, at `Collect` too
//! (#270), and the library build, which compiles no test code
//! (`module.test.code`), never sees them, even from the store a test build
//! just filled.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `greet(Clock)` needs `Show for Clock`, which only the block declares.
const MAIN: &str = "\
trait Show:
    fn show(self) -> string

struct Clock:
    hour: i32

fn greet[T < Show](t: T) -> string:
    \"hi ${t.show()}\"

pub fn main() -> void $ Console:
    println(greet(Clock { hour: 3 }))

tests:
    use std.testing.assert_equal

    struct Tag:
        name: string

    impl Show for Tag:
        fn show(self) -> string: \"tag:${self.name}\"

    impl Show for Clock:
        fn show(self) -> string: \"clock:${self.hour}\"

    it(\"selects the block's impls\"):
        assert_equal(greet(Tag { name: \"a\" }), \"hi tag:a\", reason=\"a test-only type\")
        assert_equal(greet(Clock { hour: 3 }), \"hi clock:3\", reason=\"a library type\")
";

fn built(store: &hd_cache::MemoryStore, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", MAIN);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn tests(store: &hd_cache::MemoryStore) -> Output {
    built(
        store,
        &Goal::Tests {
            module: None,
            filter: None,
        },
    )
}

#[test]
fn a_test_build_collects_the_block_impls() {
    let out = tests(&crate::warm_store());
    assert!(out.diags.is_empty(), "{}", out.render());
    assert_eq!(out.tests.len(), 1);
    assert_eq!(out.tests[0].unsupported, None);
    assert!(out.wasm.is_some());
}

#[test]
fn the_library_build_does_not_see_the_block_impls() {
    let store = crate::warm_store();
    let test_build = tests(&store);
    assert!(test_build.diags.is_empty(), "{}", test_build.render());
    let out = built(
        &store,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    let codes: Vec<Code> = out
        .diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect();
    assert_eq!(codes, [Code::UnsatisfiedTraitBound], "{}", out.render());
    assert!(out.wasm.is_none());
}

/// The module and its `tests:` block both implement `Show for Clock`.
const OVERLAP: &str = "\
trait Show:
    fn show(self) -> string

struct Clock:
    hour: i32

impl Show for Clock:
    fn show(self) -> string: \"${self.hour}h\"

fn greet[T < Show](t: T) -> string:
    \"hi ${t.show()}\"

tests:
    use std.testing.assert_equal

    impl Show for Clock:
        fn show(self) -> string: \"clock:${self.hour}\"

    it(\"greets the clock\"):
        assert_equal(greet(Clock { hour: 3 }), \"hi 3h\", reason=\"the module's impl\")
";

fn overlap_codes(goal: &Goal) -> Vec<Code> {
    let mut src = MemorySources::default();
    src.insert("main.hd", OVERLAP);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", goal);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn a_block_impl_overlapping_a_module_impl_is_reported_in_the_test_role() {
    let tests = Goal::Tests {
        module: None,
        filter: None,
    };
    assert_eq!(overlap_codes(&tests), [Code::OverlappingImpl]);
    assert_eq!(overlap_codes(&Goal::CheckTests), [Code::OverlappingImpl]);
}

#[test]
fn the_library_role_ignores_a_block_impl_overlap() {
    assert_eq!(overlap_codes(&Goal::Analyze), []);
}
