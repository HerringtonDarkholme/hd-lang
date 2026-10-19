//! Every fact and metadata expression is type-checked, whether or not any
//! `Structure` or fact read uses it (`annot.metadata.eval-as-fact`,
//! `annot.fact.eval.unread`, `cli.build.facts-at-run-time`): decorators on
//! every target, and the elements of member lines in a block with no trait
//! and in a derivation block. Each error is reported once, at its
//! expression, with or without a derivation that builds the facts.

use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Each error of a program that only analyzes, as its code and 1-based
/// line.
fn errors(main: &str) -> Vec<(Code, usize)> {
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.severity[i] == Severity::Error)
        .map(|i| {
            let lo = out.diags.primary[i].lo as usize;
            let line = main[..lo.min(main.len())].matches('\n').count() + 1;
            (out.diags.code[i], line)
        })
        .collect()
}

/// A fact type with its constructor, and a derivation `Probe` whose walker
/// reads the `Tag` of each member, so a `Structure` is generated for every
/// type that derives it.
const PRELUDE: &str = "use std.structure.{Structure, Field, Variant, Walker}

struct Tag:
    text: string

fn tag(text: string) -> Tag:
    Tag { text: text }

trait Probe:
    fn probe(self) -> string

struct Prober:
    out: string

impl[S] Walker[S] for Prober:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        t := match h.info.facts.find::[Tag]():
            .Some(found) => found.text
            .None => \"-\"
        self.out = self.out + h.info.name + \"=\" + t + \";\"
        .Ok(())

