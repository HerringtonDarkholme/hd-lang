//! A type name written where a value is needed, and a name that reaches no
//! type or trait in a body (spec/lang/03-names-and-scopes.md,
//! names.type-as-value; spec/lang/04-type-system.md, types.newtype.construct,
//! types.newtype.unwrap-permission; spec/lang/11-requirements-and-suspension.md,
//! req.key.in-scope): the base type's constructor unwraps a newtype, any
//! other call of a type name is `type-used-as-value`, an unknown type is
//! `unknown-type`, an unknown key is `unknown-trait`, and a trait's own
//! parameter is a type in its default methods. Programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

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

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("type-name-forms-{name}.wasm"),
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

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const NEWTYPES: &str = "\
data Draft:
    lines: List[string]

type Order(Draft)
type Sheet = Draft

data Box[T]:
    item: T

type Crate[T](Box[T])

type Mile(i32)
";

#[test]
fn the_base_type_constructor_unwraps_a_newtype() {
    let main = format!(
        "{NEWTYPES}
fn reopen(order: mut Order) -> void:
    let editable: mut Draft = Draft(order)
    editable.lines = [\"reopened\"]

pub fn main() -> void $ Console:
    let draft: mut Draft = Draft {{ lines: [\"first\"] }}
    let order: mut Order = Order(draft)
    reopen(order)
    println(Draft(order).lines[0])
"
    );
    assert_eq!(output_of("unwrap", &main), "reopened\n");
}

#[test]
fn an_alias_unwraps_to_the_type_it_names() {
    let main = format!(
        "{NEWTYPES}
pub fn main() -> void $ Console:
    order := Order(Draft {{ lines: [\"a\", \"b\"] }})
    println(Sheet(order).lines.len())
"
    );
    assert_eq!(output_of("alias", &main), "2\n");
}

#[test]
fn a_generic_newtype_unwraps_to_its_base_with_inferred_arguments() {
    let main = format!(
        "{NEWTYPES}
pub fn main() -> void $ Console:
    let crate: Crate[i32] = Crate(Box {{ item: 5 }})
    println(Box(crate).item)
    println(Box::[i32](crate).item)
"
    );
    assert_eq!(output_of("generic", &main), "5\n5\n");
}

#[test]
fn a_primitive_constructor_still_unwraps_a_newtype() {
    let main = format!(
        "{NEWTYPES}
pub fn main() -> void $ Console:
    println(i32(Mile(10)))
"
    );
    assert_eq!(output_of("primitive", &main), "10\n");
}

#[test]
fn unwrapping_a_readonly_newtype_gives_a_readonly_base() {
    let main = format!(
        "{NEWTYPES}
fn invalid(sent: Order) -> void:
    let _frozen: mut Draft = Draft(sent)
"
    );
    assert_eq!(codes_of(&main), vec![Code::MutableUpgrade]);
}

#[test]
fn a_call_of_a_data_type_that_unwraps_nothing_is_a_type_used_as_a_value() {
    for call in ["Draft()", "Draft(1)", "Draft(1, 2)", "Draft(lines = [])"] {
        let main = format!(
            "{NEWTYPES}
fn use_it() -> void:
    _ := {call}
"
        );
        assert_eq!(codes_of(&main), vec![Code::TypeUsedAsValue], "{call}");
    }
}

#[test]
fn a_map_called_with_an_inferred_or_written_type_is_a_type_used_as_a_value() {
    for call in [
        "let _m: Map[string, i32] = Map()",
        "let _m = Map::[string, i32]()",
        "let _m: Map[string, i32] = Map::[string, i32]()",
    ] {
        let main = format!(
            "fn use_it() -> void:
    {call}
"
        );
        assert_eq!(codes_of(&main), vec![Code::TypeUsedAsValue], "{call}");
    }
}

#[test]
fn a_set_called_with_an_inferred_or_written_type_is_a_type_used_as_a_value() {
    for call in [
        "let _s: Set[string] = Set()",
        "let _s = Set::[string]()",
        "let _s = Set[string]()",
    ] {
        let main = format!(
            "use std.collections.Set

fn use_it() -> void:
    {call}
"
        );
        assert_eq!(codes_of(&main), vec![Code::TypeUsedAsValue], "{call}");
    }
}

#[test]
fn a_type_name_that_reaches_nothing_in_a_local_annotation_is_unknown() {
    for annotation in ["list[i32]", "Set[string]", "Missing"] {
        let main = format!(
            "fn use_it() -> void:
    let _values: {annotation} = []
"
        );
        assert_eq!(codes_of(&main), vec![Code::UnknownType], "{annotation}");
    }
}

#[test]
fn a_key_that_reaches_no_trait_is_unknown() {
    for body in [
        "let _job: fn() -> void $ Mailer = fn() -> void: pass",
        "_job := fn() -> void $ Mailer: pass",
        "$.with(Mailer=1):\n        pass",
        "_ := $.use(Mailer)",
    ] {
        let main = format!(
            "fn run() -> void:
    {body}
"
        );
        assert_eq!(codes_of(&main), vec![Code::UnknownTrait], "{body}");
    }
}

#[test]
fn a_default_method_names_its_traits_own_parameter() {
    let main = "\
trait Source[T]:
    fn next(mut self) -> T?
    fn second(mut self) -> T?:
        let _first: T? = self.next()
        self.next()

data Counter:
    n: i32

impl Source[i32] for Counter:
    fn next(mut self) -> i32?:
        self.n = self.n + 1
        .Some(self.n)

pub fn main() -> void $ Console:
    let counter: mut Counter = Counter { n: 0 }
    match counter.second():
        .Some(value) => println(value)
        .None => println(0)
";
    assert_eq!(output_of("trait-param", main), "2\n");
}
