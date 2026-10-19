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
    build_with_debug_code(store, fetched, profile, MONEY)
}

fn build_with_debug_code(
    store: &MemoryStore,
    fetched: bool,
    profile: BuildProfile,
    code: &str,
) -> Output {
    let (root, money) = (sources("src/main.hd", MAIN), sources("src/lib.hd", code));
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

#[test]
fn fetched_debug_warnings_and_suppression_survive_check_and_link_cache_hits() {
    let store = MemoryStore::default();
    let code = "pub fn cents(n: i32) -> i32:\n    dbg(n)\n    let show: fn((i32,)) -> void = dbg\n    show((n,))\n    n * 100\n";
    let local = build_with_debug_code(&store, false, BuildProfile::Checked, code);
    assert!(local.diags.is_empty(), "{}", local.render());
    assert!(local.wasm.is_some());
    for _ in 0..2 {
        let fetched = build_with_debug_code(&store, true, BuildProfile::Checked, code);
        assert_eq!(
            fetched
                .diags
                .code
                .iter()
                .filter(|c| **c == Code::DbgInDependency)
                .count(),
            1,
            "{}",
            fetched.render()
        );
        assert!(fetched.render().contains("fetched package `money`"));
        assert!(fetched.counters.modules_checked.is_empty());
        assert!(fetched.wasm.is_some());
        assert_ne!(local.wasm, fetched.wasm, "suppression changes emitted code");
    }
    let local_again = build_with_debug_code(&store, false, BuildProfile::Checked, code);
    assert!(local_again.diags.is_empty(), "{}", local_again.render());
    assert_eq!(local.wasm, local_again.wasm);
}
