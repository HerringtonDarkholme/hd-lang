//! A call site's root-package bit (spec 07 `fn.caller.root-package`):
//! `Location::in_root_package()` is true at a call site in the user's own
//! code, a path dependency included, and false in a package fetched for a
//! version requirement. The bit is tested at run time; the same `check`
//! entry serves both, so a fetched build after a local one still clears it.

use std::path::Path;
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{
    BuildProfile, Dependency, Executor, Goal, Host, NoClock, Output, Packages, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;

const MAIN: &str = "\
use dep.place.{report, from_inside, shown}

pub fn main() -> void $ Console:
    println(report())
    println(from_inside())
    show := report
    println(show())
    println(shown()())
";

const PLACE: &str = "\
use std.debug.Location

@track_caller
pub fn report() -> string:
    if Location::caller().in_root_package(): \"root\" else: \"fetched\"

pub fn from_inside() -> string:
    report()

pub fn shown() -> fn() -> string:
    report
";

fn sources(path: &str, text: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert(path, text);
    s
}

fn build(store: &MemoryStore, fetched: bool) -> Output {
    let (root, place) = (sources("src/main.hd", MAIN), sources("src/lib.hd", PLACE));
    let host = Host {
        render_tir: &[],
        sources: &root,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: vec![("place".to_owned(), 1)],
        deps: vec![Dependency {
            name: "place".to_owned(),
            identity: "/w/place".to_owned(),
            sources: &place,
            requires: Vec::new(),
            fetched,
        }],
        build_profile: BuildProfile::Checked,
        ..Packages::default()
    };
    build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn stdout(name: &str, out: &Output) -> String {
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("track-caller-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

#[test]
fn a_fetched_call_site_is_not_in_the_root_package() {
    let store = MemoryStore::default();
    let local = stdout("local", &build(&store, false));
    assert_eq!(local, "root\nroot\nroot\nroot\n");
    // The second build reuses the first one's `check` entries.
    let fetched = stdout("fetched", &build(&store, true));
    assert_eq!(fetched, "root\nfetched\nroot\nfetched\n");
}
