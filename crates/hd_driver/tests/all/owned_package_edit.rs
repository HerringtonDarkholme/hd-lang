//! A dev build takes a package body's owned form from the store while the
//! body, the program's declarations and every body the passes read are
//! unchanged (#2028): after a one-module body edit, the passes run on that
//! module's bodies and the bodies that inline them, not on every body of
//! the package.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// #2028's program: `k` modules of 20 small functions and a `run` each,
/// and a `main` that calls every `run`. Module `m0` builds its list with
/// weights `i + bump`.
fn program(k: usize, bump: u32) -> Vec<(String, String)> {
    let mut files = Vec::with_capacity(k + 1);
    for m in 0..k {
        let mut src = String::from("pub struct W:\n    pub id: i32\n    pub weight: i64\n\n");
        for i in 0..20 {
            let _ = write!(
                src,
                "fn chain{i}(xs: List[W]) -> i64:\n    xs.iter().filter(fn(w): w.weight % 3 == {}).map(fn(w): w.weight * i64(w.id + {i})).sum::[i64]()\n\n",
                i % 3
            );
        }
        let b = if m == 0 { bump } else { 1 };
        let _ = write!(
            src,
            "pub fn run(n: usize) -> i64:\n    xs := [for i in 0..n => W {{ id: i32(i), weight: i64(i) + {b} }}]\n    let s: i64 = 0\n"
        );
        for i in 0..20 {
            let _ = writeln!(src, "    s = s + chain{i}(xs)");
        }
        src += "    s\n";
        files.push((format!("m{m}.hd"), src));
    }
    let mut main = String::new();
    for m in 0..k {
        let _ = writeln!(main, "use pkg.m{m}.run as run{m}");
    }
    main += "\npub fn main() -> void $ Console:\n    let s: i64 = 0\n";
    for m in 0..k {
        let _ = writeln!(main, "    s = s + run{m}(10)");
    }
    main += "    println(\"$s\")\n";
    files.push(("main.hd".into(), main));
    files
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

/// After an edit to `m0.run`'s body, a warm build owns `m0.run` and
/// `main`, whose inliner reads it, and no other body: the same two at
/// `k` = 25 and 100. Its module is the cold build's.
fn edit_owns_only_the_edit(k: usize) {
    let store = MemoryStore::default();
    let cold = build(&store, &program(k, 1));
    assert!(cold.counters.bodies_owned.len() > 21 * k);
    let again = build(&store, &program(k, 1));
    assert_eq!(again.counters.bodies_owned, Vec::<String>::new());
    let edited = program(k, 2);
    let warm = build(&store, &edited);
    let fresh = build(&MemoryStore::default(), &edited);
    assert_eq!(warm.wasm, fresh.wasm);
    assert_eq!(
        warm.counters.bodies_owned,
        ["own/main/main", "own/m0/run"],
        "k = {k}"
    );
}

#[test]
fn a_body_edit_owns_its_module_and_inliners_at_25_modules() {
    edit_owns_only_the_edit(25);
}

#[test]
fn a_body_edit_owns_its_module_and_inliners_at_100_modules() {
    edit_owns_only_the_edit(100);
}

/// A field type changed in `m1` changes how `m0.keep`, whose TIR names
/// `m1.Inner` only by path, passes its argument: `Inner` stops being
/// `Copy`, though `m0`'s source is the same. The warm build owns `keep` again, and its module is the cold
/// build's. (An impl added in another module cannot change a non-generic
/// body's selection: `impl !Copy` belongs in its target's module, and a
/// body that calls a trait its argument lacks does not check.)
#[test]
fn a_declaration_edit_in_another_module_owns_the_bodies_that_read_it() {
    let m0 = "use pkg.m1.Inner\n\npub fn keep(a: Inner) -> List[Inner]:\n    [a]\n";
    let main = "use pkg.m1.make\nuse pkg.m0.keep\n\npub fn main() -> void $ Console:\n    println(\"${keep(make()).len()}\")\n";
    let files = |m1: &str| {
        vec![
            ("m0.hd".to_owned(), m0.to_owned()),
            ("m1.hd".to_owned(), m1.to_owned()),
            ("main.hd".to_owned(), main.to_owned()),
        ]
    };
    let store = MemoryStore::default();
    let inner = "pub struct Inner:\n    pub n: i32\n";
    build(
        &store,
        &files(&format!(
            "{inner}\npub fn make() -> Inner:\n    Inner {{ n: 1 }}\n"
        )),
    );
    let edited = files(&format!(
        "{inner}    pub s: List[i32]\n\npub fn make() -> Inner:\n    Inner {{ n: 1, s: [] }}\n"
    ));
    let warm = build(&store, &edited);
    let fresh = build(&MemoryStore::default(), &edited);
    assert_eq!(warm.wasm, fresh.wasm);
    let owned = &warm.counters.bodies_owned;
    assert!(owned.iter().any(|b| b == "own/m0/keep"), "{owned:?}");
}

/// `main` inlines `m1.bump`: after an edit to `bump`'s body alone, the
/// warm build owns `main` again, with the new copy, and its module is the
/// cold build's.
#[test]
fn an_edit_to_an_inlined_package_callee_owns_its_caller() {
    let main = "use pkg.m1.bump\n\npub fn main() -> void $ Console:\n    println(bump(41))\n";
    let files = |ret: &str| {
        vec![
            (
                "m1.hd".to_owned(),
                format!("pub fn bump(x: i64) -> i64:\n    {ret}\n"),
            ),
            ("main.hd".to_owned(), main.to_owned()),
        ]
    };
    let store = MemoryStore::default();
    build(&store, &files("x + 1"));
    let warm = build(&store, &files("x + 2"));
    let fresh = build(&MemoryStore::default(), &files("x + 2"));
    assert_eq!(warm.wasm, fresh.wasm);
    assert_eq!(warm.counters.bodies_owned, ["own/main/main", "own/m1/bump"]);
}

/// `m1.pick` is generic, and `main` and `m0.use_pick` call it at `i64`:
/// after an edit to `pick`'s body alone, the warm build owns both callers
/// again, the second from the reads the first recorded for `pick`'s
/// inlined form, and its module is the cold build's.
#[test]
fn an_edit_to_a_generic_package_callee_owns_each_caller() {
    let m0 = "use pkg.m1.pick\n\npub fn use_pick(x: i64) -> i64:\n    pick(x, 2)\n";
    let main = "use pkg.m1.pick\nuse pkg.m0.use_pick\n\npub fn main() -> void $ Console:\n    println(pick(use_pick(41), 3))\n";
    let files = |body: &str| {
        vec![
            ("m0.hd".to_owned(), m0.to_owned()),
            (
                "m1.hd".to_owned(),
                format!("pub fn pick[T](a: T, b: T) -> T:\n    {body}\n"),
            ),
            ("main.hd".to_owned(), main.to_owned()),
        ]
    };
    let store = MemoryStore::default();
    build(&store, &files("a"));
    let warm = build(&store, &files("b"));
    let fresh = build(&MemoryStore::default(), &files("b"));
    assert_eq!(warm.wasm, fresh.wasm);
    let owned = &warm.counters.bodies_owned;
    for body in ["own/main/main", "own/m0/use_pick"] {
        assert!(owned.iter().any(|b| b == body), "{body} not in {owned:?}");
    }
}
