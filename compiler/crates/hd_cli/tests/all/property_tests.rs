//! Table and property test cases under `hd test` (spec/std/testing.md,
//! "Table-Test Rows" and "Property Tests"; spec/lang/10-modules.md,
//! "Registration Calls"): every row of an `it_each` call runs and is
//! reported as `name[i]`, one failing row hiding no other; a property runs
//! its generated cases, discards what `assume` rejects, shrinks a failing
//! input and prints it with `Debug`, saves the shrunk choice stream and
//! replays it on the next run; `it_prop_with` takes its generator and
//! `cases`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh package `shop` whose `src/lib.hd` is `lib`.
fn package(name: &str, lib: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
    root
}

fn hd_test(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", root.with_extension("cache"))
        .env_remove("HD_JOBS")
        .current_dir(root)
        .arg("test")
        .args(args)
        .output()
        .expect("run hd test")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `std-testing.it-each`, `.it-each.name`: three rows, the second fails,
/// and all three are reported; its repro selects the one row.
#[test]
fn every_row_runs_and_a_failing_row_hides_no_other() {
    let root = package(
        "hd-props-rows",
        "pub fn double(value: i32) -> i32: value * 2\n\n\
         tests:\n    use std.testing.{assert, it_each}\n\n    \
         it_each(\"doubles\", [1, 2, 3], body=fn!(value: i32):\n        \
         assert(double(value) != 4, reason=\"the second row fails\")\n    )\n",
    );
    let out = hd_test(&root, &["--format", "json"]);
    assert_eq!(
        text(&out.stdout),
        "{\"kind\":\"test\",\"name\":\"doubles[0]\",\"outcome\":\"passed\",\"message\":\"\"}\n\
         {\"kind\":\"test\",\"name\":\"doubles[1]\",\"outcome\":\"failed\",\"message\":\"panic: assertion-failed: the second row fails\"}\n\
         {\"kind\":\"test\",\"name\":\"doubles[2]\",\"outcome\":\"passed\",\"message\":\"\"}\n\
         {\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":2,\"failed\":1,\"ignored\":0,\"status\":1}\n",
        "{}",
        text(&out.stderr)
    );
    let out = hd_test(&root, &[]);
    assert_eq!(
        text(&out.stdout),
        "PANIC src/lib.hd:6: doubles[1]\n    panic: assertion-failed: the second row fails\n    \
         repro: hd test src/lib.hd --filter \"doubles[1]\"\n\
         test result: FAILED. 2 passed; 1 failed; 0 ignored\n"
    );
    assert_eq!(out.status.code(), Some(1));
    // The repro runs the failing row alone.
    let out = hd_test(&root, &["src/lib.hd", "--filter", "doubles[1]"]);
    assert_eq!(
        text(&out.stdout),
        "PANIC src/lib.hd:6: doubles[1]\n    panic: assertion-failed: the second row fails\n    \
         repro: hd test src/lib.hd --filter \"doubles[1]\"\n\
         test result: FAILED. 0 passed; 1 failed; 0 ignored\n"
    );
}

const PROPERTIES: &str = "use std.testing.{Arbitrary, Choices}\n\n\
@derive(Debug)\n\
pub data Point:\n    x: i64\n    y: i64\n\n\
impl Arbitrary for Point:\n    fn arbitrary(c: mut Choices) -> Point:\n        \
Point { x: c.int(0, 1000), y: c.int(0, 1000) }\n\n\
fn even(c: mut Choices) -> i64:\n    let value: i64 = c.int(0, 100)\n    \
c.assume(value % 2 == 0)\n    value\n\n\
fn small(c: mut Choices) -> i64:\n    c.int(0, 9)\n\n\
tests:\n    use std.testing.{assert, assert_equal, it_prop, it_prop_with}\n\n    \
it_prop(\"adding zero keeps a value\", prop=fn!(value: i32):\n        \
assert_equal(value + 0, value, reason=\"zero\")\n    )\n\n    \
it_prop(\"points stay near\", prop=fn!(p: Point):\n        \
assert(p.x < 50 || p.y < 30, reason=\"a far point\")\n    )\n\n    \
it_prop_with(\"only even values are checked\", gen=even, prop=fn!(n: i64):\n        \
assert(n % 2 == 0, reason=\"assume discards odd values\")\n    )\n\n    \
it_prop_with(\"eight small values\", gen=small, cases=8, prop=fn!(n: i64):\n        \
assert(n >= 0 && n <= 9, reason=\"from 0 to 9\")\n    )\n";

/// `std-testing.it-prop`, `.prop.report`, `.prop.discard`,
/// `.prop.regression-file`, `.prop.regression-format`,
/// `.prop.regression-replay`, `cli.test.seed.report`: a passing property, a
/// failing one shrunk to its least input and printed with `Debug`, an
/// `assume` that discards, and `it_prop_with` with a generator and
/// `cases=8`. The shrunk stream is saved, one draw per line, and a run
/// with another seed replays it first.
#[test]
fn a_failing_property_shrinks_saves_and_replays() {
    let root = package("hd-props-shrink", PROPERTIES);
    let failure = |seed: &str| {
        format!(
            "PANIC src/lib.hd:27: points stay near\n    panic: assertion-failed: a far point\n    \
             input: Point {{ x: 50, y: 30 }}\n    \
             repro: hd test src/lib.hd --filter \"points stay near\" --seed {seed}\n"
        )
    };
    let out = hd_test(&root, &["--seed", "7"]);
    assert_eq!(
        text(&out.stdout),
        failure("7") + "test result: FAILED. 3 passed; 1 failed; 0 ignored\n",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1));
    let saved = root.join("__regressions__/pkg/points-stay-near");
    assert_eq!(std::fs::read_to_string(&saved).expect("saved"), "50\n30\n");
    // Another seed replays the saved stream before any new case.
    let out = hd_test(&root, &["--seed", "123", "--filter", "points"]);
    assert_eq!(
        text(&out.stdout),
        failure("123") + "test result: FAILED. 0 passed; 1 failed; 0 ignored\n"
    );
}

/// `std-testing.prop.discard-limit`: a generator that discards every case
/// fails its property once more than 10 times `cases` are discarded.
#[test]
fn too_many_discards_fail_the_property() {
    let root = package(
        "hd-props-discards",
        "use std.testing.Choices\n\n\
         fn rejects(c: mut Choices) -> i64:\n    c.assume(false)\n    0\n\n\
         tests:\n    use std.testing.it_prop_with\n\n    \
         it_prop_with(\"never runs its body\", gen=rejects, cases=3, prop=fn!(n: i64):\n        \
         pass\n    )\n",
    );
    let out = hd_test(&root, &["--seed", "1"]);
    assert_eq!(
        text(&out.stdout),
        "FAIL src/lib.hd:10: never runs its body\n    \
         the property discarded 31 cases, more than 10 times its 3 cases\n    \
         repro: hd test src/lib.hd --filter \"never runs its body\" --seed 1\n\
         test result: FAILED. 0 passed; 1 failed; 0 ignored\n",
        "{}",
        text(&out.stderr)
    );
}
