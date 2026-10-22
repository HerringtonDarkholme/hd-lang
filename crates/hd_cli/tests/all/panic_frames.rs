//! Symbolized panic frames in `hd test` text reports
//! (`cli.test.report.panic`, `cli.test.report.frame`, `crates/hd_wasm_linear/DESIGN.md`
//! §15.9): a `PANIC` block lists `FILE:LINE FUNCTION` lines after its
//! `panic:` line, panic site first. An explicit panic starts at the call of
//! `panic`, never inside the runtime's stub; a panic in std code starts in
//! std. A passing run and the JSON report show no frames, and a comment
//! edit moves the reported lines without a rebuild of the code.
//!
//! A frame names its function as hd names it: the package name with `-`
//! written `_` (a single file's stem), the module path, then the function,
//! a method after its type's name; a test case's body is `test "NAME"`
//! (trait.typeid.name.package, trait.typeid.name.single-file). Repeated
//! frames print once, then how many more times they repeat.

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

fn drop_far(xs: &mut List[i32]) -> i32:
    xs.remove_at(5)

tests:
    it(\"passes\"):
        assert(total([4, 6]) == 10, \"ten\")

    it(\"sums prices\"):
        total([1, 2, 3])

    it(\"runs out\"):
        boom()

    it(\"drops past the end\"):
        let mut xs: List[i32] = [1, 2]
        drop_far(&mut xs)
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
    <std>/testing.hd:626 std.testing.assert
    src/lib.hd:7 shop.total
    src/lib.hd:21 test \"sums prices\"
    repro: hd test src/lib.hd --filter \"sums prices\"
PANIC src/lib.hd:23: runs out
    panic: explicit-panic: out of stock
    src/lib.hd:11 shop.boom
    src/lib.hd:24 test \"runs out\"
    repro: hd test src/lib.hd --filter \"runs out\"
PANIC src/lib.hd:26: drops past the end
    panic: index-out-of-bounds: list remove_at index 5 out of bounds for length 2
    <std>/collections.hd:52 std.collections.List.remove_at
    src/lib.hd:14 shop.drop_far
    src/lib.hd:28 test \"drops past the end\"
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
            .filter(|l| l.starts_with("    src/"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        frames(&root),
        [
            "    src/lib.hd:11 shop.boom",
            "    src/lib.hd:24 test \"runs out\""
        ]
    );
    let edited = LIB.replace("fn boom()", "# Runs out.\n\nfn boom()");
    std::fs::write(root.join("src/lib.hd"), edited).expect("edit");
    assert_eq!(
        frames(&root),
        [
            "    src/lib.hd:13 shop.boom",
            "    src/lib.hd:26 test \"runs out\""
        ]
    );
}

/// A method names its type, then itself, after its module's path: an
/// inherent method and a trait's method alike. A package name's `-` is
/// written `_`, and an integration test's module has no `$tests` segment.
#[test]
fn a_method_frame_names_its_type_and_module() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-frames-methods");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(root.join("tests")).expect("tests");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"corner-shop\"\n").expect("hd.toml");
    std::fs::write(
        root.join("src/cart.hd"),
        "\
pub struct Cart:
    pub items: List[i32]

impl Cart:
    pub fn take(self, i: usize) -> i32:
        self.items[i]

impl Display for Cart:
    fn to_string(self) -> string:
        panic(\"no display\")

tests:
    use std.testing.assert

    it(\"takes past the end\"):
        let c = Cart { items: [1] }
        c.take(4)

    it(\"shows\"):
        let c = Cart { items: [1] }
        assert(\"${c}\" == \"\", \"shown\")
",
    )
    .expect("cart");
    std::fs::write(
        root.join("tests/flow.hd"),
        "\
use pkg.cart.Cart

fn first(c: Cart) -> i32:
    c.take(2)

it(\"flows\"):
    first(Cart { items: [] })
",
    )
    .expect("flow");
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
PANIC src/cart.hd:15: takes past the end
    panic: index-out-of-bounds: list index out of bounds
    src/cart.hd:6 corner_shop.cart.Cart.take
    src/cart.hd:17 test \"takes past the end\"
    repro: hd test src/cart.hd --filter \"takes past the end\"
PANIC src/cart.hd:19: shows
    panic: explicit-panic: no display
    src/cart.hd:10 corner_shop.cart.Cart.to_string
    src/cart.hd:21 test \"shows\"
    repro: hd test src/cart.hd --filter \"shows\"
PANIC tests/flow.hd:6: flows
    panic: index-out-of-bounds: list index out of bounds
    src/cart.hd:6 corner_shop.cart.Cart.take
    tests/flow.hd:4 corner_shop.flow.first
    tests/flow.hd:7 test \"flows\"
    repro: hd test tests/flow.hd --filter \"flows\"
test result: FAILED. 0 passed; 3 failed; 0 ignored
",
        "{}",
        text(&out.stderr)
    );
}

const RECURSION: &str = "\
fn depth(n: i32) -> i32:
    if n == 0:
        panic(\"bottom\")
    depth(n - 1) + 1

fn forever(n: i32) -> i32:
    forever(n + 1) + 1

fn ping(n: i32) -> i32:
    pong(n + 1) + 1

fn pong(n: i32) -> i32:
    ping(n + 1) + 1

tests:
    it(\"goes five deep\"):
        depth(+5)

    it(\"never stops\"):
        forever(+0)

    it(\"plays ping pong\"):
        ping(+0)
";

/// A single file outside any package names its functions after its stem.
/// A frame repeated right after itself prints once, then its count; so
/// does a cycle of frames; so a recursion that exhausts the stack reports
/// a few lines, not one per call.
#[test]
fn a_single_file_names_its_stem_and_recursion_collapses() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-frames-recursion");
    std::fs::write(root.join("deep.hd"), RECURSION).expect("deep.hd");
    let out = hd(&root, &["test", "deep.hd"]);
    let report = text(&out.stdout);
    // The stack's depth varies with the engine and its stack size.
    let counted: Vec<String> = report
        .lines()
        .map(|l| match l.split_once(" repeated ") {
            Some((what, n))
                if n.split(' ')
                    .next()
                    .and_then(|n| n.parse::<u32>().ok())
                    .is_some_and(|n| n > 100) =>
            {
                format!("{what} repeated N more times")
            }
            _ => l.to_owned(),
        })
        .collect();
    assert!(
        counted
            .iter()
            .any(|l| l.contains("... last 2 frames repeated")),
        "{report}"
    );
    let counted = fold_cycle(counted, &["deep.ping", "deep.pong"]);
    assert_eq!(
        counted,
        [
            "PANIC deep.hd:16: goes five deep",
            "    panic: explicit-panic: bottom",
            "    deep.hd:3 deep.depth",
            "    deep.hd:4 deep.depth",
            "    ... frame repeated 4 more times",
            "    deep.hd:17 test \"goes five deep\"",
            "    repro: hd test deep.hd --filter \"goes five deep\"",
            "PANIC deep.hd:19: never stops",
            "    panic: stack-exhausted: the call stack ran out; check for recursion that never ends",
            "    deep.hd:7 deep.forever",
            "    ... frame repeated N more times",
            "    deep.hd:20 test \"never stops\"",
            "    repro: hd test deep.hd --filter \"never stops\"",
            "PANIC deep.hd:22: plays ping pong",
            "    panic: stack-exhausted: the call stack ran out; check for recursion that never ends",
            "    <cycle>",
            "    deep.hd:23 test \"plays ping pong\"",
            "    repro: hd test deep.hd --filter \"plays ping pong\"",
            "test result: FAILED. 0 passed; 3 failed; 0 ignored",
        ],
        "{}",
        text(&out.stderr)
    );
    assert!(report.len() < 2000, "{report}");
}

/// Which frame of a cycle sits innermost when the stack runs out depends
/// on the engine's stack size, so the frames of a collapsed cycle and its
/// count line fold into one marker line before comparing.
fn fold_cycle(lines: Vec<String>, cycle: &[&str]) -> Vec<String> {
    let in_cycle = |l: &str| {
        let t = l.trim_start();
        t.starts_with("... last ") || cycle.iter().any(|f| t.ends_with(&format!(" {f}")))
    };
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if !in_cycle(&l) {
            out.push(l);
        } else if out.last().is_none_or(|p| p != "    <cycle>") {
            out.push("    <cycle>".to_owned());
        }
    }
    out
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
/// name their functions (`crates/hd_wasm_linear/DESIGN.md` §15.5). Both carry
/// `hd.positions`, the anchors `hd build` resolved.
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
    assert_eq!(
        custom_sections(&debug),
        ["hd.lines", "name", "hd.positions"]
    );
    let out = hd(&root, &["build", "--release"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let release = std::fs::read(root.join("build/release/shop.wasm")).expect("release wasm");
    assert_eq!(
        custom_sections(&release),
        ["hd.lines", "hd.names", "hd.positions"]
    );
    assert!(hd_run::frames::LineTable::parse(&release).is_ok());
}

/// A derived instance's body is the template's, so its instructions map
/// to no line of the deriving module: building a program that derives
/// writes `hd.positions` without reading another source's syntax (#311).
#[test]
fn a_program_that_derives_builds_its_positions() {
    let root = package(
        "hd-frames-derive",
        "use std.hash.hash_of\n\n@derive(Eq, Hash, Debug)\nstruct Point:\n    x: i32\n    y: i32\n    name: string\n\npub fn main() -> void $ Console:\n    a := Point { x: 1, y: 2, name: \"a\" }\n    println(debug(a))\n    println(a == a)\n    println(hash_of(a) == hash_of(a))\n",
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    for (args, file) in [
        (&["build"][..], "build/debug/shop.wasm"),
        (&["build", "--release"][..], "build/release/shop.wasm"),
    ] {
        let out = hd(&root, args);
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        let wasm = std::fs::read(root.join(file)).expect("wasm");
        assert!(hd_run::frames::LineTable::parse(&wasm).is_ok(), "{args:?}");
    }
}

/// The function names of a module's `name` section.
fn function_names(wasm: &[u8]) -> Vec<String> {
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
        let mut p = at;
        if id == 0 {
            let n = leb(wasm, &mut p);
            if &wasm[p..p + n] == b"name" {
                p += n;
                while p < end {
                    let sub = wasm[p];
                    p += 1;
                    let len = leb(wasm, &mut p);
                    let sub_end = p + len;
                    if sub == 1 {
                        let count = leb(wasm, &mut p);
                        for _ in 0..count {
                            leb(wasm, &mut p);
                            let n = leb(wasm, &mut p);
                            out.push(text(&wasm[p..p + n]));
                            p += n;
                        }
                    }
                    p = sub_end;
                }
            }
        }
        at = end;
    }
    out
}

/// Runtime helpers are named by kind (`rt:entry_poll`, `rt:panic#3`), never
/// by a `Debug` dump: no `{`, `[` or `Hash128`, at most 64 characters, and
/// unique within the module.
#[test]
fn runtime_helper_names_are_short_and_readable() {
    let root = package(
        "hd-frames-helper-names",
        "pub fn main() -> void $ Console:\n    println(\"hello, world\")\n    println(7)\n",
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    let out = hd(&root, &["build"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let wasm = std::fs::read(root.join("build/debug/shop.wasm")).expect("wasm");
    let names = function_names(&wasm);
    let rt: Vec<&String> = names.iter().filter(|n| n.starts_with("rt:")).collect();
    assert!(rt.iter().any(|n| n.as_str() == "rt:entry_poll"), "{rt:?}");
    for name in &rt {
        assert!(
            !name.contains(['{', '[']) && !name.contains("Hash128") && name.len() <= 64,
            "{name}"
        );
    }
    let mut sorted = rt.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), rt.len(), "a helper name repeats: {rt:?}");
}

const BUILT: &str = "\
fn pick(items: List[i32], at: usize) -> i32:
    items[at]

pub fn main() -> void $ Console:
    println(\"start\")
    println(pick([1, 2], 5))
";

/// A module `hd build` wrote stands alone (`crates/hd_mono/DESIGN.md`
/// \"Positions\"): run with an empty cache, its panic prints the same
/// frames `hd run` prints.
#[test]
fn a_built_module_prints_its_frames_without_the_cache() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-frames-built");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(root.join("src/main.hd"), BUILT).expect("main");
    let built = hd(&root, &["build"]);
    assert!(built.status.success(), "{}", text(&built.stderr));
    let empty = root.join("empty-cache");
    std::fs::create_dir_all(&empty).expect("empty cache");
    let ran = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", &empty)
        .env_remove("HD_ENGINE")
        .current_dir(&*root)
        .arg("build/debug/shop.wasm")
        .output()
        .expect("run the module");
    assert_eq!(text(&ran.stdout), "start\n");
    assert_eq!(
        text(&ran.stderr),
        "panic: index-out-of-bounds: list index out of bounds\n    \
         src/main.hd:2 shop.main.pick\n    \
         src/main.hd:6 shop.main.main\n"
    );
    assert_eq!(ran.status.code(), Some(3));
}
