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
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    )
    .expect("write");
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
    assert_eq!(r.err, "");
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
