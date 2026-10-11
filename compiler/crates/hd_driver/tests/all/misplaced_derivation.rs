//! The placement rules of derivation syntax (spec 14 `annot.template.module`,
//! `annot.block.module`, `annot.block.newtype.error`,
//! `annot.line.placement-blocks`, `annot.no-trait.*`): each rejected form is
//! one `misplaced-derivation` and nothing else, and the valid forms are
//! silent. The single-file forms are also conformance fixtures; the
//! other-module rules need two modules.

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

fn lines(out: &Output, src: &str) -> Vec<usize> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.code[i] == Code::MisplacedDerivation)
        .map(|i| {
            src[..out.diags.primary[i].lo as usize]
                .matches('\n')
                .count()
                + 1
        })
        .collect()
}

fn clean(files: &[(&str, &str)]) {
    let out = analyze(files);
    assert_eq!(codes(&out), vec![], "{}", out.render());
}

fn misplaced(files: &[(&str, &str)], main: &str, at: &[usize]) {
    let out = analyze(files);
    assert_eq!(
        codes(&out),
        vec![Code::MisplacedDerivation; at.len()],
        "{}",
        out.render()
    );
    assert_eq!(lines(&out, main), at, "{}", out.render());
}

/// A `Show` trait with its template; `{tail}` follows.
const SHOW_TEMPLATE: &str = "use std.structure.{Structure, Field, Variant, Walker}

pub trait Show:
    fn show(self) -> string

impl Show for i64:
    fn show(self) -> string:
        \"$self\"

data Shower:
    out: string

impl[S] Walker[S] for Shower:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Show](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + value.show() + \";\"
        .Ok(())

impl[T] Show for T by Structure:
    fn show(self) -> string:
        let w: mut Shower = Shower { out: \"\" }
        _ := Structure::walk(self, w)
        w.out
";

#[test]
fn a_template_in_its_traits_module_and_a_block_with_a_line_are_clean() {
    let main = format!(
        "{SHOW_TEMPLATE}
data Point:
    x: i64
    y: i64

impl Show for Point by Structure:
    x = []
"
    );
    clean(&[("main.hd", &main)]);
}

#[test]
fn a_trait_less_block_with_member_lines_is_clean() {
    let main = "use std.structure.Structure

data Box[T]:
    item: T

impl[T] Box[T] by Structure:
    item += []
    Self += []
";
    clean(&[("main.hd", main)]);
}

#[test]
fn a_renamed_trait_less_header_is_clean() {
    clean(&[(
        "main.hd",
        "use std.structure.Structure

data Box[T]:
    item: T

impl[U] Box[U] by Structure:
    item += []
",
    )]);
}

#[test]
fn a_template_outside_its_traits_module_is_misplaced() {
    let lib = "pub trait Show:
    fn show(self) -> string
";
    let main = "use pkg.lib.show.Show
use std.structure.Structure

impl[T] Show for T by Structure:
    fn show(self) -> string:
        \"x\"
";
    misplaced(&[("main.hd", main), ("lib/show.hd", lib)], main, &[4]);
}

#[test]
fn a_block_outside_its_targets_module_is_misplaced() {
    let lib = "pub data Point:
    pub x: i64
";
    let main = format!(
        "use pkg.lib.shapes.Point
{SHOW_TEMPLATE}
impl Show for Point by Structure:
    x = []
"
    );
    let at = main.lines().count() - 1;
    misplaced(&[("main.hd", &main), ("lib/shapes.hd", lib)], &main, &[at]);
}

#[test]
fn a_trait_less_block_outside_its_targets_module_is_misplaced() {
    let lib = "pub data Point:
    pub x: i64
";
    let main = "use pkg.lib.shapes.Point
use std.structure.Structure

impl Point by Structure:
    x = []
";
    misplaced(&[("main.hd", main), ("lib/shapes.hd", lib)], main, &[4]);
}

#[test]
fn a_member_line_in_a_template_is_misplaced() {
    let main = format!("{SHOW_TEMPLATE}    out = []\n");
    let at = main.lines().count();
    misplaced(&[("main.hd", &main)], &main, &[at]);
}

#[test]
fn a_rejected_block_reports_nothing_else() {
    // The newtype block would otherwise derive `Show` over a member that
    // cannot implement it.
    let main = format!(
        "{SHOW_TEMPLATE}
data Opaque:
    value: bool

type Wrapped(Opaque)

impl Show for Wrapped by Structure
"
    );
    let at = main.lines().count();
    misplaced(&[("main.hd", &main)], &main, &[at]);
}
