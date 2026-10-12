//! `@derive` on a newtype (spec 09 `trait.derive.newtype.*`): each method of
//! the derived implementation applies the base type's method to the wrapped
//! values and wraps the result back, for every position the rules allow
//! (receiver, `Self`, `Self?`, `Result[Self, E]`, `List[Self]`). Any other
//! position is `newtype-derivation-self`. The programs build through the
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
        &format!("newtype-derive-{name}.wasm"),
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

#[test]
fn eq_hash_and_ordering_follow_the_base_type() {
    let main = "\
@derive(Eq, Hash)
type Mile(i32)

@derive(Eq, PartialOrd, Ord)
type Rank(i32)

@derive(Eq, Hash)
type Name(string)

pub fn main() -> void $ Console:
    println(Mile(3) == Mile(3))
    println(Mile(3) == Mile(4))
    println(Rank(1) < Rank(2))
    println(Rank(2) <= Rank(1))
    println(Rank(5).cmp(Rank(5)) == .Equal)
    println(Name(\"a\") == Name(\"a\"))
    distances := {Mile(5): \"short\", Mile(50): \"far\"}
    println(debug(distances.get(Mile(50))))
    println(debug(distances.get(Mile(7))))
";
    assert_eq!(
        output_of("eq-hash-ord", main),
        "true\nfalse\ntrue\nfalse\ntrue\ntrue\nOption.Some(\"far\")\nOption.None\n"
    );
}

#[test]
fn debug_renders_the_base_value() {
    let main = "\
@derive(Debug)
type Meters(i64)

pub fn main() -> void $ Console:
    let distance: i64 = 3
    println(debug(Meters(distance)) == debug(distance))
";
    assert_eq!(output_of("debug", main), "true\n");
}

#[test]
fn default_wraps_the_base_default() {
    let main = "\
use std.ops.{Default, default}

@derive(Default, Eq, Debug)
type Count(i32)

pub fn main() -> void $ Console:
    zero := Count::default()
    println(zero == Count(0))
";
    assert_eq!(output_of("default", main), "true\n");
}

#[test]
fn serialize_and_deserialize_wrap_the_base_value() {
    let main = "\
use std.json.{encode, decode}
use std.serde.{Serialize, Deserialize}

@derive(Serialize, Deserialize, Eq)
type OrderId(u64)

pub fn main() -> void $ Console:
    println(encode(OrderId(42)))
    match decode::[OrderId](\"7\"):
        .Ok(found) => println(found == OrderId(7))
        .Err(_) => println(\"error\")
    match decode::[OrderId](\"-1\"):
        .Ok(_) => println(\"ok\")
        .Err(_) => println(\"error\")
";
    assert_eq!(output_of("serde", main), "42\ntrue\nerror\n");
}

/// A user trait that uses `Self` in each position a newtype forwards.
const STEPS: &str = "\
trait Step:
    fn next(self) -> Self
    fn plus(self, other: Self) -> Self
    fn before(self, limit: Self) -> Self?
    fn guard(self, limit: Self) -> Result[Self, string]
    fn spread(self) -> List[Self]
    fn total(self, rest: List[Self]) -> Self
    fn or_else(self, other: Self?) -> Self
    fn zero() -> Self

impl Step for i32:
    fn next(self) -> i32: self + 1
    fn plus(self, other: i32) -> i32: self + other
    fn before(self, limit: i32) -> i32?:
        if self < limit: .Some(self) else: .None
    fn guard(self, limit: i32) -> Result[i32, string]:
        if self < limit: .Ok(self) else: .Err(\"too big\")
    fn spread(self) -> List[i32]: [self, self + 1, self + 2]
    fn total(self, rest: List[i32]) -> i32:
        let sum = self
        for r in rest:
            sum = sum + r
        sum
    fn or_else(self, other: i32?) -> i32:
        match other:
            .Some(v) => v
            .None => self
    fn zero() -> i32: 0

@derive(Step)
type Level(i32)
";

#[test]
fn self_positions_unwrap_and_wrap() {
    let main = format!(
        "{STEPS}
fn show(level: Level) -> i32:
    i32(level)

pub fn main() -> void $ Console:
    println(show(Level(1).next()))
    println(show(Level(1).plus(Level(5))))
    println(debug(Level(1).before(Level(3)).map(show)))
    println(debug(Level(4).before(Level(3)).map(show)))
    println(debug(Level(1).guard(Level(3)).map(show)))
    println(Level(4).guard(Level(3)).is_err())
    println(debug(Level(1).spread().map(show)))
    println(show(Level(10).total([Level(1), Level(2)])))
    println(show(Level(10).or_else(.Some(Level(3)))))
    println(show(Level(10).or_else(.None)))
    println(show(Level::zero()))
"
    );
    assert_eq!(
        output_of("positions", &main),
        "2\n6\nOption.Some(1)\nOption.None\nResult.Ok(1)\ntrue\n[1, 2, 3]\n13\n3\n10\n0\n"
    );
}

#[test]
fn a_position_a_newtype_cannot_forward_is_an_error_at_the_derive() {
    let pairing = "\
trait Pairing:
    fn pair(self) -> (Self, i32)

impl Pairing for string:
    fn pair(self) -> (string, i32): (self, 0)

@derive(Pairing)
type Tag(string)
";
    assert_eq!(codes_of(pairing), vec![Code::NewtypeDerivationSelf]);
    // `Map[Self, i32]` needs `Self < Eq & Hash` (types.map-key.declared-bound).
    let keyed = "\
trait Index < Eq & Hash:
    fn by(self) -> Map[Self, i32]

impl Index for string:
    fn by(self) -> Map[string, i32]: {self: 0}

@derive(Eq, Hash, Index)
type Tag(string)
";
    assert_eq!(codes_of(keyed), vec![Code::NewtypeDerivationSelf]);
}
