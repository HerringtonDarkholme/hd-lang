//! `dbg` in a release build (spec 10 `module.dbg.release`,
//! `module.dbg.own-code`): the user's own code, a path dependency
//! included, is rejected; a package fetched for a version requirement is
//! not; a checked build accepts all. One `check` entry serves both
//! profiles, so a release build after a checked one still rejects.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{
    BuildProfile, Dependency, Executor, Goal, Host, NoClock, Output, Packages, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const MAIN: &str = "\
use dep.money.{cents}

pub fn main() -> void $ Console:
    println(cents(2))
";

const MONEY: &str = "pub fn cents(n: i32) -> i32:\n    dbg(n)\n    n * 100\n";

fn sources(path: &str, text: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert(path, text);
    s
}

fn build(store: &MemoryStore, fetched: bool, profile: BuildProfile) -> Output {
    let (root, money) = (sources("src/main.hd", MAIN), sources("src/lib.hd", MONEY));
    let host = Host {
        render_tir: &[],
        sources: &root,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: vec![("money".to_owned(), 1)],
        deps: vec![Dependency {
            name: "money".to_owned(),
            identity: "/w/money".to_owned(),
            sources: &money,
            requires: Vec::new(),
            fetched,
        }],
        build_profile: profile,
        ..Packages::default()
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    build_packages(&host, "app", &packages, &goal)
}

fn rejections(out: &Output) -> usize {
    out.diags
        .code
        .iter()
        .filter(|c| **c == Code::DbgInRelease)
        .count()
}

#[test]
fn a_path_dependency_is_rejected_in_release_after_a_checked_build() {
    let store = MemoryStore::default();
    let checked = build(&store, false, BuildProfile::Checked);
    assert!(checked.diags.is_empty(), "{}", checked.render());
    assert!(checked.wasm.is_some());
    let release = build(&store, false, BuildProfile::Release);
    assert_eq!(rejections(&release), 1, "{}", release.render());
    assert!(release.wasm.is_none());
}

#[test]
fn a_fetched_dependency_is_not_the_users_own_code() {
    let store = MemoryStore::default();
    let release = build(&store, true, BuildProfile::Release);
    assert_eq!(rejections(&release), 0, "{}", release.render());
    assert!(release.wasm.is_some());
}
