//! Single-flight misses for concurrent builds over one store (#1423).
//!
//! Builds that share a store and start together miss the same keys at
//! once, such as every conformance thread's first build of a cold std,
//! and each computes every entry. A [`SingleFlight`] store lets one build
//! compute a missing entry while the others wait for it: the first build
//! whose read misses a key owns it, and a later build's read of that key
//! blocks until the owner writes the entry, then reads it. The owner lets
//! go of every key it has not written when its [`FlightRun`] drops, so a
//! waiter whose owner never writes the entry computes it itself.
//!
//! A build never waits for its own keys, and never waits where waiting
//! would close a cycle: when the owner of the key waits, directly or
//! through other builds, for a key this build owns, the read misses as it
//! would without the store. Only the kinds the store is made with are
//! single-flight: a kind fits when a build writes its entry soon after it
//! misses it, not only when the build ends.
//!
//! Cost: a hit costs what the inner store's read costs. A miss of a
//! single-flight kind takes one lock, reads the inner store again under
//! it and inserts into a map, so a slow inner store would serialize
//! misses; a write takes one lock and a map removal. A waiter's cycle
//! check visits each waiting build once, and each write wakes every
//! waiter to read its key again.

use crate::store::{CacheStore, EntryKind};
use hd_base::{Hash128, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

type Key = (EntryKind, Hash128);

/// A store whose builds share their misses: see the module documentation.
/// Each build reads and writes through its own [`FlightRun`].
pub struct SingleFlight<S: CacheStore> {
    inner: S,
    kinds: Vec<EntryKind>,
    state: Mutex<Flights>,
    landed: Condvar,
    next: AtomicU64,
}

/// Who owns each key in flight, and what each blocked build waits for.
#[derive(Default)]
struct Flights {
    owners: HashMap<Key, u64>,
    /// Per build, the keys its blocked threads wait for: a build on a
    /// thread pool may wait for several at once.
    waits: HashMap<u64, Vec<Key>>,
}

impl Flights {
    /// Whether build `from`, through the owners of the keys it waits for
    /// and what they wait for in turn, waits for build `to`.
    fn waits_for(&self, from: u64, to: u64) -> bool {
        let mut stack = vec![from];
        let mut seen = Vec::new();
        while let Some(b) = stack.pop() {
            if b == to {
                return true;
            }
            if seen.contains(&b) {
                continue;
            }
            seen.push(b);
            for key in self.waits.get(&b).into_iter().flatten() {
                if let Some(&owner) = self.owners.get(key) {
                    stack.push(owner);
                }
            }
        }
        false
    }

    fn stop_waiting(&mut self, build: u64, key: Key) {
        if let Some(keys) = self.waits.get_mut(&build) {
            if let Some(at) = keys.iter().position(|k| *k == key) {
                keys.swap_remove(at);
            }
            if keys.is_empty() {
                self.waits.remove(&build);
            }
        }
    }
}

impl<S: CacheStore> SingleFlight<S> {
    /// `inner`, with the misses of `kinds` single-flight.
    #[must_use]
    pub fn new(inner: S, kinds: &[EntryKind]) -> Self {
        Self {
            inner,
            kinds: kinds.to_vec(),
            state: Mutex::default(),
            landed: Condvar::new(),
            next: AtomicU64::new(0),
        }
    }

    /// The inner store.
    #[must_use]
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// A build's view of the store, which owns the keys the build misses
    /// until it writes them or drops.
    #[must_use]
    pub fn run(&self) -> FlightRun<'_, S> {
        FlightRun {
            store: self,
            id: self.next.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Flights> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Build `id`'s read of `key` after the inner store missed it: the
    /// entry once its owner writes it, or `None` when `id` owns it now.
    fn miss(&self, id: u64, key: Key) -> Option<Arc<[u8]>> {
        let mut flights = self.lock();
        loop {
            // A write between the inner read and this lock has landed.
            if let Some(bytes) = self.inner.get(key.0, &key.1) {
                return Some(bytes);
            }
            let owner = match flights.owners.get(&key) {
                None => {
                    flights.owners.insert(key, id);
                    return None;
                }
                Some(&owner) => owner,
            };
            if owner == id || flights.waits_for(owner, id) {
                return None;
            }
            flights.waits.entry(id).or_default().push(key);
            flights = self
                .landed
                .wait(flights)
                .unwrap_or_else(PoisonError::into_inner);
            flights.stop_waiting(id, key);
        }
    }

    /// Whoever wrote `key`, it is no longer in flight.
    fn wrote(&self, key: Key) {
        let mut flights = self.lock();
        if flights.owners.remove(&key).is_some() && !flights.waits.is_empty() {
            self.landed.notify_all();
        }
    }

    /// Build `id` ended: its waiters compute what it did not write.
    fn release(&self, id: u64) {
        let mut flights = self.lock();
        let before = flights.owners.len();
        flights.owners.retain(|_, owner| *owner != id);
        if flights.owners.len() != before && !flights.waits.is_empty() {
            self.landed.notify_all();
        }
    }
}

/// One build's reads and writes through a [`SingleFlight`] store.
pub struct FlightRun<'a, S: CacheStore> {
    store: &'a SingleFlight<S>,
    id: u64,
}

impl<S: CacheStore> Drop for FlightRun<'_, S> {
    fn drop(&mut self) {
        self.store.release(self.id);
    }
}

