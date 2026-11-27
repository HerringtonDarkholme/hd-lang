//! Persistent integer-keyed maps: the big-endian Patricia union used by
//! containers' `IntMap` (Okasaki/Gill), with pointer-equal subtrees skipped.
//! Union combiners are idempotent: merging shared roots contributes no
//! new information, so pointer-equal nodes need no callback.
//! The liveness arena uses pass-local u32 handles and Cell; these Sync
//! u128 payload roots cannot share it without a locking/allocator layer.
//! Keys pack compiler IDs and stable ordering positions in u128. Lookup
//! is iterative; union/drop depth is bounded by 128 key bits (at most one
//! operand swap per level), independent of source/dependency nesting.

use std::sync::Arc;

pub struct Map<V>(Option<Arc<Node<V>>>);

pub enum Merged<V> {
    Left,
    Right,
    Value(V),
}

enum Node<V> {
    Leaf(u128, V),
    Branch(Branch<V>),
}
struct Branch<V> {
    prefix: u128,
    mask: u128,
    left: Arc<Node<V>>,
    right: Arc<Node<V>>,
}

impl<V> Clone for Map<V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<V> Default for Map<V> {
    fn default() -> Self {
        Self(None)
    }
}
impl<V> Node<V> {
    fn key(&self) -> u128 {
        match self {
            Self::Leaf(k, _) => *k,
            Self::Branch(node) => node.prefix,
        }
    }
    fn mask(&self) -> u128 {
        match self {
            Self::Leaf(..) => 0,
            Self::Branch(node) => node.mask,
        }
    }
}
fn prefix(key: u128, mask: u128) -> u128 {
    key & !(mask | (mask - 1))
}
fn branch<V>(p: u128, m: u128, left: Arc<Node<V>>, right: Arc<Node<V>>) -> Arc<Node<V>> {
    Arc::new(Node::Branch(Branch {
        prefix: p,
        mask: m,
        left,
        right,
    }))
}
fn merge<V>(
    a: &Arc<Node<V>>,
    b: &Arc<Node<V>>,
    combine: &dyn Fn(&V, &V) -> Merged<V>,
) -> Arc<Node<V>> {
    crate::work::context_map_step();
    if Arc::ptr_eq(a, b) {
        return Arc::clone(a);
    }
    let (pa, ma, pb, mb) = (a.key(), a.mask(), b.key(), b.mask());
    match (a.as_ref(), b.as_ref()) {
        (Node::Leaf(k, x), Node::Leaf(_, y)) if pa == pb => match combine(x, y) {
            Merged::Left => Arc::clone(a),
            Merged::Right => Arc::clone(b),
            Merged::Value(value) => Arc::new(Node::Leaf(*k, value)),
        },
        (Node::Branch(x), Node::Branch(y)) if ma == mb && pa == pb => {
            let left = merge(&x.left, &y.left, combine);
            let right = merge(&x.right, &y.right, combine);
            if Arc::ptr_eq(&left, &x.left) && Arc::ptr_eq(&right, &x.right) {
                Arc::clone(a)
            } else if Arc::ptr_eq(&left, &y.left) && Arc::ptr_eq(&right, &y.right) {
                Arc::clone(b)
            } else {
                branch(pa, ma, left, right)
            }
        }
        (Node::Branch(x), _) if ma > mb && prefix(pb, ma) == pa => {
            if pb & ma == 0 {
                let next = merge(&x.left, b, combine);
                if Arc::ptr_eq(&next, &x.left) {
                    Arc::clone(a)
                } else {
                    branch(pa, ma, next, Arc::clone(&x.right))
                }
            } else {
                let next = merge(&x.right, b, combine);
                if Arc::ptr_eq(&next, &x.right) {
                    Arc::clone(a)
                } else {
                    branch(pa, ma, Arc::clone(&x.left), next)
                }
            }
        }
        (_, Node::Branch(_)) if mb > ma && prefix(pa, mb) == pb => {
            merge(b, a, &|y, x| match combine(x, y) {
                Merged::Left => Merged::Right,
                Merged::Right => Merged::Left,
                Merged::Value(value) => Merged::Value(value),
            })
        }
        _ => {
            let different = pa ^ pb;
            // ruled out: equal or contained prefixes matched above.
            debug_assert_ne!(different, 0);
            let m = 1 << (u128::BITS - 1 - different.leading_zeros());
            let p = prefix(pa, m);
            if pa & m == 0 {
                branch(p, m, Arc::clone(a), Arc::clone(b))
            } else {
                branch(p, m, Arc::clone(b), Arc::clone(a))
            }
        }
    }
}
impl<V> Map<V> {
    /// Identity while a clone of this immutable root is retained.
    #[must_use]
    pub fn identity(&self) -> Option<usize> {
        self.0.as_ref().map(|node| Arc::as_ptr(node).addr())
    }
    pub fn into_entries(self) -> impl Iterator<Item = (u128, V)>
    where
        V: Clone,
    {
        let mut stack: Vec<_> = self.0.into_iter().collect();
        std::iter::from_fn(move || {
            while let Some(node) = stack.pop() {
                match node.as_ref() {
                    Node::Leaf(k, value) => return Some((*k, value.clone())),
                    Node::Branch(n) => {
                        stack.push(Arc::clone(&n.right));
                        stack.push(Arc::clone(&n.left));
                    }
                }
            }
            None
        })
    }
    #[must_use]
    pub fn get(&self, key: u128) -> Option<&V> {
        let mut node = self.0.as_deref()?;
        loop {
            match node {
                Node::Leaf(k, value) => return (*k == key).then_some(value),
                Node::Branch(n) => {
                    if prefix(key, n.mask) != n.prefix {
                        return None;
                    }
                    node = if key & n.mask == 0 { &n.left } else { &n.right };
                }
            }
        }
    }
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    #[must_use]
    pub fn min_union(&self, other: &Self, rank: impl Fn(&V) -> u128) -> Self {
        self.union_by(other, |a, b| {
            if rank(a) <= rank(b) {
                Merged::Left
            } else {
                Merged::Right
            }
        })
    }
    #[must_use]
    pub fn left_union(&self, other: &Self) -> Self {
        self.union_by(other, |_, _| Merged::Left)
    }
    #[must_use]
    pub fn union_by(&self, other: &Self, combine: impl Fn(&V, &V) -> Merged<V>) -> Self {
        Self(match (&self.0, &other.0) {
            (Some(a), Some(b)) => Some(merge(a, b, &combine)),
            (a, None) | (None, a) => a.clone(),
        })
    }
    pub fn insert(&mut self, key: u128, value: V) {
        *self = self.union_by(&Self(Some(Arc::new(Node::Leaf(key, value)))), |_, _| {
            Merged::Right
        });
    }
    /// The subtree sharing `key` above its `low_bits` suffix.
    #[must_use]
    pub fn prefix(&self, key: u128, low_bits: u32) -> Iter<'_, V> {
        let high = u128::MAX.checked_shl(low_bits).unwrap_or(0);
        let mut node = self.0.as_deref();
        while let Some(Node::Branch(n)) = node {
            if n.mask & high == 0 {
                break;
            }
            node = Some(if key & n.mask == 0 { &n.left } else { &n.right });
        }
        Iter {
            stack: node
                .filter(|n| n.key() & high == key & high)
                .into_iter()
                .collect(),
        }
    }
    #[must_use]
    pub fn iter(&self) -> Iter<'_, V> {
        Iter {
            stack: self.0.as_deref().into_iter().collect(),
        }
    }
}
pub struct Iter<'a, V> {
    stack: Vec<&'a Node<V>>,
}
impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (u128, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.stack.pop() {
            match node {
                Node::Leaf(k, v) => return Some((*k, v)),
                Node::Branch(n) => {
                    self.stack.push(&n.right);
                    self.stack.push(&n.left);
                }
            }
        }
        None
    }
}

impl<V> Map<Map<V>> {
    /// Union nested indexes while reusing the unchanged outer leaves.
    #[must_use]
    pub fn nested_union(&self, other: &Self) -> Self {
        self.union_by(other, |a, b| {
            let value = a.left_union(b);
            if value.ptr_eq(a) {
                Merged::Left
            } else if value.ptr_eq(b) {
                Merged::Right
            } else {
                Merged::Value(value)
            }
        })
    }
}
impl<V: std::fmt::Debug> std::fmt::Debug for Map<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'a, V> IntoIterator for &'a Map<V> {
    type Item = (u128, &'a V);
    type IntoIter = Iter<'a, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
