//! Completed graph walks with watched type dependencies. Changes dirty only
//! affected ancestors, including active walks whose earlier child became stale.
use crate::Ty;
use hd_base::{HashMap, HashSet};
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct WalkMemo<K, D = Ty> {
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
    #[must_use]
    pub fn active(&self) -> bool {
        self.indexed
    }
    pub fn enter(&mut self, key: K) -> bool {
        if self.done.contains(&key) {
            return false;
        }
        self.dirty.remove(&key);
        true
    }
    pub fn finish(&mut self, key: K) {
        if !self.dirty.contains(&key) {
            self.done.insert(key);
        }
    }
    pub fn link(&mut self, child: K, parent: K) {
        if self.indexed {
            self.parents.entry(child).or_default().insert(parent);
        } else {
            self.pending.push((child, parent));
        }
    }
    pub fn watch(&mut self, dependency: D, key: K) {
        if !self.indexed {
            for (child, parent) in self.pending.drain(..) {
                self.parents.entry(child).or_default().insert(parent);
            }
            self.indexed = true;
        }
        self.watches.entry(dependency).or_default().insert(key);
    }
    pub fn unwatch(&mut self, dependency: D, key: K) {
        if let Some(keys) = self.watches.get_mut(&dependency) {
            keys.remove(&key);
        }
    }
    pub fn changed(&mut self, dependency: D) {
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
    pub fn forget(&mut self, key: K) {
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
