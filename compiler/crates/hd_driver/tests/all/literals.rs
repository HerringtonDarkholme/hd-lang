//! Literal typing and conversions: negative literals into unsigned types,
//! numeric literal suffixes (`250ms`), and string prefixes (`sql"..."`).

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

/// The program has no diagnostic but an `Emit` stub's `unsupported`.
fn assert_clean(src: &str) {
    let out = program(src);
    assert!(
        codes(&out).iter().all(|c| *c == Code::Unsupported),
        "{src}\n{}",
        out.render()
    );
}

fn assert_code(src: &str, code: Code) {
    let out = program(src);
    assert!(codes(&out).contains(&code), "{src}\n{}", out.render());
}

const MAIN: &str = "\npub fn main() -> void:\n    pass\n";

const MILLIS: &str = "use std.ops.num_suffix

data Millis:
    count: i64

@num_suffix
fn ms(count: i64) -> Millis:
    Millis { count: count }

";

const QUERY: &str = "use std.ops.{Template, str_prefix}

data Query:
    text: List[string]
    params: List[i32]

@str_prefix
fn sql(t: Template[i32]) -> Query:
    Query { text: t.raw_parts, params: t.values }

";

#[test]
fn a_negative_literal_needs_a_signed_type() {
    assert_clean(&format!("fn low() -> i8:\n    -128\n{MAIN}"));
    assert_clean(&format!(
        "fn low() -> i64:\n    -9223372036854775808\n{MAIN}"
    ));
    assert_code(
        &format!("fn bad() -> u8:\n    -1\n{MAIN}"),
        Code::TypeMismatch,
    );
    assert_code(
        &format!("fn bad(value: u64) -> u64:\n    (-1 + value) / 10\n{MAIN}"),
        Code::TypeMismatch,
    );
    assert_code(
        &format!("fn bad() -> i8:\n    -129\n{MAIN}"),
        Code::IntegerLiteralRange,
    );
    assert_code(
        &format!("fn bad() -> i64:\n    9223372036854775808\n{MAIN}"),
        Code::IntegerLiteralRange,
    );
}

#[test]
fn a_suffixed_literal_calls_its_suffix_function() {
    assert_clean(&format!("{MILLIS}fn delay() -> Millis:\n    250ms\n{MAIN}"));
    // The result type is the function's.
    assert_code(
        &format!("{MILLIS}fn delay() -> i64:\n    250ms\n{MAIN}"),
        Code::TypeMismatch,
    );
    // The literal is checked as the argument: a float for an `i64`.
    assert_code(
        &format!("{MILLIS}fn delay() -> Millis:\n    1.5ms\n{MAIN}"),
        Code::TypeMismatch,
    );
    assert_code(
        &format!("fn delay() -> i64:\n    5furlong\n{MAIN}"),
        Code::UnknownName,
    );
    assert_code(
        &format!("fn pt(n: i32) -> i32:\n    n\n\nfn size() -> i32:\n    12pt\n{MAIN}"),
        Code::InvalidLiteralSuffix,
    );
    assert_code(
        &format!(
            "use std.ops.num_suffix\n\n@num_suffix\nfn off(value: i32) -> i32:\n    value\n\nfn lowest() -> i32:\n    -2147483648off\n{MAIN}"
        ),
        Code::IntegerLiteralRange,
    );
}

#[test]
fn a_literal_function_marker_must_fit() {
    assert_code(
        &format!(
            "use std.ops.num_suffix\n\n@num_suffix\nfn kb(count: i64, unit: i64 = 1024) -> i64:\n    count * unit\n{MAIN}"
        ),
        Code::TypeMismatch,
    );
    assert_code(
        &format!(
            "use std.ops.num_suffix\n\n@num_suffix\nfn em(label: string) -> i64:\n    0\n{MAIN}"
        ),
        Code::UnsatisfiedTraitBound,
    );
}

