//! M4a's exit: every `lib/std` body checks through `InferTable`, the
//! solver and `TirBuilder` to verified TIR with no diagnostic when std is
//! the root package; a few bodies' TIR is pinned by golden files; and the
//! checker's own errors (rows, exhaustiveness, mismatches) are reported in
//! user programs.

use std::path::{Path, PathBuf};

use hd_base::Stage;
use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn walk(root: &Path, dir: &Path, out: &mut MemorySources) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.extension().is_some_and(|x| x == "hd") {
            let rel = p
                .strip_prefix(root)
                .expect("root")
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(&rel, &std::fs::read_to_string(&p).expect("read"));
        }
    }
}

fn std_sources() -> MemorySources {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
    let mut s = MemorySources::default();
    walk(&root, &root, &mut s);
    s
}

fn analyze_std(render: &[&str]) -> Output {
    let src = std_sources();
    let store = MemoryStore::default();
    let host = Host {
        render_tir: render,
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "std", &Goal::Analyze)
}

fn program(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// Every module of `lib/std`, plus the compiler-supplied `std.rt`, and the
/// trait default bodies among the bodies.
#[test]
fn every_std_body_checks_with_no_diagnostic() {
    let out = analyze_std(&[]);
    let body = out.report.tally(Stage::Body);
    assert_eq!(
        (body.ok, body.not_implemented, body.blocked),
        (38, 0, 0),
        "{}\n{:?}",
        out.report.render(),
        out.report.body_failures
    );
    assert_eq!(out.report.tally(Stage::ModuleFinish).ok, 38);
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    assert!(out.report.body_ok > 1000, "{}", out.report.body_ok);
    assert_eq!(out.diags.len(), 0, "{}", out.render());
}

/// The golden TIR of a few std bodies: `println` (rows, `$.use`, a cold
/// suspending call, `block_on`, interpolation, `match` on a `Result`),
/// `digit_text` (integer patterns), `Option.map` (an `Option` match with a
/// closure call) and `Iterator.take` (a closure capturing and assigning).
/// `HD_UPDATE_GOLDEN=1` rewrites the files.
#[test]
fn std_bodies_match_golden_tir() {
    let bodies = [
        ("std/console/println", "println"),
        ("std/format/digit_text", "digit_text"),
        ("std/option/impl [ T ] T ?.map", "option_map"),
        ("std/iter/impl [ T ] Iterator [ T ].take", "iterator_take"),
    ];
    let paths: Vec<&str> = bodies.iter().map(|b| b.0).collect();
    let out = analyze_std(&paths);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tir");
    let update = std::env::var_os("HD_UPDATE_GOLDEN").is_some();
    for (path, file) in bodies {
        let got = out
            .tir_text
            .get(path)
            .unwrap_or_else(|| panic!("no body {path}; have {:?}", out.tir_text.keys()));
        let golden = dir.join(format!("{file}.tir"));
        if update {
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(&golden, got).expect("write golden");
            continue;
        }
        let want = std::fs::read_to_string(&golden).unwrap_or_default();
        assert_eq!(got, &want, "{path}: TIR differs from {}", golden.display());
    }
}

/// `use a.b as c` and `use a.{b as c}` (grammar.use) reach the checker:
/// the renamed module, type and trait resolve in bodies.
#[test]
fn renamed_uses_resolve_in_bodies() {
    let mut src = MemorySources::default();
    src.insert(
        "main.hd",
        "use std.cmp as order\nuse std.cmp.Ordering as Order\nuse std.format.{Display as Show}\n\nfn pick(o: Order) -> i32:\n    match o:\n        .Less => 1\n        .Equal => 2\n        .Greater => 3\n\nfn other(o: order.Ordering) -> i32:\n    pick(o)\n\nfn show[T < Show](x: T) -> string:\n    x.to_string()\n\nfn main() -> void $ Console:\n    println(other(.Less))\n",
    );
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    assert_eq!(out.diags.len(), 0, "{}", out.render());
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
}

/// Module initialization (spec/lang/10-modules.md): top-level bindings are
/// visible to function bodies; a statement whose transitive read set
/// holds a later binding is `top-level-read-before-initialization`; a
/// non-entry module's top level is requirement-free.
#[test]
fn top_level_statements_are_checked_in_order() {
    let ok = program(
        "limit := +10\n\nfn below(n: i32) -> bool:\n    n < limit\n\nfn main() -> void $ Console:\n    println(below(3))\n",
    );
    assert!(
        !codes(&ok).iter().any(|c| matches!(
            c,
            Code::TopLevelReadBeforeInitialization | Code::UnknownName | Code::TypeMismatch
        )),
        "{}",
        ok.render()
    );
    assert_eq!(ok.report.body_failed, 0, "{:?}", ok.report.body_failures);
    let late = program(
        "first := apply(first_name)\nlet names: List[string] = [\"Ada\"]\n\nfn apply(callback: fn() -> string) -> string:\n    callback()\n\nfn first_name() -> string:\n    names[0]\n\nfn main() -> void $ Console:\n    println(first)\n",
    );
    assert!(
        codes(&late).contains(&Code::TopLevelReadBeforeInitialization),
        "{}",
        late.render()
    );
    let mut src = MemorySources::default();
    src.insert("lib.hd", "println(1)\n\npub fn one() -> i32:\n    1\n");
    src.insert(
        "main.hd",
        "use pkg.lib.{one}\n\nfn main() -> void $ Console:\n    println(one())\n",
    );
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(
        codes(&out).contains(&Code::MissingRequirement),
        "{}",
        out.render()
    );
}

/// `$.with` covers its keys in its block, and each provider must
/// implement its key (`req.with.type`).
#[test]
fn with_blocks_provide_requirements() {
    let out = program(
        "use std.console.BufferConsole\n\nfn show() -> void:\n    let mut console = BufferConsole.new()\n    $.with(Console = console):\n        println(1)\n\nfn main() -> void $ Console:\n    show()\n",
    );
    assert!(
        codes(&out).iter().all(|c| *c == Code::Unsupported),
        "{}",
        out.render()
    );
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    let out = program(
        "fn show() -> void:\n    $.with(Console = 5):\n        println(1)\n\nfn main() -> void $ Console:\n    show()\n",
    );
    assert!(
        codes(&out).contains(&Code::UnsatisfiedTraitBound),
        "{}",
        out.render()
    );
}

/// Pipes and comprehensions check (an Emit stub may stop the build).
#[test]
fn pipes_and_comprehensions_check() {
    let out = program(
        "fn add(a: i32, b: i32) -> i32:\n    a + b\n\nfn main() -> void $ Console:\n    xs := [1, 2, 3, 4]\n    y := 3 |> add(_, 1)\n    evens := [for x in xs if x % 2 == 0 => x * 10]\n    m := {for x in xs => \"$x\": x + y}\n    println(evens.len() + m.len())\n",
    );
    assert!(
        codes(&out).iter().all(|c| *c == Code::Unsupported),
        "{}",
        out.render()
    );
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    let out = program(
        "fn add(a: i32, b: i32) -> i32:\n    a + b\n\nfn main() -> void $ Console:\n    println(add(_, 1))\n",
    );
    assert!(
        codes(&out).contains(&Code::PlaceholderOutsidePipe),
        "{}",
        out.render()
    );
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

#[test]
fn a_missing_requirement_is_an_error() {
    let out = program("fn main():\n    println(42)\n");
    assert!(
        codes(&out).contains(&Code::MissingRequirement),
        "{}",
        out.render()
    );
    let ok = program("fn main() -> void $ Console:\n    println(42)\n");
    assert!(
        !codes(&ok).contains(&Code::MissingRequirement),
        "{}",
        ok.render()
    );
    // A callee's own row reaches its callers.
    let out =
        program("fn show(x: i32) -> void $ Console:\n    println(x)\n\nfn main():\n    show(1)\n");
    assert!(
        codes(&out).contains(&Code::MissingRequirement),
        "{}",
        out.render()
    );
}

#[test]
fn a_match_must_cover_every_variant() {
    let src = "enum Color:\n    Red\n    Green\n    Blue\n\nfn code(c: Color) -> i32:\n    match c:\n        .Red => 1\n        .Green => 2\n\nfn main() -> void $ Console:\n    println(code(.Red))\n";
    let out = program(src);
    assert!(
        codes(&out).contains(&Code::NonexhaustiveMatch),
        "{}",
        out.render()
    );
    let full = src.replace(
        "        .Green => 2\n",
        "        .Green => 2\n        .Blue => 3\n",
    );
    let out = program(&full);
    assert!(
        !codes(&out).contains(&Code::NonexhaustiveMatch),
        "{}",
        out.render()
    );
    // Nested: `.Some(.Red)` alone leaves `.None` and the other colors.
    let nested = "enum Color:\n    Red\n    Green\n\nfn f(c: Color?) -> i32:\n    match c:\n        .Some(.Red) => 1\n        .None => 0\n\nfn main() -> void $ Console:\n    println(f(.None))\n";
    let out = program(nested);
    assert!(
        codes(&out).contains(&Code::NonexhaustiveMatch),
        "{}",
        out.render()
    );
}

#[test]
fn checker_errors_are_reported() {
    let cases: &[(&str, Code)] = &[
        (
            "fn main() -> void $ Console:\n    let x: i32 = \"a\"\n    println(x)\n",
            Code::TypeMismatch,
        ),
        (
            "fn main() -> void $ Console:\n    x := 1\n    println(x.nope())\n",
            Code::UnknownMethod,
        ),
        (
            "fn f(o: i32?) -> i32:\n    let .Some(v) = o else:\n        println(0)\n    v\n\nfn main() -> void $ Console:\n    println(f(.None))\n",
            Code::LetElseFallsThrough,
        ),
        (
            "fn f(x: i32) -> i32:\n    y := x?\n    y\n\nfn main() -> void $ Console:\n    println(f(1))\n",
            Code::InvalidResultPropagation,
        ),
        (
            "data P:\n    x: i32\n\nfn main() -> void $ Console:\n    p := P { x: 1 }\n    println(p.y)\n",
            Code::UnknownDataField,
        ),
        (
            "fn get!() -> i32:\n    1\n\nfn main() -> void $ Console:\n    println(get!())\n",
            Code::BangCallOutsideSuspension,
        ),
        (
            "fn get() -> i32:\n    1\n\nfn main!() -> void $ Console:\n    println(get!())\n",
            Code::NotSuspending,
        ),
        (
            "fn f(o: i32?) -> i32:\n    match o:\n        _ => 0\n        .Some(x) => x\n\nfn main() -> void $ Console:\n    println(f(.None))\n",
            Code::UnreachableMatchArm,
        ),
        (
            "fn main() -> void $ Console:\n    x := .Red\n    println(1)\n",
            Code::MissingContextualEnumType,
        ),
        (
            "fn f(a: i32, b: i32) -> i32:\n    a + b\n\nfn main() -> void $ Console:\n    println(f(1, c = 2))\n",
            Code::UnknownNamedArgument,
        ),
        (
            "fn f(a: i32, b: i32) -> i32:\n    a + b\n\nfn main() -> void $ Console:\n    println(f(1, a = 2))\n",
            Code::DuplicateArgument,
        ),
        (
            "fn main() -> void $ Console:\n    let x: u8 = 300\n    println(x)\n",
            Code::IntegerLiteralRange,
        ),
        (
            "fn main() -> void $ Console:\n    while true:\n        break 1\n",
            Code::BreakValueContext,
        ),
        (
            "fn f() -> i32:\n    defer:\n        return 1\n    2\n\nfn main() -> void $ Console:\n    println(f())\n",
            Code::DeferControlFlow,
        ),
        (
            "data P:\n    x: i32\n\nfn main() -> void $ Console:\n    q := P\n    println(1)\n",
            Code::TypeUsedAsValue,
        ),
        (
            "fn f(t: (i32, i32)) -> i32:\n    let (a, a) = t\n    a\n\nfn main() -> void $ Console:\n    println(f((1, 2)))\n",
            Code::DuplicateBinding,
        ),
        (
            "fn main() -> void $ Console:\n    xs := []\n    println(1)\n",
            Code::CannotInferType,
        ),
        (
            "fn main() -> void $ Console:\n    let mut n = 0\n    println(n)\n",
            Code::MutOnPrimitive,
        ),
        (
            "fn main() -> void $ Console:\n    a := (1, 2)\n    println(a is a)\n",
            Code::IdentityRequiresReferences,
        ),
        (
            "fn main() -> void $ Console:\n    let n: mut i32 = 0\n    println(n)\n",
            Code::MutOnPrimitive,
        ),
        (
            "fn main() -> void $ Console:\n    let mut pair = (1, 2)\n    println(pair._0)\n",
            Code::MutOnTuple,
        ),
    ];
    for (src, code) in cases {
        let out = program(src);
        assert!(codes(&out).contains(code), "{src}\n{}", out.render());
    }
}
