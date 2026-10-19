//! `hd check` (`cli.check.*`, `cli.file.check-test`, `cli.json.*`): package
//! and file mode, the exit status, the JSON lines, and the warm second run.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

fn scratch(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), name)
}

/// A package `shop` whose `main.hd` holds `main_source`.
fn package(name: &str, main_source: &str) -> Scratch {
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

fn check(dir: &Scratch, args: &[&str]) -> Ran {
    check_at(dir, dir, args)
}

/// `hd check` run in `cwd`, with the build cache of `dir`.
fn check_at(cwd: &Path, dir: &Scratch, args: &[&str]) -> Ran {
    let cache = dir.cache();
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", cache)
        .current_dir(cwd)
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

const CLEAN: &str = "pub fn main() -> void $ Console:\n    println(\"ok\")\n";
const BROKEN: &str = "pub fn main() -> void $ Console:\n    println(missing)\n";

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
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.err, "check result: ok. errors: 0; warnings: 0\n");
    assert_eq!(r.out, "");
    assert!(!dir.join("build").exists(), "check writes no build output");
}

#[test]
fn package_error_exits_101_and_prints_it() {
    let dir = package("hd-check-error", BROKEN);
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(
        r.err
            .contains("error: main.hd:2:13: unknown-name: `missing` is not defined"),
        "{}",
        r.err
    );
}

/// A program the checker cannot take yet: a variant pattern with a rest
/// (`Shape.Rect(w, rest...)`). `hd build` stops on it with the same error.
const UNSUPPORTED: &str = "enum Shape:\n    Rect(w: i32, h: i32)\n\nfn area(s: Shape) -> i32:\n    match s:\n        Shape.Rect(w, rest...) => w\n\npub fn main() -> void $ Console:\n    println(area(Shape.Rect(2, 3)))\n";

#[test]
fn unsupported_program_fails_the_check() {
    let dir = package("hd-check-unsupported", UNSUPPORTED);
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert_eq!(
        r.err,
        "error: main.hd:5:5: unsupported: stage Body: a variant pattern with a rest is not supported\n\
         check result: FAILED. errors: 1; warnings: 0\n"
    );
    assert!(!dir.join("build").exists(), "check writes no build output");
}

#[test]
fn unsupported_program_fails_the_check_in_json() {
    let dir = package("hd-check-unsupported-json", UNSUPPORTED);
    let r = check(&dir, &["--format", "json"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", r.out);
    assert!(
        lines[0].starts_with(
            "{\"kind\":\"diagnostic\",\"code\":\"unsupported\",\"severity\":\"error\",\"message\":\"stage Body: "
        ),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("{\"kind\":\"summary\",\"errors\":1,\"warnings\":0,"),
        "{}",
        lines[1]
    );
}

#[test]
fn an_error_in_a_header_adds_no_unsupported_for_its_methods() {
    let source = "data Counter:\n    current: i32\n\nimpl Missing for Counter:\n    fn next(self) -> i32: self.current\n\npub fn main() -> void:\n    pass\n";
    let dir = package("hd-check-header-error", source);
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.err.contains("unknown-trait"), "{}", r.err);
    assert!(!r.err.contains("unsupported"), "{}", r.err);
    assert!(r.err.ends_with("errors: 1; warnings: 0\n"), "{}", r.err);
}

#[test]
fn check_works_from_a_directory_below_the_package() {
    let dir = package("hd-check-below", BROKEN);
    std::fs::create_dir_all(dir.join("docs")).expect("dir");
    let r = check_at(&dir.join("docs"), &dir, &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.err.contains("main.hd:2:13"), "{}", r.err);
}

#[test]
fn file_outside_a_package_is_a_single_file_program() {
    let dir = scratch("hd-check-file");
    std::fs::write(dir.join("ok.hd"), CLEAN).expect("write");
    std::fs::write(dir.join("bad.hd"), BROKEN).expect("write");
    let ok = check(&dir, &["ok.hd"]);
    assert_eq!(ok.code, Some(0), "{}", ok.err);
    let bad = check(&dir, &["bad.hd"]);
    assert_eq!(bad.code, Some(101), "{}", bad.err);
    assert!(bad.err.contains("bad.hd:2:13: unknown-name"), "{}", bad.err);
}

#[test]
fn no_file_outside_a_package_suggests_a_file_or_hd_new() {
    let dir = scratch("hd-check-nopkg");
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("hd new"), "{}", r.err);
    assert!(r.err.contains("FILE"), "{}", r.err);
}

