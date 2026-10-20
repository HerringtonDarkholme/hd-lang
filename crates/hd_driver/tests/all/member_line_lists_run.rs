//! A member line's right side is any list-typed expression
//! (`annot.line.right-typed`, `annot.line.reference-elements`,
//! `annot.line.right.not-list`): a name, a call, or a concatenation of
//! lists, in a block with no trait and in a derivation block, where `=`
//! replaces the facts before it and `+=` appends. The facts of a list
//! expression are built when the program first reads them
//! (`annot.fact.eval.lazy`), and only a literal's own elements are checked
//! for a repeated type (`annot.line.duplicate`). Programs build through
//! the driver and run on V8 (`host/run.mjs`), as the conformance runner
//! does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> hd_driver::Output {
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
    build(&host, "app", goal)
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("member-line-lists-run-{name}.wasm"),
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

/// Each error of a program that only analyzes, as its code and 1-based
/// line.
fn errors(main: &str) -> Vec<(Code, usize)> {
    let out = built(main, &Goal::Analyze);
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

/// Two fact types, lists of them in module-level lets and a function, and
/// a `Probe` derivation whose walker prints each member's `Tag` and `Note`
/// facts.
const PRELUDE: &str = "use std.inspect.Inspectable
use std.structure.{Structure, Field, Variant, Walker}

struct Tag:
    text: string

struct Note:
    text: string

fn tag(text: string) -> Tag:
    Tag { text: text }

fn note(text: string) -> Note:
    Note { text: text }

let tagged: List[&dyn Inspectable] = [&tag(\"t\")]
let noted: List[&dyn Inspectable] = [&note(\"n\")]

fn made() -> List[&dyn Inspectable]:
    [&note(\"made\")]

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
        n := match h.info.facts.find::[Note]():
            .Some(found) => found.text
            .None => \"-\"
        self.out = self.out + h.info.name + \"=\" + t + \"/\" + n + \";\"
        .Ok(())

impl[T] Probe for T by Structure:
    fn probe(self) -> string:
        let mut w = Prober { out: \"\" }
        _ := Structure::walk(self, &mut w)
        w.out
";

fn with_prelude(rest: &str) -> String {
    format!("{PRELUDE}\n{rest}")
}

/// A program printing what `Point { .. }.probe()` walks.
fn probed(name: &str, rest: &str) -> String {
    let main = with_prelude(&format!(
        "{rest}
pub fn main() -> void $ Console:
    println(Point {{ a: 1, b: 2, c: 3, d: 4 }}.probe())
"
    ));
    output_of(name, &main)
}

#[test]
fn block_with_no_trait_takes_a_name_a_call_and_a_concatenation() {
    let got = probed(
        "traitless",
        "struct Point:
    a: i64
    b: i64
    c: i64
    d: i64

impl Point by Structure:
    a = tagged
    b = made()
    c = [tagged..., noted...]
    d += [tag(\"lit\"), made()...]

impl Probe for Point by Structure:
    pass
",
    );
    assert_eq!(got, "a=t/-;b=-/made;c=t/n;d=lit/made;\n");
}

#[test]
fn derivation_block_takes_a_name_a_call_and_a_concatenation() {
    let got = probed(
        "derivation",
        "struct Point:
    a: i64
    b: i64
    c: i64
    d: i64

impl Probe for Point by Structure:
    a = tagged
    b = made()
    c = [tagged..., noted...]
    d += [tag(\"lit\"), made()...]
",
    );
    assert_eq!(got, "a=t/-;b=-/made;c=t/n;d=lit/made;\n");
}

/// `=` replaces what the declaration and an earlier line left, and `+=`
/// appends to it, for a list expression as for a literal.
#[test]
fn replace_and_extend_apply_to_a_list_expression() {
    let got = probed(
        "replace-extend",
        "struct Point:
    @tag(\"decl\")
    a: i64
    @tag(\"decl\")
    b: i64
    @tag(\"decl\")
    c: i64
    d: i64

impl Point by Structure:
    a = noted
    b += noted

impl Probe for Point by Structure:
    c = made()
    d += tagged
    d += noted
",
    );
    assert_eq!(got, "a=-/n;b=decl/n;c=-/made;d=t/n;\n");
}

/// A list expression builds its facts once, when the facts are first
/// read, and never when they are not.
#[test]
fn list_expression_runs_once_when_first_read() {
    let main = with_prelude(
        "let evaluations = +0

fn counted() -> List[&dyn Inspectable]:
    evaluations = evaluations + 1
    [&note(\"counted\")]

struct Point:
    a: i64

impl Probe for Point by Structure:
    a = counted()

pub fn main() -> void $ Console:
    println(evaluations)
    println(Point { a: 1 }.probe())
    println(evaluations)
    println(Point { a: 2 }.probe())
    println(evaluations)
",
    );
    assert_eq!(
        output_of("once", &main),
        "0\na=-/counted;\n1\na=-/counted;\n1\n"
    );
}

/// `Self` takes a list expression too, and the type-level facts are read
/// through `T::facts()`.
#[test]
fn self_line_takes_a_list_expression() {
    let main = with_prelude(
        "trait Label:
    fn label(self) -> string

impl[T] Label for T by Structure:
    fn label(self) -> string:
        let mut w = Prober { out: \"\" }
        _ := Structure::walk(self, &mut w)
        match T::facts().find::[Note]():
            .Some(found) => found.text
            .None => \"none\"

struct Point:
    a: i64

impl Point by Structure:
    Self = made()

impl Label for Point by Structure:
    Self += tagged

pub fn main() -> void $ Console:
    println(Point { a: 1 }.label())
",
    );
    assert_eq!(output_of("self-line", &main), "made\n");
}

fn one_error(code: Code, tail: &str) {
    let src = with_prelude(tail);
    let line = src
        .lines()
        .position(|l| l.contains("# here"))
        .map_or(0, |i| i + 1);
    assert_eq!(errors(&src), vec![(code, line)], "{src}");
}

fn clean(tail: &str) {
    let src = with_prelude(tail);
    assert_eq!(errors(&src), Vec::<(Code, usize)>::new(), "{src}");
}

const COUNT: &str = "let count: i64 = 5

fn words() -> string:
    \"w\"

let names: List[string] = [\"n\"]
";

#[test]
fn a_value_that_is_not_a_list_is_one_invalid_member_line() {
    for block in [
        "impl Point by Structure",
        "impl Probe for Point by Structure",
    ] {
        for right in ["count", "words()", "(count)"] {
            one_error(
                Code::InvalidMemberLine,
                &format!(
                    "{COUNT}
struct Point:
    a: i64

{block}:
    a = {right}  # here
"
                ),
            );
        }
    }
}

#[test]
fn extending_with_a_value_that_is_not_a_list_is_one_invalid_member_line() {
    one_error(
        Code::InvalidMemberLine,
        &format!(
            "{COUNT}
struct Point:
    a: i64

impl Probe for Point by Structure:
    a += count  # here
"
        ),
    );
}

#[test]
fn a_list_of_another_element_type_is_a_type_mismatch() {
    one_error(
        Code::TypeMismatch,
        &format!(
            "{COUNT}
struct Point:
    a: i64

impl Point by Structure:
    a = names  # here
"
        ),
    );
}

#[test]
fn a_spread_of_a_value_that_is_not_a_list_is_a_type_mismatch() {
    one_error(
        Code::TypeMismatch,
        &format!(
            "{COUNT}
struct Point:
    a: i64

impl Point by Structure:
    a = [count...]  # here
"
        ),
    );
}

#[test]
fn a_name_that_does_not_exist_is_reported_once() {
    one_error(
        Code::UnknownName,
        "struct Point:
    a: i64

impl Point by Structure:
    a = nowhere  # here
",
    );
}

/// A repeated type among a literal's own elements is still reported at
/// the later one, with a list expression between them.
#[test]
fn literal_elements_still_report_a_repeated_type() {
    one_error(
        Code::DuplicateFact,
        "struct Point:
    a: i64

impl Point by Structure:
    a = [tag(\"x\"), tagged..., tag(\"y\")]  # here
",
    );
}

/// What a list expression holds is known only when the program runs, so
/// it never makes a repeat at check time.
#[test]
fn a_list_expression_adds_no_repeat_at_check_time() {
    clean(
        "struct Point:
    @tag(\"decl\")
    a: i64
    @tag(\"decl\")
    b: i64

impl Point by Structure:
    a += tagged
    b = [tag(\"x\"), tagged...]
",
    );
}
