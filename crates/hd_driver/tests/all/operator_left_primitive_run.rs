//! A primitive left operand with a non-primitive right operand (#263f,
//! `expr.op.primitive.types`, `expr.op.left-literal.join`,
//! `expr.op.left-literal.no-fit`): the operator trait of the left type is
//! searched with the right operand's type. Programs build through the
//! driver and run on V8 (`host/run.mjs`).

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

/// The standard output of a `main` program that must build and succeed.
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
        &format!("operator-left-primitive-{name}.wasm"),
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

const MONEY_LEFT: &str = "\
use std.ops.Mul

data Money:
    cents: i64

impl Mul[Money] for i64:
    type Out = Money
    fn mul(self, rhs: Money) -> Money:
        Money { cents: self * rhs.cents }
";

const MONEY_RIGHT: &str = "\
use std.ops.Mul

data Money:
    cents: i64

impl Mul[i64] for Money:
    type Out = Money
    fn mul(self, rhs: i64) -> Money:
        Money { cents: self.cents * rhs }
";

/// A converted left operand has a fixed type, so `Mul[Money] for i64`
/// is the one implementation: the result is a `Money`.
#[test]
fn converted_left_operand_selects_the_left_types_implementation() {
    let main = format!(
        "{MONEY_LEFT}
fn triple(price: Money) -> Money:
    i64(3) * price

pub fn main() -> void $ Console:
    println(triple(Money {{ cents: 5 }}).cents)
"
    );
    assert_eq!(output_of("converted", &main), "15\n");
}

/// A bare literal takes its class's default width, `usize`, because an
/// implementation alone never names one: `Mul[Money] for i64` does not
/// fit `3 * price`, and the error is reported once.
#[test]
fn bare_literal_does_not_take_the_implementations_width() {
    let main = format!(
        "{MONEY_LEFT}
fn triple(price: Money) -> Money:
    3 * price
"
    );
    assert_eq!(codes_of(&main), vec![Code::TypeMismatch]);
}

/// With an implementation for the default width, the bare literal fits
/// it, while a converted operand keeps its own type.
#[test]
fn bare_literal_fits_the_default_width_implementation() {
    let main = format!(
        "{MONEY_LEFT}
impl Mul[Money] for usize:
    type Out = Money
    fn mul(self, rhs: Money) -> Money:
        Money {{ cents: i64(self) * rhs.cents + 1000 }}

pub fn main() -> void $ Console:
    let price = Money {{ cents: 7 }}
    println((3 * price).cents)
    println((i64(3) * price).cents)
"
    );
    assert_eq!(output_of("default-width", &main), "1021\n21\n");
}

/// `expr.op.left-literal.no-fit`: with only `Mul[i64] for Money`, no width
/// of `3` lets `3 * price` check, and neither does `i64(3) * price`. Each
/// is one `type-mismatch`.
#[test]
fn no_fit_is_one_type_mismatch() {
    let literal = format!(
        "{MONEY_RIGHT}
fn triple(price: Money) -> Money:
    3 * price
"
    );
    assert_eq!(codes_of(&literal), vec![Code::TypeMismatch]);
    let converted = format!(
        "{MONEY_RIGHT}
fn triple(price: Money) -> Money:
    i64(3) * price
"
    );
    assert_eq!(codes_of(&converted), vec![Code::TypeMismatch]);
}

/// The right operand of `price * 3` is checked against `i64` in
/// `Mul[i64]` (`expr.op.left-dispatch.example`).
#[test]
fn literal_right_operand_runs() {
    let main = format!(
        "{MONEY_RIGHT}
pub fn main() -> void $ Console:
    let price = Money {{ cents: 7 }}
    println((price * 3).cents)
"
    );
    assert_eq!(output_of("right-literal", &main), "21\n");
}

/// Bitwise operators, shifts, and floats take the same path, and plain
/// primitive arithmetic stays built in.
#[test]
fn other_operators_and_floats_select_by_the_left_type() {
    let main = "\
use std.ops.Add
use std.ops.Shl

data Money:
    cents: i64

impl Add[Money] for f64:
    type Out = Money
    fn add(self, rhs: Money) -> Money:
        Money { cents: i64(self) + rhs.cents }

impl Shl[Money] for u8:
    type Out = u8
    fn shl(self, rhs: Money) -> u8:
        self + u8(1)

pub fn main() -> void $ Console:
    let price = Money { cents: 7 }
    println((2.0 + price).cents)
    println(u8(3) << price)
    println(1 + 2)
";
    assert_eq!(output_of("others", main), "9\n4\n3\n");
}
