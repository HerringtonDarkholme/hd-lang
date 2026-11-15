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
    rejected_parse: bool,
}

impl CacheStore for Claims {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        let found = self.inner.get(kind, key);
        if self.rejected_parse {
            let pending = self.pending.lock().unwrap();
            assert!(
                pending
                    .get(&std::thread::current().id())
                    .is_none_or(|old| { kind == EntryKind::Parse && old == key }),
                "another query while a rejected parse's claim is held"
            );
        }
        let claimed = if self.rejected_parse {
            EntryKind::Parse
        } else {
            EntryKind::Check
        };
        if kind == claimed && found.is_none() {
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
        let claimed = if self.rejected_parse {
            EntryKind::Parse
        } else {
            EntryKind::Check
        };
        if kind == claimed {
            self.pending
                .lock()
                .unwrap()
                .retain(|_, pending| pending != key);
        }
    }

    fn abandon(&self, kind: EntryKind, key: &Hash128) {
        if self.rejected_parse && kind == EntryKind::Parse {
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

/// A rejected source cannot publish the clean-parse marker. Its failed
/// query ends before std skims, another parse, or an interface is read.
#[test]
fn a_rejected_parse_abandons_its_claim_before_the_next_query() {
    for executor in [
        Executor::Serial(SerialOrder::Fifo),
        Executor::Serial(SerialOrder::Priority),
        Executor::Serial(SerialOrder::Shuffled(1981)),
        Executor::Pool(2),
    ] {
        let mut sources = MemorySources::default();
        sources.insert(
            "main.hd",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../spec/conformance/cli/check-fix-elif-continuations/src/lib.hd"
            )),
        );
        let store = Claims {
            rejected_parse: true,
            ..Claims::default()
        };
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor,
        };
        let out = build(&host, "app", &Goal::Analyze);
        assert!(out.diags.has_errors(), "the rejected-input path must run");
        assert!(
            store.pending.lock().unwrap().is_empty(),
            "no unfinished parse claim"
        );
    }
}
