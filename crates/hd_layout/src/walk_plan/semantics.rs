//! Semantic keys for recipes, independent of sharing and mapping factorization.
use super::{Arc, Data, Hash, HashMap, Hasher, Link, Mapping, Node, Root, View, lock};
use std::cmp::Ordering;
use std::fmt;
use std::num::Wrapping;

/// A composable bucket fingerprint, never an equality proof. Value indices
/// enter linearly so adding an offset changes a cached fingerprint in O(1).
/// Structural tags, arities and discriminant encodings enter the constant term.
#[derive(Clone, Copy)]
pub(super) struct Signature {
    value: Wrapping<u64>,
    coefficient: Wrapping<u64>,
}
impl Signature {
    fn literal(value: u64) -> Self {
        Self {
            value: Wrapping(value),
            coefficient: Wrapping(0),
        }
    }
    fn index(value: u32) -> Self {
        Self {
            value: Wrapping(u64::from(value)),
            coefficient: Wrapping(1),
        }
    }
    fn then(self, next: Self) -> Self {
        // An odd multiplier keeps all 64 bits available under wrapping
        // arithmetic. Collisions only select a bucket; Eq checks the recipe.
        const BASE: Wrapping<u64> = Wrapping(0x9e37_79b1_85eb_ca87);
        Self {
            value: self.value * BASE + next.value,
            coefficient: self.coefficient * BASE + next.coefficient,
        }
    }
    fn shifted(self, by: u32) -> Self {
        Self {
            value: self.value + self.coefficient * Wrapping(u64::from(by)),
            ..self
        }
    }
}
pub(crate) fn hash_scalar<H: Hasher>(tag: u64, index: Option<u32>, state: &mut H) {
    let mut sig = Signature::literal(tag);
    if let Some(index) = index {
        sig = sig.then(Signature::index(index));
    }
    sig.value.0.hash(state);
}
impl Data {
    fn unshifted(&mut self, link: Link) -> (Link, u32) {
        let (map, after) = match &self.maps[link.map] {
            Mapping::Shift(after) => (Mapping::Shift(0), *after),
            Mapping::Select {
                slots,
                start,
                after,
            } => (
                Mapping::Select {
                    slots: slots.clone(),
                    start: *start,
                    after: 0,
                },
                *after,
            ),
        };
        (
            Link {
                node: link.node,
                map: self.mapping(map),
            },
            after,
        )
    }
}
impl Root {
    fn identity(&self) -> (usize, Link) {
        (Arc::as_ptr(&self.arena.0) as usize, self.link)
    }
    fn signature(&self) -> Signature {
        enum Task {
            Enter(Link),
            Exit(Link, Vec<(Link, u32)>),
        }
        let mut data = lock(&self.arena.0.data);
        let (base, shift) = data.unshifted(self.link);
        let mut todo = vec![Task::Enter(base)];
        while let Some(task) = todo.pop() {
            match task {
                Task::Enter(link) => {
                    if data.signatures.contains_key(&link) {
                        continue;
                    }
                    crate::work::walk_plans(1);
                    let node = data.nodes[link.node].clone();
                    let mut child = |edge: Link| {
                        let map = data.compose(link.map, edge.map);
                        data.unshifted(Link {
                            node: edge.node,
                            map,
                        })
                    };
                    let children: Vec<_> = match &node.node {
                        Node::Seq(edges) => edges.iter().map(|&e| child(e)).collect(),
                        Node::Switch(_, arms) => arms.iter().flatten().map(|&e| child(e)).collect(),
                        Node::Ref(_) | Node::Known(_) | Node::Glue => Vec::new(),
                    };
                    todo.push(Task::Exit(link, children.clone()));
                    todo.extend(children.into_iter().rev().map(|(c, _)| Task::Enter(c)));
                }
                Task::Exit(link, children) => {
                    let node = &data.nodes[link.node].node;
                    let tag = match node {
                        Node::Ref(_) => 0,
                        Node::Known(_) => 1,
                        Node::Glue => 2,
                        Node::Seq(_) => 3,
                        Node::Switch(..) => 4,
                    };
                    let mut result = Signature::literal(tag);
                    let mut child = children.into_iter();
                    match node {
                        Node::Ref(k) | Node::Known(k) => {
                            result = result.then(Signature::index(data.maps[link.map].apply(*k)));
                        }
                        Node::Glue => {}
                        Node::Seq(edges) => {
                            result = result.then(Signature::literal(edges.len() as u64));
                            for (c, shift) in child {
                                result = result.then(data.signatures[&c].shifted(shift));
                            }
                        }
                        Node::Switch(d, arms) => {
                            let mut hash = std::collections::hash_map::DefaultHasher::new();
                            (d.leaf, d.enc).hash(&mut hash);
                            result = result
                                .then(Signature::index(data.maps[link.map].apply(d.value)))
                                .then(Signature::literal(hash.finish()))
                                .then(Signature::literal(arms.len() as u64));
                            for arm in arms.iter() {
                                result = result.then(Signature::literal(u64::from(arm.is_some())));
                                if arm.is_some() {
                                    // ruled out: children contains precisely the present arms.
                                    if let Some((c, shift)) = child.next() {
                                        result = result.then(data.signatures[&c].shifted(shift));
                                    }
                                }
                            }
                        }
                    }
                    crate::work::walk_plans(match node {
                        Node::Seq(e) => e.len(),
                        Node::Switch(_, a) => a.len(),
                        _ => 0,
                    });
                    data.signatures.insert(link, result);
                }
            }
        }
        data.signatures[&base].shifted(shift)
    }
}
impl Hash for Root {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.signature().value.0.hash(state);
    }
}

