//! `hd test` on `compiler/samples/testing` (M4c): exact output and exit
//! status of passing, failing, panicking and ignored cases, a body whose
//! `?` returns an error (`module.testing.err-print`),
//! `--filter`, a warm second run, and the same output on one thread and
//! on several (`cli.jobs.result`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/testing")
}

fn cache(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

fn hd_test(cache_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", cache_dir)
        .env_remove("HD_JOBS")
        .current_dir(package())
        .arg("test")
        .args(args)
        .output()
        .expect("run hd test")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Every case of the package, in content order: `cart` before `money`,
/// then registration order. Passes are quiet.
const ALL: &str = "\
PANIC cart.hd:50: sums a discount wrong
    panic: assertion-failed: ten less two: actual 8, expected 7
    repro: hd test cart.hd --filter \"sums a discount wrong\"
PANIC cart.hd:53: finds the cheapest of none
    panic: index-out-of-bounds: list index out of bounds
    repro: hd test cart.hd --filter \"finds the cheapest of none\"
FAIL cart.hd:59: expects a panic that never comes
    the case completed, but it expects the panic `assertion-failed`
    repro: hd test cart.hd --filter \"expects a panic that never comes\"
IGNORED cart.hd:62: waits for a slow price service (needs the price service)
PANIC cart.hd:65: adds two and two
    panic: assertion-failed: two and two make five
    repro: hd test cart.hd --filter \"adds two and two\"
FAIL cart.hd:68: parses quantities
    not a quantity: lots
    repro: hd test cart.hd --filter \"parses quantities\"
test result: FAILED. 6 passed; 5 failed; 1 ignored
";

#[test]
fn reports_each_outcome_cold_then_warm() {
    let dir = cache("hd-cache-test-all");
    let _ = std::fs::remove_dir_all(&dir);
    for round in ["cold", "warm"] {
        let out = hd_test(&dir, &[]);
        assert_eq!(text(&out.stdout), ALL, "{round}: {}", text(&out.stderr));
        assert_eq!(out.status.code(), Some(1), "{round}");
    }
}

#[test]
fn the_output_is_the_same_on_one_thread_and_on_several() {
    let dir = cache("hd-cache-test-jobs");
    let _ = std::fs::remove_dir_all(&dir);
    for jobs in ["1", "4"] {
        let out = hd_test(&dir, &["--jobs", jobs]);
        assert_eq!(text(&out.stdout), ALL, "--jobs {jobs}");
        assert_eq!(out.status.code(), Some(1));
    }
}

#[test]
fn filter_selects_cases() {
    let dir = cache("hd-cache-test-filter");
    let out = hd_test(&dir, &["--filter", "fresh start"]);
    assert_eq!(
        text(&out.stdout),
        "test result: ok. 2 passed; 0 failed; 0 ignored\n"
    );
    assert_eq!(out.status.code(), Some(0));
    let out = hd_test(&dir, &["cart.hd", "--filter", "cheapest"]);
    assert_eq!(
        text(&out.stdout),
        "PANIC cart.hd:53: finds the cheapest of none\n    \
         panic: index-out-of-bounds: list index out of bounds\n    \
         repro: hd test cart.hd --filter \"finds the cheapest of none\"\n\
         test result: FAILED. 0 passed; 1 failed; 0 ignored\n"
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_file_runs_its_module_and_a_missed_filter_is_an_error() {
    let dir = cache("hd-cache-test-file");
    let out = hd_test(&dir, &["money.hd"]);
    assert_eq!(
        text(&out.stdout),
        "test result: ok. 2 passed; 0 failed; 0 ignored\n"
    );
    assert_eq!(out.status.code(), Some(0));
    // cli.test.filter.none
    let out = hd_test(&dir, &["money.hd", "--filter", "euro"]);
    assert_eq!(out.status.code(), Some(101));
    assert_eq!(
        text(&out.stderr),
        "error: no test case of `money.hd` has a name that contains `euro`\n"
    );
    // cli.command.positional
    let out = hd_test(&dir, &["."]);
    assert_eq!(out.status.code(), Some(101));
}

/// `cli.json.test.result`, `cli.json.test.order`: one test object per case
/// in content order, then the summary with the counts and status 1 when a
/// case failed. A body that returns an `.Err` fails with the error's
/// report as its message, on standard output (`module.testing.err-print`).
#[test]
fn json_lists_test_objects_then_the_summary() {
    let out = hd_test(&cache("hd-cache-test-json"), &["--format", "json"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 13, "{stdout}");
    assert!(
        lines[..12]
            .iter()
            .all(|l| l.starts_with("{\"kind\":\"test\",\"name\":"))
    );
    assert!(
        stdout.contains("{\"kind\":\"test\",\"name\":\"sums a discount wrong\",\"outcome\":\"failed\",\"message\":\"panic: assertion-failed: ten less two: actual 8, expected 7\"}"),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"name\":\"waits for a slow price service\",\"outcome\":\"ignored\",\"message\":\"needs the price service\"}"),
        "{stdout}"
    );
    assert!(
        stdout.contains("{\"kind\":\"test\",\"name\":\"parses quantities\",\"outcome\":\"failed\",\"message\":\"not a quantity: lots\"}"),
        "{stdout}"
    );
    assert_eq!(
        lines[12],
        "{\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":6,\"failed\":5,\"ignored\":1,\"status\":1}"
    );
}
