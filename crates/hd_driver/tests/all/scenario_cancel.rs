//! `Goal::Scenario` (issue #56, step 1): a scenario build links the
//! scenario exports, `hd.init`/`hd.poll`/`hd.cancel` for the entry root,
//! while a program build of the same source carries no `hd.cancel`. The
//! scenario build still runs to completion under the plain driver.

use std::path::Path;
use std::process::Command;

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;

fn built(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

fn scenario() -> Goal {
    Goal::Scenario {
        entry: "main".into(),
    }
}

fn program() -> Goal {
    Goal::Program {
        entry: "main".into(),
    }
}

/// The fixture shape the scenario runner drives: a suspending entry over
/// an empty row.
fn main() -> String {
    "fn ready!() -> i32: 42\n\npub fn main!() -> void $ Console:\n    value := ready!()\n    println(\"value=$value\")\n".to_owned()
}

/// A module's export names; validates it first.
fn exports(wasm: &[u8]) -> Vec<String> {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ExportSection(r) = payload.expect("payload") {
            for e in r {
                out.push(e.expect("export").name.to_owned());
            }
        }
    }
    out
}

#[test]
fn a_scenario_build_exports_hd_cancel() {
    let out = built(&main(), &scenario());
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let exports = exports(&wasm);
    for name in ["hd.init", "hd.poll", "hd.cancel"] {
        assert!(exports.contains(&name.to_owned()), "{exports:?}");
    }
}

#[test]
fn a_program_build_carries_no_hd_cancel() {
    let out = built(&main(), &program());
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let exports = exports(&wasm);
    assert!(exports.contains(&"hd.poll".to_owned()), "{exports:?}");
    assert!(!exports.contains(&"hd.cancel".to_owned()), "{exports:?}");
}

/// The scenario build runs to completion under the plain driver: the
/// extra export changes nothing about `hd.init`/`hd.poll`.
#[test]
fn a_scenario_build_runs_under_the_plain_driver() {
    let out = built(&main(), &scenario());
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "scenario-cancel-run.wasm");
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8(ran.stdout).expect("UTF-8"), "value=42\n");
}

/// `hd.cancel` after the root completed is a no-op, not a trap: the
/// null-root guard holds end to end on the plain driver.
#[test]
fn hd_cancel_after_completion_is_a_no_op() {
    let out = built(&main(), &scenario());
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let wasm_path = dir.join("scenario-cancel-after.wasm");
    std::fs::write(&wasm_path, wasm).expect("write wasm");
    let node_mjs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/node.mjs");
    let script_path = dir.join("scenario-cancel-after.mjs");
    std::fs::write(
        &script_path,
        format!(
            "\
import {{ readFileSync }} from \"node:fs\";
import {{ createHost }} from \"{}\";
const bytes = readFileSync(process.argv[2]);
const host = createHost(
  {{ out: (t) => process.stdout.write(t), err: (t) => process.stderr.write(t) }},
  {{ args: [], program: \"\", grants: {{}}, providers: null }},
);
const {{ instance }} = await WebAssembly.instantiate(bytes, host.imports);
const ran = await host.run(instance, \"hd.init\", \"hd.poll\");
if (ran.status !== 0 || ran.trapped) {{
  process.stderr.write(`run failed: ${{ran.status}}\\n`);
  process.exit(1);
}}
instance.exports[\"hd.cancel\"]();
host.close();
process.stderr.write(\"cancel-ok\\n\");
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
    assert_eq!(String::from_utf8(ran.stdout).expect("UTF-8"), "value=42\n");
    assert_eq!(String::from_utf8(ran.stderr).expect("UTF-8"), "cancel-ok\n");
}
