//! `hd test --affected` (spec/cli/command-line.md, "Affected Tests"): a
//! full pass records each test program's fingerprint
//! (`cli.test.affected.record`), and `--affected` then runs only the
//! programs whose fingerprint changed (`cli.test.affected`,
//! `cli.test.fingerprint`). A failing case removes its program's record
//! (`cli.test.affected.failed`), a filtered run records nothing, and
//! `hd clean` removes the records (`cli.test.affected.where`).
//!
//! The package has five test programs (`cli.test.program`): the unit tests
//! of `src/billing.hd`, of `src/invoice.hd`, which uses billing, and of
//! `src/stock.hd`, and the integration test files `tests/bill.hd`, which
//! uses billing, and `tests/count.hd`, which uses stock.

use hd_base::SliceSort as _;
use std::path::Path;
use std::process::{Command, Output};

use hd_testkit::Scratch;

const BILLING: &str = "use std.testing.assert_equal\n\npub fn price() -> i32:\n    3\n\n\
                       tests:\n    it(\"prices\"):\n        assert_equal(price(), 3, reason=\"p\")\n";

const STOCK: &str = "use std.testing.assert_equal\n\npub fn stock() -> i32:\n    5\n\n\
                     tests:\n    it(\"stocks\"):\n        assert_equal(stock(), 5, reason=\"s\")\n";

/// A fresh package with the five test programs.
fn package(name: &str) -> Scratch {
    let root = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(root.join("tests")).expect("tests");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(root.join("src/billing.hd"), BILLING).expect("billing");
    std::fs::write(root.join("src/stock.hd"), STOCK).expect("stock");
    std::fs::write(
        root.join("src/invoice.hd"),
        "use pkg.billing.price\nuse std.testing.assert_equal\n\n\
         tests:\n    it(\"invoices\"):\n        assert_equal(price() * 2, 6, reason=\"i\")\n",
    )
    .expect("invoice");
    std::fs::write(
        root.join("tests/bill.hd"),
        "use pkg.billing.price\nuse std.testing.assert_equal\n\n\
         it(\"bills\"):\n    assert_equal(price(), 3, reason=\"b\")\n",
    )
    .expect("bill");
    std::fs::write(
        root.join("tests/count.hd"),
        "use pkg.stock.stock\nuse std.testing.assert_equal\n\n\
         it(\"counts\"):\n    assert_equal(stock(), 5, reason=\"c\")\n",
    )
    .expect("count");
    root
}

