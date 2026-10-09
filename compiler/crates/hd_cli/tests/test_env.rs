//! The integration test environment of `hd test` (spec/cli/command-line.md,
//! "Test Environments"): the test grant from `[test.capabilities]` and the
//! `--cap` flags of `hd test` (`cli.test.env.grant`), the refusal of a
//! totally denied need with status 101 (`cli.cap.total.test`), each case's
//! temporary directory removed once the run ends
//! (`cli.test.env.temp-dir.removed`), `--seed` (`cli.test.seed.flag`), and
//! the JS host's side of them: the lazily made temporary directory and the
//! grant's scope checks.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

/// A fresh package `shop` with `manifest` after its `[package]` table and
/// one integration test file that prints.
fn package(name: &str, manifest: &str) -> PathBuf {
    let root = tmp(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(root.join("tests")).expect("tests");
    std::fs::write(
        root.join("hd.toml"),
        format!("[package]\nname = \"shop\"\n{manifest}"),
    )
    .expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), "pub fn total() -> i32:\n    3\n").expect("lib");
    std::fs::write(
        root.join("tests/sum.hd"),
        "use pkg.total\nuse std.testing.assert_equal\n\nit(\"prints and sums\"):\n    \
         println(\"checking\")\n    assert_equal(total(), 3, reason=\"the total\")\n",
    )
    .expect("test file");
    root
}

