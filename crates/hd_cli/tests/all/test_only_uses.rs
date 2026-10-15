//! Incremental soundness of uses that only test code makes (#238): a
//! `tests:` block, a test module, an integration test file, a doc test
//! and a dev dependency each use a module no library code uses. After a
//! warm run, an edit of that module's body or API reaches every test
//! program that reads it: a changed result fails the case, a changed
//! signature is reported at each call, never a stale pass.

use std::path::Path;
use std::process::{Command, Output};

use hd_testkit::Scratch;

const COINS: &str = "pub fn cents() -> i32:\n    100\n";

/// `total` and its doc test and `tests:` block; only test code uses
/// `money.coins`.
const LEDGER: &str = "\
## The total.
##
## ```hd
## use pkg.money.coins.{cents}
## use pkg.ledger.{total}
## use std.testing.assert_equal
##
## assert_equal(total() + cents(), 103, reason=\"doc\")
## ```
pub fn total() -> i32:
    3

tests:
    use pkg.money.coins.cents
    use std.testing.assert_equal

    it(\"adds cents\"):
        assert_equal(total() + cents(), 103, reason=\"block\")
";

const LEDGER_TEST: &str = "\
use pkg.money.coins.cents
use std.testing.assert_equal

it(\"checks cents\"):
    assert_equal(cents(), 100, reason=\"module\")
";

const BILL: &str = "\
use pkg.money.coins.cents
use std.testing.assert_equal

it(\"bills\"):
    assert_equal(cents(), 100, reason=\"file\")
";

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
}

fn hd(root: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", cache)
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run hd")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Runs `hd test` twice, cold then warm, and checks that both pass with
/// `summary`.
fn warm(root: &Path, cache: &Path, summary: &str) {
    for round in ["cold", "warm"] {
        let out = hd(root, cache, &["test"]);
        assert_eq!(text(&out.stdout), summary, "{round}: {}", text(&out.stderr));
        assert_eq!(out.status.code(), Some(0), "{round}");
    }
}

/// The `PANIC` lines of a failed `hd test`, after checking its status.
fn panics(root: &Path, cache: &Path) -> Vec<String> {
    let out = hd(root, cache, &["test"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    text(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("PANIC "))
        .map(str::to_owned)
        .collect()
}

/// The `argument-count` errors of a failed `hd test`, sorted, after
/// checking its status.
fn argument_counts(root: &Path, cache: &Path) -> Vec<String> {
    let out = hd(root, cache, &["test"]);
    assert_eq!(out.status.code(), Some(101), "{}", text(&out.stdout));
    let mut lines: Vec<String> = text(&out.stderr)
        .lines()
        .filter(|l| l.contains(": argument-count: "))
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

/// A `tests:` block, a test module, an integration test file and a doc
/// test that use `money.coins`, which no library code uses.
#[test]
fn an_edit_of_a_module_only_test_code_uses_reaches_every_test() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-test-only-uses");
    let cache = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "hd-test-only-uses-cache");
    write(
        &root,
        &[
            ("hd.toml", "[package]\nname = \"shop\"\n"),
            ("src/money/coins.hd", COINS),
            ("src/ledger.hd", LEDGER),
            ("src/ledger_test.hd", LEDGER_TEST),
            ("tests/bill.hd", BILL),
        ],
    );
    warm(
        &root,
        &cache,
        "test result: ok. 4 passed; 0 failed; 0 ignored\n",
    );

    // A body edit changes what each case computes.
    write(
        &root,
        &[("src/money/coins.hd", &COINS.replace("100", "200"))],
    );
    assert_eq!(
        panics(&root, &cache),
        [
            "PANIC src/ledger.hd:8: doc ledger.total[0]",
            "PANIC src/ledger.hd:17: adds cents",
            "PANIC src/ledger_test.hd:4: checks cents",
            "PANIC tests/bill.hd:4: bills",
        ]
    );

    // An API edit is reported at every call.
    write(
        &root,
        &[(
            "src/money/coins.hd",
            "pub fn cents(n: i32) -> i32:\n    100 * n\n",
        )],
    );
    assert_eq!(
        argument_counts(&root, &cache),
        [
            "error: src/ledger.hd:18:32: argument-count: `cents` takes 1 arguments",
            "error: src/ledger.hd:8:1: argument-count: `cents` takes 1 arguments",
            "error: src/ledger_test.hd:5:18: argument-count: `cents` takes 1 arguments",
            "error: tests/bill.hd:5:18: argument-count: `cents` takes 1 arguments",
        ]
    );
}

/// A dev dependency that a `tests:` block and an integration test file
/// use (`module.select.dev-dependencies`).
#[test]
fn an_edit_of_a_dev_dependency_reaches_its_tests() {
    let world = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-test-only-dev");
    let cache = world.join("cache");
    write(
        &world,
        &[
            (
                "shop/hd.toml",
                "[package]\nname = \"shop\"\n\n[dev-dependencies]\nfixtures = { path = \"../fixtures\" }\n",
            ),
            (
                "shop/src/ledger.hd",
                "pub fn total() -> i32:\n    3\n\ntests:\n    use dep.fixtures.sample\n    \
                 use std.testing.assert_equal\n\n    it(\"adds a sample\"):\n        \
                 assert_equal(total() + sample(), 13, reason=\"block\")\n",
            ),
            (
                "shop/tests/bill.hd",
                "use dep.fixtures.sample\nuse std.testing.assert_equal\n\n\
                 it(\"samples\"):\n    assert_equal(sample(), 10, reason=\"file\")\n",
            ),
            ("fixtures/hd.toml", "[package]\nname = \"fixtures\"\n"),
            ("fixtures/src/lib.hd", "pub fn sample() -> i32:\n    10\n"),
        ],
    );
    let shop = world.join("shop");
    warm(
        &shop,
        &cache,
        "test result: ok. 2 passed; 0 failed; 0 ignored\n",
    );

    write(
        &world,
        &[("fixtures/src/lib.hd", "pub fn sample() -> i32:\n    20\n")],
    );
    assert_eq!(
        panics(&shop, &cache),
        [
            "PANIC src/ledger.hd:8: adds a sample",
            "PANIC tests/bill.hd:4: samples",
        ]
    );

    write(
        &world,
        &[(
            "fixtures/src/lib.hd",
            "pub fn sample(n: i32) -> i32:\n    10 * n\n",
        )],
    );
    assert_eq!(
        argument_counts(&shop, &cache),
        [
            "error: src/ledger.hd:9:32: argument-count: `sample` takes 1 arguments",
            "error: tests/bill.hd:5:18: argument-count: `sample` takes 1 arguments",
        ]
    );
}
