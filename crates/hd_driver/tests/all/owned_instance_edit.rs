//! A dev build takes a generic instance's owned form from the store while
//! its instance key, its generic body, the program's declarations and
//! every body its folded form read are unchanged (#2106): a rebuild runs
//! the ownership passes on the instances that read an edited body, and on
//! no other.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// `n` structs in `types`, each passed to the generic `keep` and `pair`
/// of `gen`, whose bodies are `keep` and `pair`; `main` adds `note()`,
/// whose body is `note`. `keep` calls `bump` of `util`, which the folds
/// copy into `keep`'s folded form.
fn program(n: usize, keep: &str, pair: &str, note: &str, bump: &str) -> Vec<(String, String)> {
    let mut types = String::new();
    for i in 0..n {
        let _ = writeln!(types, "pub struct S{i}:\n    pub v: i64\n");
    }
    let util = format!("pub fn bump(x: usize) -> usize:\n    {bump}\n");
    let generic = format!(
        "use pkg.util.bump\n\npub fn keep[T](x: T) -> List[T]:\n    {keep}\n\npub fn pair[T](x: T, y: T) -> List[T]:\n    {pair}\n"
    );
    let mut main = String::from("use pkg.gen.keep\nuse pkg.gen.pair\n");
    for i in 0..n {
        let _ = writeln!(main, "use pkg.types.S{i}");
    }
    let _ = write!(
        main,
        "\npub fn note() -> i64:\n    {note}\n\npub fn main() -> void $ Console:\n    let s: usize = 0\n"
    );
    for i in 0..n {
        let _ = writeln!(
            main,
            "    s = s + keep(S{i} {{ v: {i} }}).len() + pair(S{i} {{ v: 1 }}, S{i} {{ v: 2 }}).len()"
        );
    }
    main += "    println(\"$s ${note()}\")\n    println(\"${(s, note()) == (1, 2)}\")\n";
    vec![
        ("types.hd".into(), types),
        ("util.hd".into(), util),
        ("gen.hd".into(), generic),
        ("main.hd".into(), main),
    ]
}

const KEEP: &str = "xs := [x]\n    if bump(xs.len()) > 9:\n        return []\n    xs";
const PAIR: &str = "[x, y]";
const BUMP: &str = "x + 1";

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

/// How many of `out`'s owned instances are of `path`.
fn owned(out: &Output, path: &str) -> usize {
    let all = &out.counters.instances_owned;
    all.iter().filter(|p| *p == path).count()
}

/// A rebuild with no edit owns nothing and links nothing; after an edit
/// to `main`'s `note` alone, no instance is owned again; after an edit to
/// `keep`'s body, exactly `keep`'s `n` instances are, and none of
/// `pair`'s. Each warm module is the cold build's of the same source.
fn edits_own_only_their_readers(n: usize) {
    let store = MemoryStore::default();
    let base = program(n, KEEP, PAIR, "1", BUMP);
    let cold = build(&store, &base);
    assert_eq!(owned(&cold, "own/gen/keep"), n);
    assert_eq!(owned(&cold, "own/gen/pair"), n);

    let again = build(&store, &base);
    assert_eq!(again.counters.hit("link"), 1);
    assert_eq!(again.counters.instances_owned, Vec::<String>::new());
    assert_eq!(again.counters.bodies_owned, Vec::<String>::new());

    let noted = program(n, KEEP, PAIR, "2", BUMP);
    let warm = build(&store, &noted);
    assert_eq!(warm.counters.hit("link"), 0);
    assert_eq!(
        warm.counters.instances_owned,
        Vec::<String>::new(),
        "n = {n}"
    );
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &noted).wasm);

    let kept = program(n, &KEEP.replace("> 9", "> 8"), PAIR, "2", BUMP);
    let warm = build(&store, &kept);
    assert_eq!(owned(&warm, "own/gen/keep"), n, "n = {n}");
    assert_eq!(warm.counters.instances_owned.len(), n, "n = {n}");
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &kept).wasm);
}

#[test]
fn instance_edits_own_only_their_readers_at_20_types() {
    edits_own_only_their_readers(20);
}

#[test]
fn instance_edits_own_only_their_readers_at_200_types() {
    edits_own_only_their_readers(200);
}

/// `keep`'s folded form copies `util.bump`'s body: after an edit to
/// `bump` alone, which neither `keep`'s TIR nor any declaration shows,
/// every `keep` instance is owned again from the new copy, `pair`'s are
/// not, and the module is a cold build's.
#[test]
fn an_edit_to_a_body_an_instance_folded_in_owns_that_instance() {
    let store = MemoryStore::default();
    build(&store, &program(3, KEEP, PAIR, "1", BUMP));
    let edited = program(3, KEEP, PAIR, "1", "x + 20");
    let warm = build(&store, &edited);
    assert_eq!(owned(&warm, "own/gen/keep"), 3);
    assert_eq!(owned(&warm, "own/gen/pair"), 0);
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}

/// `keep` runs at `util.make`'s opaque result: after an edit to `make`'s
/// body alone that changes the hidden type from `i64` to `String`, the
/// warm module is a cold build's.
#[test]
fn an_edit_to_an_opaque_result_rebuilds_the_instances_at_it() {
    let files = |hidden: &str| {
        vec![
            (
                "util.hd".to_owned(),
                format!("pub fn bump(x: usize) -> usize:\n    x + 1\n\npub fn make() -> impl Display:\n    {hidden}\n"),
            ),
            (
                "gen.hd".to_owned(),
                format!("use pkg.util.bump\n\npub fn keep[T](x: T) -> List[T]:\n    {KEEP}\n"),
            ),
            (
                "main.hd".to_owned(),
                "use pkg.gen.keep\nuse pkg.util.make\n\npub fn main() -> void $ Console:\n    println(\"${keep(make()).len()}\")\n".to_owned(),
            ),
        ]
    };
    let store = MemoryStore::default();
    build(&store, &files("1"));
    let edited = files("\"s\"");
    let warm = build(&store, &edited);
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}