impl<S: CacheStore> CacheStore for FlightRun<'_, S> {
    fn get(&self, kind: EntryKind, key: &Hash128) -> Option<Arc<[u8]>> {
        if let Some(bytes) = self.store.inner.get(kind, key) {
            return Some(bytes);
        }
        if !self.store.kinds.contains(&kind) {
            return None;
        }
        self.store.miss(self.id, (kind, *key))
    }
    fn put(&self, kind: EntryKind, key: &Hash128, bytes: &[u8]) {
        self.store.inner.put(kind, key, bytes);
        if self.store.kinds.contains(&kind) {
            self.store.wrote((kind, *key));
        }
    }
    fn touch(&self, kind: EntryKind, key: &Hash128) {
        self.store.inner.touch(kind, key);
    }
    fn put_many(&self, entries: &[(EntryKind, Hash128, Arc<[u8]>)]) {
        self.store.inner.put_many(entries);
        for (kind, key, _) in entries {
            if self.store.kinds.contains(kind) {
                self.store.wrote((*kind, *key));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;
    use std::sync::mpsc;
    use std::time::Duration;

    const KEY: Hash128 = Hash128(7);

    fn store() -> SingleFlight<MemoryStore> {
        SingleFlight::new(MemoryStore::default(), &[EntryKind::Check])
    }

    #[test]
    fn a_second_build_waits_for_the_first_ones_write() {
        let store = store();
        let first = store.run();
        assert!(first.get(EntryKind::Check, &KEY).is_none());
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            // cost: one thread for the test, joined by the scope.
            s.spawn(|| {
                let second = store.run();
                tx.send(second.get(EntryKind::Check, &KEY)).expect("send");
            });
            // The second build blocks while the first owns the key.
            assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
            first.put(EntryKind::Check, &KEY, b"entry");
            let got = rx.recv().expect("recv").expect("the first build's entry");
            assert_eq!(&*got, b"entry");
        });
    }

    #[test]
    fn a_dropped_owner_hands_its_key_to_a_waiter() {
        let store = store();
        let first = store.run();
        assert!(first.get(EntryKind::Check, &KEY).is_none());
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            // cost: one thread for the test, joined by the scope.
            s.spawn(|| {
                let second = store.run();
                tx.send(second.get(EntryKind::Check, &KEY).is_none())
                    .expect("send");
                // The waiter owns the key now: a third build would wait.
                assert_eq!(store.lock().owners.get(&(EntryKind::Check, KEY)), Some(&1));
            });
            assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
            drop(first);
            assert!(rx.recv().expect("recv"), "a miss, to compute itself");
        });
    }

    #[test]
    fn a_wait_that_would_close_a_cycle_misses() {
        let store = store();
        let (a, b) = (store.run(), store.run());
        let (ka, kb) = (Hash128(1), Hash128(2));
        assert!(a.get(EntryKind::Check, &ka).is_none());
        assert!(b.get(EntryKind::Check, &kb).is_none());
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|s| {
            // cost: one thread for the test, joined by the scope.
            s.spawn(|| {
                // `a` waits for `b`'s key.
                tx.send(a.get(EntryKind::Check, &kb)).expect("send");
            });
            while store.lock().waits.is_empty() {
                std::thread::yield_now();
            }
            // `b` waiting for `a`'s key would deadlock: it misses.
            assert!(b.get(EntryKind::Check, &ka).is_none());
            b.put(EntryKind::Check, &kb, b"b");
            assert_eq!(&*rx.recv().expect("recv").expect("b's entry"), b"b");
        });
    }

    #[test]
    fn other_kinds_and_own_keys_never_wait() {
        let store = store();
        let first = store.run();
        assert!(first.get(EntryKind::Check, &KEY).is_none());
        assert!(first.get(EntryKind::Check, &KEY).is_none(), "its own key");
        let second = store.run();
        assert!(second.get(EntryKind::Depfiles, &KEY).is_none());
        assert!(second.get(EntryKind::Depfiles, &KEY).is_none());
    }
}
