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

/// `use_a` calls `a` through a trait value and `use_b` calls `b`, a later
/// method of the same trait. Once a vtable keeps only the slots some call
/// through a trait value names (#2415, #2420), `b`'s slot index depends
/// on whether any call names `a`: an edit that takes away the only dyn
/// call of `a` must emit `use_b` again, and the warm module is the cold
/// build's of the edited source.
#[test]
fn removing_the_only_dyn_call_of_a_method_emits_the_dyn_calls_of_its_trait() {
    let files = |use_a: &str| {
        vec![(
            "main.hd".to_owned(),
            format!(
                "trait Shape:\n    fn a(&self) -> i64\n    fn b(&self) -> i64\n\n\
                 struct Sq:\n    k: i64\n\n\
                 impl Shape for Sq:\n    fn a(&self) -> i64:\n        self.k\n    fn b(&self) -> i64:\n        self.k + 1\n\n\
                 fn use_a(s: &dyn Shape) -> i64:\n    {use_a}\n\n\
                 fn use_b(s: &dyn Shape) -> i64:\n    s.b()\n\n\
                 pub fn main() -> void $ Console:\n    s := Sq {{ k: 2 }}\n    println(\"${{use_a(&s)}} ${{use_b(&s)}}\")\n"
            ),
        )]
    };
    let store = MemoryStore::default();
    build(&store, &files("s.a()"));
    let edited = files("1");
    let warm = build(&store, &edited);
    let emitted = own_emitted(&warm);
    assert!(emitted.contains(&"own/main/use_a"), "{emitted:?}");
    assert!(emitted.contains(&"own/main/use_b"), "{emitted:?}");
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}

/// `widen` makes no dyn call: it reads `Base`'s table from a `dyn Child`
/// table at the offset after `Child`'s own kept slots. Taking away the
/// only call of `extra` drops `Child`'s one slot, so that offset moves
/// from 4 to 0, and `widen` must be emitted again (its `callee_reps`
/// slot facts, `Collected::slot_facts`).
#[test]
fn dropping_a_child_traits_only_called_slot_emits_its_widenings_again() {
    let files = |use_extra: &str| {
        vec![(
            "main.hd".to_owned(),
            format!(
                "trait Base:\n    fn read(&self) -> i64\n\n\
                 trait Child < Base:\n    fn extra(&self) -> i64\n\n\
                 struct Sq:\n    k: i64\n\n\
                 impl Base for Sq:\n    fn read(&self) -> i64:\n        self.k\n\n\
                 impl Child for Sq:\n    fn extra(&self) -> i64:\n        self.k + 1\n\n\
                 fn widen(c: &dyn Child) -> &dyn Base:\n    c\n\n\
                 fn use_extra(c: &dyn Child) -> i64:\n    {use_extra}\n\n\
                 pub fn main() -> void $ Console:\n    s := Sq {{ k: 2 }}\n    \
                 println(\"${{widen(&s).read()}} ${{use_extra(&s)}}\")\n"
            ),
        )]
    };
    let store = MemoryStore::default();
    build(&store, &files("c.extra()"));
    let edited = files("1");
    let warm = build(&store, &edited);
    let emitted = own_emitted(&warm);
    assert!(emitted.contains(&"own/main/widen"), "{emitted:?}");
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}
