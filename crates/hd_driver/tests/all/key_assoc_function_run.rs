//! A trait with an associated function (no `self`) as a requirement key
//! and as a `dyn` type (spec 11 req.key.any-trait-members; spec 09
//! trait.dyn.member.assoc-function). The associated function has no
//! vtable slot, so a module that only uses the other members validates
//! and runs; calling it through the provider or the `dyn` value is
//! `dyn-member-unavailable`, once, at the call.

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

/// The standard output of a program that must build, validate as Wasm
/// (V8 compiles the whole module) and succeed.
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
        &format!("key-assoc-function-{name}.wasm"),
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

const FACTORY: &str = "\
trait Factory:
    fn create() -> Self
    fn name(self) -> string

data D:
    n: i32

impl Factory for D:
    fn create() -> D: D { n: 0 }
    fn name(self) -> string: \"d\"
";

#[test]
fn a_key_trait_with_an_associated_function_provides_its_other_members() {
    let main = format!(
        "{FACTORY}
fn build() -> string $ Factory:
    $.use(Factory).name()

pub fn main() -> void $ Console:
    $.with(Factory=D {{ n: 1 }}):
        println(build())
"
    );
    assert_eq!(output_of("key", &main), "d\n");
}

#[test]
fn a_dyn_value_of_a_trait_with_an_associated_function_calls_its_other_members() {
    let main = format!(
        "{FACTORY}
fn show(f: dyn Factory) -> string:
    f.name()

pub fn main() -> void $ Console:
    println(show(D {{ n: 1 }}))
"
    );
    assert_eq!(output_of("dyn", &main), "d\n");
}

#[test]
fn a_key_trait_of_associated_functions_only_has_an_empty_vtable() {
    let main = "\
trait Maker:
    fn make() -> i32

data D:
    n: i32

impl Maker for D:
    fn make() -> i32: 7

fn only() -> i32 $ Maker:
    1

pub fn main() -> void $ Console:
    $.with(Maker=D { n: 2 }):
        println(only())
";
    assert_eq!(output_of("only", main), "1\n");
}

#[test]
fn a_key_trait_with_an_associated_type_and_an_associated_function_runs() {
    let main = "\
trait Maker:
    fn make() -> i32

trait Named < Maker:
    type Label
    fn label(self) -> Self::Label

data D:
    n: i32

impl Maker for D:
    fn make() -> i32: 7

impl Named for D:
    type Label = string
    fn label(self) -> string: \"lbl\"

fn show() -> string $ Named[Label = string]:
    $.use(Named[Label = string]).label()

pub fn main() -> void $ Console:
    $.with(Named[Label = string]=D { n: 1 }):
        println(show())
";
    assert_eq!(output_of("assoc-type", main), "lbl\n");
}

#[test]
fn an_associated_function_through_a_key_is_unavailable_once_at_the_call() {
    let main = format!(
        "{FACTORY}
fn build() -> string $ Factory:
    $.use(Factory).create().name()

pub fn main() -> void:
    pass
"
    );
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}

#[test]
fn an_associated_function_through_a_dyn_value_is_unavailable_once_at_the_call() {
    let main = format!(
        "{FACTORY}
fn build(f: dyn Factory) -> string:
    f.create().name()

pub fn main() -> void:
    pass
"
    );
    assert_eq!(codes_of(&main), vec![Code::DynMemberUnavailable]);
}
