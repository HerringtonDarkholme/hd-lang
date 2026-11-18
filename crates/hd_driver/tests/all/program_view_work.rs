//! Every `Env` reads the program's impls through one view (#2146): its
//! folder list is built once per run, so its work counts the program's
//! folders, not those times the bodies the passes own.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `k` folders of one module each, three functions a module, and a
/// `main` that calls every module's `run`: the folder reads and the bodies
/// owned for a cold build.
fn view_work(k: usize) -> (usize, usize) {
    let mut src = MemorySources::default();
    let mut main = String::new();
    for m in 0..k {
        src.insert(
            &format!("d{m}/m.hd"),
            &format!(
                "fn a(x: i64) -> i64:\n    x + {m}\n\nfn b(x: i64) -> i64:\n    a(x) * 2\n\npub fn run(x: i64) -> i64:\n    b(x) + a(x)\n"
            ),
        );
        let _ = writeln!(main, "use pkg.d{m}.m.run as run{m}");
    }
    main += "\npub fn main() -> void $ Console:\n    let s: i64 = 0\n";
    for m in 0..k {
        let _ = writeln!(main, "    s = s + run{m}({m})");
    }
    main += "    println(\"$s\")\n";
    src.insert("main.hd", &main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "views", &Packages::default(), &goal);
    assert!(out.wasm.is_some(), "{}", out.render());
    (out.counters.view_folders, out.counters.bodies_owned.len())
}

/// Ten times the folders read each added folder once, while the bodies
/// owned grow with the modules.
#[test]
fn the_program_view_reads_each_folder_once_per_run() {
    let (small, small_bodies) = view_work(4);
    let (large, large_bodies) = view_work(40);
    assert_eq!(large - small, 36, "{small} then {large} folder reads");
    assert!(
        large_bodies >= small_bodies + 3 * 36,
        "{small_bodies} then {large_bodies} bodies"
    );
}
