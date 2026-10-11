//! Doc tests on `compiler/samples/doctests`: `hd test` runs each fenced
//! `hd` block of a `##` comment as a program of its own beside the
//! integration test file (`cli.test.doc.default`, `cli.test.program`),
//! names it `doc <module>.<item>[i]` (`cli.test.doc.name`), reports it at
//! its `##` line (`cli.test.doc.location`), and judges a compile-fail one
//! by its code; `hd check --tests` leaves that one out
//! (`cli.check.tests.doc`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/doctests")
}

fn hd(cache: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(cache),
        )
        .env_remove("HD_JOBS")
        .current_dir(package())
        .args(args)
        .output()
        .expect("run hd")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn doc_tests_run_with_the_integration_tests() {
    let out = hd("hd-cache-doc-all", &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
PANIC src/text.hd:16: doc text.slugify[1]
    panic: assertion-failed: stale: actual \"Ship-It\", expected \"Ship_It\"
    repro: hd test src/text.hd --filter \"doc text.slugify[1]\"
test result: FAILED. 3 passed; 1 failed; 0 ignored
",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn filter_doc_selects_the_doc_tests_by_name() {
    let out = hd(
        "hd-cache-doc-json",
        &["test", "--filter", "doc", "--format", "json"],
    );
    assert_eq!(
        text(&out.stdout),
        "\
{\"kind\":\"test\",\"name\":\"doc text.slugify[0]\",\"outcome\":\"passed\",\"message\":\"\"}
{\"kind\":\"test\",\"name\":\"doc text.slugify[1]\",\"outcome\":\"failed\",\"message\":\"panic: assertion-failed: stale: actual \\\"Ship-It\\\", expected \\\"Ship_It\\\"\"}
{\"kind\":\"test\",\"name\":\"doc text.slug_len[0]\",\"outcome\":\"passed\",\"message\":\"\"}
{\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":2,\"failed\":1,\"ignored\":0,\"status\":1}
",
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn an_integration_file_runs_alone() {
    let out = hd("hd-cache-doc-file", &["test", "tests/slugs.hd"]);
    assert_eq!(
        text(&out.stdout),
        "test result: ok. 1 passed; 0 failed; 0 ignored\n",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn check_tests_leaves_out_the_compile_fail_doc_test() {
    let out = hd("hd-cache-doc-check", &["check", "--tests"]);
    assert_eq!(
        text(&out.stderr),
        "check result: ok. errors: 0; warnings: 0\n"
    );
    assert_eq!(out.status.code(), Some(0));
}
