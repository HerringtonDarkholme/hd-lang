//! A constructor-style call is a cast only between numeric types
//! (spec/lang/04-type-system.md, types.cast.nonnumeric): any other call of a
//! primitive type name is `not-callable`, never an emit failure.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn check(items: &str) -> Vec<Code> {
    let main = format!("{items}\npub fn main() -> void: pass\n");
    let mut src = MemorySources::default();
    src.insert("main.hd", &main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out: Output = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags.code.clone()
}

#[test]
fn a_nonnumeric_constructor_call_is_not_callable() {
    for body in [
        "fn f(c: char) -> string:\n    string(c)\n",
        "fn f(n: i32) -> string:\n    string(n)\n",
        "fn f(n: i32) -> bool:\n    bool(n)\n",
        "fn f(b: bool) -> i32:\n    i32(b)\n",
        "fn f(c: char) -> i32:\n    i32(c)\n",
        "fn f() -> i32:\n    i32(\"5\")\n",
    ] {
        assert_eq!(check(body), [Code::NotCallable], "{body}");
    }
}

#[test]
fn numeric_newtype_and_enum_constructors_stay_valid() {
    let src = "\
type Meters(i32)
enum Shape:
    Dot
    Circle(radius: f64)
fn f(wide: i64, ratio: f64, m: Meters) -> void:
    _ := i16(wide)
    _ := i64(ratio)
    _ := f32(wide)
    _ := i32(m)
    _ := u8(wide)
    _ := Meters(3)
    _ := Shape.Circle(2.0)
";
    assert_eq!(check(src), []);
}
