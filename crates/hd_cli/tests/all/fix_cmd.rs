//! `hd fix` (`cli.fix.*`) on real packages: a safe fix-it applied and
//! reported, one that would add a diagnostic left alone, the exit status,
//! and no write when nothing changes. Overlap and rounds need fix-its the
//! compiler does not offer today; the unit tests of `fix_cmd.rs` cover them
//! with a stand-in check.

use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use hd_testkit::Scratch;

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

/// A package `shop` whose `src/lib.hd` holds `lib`.
fn package(name: &str, lib: &str) -> Scratch {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(dir.join("src/lib.hd"), lib).expect("lib.hd");
    dir
}

fn hd(cwd: &Path, dir: &Scratch, args: &[&str]) -> Ran {
    let output = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.cache())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run hd");
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn lib(dir: &Scratch) -> String {
    std::fs::read_to_string(dir.join("src/lib.hd")).expect("read lib.hd")
}

fn modified(dir: &Scratch) -> SystemTime {
    std::fs::metadata(dir.join("src/lib.hd"))
        .and_then(|m| m.modified())
        .expect("mtime")
}

const GREET: &str = "pub trait Greet:\n    fn greet(self) -> string\n\npub fn twice(g: Greet) -> string:\n    g.greet() + g.greet()\n";

