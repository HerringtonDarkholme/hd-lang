//! A warm check reads the interface records it touches, not every record
//! of every folder it reads (docs/compiler/data-structures.md §3.9.1,
//! #213). The solver's impl tables read every impl of the closure, and a
//! method call's index every trait; any other item is decoded only when
//! named.

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(store: &MemoryStore, main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Analyze;
    let out = build_packages(&host, "app", &Packages::default(), &goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

#[test]
fn a_warm_check_decodes_few_interface_records() {
    let store = MemoryStore::default();
    let main = "pub fn main() -> void $ Console:\n    println(\"hi\")\n";
    check(&store, main);
    let edited = "pub fn main() -> void $ Console:\n    println(\"hello\")\n";
    let warm = check(&store, edited);
    let c = &warm.counters;
    // `println` and the little it names, out of std's hundreds of items.
    assert!(c.iface_other_records <= 10, "{}", c.iface_other_records);
}
