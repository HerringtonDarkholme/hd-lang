//! `dbg` under `hd test` (spec/cli/command-line.md, "Debug Output"): the lines
//! of a failing case print with its failure. The value forms are `stdout`
//! stage cases of `hd_driver` (`tests/cases/dbg`), and release builds are
//! `spec/conformance/cli/dbg-release`. This one stays in Rust because its
//! text holds a std line number, which the test normalizes.

use std::path::Path;

use hd_testkit::Scratch;

fn scratch(name: &str) -> Scratch {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(dir.join("src")).expect("dir");
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"probe\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
    dir
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

fn hd(dir: &Path, args: &[&str]) -> Ran {
    let output = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", crate::shared_objects(dir.join("cache")))
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

/// `hd test` shows a failing case's `dbg` lines with its failure and drops
/// a passing case's (`cli.dbg.test`).
#[test]
fn hd_test_shows_dbg_lines_of_a_failing_case_only() {
    let dir = scratch("hd-dbg-test");
    std::fs::write(
        dir.join("src/lib.hd"),
        include_str!("programs/dbg_failing_case.hd"),
    )
    .expect("write");
    let ran = hd(&dir, &["test"]);
    assert_eq!(ran.code, Some(1), "{}", ran.err);
    assert_eq!(
        crate::normalize_std_lines(&ran.out),
        "\
PANIC src/lib.hd:11: fails loudly
    panic: assertion-failed: double: actual 6, expected 7
    <std>/testing.hd:LINE std.testing.check_equal
    src/lib.hd:12 test \"fails loudly\"
    src/lib.hd:2:5: x = 3
    repro: hd test src/lib.hd --filter \"fails loudly\"
test result: FAILED. 1 passed; 1 failed; 0 ignored
"
    );
}
