//! Incremental edits to a template or its walker invalidate derived bodies.
//! Conformance covers fixed-source runtime output; this test reuses a store
//! and also checks that the deriving module is rechecked.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(store: &MemoryStore, files: &[(&str, &str)]) -> Output {
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
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

/// Runs a built program on V8 and returns its standard output.
fn run(out: &Output, name: &str) -> String {
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("derive-run-{name}.wasm"),
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

/// The template and its walker in one module, the deriving type in
/// another: an edit of the walker's helper changes the derived output,
/// and an edit of the template method rechecks the deriving module
/// (`crates/hd_tir/DESIGN.md` §4.13.9).
#[test]
fn template_edits_change_the_derived_output_on_rebuild() {
    let show = |sep: &str, prefix: &str| {
        format!(
            "\
use std.structure.{{Structure, Field, Variant, Walker}}

pub trait Show:
    fn show(self) -> string

fn sep() -> string:
    \"{sep}\"

pub struct Shower:
    pub out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Display](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + h.info.name + sep() + \"$value \"
        .Ok(())

impl[T] Show for T by Structure:
    fn show(self) -> string:
        let mut w = Shower {{ out: \"{prefix}\" }}
        _ := Structure::walk(self, &mut w)
        w.out
"
        )
    };
    let main = "\
use pkg.show.Show

@derive(Show)
struct Point:
    x: i32
    y: i32

pub fn main() -> void $ Console:
    println(Point { x: 1, y: 2 }.show())
";
    let store = MemoryStore::default();
    let first = build_with(&store, &[("main.hd", main), ("show.hd", &show("=", ""))]);
    assert_eq!(run(&first, "edit-0"), "x=1 y=2 \n");
    let helper = build_with(&store, &[("main.hd", main), ("show.hd", &show(":", ""))]);
    assert_eq!(run(&helper, "edit-1"), "x:1 y:2 \n");
    let template = build_with(&store, &[("main.hd", main), ("show.hd", &show(":", "P "))]);
    assert_eq!(run(&template, "edit-2"), "P x:1 y:2 \n");
    assert!(
        template
            .counters
            .modules_checked
            .iter()
            .any(|m| m.ends_with("main")),
        "a template edit rechecks the deriving module: {:?}",
        template.counters.modules_checked
    );
}
