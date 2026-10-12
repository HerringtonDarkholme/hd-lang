//! A written type whose argument count is wrong, for a declaration of the
//! module being lowered (types.generic.too-long,
//! types.generic.default.written-missing): one error at the type, which
//! lowers to poison, so a body that would otherwise mismatch it adds none.

use hd_diag::Code;

use crate::type_default_written_run::codes_of;

const HEAD: &str = "\
data Pair[K, V]:
    key: K
    value: V

enum Choice[T]:
    Some(T)
    Empty

type Wrap[T](T)

";

fn assert_once(code: Code, bads: &[&str]) {
    for bad in bads {
        assert_eq!(codes_of(&format!("{HEAD}{bad}\n")), vec![code], "{bad}");
    }
}

#[test]
fn own_module_types_written_too_long_report_once() {
    assert_once(
        Code::ArgumentCount,
        &[
            "fn h() -> Pair[i32, i32, i32]: 5",
            "fn h(p: Pair[i32, i32, i32]) -> i32: p",
            "data Holder:\n    p: Pair[i32, i32, i32]\n\nfn h() -> Holder: Holder { p: 5 }",
            "fn h() -> Choice[i32, i32]: 5",
            "fn h(c: Choice[i32, i32]) -> i32: c",
            "data Holder:\n    c: Choice[i32, i32]\n\nfn h() -> Holder: Holder { c: 5 }",
            "fn h() -> Wrap[i32, i32]: 5",
            "fn h(w: Wrap[i32, i32]) -> i32: w",
            "data Holder:\n    w: Wrap[i32, i32]\n\nfn h() -> Holder: Holder { w: 5 }",
        ],
    );
}

#[test]
fn own_module_types_written_too_short_report_once() {
    assert_once(
        Code::PartialGenericArguments,
        &[
            "fn h() -> Pair[i32]: 5",
            "fn h(p: Pair[i32]) -> i32: p",
            "data Holder:\n    p: Pair[i32]\n\nfn h() -> Holder: Holder { p: 5 }",
            "fn h() -> Choice: 5",
            "fn h(c: Choice) -> i32: c",
            "data Holder:\n    c: Choice\n\nfn h() -> Holder: Holder { c: 5 }",
            "fn h() -> Wrap: 5",
            "fn h(w: Wrap) -> i32: w",
            "data Holder:\n    w: Wrap\n\nfn h() -> Holder: Holder { w: 5 }",
        ],
    );
}
