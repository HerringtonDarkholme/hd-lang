//! Method choice among several traits (spec 03 "Method Lookup", spec 09
//! "Trait Availability", "Inherent Methods Win", "Ambiguous Methods"): an
//! inherent method wins, two available traits with the method are
//! `ambiguous-method`, a trait-qualified call or a bound resolves it, and a
//! trait that is not available is not a candidate at all.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const TRAITS: &str = "\
trait Left:
    fn label(self) -> i32

trait Right:
    fn label(self) -> i32

data User:
    value: i32

impl Left for User:
    fn label(self) -> i32: self.value

impl Right for User:
    fn label(self) -> i32: self.value + 100
";

fn build_files(files: &[(&str, &str)], goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn analyze(files: &[(&str, &str)]) -> Output {
    build_files(files, &Goal::Analyze)
}

fn errors(out: &Output) -> Vec<Code> {
    (0..out.diags.len())
        .filter(|&i| out.diags.severity[i] == Severity::Error)
        .map(|i| out.diags.code[i])
        .collect()
}

#[track_caller]
fn accepts(files: &[(&str, &str)]) {
    let out = analyze(files);
    assert_eq!(errors(&out), Vec::<Code>::new(), "{}", out.render());
}

#[track_caller]
fn rejects(files: &[(&str, &str)], code: Code) -> Output {
    let out = analyze(files);
    assert_eq!(errors(&out), vec![code], "{}", out.render());
    out
}

fn main_with(body: &str) -> String {
    format!("{TRAITS}\n{body}\npub fn main() -> void:\n    pass\n")
}

#[test]
fn an_inherent_method_wins_over_both_traits() {
    let src = main_with(
        "impl User:
    fn label(self) -> i32: 7

fn pick(user: User) -> i32:
    user.label()
",
    );
    accepts(&[("main.hd", &src)]);
}

#[test]
fn two_available_traits_with_the_method_are_ambiguous() {
    let src = main_with(
        "fn pick(user: User) -> i32:
    user.label()
",
    );
    let out = rejects(&[("main.hd", &src)], Code::AmbiguousMethod);
    let text = out.render();
    assert!(text.contains("Left") && text.contains("Right"), "{text}");
}

#[test]
fn a_qualified_call_resolves_the_ambiguity() {
    let src = main_with(
        "fn pick(user: User) -> i32:
    Left::label(user) + Right::label(user)
",
    );
    accepts(&[("main.hd", &src)]);
}

#[test]
fn a_bound_narrows_the_choice_to_its_trait() {
    let src = main_with(
        "fn pick[T < Left](x: T) -> i32:
    x.label()

fn both(user: User) -> i32:
    pick(user)
",
    );
    accepts(&[("main.hd", &src)]);
}

const SHELF: &str = "\
pub trait Tagged:
    fn tag(self) -> i32

pub data Item:
    pub value: i32

impl Tagged for Item:
    fn tag(self) -> i32: self.value
";

#[test]
fn a_trait_that_is_not_available_is_not_a_candidate() {
    let src = "\
use pkg.shelf.{Item}

fn read(item: Item) -> i32:
    item.tag()

pub fn main() -> void:
    pass
";
    let out = rejects(
        &[("shelf.hd", SHELF), ("main.hd", src)],
        Code::UnknownMethod,
    );
    let text = out.render();
    assert!(text.contains("Tagged") && text.contains("use"), "{text}");
}

#[test]
fn importing_the_trait_makes_its_method_a_candidate() {
    let src = "\
use pkg.shelf.{Item, Tagged}

fn read(item: Item) -> i32:
    item.tag()

pub fn main() -> void:
    pass
";
    accepts(&[("shelf.hd", SHELF), ("main.hd", src)]);
}

#[test]
fn an_unavailable_trait_does_not_make_an_available_one_ambiguous() {
    let src = "\
use pkg.shelf.{Item}

trait Mine:
    fn tag(self) -> i32

impl Mine for Item:
    fn tag(self) -> i32: 1

fn read(item: Item) -> i32:
    item.tag()

pub fn main() -> void:
    pass
";
    accepts(&[("shelf.hd", SHELF), ("main.hd", src)]);
}

/// Runs the program's `tests:` block on the Node host; every test must pass.
fn run_tests(name: &str, files: &[(&str, &str)]) {
    let out = build_files(
        files,
        &Goal::Tests {
            module: None,
            filter: None,
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let runs: Vec<String> = out
        .tests
        .iter()
        .map(|t| {
            let (test, init) = t.run.expect("a runnable test");
            format!("{test}:{init}")
        })
        .collect();
    assert!(!runs.is_empty(), "no tests ran");
    let wasm = out.wasm.expect("wasm");
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("method-choice-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let result = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/test.mjs"))
        .arg(&path)
        .arg(runs.join(","))
        .output()
        .expect("node");
    let _ = std::fs::remove_file(path);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        runs.len(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for line in lines {
        assert!(line.contains("\"status\":0,"), "{line}");
        assert!(!line.contains("\"trapped\":true"), "{line}");
    }
}

#[test]
fn the_chosen_method_runs() {
    let src = format!(
        "use std.testing.assert_equal
use pkg.shelf.{{Item}}

{TRAITS}
data Gauge:
    reading: i32

impl Gauge:
    fn label(self) -> i32: self.reading

impl Left for Gauge:
    fn label(self) -> i32: 1

impl Right for Gauge:
    fn label(self) -> i32: 2

trait Mine:
    fn tag(self) -> i32

impl Mine for Item:
    fn tag(self) -> i32: 40

fn through[T < Right](x: T) -> i32:
    x.label()

tests:
    it(\"an inherent method wins\"):
        assert_equal(Gauge {{ reading: 9 }}.label(), 9, reason=\"inherent\")
    it(\"a qualified call picks its trait\"):
        user := User {{ value: 5 }}
        assert_equal(Left::label(user), 5, reason=\"Left\")
        assert_equal(Right::label(user), 105, reason=\"Right\")
    it(\"a bound picks its trait\"):
        assert_equal(through(User {{ value: 5 }}), 105, reason=\"bound Right\")
    it(\"an unavailable trait is skipped\"):
        assert_equal(Item {{ value: 2 }}.tag(), 40, reason=\"Mine, not Tagged\")
"
    );
    run_tests("chosen", &[("shelf.hd", SHELF), ("main.hd", &src)]);
}
