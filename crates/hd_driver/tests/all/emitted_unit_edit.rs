//! A dev link takes each instance's emitted unit from the store while
//! the unit's key and the instances its emission read are unchanged
//! (#2423): a rebuild emits only the instances an edit reaches, and
//! merges the rest. Each warm module is a cold build's of the same
//! source, byte for byte. A debug compiler emits every reused unit again
//! and compares it (`UnitStore::verify`), so these runs check the keys
//! too.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `n` functions `step0` .. in `fs`, each calling `util.bump`, whose body is
/// `bump`; `main` calls each and `note()`, whose body is `note`.
fn program(n: usize, note: &str, bump: &str) -> Vec<(String, String)> {
    let util = format!("pub fn bump(x: i64) -> i64:\n    {bump}\n");
    let mut fs = String::from("use pkg.util.bump\n");
    for i in 0..n {
        let _ = write!(
            fs,
            "\npub fn step{i}(x: i64) -> List[i64]:\n    [bump(x), {i}]\n"
        );
    }
    let names: Vec<String> = (0..n).map(|i| format!("step{i}")).collect();
    let mut main = format!("use pkg.fs.{{{}}}\n", names.join(", "));
    let _ = write!(
        main,
        "\npub fn note() -> string:\n    {note}\n\npub fn main() -> void $ Console:\n    let s: usize = 0\n"
    );
    for i in 0..n {
        let _ = writeln!(main, "    s = s + step{i}({i}).len()");
    }
    main += "    println(\"$s ${note()}\")\n";
    vec![
        ("util.hd".into(), util),
        ("fs.hd".into(), fs),
        ("main.hd".into(), main),
    ]
}

fn build(store: &MemoryStore, files: &[(String, String)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "own", &Packages::default(), &goal);
    assert!(out.wasm.is_some(), "{}", out.render());
    out
}

/// The units a build emitted, of the program's own items.
fn own_emitted(out: &Output) -> Vec<&str> {
    let all = &out.counters.units_emitted;
    all.iter()
        .map(String::as_str)
        .filter(|p| p.starts_with("own/"))
        .collect()
}

/// A cold build emits every unit; an edit to `note`'s body emits `note`
/// and `main`, which reads it, and reuses the rest, at any `n`; an edit
/// to `bump`'s body emits `bump` and each of its `n` callers. Each warm
/// module is the cold build's of its source, and two cold builds agree.
fn edits_emit_only_their_readers(n: usize) {
    let store = MemoryStore::default();
    let base = program(n, "\"a\"", "x + 1");
    let cold = build(&store, &base);
    assert_eq!(cold.counters.units_reused, 0);
    assert!(own_emitted(&cold).len() >= n + 3, "n = {n}");
    assert_eq!(cold.wasm, build(&MemoryStore::default(), &base).wasm);

    let noted = program(n, "\"b\"", "x + 1");
    let warm = build(&store, &noted);
    assert_eq!(
        own_emitted(&warm),
        ["own/main/main", "own/main/note"],
        "n = {n}"
    );
    assert_eq!(warm.counters.units_emitted.len(), 2, "n = {n}");
    assert_eq!(
        warm.counters.units_reused,
        cold.counters.units_emitted.len() - 2
    );
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &noted).wasm);

    let bumped = program(n, "\"b\"", "x + 2");
    let warm = build(&store, &bumped);
    let emitted = own_emitted(&warm);
    assert_eq!(emitted.len(), n + 1, "n = {n}: {emitted:?}");
    assert!(emitted.contains(&"own/util/bump"), "{emitted:?}");
    assert!(
        emitted
            .iter()
            .all(|p| *p == "own/util/bump" || p.starts_with("own/fs/step")),
        "{emitted:?}"
    );
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &bumped).wasm);
}

#[test]
fn unit_edits_emit_only_their_readers_at_5_functions() {
    edits_emit_only_their_readers(5);
}

#[test]
fn unit_edits_emit_only_their_readers_at_50_functions() {
    edits_emit_only_their_readers(50);
}

/// The merge is linear in the program: each 100 more functions add the
/// same merge work (each one's unit and `main`'s call of it), where a
/// term quadratic in the program would make the second step larger; and
/// a reused unit merges exactly as an emitted one does.
#[test]
fn the_merge_of_units_is_linear_in_the_program() {
    let merge = |n: usize| {
        let store = MemoryStore::default();
        let ((), cold) = hd_wasm::work::count_merge(|| {
            build(&store, &program(n, "\"a\"", "x + 1"));
        });
        let ((), warm) = hd_wasm::work::count_merge(|| {
            build(&store, &program(n, "\"b\"", "x + 1"));
        });
        assert_eq!(cold, warm, "n = {n}");
        cold
    };
    let [a, b, c] = [20, 120, 220].map(merge);
    assert!(a < b, "{a} {b} {c}");
    assert_eq!(b - a, c - b, "{a} {b} {c}");
}
