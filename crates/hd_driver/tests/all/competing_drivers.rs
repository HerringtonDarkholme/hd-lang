//! Competing drivers (issue #56, step 2): the root answers only the
//! driver that claimed it. A second driver's poll of a live root raises
//! `suspension-competing-driver`; the owner's own re-poll stays pending.
//! Driven on Node against a scenario build, which links the same shared
//! poll runtime both engines run.

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
            hold: None,
        },
    )
}

/// A root that stays pending past its first poll: a sleep under way.
fn main() -> String {
    "use std.time.{Clock, Duration, sleep}\n\npub fn main!() -> void $ Clock:\n    sleep!(Duration::milliseconds(60000))\n".to_owned()
}

/// Drives one scenario against `wasm`: init, one poll as driver 0 (which
/// must stay pending), then either a re-poll as driver 0 or a first poll
/// as driver 1, and reports what happened on standard output.
fn script(node_mjs: &Path) -> String {
    format!(
        "\
import {{ readFileSync }} from \"node:fs\";
import {{ createHost }} from \"{}\";
const [wasmPath, mode] = process.argv.slice(2);
const bytes = readFileSync(wasmPath);
let err = \"\";
const host = createHost(
  {{ out: () => {{}}, err: (t) => {{ err += t; }} }},
  {{ args: [], program: \"\", grants: {{}}, providers: null }},
);
const {{ instance }} = await WebAssembly.instantiate(bytes, host.imports);
host.prime(instance);
const ex = instance.exports;
ex[\"hd.init\"]();
const first = ex[\"hd.poll\"](0);
if (first !== -1) {{
  process.stdout.write(`first poll returned ${{first}}, not pending\\n`);
  process.exit(1);
}}
if (mode === \"same\") {{
  const second = ex[\"hd.poll\"](0);
  process.stdout.write(second === -1 ? \"owner-repoll-pending\\n\" : `second poll returned ${{second}}\\n`);
}} else {{
  try {{
    ex[\"hd.poll\"](1);
    process.stdout.write(\"foreign poll did not trap\\n\");
  }} catch {{
    process.stdout.write(err.includes(\"panic: suspension-competing-driver:\") ? \"competitor-rejected\\n\" : `other panic: ${{err}}\\n`);
  }}
}}
host.close();
",
        node_mjs.display(),
    )
}

fn drive(mode: &str) -> String {
    let out = built(&main());
    assert!(out.diags.is_empty(), "{}", out.render());
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let wasm_path = dir.join(format!("scenario-drivers-{mode}.wasm"));
    std::fs::write(&wasm_path, out.wasm.expect("wasm")).expect("write wasm");
    let node_mjs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/node.mjs");
    let script_path = dir.join(format!("scenario-drivers-{mode}.mjs"));
    std::fs::write(&script_path, script(&node_mjs)).expect("write script");
    let ran = Command::new("node")
        .arg(&script_path)
        .arg(&wasm_path)
        .arg(mode)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&wasm_path);
    let _ = std::fs::remove_file(&script_path);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

#[test]
fn a_second_driver_polling_a_live_root_is_rejected() {
    assert_eq!(drive("foreign"), "competitor-rejected\n");
}

#[test]
fn the_owner_repolling_a_live_root_stays_pending() {
    assert_eq!(drive("same"), "owner-repoll-pending\n");
}
