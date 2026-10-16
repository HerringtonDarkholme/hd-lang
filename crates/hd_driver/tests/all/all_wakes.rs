//! An `all!` polls only the children a wake names
//! (`crates/hd_wasm/DESIGN.md` §14.4 and §14.5): each child's host leaf
//! registers under the `all!`'s waker node, a wake sets that child's bit,
//! and later polls skip the children whose bit is clear. The count of
//! child polls is the module's exported `hd.all_polls` global.

#![cfg(feature = "wasmtime")]

use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Children in the `all!`.
const WIDE: usize = 30;

/// An `all!` of `WIDE` children, child `j` sleeping `5 * j` milliseconds,
/// so their wakes arrive apart.
fn program() -> String {
    let children: Vec<String> = (1..=WIDE).map(|j| format!("nap({j})")).collect();
    let mut names = vec!["first"];
    names.extend(vec!["_"; WIDE - 2]);
    names.push("last");
    format!(
        "use std.time.{{Clock, Duration, sleep}}\n\
         use std.task.all\n\n\
         fn nap!(j: i64) -> i64 $ Clock:\n    \
         sleep!(Duration::milliseconds(5 * j))\n    \
         j\n\n\
         pub fn main!() -> void $ Console + Clock:\n    \
         let ({}) = all!({})\n    \
         println(\"${{first}} ${{last}}\")\n",
        names.join(", "),
        children.join(", "),
    )
}

/// How many `all!` children the run polled, and its standard output.
fn polls() -> (usize, String) {
    let mut src = MemorySources::default();
    src.insert("main.hd", &program());
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target: EngineTarget::Wasmtime,
        level: OptLevel::Dev,
        ..Packages::default()
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "app", &packages, &goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let setup = hd_run::HostSetup {
        providers: Vec::new(),
        grants: hd_run::Grants::default(),
        limits: hd_run::Limits::default(),
        args: Vec::new(),
        program: String::new(),
    };
    let config = hd_run_wasmtime::EngineConfig::pipeline(OptLevel::Dev);
    let (ran, count) = hd_run_wasmtime::run_captured_reading(config, &wasm, &setup, "hd.all_polls")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ran.status, 0, "{}", String::from_utf8_lossy(&ran.stderr));
    let count = usize::try_from(count.expect("the module exports hd.all_polls")).expect("count");
    (count, String::from_utf8(ran.stdout).expect("UTF-8"))
}

#[test]
fn every_child_is_polled_once_more_after_its_wake_at_least() {
    let (count, out) = polls();
    assert_eq!(out, "1 30\n");
    assert!(count >= 2 * WIDE, "{count}");
}
