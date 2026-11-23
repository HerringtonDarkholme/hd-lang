//! The release profile (`cli.profile.release`, `types.arith.release`):
//! `+`, `-`, `*`, `**` and unary `-` wrap at every integer width, and a
//! shift count is masked to the width (`types.arith.shift-count.release`).
//! Division, remainder and `from_i64` behave as in a checked build
//! (`types.arith.always`, `trait.num.from-i64-checked`). The profile is in
//! every code and program key, so debug and release builds of one program
//! never share code; a test run stays checked (`cli.profile.test`).
//! Programs build through the driver and run on V8 (`host/run.ts`). The
//! wrapping and kept checks of single builds are stage cases
//! (`tests/cases/release-profile/`), as are programs run in every build
//! (`tests/cases/all-builds/`); these tests share a store across
//! profiles.

use std::path::Path;
use std::process::Output as Run;

use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;

fn build_in(
    store: &MemoryStore,
    src: &MemorySources,
    goal: &Goal,
    profile: BuildProfile,
) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        build_profile: profile,
        ..Packages::default()
    };
    let out = build_packages(&host, "app", &packages, goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

fn program(store: &MemoryStore, src: &MemorySources, profile: BuildProfile) -> Output {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    build_in(store, src, &goal, profile)
}

pub(super) fn run_wasm(name: &str, wasm: &[u8]) -> Run {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("release-profile-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = hd_testkit::node::command()
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

fn stdout(name: &str, ran: &Run) -> String {
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout.clone()).expect("UTF-8")
}

fn panics(name: &str, ran: &Run, category: &str) {
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        !ran.status.success() && stderr.contains(&format!("panic: {category}:")),
        "{name}: expected {category}, got status {:?}, stderr {stderr}",
        ran.status
    );
}

fn one_file(text: &str) -> MemorySources {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    src
}

const NEXT: &str = include_str!("programs/release_profile_run/next.hd");

/// Debug, then release, then debug again from one store: each build runs
/// its own profile's code, and the second debug build reads the first's.
#[test]
fn debug_and_release_builds_never_share_code() {
    let store = MemoryStore::default();
    let src = one_file(NEXT);
    let debug = program(&store, &src, BuildProfile::Checked);
    let debug_wasm = debug.wasm.expect("wasm");
    panics("debug", &run_wasm("debug", &debug_wasm), "integer-overflow");

    let release = program(&store, &src, BuildProfile::Release);
    assert_eq!(
        release.counters.miss("link"),
        1,
        "release has its own program key"
    );
    let release_wasm = release.wasm.expect("wasm");
    assert_ne!(release_wasm, debug_wasm);
    assert_eq!(
        stdout("release", &run_wasm("release", &release_wasm)),
        "-2147483648\n"
    );

    let again = program(&store, &src, BuildProfile::Checked);
    assert_eq!(again.counters.hit("link"), 1);
    let again_wasm = again.wasm.expect("wasm");
    assert_eq!(again_wasm, debug_wasm);
    panics(
        "debug-again",
        &run_wasm("debug-again", &again_wasm),
        "integer-overflow",
    );

    let release_again = program(&store, &src, BuildProfile::Release);
    assert_eq!(release_again.counters.hit("link"), 1);
    assert_eq!(release_again.wasm.as_deref(), Some(release_wasm.as_slice()));
}

/// A test run is checked whatever profile it is given, so it shares the
/// checked build's program key.
#[test]
fn a_test_run_keeps_the_test_profile() {
    let text = include_str!("programs/release_profile_run/next_with_a_test.hd");
    let src = one_file(text);
    let store = MemoryStore::default();
    let goal = Goal::Tests {
        module: None,
        filter: None,
    };
    let checked = build_in(&store, &src, &goal, BuildProfile::Checked);
    assert!(checked.wasm.is_some());
    let release = build_in(&store, &src, &goal, BuildProfile::Release);
    assert_eq!(release.counters.hit("link"), 1);
    assert_eq!(release.wasm, checked.wasm);
}
