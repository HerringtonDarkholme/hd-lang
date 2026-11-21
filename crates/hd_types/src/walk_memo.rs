//! Completed graph walks with watched type dependencies. Changes dirty only
//! affected ancestors, including active walks whose earlier child became stale.
use crate::Ty;
use hd_base::{HashMap, HashSet};
use std::hash::Hash;

#[derive(Clone, Debug)]
pub(crate) struct WalkMemo<K, D = Ty> {
    done: HashSet<K>,
    dirty: HashSet<K>,
    parents: HashMap<K, HashSet<K>>,
    pending: Vec<(K, K)>,
    watches: HashMap<D, HashSet<K>>,
    indexed: bool,
}

impl<K, D> Default for WalkMemo<K, D> {
    fn default() -> Self {
        Self {
            done: HashSet::default(),
            dirty: HashSet::default(),
            parents: HashMap::default(),
            pending: Vec::new(),
            watches: HashMap::default(),
            indexed: false,
        }
    }
}

impl<K: Copy + Eq + Hash, D: Copy + Eq + Hash> WalkMemo<K, D> {
    pub(crate) fn active(&self) -> bool {
        self.indexed
    }
    pub(crate) fn enter(&mut self, key: K) -> bool {
        if self.done.contains(&key) {
            return false;
        }
        self.dirty.remove(&key);
        true
    }
    pub(crate) fn finish(&mut self, key: K) {
        if !self.dirty.contains(&key) {
            self.done.insert(key);
        }
    }
    pub(crate) fn link(&mut self, child: K, parent: K) {
        if self.indexed {
            self.parents.entry(child).or_default().insert(parent);
        } else {
            self.pending.push((child, parent));
        }
    }
    pub(crate) fn watch(&mut self, dependency: D, key: K) {
        if !self.indexed {
            for (child, parent) in self.pending.drain(..) {
                self.parents.entry(child).or_default().insert(parent);
            }
            self.indexed = true;
        }
        self.watches.entry(dependency).or_default().insert(key);
    }
    pub(crate) fn unwatch(&mut self, dependency: D, key: K) {
        if let Some(keys) = self.watches.get_mut(&dependency) {
            keys.remove(&key);
        }
    }
    pub(crate) fn changed(&mut self, dependency: D) {
        // A watcher must run again before it can be invalidated again.
        // Consuming these edges avoids scanning already-dirty readers on
        // every subsequent clause addition or inference binding.
        let todo = self
            .watches
            .remove(&dependency)
            .into_iter()
            .flatten()
            .collect();
        self.invalidate(todo);
    }
    pub(crate) fn forget(&mut self, key: K) {
        self.invalidate(vec![key]);
    }
    fn invalidate(&mut self, mut todo: Vec<K>) {
        while let Some(key) = todo.pop() {
            if self.dirty.insert(key) {
                crate::work::type_node();
                self.done.remove(&key);
                todo.extend(
                    self.parents
                        .get(&key)
                        .into_iter()
                        .flat_map(|ps| ps.iter())
                        .copied(),
                );
            }
        }
    }
}

#[cfg(test)]
mod coverage_contracts {
    use super::WalkMemo;

    #[test]
    fn cancelling_one_watch_keeps_other_readers_and_their_parents() {
        let mut memo = WalkMemo::<u32, u32>::default();
        for key in [1, 2, 10, 20] {
            memo.finish(key);
        }
        memo.link(1, 10);
        memo.link(2, 20);
        memo.watch(7, 1);
        memo.watch(7, 2);
        memo.unwatch(7, 1);
        let ((), work) = crate::work::count_type_nodes(|| memo.changed(7));
        assert_eq!(work, 2, "only reader 2 and its parent become dirty");
        assert!(!memo.enter(1));
        assert!(!memo.enter(10));
        assert!(memo.enter(2));
        assert!(memo.enter(20));
    }

    #[test]
    fn a_cancelled_watch_can_be_registered_again_without_stale_invalidation() {
        let mut memo = WalkMemo::<u32, u32>::default();
        memo.finish(1);
        memo.finish(10);
        memo.link(1, 10);
        memo.unwatch(7, 1); // cancelling an absent subscription is inert
        memo.watch(7, 1);
        memo.unwatch(7, 99); // a different key does not remove this reader
        memo.unwatch(7, 1);
        let ((), cancelled) = crate::work::count_type_nodes(|| memo.changed(7));
        assert_eq!(cancelled, 0);
        assert!(!memo.enter(1));
        assert!(!memo.enter(10));
        memo.watch(7, 1);
        let ((), registered) = crate::work::count_type_nodes(|| memo.changed(7));
        assert_eq!(registered, 2);
        assert!(memo.enter(1));
        assert!(memo.enter(10));
        memo.finish(1);
        memo.finish(10);
        let ((), consumed) = crate::work::count_type_nodes(|| memo.changed(7));
        assert_eq!(consumed, 0, "a change consumes the subscription");
        assert!(!memo.enter(1));
        assert!(!memo.enter(10));
    }
}
