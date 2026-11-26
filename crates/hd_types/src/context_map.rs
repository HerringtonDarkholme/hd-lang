//! Persistent integer-keyed maps: the big-endian Patricia union used by
//! containers' `IntMap` (Okasaki/Gill), with pointer-equal subtrees skipped.
//! Union combiners are idempotent: merging shared roots contributes no
//! new information, so pointer-equal nodes need no callback.
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
    Branch {
        prefix: u128,
        mask: u128,
        left: Arc<Node<V>>,
        right: Arc<Node<V>>,
        len: usize,
    },
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
            Self::Branch { prefix, .. } => *prefix,
        }
    }
    fn mask(&self) -> u128 {
        match self {
            Self::Leaf(..) => 0,
            Self::Branch { mask, .. } => *mask,
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Leaf(..) => 1,
            Self::Branch { len, .. } => *len,
        }
    }
}
fn prefix(key: u128, mask: u128) -> u128 {
    key & !(mask | (mask - 1))
}
fn branch<V>(p: u128, m: u128, left: Arc<Node<V>>, right: Arc<Node<V>>) -> Arc<Node<V>> {
    let len = left.len() + right.len();
    Arc::new(Node::Branch {
        prefix: p,
        mask: m,
        left,
        right,
        len,
    })
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
        (
            Node::Branch {
                left: al,
                right: ar,
                ..
            },
            Node::Branch {
                left: bl,
                right: br,
                ..
            },
        ) if ma == mb && pa == pb => {
            let left = merge(al, bl, combine);
            let right = merge(ar, br, combine);
            if Arc::ptr_eq(&left, al) && Arc::ptr_eq(&right, ar) {
                Arc::clone(a)
            } else if Arc::ptr_eq(&left, bl) && Arc::ptr_eq(&right, br) {
                Arc::clone(b)
            } else {
                branch(pa, ma, left, right)
            }
        }
        (Node::Branch { left, right, .. }, _) if ma > mb && prefix(pb, ma) == pa => {
            if pb & ma == 0 {
                let next = merge(left, b, combine);
                if Arc::ptr_eq(&next, left) {
                    Arc::clone(a)
                } else {
                    branch(pa, ma, next, Arc::clone(right))
                }
            } else {
                let next = merge(right, b, combine);
                if Arc::ptr_eq(&next, right) {
                    Arc::clone(a)
                } else {
                    branch(pa, ma, Arc::clone(left), next)
                }
            }
        }
        (_, Node::Branch { .. }) if mb > ma && prefix(pa, mb) == pb => {
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
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |n| n.len())
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
    #[must_use]
    pub fn get(&self, key: u128) -> Option<&V> {
        let mut node = self.0.as_deref()?;
        loop {
            match node {
                Node::Leaf(k, value) => return (*k == key).then_some(value),
                Node::Branch {
                    prefix: p,
                    mask,
                    left,
                    right,
                    ..
                } => {
                    if prefix(key, *mask) != *p {
                        return None;
                    }
                    node = if key & mask == 0 { left } else { right };
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
    pub fn union(&self, other: &Self, combine: impl Fn(&V, &V) -> V) -> Self {
        self.union_by(other, |a, b| Merged::Value(combine(a, b)))
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
    pub fn insert(&mut self, key: u128, value: V)
    where
        V: Clone,
    {
        *self = self.union(&Self(Some(Arc::new(Node::Leaf(key, value)))), |_, b| {
            b.clone()
        });
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
                Node::Branch { left, right, .. } => {
                    self.stack.push(right);
                    self.stack.push(left);
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
