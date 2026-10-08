//! Slice 2's exit test (build-order.md §9) with std as a program's
//! dependency: std's folder interfaces build with no diagnostic, blobs
//! decode to equal interfaces, blob bytes are equal across shuffled serial
//! orders, and a deep-hash edit reaches a dependent's key.

use std::collections::BTreeMap;
use std::sync::Arc;

use hd_base::Stage;
use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_intern::{PathTable, ShardedInterner};
use hd_project::MemorySources;
use hd_resolve::{ItemData, Names, decode_items, encode_items};
use hd_sched::SerialOrder;
use hd_types::InternPool;

fn run_with(src: &MemorySources, store: &MemoryStore, order: SerialOrder) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(order),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn run(src: &MemorySources) -> Output {
    run_with(src, &MemoryStore::default(), SerialOrder::Priority)
}

fn hello() -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("main.hd", "fn main() -> void $ Console:\n    println(42)\n");
    s
}

fn fresh<T>(f: impl FnOnce(&Names<'_>) -> T) -> T {
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    f(&Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
    })
}

#[test]
fn std_interfaces_build_with_no_diagnostic() {
    let out = run(&hello());
    assert!(out.diags.is_empty(), "{}", out.render());
    assert!(out.wasm.is_some(), "hello world links");
    for st in [Stage::FolderIface, Stage::HeaderCheck, Stage::Coherence] {
        let t = out.report.tally(st);
        assert_eq!(
            (t.not_implemented, t.blocked),
            (0, 0),
            "{}",
            out.report.render()
        );
    }
    let folders: Vec<&str> = out.ifaces.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(folders, ["app", "std", "std.prelude", "std.testing"]);
    assert_eq!(out.report.tally(Stage::FolderIface).ok, 4);
}

#[test]
fn the_prelude_module_reexports_the_prelude_table() {
    let out = run(&hello());
    let (_, blob) = out
        .ifaces
        .iter()
        .find(|f| f.0 == "std.prelude")
        .expect("std.prelude");
    fresh(|n| {
        let (_, exports) = decode_items(n, blob).expect("decode");
        let mut got: Vec<(String, String)> = exports
            .iter()
            .filter(|e| e.module == "std.prelude")
            .map(|e| (n.module_of(e.def), n.text(e.name).to_owned()))
            .collect();
        got.sort();
        let mut want: Vec<(String, String)> = hd_resolve::PRELUDE
            .iter()
            .flat_map(|(m, names)| names.iter().map(|x| ((*m).to_owned(), (*x).to_owned())))
            .collect();
        want.sort();
        assert_eq!(got, want);
    });
}

#[test]
fn std_interfaces_hold_no_poison_type() {
    let out = run(&hello());
    for (folder, blob) in out.ifaces.iter().filter(|f| f.0.starts_with("std")) {
        fresh(|n| {
            let (items, exports) = decode_items(n, blob).expect("decode");
            assert!(!exports.is_empty(), "{folder}");
            let mut bad = Vec::new();
            for it in &items {
                let mut tys: Vec<hd_types::Ty> = it
                    .generics
                    .iter()
                    .flat_map(|g| g.bounds.iter().copied().chain(g.default))
                    .collect();
                match &it.data {
                    ItemData::Fn(s) | ItemData::Method { sig: s, .. } => {
                        tys.extend(s.params.iter().map(|p| p.1));
                        tys.push(s.ret);
                        for g in &s.generics {
                            tys.extend(g.bounds.iter().copied().chain(g.default));
                        }
                    }
                    ItemData::Data(fs) => tys.extend(fs.iter().map(|f| f.ty)),
                    ItemData::Enum { shared, variants } => tys.extend(
                        shared
                            .iter()
                            .chain(variants.iter().flat_map(|v| &v.fields))
                            .map(|f| f.ty),
                    ),
                    ItemData::Trait(t) => tys.extend(t.supers.iter().copied()),
                    ItemData::Impl { self_ty, assoc, .. } => {
                        tys.push(*self_ty);
                        tys.extend(assoc.iter().map(|a| a.1));
                    }
                    ItemData::AssocType { default, .. } => tys.extend(*default),
                    ItemData::Alias(t) | ItemData::Newtype(t) => tys.push(*t),
                }
                if tys.iter().any(|t| n.pool.has_poison(*t)) {
                    bad.push(format!("{} {:?}", n.path(it.def), it.data));
                }
            }
            assert!(
                bad.is_empty(),
                "{folder}: {} items with poison: {:#?}",
                bad.len(),
                &bad[..bad.len().min(10)]
            );
        });
    }
}