fn hd(root: &Path, temp: Option<&Path>, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_hd"));
    c.env("HD_CACHE", root.with_extension("cache"))
        .env_remove("HD_JOBS")
        .current_dir(root)
        .args(args);
    if let Some(t) = temp {
        c.env("TMPDIR", t);
    }
    c.output().expect("run hd")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const PASSED: &str = "test result: ok. 1 passed; 0 failed; 0 ignored\n";

/// `cli.test.env.grant.table`: `[capabilities]` does not apply to a test
/// case, so its `Console = false` refuses nothing under `hd test`.
#[test]
fn the_run_table_does_not_apply_to_a_test() {
    let root = package("hd-env-run-table", "[capabilities]\nConsole = false\n");
    let out = hd(&root, None, &["test"]);
    assert_eq!(text(&out.stdout), PASSED, "{}", text(&out.stderr));
    assert_eq!(out.status.code(), Some(0));
}

/// `cli.cap.total.test`, `cli.cap.total.message`, `cli.cap.order.deny`:
/// `[test.capabilities]` denies Console totally, so the integration test
/// program never starts, a flag that grants it does not lift the deny, and
/// the command ends with status 101.
#[test]
fn the_test_table_refuses_a_denied_need() {
    let root = package(
        "hd-env-test-table",
        "[test.capabilities]\nConsole = false\n",
    );
    let want = "error: denied-capability: the test program tests/sum.hd needs Console, \
                which `Console = false` in `[test.capabilities]` of hd.toml denies, so it \
                does not start\n";
    for args in [&["test"][..], &["test", "--cap", "Console=true"]] {
        let out = hd(&root, None, args);
        assert_eq!(out.status.code(), Some(101), "{args:?}");
        assert_eq!(text(&out.stdout), "", "{args:?}");
        assert_eq!(text(&out.stderr), want, "{args:?}");
    }
    let out = hd(&root, None, &["test", "--format", "json"]);
    assert_eq!(out.status.code(), Some(101));
    let stdout = text(&out.stdout);
    assert!(
        stdout.starts_with("{\"kind\":\"diagnostic\",\"code\":\"denied-capability\""),
        "{stdout}"
    );
    assert!(stdout.ends_with("\"status\":101}\n"), "{stdout}");
}

/// `cli.cap.flag.commands`, `cli.test.env.grant`: a `--cap` flag of
/// `hd test` joins the test grant, and its deny refuses the program.
#[test]
fn a_cap_flag_of_hd_test_refuses_a_denied_need() {
    let root = package("hd-env-flag", "");
    let out = hd(&root, None, &["test", "--cap", "Console=false"]);
    assert_eq!(out.status.code(), Some(101));
    assert_eq!(
        text(&out.stderr),
        "error: denied-capability: the test program tests/sum.hd needs Console, which \
         `--cap Console=false` denies, so it does not start\n"
    );
    let out = hd(&root, None, &["test", "--cap", "Console=true"]);
    assert_eq!(text(&out.stdout), PASSED, "{}", text(&out.stderr));
    // cli.cap.flag.unknown
    let out = hd(&root, None, &["test", "--cap", "Time=true"]);
    assert_eq!(out.status.code(), Some(101));
}

/// `cli.test.env.temp-dir.removed`: `hd test` leaves nothing in the
/// system's temporary directory, neither a case's directory nor the run's.
#[test]
fn the_temporary_directories_are_removed() {
    let root = package("hd-env-temp", "");
    let temp = tmp("hd-env-temp.tmpdir");
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).expect("TMPDIR");
    let out = hd(&root, Some(&temp), &["test"]);
    assert_eq!(text(&out.stdout), PASSED, "{}", text(&out.stderr));
    let left: Vec<_> = std::fs::read_dir(&temp)
        .expect("TMPDIR")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// `cli.test.seed.flag`: `--seed N` takes a whole number.
#[test]
fn seed_takes_a_whole_number() {
    let root = package("hd-env-seed", "");
    let out = hd(&root, None, &["test", "--seed", "7"]);
    assert_eq!(text(&out.stdout), PASSED, "{}", text(&out.stderr));
    let out = hd(&root, None, &["test", "--seed=7"]);
    assert_eq!(out.status.code(), Some(0));
    for bad in ["-1", "x", "1.5"] {
        let out = hd(&root, None, &["test", "--seed", bad]);
        assert_eq!(out.status.code(), Some(101), "{bad}");
        assert_eq!(
            text(&out.stderr),
            format!("error: `--seed {bad}`: N is a whole number\n")
        );
    }
}

/// The JS host's side (`compiler/host/core.mjs`): `tempDir()` makes the
/// case's directory on its first call and returns the same path after
/// (`std-testing.runner.temp-dir`, `std-testing.temp-dir.same`), the
/// host holds the case's program path and no arguments
/// (`cli.test.env.args`, `.args.program`), and the grant's scope checks
/// agree with `hd_run::Grants` (`cli.cap.scope.*`).
#[test]
fn the_js_host_holds_the_case_environment() {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/core.mjs");
    let core = std::fs::canonicalize(core).expect("core.mjs");
    let work = tmp("hd-env-js");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("pkg/fixtures")).expect("pkg");
    let work = std::fs::canonicalize(&work).expect("work");
    let pkg = work.join("pkg");
    let case = work.join("case-0");
    let script = format!(
        r#"
import {{ existsSync }} from "node:fs";
import {{ createHost }} from {core};
const sink = {{ out() {{}}, err() {{}} }};
const pkg = {pkg};
const dir = {case};
const host = createHost(sink, {{
  args: [],
  program: "tests/export.hd",
  tempDir: dir,
  seed: 7,
  grants: {{
    FsRead: [pkg, dir],
    FsWrite: [dir],
    Console: false,
    Env: ["HOME", "APP_*"],
    Http: ["*.example.com", "localhost:8080", "[::1]:9"],
  }},
}});
const checks = [
  ["no arguments", host.args.length === 0],
  ["program", host.program === "tests/export.hd"],
  ["seed", host.seed === 7],
  ["lazy", !existsSync(dir)],
  ["made", host.tempDir() === dir && existsSync(dir)],
  ["same", host.tempDir() === dir],
  ["read fixture", host.grant.coversPath("FsRead", "fixtures/orders.csv", pkg)],
  ["read temp", host.grant.coversPath("FsRead", dir + "/out.csv", pkg)],
  ["no escape", !host.grant.coversPath("FsRead", "../secret", pkg)],
  ["no sibling", !host.grant.coversPath("FsRead", pkg + "-other/a", pkg)],
  ["write temp", host.grant.coversPath("FsWrite", dir + "/out.csv", pkg)],
  ["no write in package", !host.grant.coversPath("FsWrite", "out.csv", pkg)],
  ["deny", !host.grant.allows("Console") && !host.grant.coversName("Console", "x")],
  ["no limit", host.grant.allows("Clock") && host.grant.coversName("Sys", "os")],
  ["env prefix", host.grant.coversName("Env", "APP_MODE") && !host.grant.coversName("Env", "PATH")],
  ["wildcard", host.grant.coversHost("Http", "api.example.com") && !host.grant.coversHost("Http", "example.com")],
  ["port", host.grant.coversHost("Http", "localhost", 8080) && !host.grant.coversHost("Http", "localhost", 80)],
  ["v6", host.grant.coversHost("Http", "::1", 9)],
];
const failed = checks.filter(([, ok]) => !ok).map(([name]) => name);
console.log(failed.length ? "failed: " + failed.join(", ") : "ok");
"#,
        core = js_str(&format!("file://{}", core.display())),
        pkg = js_str(&pkg.to_string_lossy()),
        case = js_str(&case.to_string_lossy()),
    );
    let out = Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .output()
        .expect("run node");
    assert_eq!(text(&out.stdout), "ok\n", "{}", text(&out.stderr));
}

/// A JS string literal.
fn js_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
