//! One place holds one fact of each concrete type, reported once at the
//! later value (annot.decorator.duplicate, annot.metadata.duplicate,
//! annot.line.duplicate, annot.fact.duplicate-decorator).

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

const HEAD: &str = "use std.annotation.annotate
use std.structure.Structure

data Rename:
    name: string

fn rename(name: string) -> Rename:
    Rename { name: name }

data Label:
    text: string

fn label(text: string) -> Label:
    Label { text: text }

@annotate::[F](.Field)
data Tag[F]:
    value: F

fn tag[F](value: F) -> Tag[F]:
    Tag::[F] { value: value }
";

fn with(tail: &str) -> String {
    format!("{HEAD}\n{tail}\n")
}

/// The line of `main` marked `# here`, which must be the one error.
fn one_error(tail: &str) {
    let src = with(tail);
    let line = src
        .lines()
        .position(|l| l.contains("# here"))
        .map_or(0, |i| i + 1);
    assert_eq!(errors(&src), vec![(Code::DuplicateFact, line)], "{src}");
}

fn clean(tail: &str) {
    let src = with(tail);
    assert_eq!(errors(&src), Vec::<(Code, usize)>::new(), "{src}");
}

#[test]
fn two_literals_of_one_type_before_a_function() {
    one_error("@\"first\"\n@\"second\"  # here\nfn twice() -> void:\n    pass");
}

#[test]
fn a_function_with_values_of_different_types_is_fine() {
    clean("@\"first\"\n@rename(\"x\")\n@label(\"y\")\nfn mixed() -> void:\n    pass");
}

#[test]
fn a_repeated_type_is_reported_once_at_the_second_value() {
    one_error(
        "@rename(\"a\")\n@rename(\"b\")  # here\n@rename(\"c\")\nfn thrice() -> void:\n    pass",
    );
}

#[test]
fn two_values_before_a_trait_an_impl_a_newtype_and_a_method() {
    one_error("@rename(\"a\")\n@rename(\"b\")  # here\ntrait Named:\n    fn name(self) -> string");
    one_error(
        "data Tool:\n    id: i32\n\n@rename(\"a\")\n@rename(\"b\")  # here\nimpl Tool:\n    fn id_of(self) -> i32:\n        self.id",
    );
    one_error("@rename(\"a\")\n@rename(\"b\")  # here\ntype Id(i64)");
    one_error(
        "data Tool:\n    id: i32\n\nimpl Tool:\n    @rename(\"a\")\n    @rename(\"b\")  # here\n    fn id_of(self) -> i32:\n        self.id",
    );
}

#[test]
fn two_values_before_a_member_a_variant_or_a_parameter() {
    one_error("data User:\n    @rename(\"a\")\n    @rename(\"b\")  # here\n    email: string");
    one_error(
        "enum Shape:\n    @rename(\"a\")\n    @rename(\"b\")  # here\n    Circle(radius: f64)",
    );
    one_error("enum Shape:\n    Circle(@rename(\"a\") @rename(\"b\") radius: f64)  # here");
}

#[test]
fn a_typed_fact_and_a_plain_fact_are_different_types() {
    clean("data Account:\n    @tag(1)\n    @rename(\"one\")\n    id: i32");
}

#[test]
fn typed_facts_of_one_type_repeat() {
    one_error("data Account:\n    @tag(1)\n    @tag(2)  # here\n    id: i32");
}

#[test]
fn two_type_level_decorators_of_one_type() {
    one_error("@rename(\"a\")\n@rename(\"b\")  # here\ndata Twice:\n    id: i64");
    clean("@rename(\"a\")\n@label(\"b\")\ndata Twice:\n    id: i64");
}

#[test]
fn a_member_line_that_appends_a_present_type_repeats_it() {
    one_error(
        "data Pair:\n    left: i64\n\nimpl Pair by Structure:\n    left += [rename(\"l\")]\n    left += [rename(\"r\")]  # here",
    );
    one_error(
        "data User:\n    @rename(\"a\")\n    name: string\n\nimpl User by Structure:\n    name += [rename(\"b\")]  # here",
    );
}

#[test]
fn a_member_line_that_appends_a_new_type_is_fine() {
    clean(
        "data User:\n    @rename(\"a\")\n    name: string\n\nimpl User by Structure:\n    name += [label(\"b\")]",
    );
}

#[test]
fn a_member_line_that_replaces_drops_the_present_type() {
    clean(
        "data User:\n    @rename(\"a\")\n    name: string\n\nimpl User by Structure:\n    name = [rename(\"b\")]",
    );
    clean(
        "data User:\n    @rename(\"a\")\n    name: string\n\nimpl User by Structure:\n    name += [label(\"b\")]\n    name = [rename(\"c\")]",
    );
}

#[test]
fn the_block_with_no_trait_appends_after_the_decorators() {
    one_error(
        "data User:\n    @rename(\"a\")\n    name: string\n\nimpl User by Structure:\n    name += [rename(\"b\")]  # here",
    );
}

#[test]
fn a_trait_derivation_after_a_clean_traitless_block_is_fine() {
    clean(
        "data User:\n    @rename(\"a\")\n    name: string\n\nimpl User by Structure:\n    name = [label(\"b\")]\n\ntrait Named:\n    fn named(self) -> string\n",
    );
}
