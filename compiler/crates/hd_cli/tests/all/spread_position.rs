//! `expr.call.spread.position` (spec/lang/05-expressions.md): a call has
//! at most one positional spread and it ends the positional arguments,
//! except where `expr.call.spread.vararg-duplicate` applies.

use std::path::Path;
use std::process::{Command, Output};

use hd_testkit::Scratch;

fn check(name: &str, lib: &str) -> (Option<i32>, String) {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"notes\"\n").expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
    let out = hd(&root, &["check"]);
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
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

const DECLS: &str = "\
fn f(a: i32, b: i32) -> i32: a + b
fn g(xs...: List[i32]) -> i32: 0
";

/// A positional after the spread, or a second spread, is reported once.
#[test]
fn a_nonfinal_spread_is_reported_once() {
    for (call, code) in [
        ("f(xs..., +1)", "nonfinal-positional-spread"),
        ("f(xs..., ys...)", "nonfinal-positional-spread"),
        ("g(xs..., +1)", "duplicate-argument"),
        ("g(xs..., ys...)", "duplicate-argument"),
    ] {
        let lib = format!("{DECLS}\npub fn run(xs: List[i32], ys: List[i32]) -> i32: {call}\n");
        let (status, err) = check("spread-position", &lib);
        assert_eq!(status, Some(101), "{call}: {err}");
        assert_eq!(err.matches(code).count(), 1, "{call}: {err}");
        assert_eq!(err.matches("error:").count(), 1, "{call}: {err}");
    }
}

/// A named argument after the spread is fine.
#[test]
fn a_named_argument_may_follow_a_spread() {
    let lib = format!("{DECLS}\npub fn run(xs: (i32,)) -> i32: f(xs..., b=+1)\n");
    let (status, err) = check("spread-named-after", &lib);
    assert_eq!(status, Some(0), "{err}");
}
