//! Symbolized panic frames in `hd test` text reports
//! (`cli.test.report.panic`, `cli.test.report.frame`, `crates/hd_wasm/DESIGN.md`
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

use hd_base::SliceSort as _;
use std::path::Path;
use std::process::Output;

use hd_testkit::Scratch;

const LIB: &str = include_str!("programs/panic_frames/lib.hd");

const OPTIONAL_REASONS: &str = include_str!("programs/panic_frames/optional_reasons.hd");

/// A fresh package `shop` whose `src/lib.hd` is `lib`.
fn package(name: &str, lib: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("hd.toml");
    std::fs::write(root.join("src/lib.hd"), lib).expect("lib");
    root
}

fn hd(root: &Path, args: &[&str]) -> Output {
    hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env("HD_JOBS", "2")
        .env_remove("HD_ENGINE")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run hd")
}

fn text(b: &[u8]) -> String {
    crate::normalize_std_lines(&String::from_utf8_lossy(b))
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
    <std>/testing.hd:LINE std.testing.assert
    src/lib.hd:7 shop.total
    src/lib.hd:21 test \"sums prices\"
    repro: hd test src/lib.hd --filter \"sums prices\"
PANIC src/lib.hd:23: runs out
    panic: explicit-panic: out of stock
    src/lib.hd:11 shop.boom
    src/lib.hd:24 test \"runs out\"
    repro: hd test src/lib.hd --filter \"runs out\"
PANIC src/lib.hd:26: drops past the end
    panic: index-out-of-bounds: list remove index 5 out of bounds for length 2
    <std>/collections/list.hd:LINE std.collections.list.List.remove
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

/// An omitted assertion reason prints no empty reason text. `assert_equal`
/// still prints both values, and both forms still identify the source line.
#[test]
fn assertion_reasons_are_optional_in_failure_reports() {
    let root = package("hd-optional-assertion-reasons", OPTIONAL_REASONS);
    let out = hd(&root, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "\
PANIC src/lib.hd:4: equal without reason
    panic: assertion-failed: actual 1, expected 2
    <std>/testing.hd:LINE std.testing.check_equal
    src/lib.hd:5 test \"equal without reason\"
    repro: hd test src/lib.hd --filter \"equal without reason\"
PANIC src/lib.hd:7: condition without reason
    panic: assertion-failed
    <std>/testing.hd:LINE std.testing.assert
    src/lib.hd:8 test \"condition without reason\"
    repro: hd test src/lib.hd --filter \"condition without reason\"
PANIC src/lib.hd:10: equal with reason
    panic: assertion-failed: three is not four: actual 3, expected 4
    <std>/testing.hd:LINE std.testing.check_equal
    src/lib.hd:11 test \"equal with reason\"
    repro: hd test src/lib.hd --filter \"equal with reason\"
test result: FAILED. 0 passed; 3 failed; 0 ignored
",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(1));
    // The JSON report of an `assert_equal` failure: its category and
    // message, which `check_equal` writes.
    let out = hd(
        &root,
        &["test", "--filter", "equal with reason", "--format", "json"],
    );
    assert_eq!(
        text(&out.stdout).lines().next(),
        Some(
            "{\"kind\":\"test\",\"name\":\"equal with reason\",\"outcome\":\"failed\",\"message\":\"panic: assertion-failed: three is not four: actual 3, expected 4\"}"
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
    let edited = LIB.replace(
        "fn boom()",
        include_str!("programs/panic_frames/a_comment_edit_moves_the_reported_lines.hd"),
    );
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
    std::fs::write(
        root.join("hd.toml"),
        "[package]\nname = \"corner-shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("hd.toml");
    std::fs::write(
        root.join("src/cart.hd"),
        include_str!("programs/panic_frames/a_method_frame_names_its_type_and_module.hd"),
    )
    .expect("cart");
    std::fs::write(
        root.join("tests/flow.hd"),
        include_str!("programs/panic_frames/a_method_frame_names_its_type_and_module_2.hd"),
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
    <std>/format.hd:LINE std.format.Display.write
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

const RECURSION: &str = include_str!("programs/panic_frames/recursion.hd");

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

fn leb(bytes: &[u8], at: &mut usize) -> Result<usize, &'static str> {
    hd_base::leb::get(bytes, at, 32)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or("invalid unsigned integer in the compiled fixture")
}

/// The custom sections of a module, by name, in order.
fn custom_sections(wasm: &[u8]) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    let mut at = 8;
    while at < wasm.len() {
        let id = wasm[at];
        at += 1;
        let size = leb(wasm, &mut at)?;
        let end = at + size;
        if id == 0 {
            let mut name_at = at;
            let n = leb(wasm, &mut name_at)?;
            out.push(text(&wasm[name_at..name_at + n]));
        }
        at = end;
    }
    Ok(out)
}

/// A debug build keeps the standard `name` section beside `hd.lines`; a
/// release build drops both name sections after copying the names its traces
/// need into `hd.positions`.
#[test]
fn release_builds_keep_names_only_in_positions() -> Result<(), &'static str> {
    let root = package(
        "hd-frames-release",
        include_str!("programs/panic_frames/release_builds_keep_names_only_in_positions.hd"),
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    let out = hd(&root, &["build"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let debug = std::fs::read(root.join("build/debug/shop.wasm")).expect("debug wasm");
    assert_eq!(
        custom_sections(&debug)?,
        ["hd.build", "hd.lines", "name", "hd.positions"]
    );
    let out = hd(&root, &["build", "--release"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let release = std::fs::read(root.join("build/release/shop.wasm")).expect("release wasm");
    assert_eq!(
        custom_sections(&release)?,
        ["hd.build", "hd.lines", "hd.positions"]
    );
    assert!(hd_run::frames::LineTable::parse(&release).is_ok());
    Ok(())
}

/// A release build copies `inner` into `outer`, then `outer` into
/// `main` (`docs/compiler/tir-v2.md`, "The inliner"), so `main` calls
/// neither; the panic in the copy inside the copy still lists `inner`,
/// `outer` and `main`, each at its own line.
#[test]
fn a_copy_inside_a_copy_reports_each_frame() {
    let root = package(
        "hd-frames-nested-copy",
        include_str!("programs/panic_frames/a_copy_inside_a_copy_reports_each_frame.hd"),
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    let out = hd(&root, &["build", "--release"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let wasm = std::fs::read(root.join("build/release/shop.wasm")).expect("release wasm");
    let named = |f: &[u8]| wasm.windows(f.len()).any(|w| w == f);
    assert!(!named(b"/main#") && !named(b"/outer#") && !named(b"/inner#"));
    assert!(named(b"shop.main.main") && named(b"shop.main.outer") && named(b"shop.main.inner"));
    let out = hd(&root, &["build/release/shop.wasm"]);
    assert_eq!(
        text(&out.stderr),
        "\
panic: integer-division-by-zero: division by zero
    src/main.hd:2 shop.main.inner
    src/main.hd:5 shop.main.outer
    src/main.hd:9 shop.main.main
"
    );
}

/// A derived instance's body is the template's, so its instructions map
/// to no line of the deriving module: building a program that derives
/// writes `hd.positions` without reading another source's syntax (#311).
#[test]
fn a_program_that_derives_builds_its_positions() {
    let root = package(
        "hd-frames-derive",
        include_str!("programs/panic_frames/a_program_that_derives_builds_its_positions.hd"),
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
fn function_names(wasm: &[u8]) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    let mut at = 8;
    while at < wasm.len() {
        let id = wasm[at];
        at += 1;
        let size = leb(wasm, &mut at)?;
        let end = at + size;
        let mut p = at;
        if id == 0 {
            let n = leb(wasm, &mut p)?;
            if &wasm[p..p + n] == b"name" {
                p += n;
                while p < end {
                    let sub = wasm[p];
                    p += 1;
                    let len = leb(wasm, &mut p)?;
                    let sub_end = p + len;
                    if sub == 1 {
                        let count = leb(wasm, &mut p)?;
                        for _ in 0..count {
                            leb(wasm, &mut p)?;
                            let n = leb(wasm, &mut p)?;
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
    Ok(out)
}

/// Runtime helpers are named by kind (`rt:entry_poll`, `rt:panic#3`), never
/// by a `Debug` dump: no `{`, `[` or `Hash128`, at most 64 characters, and
/// unique within the module.
#[test]
fn runtime_helper_names_are_short_and_readable() -> Result<(), &'static str> {
    let root = package(
        "hd-frames-helper-names",
        include_str!("programs/panic_frames/runtime_helper_names_are_short_and_readable.hd"),
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    let out = hd(&root, &["build"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let wasm = std::fs::read(root.join("build/debug/shop.wasm")).expect("wasm");
    let names = function_names(&wasm)?;
    let rt: Vec<&String> = names.iter().filter(|n| n.starts_with("rt:")).collect();
    assert!(rt.iter().any(|n| n.as_str() == "rt:entry_poll"), "{rt:?}");
    for name in &rt {
        assert!(
            !name.contains(['{', '[']) && !name.contains("Hash128") && name.len() <= 64,
            "{name}"
        );
    }
    let mut sorted = rt.clone();
    sorted.sort_stable();
    sorted.dedup();
    assert_eq!(sorted.len(), rt.len(), "a helper name repeats: {rt:?}");
    Ok(())
}

const BUILT: &str = include_str!("programs/panic_frames/built.hd");

/// A module `hd build` wrote stands alone (`crates/hd_mono/DESIGN.md`
/// \"Positions\"): run with an empty cache, its panic prints the same
/// frames `hd run` prints.
#[test]
fn a_built_module_prints_its_frames_without_the_cache() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-frames-built");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("hd.toml");
    std::fs::write(root.join("src/main.hd"), BUILT).expect("main");
    let built = hd(&root, &["build"]);
    assert!(built.status.success(), "{}", text(&built.stderr));
    let empty = root.join("empty-cache");
    std::fs::create_dir_all(&empty).expect("empty cache");
    let ran = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
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

/// Sets every file under `dir` an hour back, so the listing stamps it and
/// a run may store what it builds (#1510).
fn settle(dir: &Path) {
    let past = std::time::SystemTime::now() - std::time::Duration::from_hours(1);
    for entry in std::fs::read_dir(dir).expect("dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            settle(&path);
        } else {
            std::fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|f| f.set_modified(past))
                .expect("stamp");
        }
    }
}

/// Cost Model (#1510): a warm `hd test` or `hd run` with nothing changed
/// takes what a run of the same sources stored, and its panics print the
/// same frames as the run that built them: they are resolved again only
/// when a panic asks.
#[test]
fn a_stored_build_prints_the_frames_of_its_panics() {
    let root = package("hd-frames-stored", LIB);
    std::fs::write(root.join("src/main.hd"), BUILT).expect("main");
    settle(&root);
    let runs: Vec<(String, String)> = (0..3)
        .map(|_| {
            let tested = hd(&root, &["test", "--filter", "runs out"]);
            let ran = hd(&root, &["run"]);
            (text(&tested.stdout), text(&ran.stderr))
        })
        .collect();
    assert!(
        runs[0]
            .0
            .contains("    src/lib.hd:11 shop.boom\n    src/lib.hd:24 test \"runs out\"\n"),
        "{}",
        runs[0].0
    );
    assert_eq!(
        runs[0].1,
        "panic: index-out-of-bounds: list index out of bounds\n    \
         src/main.hd:2 shop.main.pick\n    \
         src/main.hd:6 shop.main.main\n"
    );
    assert_eq!(runs[1], runs[0]);
    assert_eq!(runs[2], runs[0]);
}

#[cfg(feature = "wasmtime")]
#[test]
fn a_native_release_reports_an_inlined_trap() {
    let root = package(
        "native-release-frames",
        include_str!("programs/panic_frames/a_copy_inside_a_copy_reports_each_frame.hd"),
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    let out = hd(&root, &["build", "--engine", "wasmtime", "--release"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = hd(&root, &["--engine", "wasmtime", "build/release/shop.cwasm"]);
    assert_eq!(out.status.code(), Some(3));
    let error = text(&out.stderr);
    assert!(error.contains("integer-division-by-zero"), "{error}");
    // #2385 accepts Wasmtime's omission of native-inlined frames. The
    // trap category/status still survives inlining and serialization.
}

#[cfg(feature = "wasmtime")]
#[test]
fn a_native_release_keeps_noninlined_frames_without_sources() {
    let root = package(
        "native-release-recursive-frames",
        include_str!("programs/panic_frames/native_recursive.hd"),
    );
    std::fs::rename(root.join("src/lib.hd"), root.join("src/main.hd")).expect("main");
    let out = hd(&root, &["build", "--engine", "wasmtime", "--release"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    std::fs::rename(root.join("src"), root.join("hidden-sources")).expect("hide sources");
    let out = hd(&root, &["--engine", "wasmtime", "build/release/shop.cwasm"]);
    assert_eq!(out.status.code(), Some(3));
    let error = text(&out.stderr);
    assert!(error.contains("integer-division-by-zero"), "{error}");
    assert!(error.contains("src/main.hd:4 shop.main.depth"), "{error}");
}
