//! Insertion's header copies (`docs/compiler/tir-v2.md`, stage 2 commit
//! 13a): a match binding of a map's list keeps its value when the map's
//! list changes in place afterwards. The checker's former copies went
//! into `let` bindings only, so there the binding saw the push.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The standard output of `main` built through the ownership passes.
fn run_owned(name: &str, main: &str) -> String {
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
        build_profile: BuildProfile::Checked,
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("header-copies-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
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
fn a_match_binding_of_a_map_value_keeps_its_value() {
    let main = "\
fn kept(mut m: Map[string, List[i64]]) -> usize:
    match m.get(\"a\"):
        .Some(xs) =>
            m[\"a\"].push(3)
            xs.len()
        .None => 0

pub fn main() -> void $ Console:
    let mut m: Map[string, List[i64]] = {\"a\": [1, 2]}
    println(kept(m))
    println(m[\"a\"].len())
";
    assert_eq!(run_owned("match-binding", main), "2\n2\n");
}
