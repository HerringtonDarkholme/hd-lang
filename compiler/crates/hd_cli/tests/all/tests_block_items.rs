//! The items of a `tests:` block (spec/lang/02-grammar.md "Test Blocks",
//! spec/lang/03-names-and-scopes.md "Tests Blocks"): a helper function, a
//! helper data type and a use declared in the block serve its test cases
//! (`grammar.tests.item-forms`, `names.tests.sees-module`); non-test code
//! that names a test item gets `unknown-name` (`names.tests.inside-only`);
//! and an integration test, which sees the package built without its test
//! code (`module.test.integration.view`), cannot import one.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The library with a `tests:` block that declares a use, a data type and
/// a function, which its test cases use.
const LIB: &str = "\
pub fn late_fee(days: i32) -> i32:
    if days > 30: 5 else: 0

tests:
    use std.testing.assert_equal

    data Invoice:
        days: i32

    fn overdue() -> Invoice:
        Invoice { days: 31 }

    it(\"charges a fee after 30 days\"):
        assert_equal(late_fee(overdue().days), 5, reason=\"one day late\")

    it(\"charges nothing on time\"):
        assert_equal(late_fee(Invoice { days: 30 }.days), 0, reason=\"thirty days is on time\")
";

/// A fresh package `shop` whose `src/lib.hd` is `lib`, with the given
/// integration test files.
fn package(name: &str, lib: &str, tests: &[(&str, &str)]) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("cache"));
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(root.join("tests")).expect("tests");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
    for (file, text) in tests {
        std::fs::write(root.join("tests").join(file), text).expect("test file");
    }
    root
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

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn test_cases_use_the_block_s_helpers_and_use() {
    let root = package("tests-block-helpers", LIB, &[]);
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "test result: ok. 2 passed; 0 failed; 0 ignored\n",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0));
    let out = hd(&root, &["check", "--tests"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

/// Non-test code sees no test item, whether or not test code is checked.
#[test]
fn non_test_code_cannot_name_a_test_item() {
    let lib = format!("{LIB}\nfn report() -> i32:\n    overdue().days\n");
    let root = package("tests-block-outside", &lib, &[]);
    for args in [&["check", "--tests"][..], &["check"][..]] {
        let out = hd(&root, args);
        let err = text(&out.stderr);
        assert_eq!(out.status.code(), Some(101), "{args:?}: {err}");
        assert!(err.contains("unknown-name"), "{args:?}: {err}");
        assert!(err.contains("overdue"), "{args:?}: {err}");
    }
}

/// An integration test sees the package built without its test code, so
/// a test item is no declaration of it.
#[test]
fn an_integration_test_cannot_import_a_test_item() {
    let test = "\
use pkg.overdue
use std.testing.assert

it(\"sees no test item\"):
    assert(overdue().days > 0, reason=\"a test item\")
";
    let root = package("tests-block-integration", LIB, &[("fees.hd", test)]);
    let out = hd(&root, &["test"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(101), "{err}");
    assert!(err.contains("unknown-import"), "{err}");
}
