//! Generic parameters, associated types and trait-value bindings reach
//! instance keys by stable content, never by run IDs (`crates/hd_sched/DESIGN.md` §6.5):
//! the same program builds to the same Wasm and the same cache entries
//! under shuffled task orders, both executors, shuffled file order, and
//! cold versus warm after an unrelated edit.

use std::collections::BTreeMap;

use hd_cache::{CacheStore, EntryKind, MemoryStore};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const SHAPES: &str = "\
pub trait Supplier:
    type Item
    fn get(self) -> Self::Item

pub struct Ints:
    pub value: i32

pub struct Words:
    pub value: string

impl Supplier for Ints:
    type Item = i32
    fn get(self) -> i32: self.value

impl Supplier for Words:
    type Item = string
    fn get(self) -> string: self.value

pub trait Named:
    fn name(self) -> string

impl Named for Ints:
    fn name(self) -> string: \"ints\"

impl Named for Words:
    fn name(self) -> string: self.value
";

const MAIN: &str = "\
use pkg.geo.shapes.{Supplier, Named, Ints, Words}


fn second[A, B](a: A, b: B) -> B:
    b

fn label[T < Named](t: T) -> string:
    t.name()

pub fn main() -> void $ Console:
    println(label(Ints { value: 3 }))
    words := Words { value: \"w\" }
    println(label(words))
    println(second(+1, \"x\"))
    println(\"${Ints { value: 4 }.get()}\")
";

const UNRELATED: &str = "\
pub fn unrelated(a: i32) -> i32:
    a + 1
";

type Entries = BTreeMap<(EntryKind, u128), Vec<u8>>;

fn sources(files: &[(&str, &str)]) -> MemorySources {
    let mut s = MemorySources::default();
    for (path, text) in files {
        s.insert(path, text);
    }
    s
}

fn run(store: &MemoryStore, src: &MemorySources, executor: Executor) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor,
    };
    build(
        &host,
        "demo",
        &Goal::Program {
            entry: "app.main".into(),
        },
    )
}

fn entries(store: &MemoryStore) -> Entries {
    store
        .drain_new()
        .into_iter()
        .filter_map(|(kind, key)| {
            let bytes = store.get(kind, &key)?;
            Some(((kind, key.0), bytes.to_vec()))
        })
        .collect()
}

fn build_in(src: &MemorySources, executor: Executor) -> (Output, Entries) {
    let store = MemoryStore::default();
    let out = run(&store, src, executor);
    assert!(out.diags.is_empty(), "{}", out.render());
    assert!(out.wasm.is_some());
    let e = entries(&store);
    (out, e)
}

fn same_entries(got: &Entries, want: &Entries, what: &str) {
    let only = |a: &Entries, b: &Entries| -> Vec<String> {
        a.iter()
            .filter(|(k, v)| b.get(k) != Some(v))
            .map(|(k, _)| format!("{:?}:{:x}", k.0, k.1))
            .collect()
    };
    assert!(
        got == want,
        "{what}: only in this run {:?}, only in the base {:?}",
        only(got, want),
        only(want, got)
    );
}

fn base() -> MemorySources {
    sources(&[("app/main.hd", MAIN), ("geo/shapes.hd", SHAPES)])
}

/// Builds `src` in every task order, on the pool and under other hash
/// seeds; each build matches the priority-order one. Returns that base.
fn in_every_order(src: fn() -> MemorySources) -> (Output, Entries) {
    let (base_out, base_entries) = build_in(&src(), Executor::Serial(SerialOrder::Priority));
    for order in [
        SerialOrder::Fifo,
        SerialOrder::Shuffled(1),
        SerialOrder::Shuffled(7),
        SerialOrder::Shuffled(0x5eed),
    ] {
        let (out, e) = build_in(&src(), Executor::Serial(order));
        assert_eq!(out.wasm, base_out.wasm, "{order:?}");
        same_entries(&e, &base_entries, &format!("{order:?}"));
    }
    for n in [2, 4] {
        let (out, e) = build_in(&src(), Executor::Pool(n));
        assert_eq!(out.wasm, base_out.wasm, "pool {n}");
        same_entries(&e, &base_entries, &format!("pool {n}"));
    }
    // Another hash-table seed: no table's order reaches the output
    // (docs/compiler/data-structures.md §3.2).
    for seed in [1, 0x5eed] {
        hd_base::set_hash_seed(seed);
        let (out, e) = build_in(&src(), Executor::Pool(2));
        hd_base::set_hash_seed(0);
        assert_eq!(out.wasm, base_out.wasm, "seed {seed}");
        same_entries(&e, &base_entries, &format!("seed {seed}"));
    }
    (base_out, base_entries)
}

#[test]
fn generic_programs_build_identically_in_every_order() {
    let (base_out, base_entries) = in_every_order(base);
    let flipped = sources(&[("geo/shapes.hd", SHAPES), ("app/main.hd", MAIN)]);
    let (out, e) = build_in(&flipped, Executor::Serial(SerialOrder::Shuffled(3)));
    assert_eq!(out.wasm, base_out.wasm, "flipped files");
    same_entries(&e, &base_entries, "flipped files");
}

#[test]
fn an_unrelated_edit_keeps_generic_instances() {
    let (cold, cold_entries) = build_in(&base(), Executor::Serial(SerialOrder::Priority));
    let edited = sources(&[
        ("app/main.hd", MAIN),
        ("geo/shapes.hd", SHAPES),
        ("geo/unrelated.hd", UNRELATED),
    ]);
    let (other, other_entries) = build_in(&edited, Executor::Serial(SerialOrder::Shuffled(9)));
    assert_eq!(other.wasm, cold.wasm);
    for (k, bytes) in &cold_entries {
        if matches!(k.0, EntryKind::Codepack | EntryKind::Clpack) {
            assert_eq!(other_entries.get(k), Some(bytes), "{:?}", k.0);
        }
    }
    let store = MemoryStore::default();
    let first = run(&store, &base(), Executor::Serial(SerialOrder::Priority));
    assert!(first.diags.is_empty(), "{}", first.render());
    let warm = run(&store, &edited, Executor::Serial(SerialOrder::Fifo));
    assert!(warm.diags.is_empty(), "{}", warm.render());
    assert_eq!(warm.wasm, cold.wasm);
}

// Derived `Eq` and `Debug` instances: their bodies rewrite the template's
// `Structure` calls, and no word of the template's call may stay behind
// with a run's pool ID in it (#610).
const RECORDS: &str = "\
@derive(Eq, Debug)
pub struct Pair[T]:
    pub left: T
    pub right: T

@derive(Eq, Debug)
pub enum Shape:
    Circle(i32)
    Square(Pair[i32])
";

const RECORDS_MAIN: &str = "\
use pkg.geo.records.{Pair, Shape}

pub fn main() -> void $ Console:
    a := Shape.Square(Pair { left: +1, right: +2 })
    b := Shape.Circle(+3)
    println(\"${a == b} ${Pair { left: \"x\", right: \"y\" } == Pair { left: \"x\", right: \"y\" }}\")
";

fn records() -> MemorySources {
    sources(&[("app/main.hd", RECORDS_MAIN), ("geo/records.hd", RECORDS)])
}

#[test]
fn derived_instances_build_identically_in_every_order() {
    in_every_order(records);
}
