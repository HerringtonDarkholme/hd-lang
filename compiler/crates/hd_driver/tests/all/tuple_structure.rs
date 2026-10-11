//! A tuple's `Structure` at codegen (#126; codegen.md §13.6,
//! `annot.tuple.*`): the trait solver's `Structure` row answers a concrete
//! tuple with `Builtin(Structure)`, and collection lowers each method per
//! tuple type with the derivation generator. So std's tuple templates
//! (`Eq`, `PartialOrd`, `Ord`, `Hash`, `Debug`, `Display`, `Default`) and
//! a user's tuple template run. Programs build through the driver and run
//! on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::{Command, Output as Ran};

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `main` as the entry module and runs it on V8.
fn run(name: &str, main: &str) -> Ran {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tuple-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// The standard output of a program that must succeed.
fn output_of(name: &str, main: &str) -> String {
    let ran = run(name, main);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

#[test]
fn tuple_equality_through_the_eq_template() {
    let main = "\
pub fn main() -> void $ Console:
    let a: (i64, string) = (1, \"x\")
    let b: (i64, string) = (1, \"x\")
    let c: (i64, string) = (1, \"y\")
    println(a == b)
    println(a == c)
    println(a != c)
    println(((1, \"p\"), true) == ((1, \"p\"), true))
    println(((1, \"p\"), true) == ((2, \"p\"), true))
";
    assert_eq!(output_of("eq", main), "true\nfalse\ntrue\ntrue\nfalse\n");
}

#[test]
fn tuple_ordering_element_by_element() {
    let main = "\
pub fn main() -> void $ Console:
    println((1, \"b\") < (2, \"a\"))
    println((1, \"b\") < (1, \"a\"))
    println((1, \"a\") <= (1, \"a\"))
    println((3, 0) > (2, 9))
    println(debug((1, 2).cmp((1, 3))))
    println(debug((1, 3).cmp((1, 3))))
    println(debug(((0, 5), 1).cmp(((0, 4), 9))))
";
    assert_eq!(
        output_of("ord", main),
        "true\nfalse\ntrue\ntrue\nOrdering.Less\nOrdering.Equal\nOrdering.Greater\n"
    );
}

#[test]
fn tuple_hash_through_the_hash_template() {
    // A tuple key in a `Map` stops at emission (map keys other than
    // integers and strings), so the hash value is compared directly.
    let main = "\
use std.hash.hash_of

pub fn main() -> void $ Console:
    println(hash_of((1, \"a\")) == hash_of((1, \"a\")))
    println(hash_of((1, \"a\")) == hash_of((1, \"b\")))
    println(hash_of((1, \"a\")) == hash_of((2, \"a\")))
    println(hash_of(((1, 2), \"a\")) == hash_of(((1, 2), \"a\")))
";
    assert_eq!(output_of("hash", main), "true\nfalse\nfalse\ntrue\n");
}

#[test]
fn nested_tuple_debug_and_display() {
    let main = "\
pub fn main() -> void $ Console:
    let t: (i32, (string, bool), List[i32]) = (1, (\"a\", true), [2, 3])
    println(debug(t))
    let u: (i32, (string, bool), char) = (1, (\"a\", true), 'z')
    println(\"$u\")
    println(debug((7,)))
    println(\"${(7,)}\")
    println((1, (2, 3)).to_string())
";
    assert_eq!(
        output_of("text", main),
        "(1, (\"a\", true), [2, 3])\n\
         (1, (a, true), z)\n\
         (7,)\n\
         (7,)\n\
         (1, (2, 3))\n"
    );
}

/// `build` at a tuple (`annot.tuple.build`), through a user tuple
/// template's source: the source names the members by
/// position, last first, so the tuple is built from one value per member
/// whatever order the keys come in. (std's tuple `Default` builds through
/// its own source; the conformance fixture `default-tuple-thirteen-elements`
/// runs it.)
#[test]
fn tuple_build_through_a_source() {
    let main = "\
use std.function.Tuple
use std.structure.{Structure, Field, Variant, Source, Members, Key}

trait Fill:
    fn fill() -> Self

data Filler[S]:
    pub at: usize

impl[S] Source[S] for Filler[S]:
    type Error = never

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], never]:
        .Ok(choices[0])

    fn next(mut self, members: Members[S]) -> Result[Key[S], never]:
        n := members.infos.len()
        if self.at >= n:
            return .Ok(members.end())
        info := members.infos[n - 1 - self.at]
        self.at = self.at + 1
        .Ok(members.at(info.position))

    fn member[F < Fill](mut self, h: Field[S, F], previous: F?) -> Result[F, never]:
        .Ok(F::fill())

    fn missing[F](mut self, h: Field[S, F]) -> Result[F, never]:
        panic(\"the filler names every member\")

impl Fill for i32:
    fn fill() -> i32: 7

impl Fill for string:
    fn fill() -> string: \"s\"

impl[T < Tuple] Fill for T by Structure:
    fn fill() -> T:
        let s: mut Filler[T] = Filler::[T] { at: 0 }
        match T::build(s):
            .Ok(value) => value
            .Err(_) => panic(\"a filler never fails\")

fn fill_of[T < Fill]() -> T:
    T::fill()

pub fn main() -> void $ Console:
    let t: (i32, string, i32) = fill_of::[(i32, string, i32)]()
    println(debug(t))
    let n: (string, (i32, string)) = fill_of::[(string, (i32, string))]()
    println(debug(n))
";
    assert_eq!(
        output_of("build", main),
        "(7, \"s\", 7)\n(\"s\", (7, \"s\"))\n"
    );
}

#[test]
fn assert_equal_on_tuples_passes_and_reports_both_values() {
    let pass = "\
use std.testing.assert_equal

pub fn main() -> void $ Console:
    assert_equal((1, \"one\", (true, 2)), (1, \"one\", (true, 2)), reason=\"equal\")
    println(\"passed\")
";
    assert_eq!(output_of("assert-pass", pass), "passed\n");
    let fail = "\
use std.testing.assert_equal

pub fn main() -> void:
    assert_equal((1, \"one\", (true, 2)), (1, \"one\", (false, 2)), reason=\"last differs\")
";
    let ran = run("assert-fail", fail);
    assert!(!ran.status.success(), "an unequal assert_equal panics");
    let err = String::from_utf8_lossy(&ran.stderr);
    assert!(
        err.contains(
            "assertion-failed: last differs: actual (1, \"one\", (true, 2)), \
             expected (1, \"one\", (false, 2))"
        ),
        "{err}"
    );
}

/// A user's tuple template: one path with std's, its instance collected
/// per tuple type, reading the tuple's `Structure` (`annot.tuple.*`):
/// `name()` is `""`, the one variant is named `""` with `of_data` true, and
/// the members are `_0`, `_1`... positional, not embedded.
#[test]
fn user_tuple_template_reads_the_tuple_structure() {
    let main = "\
use std.function.Tuple
use std.structure.{Structure, Field, Variant, Walker, Describer}

trait Shape:
    fn shape(self) -> string
    fn members() -> string

data Lister:
    pub out: string

impl[S] Walker[S] for Lister:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + \"[\" + v.info.name + \"#\" + \"${v.info.index}\" + \" data=\" + \"${v.info.of_data}\" + \"]\"
        .Ok(())

    fn member[F < Display](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + \" \" + h.info.name + \"@\" + \"${h.info.position}\" + \"=\" + \"$value\"
        if h.info.positional:
            self.out = self.out + \"p\"
        if h.info.embedded:
            self.out = self.out + \"e\"
        .Ok(())

impl[S] Describer[S] for Lister:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + \"<\" + v.info.name + \">\"
        .Ok(())

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        self.out = self.out + \" \" + h.info.name
        .Ok(())

impl[T < Tuple] Shape for T by Structure:
    fn shape(self) -> string:
        let w: mut Lister = Lister { out: \"name=\" + T::name() + \" \" }
        _ := Structure::walk(self, w)
        w.out

    fn members() -> string:
        let d: mut Lister = Lister { out: \"\" }
        _ := T::describe(d)
        d.out

fn members_of[T < Shape]() -> string:
    T::members()

pub fn main() -> void $ Console:
    println((1, \"a\").shape())
    println((true, 2, 'c').shape())
    println((1, \"a\").shape())
    println(members_of::[(i32, string, bool)]())
";
    assert_eq!(
        output_of("user", main),
        "name= [#0 data=true] _0@0=1p _1@1=ap\n\
         name= [#0 data=true] _0@0=truep _1@1=2p _2@2=cp\n\
         name= [#0 data=true] _0@0=1p _1@1=ap\n\
         <> _0 _1 _2\n"
    );
}
