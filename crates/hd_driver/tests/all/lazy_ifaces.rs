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
    let goal = Goal::Analyze;
    let out = build_packages(&host, "app", &Packages::default(), &goal);
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

/// Canonical entries may be evicted independently of the last-run rows.
#[derive(Default)]
struct EvictedIfaces {
    inner: MemoryStore,
    evict: AtomicBool,
    misses: AtomicUsize,
}

impl CacheStore for EvictedIfaces {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        if kind == EntryKind::Iface && self.evict.load(Ordering::Relaxed) {
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
fn retained_rows_rebuild_an_evicted_canonical_interface() {
    let store = EvictedIfaces::default();
    check(&store, include_str!("programs/lazy_ifaces/main.hd"));
    store.evict.store(true, Ordering::Relaxed);
    let edited = include_str!("programs/lazy_ifaces/edited.hd");
    check(&store, edited);
    assert!(store.misses.load(Ordering::Relaxed) > 0);
    store.evict.store(false, Ordering::Relaxed);
    check(&store, edited);
}
