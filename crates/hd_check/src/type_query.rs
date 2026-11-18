//! Query-local type dependencies. Inference changes invalidate only readers
//! and their ancestors; unchanged shared subgraphs are observed once.
use hd_base::{HashMap, HashSet};
use hd_types::unify::{ClassChange, Snapshot};
use hd_types::{InferTable, Ty, TyData, Types, WalkMemo};
use std::hash::Hash;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node<K> {
    Type(Ty),
    Query(K),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Dependency {
    Class(u32),
    Environment,
}

pub(crate) struct TypeQueries<K, V = bool> {
    walk: WalkMemo<Node<K>, Dependency>,
    answers: HashMap<K, V>,
    cursor: Snapshot,
    environment: usize,
}

impl<K: Copy + Eq + Hash, V: Copy> TypeQueries<K, V> {
    pub(crate) fn new(table: &InferTable, environment: usize) -> Self {
        Self {
            walk: WalkMemo::default(),
            answers: HashMap::default(),
            cursor: table.snapshot(),
            environment,
        }
    }

    pub(crate) fn sync(&mut self, table: &InferTable, environment: usize) {
        for change in table.changes_since(self.cursor) {
            let class = match change {
                ClassChange::Merged { from, .. } => from,
                ClassChange::Value(v) => v,
            };
            self.walk.changed(Dependency::Class(class));
        }
        self.cursor = table.snapshot();
        if environment != self.environment {
            self.walk.changed(Dependency::Environment);
            self.environment = environment;
        }
    }

    pub(crate) fn cached(&mut self, key: K) -> Option<V> {
        if self.walk.enter(Node::Query(key)) {
            None
        } else {
            self.answers.get(&key).copied()
        }
    }

    pub(crate) fn finish(&mut self, key: K, answer: V) {
        self.answers.insert(key, answer);
        self.walk.finish(Node::Query(key));
    }

    pub(crate) fn link(&mut self, child: K, parent: K) {
        self.walk.link(Node::Query(child), Node::Query(parent));
    }

    pub(crate) fn watch_class(&mut self, key: K, class: u32) {
        self.walk.watch(Dependency::Class(class), Node::Query(key));
    }

    pub(crate) fn changed_class(&mut self, class: u32) {
        self.walk.changed(Dependency::Class(class));
    }

    pub(crate) fn environment(&mut self, key: K) {
        self.walk.watch(Dependency::Environment, Node::Query(key));
    }

    pub(crate) fn observe(&mut self, pool: Types<'_>, table: &InferTable, ty: Ty, key: K) {
        if pool.has_assoc(ty) {
            self.environment(key);
        }
        if !pool.has_infer(ty) {
            return;
        }
        self.walk.link(Node::Type(ty), Node::Query(key));
        let mut todo = vec![(ty, false)];
        let mut active = HashSet::default();
        let mut children = Vec::new();
        while let Some((ty, done)) = todo.pop() {
            let node = Node::Type(ty);
            if done {
                active.remove(&ty);
                self.walk.finish(node);
                continue;
            }
            if active.contains(&ty) || !self.walk.enter(node) {
                continue;
            }
            hd_types::work::type_node();
            active.insert(ty);
            todo.push((ty, true));
            if let Some(class) = table.class_of(pool, ty) {
                self.walk.watch(Dependency::Class(class), node);
            }
            let resolved = table.shallow(pool, ty);
            children.clear();
            if resolved != ty {
                children.push(resolved);
            } else if let TyData::Fn {
                params,
                result,
                row,
                ..
            } = pool.get(ty)
            {
                children.extend_from_slice(pool.list_items(params));
                children.push(result);
                children.push(pool.intern_ty(&TyData::Row(row)));
            } else {
                pool.children(ty, &mut children);
            }
            for &child in children.iter().rev() {
                if pool.has_infer(child) {
                    self.walk.link(Node::Type(child), node);
                    todo.push((child, false));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_types::{InternPool, LocalPool, VarKind};

    #[test]
    fn query_dependencies_ignore_unrelated_bindings_and_follow_aliases() {
        for width in [100, 1_000] {
            let global = InternPool::new();
            let local = LocalPool::new();
            let p = Types::with_local(&global, &local);
            let mut table = InferTable::default();
            let leaf = table.fresh(p, VarKind::General);
            let mut ty = leaf;
            for _ in 0..20 {
                ty = p.intern_ty(&TyData::Tuple {
                    elems: p.list(&[ty, ty]),
                    rest: None,
                });
            }
            let mut memo = TypeQueries::new(&table, 0);
            assert_eq!(memo.cached(0), None);
            memo.observe(p, &table, ty, 0);
            memo.finish(0, false);
            let ((), work) = hd_types::work::count_type_nodes(|| {
                for _ in 0..width {
                    let other = table.fresh(p, VarKind::General);
                    table.unify(p, other, Ty::I32).unwrap();
                    memo.sync(&table, 0);
                    assert_eq!(memo.cached(0), Some(false));
                }
            });
            assert_eq!(
                work, 0,
                "unrelated inference changes do not walk cached queries"
            );
            let alias = table.fresh(p, VarKind::General);
            table.unify(p, leaf, alias).unwrap();
            memo.sync(&table, 0);
            // Merging can preserve the watched representative; either answer is
            // valid until its value changes. Reobserve if it was invalidated.
            let _ = memo.cached(0);
            memo.observe(p, &table, ty, 0);
            memo.finish(0, false);
            table.unify(p, alias, Ty::BOOL).unwrap();
            memo.sync(&table, 0);
            assert_eq!(
                memo.cached(0),
                None,
                "binding an alias invalidates its readers"
            );
            memo.observe(p, &table, ty, 0);
            memo.finish(0, true);
            memo.environment(0);
            memo.sync(&table, 1);
            assert_eq!(
                memo.cached(0),
                None,
                "environment readers follow clause changes"
            );
        }
    }
}
