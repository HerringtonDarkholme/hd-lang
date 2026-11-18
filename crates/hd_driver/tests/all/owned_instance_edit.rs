//! A dev build takes a generic instance's owned form from the store while
//! its instance key, its generic body, the program's declarations and
//! every body its folded form read are unchanged (#2106): a rebuild runs
//! the ownership passes on the instances that read an edited body, and on
//! no other.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Dependency, Executor, Goal, Host, NoClock, Output, Packages, build_packages};
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
    build_with(store, files, &[])
}

/// `files` built as package `own`, which requires package `shape` of
/// `dep` when `dep` is not empty.
fn build_with(store: &MemoryStore, files: &[(String, String)], dep: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let mut shape = MemorySources::default();
    for (path, text) in dep {
        shape.insert(path, text);
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
    let packages = if dep.is_empty() {
        Packages::default()
    } else {
        Packages {
            requires: vec![("shape".to_owned(), 1)],
            deps: vec![Dependency {
                name: "shape".to_owned(),
                identity: "/w/shape".to_owned(),
                sources: &shape,
                requires: Vec::new(),
                fetched: false,
            }],
            ..Packages::default()
        }
    };
    let out = build_packages(&host, "own", &packages, &goal);
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

/// `program` with `note` private, so its row is inferred.
fn private_note(n: usize, note: &str) -> Vec<(String, String)> {
    let mut files = program(n, KEEP, PAIR, note, BUMP);
    for (path, text) in &mut files {
        if path == "main.hd" {
            *text = text.replace("pub fn note()", "fn note()");
        }
    }
    files
}

/// The program's declarations are each module's checked items, not its
/// text: after an edit to the body of the private `note`, whose row is
/// inferred and stays empty, no instance is owned again, and of the
/// bodies only `note` and `main`, which inlines it. Each warm module is
/// the cold build's.
fn a_private_body_edit_owns_only_its_readers(n: usize) {
    let store = MemoryStore::default();
    let cold = build(&store, &private_note(n, "1"));
    assert_eq!(owned(&cold, "own/gen/keep"), n);
    let edited = private_note(n, "2");
    let warm = build(&store, &edited);
    assert_eq!(
        warm.counters.instances_owned,
        Vec::<String>::new(),
        "n = {n}"
    );
    let bodies = &warm.counters.bodies_owned;
    assert!(bodies.iter().any(|b| b == "own/main/note"), "{bodies:?}");
    assert!(
        bodies
            .iter()
            .all(|b| b == "own/main/note" || b == "own/main/main"),
        "n = {n}: {bodies:?}"
    );
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}

#[test]
fn a_private_body_edit_owns_only_its_readers_at_20_types() {
    a_private_body_edit_owns_only_its_readers(20);
}

#[test]
fn a_private_body_edit_owns_only_its_readers_at_200_types() {
    a_private_body_edit_owns_only_its_readers(200);
}

/// An edit to `note`'s body that changes its inferred row, from none to
/// `Console`, changes `main`'s checked items: its readers are owned again
/// (all of the program's, until recorded reads, `INSTANCE_TIR.md` at 421bcec83, step
/// 6b), and the warm module is the cold build's.
#[test]
fn an_edit_that_changes_an_inferred_row_owns_its_readers() {
    let store = MemoryStore::default();
    build(&store, &private_note(3, "1"));
    let edited = private_note(3, "println(\"n\")\n    1");
    let warm = build(&store, &edited);
    assert_eq!(owned(&warm, "own/gen/keep"), 3);
    let bodies = &warm.counters.bodies_owned;
    assert!(bodies.iter().any(|b| b == "own/main/main"), "{bodies:?}");
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}

/// `keep` runs at the inferred result of the private `item`: an edit to
/// `item`'s body alone that changes that result from `i64` to `string`
/// owns `keep` at `string`, and the warm module is the cold build's.
#[test]
fn an_edit_that_changes_an_inferred_result_owns_its_readers() {
    let files = |item: &str| {
        vec![
            (
                "gen.hd".to_owned(),
                format!("use pkg.util.bump\n\npub fn keep[T](x: T) -> List[T]:\n    {KEEP}\n"),
            ),
            (
                "util.hd".to_owned(),
                format!("pub fn bump(x: usize) -> usize:\n    {BUMP}\n"),
            ),
            (
                "main.hd".to_owned(),
                format!(
                    "use pkg.gen.keep\n\nfn item():\n    {item}\n\npub fn main() -> void $ Console:\n    println(\"${{keep(item()).len()}}\")\n"
                ),
            ),
        ]
    };
    let store = MemoryStore::default();
    build(&store, &files("1"));
    let edited = files("\"s\"");
    let warm = build(&store, &edited);
    assert_eq!(owned(&warm, "own/gen/keep"), 1);
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}

/// A field added to `shape.P` in another package makes it not `Copy`:
/// the root's `keep` instance at `P` and the body that passes a `P` are
/// owned again, and the warm module is the cold build's.
#[test]
fn a_layout_edit_in_another_package_owns_its_dependents() {
    let root = vec![
        (
            "gen.hd".to_owned(),
            "pub fn keep[T](x: T) -> List[T]:\n    [x]\n".to_owned(),
        ),
        (
            "main.hd".to_owned(),
            "use dep.shape.{P, make}\nuse pkg.gen.keep\n\nfn wrap(p: P) -> List[P]:\n    keep(p)\n\npub fn main() -> void $ Console:\n    println(\"${wrap(make()).len()}\")\n".to_owned(),
        ),
    ];
    let shape = |fields: &str, init: &str| {
        format!(
            "pub struct P:\n    pub n: i32\n{fields}\npub fn make() -> P:\n    P {{ n: 1{init} }}\n"
        )
    };
    let store = MemoryStore::default();
    let before = shape("", "");
    build_with(&store, &root, &[("src/lib.hd", &before)]);
    let after = shape("    pub s: List[i32]\n", ", s: []");
    let warm = build_with(&store, &root, &[("src/lib.hd", &after)]);
    assert_eq!(owned(&warm, "own/gen/keep"), 1);
    let bodies = &warm.counters.bodies_owned;
    assert!(bodies.iter().any(|b| b == "own/main/wrap"), "{bodies:?}");
    let fresh = build_with(&MemoryStore::default(), &root, &[("src/lib.hd", &after)]);
    assert_eq!(warm.wasm, fresh.wasm);
}

/// `keep` runs at `L`, a struct declared in the body of the private
/// `item`, whose inferred result names it: a field added to `L`, an edit
/// to `item`'s body alone that makes `L` not `Copy`, owns `keep` at `L`
/// again, and the warm module is the cold build's.
#[test]
fn an_edit_to_a_local_type_in_a_private_body_owns_its_readers() {
    let files = |fields: &str, init: &str| {
        vec![
            (
                "gen.hd".to_owned(),
                "pub fn keep[T](x: T) -> List[T]:\n    [x]\n".to_owned(),
            ),
            (
                "main.hd".to_owned(),
                format!(
                    "use pkg.gen.keep\n\nfn item():\n    struct L:\n        n: i32\n{fields}    L {{ n: 1{init} }}\n\npub fn main() -> void $ Console:\n    println(\"${{keep(item()).len()}}\")\n"
                ),
            ),
        ]
    };
    let store = MemoryStore::default();
    build(&store, &files("", ""));
    let edited = files("        s: List[i32]\n", ", s: []");
    let warm = build(&store, &edited);
    assert_eq!(owned(&warm, "own/gen/keep"), 1);
    assert_eq!(warm.wasm, build(&MemoryStore::default(), &edited).wasm);
}
