//! `dbg` (spec/lang/10-modules.md, "Debug Printing"; spec/cli/command-line.md,
//! "Debug Output") on V8: what it prints and where, what it returns, and how
//! `hd test` shows its lines. The spec's own fixtures are in
//! `spec/conformance/cli`; these cover the value forms they leave out.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("dir");
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"probe\"\n").expect("write");
    dir
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

fn hd(dir: &Path, args: &[&str]) -> Ran {
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.join("cache"))
        .env_remove("HD_JOBS")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run hd");
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Runs `src/main.hd` with `source`: its stdout and its stderr.
fn run_main(name: &str, source: &str) -> Ran {
    let dir = scratch(name);
    std::fs::write(dir.join("src/main.hd"), source).expect("write");
    hd(&dir, &["run"])
}

/// `dbg(x)` is a statement that returns nothing: the program's value is
/// unchanged, its line holds the location, the argument's source text and
/// its value, and it goes to standard error (`module.dbg.line`,
/// `cli.dbg.run`).
#[test]
fn dbg_prints_the_argument_text_and_its_value_to_stderr() {
    let ran = run_main(
        "hd-dbg-line",
        "\
fn line_total(price: i32, qty: i32) -> i32:
    dbg(price * qty)
    price * qty + 50

pub fn main() -> void $ Console:
    println(line_total(125, 10))
",
    );
    assert_eq!(ran.code, Some(0), "{}", ran.err);
    assert_eq!(ran.out, "1300\n");
    assert_eq!(ran.err, "src/main.hd:2:5: price * qty = 1250\n");
}