#[test]
fn blobs_decode_to_equal_interfaces() {
    let out = run(&hello());
    assert!(!out.ifaces.is_empty());
    for (folder, blob) in &out.ifaces {
        let again = fresh(|n| {
            let (items, exports) = decode_items(n, blob).expect("decode");
            let (items2, exports2) = decode_items(n, blob).expect("decode twice");
            assert_eq!((&items, &exports), (&items2, &exports2), "{folder}");
            encode_items(n, &items, &exports).expect("encode")
        });
        assert_eq!(
            &again[..],
            &blob[..],
            "{folder}: re-encoding changed the bytes"
        );
        let third = fresh(|n| {
            let (items, exports) = decode_items(n, &again).expect("decode");
            encode_items(n, &items, &exports).expect("encode")
        });
        assert_eq!(third, again, "{folder}");
    }
}

#[test]
fn blob_bytes_are_equal_across_shuffled_serial_orders() {
    let src = hello();
    let blobs = |order| -> BTreeMap<String, Arc<[u8]>> {
        run_with(&src, &MemoryStore::default(), order)
            .ifaces
            .into_iter()
            .collect()
    };
    let base = blobs(SerialOrder::Priority);
    assert_eq!(base.len(), 4);
    for order in [
        SerialOrder::Fifo,
        SerialOrder::Shuffled(1),
        SerialOrder::Shuffled(7),
        SerialOrder::Shuffled(0x5eed),
    ] {
        assert_eq!(blobs(order), base, "{order:?}");
    }
}

#[test]
fn a_type_reached_only_through_a_signature_changes_the_dependents_key() {
    let types = |field: &str| format!("pub data User:\n    pub name: {field}\n");
    let service = "use pkg.user.types.{User}\n\npub fn load() -> User:\n    User { name: 1 }\n";
    let main =
        "use pkg.service.load.{load}\n\nfn main() -> void $ Console:\n    println(load().name)\n";
    let program = |field: &str| {
        let mut s = MemorySources::default();
        s.insert("main.hd", main);
        s.insert("service/load.hd", service);
        s.insert("user/types.hd", &types(field));
        s
    };
    let store = MemoryStore::default();
    let a = run_with(&program("i32"), &store, SerialOrder::Priority);
    assert!(a.diags.is_empty(), "{}", a.render());
    let b = run_with(&program("i64"), &store, SerialOrder::Priority);
    let deep = |o: &Output, f: &str| o.counters.deep_hashes.get(f).copied();
    assert_ne!(deep(&a, "app.user"), deep(&b, "app.user"));
    // `service` did not change, but its signature names `User`.
    assert_ne!(deep(&a, "app.service"), deep(&b, "app.service"));
    // `main` names neither `User` nor its folder.
    assert_ne!(
        a.counters.check_keys.get("app.main"),
        b.counters.check_keys.get("app.main")
    );
    assert!(b.counters.ifaces_built.contains(&"app.service".to_owned()));
    assert!(!b.counters.ifaces_built.contains(&"std".to_owned()));
}

const STATS: &str = "\
use std.cmp.Ordering

pub data Point:
    pub x: i32
    pub y: i32

impl Eq for Point:
    fn eq(self, other: Point) -> bool:
        self.x == other.x

pub fn count(xs: List[i32], fallback: i32?) -> i32:
    3

pub fn rank(o: Ordering, seen: Map[string, List[i32]]) -> Result[i32, string]?:
    .None

pub fn total(a: i32, b: i32) -> i32:
    a + b
";

