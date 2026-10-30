//! Owned iterator adapters run in both optimization pipelines.
use hd_base::{EngineTarget, OptLevel};
use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;
use std::{path::Path, process::Command};
fn wasm_of(main: &str, optimized: bool) -> Vec<u8> {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target: EngineTarget::V8,
        level: if optimized {
            OptLevel::Optimized
        } else {
            OptLevel::Dev
        },
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
    assert!(out.diags.is_empty(), "{}", out.render());
    out.wasm.expect("wasm")
}
fn run(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), &format!("{name}.wasm"));
    std::fs::write(&path, wasm).expect("write");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}
const ITERATOR_ADAPTERS: &str = "\
fn double(x: i32) -> i32:
    x * 2

fn total(items: List[i32]) -> i32:
    items.iter().map(fn(x): x * 3 + 1).filter(fn(x): x % 2 == 0).fold(0, fn(a, x): a + x)

fn refs(items: List[i32]) -> i32:
    items.iter().map(double).fold(0, fn(a, x): a + x)

pub fn main() -> void $ Console:
    println(total([+1, +2, +3, +4, +5]))
    println(refs([+1, +2, +3, +4]))
";

#[test]
fn named_iterator_adapters_run_in_both_pipelines() {
    for optimized in [false, true] {
        let wasm = wasm_of(ITERATOR_ADAPTERS, optimized);
        assert_eq!(run("named-adapters", &wasm), "30\n20\n");
    }
}