/// Union equal subgraphs after visiting their children. Unlike a visited-pair
/// table, equivalence classes also avoid the pair-product of differently shared
/// but equal DAGs. Parent links and rank live on the heap.
#[derive(Default)]
struct Equal {
    ids: HashMap<(usize, Link), usize>,
    parents: Vec<usize>,
    ranks: Vec<u8>,
}
impl Equal {
    fn id(&mut self, root: &Root) -> usize {
        let next = self.parents.len();
        *self.ids.entry(root.identity()).or_insert_with(|| {
            self.parents.push(next);
            self.ranks.push(0);
            next
        })
    }
    fn find(&mut self, mut id: usize) -> usize {
        while self.parents[id] != id {
            self.parents[id] = self.parents[self.parents[id]];
            id = self.parents[id];
        }
        id
    }
    fn union(&mut self, a: usize, b: usize) {
        let (mut a, mut b) = (self.find(a), self.find(b));
        // Pair checked distinct classes before posting this continuation.
        // Child proofs have lower structural height, so cannot join these two.
        if self.ranks[a] < self.ranks[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parents[b] = a;
        if self.ranks[a] == self.ranks[b] {
            self.ranks[a] += 1;
        }
    }
}
impl Ord for Root {
    fn cmp(&self, other: &Self) -> Ordering {
        enum Task {
            Pair(Root, Root),
            Unite(usize, usize),
            Result(Ordering),
        }
        let mut todo = vec![Task::Pair(self.clone(), other.clone())];
        let mut equal = Equal::default();
        while let Some(task) = todo.pop() {
            match task {
                Task::Result(Ordering::Equal) => {}
                Task::Result(order) => return order,
                Task::Unite(a, b) => equal.union(a, b),
                Task::Pair(mut a, mut b) => {
                    if a.identity() == b.identity() {
                        continue;
                    }
                    // Translation preserves the lexicographic order of all
                    // valid u32 indices. Removing an equal outer translation
                    // lets differently shifted uses share one equality proof.
                    let base = |root: &Root| {
                        let (link, shift) = lock(&root.arena.0.data).unshifted(root.link);
                        (
                            Root {
                                arena: root.arena.clone(),
                                link,
                            },
                            shift,
                        )
                    };
                    let ((aa, sa), (bb, sb)) = (base(&a), base(&b));
                    if sa == sb {
                        a = aa;
                        b = bb;
                    }
                    let (ia, ib) = (equal.id(&a), equal.id(&b));
                    if equal.find(ia) == equal.find(ib) {
                        continue;
                    }
                    crate::work::walk_plans(1);
                    let (a, b) = (a.view(), b.view());
                    let tag = |v: &View| match v {
                        View::Ref(_) => 0,
                        View::Known(_) => 1,
                        View::Glue => 2,
                        View::Seq(_) => 3,
                        View::Switch(..) => 4,
                    };
                    todo.push(Task::Unite(ia, ib));
                    match (a, b) {
                        (View::Ref(a), View::Ref(b)) | (View::Known(a), View::Known(b)) => {
                            todo.push(Task::Result(a.cmp(&b)));
                        }
                        (View::Seq(a), View::Seq(b)) => {
                            todo.push(Task::Result(a.len().cmp(&b.len())));
                            todo.extend(a.into_iter().zip(b).rev().map(|(a, b)| Task::Pair(a, b)));
                        }
                        (View::Switch(a, aa), View::Switch(b, bb)) => {
                            if a != b {
                                return a.cmp(&b);
                            }
                            todo.push(Task::Result(aa.len().cmp(&bb.len())));
                            todo.extend(aa.into_iter().zip(bb).rev().map(|(a, b)| match (a, b) {
                                (Some(a), Some(b)) => Task::Pair(a, b),
                                (None, None) => Task::Result(Ordering::Equal),
                                (None, Some(_)) => Task::Result(Ordering::Less),
                                (Some(_), None) => Task::Result(Ordering::Greater),
                            }));
                        }
                        (View::Glue, View::Glue) => {}
                        (a, b) => return tag(&a).cmp(&tag(&b)),
                    }
                }
            }
        }
        Ordering::Equal
    }
}
impl PartialOrd for Root {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Root {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Root {}

impl fmt::Debug for Root {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Task {
            Root(Root),
            Text(&'static str),
        }
        let mut todo = vec![Task::Root(self.clone())];
        while let Some(task) = todo.pop() {
            match task {
                Task::Text(text) => f.write_str(text)?,
                Task::Root(root) => match root.view() {
                    View::Ref(k) => write!(f, "Ref({k})")?,
                    View::Known(k) => write!(f, "Known({k})")?,
                    View::Glue => f.write_str("Glue")?,
                    View::Seq(children) => {
                        f.write_str("Seq([")?;
                        todo.push(Task::Text("])"));
                        for (i, child) in children.into_iter().enumerate().rev() {
                            todo.push(Task::Root(child));
                            if i != 0 {
                                todo.push(Task::Text(", "));
                            }
                        }
                    }
                    View::Switch(d, arms) => {
                        write!(f, "Switch({d:?}, [")?;
                        todo.push(Task::Text("])"));
                        for (i, arm) in arms.into_iter().enumerate().rev() {
                            if let Some(root) = arm {
                                todo.push(Task::Text(")"));
                                todo.push(Task::Root(root));
                                todo.push(Task::Text("Some("));
                            } else {
                                todo.push(Task::Text("None"));
                            }
                            if i != 0 {
                                todo.push(Task::Text(", "));
                            }
                        }
                    }
                },
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::Arena;
    use super::super::oracle::Walk;
    use super::*;
    use crate::Val;
    use crate::{Discr, Enc, Leaf};

    fn discr(value: u32) -> Discr {
        Discr {
            value,
            leaf: Leaf {
                val: Val::I32,
                offset: 0,
                bytes: 4,
                signed: false,
            },
            enc: Enc::Direct,
        }
    }
    fn hash(root: &Root) -> u64 {
        let mut state = std::collections::hash_map::DefaultHasher::new();
        root.hash(&mut state);
        state.finish()
    }
    fn old(root: &Root) -> Walk {
        match root.view() {
            View::Ref(k) => Walk::Ref(k),
            View::Known(k) => Walk::Known(k),
            View::Glue => Walk::Glue,
            View::Seq(children) => Walk::Seq(children.iter().map(old).collect()),
            View::Switch(d, arms) => Walk::Switch(
                Box::new(d),
                arms.iter().map(|a| a.as_ref().map(old)).collect(),
            ),
        }
    }
    #[test]
    fn keys_match_tree_order_and_debug_across_arena_and_mapping_factorizations() {
        let mut roots = Vec::new();
        for _ in 0..2 {
            let a = Arena::default();
            for k in 0..4 {
                let r = a.reference(k);
                let s = a.sequence(vec![a.reference(k), a.known(0)]);
                roots.extend([
                    r.clone(),
                    a.known(k),
                    a.glue(),
                    a.sequence(vec![]),
                    s.clone(),
                    a.sequence(vec![r.clone()]),
                    a.switch(discr(k), vec![None, Some(s.clone())]),
                    a.switch(discr(k), vec![Some(r), None]),
                    s.shifted(2),
                    s.selected(&[3, 2, 1, 0]),
                    s.shifted(1).selected(&[99, 3, 2, 1, 0]),
                    s.selected(&[0, 0, 0, 0]).selected(&[3]),
                ]);
            }
        }
        for a in &roots {
            assert_eq!(format!("{a:?}"), format!("{:?}", old(a)));
            for b in &roots {
                assert_eq!(a.cmp(b), old(a).cmp(&old(b)));
                assert_eq!(a.partial_cmp(b), Some(old(a).cmp(&old(b))));
                assert_eq!(a == b, old(a) == old(b));
                if old(a) == old(b) {
                    assert_eq!(hash(a), hash(b));
                }
            }
        }
    }
    #[test]
    fn equality_handles_one_graph_sharing_a_leaf_that_the_other_maps_twice() {
        let a = Arena::default();
        let b = Arena::default();
        let left = a.sequence(vec![a.reference(0), a.reference(1).selected(&[1, 0])]);
        let right = b.sequence(vec![b.reference(0); 2]);
        assert_eq!(left, right);
        assert_eq!(hash(&left), hash(&right));
    }
    #[test]
    fn shifted_prefix_hashes_and_equal_shared_graphs_have_bounded_work() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                for (width, depth) in [(10usize, 20usize), (100, 20), (1, 20_000)] {
                    let first = Arena::default();
                    let second = Arena::default();
                    let (mut a, mut b) = (first.reference(0), second.reference(0));
                    let (((), ha, hb), work) = crate::work::count_walk_plans(|| {
                        for _ in 0..depth {
                            // Different map placement denotes identical recipes.
                            a = first.switch(discr(0), vec![Some(a.shifted(1)); width]);
                            b = second
                                .switch(discr(0), vec![Some(b.clone()); width])
                                .shifted(1);
                            // b's outer shift also maps its own discriminant; align it.
                            a = first.switch(
                                discr(1),
                                match a.view() {
                                    View::Switch(_, arms) => arms,
                                    _ => unreachable!(),
                                },
                            );
                            assert_eq!(hash(&a), hash(&b));
                        }
                        ((), hash(&a), hash(&b))
                    });
                    assert_eq!(ha, hb);
                    assert!(
                        work <= (40 * width * depth + 32) as u64,
                        "hash {width}/{depth}: {work}"
                    );
                    let (order, work) = crate::work::count_walk_plans(|| a.cmp(&b));
                    assert_eq!(order, Ordering::Equal);
                    assert!(
                        work <= (12 * width * depth + 16) as u64,
                        "compare {width}/{depth}: {work}"
                    );
                    let (_, work) = crate::work::count_walk_plans(|| {
                        hash(&a);
                        hash(&b);
                        a.cmp(&a)
                    });
                    assert_eq!(work, 0);
                    if width == 1 {
                        let debug = format!("{a:?}");
                        assert!(debug.starts_with("Switch("));
                        assert!(debug.ends_with("])"));
                    }
                }
            })
            .expect("small-stack thread")
            .join()
            .expect("iterative semantic operations and destruction");
    }
    #[test]
    fn translated_shared_graph_comparison_does_not_expand_offset_contexts() {
        for width in [100u32, 1000] {
            let first = Arena::default();
            let second = Arena::default();
            let (mut a, mut b) = (first.reference(0), second.reference(0));
            for _ in 0..20 {
                a = first.sequence((0..width).map(|by| a.shifted(by)).collect());
                b = second.sequence((0..width).map(|by| b.shifted(by)).collect());
            }
            let (order, work) = crate::work::count_walk_plans(|| a.cmp(&b));
            assert_eq!(order, Ordering::Equal);
            assert!(work <= 8 * u64::from(width * 20 + 1), "{width}: {work}");
            assert_eq!(hash(&a), hash(&b));
        }
    }
}
