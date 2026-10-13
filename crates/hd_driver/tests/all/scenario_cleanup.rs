//! `hd.scenario.cleanup` (issue #56, step 3): a scenario build calls
//! the entry module's top-level `cleanup_ran` in the same instance when
//! it declares one, and links no such export when it does not.

use std::path::Path;
use std::process::Command;

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Scenario {
            entry: "main".into(),
        },
    )
}

/// A module's export names; validates it first.
fn exports(wasm: &[u8]) -> Vec<String> {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ExportSection(r) = payload.expect("payload") {
            for e in r {
                exports.push(e.expect("export").name.to_owned());
            }
        }
    }
    exports
}

fn with_cleanup() -> String {
    "let cleaned: bool = false\n\nfn ready!() -> i32: 42\n\npub fn main!() -> void:\n    _ := ready!()\n    cleaned = true\n\npub fn cleanup_ran() -> bool:\n    cleaned\n"
        .to_owned()
}

fn without_cleanup() -> String {
    "fn ready!() -> i32: 42\n\npub fn main!() -> void:\n    _ := ready!()\n".to_owned()
}

#[test]
fn a_scenario_build_calls_the_entry_cleanup() {
    let out = built(&with_cleanup());
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.clone().expect("wasm");
    assert!(
        exports(&wasm).contains(&"hd.scenario.cleanup".to_owned()),
        "{}",
        out.render()
    );
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let wasm_path = dir.join("scenario-cleanup-call.wasm");
    std::fs::write(&wasm_path, wasm).expect("write wasm");
    let node_mjs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/node.mjs");
    let script_path = dir.join("scenario-cleanup-call.mjs");
    std::fs::write(
        &script_path,
        format!(
            "\
import {{ readFileSync }} from \"node:fs\";
import {{ createHost }} from \"{}\";
const [wasmPath] = process.argv.slice(2);
const bytes = readFileSync(wasmPath);
const host = createHost(
  {{ out: () => {{}}, err: (t) => process.stderr.write(t) }},
  {{ args: [], program: \"\", grants: {{}}, providers: null }},
);
const {{ instance }} = await WebAssembly.instantiate(bytes, host.imports);
host.prime(instance);
const ex = instance.exports;
ex[\"hd.init\"]();
const before = ex[\"hd.scenario.cleanup\"]();
const status = ex[\"hd.poll\"](0);
const after = ex[\"hd.scenario.cleanup\"]();
process.stdout.write(`${{before}} ${{status}} ${{after}}\\n`);
host.close();
",
            node_mjs.display(),
        ),
    )
    .expect("write script");
    let ran = Command::new("node")
        .arg(&script_path)
        .arg(&wasm_path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&wasm_path);
    let _ = std::fs::remove_file(&script_path);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    // Cleanup reads the same instance: false before the run, true after.
    assert_eq!(String::from_utf8(ran.stdout).expect("UTF-8"), "0 0 1\n");
}

#[test]
fn a_scenario_without_cleanup_links_no_cleanup_export() {
    let out = built(&without_cleanup());
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let exports = exports(&wasm);
    assert!(
        !exports.contains(&"hd.scenario.cleanup".to_owned()),
        "{exports:?}"
    );
    assert!(exports.contains(&"hd.cancel".to_owned()), "{exports:?}");
}