#[test]
fn a_prefixed_string_calls_its_prefix_function() {
    assert_clean(&format!(
        "{QUERY}fn by_id(id: i32) -> Query:\n    sql\"select * from users where id = $id\"\n{MAIN}"
    ));
    assert_clean(&format!(
        "{QUERY}fn by_id(id: i32) -> Query:\n    sql\"id = ${{id + 1}} or $id\"\n{MAIN}"
    ));
    assert_code(
        &format!("{QUERY}fn find(name: string) -> Query:\n    sql\"id = $name\"\n{MAIN}"),
        Code::TypeMismatch,
    );
    assert_code(
        &format!("{QUERY}fn find(big: i64) -> Query:\n    sql\"id = $big\"\n{MAIN}"),
        Code::ImplicitNarrowing,
    );
    assert_code(
        &format!(
            "fn plain(t: string) -> string:\n    t\n\nfn render() -> string:\n    plain\"x\"\n{MAIN}"
        ),
        Code::InvalidStringPrefix,
    );
    assert_code(
        &format!("fn render() -> string:\n    nope\"x\"\n{MAIN}"),
        Code::UnknownName,
    );
}

/// Every error code the program reports but an `Emit` stub's
/// `unsupported`.
fn errors(src: &str) -> Vec<Code> {
    let out = program(src);
    out.diags
        .code
        .iter()
        .zip(&out.diags.severity)
        .filter(|(c, s)| **c != Code::Unsupported && **s == hd_diag::Severity::Error)
        .map(|(c, _)| *c)
        .collect()
}

#[test]
fn an_open_literal_wraps_into_an_optional() {
    // `types.literal.local.expected` through `.Some` wrapping: the class
    // meets the optional's payload type.
    assert_clean(&format!(
        "fn f() -> i32?:\n    let x: i32? = 42\n    x\n{MAIN}"
    ));
    assert_clean(&format!(
        "fn f(flag: bool) -> i32?:\n    if flag: 40 else: .None\n{MAIN}"
    ));
    assert_clean(&format!("fn f() -> List[f64?]:\n    [1.5, .None]\n{MAIN}"));
    assert_code(
        &format!("fn f() -> u8?:\n    let x: u8? = 300\n    x\n{MAIN}"),
        Code::IntegerLiteralRange,
    );
}

#[test]
fn a_literal_converts_to_a_trait_value_at_its_default_type() {
    // `types.literal.local.erased`: `5` is a `usize`, which is `Display`.
    assert_clean(&format!(
        "fn f() -> string:\n    let x: dyn Display = 5\n    \"$x\"\n{MAIN}"
    ));
    let gauge = "trait Gauge:\n    fn level(self) -> f64\n\nimpl Gauge for f64:\n    fn level(self) -> f64:\n        self\n\n";
    assert_clean(&format!(
        "{gauge}fn f() -> f64:\n    let g: dyn Gauge = 0.5\n    g.level()\n{MAIN}"
    ));
    // `0` falls back to `usize`, which is no `Gauge`; a value whose type
    // lacks the impl does not convert (`trait.dyn.convert`).
    assert_eq!(
        errors(&format!(
            "{gauge}fn f() -> f64:\n    let g: dyn Gauge = 0\n    g.level()\n{MAIN}"
        )),
        vec![Code::TypeMismatch]
    );
}

#[test]
fn an_integer_literal_never_takes_a_float_type() {
    // `types.literal.int-not-float`.
    assert_eq!(
        errors(&format!(
            "fn f() -> f64:\n    let r: f64 = 1\n    r\n{MAIN}"
        )),
        vec![Code::TypeMismatch]
    );
    assert_eq!(
        errors(&format!(
            "fn f() -> f64:\n    let r = 0.5 * 2\n    r\n{MAIN}"
        )),
        vec![Code::TypeMismatch]
    );
    assert_clean(&format!(
        "fn f() -> f64:\n    let r: f64 = 1.0\n    r * 2.0\n{MAIN}"
    ));
}
