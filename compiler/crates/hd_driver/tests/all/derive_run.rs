//! Derived implementations run (#119; checking-and-tir.md §4.13.9,
//! codegen.md §12.3): each derivation's template methods, checked at the
//! opt-in, become the derived implementation's bodies, and the
//! derivation's `Structure` (`walk`, `describe`, `build`, `name`,
//! `facts`) is generated as ordinary bodies. Programs build through the
//! driver and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("derive-run-{name}.wasm"));
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

fn output_of(name: &str, files: &[(&str, &str)]) -> String {
    run(&build_with(&MemoryStore::default(), files), name)
}

#[test]
fn std_eq_and_debug_derive_on_data_and_enums() {
    let main = "\
@derive(Eq, Debug)
data Point:
    x: i32
    label: string

@derive(Eq, Debug)
enum Shape:
    Dot
    Line(from: Point, to: Point)
    Pair(i32, i32)

pub fn main() -> void $ Console:
    a := Point { x: 1, label: \"a\" }
    println(a == Point { x: 1, label: \"a\" })
    println(a == Point { x: 1, label: \"b\" })
    println(Shape.Pair(1, 2) == Shape.Pair(1, 2))
    println(Shape.Pair(1, 2) == Shape.Pair(1, 3))
    println(Shape.Dot == Shape.Line(a, a))
    println(debug(a))
    println(debug(Shape.Dot))
    println(debug(Shape.Line(a, Point { x: 2, label: \"b\" })))
    println(debug(Shape.Pair(1, 2)))
";
    assert_eq!(
        output_of("std", &[("main.hd", main)]),
        "true\nfalse\ntrue\nfalse\nfalse\n\
         Point { x: 1, label: \"a\" }\n\
         Shape.Dot\n\
         Shape.Line(from=Point { x: 1, label: \"a\" }, to=Point { x: 2, label: \"b\" })\n\
         Shape.Pair(1, 2)\n"
    );
}

/// A user template over `walk`, `describe` and `name`, derived for an
/// enum: the walk visits only the value's variant, `describe` every
/// variant, each with its members' handles.
const LISTING: &str = "\
use std.structure.{Structure, Field, Variant, Walker, Describer}

trait Listing:
    fn listing(self) -> string
    fn shape() -> string

data Lister:
    pub out: string

impl[S] Walker[S] for Lister:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + v.info.name + \"#\" + \"${v.info.index}\" + \"(\"
        .Ok(())

    fn member[F < Display](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + h.info.name + \"=\" + \"$value\" + \";\"
        .Ok(())

data Shaper:
    pub out: string

impl[S] Describer[S] for Shaper:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + \"|\" + v.info.name
        .Ok(())

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        self.out = self.out + \" \" + h.info.name
        .Ok(())

impl[T] Listing for T by Structure:
    fn listing(self) -> string:
        let mut w = Lister { out: T::name() + \":\" }
        _ := Structure::walk(self, w)
        w.out + \")\"

    fn shape() -> string:
        let mut d = Shaper { out: Structure::name() }
        _ := T::describe(d)
        d.out
";

#[test]
fn a_user_template_derives_for_an_enum() {
    let main = "\
@derive(Listing)
enum Reply:
    Sent(id: i64)
    Raw(string)
    Busy

pub fn main() -> void $ Console:
    println(Reply.Sent(7).listing())
    println(Reply.Raw(\"hi\").listing())
    println(Reply.Busy.listing())
    println(Reply::shape())
";
    assert_eq!(
        output_of("user", &[("main.hd", &format!("{LISTING}\n{main}"))]),
        "Reply:Sent#0(id=7;)\nReply:Raw#1(_0=hi;)\nReply:Busy#2()\n\
         Reply|Sent id|Raw _0|Busy\n"
    );
}

#[test]
fn a_generic_type_derives_under_its_parameter_bound() {
    let main = "\
@derive(Eq, Debug)
data Box[T]:
    value: T
    count: i32

impl[T < Display] Listing for Box[T] by Structure

pub fn main() -> void $ Console:
    println(Box { value: \"a\", count: 1 } == Box { value: \"a\", count: 1 })
    println(Box { value: 2, count: 1 } == Box { value: 3, count: 1 })
    println(debug(Box { value: Box { value: true, count: 0 }, count: 2 }))
    println(Box { value: \"four\", count: 3 }.listing())
";
    assert_eq!(
        output_of("generic", &[("main.hd", &format!("{LISTING}\n{main}"))]),
        "true\nfalse\nBox { value: Box { value: true, count: 0 }, count: 2 }\n\
         Box:Box#0(value=four;count=3;)\n"
    );
}

#[test]
fn build_reads_each_member_from_a_source() {
    let main = "\
use std.structure.{Structure, Field, Variant, Members, Key, Source}

trait Fill:
    fn fill(seed: i64) -> Self

trait FromNum:
    fn from_num(n: i64) -> Self

impl FromNum for i64:
    fn from_num(n: i64) -> i64: n

impl FromNum for string:
    fn from_num(n: i64) -> string: \"#$n\"

# Chooses the last variant, names every member in order, and reads the
# next number for each.
data Counter:
    next: i64
    at: usize

impl[S] Source[S] for Counter:
    type Error = string

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], string]:
        .Ok(choices[choices.len() - 1])

    fn next(mut self, members: Members[S]) -> Result[Key[S], string]:
        if self.at >= members.infos.len():
            return .Ok(members.end())
        info := members.infos[self.at]
        self.at = self.at + 1
        .Ok(members.at(info.position))

    fn member[F < FromNum](mut self, h: Field[S, F], previous: F?) -> Result[F, string]:
        self.next = self.next + 1
        .Ok(F::from_num(self.next))

    fn missing[F](mut self, h: Field[S, F]) -> Result[F, string]:
        .Err(\"missing \" + h.info.name)

impl[T] Fill for T by Structure:
    fn fill(seed: i64) -> T:
        let source: mut Counter = Counter { next: seed, at: 0 }
        match T::build(source):
            .Ok(value) => value
            .Err(message) => panic(message)

@derive(Fill, Debug)
data Order:
    id: i64
    label: string

@derive(Fill, Debug)
enum Event:
    Start
    Move(dx: i64, dy: i64)

@derive(Debug)
data Box[T]:
    value: T

impl[T < FromNum] Fill for Box[T] by Structure

pub fn main() -> void $ Console:
    println(debug(Order::fill(10)))
    println(debug(Event::fill(0)))
    println(debug(Box::[string]::fill(4)))
";
    assert_eq!(
        output_of("build", &[("main.hd", main)]),
        "Order { id: 11, label: \"#12\" }\nEvent.Move(dx=1, dy=2)\nBox { value: \"#5\" }\n"
    );
}

/// The template and its walker in one module, the deriving type in
/// another: an edit of the walker's helper changes the derived output,
/// and an edit of the template method rechecks the deriving module
/// (checking-and-tir.md §4.13.9).
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

pub data Shower:
    pub out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Display](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + h.info.name + sep() + \"$value \"
        .Ok(())

impl[T] Show for T by Structure:
    fn show(self) -> string:
        let mut w = Shower {{ out: \"{prefix}\" }}
        _ := Structure::walk(self, w)
        w.out
"
        )
    };
    let main = "\
use pkg.show.Show

@derive(Show)
data Point:
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
