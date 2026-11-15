//! Claim lifetime follows one computation, not an OS worker across tasks.
//! A store makes a second unrelated Check miss fail deterministically
//! while this worker still owes a previous Check's publication (#1981).

use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use hd_base::{Hash128, HashMap};
use hd_cache::{CacheStore, EntryKind, MemoryStore};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

#[derive(Default)]
struct Claims {
    inner: MemoryStore,
    pending: Mutex<HashMap<ThreadId, Hash128>>,
}

impl CacheStore for Claims {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        let found = self.inner.get(kind, key);
        if kind == EntryKind::Check && found.is_none() {
            let previous = self
                .pending
                .lock()
                .unwrap()
                .insert(std::thread::current().id(), *key);
            assert!(
                previous.is_none_or(|previous| previous == *key),
                "worker waits on an unrelated check before publishing {previous:?}"
            );
        }
        found
    }

    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        self.inner.put(kind, key, bytes);
        if kind == EntryKind::Check {
            self.pending
                .lock()
                .unwrap()
                .retain(|_, pending| pending != key);
        }
    }

    fn touch(&self, kind: EntryKind, key: &Hash128) {
        self.inner.touch(kind, key);
    }
}

#[test]
fn a_worker_publishes_its_check_before_waiting_for_an_unrelated_module() {
    for executor in [
        Executor::Serial(SerialOrder::Fifo),
        Executor::Serial(SerialOrder::Priority),
        Executor::Serial(SerialOrder::Shuffled(1981)),
        Executor::Pool(2),
    ] {
        let mut sources = MemorySources::default();
        sources.insert(
            "main.hd",
            "pub fn main() -> void $ Console:\n    println(\"hi\")\n",
        );
        let store = Claims::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor,
        };
        let out = build(
            &host,
            "app",
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(out.wasm.is_some(), "{}", out.render());
        assert!(store.pending.lock().unwrap().is_empty());
    }
}
