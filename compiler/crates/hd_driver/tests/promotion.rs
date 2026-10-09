//! Member promotion through embedded parts (spec 03 "Depths And Promoted
//! Members", "Promoted Member Access"): promoted fields and methods read,
//! write and call through the embedded part and run correctly; a conflict
//! is `ambiguous-promoted-member` at the declaration, and a trait method
//! beside a promoted one is `ambiguous-method` at the call.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_main(main: &str, goal: &Goal) -> Output {
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
    build(&host, "app", goal)
}

fn errors(src: &str) -> Vec<Code> {
    let out = build_main(src, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// Runs the program's `tests:` block on the Node host; every test must pass.
fn run_tests(name: &str, src: &str) {
    let out = build_main(
        src,
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("promotion-{name}.wasm"));
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

const PARTS: &str = "\
data Clock:
    pub ticks: i32
    pub label: i32

impl Clock:
    pub fn read(self) -> i32:
        self.ticks

    pub fn advance(mut self, by: i32) -> void:
        self.ticks = self.ticks + by

data Device:
    Clock
    id: i32

data Rack:
    Device
    slot: i32
";

#[test]
fn promoted_fields_and_methods_run_through_the_part() {
    run_tests(
        "through",
        &format!(
        "use std.testing.assert_equal

{PARTS}
fn bump(rack: mut Rack) -> void:
    rack.ticks = rack.ticks + 1
    rack.advance(10)

tests:
    it(\"reads a promoted field and calls a promoted method\"):
        device := Device {{ Clock: ...Clock {{ ticks: 5, label: 9 }}, id: 2 }}
        assert_equal(device.ticks, 5, reason=\"depth 1 field\")
        assert_equal(device.read(), 5, reason=\"depth 1 method\")
        assert_equal(device.label, 9, reason=\"second promoted field\")
    it(\"writes a promoted field and calls a promoted mut method\"):
        let mut device = Device {{ Clock: ...Clock {{ ticks: 1, label: 0 }}, id: 2 }}
        device.ticks = 4
        device.advance(3)
        assert_equal(device.Clock.ticks, 7, reason=\"the part holds both writes\")
    it(\"promotes through two parts\"):
        let mut rack = Rack {{ Device: ...Device {{ Clock: ...Clock {{ ticks: 1, label: 0 }}, id: 2 }}, slot: 3 }}
        bump(rack)
        assert_equal(rack.ticks, 12, reason=\"depth 2 field and mut method\")
        assert_equal(rack.read(), 12, reason=\"depth 2 method\")
        assert_equal(rack.Device.Clock.ticks, 12, reason=\"explicit path\")
"
    ),
    );
}

#[test]
fn a_shallower_own_member_hides_a_promoted_one() {
    run_tests(
        "hides",
        &format!(
            "use std.testing.assert_equal

data Gauge:
    Clock
    pub ticks: i32

{PARTS}
tests:
    it(\"the own field wins\"):
        gauge := Gauge {{ Clock: ...Clock {{ ticks: 1, label: 0 }}, ticks: 8 }}
        assert_equal(gauge.ticks, 8, reason=\"own pub field hides the part's\")
        assert_equal(gauge.Clock.ticks, 1, reason=\"the part through its path\")
"
        ),
    );
}

#[test]
fn a_promoted_mut_method_needs_a_mutable_root() {
    let src = format!(
        "{PARTS}
fn invalid(device: Device) -> void:
    device.advance(1)

fn also_invalid(device: Device) -> void:
    device.ticks = 1

pub fn main() -> void:
    pass
"
    );
    let found = errors(&src);
    assert!(found.contains(&Code::MutableReceiverRequired), "{found:?}");
    assert!(found.contains(&Code::ReadonlyRoot), "{found:?}");
}

#[test]
fn two_parts_with_one_promoted_name_conflict_at_the_declaration() {
    let field = "\
data Left:
    pub id: i32

data Right:
    pub id: i32

data Both:
    Left
    Right

pub fn main() -> void:
    pass
";
    assert_eq!(errors(field), vec![Code::AmbiguousPromotedMember]);
    let method = "\
data Left:
    x: i32

impl Left:
    pub fn name(self) -> i32:
        1

data Right:
    y: i32

impl Right:
    pub fn name(self) -> i32:
        2

data Both:
    Left
    Right

pub fn main() -> void:
    pass
";
    assert_eq!(errors(method), vec![Code::AmbiguousPromotedMember]);
}

#[test]
fn a_private_own_member_beside_a_promoted_one_conflicts() {
    let src = "\
data Base:
    pub id: i32

data Record:
    Base
    id: i32

pub fn main() -> void:
    pass
";
    assert_eq!(errors(src), vec![Code::AmbiguousPromotedMember]);
    let hidden = "\
data Base:
    pub id: i32

data Record:
    Base
    pub id: i32

pub fn main() -> void:
    pass
";
    assert_eq!(errors(hidden), Vec::<Code>::new());
}

#[test]
fn a_trait_method_beside_a_promoted_method_is_ambiguous() {
    let src = "\
trait Describe:
    fn describe(self) -> i32

data Base:
    n: i32

impl Base:
    pub fn describe(self) -> i32:
        self.n

data Page:
    Base

impl Describe for Page:
    fn describe(self) -> i32:
        0

fn invalid(page: Page) -> i32:
    page.describe()

pub fn main() -> void:
    pass
";
    assert_eq!(errors(src), vec![Code::AmbiguousMethod]);
}
