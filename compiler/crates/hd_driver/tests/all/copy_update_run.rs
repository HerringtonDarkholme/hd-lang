//! Data-literal spread, `D { ...src, f: e }` (expr.update.*, data.update.*,
//! data.part.copy-update): programs build through the driver and run on
//! V8 (`host/run.mjs`), as the conformance runner does.

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("copy-update-{name}.wasm"));
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

/// The rendered diagnostics of a program that must not build.
fn errors_of(main: &str) -> String {
    let out = built(main);
    assert!(out.wasm.is_none(), "the program must not build");
    out.render()
}

#[test]
fn a_copy_replaces_one_field_and_leaves_the_source() {
    let main = "\
data User:
    name: string
    age: i32
    admin: bool

pub fn main() -> void $ Console:
    ada := User { name: \"Ada\", age: 36, admin: true }
    renamed := User { ...ada, name: \"Grace\" }
    println(\"${renamed.name} ${renamed.age} ${renamed.admin}\")
    println(\"${ada.name} ${ada.age} ${ada.admin}\")
";
    assert_eq!(
        output_of("replace-one", main),
        "Grace 36 true\nAda 36 true\n"
    );
}

#[test]
fn a_copy_replacing_nothing_is_a_distinct_value() {
    let main = "\
data Counter:
    value: i32

pub fn main() -> void $ Console:
    let mut original = Counter { value: 1 }
    let mut copy = Counter { ...original }
    copy.value = 2
    println(\"${original.value} ${copy.value}\")
    original.value = 3
    println(\"${original.value} ${copy.value}\")
";
    assert_eq!(output_of("replace-none", main), "1 2\n3 2\n");
}

#[test]
fn a_readonly_source_fills_a_readonly_result() {
    // expr.update.readonly-source, expr.update.readonly-fill
    let main = "\
data Child:
    name: string

data Parent:
    child: mut Child
    label: string

fn copy(source: Parent) -> Parent:
    Parent { ...source, label: \"copy\" }

pub fn main() -> void $ Console:
    let mut shared = Child { name: \"a\" }
    original := Parent { child: shared, label: \"orig\" }
    duplicate := copy(original)
    shared.name = \"b\"
    println(\"${duplicate.label} ${duplicate.child.name}\")
";
    assert_eq!(output_of("readonly-fill", main), "copy b\n");
}

#[test]
fn a_mut_field_of_a_readonly_source_needs_a_mut_replacement() {
    // expr.update.readonly-fill, data.update.mutable-result
    let valid = "\
data Child:
    name: string

data Parent:
    child: mut Child
    label: string

fn fresh_child(source: Parent) -> mut Parent:
    Parent { ...source, child: Child { name: \"new\" } }

pub fn main() -> void $ Console:
    original := Parent { child: Child { name: \"old\" }, label: \"l\" }
    let mut made = fresh_child(original)
    made.child.name = \"changed\"
    println(\"${original.child.name} ${made.child.name} ${made.label}\")
";
    assert_eq!(output_of("mut-valid", valid), "old changed l\n");
    let invalid = "\
data Child:
    name: string

data Parent:
    child: mut Child
    label: string

fn upgrade(source: Parent) -> mut Parent:
    Parent { ...source, label: \"x\" }

pub fn main() -> void:
    pass
";
    let err = errors_of(invalid);
    assert!(err.contains("mutable-upgrade"), "{err}");
}

#[test]
fn a_mismatched_source_type_is_reported() {
    // expr.update.exact-type
    let main = "\
data Other:
    value: i32

data Counter:
    value: i32

fn bad(other: Other) -> Counter:
    Counter { ...other }

pub fn main() -> void:
    pass
";
    let err = errors_of(main);
    assert!(err.contains("type-mismatch"), "{err}");
}

#[test]
fn a_generic_data_type_copies_with_substituted_fields() {
    // expr.update.generic
    let main = "\
data Pair[T]:
    first: T
    second: T

fn source() -> Pair[i32]: Pair { first: 20, second: 1 }

pub fn main() -> void $ Console:
    pair := Pair { ...source(), second: 22 }
    println(pair.first + pair.second)
    words := Pair { first: \"a\", second: \"b\" }
    swapped := Pair { ...words, first: \"z\" }
    println(\"${swapped.first}${swapped.second}\")
";
    assert_eq!(output_of("generic", main), "42\nzb\n");
}

#[test]
fn a_copy_shares_composite_fields_with_its_source() {
    // expr.update.shallow
    let main = "\
data Child:
    name: string

data Parent:
    child: mut Child
    label: string

pub fn main() -> void $ Console:
    let mut original = Parent { child: Child { name: \"a\" }, label: \"x\" }
    let mut copy = Parent { ...original, label: \"y\" }
    copy.child.name = \"b\"
    println(original.child.name)
    println(original.label)
";
    assert_eq!(output_of("shallow", main), "b\nx\n");
}

#[test]
fn a_copy_makes_its_own_embedded_parts_when_the_spread_runs() {
    // data.part.copy-update, data.part.copy-time
    let main = "\
data Stamp:
    pub at: i32

impl Stamp:
    pub fn touch(mut self, at: i32) -> void:
        self.at = at

data Post:
    Stamp
    id: string

fn bump(stamp: mut Stamp) -> string:
    stamp.at = stamp.at + 1
    \"bumped\"

pub fn main() -> void $ Console:
    let mut post = Post { Stamp: ...Stamp { at: 1 }, id: \"p\" }
    let mut copy = Post { ...post, id: bump(post.Stamp) }
    println(\"${copy.at} ${post.at}\")
    copy.touch(9)
    println(\"${copy.at} ${post.at}\")
";
    assert_eq!(output_of("parts", main), "1 2\n9 2\n");
}
