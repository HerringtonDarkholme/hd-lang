//! A least-common-type site checks each value once
//! (`crates/hd_check/DESIGN.md` §4.3): an optional value after plain ones
//! widens the site in place, and the values checked before it get their
//! `WrapSome` in reserved slots. Nested sites that each widen therefore
//! cost fuel in proportion to their size, where starting a site over
//! would check its inner sites again at every level.

use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `if c: +1 else: [+1, ...][1]` nested `depth` times around `x`: every
/// `if` and every list literal is a site that widens to `i32?`.
fn nested(depth: usize) -> String {
    let mut e = "x".to_owned();
    for _ in 0..depth {
        e = format!("(if c: +1 else: [+1, {e}][1])");
    }
    format!("fn deep(c: bool, x: i32?) -> i32?:\n    {e}\n\npub fn main() -> void:\n    pass\n")
}

/// The diagnostics of `main` built with a body fuel of `fuel`.
fn codes(main: &str, fuel: u64) -> Vec<hd_diag::Code> {
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
        body_fuel: Some(fuel),
        ..Packages::default()
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    build_packages(&host, "app", &packages, &goal).diags.code
}

#[test]
fn nested_widening_sites_check_each_value_once() {
    // Depth 12 is about 100 expressions; checking each once fits in a
    // budget that one recheck per enclosing site (2^12 checks of the
    // innermost) exceeds many times over.
    assert_eq!(codes(&nested(12), 2_000), []);
}
