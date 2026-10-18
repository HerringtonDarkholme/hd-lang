//! `^` is only the bitwise XOR operator and `own` is an ordinary name:
//! neither writes a mark on a type, a binding or a receiver (issue #163).

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

pub fn main() -> void:
    pass
";

#[test]
fn the_word_own_stays_a_name_and_caret_is_xor() {
    let program = format!(
        "{PRELUDE}\nfn twice(own: i32) -> i32:\n    let mine = own\n    own + mine\n\nfn xor(a: i32, b: i32) -> i32:\n    let c = a ^ b\n    c ^= a\n    c\n"
    );
    assert_eq!(codes(&program), Vec::<Code>::new());
}

#[test]
fn a_caret_on_a_type_or_a_receiver_is_a_syntax_error() {
    let forms = [
        "fn size(f: ^File) -> i32: 0\n",
        "fn fill(f: ^mut File) -> void: pass\n",
        "fn make(f: File) -> void:\n    let v: ^File = f\n",
        "impl File:\n    fn peek(^self) -> i32: 0\n",
        "impl File:\n    fn reset(^mut self) -> void: pass\n",
    ];
    for form in forms {
        let program = format!("{PRELUDE}\n{form}");
        let got = codes(&program);
        assert!(
            !got.is_empty() && got.iter().all(|c| *c == Code::SyntaxError),
            "for:\n{form}\ngot {got:?}"
        );
    }
}
