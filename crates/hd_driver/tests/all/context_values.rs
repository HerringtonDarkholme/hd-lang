//! Context values in bodies (`crates/hd_tir/DESIGN.md` §4.13.4 and `crates/hd_mono/DESIGN.md`
//! §12.4, "Context values"; spec 11 "Reusable Contexts"): `$.context`
//! builds a value of `$.Context[$ Row]`, one provider per key, and
//! `$.with(ctx...)` installs its providers. Entries apply left to right
//! and a later one wins (`req.context.order`).

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

/// [`output_of`], building against `store`, which earlier builds filled.
fn output_in(store: &MemoryStore, name: &str, main: &str) -> String {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = run(store, main, &goal);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), &format!("context-{name}.wasm"));
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
    fn tag(self) -> string: \"log:\" + self.label

fn current() -> string $ Tag:
    $.use(Tag).name()

fn both() -> string $ Tag + Log:
    $.use(Tag).name() + \",\" + $.use(Log).tag()
";

/// One item path with one body under two rows takes two code entries: a
/// body's code key covers its own signature, whose row keys are its
/// provider parameters (`crates/hd_mono/DESIGN.md` §12.4). A program whose `read` needs
/// `Tag` must not lend its code to one whose `read` also needs `Log`.
#[test]
fn one_body_under_two_rows_shares_no_code() {
    let store = crate::warm_store();
    let first = format!(
        "{TAGS}
fn read() -> i32 $ Tag: 42

fn call(job: fn() -> i32 $ Tag) -> i32 $ Tag:
    job()

pub fn main() -> void $ Console:
    $.with($.context(Tag=Named {{ label: \"a\" }})...):
        println(call(read))
"
    );
    assert_eq!(output_in(&store, "row-first", &first), "42\n");
    let second = format!(
        "{TAGS}
fn read() -> i32 $ Tag + Log: 42

fn call(job: fn() -> i32 $ Tag + Log) -> i32 $ Tag + Log:
    job()

pub fn main() -> void $ Console:
    $.with(Tag=Named {{ label: \"a\" }}, Log=Named {{ label: \"b\" }}):
        println(call(read))
"
    );
    assert_eq!(output_in(&store, "row-second", &second), "42\n");
}
