//! Ownership marks parse in every program (13-ownership.md, Mark Syntax).
//! Headers check them (Modes In Declarations), and bodies check modes
//! (Modes In Bodies); what later sections check stays `unsupported`
//! (issue #163), and a program without marks stays clean.

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

/// Bodies check modes (Modes In Bodies): lending, owned results, fresh
/// values, `let own`, and owned and borrowed locals.
#[test]
fn body_marks_check() {
    let forms = [
        "fn size(f: ^File) -> i32: f.fd\n",
        "fn open() -> own File: File { fd: 1 }\n",
        "fn size(f: ^File) -> i32: 0\n\nfn call(f: File) -> i32: size(f)\n",
        "fn make() -> i32:\n    let own f = File { fd: 1 }\n    f.fd\n",
        "fn make() -> i32:\n    let f: own File = File { fd: 1 }\n    f.fd\n",
        "fn make(f: File) -> i32:\n    let v: ^File = f\n    v.fd\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, Vec::<Code>::new(), "for:\n{form}");
    }
}

/// What later sections check stays `unsupported` (the note of
/// 13-ownership.md): a value of an `own`-bounded type parameter, owned
/// type arguments, captures, iteration and patterns over moded values,
/// and marks in a supertrait list.
#[test]
fn later_body_forms_are_unsupported() {
    let forms = [
        "fn keep[T < own Any](x: T) -> T: x\n",
        "fn make() -> void:\n    fn inner[T < own Any](x: T) -> T: x\n    pass\n",
        "trait Keeper < own Named:\n    fn keep(self) -> void\n",
        "fn id[T](x: T) -> T: x\n\nfn make(f: own File) -> i32:\n    g := id(f)\n    g.fd\n",
        "fn size(f: ^File) -> i32: f.fd\n\nfn make(f: own File) -> i32:\n    g := fn() -> i32: size(f)\n    g()\n",
        "fn walk(xs: ^List[own File]) -> void:\n    for x in xs:\n        pass\n",
        "fn pick(f: own File?) -> void:\n    match f:\n        .Some(_) => pass\n        .None => pass\n",
        "fn plain(f: ^File) -> File: f\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, vec![Code::Unsupported], "for:\n{form}");
    }
}

/// The prototype's limits (13-ownership.md, Prototype Limits) and the
/// plain-only providers and boundaries reject with their own codes, and
/// stand for the body's other ownership gaps.
#[test]
fn prototype_limits_reject() {
    let limit = Code::NotInOwnershipPrototype;
    let forms = [
        ("let own TOP = File { fd: 1 }\n", limit),
        (
            "fn open() -> own File: File { fd: 1 }\n\nTOP := open()\n",
            limit,
        ),
        ("TOP := File { fd: 1 }\n\nlet VIEW: ^File = TOP\n", limit),
        (
            "TOP := File { fd: 1 }\n\nfn size(f: ^File) -> i32: f.fd\n\nfn read() -> i32: size(TOP)\n",
            limit,
        ),
        (
            "fn handle!(f: own File) -> void: pass\n\nfn later(f: own File) -> void:\n    _ := handle(f)\n",
            limit,
        ),
        (
            "impl Named for File:\n    fn name(self) -> string: \"file\"\n\nfn named(f: own File) -> void:\n    $.with(Named = f):\n        pass\n",
            Code::ProviderNotPlain,
        ),
    ];
    for (form, code) in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, vec![code], "for:\n{form}");
    }
}

#[test]
fn the_word_own_without_a_mark_stays_clean() {
    let program = format!(
        "{PRELUDE}\nfn twice(own: i32) -> i32:\n    let mine = own\n    own + mine\n\nfn xor(a: i32, b: i32) -> i32: a ^ b\n"
    );
    assert_eq!(codes(&program), Vec::<Code>::new());
}
