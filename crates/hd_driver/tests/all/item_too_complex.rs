//! A body that runs out of fuel is one `item-too-complex` on its item
//! (spec 07 `fn.limit.complexity.error`), not an internal "not
//! implemented". The limit is implementation-defined, so no conformance
//! fixture pins it: these tests set a small budget through the test-only
//! `Packages::body_fuel` hook. The diagnostics reported before stay, and
//! an omitted result type becomes poison, so callers see no cascade.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A budget that a few dozen expressions exhaust, and `main` never does.
const FUEL: u64 = 64;

/// Each diagnostic's code and 1-based line in `main.hd`.
fn diags(main: &str, fuel: Option<u64>) -> Vec<(Code, usize)> {
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
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "app", &packages, &goal);
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
