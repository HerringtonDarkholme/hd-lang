//! A folder's `iface` entry is written before the read list that keys it,
//! so a concurrent build that waits for the list (`hd_cache::SingleFlight`)
//! and then reads it finds the entry too, instead of building the folder
//! again (#1423).

use std::sync::{Arc, Mutex};

use hd_base::Hash128;
use hd_cache::{CacheStore, EntryKind, MemoryStore, decode_entry};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// A memory store that records the section count of each `iface` write.
#[derive(Default)]
struct Recording {
    inner: MemoryStore,
    iface_puts: Mutex<Vec<usize>>,
}

impl CacheStore for Recording {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        self.inner.get(kind, key)
    }
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        if kind == EntryKind::Iface {
            let (_, sections) = decode_entry(bytes).expect("entry");
            self.iface_puts.lock().expect("puts").push(sections.len());
        }
        self.inner.put(kind, key, bytes);
    }
    fn touch(&self, kind: EntryKind, key: &Hash128) {
        self.inner.touch(kind, key);
    }
}

#[test]
fn a_folder_writes_its_interface_before_the_read_list_that_keys_it() {
    let mut src = MemorySources::default();
    src.insert(
        "main.hd",
        "pub fn main() -> void $ Console:\n    println(\"hi\")\n",
    );
    let store = Recording::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    assert!(out.diags.is_empty(), "{}", out.render());
    let puts = store.iface_puts.lock().expect("puts");
    // A read list is one section; an interface is several.
    let lists = puts.iter().filter(|n| **n == 1).count();
    assert!(lists > 0, "{puts:?}");
    let mut interfaces = 0;
    for n in puts.iter() {
        if *n == 1 {
            assert!(interfaces > 0, "a read list before its interface: {puts:?}");
            interfaces -= 1;
        } else {
            interfaces += 1;
        }
    }
}
