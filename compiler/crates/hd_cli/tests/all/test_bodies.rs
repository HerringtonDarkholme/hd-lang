//! Test bodies (spec/lang/10-modules.md "Test Cases", "Test Outcomes";
//! spec/lang/05-expressions.md "Propagation In Test Blocks"): `hd test`
//! runs each form of an `it` body and reports its outcome, and checking
//! holds each form to its rules.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh package `notes` whose `src/lib.hd` is `lib`.
fn package(name: &str, lib: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("cache"));
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"notes\"\n").expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
    root
}

fn hd(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", root.with_extension("cache"))
        .env_remove("HD_JOBS")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run hd")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Errors with a cause, which a `?` in a test body propagates.
const ERRORS: &str = "\
@error(\"disk full\")
pub data DiskError:
    free: i64

@error
pub enum SaveError:
    @error(\"cannot save $path\")
    Write(path: string, @source error: DiskError)

pub fn save(free: i64) -> Result[void, SaveError]:
    if free > 0: .Ok(()) else: .Err(SaveError.Write(\"notes.txt\", DiskError { free: free }))
";

/// `expr.try.test.with-try`, `module.testing.err-print`: a body that uses
/// `?` returns `Result[void, dyn Error]`. `.Ok(())` passes; an `.Err` fails,
/// and the runner prints the error and its cause chain in its own report on
/// standard output, for an `it` body and an `it_each` row alike.
#[test]
fn a_try_body_passes_on_ok_and_reports_an_err_with_its_causes() {
    let lib = format!(
        "{ERRORS}
tests:
    use std.testing.it_each

    it(\"saves with room\"):
        save(10)?
        .Ok(())

    it(\"saves on a full disk\"):
        save(0)?
        .Ok(())

    it_each(\"saves each\", [5, 0], body=fn!(free: i64):
        save(free)?
        .Ok(())
    )
"
    );
    let root = package("test-bodies-try", &lib);
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
FAIL src/lib.hd:20: saves on a full disk
    cannot save notes.txt
    caused by: disk full
    repro: hd test src/lib.hd --filter \"saves on a full disk\"
FAIL src/lib.hd:24: saves each[1]
    cannot save notes.txt
    caused by: disk full
    repro: hd test src/lib.hd --filter \"saves each[1]\"
test result: FAILED. 2 passed; 2 failed; 0 ignored
"
    );
    assert_eq!(text(&out.stderr), "");
    assert_eq!(out.status.code(), Some(1));
    let out = hd(
        &root,
        &["test", "--format", "json", "--filter", "full disk"],
    );
    assert_eq!(
        text(&out.stdout),
        "\
{\"kind\":\"test\",\"name\":\"saves on a full disk\",\"outcome\":\"failed\",\"message\":\"cannot save notes.txt\\ncaused by: disk full\"}
{\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":0,\"failed\":1,\"ignored\":0,\"status\":1}
"
    );
    assert_eq!(text(&out.stderr), "");
}

/// `module.testing.it.form`, `expr.try.test.explicit-closure`: an explicit
/// body, given by name or after the name, keeps its own `Termination`
/// result. `ExitCode(0)` and `.Ok(())` pass, another code and an `.Err`
/// fail. A plain function and a non-suspending closure are bodies too, a
/// body with a `timeout` reports it first, and a body whose row holds
/// `TestRunner` runs in a unit test (`module.testing.unit-row.test-runner`).
#[test]
fn an_explicit_body_keeps_its_own_result() {
    let lib = "\
use std.process.ExitCode
use std.testing.TestRunner
use std.time.s

fn check_port(port: i32) -> Result[void, string]:
    if port > 0: .Ok(()) else: .Err(\"bad port: ${port}\")

fn exit_zero() -> ExitCode:
    ExitCode(0)

fn first_row(rows: List[string]) -> string $ TestRunner:
    rows[$.use(TestRunner).row(rows.len())]

tests:
    use std.testing.assert_equal

    it(\"passes with code zero\", body=fn!() -> ExitCode: ExitCode(0))

    it(\"fails with code three\", body=fn!() -> ExitCode: ExitCode(3))

    it(\"passes an ok port\", fn!() -> Result[void, string]: check_port(80))

    it(\"fails a bad port\", body=fn!() -> Result[void, string]: check_port(0))

    it(\"takes a function\", body=exit_zero)

    it(\"takes a plain closure\", body=fn(): assert_equal(1 + 1, 2, reason=\"sums\"))

    it(\"takes a timeout\", timeout=5s, body=fn!(): assert_equal(2 + 2, 4, reason=\"sums\"))

    it(\"uses the runner\", body=fn!() -> void $ TestRunner: _ := first_row([\"a\"]))
";
    let root = package("test-bodies-explicit", lib);
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
FAIL src/lib.hd:19: fails with code three
    `report()` returned `ExitCode(3)`
    repro: hd test src/lib.hd --filter \"fails with code three\"
FAIL src/lib.hd:23: fails a bad port
    bad port: 0
    repro: hd test src/lib.hd --filter \"fails a bad port\"
test result: FAILED. 6 passed; 2 failed; 0 ignored
",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(text(&out.stderr), "");
    assert_eq!(out.status.code(), Some(1));
}

/// Checking an explicit body: its result must implement `Termination`
/// (`expr.try.test.explicit-closure`), its row may hold only what the
/// runner binds for a unit test (`module.testing.unit-row.test-runner`),
/// and `it` takes exactly one body.
#[test]
fn an_explicit_body_is_checked() {
    for (body, code) in [
        (
            "it(\"an optional result\", body=fn!() -> i32?: .None)",
            "unsatisfied-trait-bound",
        ),
        (
            "it(\"prints\", body=fn!() -> void $ Console: println(1))",
            "missing-requirement",
        ),
        ("it(\"no body\")", "argument-count"),
        (
            "it(\"two bodies\", fn!(): pass, body=fn!(): pass)",
            "duplicate-argument",
        ),
    ] {
        let root = package("test-bodies-checked", &format!("tests:\n    {body}\n"));
        let out = hd(&root, &["check", "--tests"]);
        let err = text(&out.stderr);
        assert_eq!(out.status.code(), Some(101), "{body}: {err}");
        assert!(err.contains(code), "{body}: {err}");
    }
}

/// `std-testing.option.timeout-any-duration`, `.timeout-at-run`: the case
/// of an explicit body evaluates its `timeout` before it calls the body,
/// so a body that runs past it fails with `time-limit`, and one that uses
/// `?` within its budget passes.
#[test]
fn an_explicit_body_runs_under_its_timeout() {
    let lib = format!(
        "{ERRORS}
use std.time.{{Duration, h, ms}}

fn budget() -> Duration: 1h

fn forever(stop: i32) -> i32:
    let total: i32 = 0
    while total != stop:
        total = (total + 1) % 1000
    total

tests:
    it(\"spins\", timeout=100ms, body=fn!(): _ := forever(-1))

    it(\"saves in time\", timeout=budget(), body=fn!() -> Result[void, SaveError]: save(1))
"
    );
    let root = package("test-bodies-timeout", &lib);
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
PANIC src/lib.hd:24: spins
    panic: time-limit: the test case ran longer than its timeout of 100ms
    repro: hd test src/lib.hd --filter \"spins\"
test result: FAILED. 1 passed; 1 failed; 0 ignored
",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1));
}
