//! `hd check` (`cli.check.*`, `cli.file.check-test`, `cli.json.*`): package
//! and file mode, the exit status, the JSON lines, and the warm second run.

use std::path::Path;

use hd_testkit::Scratch;

fn scratch(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), name)
}

/// A package `shop` whose `main.hd` holds `main_source`.
fn package(name: &str, main_source: &str) -> Scratch {
    let dir = scratch(name);
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
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

fn check(dir: &Scratch, args: &[&str]) -> Ran {
    check_at(dir, dir, args)
}

/// `hd check` run in `cwd`, with the build cache of `dir`.
fn check_at(cwd: &Path, dir: &Scratch, args: &[&str]) -> Ran {
    let cache = dir.cache();
    let output = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", crate::shared_objects(cache))
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

const CLEAN: &str = include_str!("programs/prints_ok.hd");
const BROKEN: &str = include_str!("../../../../spec/conformance/cli/json-run/broken.hd");

/// The number after `"modules_checked":` in a summary line.
fn modules_checked(out: &str) -> u32 {
    let tail = out
        .split("\"modules_checked\":")
        .nth(1)
        .expect("modules_checked");
    tail.trim_end_matches(['}', '\n']).parse().expect("count")
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
fn second_run_is_warm() {
    let dir = package("hd-check-warm", CLEAN);
    crate::own_objects(&dir.cache());
    let args = ["--format", "json"];
    let cold = check(&dir, &args);
    assert_eq!(cold.code, Some(0), "{}", cold.err);
    let warm = check(&dir, &args);
    assert_eq!(warm.code, Some(0), "{}", warm.err);
    assert!(modules_checked(&cold.out) > 0, "{}", cold.out);
    assert_eq!(modules_checked(&warm.out), 0, "{}", warm.out);
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

/// A linked directory that leads back above itself (`src/loop -> ..`) is
/// skipped, not walked again until the path runs out (#285).
#[cfg(unix)]
#[test]
fn a_symlink_cycle_under_src_is_skipped() {
    let dir = scratch("hd-check-symlink-cycle");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
    std::fs::write(dir.join("src/main.hd"), CLEAN).expect("write");
    std::os::unix::fs::symlink("..", dir.join("src/loop")).expect("symlink");
    let r = check(&dir, &[]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.err, "check result: ok.\n");
    assert_eq!(r.out, "");
}

/// `hd check FILE` checks only FILE's module and the modules it uses,
/// deeply (`crates/hd_command/DESIGN.md` §7.2): a sibling it does not reach is
/// not checked, and the output is as before.
#[test]
fn check_file_checks_only_the_modules_it_reaches() {
    let dir = package(
        "hd-check-file-reach",
        include_str!("programs/reach/main.hd"),
    );
    crate::own_objects(&dir.cache());
    std::fs::write(dir.join("util.hd"), include_str!("programs/reach/util.hd")).expect("write");
    std::fs::write(dir.join("side.hd"), include_str!("programs/reach/side.hd")).expect("write");
    std::fs::create_dir_all(dir.join("extra")).expect("mkdir");
    std::fs::write(
        dir.join("extra/broken.hd"),
        include_str!("programs/reach/broken.hd"),
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

/// Logical entry count: physical packs, index and lock files do not each
/// represent an entry. Reuse the validated index/range reader from GC tests.
fn entries(cache: &Path) -> usize {
    super::cache_gc::entries(&cache.join("obj")).len()
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
    crate::own_objects(&dir.cache());
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
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
    let mut child = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", crate::shared_objects(dir.cache()))
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

/// Compact JSON keeps actual repairs, but omits empty fixes and check-only
/// test counters. Counts describe diagnostics, independently of cache work.
#[test]
fn compact_check_json_omits_empty_fields() {
    let dir = package("hd-check-compact", BROKEN);
    let r = check(&dir, &["--format", "json"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    let rows: Vec<serde_json::Value> = r
        .out
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON line"))
        .collect();
    let diagnostics: Vec<_> = rows
        .iter()
        .filter(|row| row["kind"] == "diagnostic")
        .collect();
    assert!(!diagnostics.is_empty());
    for row in &diagnostics {
        assert!(row.get("fixes").is_none(), "{row}");
    }
    let summary = rows.last().expect("summary");
    assert_eq!(summary["kind"], "summary");
    assert_eq!(summary["errors"].as_u64(), Some(diagnostics.len() as u64));
    assert!(summary["modules_checked"].is_number());
    for key in ["passed", "failed", "ignored"] {
        assert!(summary.get(key).is_none(), "{summary}");
    }
}
