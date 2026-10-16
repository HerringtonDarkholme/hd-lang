//! `match` compiles to a decision tree (`crates/hd_check/DESIGN.md` §7.2):
//! each test is emitted once per path, so the TIR of a nested-enum match
//! grows linearly with its depth; and the tree keeps the source order of
//! arms and guards (spec/lang/06-control-flow.md, `flow.match.guard.*`).
//! Programs build through the driver and run on V8 (`host/run.mjs`), as
//! the conformance runner does.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;

/// Enums `E0` to `E{depth}`, each `A` wrapping the one below, and a
/// staircase match on `E{depth}`: arm `k` matches `k` levels of `.A`
/// around `1` (the deepest) or `_`.
fn staircase(depth: usize) -> String {
    let mut s = String::from("enum E0:\n    A(i32)\n    B\n\n");
    for d in 1..=depth {
        let _ = write!(s, "enum E{d}:\n    A(E{})\n    B\n\n", d - 1);
    }
    let _ = writeln!(s, "fn f(x: E{depth}) -> i32:\n    match x:");
    let wrap = |inner: &str, levels: usize| {
        let mut p = inner.to_owned();
        for _ in 0..levels {
            p = format!(".A({p})");
        }
        p
    };
    let _ = writeln!(s, "        {} => {}", wrap("1", depth + 1), depth + 1);
    for k in (1..=depth).rev() {
        let _ = writeln!(s, "        {} => {k}", wrap("_", k));
    }
    s.push_str("        _ => 0\n\npub fn main() -> void:\n    pass\n");
    s
}

/// The rendered TIR of `fn f` in a program, from a cold store so the
/// body is checked here.
fn tir_of_f(main: &str) -> String {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &["app/main/f"],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    assert!(out.diags.is_empty(), "{}", out.render());
    out.tir_text
        .get("app/main/f")
        .unwrap_or_else(|| panic!("no body; have {:?}", out.tir_text.keys()))
        .clone()
}

#[test]
fn nested_enum_match_tir_grows_linearly() {
    let sizes: Vec<usize> = [4, 8, 16]
        .iter()
        .map(|&d| tir_of_f(&staircase(d)).lines().count())
        .collect();
    // Doubling the depth at most doubles the TIR, plus a constant.
    assert!(
        sizes[1] <= 2 * sizes[0] + 8 && sizes[2] <= 2 * sizes[1] + 8,
        "TIR lines at depth 4, 8, 16: {sizes:?}"
    );
}

fn built(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
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

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("match-decision-tree-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

#[test]
fn a_staircase_match_picks_the_first_matching_arm() {
    let mut main = staircase(3).replace("pub fn main() -> void:\n    pass\n", "");
    main.push_str(
        "pub fn main() -> void $ Console:
    println(f(.A(.A(.A(.A(1))))))
    println(f(.A(.A(.A(.A(2))))))
    println(f(.A(.A(.A(.B)))))
    println(f(.A(.A(.B))))
    println(f(.A(.B)))
    println(f(.B))
",
    );
    assert_eq!(output_of("staircase", &main), "4\n3\n3\n2\n1\n0\n");
}

#[test]
fn guards_run_in_arm_order_only_after_their_pattern_matches() {
    let main = "enum Shape:
    Dot
    Line(len: i32)
    Pair(a: i32, b: i32)

fn seen(tag: string, v: bool) -> bool $ Console:
    println(\"guard $tag\")
    v

fn pick(s: Shape, t: (i32, string)) -> string $ Console:
    match (s, t):
        (.Line(1), (0, \"x\")) => \"line one x\"
        (.Pair(a, 2), (n, _)) if seen(\"pair\", a == n) => \"pair a == n\"
        (.Line(len), (0, _)) if seen(\"line\", len > 5) => \"long line at zero\"
        (_, (0, \"x\")) if seen(\"x\", true) => \"zero x\"
        (.Pair(_, b), _) if seen(\"b\", b > 0) => \"positive b\"
        (.Line(_), (_, \"y\")) => \"line y\"
        _ => \"other\"

pub fn main() -> void $ Console:
    println(pick(.Line(1), (0, \"x\")))
    println(pick(.Line(9), (0, \"x\")))
    println(pick(.Line(2), (0, \"x\")))
    println(pick(.Pair(3, 2), (3, \"z\")))
    println(pick(.Pair(3, 2), (4, \"z\")))
    println(pick(.Pair(0, 0), (0, \"x\")))
    println(pick(.Line(2), (1, \"y\")))
    println(pick(.Dot, (1, \"y\")))
";
    assert_eq!(
        output_of("guards", main),
        "line one x\n\
         guard line\nlong line at zero\n\
         guard line\nguard x\nzero x\n\
         guard pair\npair a == n\n\
         guard pair\nguard b\npositive b\n\
         guard x\nzero x\n\
         line y\n\
         other\n"
    );
}
