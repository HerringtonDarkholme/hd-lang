//! Borrow marks parse in every program (13-ownership.md, Mark Syntax).
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
        "fn size(f: ^File) -> i32: 0\n",
        "fn fill(f: ^mut File) -> void: pass\n",
        "fn job(run: ^fn() -> void) -> void: pass\n",
        "fn each(xs: List[File], visit: fn(^File) -> void) -> void: pass\n",
        "data Hook:\n    on_open: fn(^File) -> void\n",
        "impl File:\n    fn peek(^self) -> i32: 0\n",
        "impl File:\n    fn reset(^mut self) -> void: pass\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, Vec::<Code>::new(), "for:\n{form}");
    }
}

/// A mark goes only on a parameter, a receiver, a binding, or a function
/// type's parameter (`own.header.placement`); elsewhere it is
/// `misplaced-borrow`, and the marked type reports nothing more.
#[test]
fn misplaced_marks_reject() {
    let forms = [
        "fn view(f: ^File) -> ^File: f\n",
        "data Reader:\n    source: ^File\n",
        "enum Found:\n    Some(f: ^File)\n",
        "fn walk(xs: List[^File]) -> void: pass\n",
        "fn pick(f: ^File?) -> void: pass\n",
        "fn pair(p: (^File, i32)) -> void: pass\n",
        "fn hold(make: fn() -> ^File) -> void: pass\n",
        "type View = ^File\n",
        "fn grab(f: File) -> void:\n    let _xs: List[^File] = []\n",
        "fn make(f: File) -> void:\n    _g := fn(x: ^File) -> ^File: x\n",
        "fn id[T](x: T) -> T: x\n\nfn grab(f: File) -> void:\n    _g := id::[^File](f)\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, vec![Code::MisplacedBorrow], "for:\n{form}");
    }
}

/// Bodies check modes (Modes In Bodies): lending plain values and
/// borrowed locals.
#[test]
fn body_marks_check() {
    let forms = [
        "fn size(f: ^File) -> i32: f.fd\n",
        "fn size(f: ^File) -> i32: 0\n\nfn call(f: File) -> i32: size(f)\n",
        "fn make(f: File) -> i32:\n    let v: ^File = f\n    v.fd\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, Vec::<Code>::new(), "for:\n{form}");
    }
}

/// What later sections check stays `unsupported` (the note of
/// 13-ownership.md): borrowed type arguments that a generic call solves,
/// captures, iteration and patterns over borrowed values, and a borrow
/// in a plain position.
#[test]
fn later_body_forms_are_unsupported() {
    let forms = [
        "fn id[T](x: T) -> T: x\n\nfn make(f: ^File) -> i32:\n    g := id(f)\n    g.fd\n",
        "fn size(f: ^File) -> i32: f.fd\n\nfn make(f: ^File) -> i32:\n    g := fn() -> i32: size(f)\n    g()\n",
        "fn walk(xs: ^List[File]) -> void:\n    for x in xs:\n        pass\n",
        "fn plain(f: ^File) -> File: f\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, vec![Code::Unsupported], "for:\n{form}");
    }
}

/// The prototype's limits (13-ownership.md, Prototype Limits) and the
/// plain-only providers reject with their own codes, and stand for the
/// body's other ownership gaps.
#[test]
fn prototype_limits_reject() {
    let limit = Code::NotInOwnershipPrototype;
    let forms = [
        ("TOP := File { fd: 1 }\n\nlet VIEW: ^File = TOP\n", limit),
        (
            "TOP := File { fd: 1 }\n\nfn size(f: ^File) -> i32: f.fd\n\nfn read() -> i32: size(TOP)\n",
            limit,
        ),
        (
            "impl Named for File:\n    fn name(self) -> string: \"file\"\n\nfn named(f: ^File) -> void:\n    $.with(Named = f):\n        pass\n",
            Code::ProviderNotPlain,
        ),
    ];
    for (form, code) in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert_eq!(got, vec![code], "for:\n{form}");
    }
}

/// `own` is an ordinary name: no mark writes it.
#[test]
fn the_word_own_stays_a_name() {
    let program = format!(
        "{PRELUDE}\nfn twice(own: i32) -> i32:\n    let mine = own\n    own + mine\n\nfn xor(a: i32, b: i32) -> i32: a ^ b\n"
    );
    assert_eq!(codes(&program), Vec::<Code>::new());
}
