//! Tuple templates hold only for tuples whose members meet the walker's
//! bound (spec 14 `annot.template.tuple.implements`, `.unmet`,
//! `annot.walker.obligation`, `.rest`): a tuple of hashable members is a
//! map key, a tuple with a float, a function or an unhashable data member
//! is not, and the use reports one `unsatisfied-trait-bound`. A program
//! that uses such a tuple stops at checking, never in collection. Valid
//! programs build through the driver and run on V8 (`host/run.mjs`), as
//! the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(files: &[(&str, &str)], goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
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

fn program() -> Goal {
    Goal::Program {
        entry: "main".into(),
    }
}

/// The diagnostic codes of an analysis, unused bindings left out.
fn codes(files: &[(&str, &str)]) -> Vec<Code> {
    let out = built(files, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::UnusedLocalBinding)
        .collect()
}

fn main_codes(main: &str) -> Vec<Code> {
    codes(&[("main.hd", main)])
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(&[("main.hd", main)], &program());
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("tuple-template-{name}.wasm"),
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

const BOUND: &[Code] = &[Code::UnsatisfiedTraitBound];

#[test]
fn tuples_of_hashable_members_are_map_keys_and_display() {
    let src = "\
@derive(Eq, Hash)
struct Id:
    value: string

fn count[K < Eq & Hash](k: K) -> usize:
    let mut m: Map[(K, i32), string] = {}
    m[(k, +1)] = \"a\"
    m[(k, +1)] = \"b\"
    m.len()

pub fn main() -> void $ Console:
    let mut m: Map[(i32, (string, bool)), string] = {}
    m[(+1, (\"a\", true))] = \"x\"
    m[(+1, (\"a\", true))] = \"y\"
    m[(+2, (\"a\", true))] = \"z\"
    println(m.len())
    let mut ids: Map[(Id, char), i32] = {}
    ids[(Id { value: \"k\" }, 'c')] = +3
    println(ids[(Id { value: \"k\" }, 'c')])
    println(count(\"q\"))
    let t = (+1, \"two\", (true, 2.5))
    println(\"$t\")
";
    assert_eq!(output_of("valid", src), "2\n3\n1\n(1, two, (true, 2.5))\n");
}

#[test]
fn a_float_member_is_no_map_key() {
    assert_eq!(
        main_codes("let points: Map[(i32, f64), string] = {}\n"),
        BOUND
    );
}

#[test]
fn a_function_member_is_no_map_key() {
    assert_eq!(
        main_codes("let m: Map[(i32, fn(i32) -> i32), string] = {}\n"),
        BOUND
    );
}

#[test]
fn an_unhashable_data_member_is_no_map_key() {
    let src = "\
struct Secret:
    value: i32

let m: Map[(Secret, i32), string] = {}
";
    assert_eq!(main_codes(src), BOUND);
}

#[test]
fn a_field_and_a_derived_member_ask_the_same_of_a_tuple() {
    // A field's type is checked with the headers; a derivation checks
    // its members' types against the walker.
    assert_eq!(
        main_codes("struct Holder:\n    pub m: Map[(i32, f64), i32]\n"),
        BOUND
    );
    assert_eq!(
        main_codes("@derive(Eq, Hash)\nstruct Keyed:\n    pair: (i32, f64)\n"),
        [Code::DeriveFieldMissingTrait]
    );
}

#[test]
fn a_nested_tuple_meets_the_bound_through_its_own_members() {
    assert_eq!(
        main_codes("let m: Map[(i32, (string, f64)), string] = {}\n"),
        BOUND
    );
    assert_eq!(
        main_codes("let m: Map[(i32, (string, (bool, char))), string] = {}\n"),
        Vec::<Code>::new()
    );
}

#[test]
fn a_generic_member_meets_the_bound_through_the_environment() {
    let with = "\
fn keyed[T < Eq & Hash](t: T) -> usize:
    let m: Map[(T, i32), string] = {}
    m.len()
";
    assert_eq!(main_codes(with), Vec::<Code>::new());
    let without = "\
fn keyed[T](t: T) -> usize:
    let m: Map[(T, i32), string] = {}
    m.len()
";
    assert_eq!(main_codes(without), BOUND);
}

#[test]
fn display_and_default_need_every_member() {
    let display = "\
struct Secret:
    value: i32

fn show(pair: (i32, Secret)) -> string:
    \"$pair\"
";
    assert_eq!(main_codes(display), BOUND);
    let default = "\
use std.ops.Default

struct Secret:
    value: i32

fn fresh[T < Default]() -> T:
    T::default()

fn start() -> (i32, Secret):
    fresh()
";
    assert_eq!(main_codes(default), BOUND);
}

#[test]
fn a_rest_element_meets_the_bound_by_its_items_or_its_list() {
    // `Hash`'s walker implements `rest`, so the items need `Hash`.
    assert_eq!(
        main_codes("fn f(m: Map[(i32, List[i32]...), i32]) -> usize:\n    m.len()\n"),
        Vec::<Code>::new()
    );
    assert_eq!(
        main_codes("fn f(m: Map[(i32, List[f64]...), i32]) -> usize:\n    m.len()\n"),
        BOUND
    );
    // `Default`'s source has no `rest`: the rest is a `List`, which is
    // `Default` for any item.
    let default = "\
use std.ops.Default

struct Secret:
    value: i32

fn fresh[T < Default]() -> T:
    T::default()

fn hidden() -> (i32, List[Secret]...):
    fresh()

fn fixed() -> (Secret, List[i32]...):
    fresh()
";
    assert_eq!(main_codes(default), BOUND);
}

#[test]
fn running_a_program_with_an_unmet_tuple_stops_at_checking() {
    let src = "\
pub fn main() -> void $ Console:
    let points: Map[(i32, f64), string] = {}
    println(points.len())
";
    let out = built(&[("main.hd", src)], &program());
    let codes: Vec<Code> = out
        .diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect();
    assert_eq!(codes, BOUND, "{}", out.render());
    assert!(out.wasm.is_none());
    assert!(
        !out.report.stages.contains_key(&hd_base::Stage::Collect),
        "collection never ran: {:?}",
        out.report.reasons
    );
}

/// A user library's tuple template, in its own module: its walker's
/// `member` needs `Show`, and the template gets the walker from a helper,
/// so only checking its body names the walker.
const SHOW: &str = "\
use std.function.Tuple
use std.structure.{Structure, Field, Variant, Walker}

pub trait Show:
    fn show(self) -> string

impl Show for i64:
    fn show(self) -> string:
        \"$self\"

struct Shower:
    out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Show](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + value.show() + \";\"
        .Ok(())

fn shower() -> Shower:
    Shower { out: \"\" }

impl[T < Tuple] Show for T by Structure:
    fn show(self) -> string:
        let mut w = shower()
        _ := Structure::walk(self, &mut w)
        w.out
";

#[test]
fn a_user_tuple_template_asks_its_walkers_bound() {
    let ok = "\
use pkg.show.{Show}

fn both(pair: (i64, (i64, i64))) -> string:
    pair.show()
";
    assert_eq!(
        codes(&[("show.hd", SHOW), ("main.hd", ok)]),
        Vec::<Code>::new()
    );
    let bad = "\
use pkg.show.{Show}

fn needs[T < Show](t: T) -> string:
    t.show()

fn both(pair: (i64, string)) -> string:
    needs(pair)
";
    assert_eq!(codes(&[("show.hd", SHOW), ("main.hd", bad)]), BOUND);
}
