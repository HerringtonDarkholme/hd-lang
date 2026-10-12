//! Nested optionals (spec 04 `types.option.nest`, `types.assign.optional`):
//! an `i32?` is injected into an `i32??` by one `.Some` layer, an `i32` is
//! not, and the three states of an `i32??` stay distinct at run time.
//! Programs build through the driver and run on V8 (`host/run.mjs`), as the
//! conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
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
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("nested-optional-{name}.wasm"),
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

/// The error codes of a program, in order.
fn codes(main: &str) -> Vec<Code> {
    built(main)
        .diags
        .code
        .iter()
        .copied()
        .filter(|c| !matches!(c, Code::Unsupported | Code::UnusedLocalBinding))
        .collect()
}

const SHOW: &str = "\
fn show(value: i32??) -> string:
    match value:
        .None => \"outer none\"
        .Some(.None) => \"inner none\"
        .Some(.Some(number)) => \"value $number\"
";

/// The `i32??` of an `i32?` is present outside, and all three states of
/// an `i32??` match apart, through each site that injects.
#[test]
fn an_optional_injects_into_a_nested_optional_at_every_site() {
    let main = format!(
        "{SHOW}
data Cell:
    value: i32??

fn returned(n: i32?) -> i32??:
    n

fn early(n: i32?) -> i32??:
    return n

fn chosen(flag: bool, n: i32?) -> i32??:
    if flag: n else: .None

pub fn main() -> void $ Console:
    let some: i32? = .Some(3)
    let none: i32? = .None
    let bound: i32?? = none
    println(show(bound))
    println(show(some))
    println(show(none))
    println(show(returned(some)))
    println(show(returned(none)))
    println(show(early(none)))
    println(show(chosen(true, none)))
    println(show(chosen(false, none)))
    println(show(Cell {{ value: none }}.value))
    let mut slot: i32?? = .None
    slot = some
    println(show(slot))
    slot = none
    println(show(slot))
    slot = .None
    println(show(slot))
    let list: List[i32??] = [some, .None, none, .Some(.Some(1))]
    for item in list:
        println(show(item))
"
    );
    let want = "inner none\nvalue 3\ninner none\nvalue 3\ninner none\ninner none\n\
inner none\nouter none\ninner none\nvalue 3\ninner none\nouter none\n\
value 3\nouter none\ninner none\nvalue 1\n";
    assert_eq!(output_of("sites", &main), want);
}

/// `.None` is the outer absent value and `.Some(.None)` a present outer
/// one, for scalar, string and data payloads (`types.option.nest.none`).
#[test]
fn none_and_some_none_differ_for_every_payload_layout() {
    let main = "\
data Item:
    name: string

fn text(value: string??) -> string:
    match value:
        .None => \"outer none\"
        .Some(.None) => \"inner none\"
        .Some(.Some(s)) => \"value $s\"

fn item(value: Item??) -> string:
    match value:
        .None => \"outer none\"
        .Some(.None) => \"inner none\"
        .Some(.Some(it)) => \"item ${it.name}\"

pub fn main() -> void $ Console:
    let no_text: string? = .None
    let some_text: string? = .Some(\"hi\")
    println(text(no_text))
    println(text(some_text))
    println(text(.Some(.None)))
    println(text(.None))
    let no_item: Item? = .None
    println(item(no_item))
    println(item(.Some(Item { name: \"z\" })))
    println(item(.Some(.None)))
    println(item(.None))
";
    assert_eq!(
        output_of("layouts", main),
        "inner none\nvalue hi\ninner none\nouter none\n\
inner none\nitem z\ninner none\nouter none\n"
    );
}

/// The layers show in the `Debug` text and in equality, and a third layer
/// takes one injection of an `i32??`.
#[test]
fn nested_layers_show_in_debug_text_and_equality() {
    let main = "\
fn deep(n: i32??) -> i32???:
    n

pub fn main() -> void $ Console:
    let none: i32? = .None
    let outer_none: i32?? = .None
    let inner_none: i32?? = none
    println(debug(outer_none))
    println(debug(inner_none))
    println(debug(deep(inner_none)))
    println(debug(deep(.None)))
    println(outer_none == inner_none)
    println(inner_none == inner_none)
    println(outer_none != inner_none)
";
    assert_eq!(
        output_of("debug", main),
        "Option.None\nOption.Some(Option.None)\n\
Option.Some(Option.Some(Option.None))\nOption.Some(Option.None)\nfalse\ntrue\ntrue\n"
    );
}

/// An `i32?` and an `i32??` join to the `i32??`, whichever comes first
/// (`types.lct.optional`).
#[test]
fn an_optional_and_a_nested_optional_join_to_the_nested_one() {
    let main = format!(
        "{SHOW}
pub fn main() -> void $ Console:
    let some: i32? = .Some(3)
    let none: i32? = .None
    let nested: i32?? = .Some(.None)
    let flag = true
    for item in [some, nested, none]:
        println(show(item))
    for item in [nested, some]:
        println(show(item))
    println(show(if flag: some else: nested))
    println(show(if flag: nested else: none))
"
    );
    assert_eq!(
        output_of("join", &main),
        "value 3\ninner none\ninner none\ninner none\nvalue 3\nvalue 3\ninner none\n"
    );
}

/// A plain `T` is not a `T??`: injection adds one layer only, at every
/// site (`types.option.nest.accept`, `types.assign.optional`).
#[test]
fn a_plain_value_is_not_a_nested_optional() {
    let sites = [
        "let nested: i32?? = 1",
        "let n: i32 = 1\n    let nested: i32?? = n",
        "let n: i32 = 1\n    take(n)",
        "let n: i32 = 1\n    let mut nested: i32?? = .None\n    nested = n",
    ];
    for site in sites {
        let main =
            format!("fn take(v: i32??) -> void:\n    pass\n\npub fn main() -> void:\n    {site}\n");
        let found = codes(&main);
        assert!(
            found.iter().all(|c| *c == Code::TypeMismatch) && !found.is_empty(),
            "{site}: {found:?}"
        );
    }
    let ret = "fn give(n: i32) -> i32??:\n    n\n\npub fn main() -> void:\n    pass\n";
    assert_eq!(codes(ret), vec![Code::TypeMismatch]);
}

/// A nested optional is not an optional of fewer layers, and a value of
/// two layers fewer does not inject (`types.assign.optional`).
#[test]
fn layers_never_drop_or_double() {
    let drop = "\
pub fn main() -> void:
    let nested: i32?? = .None
    let single: i32? = nested
";
    assert_eq!(codes(drop), vec![Code::TypeMismatch]);
    let double = "\
pub fn main() -> void:
    let single: i32? = .None
    let triple: i32??? = single
";
    assert_eq!(codes(double), vec![Code::TypeMismatch]);
}

/// A `T` and a `T??` have no common type, even after an `i32?` joined
/// (`types.lct.optional.one-layer`).
#[test]
fn a_plain_value_and_a_nested_optional_have_no_common_type() {
    let main = "\
pub fn main() -> void:
    let nested: i32?? = .None
    let list = [1, nested]
";
    assert!(codes(main).contains(&Code::NoCommonType));
}
