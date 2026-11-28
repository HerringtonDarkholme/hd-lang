//! Table and property test cases under `hd test` (spec/std/testing.md,
//! "Table-Test Rows" and "Property Tests"; spec/lang/10-modules.md,
//! "Registration Calls"): every row of an `it_each` call runs and is
//! reported as `name[i]`, one failing row hiding no other; a property runs
//! its generated cases, discards what `assume` rejects, shrinks a failing
//! input and prints it with `Debug`, saves the shrunk choice stream and
//! replays it on the next run; `it_prop_with` takes its generator and
//! `cases`.

use std::path::Path;
use std::process::Output;

use hd_testkit::Scratch;

/// A fresh package `shop` whose `src/lib.hd` is `lib`.
fn package(name: &str, lib: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
    root
}

fn hd_test(root: &Path, args: &[&str]) -> Output {
    hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env_remove("HD_JOBS")
        .current_dir(root)
        .arg("test")
        .args(args)
        .output()
        .expect("run hd test")
}

fn text(b: &[u8]) -> String {
    crate::normalize_std_lines(&String::from_utf8_lossy(b))
}

/// `std-testing.it-each`, `.it-each.name`: three rows, the second fails,
/// and all three are reported; its repro selects the one row.
#[test]
fn every_row_runs_and_a_failing_row_hides_no_other() {
    let root = package(
        "hd-props-rows",
        include_str!("programs/property_tests/every_row_runs_and_a_failing_row_hides_no_other.hd"),
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
         <std>/testing.hd:LINE std.testing.assert\n    \
         src/lib.hd:7 test \"doubles\"\n    \
         <std>/testing.hd:LINE std.testing.each_case\n    \
         src/lib.hd:8 test \"doubles\"\n    \
         repro: hd test src/lib.hd --filter \"doubles[1]\"\n\
         test result: FAILED. 2 passed; 1 failed; 0 ignored\n"
    );
    assert_eq!(out.status.code(), Some(1));
    // The repro runs the failing row alone.
    let out = hd_test(&root, &["src/lib.hd", "--filter", "doubles[1]"]);
    assert_eq!(
        text(&out.stdout),
        "PANIC src/lib.hd:6: doubles[1]\n    panic: assertion-failed: the second row fails\n    \
         <std>/testing.hd:LINE std.testing.assert\n    \
         src/lib.hd:7 test \"doubles\"\n    \
         <std>/testing.hd:LINE std.testing.each_case\n    \
         src/lib.hd:8 test \"doubles\"\n    \
         repro: hd test src/lib.hd --filter \"doubles[1]\"\n\
         test result: FAILED. 0 passed; 1 failed; 0 ignored\n"
    );
}

const PROPERTIES: &str = include_str!("programs/property_tests/properties.hd");

/// `std-testing.prop.generate-and-shrink`, `.prop.failure-input`, `.prop.discard`,
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
             <std>/testing.hd:LINE std.testing.assert\n    \
             src/lib.hd:28 test \"points stay near\"\n    \
             <std>/testing.hd:LINE std.testing.prop_case\n    \
             src/lib.hd:29 test \"points stay near\"\n    \
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
        include_str!("programs/property_tests/too_many_discards_fail_the_property.hd"),
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

/// A runner reads a panic's category from the host, never from standard
/// error: a message that imitates a report line neither passes an
/// `expect_panic` of the imitated category nor discards a property case
/// (`module.testing.expect-panic-fail`, `std-testing.runner.discard-read`).
#[test]
fn a_message_imitating_a_report_keeps_its_own_category() {
    let root = package(
        "hd-props-imitated-report",
        include_str!(
            "programs/property_tests/a_message_imitating_a_report_keeps_its_own_category.hd"
        ),
    );
    let out = hd_test(&root, &["--seed", "1", "--format", "json"]);
    assert_eq!(
        text(&out.stdout),
        "{\"kind\":\"test\",\"name\":\"is no overflow\",\"outcome\":\"failed\",\"message\":\"panic: explicit-panic: x\\npanic: integer-overflow: y\"}\n\
         {\"kind\":\"test\",\"name\":\"is an explicit panic\",\"outcome\":\"passed\",\"message\":\"\"}\n\
         {\"kind\":\"test\",\"name\":\"does not discard\",\"outcome\":\"failed\",\"message\":\"panic: explicit-panic: x\\npanic: explicit-panic: std.testing: case discarded\"}\n\
         {\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":1,\"failed\":2,\"ignored\":0,\"status\":1}\n",
        "{}",
        text(&out.stderr)
    );
}