/// `dbg()` prints its location alone, and each argument of
/// `dbg(a, b)` a line of its own, in order (`module.dbg.body.print`).
#[test]
fn dbg_with_no_arguments_prints_its_location_and_with_several_a_line_each() {
    let ran = run_main(
        "hd-dbg-several",
        "\
pub fn main() -> void $ Console:
    dbg()
    dbg(1, \"two\", [3])
    dbg(
        1 +
            2,
    )
    println(\"done\")
",
    );
    assert_eq!(ran.code, Some(0), "{}", ran.err);
    assert_eq!(ran.out, "done\n");
    assert_eq!(
        ran.err,
        "\
src/main.hd:2:5
src/main.hd:3:5: 1 = 1
src/main.hd:3:5: \"two\" = \"two\"
src/main.hd:3:5: [3] = [3]
src/main.hd:4:5: 1 + 2 = 3
"
    );
}

/// A value whose type has no `Debug` prints its structure, each part by the
/// same rules, a recursive type through itself (`module.dbg.value.*`).
#[test]
fn dbg_prints_structure_for_types_without_debug() {
    let ran = run_main(
        "hd-dbg-structure",
        "\
data Node:
    label: string
    next: Node?

data Pair:
    left: List[Node]
    right: (i32, Node?)

enum Shape:
    Dot
    Pair(i32, string)
    Tagged(tag: bool)

pub fn main() -> void $ Console:
    list := Node { label: \"a\", next: .Some(Node { label: \"b\", next: .None }) }
    dbg(list)
    dbg(Pair { left: [list], right: (1, .None) })
    dbg([Shape.Dot, Shape.Pair(1, \"x\"), Shape.Tagged(tag=true)])
    dbg({ \"k\": Shape.Dot })
    let failed: Result[i32, Shape] = .Err(Shape.Dot)
    dbg(failed)
    println(\"done\")
",
    );
    assert_eq!(ran.code, Some(0), "{}", ran.err);
    assert_eq!(ran.out, "done\n");
    // A value wider than 80 columns lays out over lines; `{"k": ...}` fits.
    assert_eq!(
        ran.err,
        "\
src/main.hd:16:5: list = Node {
    label: \"a\",
    next: Option.Some(
        Node {
            label: \"b\",
            next: Option.None,
        },
    ),
}
src/main.hd:17:5: Pair { left: [list], right: (1, .None) } = Pair {
    left: [
        Node {
            label: \"a\",
            next: Option.Some(
                Node {
                    label: \"b\",
                    next: Option.None,
                },
            ),
        },
    ],
    right: (
        1,
        Option.None,
    ),
}
src/main.hd:18:5: [Shape.Dot, Shape.Pair(1, \"x\"), Shape.Tagged(tag=true)] = [
    Shape.Dot,
    Shape.Pair(
        1,
        \"x\",
    ),
    Shape.Tagged(
        tag=true,
    ),
]
src/main.hd:19:5: { \"k\": Shape.Dot } = {\"k\": Shape.Dot}
src/main.hd:21:5: failed = Result.Err(Shape.Dot)
"
    );
}

/// A long list prints its first 100 entries, then `… N more`
/// (`module.dbg.limit.entries`), over lines when the value does not fit
/// (`module.dbg.layout`).
#[test]
fn dbg_limits_a_long_list_and_lays_a_wide_value_over_lines() {
    let ran = run_main(
        "hd-dbg-large",
        "\
pub fn main() -> void $ Console:
    let items: mut List[i32] = []
    for n in 0..102:
        items.push(n)
    dbg(items)
",
    );
    assert_eq!(ran.code, Some(0), "{}", ran.err);
    let lines: Vec<&str> = ran.err.lines().collect();
    assert_eq!(lines.first(), Some(&"src/main.hd:5:5: items = ["));
    assert_eq!(lines.get(1), Some(&"    0,"));
    assert_eq!(lines.get(100), Some(&"    99,"));
    assert_eq!(lines.get(101), Some(&"    … 2 more"));
    assert_eq!(lines.last(), Some(&"]"));
}

/// A value reached again while it is printed prints `<cycle>`
/// (`module.dbg.cycle.tracked`).
#[test]
fn dbg_marks_a_cycle() {
    let ran = run_main(
        "hd-dbg-cycle",
        "\
data Ring:
    name: string
    next: mut Ring?

pub fn main() -> void $ Console:
    let a: mut Ring = Ring { name: \"a\", next: .None }
    let b: mut Ring = Ring { name: \"b\", next: .Some(a) }
    a.next = .Some(b)
    dbg(a)
",
    );
    assert_eq!(ran.code, Some(0), "{}", ran.err);
    // The line is wider than 80 columns, so it lays out over lines.
    assert_eq!(
        ran.err,
        "\
src/main.hd:9:5: a = Ring {
    name: \"a\",
    next: Option.Some(
        Ring {
            name: \"b\",
            next: Option.Some(
                <cycle>,
            ),
        },
    ),
}
"
    );
}

/// A generic function prints a value whose type has `Debug` by the bound in
/// scope (`module.dbg.value.generic`), and needs no requirement
/// (`module.dbg.no-requirement`).
#[test]
fn dbg_in_generic_code_prints_through_the_bound() {
    let ran = run_main(
        "hd-dbg-generic",
        "\
fn show[T < Debug](value: T) -> T:
    dbg(value)
    value

pub fn main() -> void $ Console:
    println(show(7))
",
    );
    assert_eq!(ran.code, Some(0), "{}", ran.err);
    assert_eq!(ran.out, "7\n");
    assert_eq!(ran.err, "src/main.hd:2:5: value = 7\n");
}

/// `hd test` shows a failing case's `dbg` lines with its failure and drops
/// a passing case's (`cli.dbg.test`).
#[test]
fn hd_test_shows_dbg_lines_of_a_failing_case_only() {
    let dir = scratch("hd-dbg-test");
    std::fs::write(
        dir.join("src/lib.hd"),
        "\
pub fn double(x: i32) -> i32:
    dbg(x)
    x * 2

tests:
    use std.testing.assert_equal

    it(\"passes quietly\"):
        assert_equal(double(2), 4, reason=\"double\")

    it(\"fails loudly\"):
        assert_equal(double(3), 7, reason=\"double\")
",
    )
    .expect("write");
    let ran = hd(&dir, &["test"]);
    assert_eq!(ran.code, Some(1), "{}", ran.err);
    assert_eq!(
        ran.out,
        "\
PANIC src/lib.hd:11: fails loudly
    panic: assertion-failed: double: actual 6, expected 7
    src/lib.hd:2:5: x = 3
    repro: hd test src/lib.hd --filter \"fails loudly\"
test result: FAILED. 1 passed; 1 failed; 0 ignored
"
    );
}
