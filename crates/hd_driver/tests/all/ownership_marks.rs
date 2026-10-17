//! Ownership marks parse in every program (13-ownership.md, Mark Syntax).
//! Headers check them (Modes In Declarations); a body that holds a moded
//! value is `unsupported` until a later section checks bodies (issue #163),
//! and a program without marks stays clean.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn codes(main: &str) -> Vec<Code> {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    out.diags.code.clone()
}

const PRELUDE: &str = "\
data File:
    fd: i32

trait Named:
    fn name(self) -> string

pub fn main() -> void:
    pass
";

#[test]
fn header_marks_check() {
    let forms = [
        "fn archive(f: own File) -> void: pass\n",
        "fn size(f: ^File) -> i32: 0\n",
        "fn fill(f: ^mut File) -> void: pass\n",
        "fn view(a: ^File) -> ^{a} File: panic(\"stub\")\n",
        "fn job(run: own fn() -> void) -> void: pass\n",
        "data Parser[^doc]:\n    at: i32\n",
        "data Parser[^doc]:\n    at: i32\n\nfn parse(p: Parser[^]) -> void: pass\n",
        "fn keep[T < own Any](x: i32) -> i32: x\n",
        "fn first[^c](x: ^List[^{c} File]) -> ^{c} File: panic(\"stub\")\n",
        "impl File:\n    fn close(own self) -> void: pass\n",
        "impl File:\n    fn peek(^self) -> i32: 0\n",
        "impl File:\n    fn reset(^mut self) -> void: pass\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, Vec::<Code>::new(), "for:\n{form}");
    }
}

/// Bodies check marks from a later section on (the note of 13-ownership.md): a
/// body mark, a read of a moded value, or a value of an `own`-bounded type
/// parameter is `unsupported`, with nothing else.
#[test]
fn body_marks_are_unsupported() {
    let forms = [
        "fn size(f: ^File) -> i32: f.fd\n",
        "fn open() -> own File: File { fd: 1 }\n",
        "fn size(f: ^File) -> i32: 0\n\nfn call(f: File) -> i32: size(f)\n",
        "fn keep[T < own Any](x: T) -> T: x\n",
        "fn make() -> void:\n    let own f = File { fd: 1 }\n",
        "fn make() -> void:\n    let f: own File = File { fd: 1 }\n",
        "fn make(f: File) -> void:\n    let v: ^File = f\n",
        "fn make() -> void:\n    fn inner[T < own Any](x: T) -> T: x\n    pass\n",
        "let own TOP = File { fd: 1 }\n",
        "trait Keeper < own Named:\n    fn keep(self) -> void\n",
        "@derive(OwnOnly)\ndata Cell[T]:\n    value: T\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, vec![Code::Unsupported], "for:\n{form}");
    }
}

#[test]
fn the_word_own_without_a_mark_stays_clean() {
    let program = format!(
        "{PRELUDE}\nfn twice(own: i32) -> i32:\n    let mine = own\n    own + mine\n\nfn xor(a: i32, b: i32) -> i32: a ^ b\n"
    );
    assert_eq!(codes(&program), Vec::<Code>::new());
}