/// `cli.fix.command.with-capabilities`, `cli.fix.report`, `cli.fix.status`: the `dyn` of
/// `types.trait.value.dyn-required.reference-values` is written, the change is printed with
/// its file and code, and with no error left the status is 0.
#[test]
fn applies_a_safe_fix_and_reports_it() {
    let dir = package("hd-fix-dyn", GREET);
    let r = hd(&dir, &dir, &["fix"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(lib(&dir), GREET.replace("g: Greet", "g: &dyn Greet"));
    assert_eq!(
        r.err,
        "fixed: src/lib.hd:4:17: trait-used-as-type: insert `&dyn` before the trait name\nfix result: ok. fixed: 1; errors: 0; warnings: 0\n"
    );
    assert_eq!(r.out, "");
    assert!(!dir.join("src/lib.hd.fix-tmp").exists());
    assert_eq!(hd(&dir, &dir, &["check"]).code, Some(0));
    // From a directory below the package root, a second run finds nothing.
    let before = modified(&dir);
    let r = hd(&dir.join("src"), &dir, &["fix"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.err, "fix result: ok. fixed: 0; errors: 0; warnings: 0\n");
    assert_eq!(modified(&dir), before);
}

/// `cli.fix.safe`: `dyn Supplier` leaves `Item` unbound, a new
/// `trait-not-dynamically-safe`, so the fix-it is not applied and the
/// file is not written. A private trait would bring `private-type-leak`.
#[test]
fn a_fix_that_adds_a_diagnostic_is_not_applied() {
    for (name, source) in [
        (
            "hd-fix-unsafe-assoc",
            "pub trait Supplier:\n    type Item\n    fn get(self) -> Self::Item\n\npub fn drain(source: Supplier) -> void:\n    pass\n",
        ),
        (
            "hd-fix-unsafe-private",
            "trait Greet:\n    fn greet(self) -> string\n\npub fn twice(g: Greet) -> string:\n    g.greet()\n",
        ),
    ] {
        let dir = package(name, source);
        let before = modified(&dir);
        let r = hd(&dir, &dir, &["fix"]);
        assert_eq!(r.code, Some(101), "{}", r.err);
        assert_eq!(lib(&dir), source);
        assert_eq!(modified(&dir), before);
        assert!(r.err.contains(": trait-used-as-type: "), "{}", r.err);
        assert!(
            r.err.ends_with("fixed: 0; errors: 1; warnings: 0\n"),
            "{}",
            r.err
        );
        assert!(!r.err.contains("fixed: src/"), "{}", r.err);
    }
}

/// `cli.fix.command.with-capabilities`, `cli.fix.status`: one round applies the fix-its of
/// two diagnostics, and an error with no fix-it remains, so the status is
/// 101 and the remaining diagnostic follows the changes.
#[test]
fn applies_every_safe_fix_and_fails_while_an_error_remains() {
    let source = "pub trait Greet:\n    fn greet(self) -> string\n\npub fn twice(g: Greet) -> string:\n    g.greet()\n\npub fn run[R](job: fn() -> void $ R) -> void $ R:\n    job()\n\npub fn count() -> i32:\n    \"one\"\n";
    let dir = package("hd-fix-remaining", source);
    let r = hd(&dir, &dir, &["fix"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    let fixed = source
        .replace("g: Greet", "g: &dyn Greet")
        .replace("run[R]", "run[$R]");
    assert_eq!(lib(&dir), fixed);
    let lines: Vec<&str> = r.err.lines().collect();
    assert_eq!(lines.len(), 4, "{}", r.err);
    assert!(lines[0].starts_with("fixed: src/lib.hd:4:17: trait-used-as-type: "));
    assert!(lines[1].starts_with("fixed: src/lib.hd:7:12: generic-kind-mismatch: "));
    assert!(lines[2].starts_with("error: src/lib.hd:11:5: type-mismatch: "));
    assert_eq!(
        lines[3],
        "fix result: FAILED. fixed: 2; errors: 1; warnings: 0"
    );
}

/// No fix-it to apply writes no file, with or without an error.
#[test]
fn writes_nothing_without_a_fix() {
    for (name, source, code) in [
        ("hd-fix-clean", "pub fn one() -> i32:\n    +1\n", 0),
        (
            "hd-fix-unfixable",
            "pub fn one() -> i32:\n    \"one\"\n",
            101,
        ),
    ] {
        let dir = package(name, source);
        let before = modified(&dir);
        let r = hd(&dir, &dir, &["fix"]);
        assert_eq!(r.code, Some(code), "{}", r.err);
        assert_eq!(lib(&dir), source);
        assert_eq!(modified(&dir), before);
    }
}

/// `hd fix` takes no FILE and no `--format` (the Commands table,
/// `cli.json.commands`), and works on a package only.
#[test]
fn rejects_a_file_a_format_and_no_package() {
    let dir = package("hd-fix-args", GREET);
    for args in [
        &["fix", "src/lib.hd"][..],
        &["fix", "--format", "json"][..],
        &["fix", "--jobs", "0"][..],
    ] {
        let r = hd(&dir, &dir, args);
        assert_eq!(r.code, Some(101), "{args:?}: {}", r.err);
    }
    assert_eq!(lib(&dir), GREET);
    let r = hd(&dir, &dir, &["fix", "--jobs", "1"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    let outside = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-fix-outside");
    let r = hd(&outside, &outside, &["fix"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    let ws = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-fix-workspace");
    std::fs::write(ws.join("hd.toml"), "[workspace]\nmembers = []\n").expect("hd.toml");
    let r = hd(&ws, &ws, &["fix"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.err.contains("workspace"), "{}", r.err);
}

/// A warm cache keeps a fix-it whose diagnostic it re-emits, placed by
/// the code it fixes: after `hd check` and an edit that moves the
/// signature down, `hd fix` still writes `dyn` before the trait name.
#[test]
fn a_warm_cache_keeps_fix_its_where_their_code_moved() {
    let dir = package("hd-fix-warm", GREET);
    let r = hd(&dir, &dir, &["check", "--format", "json"]);
    assert!(r.out.contains("\"text\":\"&dyn \""), "{}", r.out);
    let moved = format!("# a comment that moves every byte below it\n\n{GREET}");
    std::fs::write(dir.join("src/lib.hd"), &moved).expect("write");
    let r = hd(&dir, &dir, &["check", "--format", "json"]);
    assert!(r.out.contains("\"modules_checked\":0"), "{}", r.out);
    assert!(r.out.contains("\"start\":107,\"end\":107"), "{}", r.out);
    let r = hd(&dir, &dir, &["fix"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(lib(&dir), moved.replace("g: Greet", "g: &dyn Greet"));
}
