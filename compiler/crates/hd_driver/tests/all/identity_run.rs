//! Identity comparison, `a is b` (expr.is.*): programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("identity-{name}.wasm"));
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

const PRELUDE: &str = "\
data User:
    name: string

data Unit: pass

data Other: pass

data Marker[T]: pass

impl Display for User:
    fn to_string(self) -> string:
        self.name
";

fn run(name: &str, main: &str) -> String {
    output_of(name, &format!("{PRELUDE}\n{main}"))
}

#[test]
fn one_data_value_is_itself_and_equal_values_differ() {
    let main = "\
pub fn main() -> void $ Console:
    ada := User { name: \"Ada\" }
    alias := ada
    twin := User { name: \"Ada\" }
    println(\"${ada is alias} ${ada is twin} ${ada is ada}\")
";
    assert_eq!(run("data", main), "true false true\n");
}

#[test]
fn a_list_and_its_mut_alias_share_identity() {
    let main = "\
pub fn main() -> void $ Console:
    let items: mut List[i32] = [+1]
    same := items
    let view: List[i32] = items
    let other: List[i32] = [+1]
    println(\"${items is same} ${items is view} ${view is other}\")
";
    assert_eq!(run("list", main), "true true false\n");
}

#[test]
fn a_data_value_is_its_trait_value_conversion() {
    let main = "\
pub fn main() -> void $ Console:
    ada := User { name: \"Ada\" }
    twin := User { name: \"Ada\" }
    let shown: dyn Display = ada
    let erased: dyn Any = ada
    println(\"${ada is shown} ${shown is ada} ${ada is erased} ${twin is shown} ${shown is erased}\")
";
    assert_eq!(run("trait-value", main), "true true true false true\n");
}

#[test]
fn fieldless_data_values_are_canonical() {
    let main = "\
fn make() -> Unit:
    Unit {}

pub fn main() -> void $ Console:
    first := Unit {}
    second := Unit {}
    other := Other {}
    let a: dyn Any = first
    let b: dyn Any = other
    let ints: Marker[i32] = Marker {}
    let strs: Marker[string] = Marker {}
    let ints2: Marker[i32] = Marker {}\n    let c: dyn Any = ints
    let d: dyn Any = strs
    println(\"${first is second} ${make() is first} ${a is b} ${c is d} ${ints is ints2}\")
";
    assert_eq!(run("canonical", main), "true true false false true\n");
}

#[test]
fn distinct_identity_is_a_negated_is() {
    let main = "\
pub fn main() -> void $ Console:
    ada := User { name: \"Ada\" }
    twin := User { name: \"Ada\" }
    println(\"${!(ada is twin)} ${!(ada is ada)}\")
";
    assert_eq!(run("negated", main), "true false\n");
}
