//! `Packages::reuse` (#1423): a later build takes each folder interface an
//! earlier build of the caller left, instead of reading and decoding its
//! `iface` entry again, and builds what a build without it builds. A folder
//! whose key moved is read as usual.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, Reuse, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build(main: &str, store: &MemoryStore, goal: &Goal, reuse: Option<&Reuse>) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        reuse,
        ..Packages::default()
    };
    build_packages(&host, "app", &packages, goal)
}

fn program() -> Goal {
    Goal::Program {
        entry: "main".to_owned(),
    }
}

fn iface_reads(out: &Output) -> usize {
    out.counters.hits.get("iface").copied().unwrap_or(0)
        + out.counters.misses.get("iface").copied().unwrap_or(0)
}

const MAIN: &str = include_str!("programs/reuse_ifaces/main.hd");

#[test]
fn a_program_build_takes_the_interfaces_its_check_build_left() {
    let store = MemoryStore::default();
    let reuse = Reuse::default();
    let check = build(MAIN, &store, &Goal::CheckTests, Some(&reuse));
    assert!(!check.diags.has_errors(), "{}", check.render());
    let reused = build(MAIN, &store, &program(), Some(&reuse));
    let fresh = build(MAIN, &store, &program(), None);
    assert!(fresh.wasm.is_some(), "{}", fresh.render());
    assert_eq!(reused.wasm, fresh.wasm);
    assert_eq!(reused.render(), fresh.render());
    assert_eq!(iface_reads(&reused), 0);
    assert!(iface_reads(&fresh) > 0);
    // What the program build decoded is its own count, not the check's.
    assert!(
        reused.counters.iface_records <= fresh.counters.iface_records,
        "{} > {}",
        reused.counters.iface_records,
        fresh.counters.iface_records
    );
}

#[test]
fn a_folder_whose_key_moved_is_read_again() {
    let store = MemoryStore::default();
    let reuse = Reuse::default();
    build(MAIN, &store, &Goal::CheckTests, Some(&reuse));
    let edited = MAIN.replace("-> i32:\n    41", "-> i64:\n    41");
    let reused = build(&edited, &store, &program(), Some(&reuse));
    let fresh = build(&edited, &MemoryStore::default(), &program(), None);
    assert!(fresh.wasm.is_some(), "{}", fresh.render());
    assert_eq!(reused.wasm, fresh.wasm);
    // The root folder alone: std's interfaces are taken.
    assert_eq!(reused.counters.ifaces_built, ["app"]);
    assert_eq!(iface_reads(&reused), 1);
}

#[test]
fn a_reused_interface_reports_its_header_findings_again() {
    let text = include_str!("programs/reuse_ifaces/header_findings.hd");
    let store = MemoryStore::default();
    let reuse = Reuse::default();
    let check = build(text, &store, &Goal::CheckTests, Some(&reuse));
    assert!(check.diags.has_errors(), "{}", check.render());
    let reused = build(text, &store, &program(), Some(&reuse));
    let fresh = build(text, &store, &program(), None);
    assert_eq!(iface_reads(&reused), 0);
    assert_eq!(reused.render(), fresh.render());
    assert!(reused.diags.has_errors());
}

/// A new run decodes the anonymous sum from its stored interface and bodies,
/// preserving nested positional identities through checking and collection.
#[test]
fn anonymous_sums_survive_warm_interface_and_body_reads() {
    let text = include_str!("programs/reuse_ifaces/anonymous_sums.hd");
    assert_anonymous_warm_reads(text);
}

/// Fixed, nested and rest products keep their shape through both cache layers.
#[test]
fn anonymous_products_survive_warm_interface_and_body_reads() {
    let text = include_str!("programs/reuse_ifaces/anonymous_products.hd");
    assert_anonymous_warm_reads(text);
}

fn assert_anonymous_warm_reads(text: &str) {
    let store = MemoryStore::default();
    let cold = build(text, &store, &program(), None);
    assert!(cold.wasm.is_some(), "{}", cold.render());
    let warm = build(text, &store, &program(), None);
    assert!(!warm.diags.has_errors(), "{}", warm.render());
    assert_eq!(warm.wasm, cold.wasm);
    assert_eq!(warm.render(), cold.render());
    assert!(warm.counters.hit("iface") > 0);
    assert_eq!(warm.counters.miss("iface"), 0);
    assert!(warm.counters.bodies_owned.is_empty());
}
