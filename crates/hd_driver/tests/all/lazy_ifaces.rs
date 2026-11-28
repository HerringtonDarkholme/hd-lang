//! A warm check reads the interface records it touches, not every record
//! of every unit it reads (docs/compiler/data-structures.md §3.9.1,
//! #213). The solver's impl tables read every impl of the closure, and a
//! method call's index every trait; any other item is decoded only when
//! named.

use hd_base::Hash128;
use hd_cache::{CacheStore, EntryKind, MemoryStore};
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn check(store: &dyn CacheStore, main: &str) -> Output {
    build(store, main, &Goal::Analyze)
}

fn build(store: &dyn CacheStore, main: &str, goal: &Goal) -> Output {
    // A stable identity exercises retained last-run rows, not only entries.
    let mut src = MemorySources::named("lazy-ifaces");
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(&host, "app", &Packages::default(), goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

#[test]
fn a_warm_check_decodes_few_interface_records() {
    let store = MemoryStore::default();
    let main = include_str!("programs/lazy_ifaces/main.hd");
    check(&store, main);
    let edited = include_str!("programs/lazy_ifaces/edited.hd");
    let warm = check(&store, edited);
    let c = &warm.counters;
    // `println` and the little it names, out of std's hundreds of items.
    assert!(c.iface_other_records <= 10, "{}", c.iface_other_records);
}

/// Evict both standalone interfaces and their canonical source-set records.
#[derive(Default)]
struct EvictedIfaces {
    inner: MemoryStore,
    evict: AtomicBool,
    misses: AtomicUsize,
}

impl CacheStore for EvictedIfaces {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        if matches!(kind, EntryKind::Iface | EntryKind::Depfiles)
            && self.evict.load(Ordering::Relaxed)
        {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        } else {
            self.inner.get(kind, key)
        }
    }

    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        self.inner.put(kind, key, bytes);
    }

    fn touch(&self, kind: EntryKind, key: &Hash128) {
        self.inner.touch(kind, key);
    }
}

#[test]
fn an_evicted_canonical_interface_record_is_rebuilt() {
    let store = EvictedIfaces::default();
    check(&store, include_str!("programs/lazy_ifaces/main.hd"));
    store.evict.store(true, Ordering::Relaxed);
    let edited = include_str!("programs/lazy_ifaces/edited.hd");
    let rebuilt = check(&store, edited);
    assert!(
        rebuilt
            .counters
            .ifaces_built
            .iter()
            .any(|p| p == "app.main")
    );
    assert!(store.misses.load(Ordering::Relaxed) > 0);
    store.evict.store(false, Ordering::Relaxed);
    check(&store, edited);
}

/// Count one interface blob in actual stored frames, including the indexed
/// record format. Other Depfiles clients are rejected by the complete decode.
fn stored_blobs(kind: EntryKind, bytes: &[u8], wanted: Hash128) -> usize {
    let Ok((_, sections)) = hd_cache::decode_entry(bytes) else {
        return 0;
    };
    let Some((_, payload)) = sections.first() else {
        return 0;
    };
    if kind == EntryKind::Iface {
        return usize::from(hd_base::hash128(payload) == wanted);
    }
    if kind != EntryKind::Depfiles {
        return 0;
    }
    let mut r = hd_base::wire::Reader::new(payload);
    let mut count = 0;
    for _ in 0..r.count() {
        let tag = r.u8();
        if tag > 2 {
            return 0;
        }
        let _path = r.blob();
        let _key = r.hash();
        let _source = r.hash();
        let data = r.blob();
        if !r.ok() {
            return 0;
        }
        if tag == 1 {
            let mut row = hd_base::wire::Reader::new(data);
            let _reads = row.blob();
            let blob = row.blob();
            count += usize::from(row.ok() && hd_base::hash128(blob) == wanted);
        }
    }
    if r.ok() && r.at_end() { count } else { 0 }
}

#[test]
fn a_named_check_stores_each_interface_blob_once() {
    let store = MemoryStore::default();
    let out = check(&store, include_str!("programs/lazy_ifaces/main.hd"));
    let root = out
        .counters
        .iface_blobs
        .iter()
        .find(|(path, _)| !path.starts_with("std"))
        .expect("root interface")
        .1;
    let entries: std::collections::BTreeSet<_> = store.drain_new().into_iter().collect();
    let copies: usize = entries
        .into_iter()
        .map(|(kind, key)| {
            let bytes = store.get(kind, &key).expect("written entry");
            stored_blobs(kind, &bytes, *root)
        })
        .sum();
    assert_eq!(
        copies, 1,
        "canonical interface blob is never repeated in a row and standalone entry"
    );
}

#[test]
fn a_named_program_build_reuses_its_checks_canonical_interfaces() {
    let store = MemoryStore::default();
    let main = include_str!("programs/lazy_ifaces/main.hd");
    check(&store, main);
    let out = build(
        &store,
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.wasm.is_some(), "{}", out.render());
    assert!(
        out.counters.ifaces_built.is_empty(),
        "{:?}",
        out.counters.ifaces_built
    );
}

#[test]
fn a_body_edit_does_not_rewrite_canonical_interface_records() {
    let store = MemoryStore::default();
    let out = check(&store, include_str!("programs/lazy_ifaces/main.hd"));
    let root = out
        .counters
        .iface_blobs
        .iter()
        .find(|(path, _)| !path.starts_with("std"))
        .expect("root interface")
        .1;
    let canonical: std::collections::BTreeSet<_> = store
        .drain_new()
        .into_iter()
        .filter(|&(kind, key)| {
            stored_blobs(kind, &store.get(kind, &key).expect("written entry"), *root) > 0
        })
        .collect();
    assert_eq!(canonical.len(), 1);
    check(&store, include_str!("programs/lazy_ifaces/edited.hd"));
    assert!(
        store
            .drain_new()
            .into_iter()
            .all(|entry| !canonical.contains(&entry)),
        "a body-only edit must not rewrite the canonical interface record"
    );
}
