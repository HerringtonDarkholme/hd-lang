//! Impls declared in a `tests:` block (`names.tests.module-items`): the
//! library build, which compiles no test code (`module.test.code`), never
//! sees them, even from the store a test build just filled. The test build's
//! own checks are stage cases (`tests/cases/tests-block-impls/`).

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `greet(Clock)` needs `Show for Clock`, which only the block declares.
const MAIN: &str = include_str!("programs/tests_block_impls/main.hd");

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
