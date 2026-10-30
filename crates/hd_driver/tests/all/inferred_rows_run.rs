//! Inferred rows of private callables (#194; spec 11
//! `req.row.omitted.inferred-private`, `req.row.omitted.cycle`,
//! `req.row.omitted.closure-row`; `crates/hd_tir/DESIGN.md` §4.13.1 "M3:
//! inferred rows"). A private function or inherent method without a `$`
//! clause takes the least row its body needs, a recursive group the least
//! rows that satisfy every member, and each caller sees that row. Programs
//! build through the driver and run on V8 (`host/run.ts`), as the
//! conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(store: &MemoryStore, main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

/// The program's standard output, building against a previously filled store.
fn output_in(store: &MemoryStore, name: &str, main: &str) -> String {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = run(store, main, &goal);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("inferred-row-{name}.wasm"),
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

const TAGS: &str = "
pub trait Tag:
    fn name(self) -> string

pub trait Log:
    fn tag(self) -> string

struct Named:
    label: string

impl Tag for Named:
    fn name(self) -> string: self.label

impl Log for Named:
    fn tag(self) -> string: self.label
";

/// An edit that changes a private function's inferred row changes the
/// code of every caller that passes it providers: `read`'s text and
/// `call`'s are equal in both programs, but `inner` needs `Log` only in
/// the second, so `read`'s row and `call`'s call of it change. The two
/// builds share one store and must not share that code.
#[test]
fn an_edit_that_changes_an_inferred_row_misses_its_callers_code() {
    let store = crate::warm_store();
    let program = |inner: &str| {
        format!(
            "{TAGS}
fn inner() -> string:
    {inner}

fn read() -> string:
    inner()

fn call() -> string $ Tag + Log:
    read()

pub fn main() -> void $ Console:
    $.with(Tag=Named {{ label: \"t\" }}, Log=Named {{ label: \"l\" }}):
        println(call())
"
        )
    };
    let first = program("$.use(Tag).name()");
    assert_eq!(output_in(&store, "edit-first", &first), "t\n");
    let second = program("$.use(Tag).name() + $.use(Log).tag()");
    assert_eq!(output_in(&store, "edit-second", &second), "tl\n");
    assert_eq!(output_in(&store, "edit-again", &first), "t\n");
}