#[test]
fn unknown_option_is_a_usage_error() {
    let dir = package("hd-check-usage", CLEAN);
    let r = check(&dir, &["--bogus"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("--bogus"), "{}", r.err);
}

#[test]
fn json_lines_have_diagnostics_then_a_summary() {
    let dir = package("hd-check-json", BROKEN);
    let r = check(&dir, &["--format", "json"]);
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
    let r = check(&dir, &["--format=json"]);
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
    let r = check(&dir, &["--format", "json", "bad.hd"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.out.contains("\"file\":\"bad.hd\""), "{}", r.out);
}

#[test]
fn second_run_is_warm() {
    let dir = package("hd-check-warm", CLEAN);
    let args = ["--format", "json"];
    let cold = check(&dir, &args);
    assert_eq!(cold.code, Some(0), "{}", cold.err);
    let warm = check(&dir, &args);
    assert_eq!(warm.code, Some(0), "{}", warm.err);
    assert!(modules_checked(&cold.out) > 0, "{}", cold.out);
    assert_eq!(modules_checked(&warm.out), 0, "{}", warm.out);
}

/// A library package with one clean module and three modules with problems:
/// `a.hd` has two `unknown-name` errors and a warning, `b.hd` a
/// `type-mismatch` error, and `c.hd` an `unknown-name` error.
fn broken_library(name: &str) -> Scratch {
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
    let bad = check(&dir, &[]);
    assert_eq!(bad.code, Some(101), "{}", bad.err);
    assert_eq!(bad.out, "");
    assert_eq!(
        bad.err.lines().last(),
        Some("check result: FAILED. errors: 4; warnings: 1"),
        "{}",
        bad.err
    );
    let ok = check(&dir, &["src/util.hd"]);
    assert_eq!(ok.code, Some(0), "{}", ok.err);
    assert_eq!(ok.err, "check result: ok. errors: 0; warnings: 0\n");
}

#[test]
fn summary_mode_counts_per_file_and_code_in_path_then_code_order() {
    let dir = broken_library("hd-check-summary");
    let r = check(&dir, &["--summary"]);
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
    let all = check(&dir, &[]);
    let two = check(&dir, &["--max-errors", "2"]);
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
    let eq = check(&dir, &["--max-errors=2"]);
    assert_eq!(eq.err, two.err);
    let big = check(&dir, &["--max-errors", "9"]);
    assert_eq!(big.err, all.err);
}

#[test]
fn max_errors_rejects_zero_and_text() {
    let dir = broken_library("hd-check-max-bad");
    for bad in ["0", "many"] {
        let r = check(&dir, &["--max-errors", bad]);
        assert_eq!(r.code, Some(101), "{}", r.err);
        assert!(r.err.contains("--max-errors"), "{}", r.err);
    }
}

#[test]
fn json_ignores_summary_and_max_errors() {
    let dir = broken_library("hd-check-json-opts");
    let plain = check(&dir, &["--format", "json"]);
    let opts = check(
        &dir,
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
    let r = check(&dir, &["src/user.hd"]);
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
fn bad_manifest(name: &str) -> Scratch {
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
    let r = check(&dir, &[]);
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
    let json = check(&dir, &["--format", "json"]);
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
    let r = check(&dir, &["--format", "json"]);
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
fn code_kinds(name: &str) -> Scratch {
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
            "pub fn main() -> void:\n    _ := in_integration\n",
        ),
        (
            "tasks/seed.hd",
            "pub fn main() -> void:\n    _ := in_task\n",
        ),
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
    let plain = check(&dir, &[]);
    assert_eq!(plain.code, Some(0), "{}", plain.err);
    let tests = check(&dir, &["--tests"]);
    assert_eq!(tests.code, Some(101));
    assert_eq!(
        missing(&tests.err),
        ["in_block", "in_unit", "in_integration"],
        "{}",
        tests.err
    );
    let all = check(&dir, &["--all"]);
    assert_eq!(
        missing(&all.err),
        ["in_block", "in_unit", "in_task", "in_integration"],
        "{}",
        all.err
    );
    let task = check(&dir, &["tasks/seed.hd"]);
    assert_eq!(missing(&task.err), ["in_task"], "{}", task.err);
    let flow = check(&dir, &["tests/flow.hd"]);
    assert_eq!(missing(&flow.err), ["in_integration"], "{}", flow.err);
}

/// `module.test.integration.no-path`, `cli.task.no-path`: test and task
/// code has no module path, so no message names one; a use that misses
/// names the path as written instead.
#[test]
fn messages_name_test_and_task_code_by_file() {
    let dir = code_kinds("hd-check-no-path");
    std::fs::write(dir.join("tasks/seed.hd"), "use self.nothing\n").expect("write");
    std::fs::create_dir_all(dir.join("tests/common")).expect("dir");
    std::fs::write(dir.join("tests/common/mod.hd"), "use super.up\n").expect("write");
    std::fs::write(dir.join("tests/common/up.hd"), "").expect("write");
    for format in ["text", "json"] {
        let r = check(&dir, &["--all", "--format", format]);
        assert_eq!(r.code, Some(101));
        let all = format!("{}{}", r.out, r.err);
        assert!(!all.contains('$'), "{all}");
        assert!(all.contains("no module named `self.nothing`"), "{all}");
        assert!(all.contains("no module named `super.up`"), "{all}");
    }
}

/// `module.test.doc.view`, `module.test.integration.pkg-root`: doc tests
/// of an executable cannot import its entry module through `pkg`.
#[test]
fn messages_name_doc_tests_library_view() {
    let dir = scratch("hd-check-doc-library-view");
    std::fs::create_dir_all(dir.join("src")).expect("dir");
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"app2\"\n").expect("write");
    std::fs::write(
        dir.join("src/main.hd"),
        "## ```hd\n## use pkg.parse_version\n## pass\n## ```\n\
         pub fn parse_version() -> i32: +1\n\n\
         pub fn main() -> void: pass\n",
    )
    .expect("write");
    for format in ["text", "json"] {
        for _ in 0..2 {
            let r = check(&dir, &["--tests", "--format", format]);
            assert_eq!(r.code, Some(101));
            let all = format!("{}{}", r.out, r.err);
            assert!(all.contains("no module named `pkg.parse_version`"), "{all}");
            assert!(all.contains("doc tests see only library modules"), "{all}");
        }
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
    let r = check(&dir, &["--tests"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    std::fs::write(
        dir.join("src/lib.hd"),
        "use dep.fixtures.{sample}\n\npub fn double(v: i32) -> i32:\n    v * sample()\n",
    )
    .expect("write");
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101));
    assert!(
        r.err.starts_with("error: src/lib.hd:1:1: test-only-use: ")
            && r.err.contains("hd remove fixtures"),
        "{}",
        r.err
    );
    // `module.test.dev-dependency.in-tests`: a `tests:` block may use it.
    std::fs::write(
        dir.join("src/lib.hd"),
        "pub fn double(v: i32) -> i32:\n    v * 2\n\ntests:\n    use dep.fixtures.{sample}\n\n    it(\"doubles the sample\"):\n        _ := double(sample())\n",
    )
    .expect("write");
    let r = check(&dir, &["--tests"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
}

/// `module.cycle.diagnostic.*`: the diagnostic shows one shortest loop
/// with the `use` making each edge, the tangle size, and a fix moving a
/// lone `x.hd` beside its own `mod.hd`.
#[test]
fn a_folder_cycle_shows_its_loop_uses_tangle_and_fix() {
    let dir = scratch("hd-check-cycle-detail");
    for (file, text) in [
        ("hd.toml", "[package]\nname = \"cyc\"\n"),
        (
            "src/shop/mod.hd",
            "use pkg.cart.item\n\npub fn shop_name() -> string:\n    item()\n",
        ),
        (
            "src/cart/mod.hd",
            "use pkg.shop.shop_name\n\npub fn item() -> string:\n    shop_name()\n",
        ),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(
        r.err.contains(
            "folder-cycle: src/cart -> src/shop -> src/cart\n  src/cart/mod.hd:1:1: use pkg.shop.shop_name\n  src/shop/mod.hd:1:1: use pkg.cart.item\ntangle of 2 folders\n"
        ),
        "{}",
        r.err
    );
    let dir = scratch("hd-check-cycle-fix");
    for (file, text) in [
        ("hd.toml", "[package]\nname = \"cyc\"\n"),
        (
            "src/shop/mod.hd",
            "use pkg.cart.{cart}\n\npub fn shop() -> i32:\n    cart()\n",
        ),
        (
            "src/cart.hd",
            "use pkg.shop.{shop}\n\npub fn cart() -> i32:\n    shop()\n",
        ),
        ("src/cart/extra.hd", "pub fn extra() -> i32:\n    1\n"),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    let r = check(&dir, &["--format", "json"]);
    assert!(
        r.out.contains("\"code\":\"folder-cycle\"")
            && r.out.contains("move src/cart.hd to src/cart/mod.hd"),
        "{}",
        r.out
    );
}

/// `crates/hd_resolve/DESIGN.md` §4.8 rule 5: the folders of a cycle
/// resolve together, so the cycle is reported and the uses across it
/// still resolve; a body error in one of them is still found.
#[test]
fn a_folder_cycle_is_one_error_and_bodies_still_check() {
    let dir = scratch("hd-check-cycle");
    for (file, text) in [
        ("hd.toml", "[package]\nname = \"shop\"\n"),
        (
            "src/a/x.hd",
            "use pkg.b.y.{two}\n\npub fn one() -> i32:\n    two() - 1\n",
        ),
        (
            "src/b/y.hd",
            "use pkg.a.x.{one}\n\npub fn two() -> i32:\n    one() + missing\n",
        ),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("folder-cycle"), "{}", r.err);
    assert!(r.err.contains("src/b/y.hd:4:13: unknown-name"), "{}", r.err);
    assert!(
        !r.err.contains("unknown-module") && !r.err.contains("unknown-import"),
        "{}",
        r.err
    );
}

/// `crates/hd_cache/DESIGN.md` §5.4: a cache directory that cannot be
/// written gives one warning, and the check still succeeds uncached.
#[cfg(unix)]
#[test]
fn an_unwritable_cache_warns_once_and_the_check_succeeds() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = package("hd-check-readonly-cache", CLEAN);
    let cache = dir.cache();
    std::fs::create_dir_all(&cache).expect("cache");
    let mode = |m| std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(m));
    mode(0o555).expect("read-only");
    let r = check(&dir, &["--tests"]);
    mode(0o755).expect("writable");
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        r.err
            .matches("warning: cannot write the compiled cache")
            .count(),
        1,
        "{}",
        r.err
    );
    assert!(r.err.contains("check result: ok."), "{}", r.err);
}

/// `crates/hd_resolve/DESIGN.md` §4.8 rule 4: a tangle of three folders
/// with two loops through it is one strongly connected component, so one
/// `folder-cycle` error.
#[test]
fn a_three_folder_tangle_is_one_folder_cycle() {
    let dir = scratch("hd-check-cycle-tangle");
    for (file, text) in [
        ("hd.toml", "[package]\nname = \"shop\"\n"),
        (
            "src/a/x.hd",
            "use pkg.b.y.{two}\n\npub fn one() -> i32:\n    1\n",
        ),
        (
            "src/b/y.hd",
            "use pkg.a.x.{one}\nuse pkg.c.z.{three}\n\npub fn two() -> i32:\n    one() + 1\n",
        ),
        (
            "src/c/z.hd",
            "use pkg.a.x.{one}\n\npub fn three() -> i32:\n    one() + 2\n",
        ),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert_eq!(r.err.matches("folder-cycle:").count(), 1, "{}", r.err);
    assert!(r.err.contains("tangle of 3 folders"), "{}", r.err);
}

/// `module.cycle.test-code`: a test module may use a helper folder that
/// uses its own folder; test code makes no folder edge, so no cycle.
#[test]
fn test_code_makes_no_folder_cycle() {
    let dir = scratch("hd-check-test-edges");
    for (file, text) in [
        ("hd.toml", "[package]\nname = \"shop\"\n"),
        ("src/shop/cart.hd", "pub fn total() -> i32:\n    3\n"),
        (
            "src/testkit/kit.hd",
            "use pkg.shop.cart.{total}\n\npub fn twice() -> i32:\n    total() * 2\n",
        ),
        (
            "src/shop/cart_test.hd",
            "use pkg.testkit.kit.{twice}\n\nfn six() -> i32:\n    twice()\n",
        ),
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    let r = check(&dir, &["--tests"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
}

/// `hd WORDS` in `dir`, with the build cache of `dir`.
fn hd(dir: &Scratch, words: &[&str]) -> Ran {
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.cache())
        .current_dir(dir)
        .args(words)
        .output()
        .expect("run hd");
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

const SCRIPT: &str = "name := \"Ada\"\nprintln(\"hello, $name\")\n";

/// Runs the steps of `orders`, each order on a fresh directory holding
/// `files` and a cold cache, and has `judge` judge each step's result,
/// so whichever command runs first, every command answers the same.
fn in_each_order(
    name: &str,
    files: &[(&str, &str)],
    orders: &[&[&[&str]]],
    judge: &dyn Fn(&[&str], &Ran),
) {
    for (i, order) in orders.iter().enumerate() {
        let dir = scratch(&format!("{name}-{i}"));
        for (file, text) in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
            std::fs::write(path, text).expect("write");
        }
        for words in *order {
            judge(words, &hd(&dir, words));
        }
    }
}

/// `cli.file.check-test`, `module.init.script-body`,
/// `module.console.println-script`: `hd check FILE` checks a script as
/// `hd FILE` runs it, so its top level may print, and neither command's
/// cache entry changes the other's answer (#282).
#[test]
fn a_script_checks_as_it_runs_whichever_runs_first() {
    let check: &[&str] = &["check", "s.hd"];
    let run: &[&str] = &["s.hd"];
    in_each_order(
        "hd-check-script",
        &[("s.hd", SCRIPT)],
        &[&[check, run], &[run, check]],
        &|words, r| {
            assert_eq!(r.code, Some(0), "{words:?}: {}", r.err);
            if words == run {
                assert_eq!(r.out, "hello, Ada\n", "{}", r.err);
            }
        },
    );
}

/// `module.init.script-empty`: an empty script checks and runs, doing
/// nothing, whichever command runs first.
#[test]
fn an_empty_script_checks_and_runs_whichever_runs_first() {
    let check: &[&str] = &["check", "z.hd"];
    let run: &[&str] = &["z.hd"];
    in_each_order(
        "hd-check-empty-script",
        &[("z.hd", "")],
        &[&[check, run], &[run, check]],
        &|words, r| {
            assert_eq!(r.code, Some(0), "{words:?}: {}", r.err);
            assert_eq!(r.out, "", "{words:?}");
        },
    );
}

/// `module.init.tests.no-entry`, `module.init.tests.requirement-free`: a
/// test run runs no entry behavior, so `hd test` and `hd check --tests`
/// reject a tested script's printing top level, while `hd check` and
/// `hd FILE` accept it, in every order.
#[test]
fn a_tested_script_prints_only_when_it_runs_whichever_runs_first() {
    let tested = format!("{SCRIPT}\ntests:\n    it(\"runs\"):\n        pass\n");
    let check: &[&str] = &["check", "t.hd"];
    let check_tests: &[&str] = &["check", "--tests", "t.hd"];
    let run: &[&str] = &["t.hd"];
    let test: &[&str] = &["test", "t.hd"];
    in_each_order(
        "hd-check-tested-script",
        &[("t.hd", &tested)],
        &[
            &[check, run, test, check_tests],
            &[test, check_tests, run, check],
            &[run, check_tests, check, test],
        ],
        &|words, r| {
            if words == test || words == check_tests {
                assert_eq!(r.code, Some(101), "{words:?}");
                assert!(r.err.contains("t.hd:2:1: missing-requirement"), "{}", r.err);
            } else {
                assert_eq!(r.code, Some(0), "{words:?}: {}", r.err);
            }
            if words == run {
                assert_eq!(r.out, "hello, Ada\n", "{}", r.err);
            }
        },
    );
}

/// `cli.package.no-root`, `cli.check.all`: in a package, a file under no
/// root checks as the single-file program `hd FILE` runs, and a task's
/// script top level checks as `hd run` runs it, in every order.
#[test]
fn package_scripts_check_as_they_run_whichever_runs_first() {
    let files = [
        ("hd.toml", "[package]\nname = \"shop\"\n"),
        ("src/main.hd", CLEAN),
        ("tasks/greet.hd", SCRIPT),
        ("tool.hd", SCRIPT),
    ];
    let check: &[&str] = &["check", "tool.hd"];
    let run: &[&str] = &["tool.hd"];
    in_each_order(
        "hd-check-no-root-script",
        &files,
        &[&[check, run], &[run, check]],
        &|words, r| {
            assert_eq!(r.code, Some(0), "{words:?}: {}", r.err);
            if words == run {
                assert_eq!(r.out, "hello, Ada\n", "{}", r.err);
            }
        },
    );
    let files = [
        ("hd.toml", "[package]\nname = \"shop\"\n"),
        ("src/main.hd", CLEAN),
        ("tasks/greet.hd", SCRIPT),
    ];
    let all: &[&str] = &["check", "--all"];
    let task: &[&str] = &["check", "tasks/greet.hd"];
    let run: &[&str] = &["run", "greet"];
    in_each_order(
        "hd-check-task-script",
        &files,
        &[&[all, task, run], &[run, task, all]],
        &|words, r| {
            assert_eq!(r.code, Some(0), "{words:?}: {}", r.err);
            if words == run {
                assert_eq!(r.out, "hello, Ada\n", "{}", r.err);
            }
        },
    );
}

/// `cli.file.run`, `cli.file.check-test`, `cli.package.no-root`,
/// `module.single-file.definition`: a file under no root of a package is a
/// single-file program that may use only std, so no command loads the
/// package's modules, and a use of `pkg` is an error in each (#284).
#[test]
fn a_no_root_file_in_a_package_uses_only_std() {
    let tool = "use pkg.main\n\nprintln(\"tool\")\n";
    let tested = "tests:\n    it(\"runs\"):\n        pass\n\nuse pkg.main\n";
    let files = [
        ("hd.toml", "[package]\nname = \"shop\"\n"),
        ("src/main.hd", CLEAN),
        ("tasks/greet.hd", SCRIPT),
        ("tool.hd", tool),
        ("tested.hd", tested),
    ];
    let words: [&[&str]; 4] = [
        &["tool.hd"],
        &["check", "tool.hd"],
        &["test", "tested.hd"],
        &["check", "--tests", "tested.hd"],
    ];
    in_each_order(
        "hd-no-root-uses-only-std",
        &files,
        &[&words, &[words[3], words[2], words[1], words[0]]],
        &|words, r| {
            let file = words.last().expect("a file");
            assert_ne!(r.code, Some(0), "{words:?}");
            assert!(
                r.err.contains(&format!("{file}:")) && r.err.contains("unknown-module"),
                "{words:?}: {}",
                r.err
            );
            assert!(!r.err.contains("greet.hd"), "{words:?}: {}", r.err);
        },
    );
}

/// `--format` is a global flag: before the command's name it works as it
/// does after it.
#[test]
fn a_format_flag_before_the_command_selects_json() {
    let dir = package("hd-check-format-first", BROKEN);
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.cache())
        .current_dir(&*dir)
        .args(["--format", "json", "check", "main.hd"])
        .output()
        .expect("run hd");
    let out = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(101), "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(
        lines[0].starts_with("{\"kind\":\"diagnostic\",\"code\":\"unknown-name\""),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("{\"kind\":\"summary\""),
        "{}",
        lines[1]
    );
}

/// A linked directory that leads back above itself (`src/loop -> ..`) is
/// skipped, not walked again until the path runs out (#285).
#[cfg(unix)]
#[test]
fn a_symlink_cycle_under_src_is_skipped() {
    let dir = scratch("hd-check-symlink-cycle");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("write");
    std::fs::write(dir.join("src/main.hd"), CLEAN).expect("write");
    std::os::unix::fs::symlink("..", dir.join("src/loop")).expect("symlink");
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.err, "check result: ok. errors: 0; warnings: 0\n");
    assert_eq!(r.out, "");
}

/// `hd check FILE` checks only FILE's module and the modules it uses,
/// deeply (`crates/hd_cli/DESIGN.md` §7.2): a sibling it does not reach is
/// not checked, and the output is as before.
#[test]
fn check_file_checks_only_the_modules_it_reaches() {
    let dir = package(
        "hd-check-file-reach",
        "use pkg.util.{double}\n\npub fn main() -> void $ Console:\n    println(double(+2))\n",
    );
    std::fs::write(
        dir.join("util.hd"),
        "pub fn double(n: i32) -> i32:\n    n * 2\n",
    )
    .expect("write");
    std::fs::write(dir.join("side.hd"), "pub fn sibling() -> i32:\n    +1\n").expect("write");
    std::fs::create_dir_all(dir.join("extra")).expect("mkdir");
    std::fs::write(
        dir.join("extra/broken.hd"),
        "pub fn broken() -> i32:\n    missing\n",
    )
    .expect("write");
    let r = check(&dir, &["--format", "json", "main.hd"]);
    assert_eq!(r.code, Some(0), "{}", r.out);
    assert_eq!(modules_checked(&r.out), 2, "main and util only: {}", r.out);
    let r = check(&dir, &["--format", "json", "extra/broken.hd"]);
    assert_eq!(r.code, Some(101), "{}", r.out);
    assert!(r.out.contains("\"code\":\"unknown-name\""), "{}", r.out);
    assert_eq!(modules_checked(&r.out), 1, "{}", r.out);
}

/// The entry files under a cache's `obj` tree, without the size ledger.
fn entries(cache: &Path) -> usize {
    fn walk(dir: &Path, n: &mut usize) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, n);
            } else if path.file_name().is_some_and(|f| f != "stats") {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    walk(&cache.join("obj"), &mut n);
    n
}

/// Print, then publish (`crates/hd_cache/DESIGN.md` §5.4, #250): after a
/// warm edit, `hd check` writes its report before any new cache entry. The
/// report is larger than a pipe holds, so `hd` stays blocked in it until
/// the test reads the rest, and the entries the test sees meanwhile are
/// the ones from before.
#[test]
fn a_warm_check_prints_before_it_publishes() {
    use std::fmt::Write as _;
    use std::io::Read as _;
    let dir = scratch("hd-check-print-then-publish");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("write");
    std::fs::write(dir.join("src/main.hd"), CLEAN).expect("write");
    let mut noisy = String::new();
    for i in 0..3000 {
        let _ = writeln!(noisy, "fn f{i}() -> i32: missing{i}");
    }
    std::fs::write(dir.join("src/noisy.hd"), noisy).expect("write");
    assert_eq!(check(&dir, &["--format", "json"]).code, Some(101));
    let before = entries(&dir.cache());
    assert!(before > 0, "the first check publishes");
    let edited = CLEAN.replace("\"ok\"", "\"edited\"");
    std::fs::write(dir.join("src/main.hd"), edited).expect("write");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.cache())
        .current_dir(&*dir)
        .args(["check", "--format", "json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("run hd");
    let mut stdout = child.stdout.take().expect("stdout");
    let mut first = [0; 4096];
    let n = stdout.read(&mut first).expect("read");
    assert!(n > 0, "the report starts");
    assert_eq!(entries(&dir.cache()), before, "no entry before the report");
    let mut rest = Vec::new();
    stdout.read_to_end(&mut rest).expect("read");
    assert!(rest.len() > 100_000, "the report outgrows the pipe");
    assert_eq!(child.wait().expect("wait").code(), Some(101));
    assert!(entries(&dir.cache()) > before, "the edit's entries follow");
}

/// `cli.jobs.result`: the thread count changes no output, with or without
/// diagnostics.
#[test]
fn jobs_change_no_output() {
    for (name, source) in [("hd-check-jobs-ok", CLEAN), ("hd-check-jobs-bad", BROKEN)] {
        let dir = package(name, source);
        let one = check(&dir, &["--jobs", "1"]);
        let eight = check(&dir, &["--jobs=8"]);
        assert_eq!(one.code, eight.code, "{}", eight.err);
        assert_eq!(one.out, eight.out);
        assert_eq!(one.err, eight.err);
        let plain = check(&dir, &[]);
        assert_eq!(one.err, plain.err);
    }
}

/// `hd build --jobs N` builds the module like `hd build` does.
#[test]
fn build_accepts_jobs() {
    let dir = package("hd-build-jobs", CLEAN);
    let cache = dir.cache();
    let built = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", cache)
        .current_dir(&dir)
        .args(["build", "--jobs", "2"])
        .output()
        .expect("run hd");
    assert_eq!(built.status.code(), Some(0), "{:?}", built.stderr);
    assert!(dir.join("build/debug/shop.wasm").is_file());
}

/// `--jobs 0` and `--jobs many` are the usage error `hd test` gives.
#[test]
fn bad_jobs_is_the_same_usage_error_everywhere() {
    let dir = package("hd-jobs-bad", CLEAN);
    for value in ["0", "many"] {
        let message = format!("`--jobs {value}`: N is a whole number of at least 1");
        for command in ["check", "build", "test", "fmt", "fix"] {
            let out = Command::new(env!("CARGO_BIN_EXE_hd"))
                .env("HD_CACHE", dir.cache())
                .current_dir(&dir)
                .args([command, "--jobs", value])
                .output()
                .expect("run hd");
            let err = String::from_utf8_lossy(&out.stderr);
            assert_ne!(out.status.code(), Some(0), "{command}: {err}");
            assert!(err.contains(&message), "{command}: {err}");
        }
    }
}
