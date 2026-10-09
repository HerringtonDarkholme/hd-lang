//! A requirement row's canonical encoding is content order, not this run's
//! interning order (scheduler.md §6.5, cache.md determinism): the same
//! row-polymorphic program builds to the same Wasm and the same cache
//! entries under shuffled task orders and both executors, and an unrelated
//! edit leaves the untouched instances' entries as they were.

use std::collections::BTreeMap;

use hd_cache::{CacheStore, EntryKind, MemoryStore};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const KEYS: &str = "\
pub trait Clock:
    fn now(self) -> i32

pub trait Tag:
    fn id(self) -> i32

pub data FixedClock:
    value: i32

pub data FixedTag:
    value: i32

impl Clock for FixedClock:
    fn now(self) -> i32: self.value

impl Tag for FixedTag:
    fn id(self) -> i32: self.value
";

const MAIN: &str = "\
use pkg.geo.keys.{Clock, Tag, FixedClock, FixedTag}

fn run_all[$R](callback: fn() -> i32 $ R + Clock + Tag) -> i32 $ R:
    $.with(Clock=FixedClock { value: 7 }, Tag=FixedTag { value: 10 }):
        callback()

fn run_two[$R](callback: fn() -> i32 $ R + Tag + Console) -> i32 $ R + Console:
    $.with(Tag=FixedTag { value: 20 }):
        callback()

fn three() -> i32 $ Tag + Clock + Console:
    println(\"${$.use(Tag).id()}\")
    $.use(Clock).now()

fn tagged() -> i32 $ Console + Clock:
    run_all(fn(): $.use(Tag).id() + $.use(Clock).now())

pub fn main() -> void $ Console:
    $.with(Clock=FixedClock { value: 1 }):
        println(\"${tagged()}\")
        println(\"${run_two(fn(): $.use(Tag).id())}\")
        println(\"${run_all(fn(): three())}\")
";

const UNRELATED: &str = "\
pub fn unrelated(a: i32) -> i32:
    a + 1
";

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

/// Every entry a run wrote, by kind and key, with its bytes.
fn entries(store: &MemoryStore) -> BTreeMap<(EntryKind, u128), Vec<u8>> {
    store
        .drain_new()
        .into_iter()
        .filter_map(|(kind, key)| {
            let bytes = store.get(kind, &key)?;
            Some(((kind, key.0), bytes.to_vec()))
        })
        .collect()
}

fn build_in(
    src: &MemorySources,
    executor: Executor,
) -> (Output, BTreeMap<(EntryKind, u128), Vec<u8>>) {
    let store = MemoryStore::default();
    let out = run(&store, src, executor);
    assert!(out.diags.is_empty(), "{}", out.render());
    assert!(out.wasm.is_some());
    let e = entries(&store);
    (out, e)
}

type Entries = BTreeMap<(EntryKind, u128), Vec<u8>>;

/// Equal entry sets, reported by kind and key so a failure stays readable.
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
    sources(&[("app/main.hd", MAIN), ("geo/keys.hd", KEYS)])
}

#[test]
fn row_polymorphic_programs_build_identically_in_every_order() {
    let (base_out, base_entries) = build_in(&base(), Executor::Serial(SerialOrder::Priority));
    for order in [
        SerialOrder::Fifo,
        SerialOrder::Shuffled(1),
        SerialOrder::Shuffled(7),
        SerialOrder::Shuffled(0x5eed),
    ] {
        let (out, e) = build_in(&base(), Executor::Serial(order));
        assert_eq!(out.wasm, base_out.wasm, "{order:?}");
        same_entries(&e, &base_entries, &format!("{order:?}"));
    }
    for n in [2, 4] {
        let (out, e) = build_in(&base(), Executor::Pool(n));
        assert_eq!(out.wasm, base_out.wasm, "pool {n}");
        same_entries(&e, &base_entries, &format!("pool {n}"));
    }
    // File insertion order changes which type and trait is interned first.
    let flipped = sources(&[("geo/keys.hd", KEYS), ("app/main.hd", MAIN)]);
    let (out, e) = build_in(&flipped, Executor::Serial(SerialOrder::Shuffled(3)));
    assert_eq!(out.wasm, base_out.wasm, "flipped files");
    same_entries(&e, &base_entries, "flipped files");
}

#[test]
fn an_unrelated_edit_keeps_the_untouched_instances() {
    let (cold, cold_entries) = build_in(&base(), Executor::Serial(SerialOrder::Priority));

    // A fresh store and an extra module interned first: the untouched
    // instances get the same code entries even though every run ID shifted.
    let edited = sources(&[
        ("app/main.hd", MAIN),
        ("geo/keys.hd", KEYS),
        ("geo/unrelated.hd", UNRELATED),
    ]);
    let (other, other_entries) = build_in(&edited, Executor::Serial(SerialOrder::Shuffled(9)));
    assert_eq!(other.wasm, cold.wasm);
    for (k, bytes) in &cold_entries {
        if matches!(k.0, EntryKind::Codepack | EntryKind::Clpack) {
            assert_eq!(other_entries.get(k), Some(bytes), "{:?}", k.0);
        }
    }

    // A warm store: the edit re-checks the new module only, and no
    // instance is emitted again.
    let store = MemoryStore::default();
    let first = run(&store, &base(), Executor::Serial(SerialOrder::Priority));
    assert!(first.counters.emitted > 0);
    let warm = run(&store, &edited, Executor::Serial(SerialOrder::Fifo));
    assert!(warm.diags.is_empty(), "{}", warm.render());
    assert_eq!(warm.counters.emitted, 0);
    assert_eq!(warm.wasm, cold.wasm);
}