fn hd(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", root.with_extension("cache"))
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

/// Runs `hd test ARGS` and returns its summary line, after checking that
/// it passed.
fn summary(root: &Path, args: &[&str]) -> String {
    let out = hd(root, &[&["test"], args].concat());
    assert_eq!(
        out.status.code(),
        Some(0),
        "{args:?}: {}",
        text(&out.stderr)
    );
    text(&out.stdout)
}

/// The names of the cases `hd test --format json ARGS` ran, sorted.
fn ran(root: &Path, args: &[&str]) -> Vec<String> {
    let out = hd(root, &[&["test", "--format", "json"], args].concat());
    let mut names: Vec<String> = text(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("{\"kind\":\"test\",\"name\":\""))
        .filter_map(|l| l.split_once('"').map(|(n, _)| n.to_owned()))
        .collect();
    names.sort_stable();
    names
}

const ALL_PASS: &str = "test result: ok. 5 passed; 0 failed; 0 ignored\n";
const ALL_SKIPPED: &str = "test result: ok. 0 passed; 0 failed; 0 ignored; 5 programs skipped\n";

/// `cli.test.affected.record`, `cli.test.affected`: after a full pass,
/// `--affected` runs nothing and says how many programs it skipped.
#[test]
fn a_full_pass_records_and_affected_then_runs_nothing() {
    let root = package("hd-affected-pass");
    assert_eq!(summary(&root, &[]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected"]), ALL_SKIPPED);
    assert_eq!(summary(&root, &["--affected"]), ALL_SKIPPED);
    // Without `--affected`, every program runs.
    assert_eq!(summary(&root, &[]), ALL_PASS);
}

/// `cli.test.fingerprint`: an edit of `src/billing.hd` reruns its unit
/// tests and those of `src/invoice.hd` and `tests/bill.hd`, which reach
/// it, and skips `src/stock.hd` and `tests/count.hd`, which do not.
#[test]
fn an_edit_reruns_only_the_programs_that_reach_it() {
    let root = package("hd-affected-edit");
    assert_eq!(summary(&root, &[]), ALL_PASS);
    std::fs::write(
        root.join("src/billing.hd"),
        BILLING.replace("    3\n", "    1 + 2\n"),
    )
    .expect("edit");
    assert_eq!(ran(&root, &["--affected"]), ["bills", "invoices", "prices"]);
    // That run passed in full, so it recorded the new fingerprints.
    assert_eq!(summary(&root, &["--affected"]), ALL_SKIPPED);
    // An integration test file's edit reruns that file alone.
    let count = root.join("tests/count.hd");
    let edited = std::fs::read_to_string(&count).expect("count") + "\n# edited\n";
    std::fs::write(&count, edited).expect("edit count");
    assert_eq!(ran(&root, &["--affected"]), ["counts"]);
    // A data file of the package reaches the integration tests alone,
    // which may read it.
    std::fs::write(root.join("orders.csv"), "1,2\n").expect("data");
    assert_eq!(ran(&root, &["--affected"]), ["bills", "counts"]);
}

/// `cli.test.affected.failed`: a failing case removes its program's
/// record, so after the fix is undone `--affected` still runs it.
#[test]
fn a_failing_case_removes_the_record() {
    let root = package("hd-affected-fail");
    assert_eq!(summary(&root, &[]), ALL_PASS);
    let stock = root.join("src/stock.hd");
    std::fs::write(&stock, STOCK.replace("    5\n", "    6\n")).expect("break");
    let out = hd(&root, &["test"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).ends_with("test result: FAILED. 3 passed; 2 failed; 0 ignored\n"),
        "{}",
        text(&out.stdout)
    );
    // Back to the text whose fingerprint was recorded: the failure removed
    // the records, so both programs that reach stock run again.
    std::fs::write(&stock, STOCK).expect("restore");
    assert_eq!(ran(&root, &["--affected"]), ["counts", "stocks"]);
}

/// `cli.test.affected.record`: a filtered run records nothing, also for
/// the programs whose every case it ran.
#[test]
fn a_filtered_run_records_nothing() {
    let root = package("hd-affected-filter");
    let out = summary(&root, &["--filter", "s"]);
    assert_eq!(out, ALL_PASS);
    assert_eq!(summary(&root, &["--affected"]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected"]), ALL_SKIPPED);
    // `--affected` with a filter runs the filtered cases of the changed
    // programs, and records nothing either.
    std::fs::write(
        root.join("src/billing.hd"),
        BILLING.replace("    3\n", "    1 + 2\n"),
    )
    .expect("edit");
    assert_eq!(ran(&root, &["--affected", "--filter", "bill"]), ["bills"]);
    assert_eq!(ran(&root, &["--affected"]), ["bills", "invoices", "prices"]);
}

/// `cli.test.fingerprint`: a test option such as `--seed` or `--release`
/// is part of every fingerprint, so changing it reruns every program.
#[test]
fn seed_and_release_rerun_everything() {
    let root = package("hd-affected-options");
    assert_eq!(summary(&root, &[]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected", "--seed", "7"]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected", "--seed", "7"]), ALL_SKIPPED);
    assert_eq!(summary(&root, &["--affected", "--seed", "8"]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected", "--release"]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected", "--release"]), ALL_SKIPPED);
    assert_eq!(summary(&root, &["--affected"]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected"]), ALL_SKIPPED);
}

/// `cli.test.affected.where`: the records live in the build directory, so
/// after `hd clean`, `--affected` runs every program.
#[test]
fn hd_clean_resets_the_records() {
    let root = package("hd-affected-clean");
    assert_eq!(summary(&root, &[]), ALL_PASS);
    assert!(root.join("build").is_dir());
    let out = hd(&root, &["clean"]);
    assert_eq!(text(&out.stdout), "removed build\n");
    assert_eq!(summary(&root, &["--affected"]), ALL_PASS);
    assert_eq!(summary(&root, &["--affected"]), ALL_SKIPPED);
}

/// `cli.test.affected` with a FILE: `hd test FILE` records the programs
/// of FILE's module, which a whole-package `--affected` then skips.
#[test]
fn a_file_run_records_its_programs() {
    let root = package("hd-affected-file");
    assert_eq!(
        summary(&root, &["src/billing.hd"]),
        "test result: ok. 1 passed; 0 failed; 0 ignored\n"
    );
    assert_eq!(
        ran(&root, &["--affected"]),
        ["bills", "counts", "invoices", "stocks"]
    );
    assert_eq!(
        summary(&root, &["src/stock.hd", "--affected"]),
        "test result: ok. 0 passed; 0 failed; 0 ignored; 1 program skipped\n"
    );
}

/// `cli.test.fingerprint`: the engine is part of every fingerprint, and
/// `--affected` works on wasmtime as on Node.
#[cfg(feature = "wasmtime")]
#[test]
fn affected_works_on_wasmtime_and_the_engine_reruns() {
    let root = package("hd-affected-wasmtime");
    let wasmtime = ["--engine", "wasmtime"];
    assert_eq!(summary(&root, &wasmtime), ALL_PASS);
    assert_eq!(
        summary(&root, &["--engine", "wasmtime", "--affected"]),
        ALL_SKIPPED
    );
    std::fs::write(
        root.join("src/billing.hd"),
        BILLING.replace("    3\n", "    1 + 2\n"),
    )
    .expect("edit");
    assert_eq!(
        ran(&root, &["--engine", "wasmtime", "--affected"]),
        ["bills", "invoices", "prices"]
    );
    assert_eq!(summary(&root, &["--affected"]), ALL_PASS);
}

/// `cli.test.fingerprint` for uses only test code makes (#238): a
/// `tests:` block, a test module, an integration test file and a doc test
/// use `money.coins`, which no library code uses, and an integration test
/// file uses a dev dependency. An edit of either reruns the programs
/// that read it.
#[test]
fn an_edit_reruns_the_programs_whose_test_code_reaches_it() {
    let world = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-affected-test-only");
    let root = world.join("shop");
    let files: [(&str, &str); 9] = [
        (
            "shop/hd.toml",
            "[package]\nname = \"shop\"\n\n[dev-dependencies]\nfixtures = { path = \"../fixtures\" }\n",
        ),
        (
            "shop/src/money/coins.hd",
            "pub fn cents() -> i32:\n    100\n",
        ),
        ("shop/src/stock.hd", STOCK),
        (
            "shop/src/ledger.hd",
            "## The total.\n##\n## ```hd\n## use pkg.money.coins.{cents}\n## use pkg.ledger.{total}\n\
             ## use std.testing.assert_equal\n##\n\
             ## assert_equal(total() + cents(), 103, reason=\"doc\")\n## ```\n\
             pub fn total() -> i32:\n    3\n\n\
             tests:\n    use pkg.money.coins.cents\n    use std.testing.assert_equal\n\n    \
             it(\"adds cents\"):\n        assert_equal(total() + cents(), 103, reason=\"block\")\n",
        ),
        (
            "shop/src/ledger_test.hd",
            "use pkg.money.coins.cents\nuse std.testing.assert_equal\n\n\
             it(\"checks cents\"):\n    assert_equal(cents(), 100, reason=\"module\")\n",
        ),
        (
            "shop/tests/bill.hd",
            "use pkg.money.coins.cents\nuse std.testing.assert_equal\n\n\
             it(\"bills\"):\n    assert_equal(cents(), 100, reason=\"file\")\n",
        ),
        (
            "shop/tests/sample.hd",
            "use dep.fixtures.sample\nuse std.testing.assert_equal\n\n\
             it(\"samples\"):\n    assert_equal(sample(), 10, reason=\"dev\")\n",
        ),
        ("fixtures/hd.toml", "[package]\nname = \"fixtures\"\n"),
        ("fixtures/src/lib.hd", "pub fn sample() -> i32:\n    10\n"),
    ];
    for (path, text) in files {
        let path = world.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    assert_eq!(
        summary(&root, &[]),
        "test result: ok. 6 passed; 0 failed; 0 ignored\n"
    );
    std::fs::write(
        root.join("src/money/coins.hd"),
        "pub fn cents() -> i32:\n    50 + 50\n",
    )
    .expect("edit");
    assert_eq!(
        ran(&root, &["--affected"]),
        ["adds cents", "bills", "checks cents", "doc ledger.total[0]"]
    );
    std::fs::write(
        world.join("fixtures/src/lib.hd"),
        "pub fn sample() -> i32:\n    5 + 5\n",
    )
    .expect("edit");
    assert!(ran(&root, &["--affected"]).contains(&"samples".to_owned()));
}
