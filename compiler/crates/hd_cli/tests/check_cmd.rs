//! `hd check` (`cli.check.*`, `cli.file.check-test`, `cli.json.*`): package
//! and file mode, the exit status, the JSON lines, and the warm second run.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// A package `shop` whose `main.hd` holds `main_source`.
fn package(name: &str, main_source: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("write");
    std::fs::write(dir.join("main.hd"), main_source).expect("write");
    dir
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

fn check(dir: &Path, cache: &str, args: &[&str]) -> Ran {
    let cache = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(cache);
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", cache)
        .current_dir(dir)
        .arg("check")
        .args(args)
        .output()
        .expect("run hd");
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

const CLEAN: &str = "fn main() -> void $ Console:\n    println(\"ok\")\n";
const BROKEN: &str = "fn main() -> void $ Console:\n    println(missing)\n";

/// The number after `"modules_checked":` in a summary line.
fn modules_checked(out: &str) -> u32 {
    let tail = out
        .split("\"modules_checked\":")
        .nth(1)
        .expect("modules_checked");
    tail.trim_end_matches(['}', '\n']).parse().expect("count")
}

#[test]
fn clean_package_exits_zero() {
    let dir = package("hd-check-clean", CLEAN);
    let r = check(&dir, "hd-check-cache-clean", &[]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.err, "check result: ok. errors: 0; warnings: 0\n");
    assert_eq!(r.out, "");
    assert!(!dir.join("build").exists(), "check writes no build output");
}

#[test]
fn package_error_exits_101_and_prints_it() {
    let dir = package("hd-check-error", BROKEN);
    let r = check(&dir, "hd-check-cache-error", &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(
        r.err
            .contains("error: main.hd:2:13: unknown-name: `missing` is not defined"),
        "{}",
        r.err
    );
}

#[test]
fn check_works_from_a_directory_below_the_package() {
    let dir = package("hd-check-below", BROKEN);
    std::fs::create_dir_all(dir.join("docs")).expect("dir");
    let r = check(&dir.join("docs"), "hd-check-cache-below", &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.err.contains("main.hd:2:13"), "{}", r.err);
}

#[test]
fn file_outside_a_package_is_a_single_file_program() {
    let dir = scratch("hd-check-file");
    std::fs::write(dir.join("ok.hd"), CLEAN).expect("write");
    std::fs::write(dir.join("bad.hd"), BROKEN).expect("write");
    let ok = check(&dir, "hd-check-cache-file", &["ok.hd"]);
    assert_eq!(ok.code, Some(0), "{}", ok.err);
    let bad = check(&dir, "hd-check-cache-file", &["bad.hd"]);
    assert_eq!(bad.code, Some(101), "{}", bad.err);
    assert!(bad.err.contains("bad.hd:2:13: unknown-name"), "{}", bad.err);
}

#[test]
fn no_file_outside_a_package_suggests_a_file_or_hd_new() {
    let dir = scratch("hd-check-nopkg");
    let r = check(&dir, "hd-check-cache-nopkg", &[]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("hd new"), "{}", r.err);
    assert!(r.err.contains("FILE"), "{}", r.err);
}

#[test]
fn unknown_option_is_a_usage_error() {
    let dir = package("hd-check-usage", CLEAN);
    let r = check(&dir, "hd-check-cache-usage", &["--bogus"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("--bogus"), "{}", r.err);
}

#[test]
fn json_lines_have_diagnostics_then_a_summary() {
    let dir = package("hd-check-json", BROKEN);
    let r = check(&dir, "hd-check-cache-json", &["--format", "json"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert_eq!(r.err, "");
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", r.out);
    assert!(
        lines[0].starts_with(
            "{\"kind\":\"diagnostic\",\"code\":\"unknown-name\",\"severity\":\"error\",\"message\":\""
        ),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].ends_with("\"file\":\"main.hd\",\"line\":2,\"column\":13,\"fixes\":[]}"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with(
            "{\"kind\":\"summary\",\"errors\":1,\"warnings\":0,\"passed\":0,\"failed\":0,\"ignored\":0,\"status\":101,\"modules_checked\":"
        ),
        "{}",
        lines[1]
    );
}

#[test]
fn json_summary_of_a_clean_package_is_written_on_success() {
    let dir = package("hd-check-json-clean", CLEAN);
    let r = check(&dir, "hd-check-cache-json-clean", &["--format=json"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.out.lines().count(), 1, "{}", r.out);
    assert!(
        r.out.contains("\"errors\":0,\"warnings\":0") && r.out.contains("\"status\":0"),
        "{}",
        r.out
    );
}

#[test]
fn json_file_outside_a_package_is_the_path_as_written() {
    let dir = scratch("hd-check-json-file");
    std::fs::write(dir.join("bad.hd"), BROKEN).expect("write");
    let r = check(
        &dir,
        "hd-check-cache-json-file",
        &["--format", "json", "bad.hd"],
    );
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.out.contains("\"file\":\"bad.hd\""), "{}", r.out);
}

#[test]
fn second_run_is_warm() {
    let _ = std::fs::remove_dir_all(
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hd-check-cache-warm"),
    );
    let dir = package("hd-check-warm", CLEAN);
    let args = ["--format", "json"];
    let cold = check(&dir, "hd-check-cache-warm", &args);
    assert_eq!(cold.code, Some(0), "{}", cold.err);
    let warm = check(&dir, "hd-check-cache-warm", &args);
    assert_eq!(warm.code, Some(0), "{}", warm.err);
    assert!(modules_checked(&cold.out) > 0, "{}", cold.out);
    assert_eq!(modules_checked(&warm.out), 0, "{}", warm.out);
}

/// A library package with one clean module and three modules with problems:
/// `a.hd` has two `unknown-name` errors and a warning, `b.hd` a
/// `type-mismatch` error, and `c.hd` an `unknown-name` error.
fn broken_library(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"lib\"\n").expect("write");
    std::fs::create_dir_all(dir.join("src")).expect("dir");
    let files = [
        ("util.hd", "pub fn double(v: i32) -> i32:\n    v * 2\n"),
        (
            "a.hd",
            "pub fn f() -> i32:\n    unused := +1\n    one\n\npub fn g() -> i32:\n    two\n",
        ),
        ("b.hd", "pub fn h() -> i32:\n    \"text\"\n"),
        ("c.hd", "pub fn k() -> i32:\n    three\n"),
    ];
    for (file, text) in files {
        std::fs::write(dir.join("src").join(file), text).expect("write");
    }
    dir
}

#[test]
fn text_summary_line_says_ok_or_failed_with_both_counts() {
    let dir = broken_library("hd-check-line");
    let bad = check(&dir, "hd-check-cache-line", &[]);
    assert_eq!(bad.code, Some(101), "{}", bad.err);
    assert_eq!(bad.out, "");
    assert_eq!(
        bad.err.lines().last(),
        Some("check result: FAILED. errors: 4; warnings: 1"),
        "{}",
        bad.err
    );
    let ok = check(&dir, "hd-check-cache-line", &["src/util.hd"]);
    assert_eq!(ok.code, Some(0), "{}", ok.err);
    assert_eq!(ok.err, "check result: ok. errors: 0; warnings: 0\n");
}

#[test]
fn summary_mode_counts_per_file_and_code_in_path_then_code_order() {
    let dir = broken_library("hd-check-summary");
    let r = check(&dir, "hd-check-cache-summary", &["--summary"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert_eq!(r.out, "");
    assert_eq!(
        r.err,
        "error: src/a.hd: unknown-name: 2\n\
         warning: src/a.hd: unused-local-binding: 1\n\
         error: src/b.hd: type-mismatch: 1\n\
         error: src/c.hd: unknown-name: 1\n\
         check result: FAILED. errors: 4; warnings: 1\n"
    );
}

#[test]
fn max_errors_stops_printing_but_not_counting_or_the_status() {
    let dir = broken_library("hd-check-max");
    let all = check(&dir, "hd-check-cache-max", &[]);
    let two = check(&dir, "hd-check-cache-max", &["--max-errors", "2"]);
    assert_eq!(two.code, all.code);
    assert_eq!(two.code, Some(101));
    let shown = two.err.lines().filter(|l| l.contains(": src/")).count();
    let errors = two.err.lines().filter(|l| l.starts_with("error: ")).count();
    assert_eq!(errors, 2, "{}", two.err);
    assert!(shown < all.err.lines().filter(|l| l.contains(": src/")).count());
    assert_eq!(
        two.err.lines().last(),
        Some("check result: FAILED. errors: 4; warnings: 1")
    );
    let eq = check(&dir, "hd-check-cache-max", &["--max-errors=2"]);
    assert_eq!(eq.err, two.err);
    let big = check(&dir, "hd-check-cache-max", &["--max-errors", "9"]);
    assert_eq!(big.err, all.err);
}

#[test]
fn max_errors_rejects_zero_and_text() {
    let dir = broken_library("hd-check-max-bad");
    for bad in ["0", "many"] {
        let r = check(&dir, "hd-check-cache-max-bad", &["--max-errors", bad]);
        assert_eq!(r.code, Some(101), "{}", r.err);
        assert!(r.err.contains("--max-errors"), "{}", r.err);
    }
}

#[test]
fn json_ignores_summary_and_max_errors() {
    let dir = broken_library("hd-check-json-opts");
    let plain = check(&dir, "hd-check-cache-json-opts", &["--format", "json"]);
    let opts = check(
        &dir,
        "hd-check-cache-json-opts",
        &["--format", "json", "--summary", "--max-errors", "1"],
    );
    assert_eq!(plain.out.lines().count(), 6, "{}", plain.out);
    assert_eq!(
        plain.out.lines().take(5).collect::<Vec<_>>(),
        opts.out.lines().take(5).collect::<Vec<_>>()
    );
}

#[test]
fn file_in_a_package_reports_the_modules_it_uses_and_not_the_others() {
    let dir = broken_library("hd-check-file-uses");
    std::fs::write(
        dir.join("src/user.hd"),
        "use pkg.b.{h}\n\npub fn u() -> i32:\n    h()\n",
    )
    .expect("write");
    let r = check(&dir, "hd-check-cache-file-uses", &["src/user.hd"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.err.contains("src/b.hd:2:5: type-mismatch"), "{}", r.err);
    assert!(!r.err.contains("src/a.hd"), "{}", r.err);
    assert!(!r.err.contains("src/c.hd"), "{}", r.err);
    assert_eq!(
        r.err.lines().last(),
        Some("check result: FAILED. errors: 1; warnings: 0")
    );
}

/// A package whose manifest has the `invalid-requirement` of a github.com
/// path without a repository on line 5 and a requirement on line 6 that
/// has no `hd.sum` entry.
fn bad_manifest(name: &str) -> PathBuf {
    let dir = package(name, BROKEN);
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[dependencies]\njson = \"github.com/acme@2.1.0\"\nyaml = \"github.com/acme/yaml@1.0.0\"\n",
    )
    .expect("write");
    dir
}

#[test]
fn manifest_errors_stop_the_check_before_compiling() {
    let dir = bad_manifest("hd-check-manifest");
    let r = check(&dir, "hd-check-cache-manifest", &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    let lines: Vec<&str> = r.err.lines().collect();
    assert_eq!(lines.len(), 3, "{}", r.err);
    assert!(
        lines[0].starts_with("error: hd.toml:5:1: invalid-requirement: "),
        "{}",
        r.err
    );
    assert!(
        lines[1].starts_with("error: hd.toml:6:1: missing-sum-entry: ")
            && lines[1].contains("hd fetch")
            && lines[1].contains("hd add"),
        "{}",
        r.err
    );
    assert_eq!(lines[2], "check result: FAILED. errors: 2; warnings: 0");
    let json = check(&dir, "hd-check-cache-manifest", &["--format", "json"]);
    assert_eq!(json.code, Some(101), "{}", json.err);
    let lines: Vec<&str> = json.out.lines().collect();
    assert_eq!(lines.len(), 3, "{}", json.out);
    assert!(
        lines[1].contains("\"code\":\"missing-sum-entry\"")
            && lines[1].contains("\"file\":\"hd.toml\",\"line\":6,"),
        "{}",
        lines[1]
    );
    assert!(lines[2].contains("\"errors\":2"), "{}", lines[2]);
}

#[test]
fn json_mode_reports_a_command_error_as_a_diagnostic_with_no_code() {
    let dir = scratch("hd-check-json-nopkg");
    let r = check(&dir, "hd-check-cache-json-nopkg", &["--format", "json"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert_eq!(r.err, "");
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", r.out);
    assert!(
        lines[0].starts_with("{\"kind\":\"diagnostic\",\"code\":null,\"severity\":\"error\"")
            && lines[0].contains("\"file\":null,\"line\":null,\"column\":null"),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains("\"status\":101"), "{}", lines[1]);
}

/// A package with one error in each kind of code: a `tests:` block, a test
/// module, an integration test file, and a task.
fn code_kinds(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("write");
    let files = [
        (
            "src/lib.hd",
            "pub fn double(v: i32) -> i32:\n    v * 2\n\ntests:\n    it(\"block\"):\n        _ := in_block\n",
        ),
        ("src/lib_test.hd", "fn helper() -> i32:\n    in_unit\n"),
        (
            "tests/flow.hd",
            "fn main() -> void:\n    _ := in_integration\n",
        ),
        ("tasks/seed.hd", "fn main() -> void:\n    _ := in_task\n"),
    ];
    for (file, text) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    dir
}

/// The names the errors of a text report are about, in order.
fn missing(err: &str) -> Vec<&str> {
    err.lines().filter_map(|l| l.split('`').nth(1)).collect()
}

/// `cli.check.default`, `cli.check.tests`, `cli.check.all`: plain `hd check`
/// checks no test code and no task; `--tests` adds the `tests:` blocks,
/// test modules and integration tests; `--all` adds the tasks. A FILE of
/// test code or a task is checked as that kind.
#[test]
fn tests_and_all_widen_what_check_covers() {
    let dir = code_kinds("hd-check-kinds");
    let cache = "hd-check-cache-kinds";
    let plain = check(&dir, cache, &[]);
    assert_eq!(plain.code, Some(0), "{}", plain.err);
    let tests = check(&dir, cache, &["--tests"]);
    assert_eq!(tests.code, Some(101));
    assert_eq!(
        missing(&tests.err),
        ["in_block", "in_unit", "in_integration"],
        "{}",
        tests.err
    );
    let all = check(&dir, cache, &["--all"]);
    assert_eq!(
        missing(&all.err),
        ["in_block", "in_unit", "in_task", "in_integration"],
        "{}",
        all.err
    );
    let task = check(&dir, cache, &["tasks/seed.hd"]);
    assert_eq!(missing(&task.err), ["in_task"], "{}", task.err);
    let flow = check(&dir, cache, &["tests/flow.hd"]);
    assert_eq!(missing(&flow.err), ["in_integration"], "{}", flow.err);
}

/// `module.test.integration.no-path`, `cli.task.no-path`: test and task
/// code has no module path, so no message names one; a use that misses
/// names the directory instead.
#[test]
fn messages_name_test_and_task_code_by_file() {
    let dir = code_kinds("hd-check-no-path");
    std::fs::write(dir.join("tasks/seed.hd"), "use self.nothing\n").expect("write");
    std::fs::create_dir_all(dir.join("tests/common")).expect("dir");
    std::fs::write(dir.join("tests/common/mod.hd"), "use super.up\n").expect("write");
    for format in ["text", "json"] {
        let r = check(
            &dir,
            "hd-check-cache-no-path",
            &["--all", "--format", format],
        );
        assert_eq!(r.code, Some(101));
        let all = format!("{}{}", r.out, r.err);
        assert!(!all.contains('$'), "{all}");
        assert!(all.contains("no module named `tasks`"), "{all}");
        assert!(all.contains("no module named `tests`"), "{all}");
    }
}

/// `module.test.dev-dependency`, `module.test.non-test-use.dev-dependency`:
/// a test module may use a dev dependency, and library code may not.
#[test]
fn dev_dependencies_are_for_test_code_only() {
    let dir = scratch("hd-check-dev");
    for (file, text) in [
        (
            "hd.toml",
            "[package]\nname = \"shop\"\n\n[dev-dependencies]\nfixtures = { path = \"fixtures\" }\n",
        ),
        ("fixtures/hd.toml", "[package]\nname = \"fixtures\"\n"),
        ("fixtures/src/lib.hd", "pub fn sample() -> i32:\n    2\n"),
        ("src/lib.hd", "pub fn double(v: i32) -> i32:\n    v * 2\n"),
        (
            "src/lib_test.hd",
            "use dep.fixtures.{sample}\nuse pkg.{double}\n\nfn four() -> i32:\n    double(sample())\n",
        ),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    let r = check(&dir, "hd-check-cache-dev", &["--tests"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    std::fs::write(
        dir.join("src/lib.hd"),
        "use dep.fixtures.{sample}\n\npub fn double(v: i32) -> i32:\n    v * sample()\n",
    )
    .expect("write");
    let r = check(&dir, "hd-check-cache-dev", &[]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.starts_with("error: src/lib.hd:1:1:"), "{}", r.err);
}
