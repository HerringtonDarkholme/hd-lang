//! Definite initialization counts implicit calls
//! (`module.init.definite.implicit`): a top-level statement that reaches a
//! binding through an interpolation, a `for`, an operator, an index, a
//! callable value, a `?` conversion, a derived impl, a generic function's
//! bound or a trait value is `top-level-read-before-initialization`,
//! once. The same program with the binding first prints its value.
//! Programs build through the driver and run on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// One form of implicit call. `pre` is what the statement needs and does
/// not read `later` (types, helper functions). `post` is the impl, after
/// the binding, whose body reads it. `value` is the top-level statement's
/// expression, and `printed` what the accepted program prints.
struct Case {
    name: &'static str,
    pre: &'static str,
    post: &'static str,
    value: &'static str,
    printed: &'static str,
}

const TAG: &str = "data Tag:\n    n: i32\n";

fn build_main(main: &str) -> Output {
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

const MAIN: &str = "\npub fn main() -> void $ Console:\n    println(first)\n";

/// The statement before the binding it reaches.
fn late(c: &Case) -> String {
    format!(
        "{TAG}{}\nfirst := {}\nlater := +5\n\n{}{MAIN}",
        c.pre, c.value, c.post
    )
}

/// The binding before the statement.
fn in_order(c: &Case) -> String {
    format!(
        "{TAG}{}\nlater := +5\n\n{}\nfirst := {}\n{MAIN}",
        c.pre, c.post, c.value
    )
}

fn run(name: &str, main: &str) -> String {
    let out = build_main(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("init-implicit-{name}.wasm"),
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

fn check(c: &Case) {
    let out = build_main(&late(c));
    assert_eq!(
        out.diags.code,
        vec![Code::TopLevelReadBeforeInitialization],
        "{}: {}",
        c.name,
        out.render()
    );
    assert_eq!(run(c.name, &in_order(c)), format!("{}\n", c.printed));
}

#[test]
fn an_interpolation_reaches_the_display_impl() {
    check(&Case {
        name: "display",
        pre: "",
        post: "impl Display for Tag:\n    fn to_string(self) -> string: \"tag ${later}\"\n",
        value: "\"${Tag { n: 1 }}\"",
        printed: "tag 5",
    });
}

#[test]
fn a_for_loop_reaches_iter() {
    check(&Case {
        name: "iterable",
        pre: "data Src:\n    n: i32\n\nfn count() -> i32:\n    let t = +0\n    for x in Src { n: 1 }:\n        t = t + x\n        break\n    t\n",
        post: "impl Iterable[i32] for Src:\n    fn iter(self) -> &mut Iterator[i32]:\n        Iterator::from_fn(fn() -> i32?: .Some(later))\n",
        value: "count()",
        printed: "5",
    });
}

#[test]
fn an_equality_operator_reaches_eq() {
    check(&Case {
        name: "eq",
        pre: "",
        post: "impl Eq for Tag:\n    fn eq(self, other: Tag) -> bool: self.n + later == other.n\n",
        value: "Tag { n: 1 } == Tag { n: 6 }",
        printed: "true",
    });
}

#[test]
fn an_ordering_operator_reaches_partial_cmp() {
    check(&Case {
        name: "partial-cmp",
        pre: "",
        post: "impl Eq for Tag:\n    fn eq(self, other: Tag) -> bool: true\n\nimpl PartialOrd for Tag:\n    fn partial_cmp(self, other: Tag) -> Ordering?: (self.n + later).partial_cmp(other.n)\n",
        value: "Tag { n: 1 } < Tag { n: 2 }",
        printed: "false",
    });
}

#[test]
fn an_arithmetic_operator_reaches_add() {
    check(&Case {
        name: "add",
        pre: "use std.ops.Add\n",
        post: "impl Add for Tag:\n    type Out = i32\n    fn add(self, rhs: Tag) -> i32: self.n + rhs.n + later\n",
        value: "Tag { n: 1 } + Tag { n: 2 }",
        printed: "8",
    });
}

#[test]
fn a_unary_operator_reaches_neg() {
    check(&Case {
        name: "neg",
        pre: "use std.ops.Neg\n",
        post: "impl Neg for Tag:\n    type Out = i32\n    fn neg(self) -> i32: later - self.n\n",
        value: "-Tag { n: 1 }",
        printed: "4",
    });
}

#[test]
fn an_index_read_reaches_index() {
    check(&Case {
        name: "index",
        pre: "use std.ops.Index\n",
        post: "impl Index[i32] for Tag:\n    type Out = i32\n    fn index(self, key: i32) -> i32: self.n + key + later\n",
        value: "Tag { n: 1 }[2]",
        printed: "8",
    });
}

#[test]
fn an_index_assignment_reaches_index_set() {
    check(&Case {
        name: "index-set",
        pre: "use std.ops.IndexSet\n\nfn fill() -> i32:\n    let mut t = Tag { n: 1 }\n    t[0] = 7\n    t.n\n",
        post: "impl IndexSet[i32, i32] for Tag:\n    fn index_set(&mut self, key: i32, value: i32) -> void:\n        self.n = key + value + later\n",
        value: "fill()",
        printed: "12",
    });
}

#[test]
fn calling_a_value_reaches_apply() {
    check(&Case {
        name: "apply",
        pre: "use std.ops.Apply\n",
        post: "impl Apply for Tag:\n    type Out = i32\n    fn apply(self) -> i32: self.n + later\n",
        value: "Tag { n: 1 }()",
        printed: "6",
    });
}

#[test]
fn a_question_mark_reaches_from() {
    check(&Case {
        name: "from",
        pre: "use std.convert.From\n\ndata Wide:\n    m: i32\n\nfn risky() -> Result[i32, Tag]:\n    .Err(Tag { n: 1 })\n\nfn wrap() -> Result[i32, Wide]:\n    risky()?\n    .Ok(1)\n\nfn code() -> i32:\n    match wrap():\n        .Ok(v) => v\n        .Err(e) => e.m\n",
        post: "impl From[Tag] for Wide:\n    fn from(e: Tag) -> Wide: Wide { m: e.n + later }\n",
        value: "code()",
        printed: "6",
    });
}

#[test]
fn a_derived_impl_reaches_its_fields_impls() {
    check(&Case {
        name: "derived",
        pre: "@derive(Eq)\ndata Wrap:\n    t: Tag\n",
        post: "impl Eq for Tag:\n    fn eq(self, other: Tag) -> bool: self.n + later == other.n\n",
        value: "Wrap { t: Tag { n: 1 } } == Wrap { t: Tag { n: 6 } }",
        printed: "true",
    });
}

#[test]
fn a_std_function_bound_reaches_the_impls_it_is_given() {
    check(&Case {
        name: "bound",
        pre: "use std.cmp.min\n",
        post: "impl Eq for Tag:\n    fn eq(self, other: Tag) -> bool: self.n + later == other.n\n\nimpl PartialOrd for Tag:\n    fn partial_cmp(self, other: Tag) -> Ordering?: (self.n + later).partial_cmp(other.n)\n\nimpl Ord for Tag:\n    fn cmp(self, other: Tag) -> Ordering: (self.n + later).cmp(other.n)\n",
        value: "min(Tag { n: 1 }, Tag { n: 2 }).n",
        printed: "1",
    });
}

#[test]
fn a_trait_value_reaches_every_impl() {
    check(&Case {
        name: "trait-value",
        pre: "trait Name:\n    fn name(self) -> string\n\ndata Other:\n    n: i32\n\nimpl Name for Tag:\n    fn name(self) -> string: \"a\"\n\nfn pick() -> dyn Name:\n    Tag { n: 1 }\n",
        post: "impl Name for Other:\n    fn name(self) -> string: \"b${later}\"\n",
        value: "pick().name()",
        printed: "a",
    });
}

/// An interpolation of a type with no impl in the module does not reach
/// the module's `Display` impl.
#[test]
fn an_unrelated_display_impl_is_not_reached() {
    let main = format!(
        "{TAG}\nx := +3\nfirst := \"${{x}} ${{true}}\"\nlater := +5\n\nimpl Display for Tag:\n    fn to_string(self) -> string: \"tag ${{later}}\"\n{MAIN}"
    );
    assert_eq!(run("unrelated", &main), "3 true\n");
}

/// A use that fails to find its impl is that one error, with no
/// initialization error beside it.
#[test]
fn a_missing_impl_adds_no_initialization_error() {
    let main = format!(
        "{TAG}\nfirst := \"${{[Tag {{ n: 1 }}]}}\"\nlater := +5\n\nimpl Display for Tag:\n    fn to_string(self) -> string: \"tag ${{later}}\"\n{MAIN}"
    );
    let out = build_main(&main);
    assert_eq!(
        out.diags.code,
        vec![Code::UnsatisfiedTraitBound],
        "{}",
        out.render()
    );
}
