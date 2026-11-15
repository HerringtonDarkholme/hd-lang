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
            // The driver runs every task inside `hd_cache::task`, so a
            // claim taken in a task nested in another's join is told
            // from one its own task left (#2018).
            assert!(hd_cache::task_depth() >= 1, "a claimed miss outside a task");
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

/// A store that claims every `check`, `iface` and `graph` miss, and whose
/// put of `stop`'s kind fails: the computation that missed it ends
/// without its put, as an internal stop ends it (#2021).
struct Stops {
    inner: MemoryStore,
    held: Mutex<Vec<(EntryKind, Hash128)>>,
    stop: Option<EntryKind>,
}

impl CacheStore for Stops {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        let found = self.inner.get(kind, key);
        if found.is_none() && matches!(kind, EntryKind::Check | EntryKind::Iface | EntryKind::Graph)
        {
            self.held.lock().unwrap().push((kind, *key));
        }
        found
    }

    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        assert!(
            Some(kind) != self.stop,
            "the computation stops before its put"
        );
        self.inner.put(kind, key, bytes);
        self.held.lock().unwrap().retain(|k| *k != (kind, *key));
    }

    fn abandon(&self, kind: EntryKind, key: &Hash128) {
        self.held.lock().unwrap().retain(|k| *k != (kind, *key));
    }

    fn touch(&self, kind: EntryKind, key: &Hash128) {
        self.inner.touch(kind, key);
    }
}

/// Each claimed kind's computation ends its claim on every exit, not
/// only at its put: a `check` miss (`module_prep` through
/// `module_finish`), an `iface` miss (`folder_iface`, the read list and
/// the interface) and a `graph` miss (a folder's header findings, the
/// coherence rows, the package's parts) that stop before their puts
/// leave no claim held, and a build that finishes leaves none either.
#[test]
fn a_computation_that_stops_before_its_put_ends_its_claim() {
    for stop in [
        None,
        Some(EntryKind::Check),
        Some(EntryKind::Iface),
        Some(EntryKind::Graph),
    ] {
        let mut sources = MemorySources::default();
        sources.insert(
            "main.hd",
            "use util\nstruct P:\n    x: i32\nimpl P: Eq\npub fn main():\n    let x = util.one()\n",
        );
        sources.insert("util/util.hd", "pub fn one() -> i32: 1\n");
        let store = Stops {
            inner: MemoryStore::default(),
            held: Mutex::default(),
            stop,
        };
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Fifo),
        };
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build(&host, "app", &Goal::Analyze)
        }));
        assert_eq!(
            stopped.is_err(),
            stop.is_some(),
            "{stop:?}: the put must be reached"
        );
        assert_eq!(
            *store.held.lock().unwrap(),
            [],
            "{stop:?}: a stopped computation left its claim"
        );
    }
}
