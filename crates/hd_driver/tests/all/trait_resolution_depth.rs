//! Bound proof depth (spec 09 `trait.bound.depth.limit`): a proof that
//! needs a bound deeper than 64 nested bounds errors with
//! `trait-resolution-depth`, whether or not a deeper proof would succeed.
//! Depths count from the use, so `Box` nested 63 deep proves and 64 deep
//! errors.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &MemoryStore::default(),
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

fn program(nested: usize) -> String {
    let mut ty = "i32".to_owned();
    for _ in 0..nested {
        ty = format!("Box[{ty}]");
    }
    format!(
        "trait Marker\n\nstruct Box[T]:\n    value: T\n\nimpl Marker for i32\n\nimpl[T < Marker] Marker for Box[T]\n\nfn need[T < Marker](value: T) -> void:\n    pass\n\nfn deep(value: {ty}) -> void:\n    need(value)\n"
    )
}

#[test]
fn sixty_four_nested_bounds_error() {
    let out = analyze(&program(64));
    assert_eq!(codes(&out), vec![Code::TraitResolutionDepth]);
}

#[test]
fn sixty_three_nested_bounds_prove() {
    let out = analyze(&program(63));
    assert!(codes(&out).is_empty(), "{}", out.render());
}

#[test]
fn shallow_bounds_prove() {
    let out = analyze(&program(2));
    assert!(codes(&out).is_empty(), "{}", out.render());
}