#[test]
fn std_typed_signatures_compile_through_headers() {
    let mut s = MemorySources::default();
    s.insert("stats/calc.hd", STATS);
    s.insert(
        "main.hd",
        "use pkg.stats.calc.{total}\n\nfn main() -> void $ Console:\n    println(total(3, 4))\n",
    );
    let out = run(&s);
    let t = out.report.tally(Stage::FolderIface);
    assert_eq!((t.ok, t.not_implemented, t.blocked), (5, 0, 0));
    assert_eq!(out.report.tally(Stage::HeaderCheck).ok, 5);
    assert_eq!(out.report.tally(Stage::Coherence).ok, 1);
    let text = out.render();
    assert!(
        !text.contains("unknown-type") && !text.contains("unknown-import"),
        "{text}"
    );
}

fn check(files: &[(&str, &str)]) -> String {
    let mut s = MemorySources::default();
    for (p, t) in files {
        s.insert(p, t);
    }
    if !files.iter().any(|f| f.0 == "main.hd") {
        s.insert("main.hd", "fn main() -> void $ Console:\n    println(1)\n");
    }
    run(&s).render()
}

#[test]
fn name_phase_errors() {
    let r = check(&[
        (
            "a/x.hd",
            "pub use pkg.a.y.{B}\nuse pkg.a.y.{Hidden}\nuse pkg.b.z.{Nope}\npub data A:\n    pub v: i32\n",
        ),
        ("a/y.hd", "pub use pkg.a.x.{B}\ndata Hidden:\n    v: i32\n"),
        ("b/z.hd", "pub data Z:\n    pub v: i32\n"),
    ]);
    assert!(r.contains("re-export-loop"), "{r}");
    assert!(r.contains("private-import"), "{r}");
    assert!(r.contains("unknown-import"), "{r}");
    let r = check(&[("a/x.hd", "use std.nothing.{X}\n")]);
    assert!(r.contains("unknown-module"), "{r}");
    let r = check(&[("a/x.hd", "fn println(v: i32) -> void:\n    pass\n")]);
    assert!(r.contains("prelude-name-shadow"), "{r}");
}

#[test]
fn ownership_errors() {
    let r = check(&[(
        "a/x.hd",
        "impl Display for i32:\n    fn to_text(self) -> string:\n        \"\"\n",
    )]);
    assert!(r.contains("orphan-impl"), "{r}");
    let r = check(&[
        ("a/x.hd", "pub trait Shape:\n    fn area(self) -> i32\n"),
        ("b/y.hd", "pub data P:\n    pub v: i32\n"),
        (
            "c/z.hd",
            "use pkg.a.x.{Shape}\nuse pkg.b.y.{P}\n\nimpl Shape for P:\n    fn area(self) -> i32:\n        1\n",
        ),
    ]);
    assert!(r.contains("nonlocal-impl"), "{r}");
}

#[test]
fn orphan_impl_overlapping_std_is_one_error() {
    let r = check(&[(
        "a/x.hd",
        "impl Display for i32:\n    fn to_string(self) -> string:\n        \"\"\n",
    )]);
    assert_eq!(r.lines().count(), 1, "{r}");
    assert!(r.contains("error: a/x.hd:0..66: orphan-impl: "), "{r}");
    assert!(!r.contains("overlapping-impl"), "{r}");
}

#[test]
fn stage_b_and_coherence_errors() {
    let r = check(&[(
        "a/x.hd",
        "pub data Set[K < Hash]:\n    pub k: K\n\npub fn first[T](s: Set[T]) -> i32:\n    1\n",
    )]);
    assert!(r.contains("unsatisfied-trait-bound"), "{r}");
    let r = check(&[(
        "a/x.hd",
        "pub trait A:\n    fn a(self) -> i32\npub trait B < A:\n    fn b(self) -> i32\npub data P:\n    pub v: i32\nimpl B for P:\n    fn b(self) -> i32:\n        1\n",
    )]);
    assert!(r.contains("missing-supertrait-implementation"), "{r}");
    let r = check(&[(
        "a/x.hd",
        "pub trait Shape:\n    fn area(self) -> i32\npub data Box[T]:\n    pub v: T\nimpl[T] Shape for Box[T]:\n    fn area(self) -> i32:\n        1\nimpl Shape for Box[i32]:\n    fn area(self) -> i32:\n        2\n",
    )]);
    assert!(
        r.contains("overlapping-impl") && r.contains("Box[i32]"),
        "{r}"
    );
}