impl[T] Probe for T by Structure:
    fn probe(self) -> string:
        let mut w = Prober { out: \"\" }
        _ := Structure::walk(self, &mut w)
        w.out
";

/// The error each kind of bad fact value makes.
const BAD: [(&str, Code); 3] = [
    ("nope()", Code::UnknownName),
    ("tag(1)", Code::TypeMismatch),
    ("tag()", Code::ArgumentCount),
];

/// The line of `src` marked `# here`.
fn marked(src: &str) -> usize {
    src.lines()
        .position(|l| l.contains("# here"))
        .map_or(0, |i| i + 1)
}

/// `tail` after the prelude, with `bad` for `{bad}`, has the one `code`
/// error at the line marked `# here`.
fn one_error(tail: &str, (bad, code): (&str, Code)) {
    let src = format!("{PRELUDE}\n{}", tail.replace("{bad}", bad));
    assert_eq!(errors(&src), vec![(code, marked(&src))], "{src}");
}

/// The program that reads the facts: it derives `Probe` for `A` and walks
/// one. The type `A` of `tail` must be a data type of one `i64` member `x`.
fn walked(tail: &str) -> String {
    format!(
        "{tail}
pub fn main() -> void $ Console:
    println(A {{ x: 1 }}.probe())
"
    )
}

/// A place a decorator may precede, with `{bad}` for the fact value.
const DECORATED: [&str; 14] = [
    "@{bad}  # here\nstruct A:\n    x: i64\n",
    "struct A:\n    @{bad}  # here\n    x: i64\n",
    "struct A:\n    x: i64\n\n@{bad}  # here\nenum E:\n    One\n    Two\n",
    "struct A:\n    x: i64\n\nenum E:\n    @{bad}  # here\n    One\n    Two\n",
    "struct A:\n    x: i64\n\nenum E:\n    One(@{bad} n: i64)  # here\n    Two\n",
    "struct A:\n    x: i64\n\n@{bad}  # here\ntype Meters(i64)\n",
    "struct A:\n    x: i64\n\n@{bad}  # here\ntrait Handler:\n    fn handle(self) -> i64\n",
    "struct A:\n    x: i64\n\ntrait Handler:\n    @{bad}  # here\n    fn handle(self) -> i64\n",
    "struct A:\n    x: i64\n\n@{bad}  # here\nimpl A:\n    pub fn go(self) -> i64: 1\n",
    "struct A:\n    x: i64\n\nimpl A:\n    @{bad}  # here\n    pub fn go(self) -> i64: 1\n",
    "struct A:\n    x: i64\n\nimpl A:\n    pub fn go(self, @{bad} n: i64) -> i64: n  # here\n",
    "struct A:\n    x: i64\n\nfn f(@{bad} n: i64) -> i64:  # here\n    n\n",
    "struct A:\n    x: i64\n\n@{bad}  # here\nfn f() -> void:\n    pass\n",
    "struct A:\n    x: i64\n\n@{bad}  # here\nimpl A by Structure:\n    pass\n",
];

#[test]
fn decorators_on_every_target_are_checked_with_nothing_reading_them() {
    for place in DECORATED {
        for bad in BAD {
            one_error(place, bad);
        }
    }
}

/// A block with no trait and a derivation block, writing `{bad}` into the
/// literal list of a member line.
const LINES: [&str; 10] = [
    "struct A:\n    x: i64\n\nimpl A by Structure:\n    x = [{bad}]  # here\n",
    "struct A:\n    x: i64\n\nimpl A by Structure:\n    x += [{bad}]  # here\n",
    "struct A:\n    x: i64\n\nimpl A by Structure:\n    Self = [{bad}]  # here\n",
    "struct A:\n    x: i64\n\nimpl A by Structure:\n    x = [tag(\"ok\"), {bad}]  # here\n",
    "struct A:\n    x: i64\n\nimpl Probe for A by Structure:\n    x = [{bad}]  # here\n",
    "struct A:\n    x: i64\n\nimpl Probe for A by Structure:\n    x += [{bad}]  # here\n",
    "struct A:\n    x: i64\n\nimpl Probe for A by Structure:\n    Self += [{bad}]  # here\n",
    "struct A:\n    x: i64\n\nimpl Probe for A by Structure:\n    x = [{bad}]  # here\n    x = [tag(\"ok\")]\n",
    "struct A:\n    x: i64\n\nenum E:\n    One\n    Two\n\nimpl Probe for E by Structure:\n    One = [{bad}]  # here\n",
    "struct B[T]:\n    v: T\n\nstruct A:\n    x: i64\n\nimpl[T] Probe for B[T] by Structure:\n    v = [{bad}]  # here\n",
];

#[test]
fn member_line_elements_are_checked_with_nothing_reading_them() {
    for lines in LINES {
        for bad in BAD {
            one_error(lines, bad);
        }
    }
}

#[test]
fn a_derivation_that_reads_the_facts_adds_no_second_error() {
    let cases = [
        "@{bad}  # here\nstruct A:\n    x: i64\n\nimpl Probe for A by Structure:\n    pass\n",
        "struct A:\n    @{bad}  # here\n    x: i64\n\nimpl Probe for A by Structure:\n    pass\n",
        "struct A:\n    x: i64\n\nimpl A by Structure:\n    x = [{bad}]  # here\n\nimpl Probe for A by Structure:\n    pass\n",
        "struct A:\n    x: i64\n\nimpl Probe for A by Structure:\n    x = [{bad}]  # here\n",
    ];
    for case in cases {
        for bad in BAD {
            let tail = walked(case);
            one_error(&tail, bad);
        }
    }
}

/// Two derivations of one type read its declaration facts, and each is
/// built for both; the error is still one.
#[test]
fn two_derivations_report_a_declaration_fact_once() {
    let tail = "trait Other:
    fn other(self) -> string

impl[T] Other for T by Structure:
    fn other(self) -> string:
        let mut w = Prober { out: \"\" }
        _ := Structure::walk(self, &mut w)
        T::name()

struct A:
    @nope()  # here
    x: i64

impl Probe for A by Structure:
    pass

impl Other for A by Structure:
    pass
";
    let src = format!("{PRELUDE}\n{tail}");
    assert_eq!(
        errors(&src),
        vec![(Code::UnknownName, marked(&src))],
        "{src}"
    );
}

#[test]
fn function_facts_are_checked_once_beside_parameter_facts() {
    let tail = "@nope()  # here
fn f(@alsonope() n: i64) -> i64:
    n
";
    let src = format!("{PRELUDE}\n{tail}");
    let line = marked(&src);
    assert_eq!(
        errors(&src),
        vec![(Code::UnknownName, line), (Code::UnknownName, line + 1)],
        "{src}"
    );
}

#[test]
fn every_bad_fact_of_one_place_is_reported() {
    let tail = "struct A:
    @nope()  # here
    @tag(1)
    x: i64

impl A by Structure:
    x = [tag(2), alsonope()]
";
    let src = format!("{PRELUDE}\n{tail}");
    let line = marked(&src);
    assert_eq!(
        errors(&src),
        vec![
            (Code::UnknownName, line),
            (Code::TypeMismatch, line + 1),
            (Code::TypeMismatch, line + 5),
            (Code::UnknownName, line + 5),
        ],
        "{src}"
    );
}

#[test]
fn a_block_on_call_in_a_fact_is_one_error_with_nothing_reading_it() {
    let tail = "use std.task.block_on

fn ready!() -> string:
    \"p\"

fn quiet[T](source: T) -> Tag:
    tag(\"p\")

@quiet(block_on(ready()))  # here
struct A:
    x: i64

impl A by Structure:
    x = [quiet(block_on(ready()))]
";
    let src = format!("{PRELUDE}\n{tail}");
    let line = marked(&src);
    assert_eq!(
        errors(&src),
        vec![
            (Code::SuspensionForbiddenContext, line),
            (Code::SuspensionForbiddenContext, line + 5),
        ],
        "{src}"
    );
}

/// Valid facts everywhere stay clean, read or not.
#[test]
fn valid_facts_are_not_errors() {
    let tail = "@tag(\"type\")
struct A:
    @tag(\"member\")
    x: i64

impl A by Structure:
    x = [tag(\"line\")]
    Self = [tag(\"self\")]

@tag(\"enum\")
enum E:
    @tag(\"variant\")
    One(@tag(\"payload\") n: i64)
    Two

@tag(\"newtype\")
type Meters(i64)

@tag(\"trait\")
trait Handler:
    @tag(\"method\")
    fn handle(self, @tag(\"param\") n: i64) -> i64

@tag(\"impl\")
impl Handler for A:
    @tag(\"method\")
    fn handle(self, @tag(\"param\") n: i64) -> i64: n

@tag(\"fn\")
fn f(@tag(\"param\") n: i64) -> i64:
    n

impl Probe for E by Structure:
    One = [tag(\"one\")]
";
    let src = format!("{PRELUDE}\n{tail}");
    assert_eq!(errors(&src), Vec::<(Code, usize)>::new(), "{src}");
    let src = format!("{PRELUDE}\n{}", walked(tail));
    let tail = format!("{src}\nimpl Probe for A by Structure:\n    pass\n");
    assert_eq!(errors(&tail), Vec::<(Code, usize)>::new(), "{tail}");
}
