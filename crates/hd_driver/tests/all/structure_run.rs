//! Incremental invalidation and cache reuse after a derivation field edit.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(store: &MemoryStore, files: &[(&str, &str)]) -> Output {
    build_by(store, files, build)
}

fn build_by(
    store: &MemoryStore,
    files: &[(&str, &str)],
    build: fn(&Host<'_>, &str, &Goal) -> Output,
) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    // A block with member lines beside a `@derive` of another trait of
    // its package warns, and stays legal (`annot.line.drift.derive`,
    // `annot.line.drift.legal`); nothing else may be reported.
    assert!(
        out.diags
            .code
            .iter()
            .all(|c| *c == hd_diag::Code::DerivationLineDrift),
        "{}",
        out.render()
    );
    out
}

fn run(out: &Output, name: &str) -> String {
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("structure-run-{name}.wasm"),
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
fn a_field_edit_rebuilds_the_generated_bodies() {
    let store = MemoryStore::default();
    let main = "\
use pkg.shapes.Point
use std.ops.Default

pub fn main() -> void $ Console:
    println(debug(Point::default()))
";
    let shapes = |fields: &str| {
        format!("use std.ops.Default\n\n@derive(Debug, Default)\npub struct Point:\n{fields}")
    };
    let build_of = |fields: &str| {
        let text = shapes(fields);
        build_with(&store, &[("main.hd", main), ("shapes.hd", &text)])
    };
    let steps = [
        ("    x: i32\n    y: i32\n", "Point { x: 0, y: 0 }\n"),
        ("    y: i32\n    x: i32\n", "Point { y: 0, x: 0 }\n"),
        (
            "    y: i32\n    x: i32\n    label: string\n",
            "Point { y: 0, x: 0, label: \"\" }\n",
        ),
        (
            "    x: i32\n    label: string\n",
            "Point { x: 0, label: \"\" }\n",
        ),
        (
            "    w: i32\n    label: string\n",
            "Point { w: 0, label: \"\" }\n",
        ),
    ];
    for (k, (fields, want)) in steps.iter().enumerate() {
        let out = build_of(fields);
        assert_eq!(run(&out, &format!("edit-{k}")), *want, "step {k}");
        assert_eq!(out.counters.hit("link"), 0, "step {k} reused a stale link");
        // A rebuild with no edit reuses everything.
        let again = build_of(fields);
        assert_eq!(again.counters.hit("link"), 1, "step {k} rebuilt unchanged");
        assert_eq!(again.wasm, out.wasm);
    }
}

#[test]
fn a_doc_edit_updates_metadata_without_rechecking_a_dependent() {
    let store = MemoryStore::default();
    let main = "\
use pkg.shapes.options.Pack
use std.cli.Cli

pub fn main() -> void $ Console:
    match Pack::parse([\"--help\"]):
        .Err(.Help(text)) => println(text)
        _ => panic(\"expected help\")
";
    let options = |about: &str, field: &str| {
        format!(
            "\
use std.cli.Cli

## {about}
@derive(Cli)
pub struct Pack:
    ## {field}
    pub verbose: bool
"
        )
    };
    let first = options("First about.", "First field.");
    let a = build_with(&store, &[("main.hd", main), ("shapes/options.hd", &first)]);
    let output = run(&a, "docs-first");
    assert!(output.contains("First about."), "{output}");
    assert!(output.contains("First field."), "{output}");
    let second = options("Changed about.", "Changed field.");
    let b = build_with(&store, &[("main.hd", main), ("shapes/options.hd", &second)]);
    assert_eq!(a.counters.deep_hashes, b.counters.deep_hashes);
    assert_eq!(
        a.counters.check_keys["app.main"],
        b.counters.check_keys["app.main"]
    );
    assert!(!b.counters.modules_checked.contains(&"app.main".to_owned()));
    assert!(
        b.counters
            .modules_checked
            .contains(&"app.shapes.options".to_owned())
    );
    let output = run(&b, "docs-changed");
    assert!(output.contains("Changed about."), "{output}");
    assert!(output.contains("Changed field."), "{output}");
    assert!(
        !output.contains("First about.") && !output.contains("First field."),
        "{output}"
    );
    let warm = build_with(&store, &[("main.hd", main), ("shapes/options.hd", &second)]);
    assert_eq!(warm.wasm, b.wasm);
    assert!(
        !warm
            .counters
            .modules_checked
            .iter()
            .any(|m| m.starts_with("app."))
    );
}
