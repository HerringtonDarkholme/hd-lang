//! Symbolized panic frames in `hd test` text reports
//! (`cli.test.report.panic`, `cli.test.report.frame`, wasm-layout.md
//! §15.9): a `PANIC` block lists `FILE:LINE FUNCTION` lines after its
//! `panic:` line, panic site first. An explicit panic starts at the call of
//! `panic`, never inside the runtime's stub; a panic in std code starts in
//! std. A passing run and the JSON report show no frames, and a comment
//! edit moves the reported lines without a rebuild of the code.

use std::path::Path;
use std::process::{Command, Output};

use hd_testkit::Scratch;

const LIB: &str = "\
use std.testing.assert

fn total(prices: List[i32]) -> i32:
    let sum = +0
    for p in prices:
        sum += p
    assert(sum == 10, \"total is 10\")
    sum

fn boom() -> void:
    panic(\"out of stock\")

fn drop_far(xs: mut List[i32]) -> i32:
    xs.remove_at(5)

tests:
    it(\"passes\"):
        assert(total([4, 6]) == 10, \"ten\")

    it(\"sums prices\"):
        total([1, 2, 3])

    it(\"runs out\"):
        boom()

    it(\"drops past the end\"):
        let xs: mut List[i32] = [1, 2]
        drop_far(xs)
";

/// A fresh package `shop` whose `src/lib.hd` is `lib`.
fn package(name: &str, lib: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
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

/// A helper's failed assertion lists std's `assert`, the helper, then the
/// case; an explicit panic starts at its `panic` call; an index past the
/// end of a list in std code starts in std, then the user's frames.
#[test]
fn a_panic_lists_its_frames_panic_site_first() {
    let root = package("hd-frames-report", LIB);
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
PANIC src/lib.hd:20: sums prices
    panic: assertion-failed: total is 10
    <std>/testing.hd:594 std/testing/assert
    src/lib.hd:7 shop/total
    src/lib.hd:21 shop/$test1
    repro: hd test src/lib.hd --filter \"sums prices\"
PANIC src/lib.hd:23: runs out
    panic: explicit-panic: out of stock
    src/lib.hd:11 shop/boom
    src/lib.hd:24 shop/$test2
    repro: hd test src/lib.hd --filter \"runs out\"
PANIC src/lib.hd:26: drops past the end
    panic: index-out-of-bounds: list remove_at index 5 out of bounds for length 2
    <std>/collections.hd:49 std/collections/impl [ T ] List [ T ].remove_at
    src/lib.hd:14 shop/drop_far
    src/lib.hd:28 shop/$test3
    repro: hd test src/lib.hd --filter \"drops past the end\"
test result: FAILED. 1 passed; 3 failed; 0 ignored
",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1));
}

/// Frames are a text-report feature: a passing run prints none, and a JSON
/// test object carries the category and message only.
#[test]
fn passes_and_json_show_no_frames() {
    let root = package("hd-frames-quiet", LIB);
    let out = hd(&root, &["test", "--filter", "passes"]);
    assert_eq!(
        text(&out.stdout),
        "test result: ok. 1 passed; 0 failed; 0 ignored\n"
    );
    let out = hd(&root, &["test", "--filter", "runs out", "--format", "json"]);
    assert_eq!(
        text(&out.stdout).lines().next(),
        Some(
            "{\"kind\":\"test\",\"name\":\"runs out\",\"outcome\":\"failed\",\"message\":\"panic: explicit-panic: out of stock\"}"
        ),
        "{}",
        text(&out.stderr)
    );
}

/// Lines are read from the current sources when a frame is printed, so a
/// comment added above a panic moves its line on a warm run.
#[test]
fn a_comment_edit_moves_the_reported_lines() {
    let root = package("hd-frames-edit", LIB);
    let frames = |root: &Path| {
        let out = hd(root, &["test", "--filter", "runs out"]);
        text(&out.stdout)
            .lines()
            .filter(|l| l.contains(" shop/"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        frames(&root),
        [
            "    src/lib.hd:11 shop/boom",
            "    src/lib.hd:24 shop/$test2"
        ]
    );
    let edited = LIB.replace("fn boom()", "# Runs out.\n\nfn boom()");
    std::fs::write(root.join("src/lib.hd"), edited).expect("edit");
    assert_eq!(
        frames(&root),
        [
            "    src/lib.hd:13 shop/boom",
            "    src/lib.hd:26 shop/$test2"
        ]
    );
}

/// The custom sections of a module, by name, in order.
fn custom_sections(wasm: &[u8]) -> Vec<String> {
    fn leb(b: &[u8], at: &mut usize) -> usize {
        let mut v = 0;
        let mut shift = 0;
        loop {
            let byte = b[*at];
            *at += 1;
            v |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }
    let mut out = Vec::new();
    let mut at = 8;
    while at < wasm.len() {
        let id = wasm[at];
        at += 1;
        let size = leb(wasm, &mut at);
        let end = at + size;
        if id == 0 {
            let mut name_at = at;
            let n = leb(wasm, &mut name_at);
            out.push(text(&wasm[name_at..name_at + n]));
        }
        at = end;
    }
    out
}

/// A debug build keeps the standard `name` section beside `hd.lines`; a
/// release build drops it for the compact `hd.names`, so its panics still
/// name their functions (wasm-layout.md §15.5).
#[test]
fn release_builds_name_functions_in_hd_names() {
    let root = package(
        "hd-frames-release",
        "pub fn main() -> void $ Console:\n    println(42)\n",
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    let out = hd(&root, &["build"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let debug = std::fs::read(root.join("build/debug/shop.wasm")).expect("debug wasm");
    assert_eq!(custom_sections(&debug), ["hd.lines", "name"]);
    let out = hd(&root, &["build", "--release"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let release = std::fs::read(root.join("build/release/shop.wasm")).expect("release wasm");
    assert_eq!(custom_sections(&release), ["hd.lines", "hd.names"]);
    assert!(hd_run::frames::LineTable::parse(&release).is_ok());
}
