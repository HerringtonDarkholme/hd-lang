//! Literal typing and conversions: negative literals into unsigned types,
//! numeric literal suffixes (`250ms`), and string prefixes (`sql"..."`).

use hd_diag::Code;
use hd_driver::{
    Dependency, Executor, Goal, Host, NoClock, Output, Packages, build, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn with_dependency(main: &str, dep: &str) -> Output {
    let mut root = MemorySources::default();
    root.insert("src/main.hd", main);
    let mut dep_sources = MemorySources::default();
    dep_sources.insert("src/lib.hd", "");
    dep_sources.insert("src/marks.hd", dep);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &root,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: vec![("pkg".to_owned(), 1)],
        deps: vec![Dependency {
            name: "pkg".to_owned(),
            identity: "pkg".to_owned(),
            sources: &dep_sources,
            requires: Vec::new(),
            fetched: false,
        }],
        ..Packages::default()
    };
    build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

fn assert_clean_output(out: &Output, src: &str) {
    assert!(
        codes(out).iter().all(|c| *c == Code::Unsupported),
        "{src}\n{}",
        out.render()
    );
}

/// The program has no diagnostic but an `Emit` stub's `unsupported`.
fn assert_clean(src: &str) {
    assert_clean_output(&program(src), src);
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

// `expr.literal-fn.marker` and `.not-marked`: a marker is recognized by its
// resolved declaration, including aliases and qualified paths, never by text.
#[test]
fn a_numeric_suffix_marker_resolves_by_qualified_name() {
    for (import, marker) in [
        ("use std.ops as ops\n", "ops.num_suffix"),
        ("use std.ops.num_suffix as suffix\n", "suffix"),
    ] {
        let src = format!(
            "{import}@{marker}\nfn kb(count: i32) -> i32:\n    count\n\nfn delay() -> i32: 250kb{MAIN}"
        );
        assert_clean(&src);
    }
    let src = format!(
        "use dep.pkg\n@pkg.marks.num_suffix\nfn kb(count: i32) -> i32:\n    count\n\nfn delay() -> i32: 250kb{MAIN}"
    );
    assert_clean_output(
        &with_dependency(&src, "pub use std.ops.{num_suffix}\n"),
        &src,
    );
}

#[test]
fn a_string_prefix_marker_resolves_by_qualified_name() {
    for (import, marker) in [
        ("use std.ops as ops\n", "ops.str_prefix"),
        ("use std.ops.str_prefix as prefix\n", "prefix"),
    ] {
        let src = format!(
            "{import}use std.ops.Template\n@{marker}\nfn sql(t: Template[i32]) -> i32:\n    1\n\nfn find() -> i32: sql\"select\"{MAIN}"
        );
        assert_clean(&src);
    }
    let src = format!(
        "use std.ops.Template\nuse dep.pkg\n@pkg.marks.str_prefix\nfn sql(t: Template[i32]) -> i32:\n    1\n\nfn find() -> i32: sql\"select\"{MAIN}"
    );
    assert_clean_output(
        &with_dependency(&src, "pub use std.ops.{str_prefix}\n"),
        &src,
    );
}

#[test]
fn a_same_named_local_marker_does_not_hijack_literal_functions() {
    let suffix = "fn num_suffix() -> i32: 1\n\n@num_suffix\nfn kb(count: i32) -> i32:\n    count\n\nfn delay() -> i32: 250kb";
    assert_eq!(
        errors(&format!("{suffix}{MAIN}")),
        vec![Code::InvalidLiteralSuffix]
    );
    let prefix = "use std.ops.Template\n\nfn str_prefix() -> i32: 1\n\n@str_prefix\nfn sql(t: Template[i32]) -> i32:\n    1\n\nfn find() -> i32: sql\"select\"";
    assert_eq!(
        errors(&format!("{prefix}{MAIN}")),
        vec![Code::InvalidStringPrefix]
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

fn float_main(body: &str) -> String {
    format!("pub fn main() -> void:\n{body}\n    pass\n")
}

#[test]
fn float_literal_past_f64_range_is_rejected() {
    for body in [
        "    large := 1.5e1_000",
        "    let x: f64 = 1e999",
        "    let x: f64 = -1e999",
        "    x := 1.8e308",
    ] {
        assert_code(&float_main(body), Code::FloatLiteralRange);
    }
}

#[test]
fn float_literal_past_f32_range_is_rejected() {
    for body in [
        "    let x: f32 = 1e40",
        "    let x: f32 = -1e40",
        "    let x: f32 = 3.5e38",
        "    let x: f32 = 3.4028236e38",
    ] {
        assert_code(&float_main(body), Code::FloatLiteralRange);
    }
}

#[test]
fn float_literal_takes_f32_from_expected_type() {
    let src =
        "fn take(x: f32) -> f32:\n    x\n\npub fn main() -> void:\n    take(1e40)\n    pass\n";
    assert_code(src, Code::FloatLiteralRange);
    let src = "fn give() -> f32:\n    -1e40\n\npub fn main() -> void:\n    pass\n";
    assert_code(src, Code::FloatLiteralRange);
}

#[test]
fn float_literal_in_range_is_accepted() {
    for body in [
        "    let a: f64 = 1.7976931348623157e308",
        "    let b: f64 = -1.7976931348623157e308",
        "    let c: f64 = 1e-999",
        "    let d: f32 = 3.4028234663852886e38",
        "    let e: f32 = 3.4028235e38",
        "    let f: f32 = 1e-60",
        "    let g: f32 = 1e38",
        "    h := 1e300",
    ] {
        let src = float_main(body);
        let out = program(&src);
        assert!(
            !codes(&out).contains(&Code::FloatLiteralRange),
            "{src}\n{}",
            out.render()
        );
    }
}

/// `expr.arith.unary-minus`: unary `-` on an unsigned operand is a
/// `type-mismatch`, including when the type is fixed after the operand.
#[test]
fn unary_minus_on_unsigned_is_rejected() {
    for body in [
        "fn f(n: u64) -> u64:\n    -n\n",
        "fn f(n: u8) -> u8:\n    -(n + 1)\n",
        "fn f() -> u32:\n    let x: u32 = -(2 + 2)\n    x\n",
        "fn g() -> usize:\n    1\n\nfn f() -> usize:\n    -g()\n",
        "fn f() -> usize:\n    let z = -(2 + 2)\n    z\n",
        "fn f() -> u16:\n    let x: u16 = -(1)\n    x\n",
    ] {
        let src = format!("{body}{MAIN}");
        let out = program(&src);
        assert_eq!(codes(&out), [Code::TypeMismatch], "{src}\n{}", out.render());
    }
}

#[test]
fn unary_minus_on_signed_and_float_is_valid() {
    for body in [
        "fn f(n: i32) -> i32:\n    -n\n",
        "fn f() -> i32:\n    let x: i32 = -(2 + 2)\n    x\n",
        "fn f(z: i64) -> i64:\n    -(z + 2)\n",
        "fn f() -> i8:\n    -(-128 + 1)\n",
        "fn f(x: f32) -> f32:\n    -x\n",
        "fn f() -> f64:\n    -(1.5 + 2.0)\n",
    ] {
        assert_clean(&format!("{body}{MAIN}"));
    }
}

/// `expr.power.negated-literal`: a negated literal is signed, with or
/// without parentheses, so a bare literal under `-` takes `i32` rather
/// than failing as unsigned `usize`.
#[test]
fn a_negated_bare_literal_is_signed_through_parentheses() {
    for body in [
        "fn f() -> i32:\n    -(5)\n",
        "fn f() -> i32:\n    -((5))\n",
        "fn f() -> i32:\n    +(5)\n",
        "fn f(x: i32) -> i32:\n    x + -(5)\n",
    ] {
        assert_clean(&format!("{body}{MAIN}"));
    }
}

/// `types.literal.local.signed`: in `-2 ** 2` with no expected type the
/// literal is signed, so the power is `i32` and the negation applies.
#[test]
fn a_signed_literal_under_power_checks_and_evaluates() {
    for body in [
        "fn f() -> i32:\n    -2 ** 2\n",
        "fn f() -> i32:\n    -(2 ** 2)\n",
        "fn f(x: i32) -> i32:\n    x + -2 ** 2\n",
    ] {
        assert_clean(&format!("{body}{MAIN}"));
    }
}
