//! Flat, shared count recipes. Edges own IDs and mappings, never child roots.
//! Roots keep their arena alive; dropping an arena releases flat arrays.

use super::Discr;
use hd_base::{HashMap, SliceSort as _};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

mod semantics;
pub(super) use semantics::hash_scalar;

#[derive(Clone)]
struct Slots {
    values: Arc<[u32]>,
    hash: u64,
    max: Option<u32>,
    min: Option<u32>,
}
impl Slots {
    fn new(values: Vec<u32>) -> Self {
        crate::work::walk_plans(values.len());
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        values.hash(&mut hash);
        let max = values.iter().copied().max();
        let min = values.iter().copied().min();
        Self {
            values: values.into(),
            hash: hash.finish(),
            max,
            min,
        }
    }
}
impl PartialEq for Slots {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.values, &other.values) || self.values == other.values
    }
}
impl Eq for Slots {}
impl Hash for Slots {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Mapping {
    Shift(u32),
    Select {
        slots: Slots,
        start: usize,
        after: u32,
    },
}
impl Mapping {
    fn apply(&self, k: u32) -> u32 {
        match self {
            Self::Shift(by) => k + by,
            Self::Select {
                slots,
                start,
                after,
            } => slots.values[*start + k as usize] + after,
        }
    }
    fn upper(&self, max: Option<u32>) -> Option<u32> {
        max.and_then(|k| match self {
            Self::Shift(by) => Some(k.saturating_add(*by)),
            Self::Select { slots, after, .. } => slots.max.map(|m| m.saturating_add(*after)),
        })
    }
    fn addresses(&self, bounds: Option<(u32, u32)>) -> Option<(u32, u32)> {
        bounds.and_then(|(lo, hi)| match self {
            Self::Shift(by) => Some((lo.saturating_add(*by), hi.saturating_add(*by))),
            Self::Select { slots, after, .. } => slots
                .min
                .zip(slots.max)
                .map(|(lo, hi)| (lo.saturating_add(*after), hi.saturating_add(*after))),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Link {
    node: usize,
    map: usize,
}
#[derive(Clone, PartialEq, Eq, Hash)]
enum Node {
    Ref(u32),
    Known(u32),
    Glue,
    Seq(Arc<[Link]>),
    Switch(Discr, Arc<[Option<Link>]>),
}
struct Row {
    node: Node,
    max: Option<u32>,
    has_glue: bool,
    addresses: Option<(u32, u32)>,
    normalized: bool,
}
#[derive(Default)]
struct Foreign {
    // The weak control block prevents source-address reuse without retaining
    // another arena's node storage or introducing recursive arena destruction.
    _owner: Weak<Store>,
    nodes: HashMap<usize, usize>,
}
struct Data {
    nodes: Vec<Arc<Row>>,
    ids: HashMap<Node, usize>,
    maps: Vec<Mapping>,
    map_ids: HashMap<Mapping, usize>,
    compositions: HashMap<(usize, usize), usize>,
    foreign: HashMap<usize, Foreign>,
    skips: HashMap<Vec<u32>, usize>,
    skip_values: Vec<Arc<hd_base::HashSet<u32>>>,
    filtered: HashMap<(Link, usize), Option<Link>>,
    signatures: HashMap<Link, semantics::Signature>,
}
impl Default for Data {
    fn default() -> Self {
        let identity = Mapping::Shift(0);
        Self {
            nodes: Vec::new(),
            ids: HashMap::default(),
            maps: vec![identity.clone()],
            map_ids: HashMap::from_iter([(identity, 0)]),
            compositions: HashMap::default(),
            foreign: HashMap::default(),
            skips: HashMap::default(),
            skip_values: Vec::new(),
            filtered: HashMap::default(),
            signatures: HashMap::default(),
        }
    }
}
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poison) => poison.into_inner(),
    }
}
impl Data {
    fn mapping(&mut self, map: Mapping) -> usize {
        if let Some(&id) = self.map_ids.get(&map) {
            return id;
        }
        let id = self.maps.len();
        crate::work::walk_plans(1);
        self.maps.push(map.clone());
        self.map_ids.insert(map, id);
        id
    }
    /// outer(inner(k)); shift composition does no table work. Each distinct
    /// selection composition reads its input entries once, then is memoized.
    fn compose(&mut self, outer: usize, inner: usize) -> usize {
        if outer == 0 {
            return inner;
        }
        if inner == 0 {
            return outer;
        }
        if let Some(&id) = self.compositions.get(&(outer, inner)) {
            return id;
        }
        crate::work::walk_plans(1);
        let result = match (&self.maps[outer], &self.maps[inner]) {
            (Mapping::Shift(a), Mapping::Shift(b)) => Mapping::Shift(a + b),
            (
                Mapping::Shift(a),
                Mapping::Select {
                    slots,
                    start,
                    after,
                },
            ) => Mapping::Select {
                slots: slots.clone(),
                start: *start,
                after: a + after,
            },
            (
                Mapping::Select {
                    slots,
                    start,
                    after,
                },
                Mapping::Shift(b),
            ) => Mapping::Select {
                slots: slots.clone(),
                start: start + *b as usize,
                after: *after,
            },
            (
                Mapping::Select {
                    slots: outer,
                    start: outer_start,
                    after: outer_after,
                },
                Mapping::Select {
                    slots,
                    start,
                    after,
                },
            ) => {
                // Selection rows describe only indices used by their recipe.
                // Extend unused backing entries with zero when composing; those
                // entries can name slots absent from the outer selection.
                let values = slots
                    .values
                    .get(*start..)
                    .unwrap_or_default()
                    .iter()
                    .map(|&k| {
                        k.checked_add(*after)
                            .and_then(|k| outer_start.checked_add(k as usize))
                            .and_then(|k| outer.values.get(k))
                            .and_then(|k| k.checked_add(*outer_after))
                            .unwrap_or_default()
                    })
                    .collect();
                Mapping::Select {
                    slots: Slots::new(values),
                    start: 0,
                    after: 0,
                }
            }
        };
        let id = self.mapping(result);
        self.compositions.insert((outer, inner), id);
        id
    }
    fn mapped_max(&self, link: Link) -> Option<u32> {
        self.maps[link.map].upper(self.nodes[link.node].max)
    }
    fn mapped_addresses(&self, link: Link) -> Option<(u32, u32)> {
        self.maps[link.map].addresses(self.nodes[link.node].addresses)
    }
    fn intern(&mut self, node: Node) -> usize {
        crate::work::walk_plans(match &node {
            Node::Seq(edges) => 1 + edges.len(),
            Node::Switch(_, arms) => 1 + arms.len(),
            Node::Ref(_) | Node::Known(_) | Node::Glue => 1,
        });
        if let Some(&id) = self.ids.get(&node) {
            return id;
        }
        let max = match &node {
            Node::Ref(k) | Node::Known(k) => Some(*k),
            Node::Glue => None,
            Node::Seq(edges) => edges.iter().filter_map(|&e| self.mapped_max(e)).max(),
            Node::Switch(d, arms) => arms
                .iter()
                .flatten()
                .filter_map(|&e| self.mapped_max(e))
                .chain([d.value])
                .max(),
        };
        let id = self.nodes.len();
        let has_glue = match &node {
            Node::Glue => true,
            Node::Seq(edges) => edges.iter().any(|e| self.nodes[e.node].has_glue),
            Node::Switch(_, arms) => arms.iter().flatten().any(|e| self.nodes[e.node].has_glue),
            Node::Ref(_) | Node::Known(_) => false,
        };
        let mut addresses: Option<(u32, u32)> = None;
        let mut include = |bounds| {
            addresses = match (addresses, bounds) {
                (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
                (a, None) | (None, a) => a,
            };
        };
        match &node {
            Node::Ref(k) | Node::Known(k) => include(Some((*k, *k))),
            Node::Glue => {}
            Node::Seq(edges) => edges
                .iter()
                .for_each(|&e| include(self.mapped_addresses(e))),
            Node::Switch(_, arms) => arms
                .iter()
                .flatten()
                .for_each(|&e| include(self.mapped_addresses(e))),
        }
        // `without` also normalizes sequences even when no address is removed.
        // Only already-normal recipes may bypass that transformation.
        let normalized = match &node {
            Node::Ref(_) | Node::Known(_) | Node::Glue => true,
            Node::Seq(edges) => {
                edges.len() >= 2
                    && edges.iter().all(|e| {
                        self.nodes[e.node].normalized
                            && !matches!(self.nodes[e.node].node, Node::Glue)
                    })
            }
            Node::Switch(_, arms) => arms.iter().flatten().all(|e| self.nodes[e.node].normalized),
        };
        self.nodes.push(Arc::new(Row {
            node: node.clone(),
            max,
            has_glue,
            addresses,
            normalized,
        }));
        self.ids.insert(node, id);
        id
    }
}

#[derive(Clone, Default)]
pub(super) struct Arena(Arc<Store>);
#[derive(Default)]
struct Store {
    data: Mutex<Data>,
    imports: Mutex<()>,
}
#[derive(Clone)]
pub struct Root {
    arena: Arena,
    link: Link,
}
pub(super) enum View {
    Ref(u32),
    Known(u32),
    Glue,
    Seq(Vec<Root>),
    Switch(Discr, Vec<Option<Root>>),
}
#[derive(Default)]
struct Scope {
    arena: Option<Arena>,
}
thread_local! {
    // A cache scope does not imply a compound recipe. Most bodies use only
    // inline scalars; allocate their shared store only at the first compound.
    static ACTIVE: std::cell::RefCell<Option<Scope>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn cached<R>(f: impl FnOnce() -> R) -> R {
    struct Reset(Option<Scope>);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.set(self.0.take());
        }
    }
    let _reset = Reset(ACTIVE.replace(Some(Scope::default())));
    f()
}
pub(super) fn cached_here<R>(f: impl FnOnce() -> R) -> R {
    if ACTIVE.with_borrow(Option::is_some) {
        f()
    } else {
        cached(f)
    }
}
impl Arena {
    pub(super) fn for_roots<'a>(mut roots: impl Iterator<Item = &'a Root>) -> Self {
        ACTIVE
            .with_borrow_mut(|scope| {
                scope
                    .as_mut()
                    .map(|scope| scope.arena.get_or_insert_default().clone())
            })
            .or_else(|| roots.next().map(|r| r.arena.clone()))
            .unwrap_or_default()
    }
    fn root(&self, node: Node) -> Root {
        let id = lock(&self.0.data).intern(node);
        Root {
            arena: self.clone(),
            link: Link { node: id, map: 0 },
        }
    }
    pub(super) fn reference(&self, k: u32) -> Root {
        self.root(Node::Ref(k))
    }
    pub(super) fn known(&self, k: u32) -> Root {
        self.root(Node::Known(k))
    }
    pub(super) fn glue(&self) -> Root {
        self.root(Node::Glue)
    }
    pub(super) fn sequence(&self, children: Vec<Root>) -> Root {
        let links = children.into_iter().map(|c| self.import(&c)).collect();
        self.root(Node::Seq(links))
    }
    pub(super) fn switch(&self, discr: Discr, arms: Vec<Option<Root>>) -> Root {
        let links = arms
            .into_iter()
            .map(|a| a.map(|c| self.import(&c)))
            .collect();
        self.root(Node::Switch(discr, links))
    }
    fn import(&self, root: &Root) -> Link {
        enum Task {
            Enter(usize),
            Exit(usize, Arc<Row>),
        }

        if Arc::ptr_eq(&self.0, &root.arena.0) {
            return root.link;
        }
        // Only imports are serialized. Source reads take its data lock, never
        // its import lock; opposite-direction imports hold no pair of data locks.
        let _import = lock(&self.0.imports);
        let owner = Arc::as_ptr(&root.arena.0) as usize;
        lock(&self.0.data)
            .foreign
            .entry(owner)
            .or_insert_with(|| Foreign {
                _owner: Arc::downgrade(&root.arena.0),
                nodes: HashMap::default(),
            });
        let mut todo = vec![Task::Enter(root.link.node)];
        while let Some(task) = todo.pop() {
            match task {
                Task::Enter(id) => {
                    if lock(&self.0.data).foreign[&owner].nodes.contains_key(&id) {
                        continue;
                    }
                    crate::work::walk_plans(1);
                    let row = lock(&root.arena.0.data).nodes[id].clone();
                    let children: Vec<_> = match &row.node {
                        Node::Seq(edges) => edges.iter().map(|e| e.node).collect(),
                        Node::Switch(_, arms) => arms.iter().flatten().map(|e| e.node).collect(),
                        Node::Ref(_) | Node::Known(_) | Node::Glue => Vec::new(),
                    };
                    todo.push(Task::Exit(id, row));
                    todo.extend(children.into_iter().rev().map(Task::Enter));
                }
                Task::Exit(id, row) => {
                    // Source snapshots never remain locked while target state
                    // is changed, so opposite-direction imports cannot deadlock.
                    let mapped: Vec<_> = match &row.node {
                        Node::Seq(edges) => edges.iter().copied().collect(),
                        Node::Switch(_, arms) => arms.iter().flatten().copied().collect(),
                        Node::Ref(_) | Node::Known(_) | Node::Glue => Vec::new(),
                    };
                    let maps: Vec<_> = {
                        let source = lock(&root.arena.0.data);
                        mapped.iter().map(|e| source.maps[e.map].clone()).collect()
                    };
                    let mut target = lock(&self.0.data);
                    let links: Vec<_> = mapped
                        .iter()
                        .zip(maps)
                        .map(|(e, map)| Link {
                            // ruled out: each child's Enter/Exit precedes this Exit.
                            node: target.foreign[&owner].nodes[&e.node],
                            map: target.mapping(map),
                        })
                        .collect();
                    let node = match &row.node {
                        Node::Ref(k) => Node::Ref(*k),
                        Node::Known(k) => Node::Known(*k),
                        Node::Glue => Node::Glue,
                        Node::Seq(_) => Node::Seq(links.into()),
                        Node::Switch(d, arms) => {
                            let mut links = links.into_iter();
                            Node::Switch(
                                *d,
                                arms.iter()
                                    .map(|a| a.as_ref().and_then(|_| links.next()))
                                    .collect(),
                            )
                        }
                    };
                    let node = target.intern(node);
                    target
                        .foreign
                        .get_mut(&owner)
                        .map(|f| f.nodes.insert(id, node));
                }
            }
        }
        let map = lock(&root.arena.0.data).maps[root.link.map].clone();
        let mut target = lock(&self.0.data);
        Link {
            node: target.foreign[&owner].nodes[&root.link.node],
            map: target.mapping(map),
        }
    }
}
impl Root {
    pub(super) fn has_glue(&self) -> bool {
        lock(&self.arena.0.data).nodes[self.link.node].has_glue
    }
    pub(super) fn indices(&self, addresses_only: bool) -> Vec<u32> {
        let mut seen = hd_base::HashSet::default();
        let mut todo = vec![self.clone()];
        let mut out = Vec::new();
        while let Some(root) = todo.pop() {
            if !seen.insert(root.link) {
                continue;
            }
            match root.view() {
                View::Ref(k) | View::Known(k) => out.push(k),
                View::Glue => {}
                View::Seq(children) => todo.extend(children),
                View::Switch(d, arms) => {
                    if !addresses_only {
                        out.push(d.value);
                    }
                    todo.extend(arms.into_iter().flatten());
                }
            }
        }
        out
    }
    /// The mapped scalar head, without allocating children of a compound node.
    pub(super) fn scalar(&self) -> Option<View> {
        let data = lock(&self.arena.0.data);
        match &data.nodes[self.link.node].node {
            Node::Ref(k) => Some(View::Ref(data.maps[self.link.map].apply(*k))),
            Node::Known(k) => Some(View::Known(data.maps[self.link.map].apply(*k))),
            Node::Glue => Some(View::Glue),
            Node::Seq(_) | Node::Switch(..) => None,
        }
    }
    pub(super) fn shifted(&self, by: u32) -> Self {
        if by == 0 {
            return self.clone();
        }
        let mut data = lock(&self.arena.0.data);
        if data.nodes[self.link.node].max.is_none() {
            return self.clone();
        }
        let outer = data.mapping(Mapping::Shift(by));
        let map = data.compose(outer, self.link.map);
        Self {
            arena: self.arena.clone(),
            link: Link {
                node: self.link.node,
                map,
            },
        }
    }
    pub(super) fn selected(&self, positions: &[u32]) -> Self {
        let mut data = lock(&self.arena.0.data);
        let max = data.mapped_max(self.link);
        if max.is_none() {
            return self.clone();
        }
        crate::work::walk_plans(positions.len());
        if max.is_some_and(|m| (m as usize) < positions.len())
            && positions.iter().enumerate().all(|(k, &v)| k == v as usize)
        {
            return self.clone();
        }
        let outer = data.mapping(Mapping::Select {
            slots: Slots::new(positions.to_vec()),
            start: 0,
            after: 0,
        });
        let map = data.compose(outer, self.link.map);
        Self {
            arena: self.arena.clone(),
            link: Link {
                node: self.link.node,
                map,
            },
        }
    }
    /// Drops selected address slots, retaining discriminants. Each distinct
    /// (recipe link, skip set) is transformed once, including shared subgraphs.
    pub(super) fn without(&self, skip: &[u32]) -> Option<Self> {
        enum Task {
            Enter(Link),
            Seq(Link, Vec<Link>),
            Switch(Link, Discr, Vec<Option<Link>>),
        }

        let mut key = skip.to_vec();
        key.sort_stable();
        key.dedup();
        crate::work::walk_plans(skip.len());
        let (which, skip) = {
            let mut data = lock(&self.arena.0.data);
            let which = if let Some(&which) = data.skips.get(&key) {
                which
            } else {
                let which = data.skip_values.len();
                data.skip_values
                    .push(Arc::new(key.iter().copied().collect()));
                data.skips.insert(key, which);
                which
            };
            (which, data.skip_values[which].clone())
        };
        let mut todo = vec![Task::Enter(self.link)];
        while let Some(task) = todo.pop() {
            match task {
                Task::Enter(link) => {
                    {
                        let mut data = lock(&self.arena.0.data);
                        if data.filtered.contains_key(&(link, which)) {
                            continue;
                        }
                        if data.nodes[link.node].normalized
                            && matches!(data.nodes[link.node].node, Node::Seq(_) | Node::Switch(..))
                        {
                            let bounds = data.mapped_addresses(link);
                            crate::work::walk_plans(skip.len() + 1);
                            if bounds.is_none_or(|(lo, hi)| skip.iter().all(|&k| k < lo || k > hi))
                            {
                                data.filtered.insert((link, which), Some(link));
                                continue;
                            }
                        }
                    }
                    crate::work::walk_plans(1);
                    let root = Self {
                        arena: self.arena.clone(),
                        link,
                    };
                    match root.view() {
                        View::Ref(k) | View::Known(k) => {
                            lock(&self.arena.0.data)
                                .filtered
                                .insert((link, which), (!skip.contains(&k)).then_some(link));
                        }
                        View::Glue => {
                            lock(&self.arena.0.data)
                                .filtered
                                .insert((link, which), Some(link));
                        }
                        View::Seq(children) => {
                            let children: Vec<_> = children.into_iter().map(|r| r.link).collect();
                            todo.push(Task::Seq(link, children.clone()));
                            todo.extend(children.into_iter().rev().map(Task::Enter));
                        }
                        View::Switch(discr, arms) => {
                            let arms: Vec<_> =
                                arms.into_iter().map(|r| r.map(|r| r.link)).collect();
                            todo.push(Task::Switch(link, discr, arms.clone()));
                            todo.extend(arms.into_iter().rev().flatten().map(Task::Enter));
                        }
                    }
                }
                Task::Seq(link, children) => {
                    let mut data = lock(&self.arena.0.data);
                    let mut children: Vec<_> = children
                        .into_iter()
                        .filter_map(|c| {
                            crate::work::walk_plans(1);
                            // Child results precede their parent's exit task.
                            data.filtered[&(c, which)]
                        })
                        .collect();
                    let glue = |c: &Link| matches!(data.nodes[c.node].node, Node::Glue);
                    if children.iter().any(|c| !glue(c)) {
                        children.retain(|c| !glue(c));
                    } else {
                        children.truncate(1);
                    }
                    let result = match children.len() {
                        0 => None,
                        1 => children.pop(),
                        _ => Some(Link {
                            node: data.intern(Node::Seq(children.into())),
                            map: 0,
                        }),
                    };
                    data.filtered.insert((link, which), result);
                }
                Task::Switch(link, discr, arms) => {
                    let mut data = lock(&self.arena.0.data);
                    let arms = arms
                        .into_iter()
                        .map(|c| {
                            crate::work::walk_plans(1);
                            c.and_then(|c| data.filtered[&(c, which)])
                        })
                        .collect();
                    let result = Link {
                        node: data.intern(Node::Switch(discr, arms)),
                        map: 0,
                    };
                    data.filtered.insert((link, which), Some(result));
                }
            }
        }
        lock(&self.arena.0.data).filtered[&(self.link, which)].map(|link| Self {
            arena: self.arena.clone(),
            link,
        })
    }

    pub(super) fn view(&self) -> View {
        let mut data = lock(&self.arena.0.data);
        let node = data.nodes[self.link.node].clone();
        let mapping = data.maps[self.link.map].clone();
        crate::work::walk_plans(1);
        let mut child = |link: Link| Root {
            arena: self.arena.clone(),
            link: Link {
                node: link.node,
                map: data.compose(self.link.map, link.map),
            },
        };
        match &node.node {
            Node::Ref(k) => View::Ref(mapping.apply(*k)),
            Node::Known(k) => View::Known(mapping.apply(*k)),
            Node::Glue => View::Glue,
            Node::Seq(edges) => View::Seq(edges.iter().map(|&e| child(e)).collect()),
            Node::Switch(d, arms) => {
                let mut d = *d;
                d.value = mapping.apply(d.value);
                View::Switch(d, arms.iter().map(|a| a.map(&mut child)).collect())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Enc, Leaf};
    use crate::module::Val;

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
    fn leaves(root: Root) -> Vec<u32> {
        let mut out = Vec::new();
        let mut todo = vec![root];
        while let Some(root) = todo.pop() {
            match root.view() {
                View::Ref(k) | View::Known(k) => out.push(k),
                View::Glue => {}
                View::Seq(children) => todo.extend(children.into_iter().rev()),
                View::Switch(_, arms) => todo.extend(arms.into_iter().rev().flatten()),
            }
        }
        out
    }
    #[test]
    fn scalar_cache_scopes_do_no_arena_work_at_tenfold_growth() {
        use crate::layout::Walk;
        for width in [100, 1000] {
            let ((), work) =
                crate::work::count_walk_plans(|| {
                    for _ in 0..width {
                        cached(|| {
                            cached_here(|| {
                                let scalar = Walk::Ref(1).shifted(2).selected(&[0, 1, 2, 4]);
                                assert_eq!(scalar.without(&[0]), Some(Walk::Ref(4)));
                                assert!(ACTIVE.with_borrow(|scope| matches!(
                                    scope,
                                    Some(Scope { arena: None })
                                )));
                            });
                        });
                        assert!(ACTIVE.with_borrow(Option::is_none));
                    }
                });
            assert_eq!(work, 0, "{width} scalar-only scopes");
        }
    }
    #[test]
    fn lazy_cache_scopes_reuse_and_restore_their_first_arena() {
        cached(|| {
            assert!(ACTIVE.with_borrow(|scope| matches!(scope, Some(Scope { arena: None }))));
            let first = Arena::for_roots(std::iter::empty());
            let root = first.sequence(vec![first.reference(0), first.known(1)]);
            cached_here(|| {
                let reused = Arena::for_roots(std::iter::empty());
                assert!(Arc::ptr_eq(&first.0, &reused.0));
            });
            cached(|| {
                assert!(ACTIVE.with_borrow(|scope| matches!(scope, Some(Scope { arena: None }))));
                let nested = Arena::for_roots(std::iter::once(&root));
                assert!(!Arc::ptr_eq(&first.0, &nested.0));
                assert_eq!(leaves(nested.sequence(vec![root])), [0, 1]);
            });
            let restored = Arena::for_roots(std::iter::empty());
            assert!(Arc::ptr_eq(&first.0, &restored.0));
        });
        assert!(ACTIVE.with_borrow(Option::is_none));
    }
    #[test]
    fn mappings_compose_without_changing_the_recipe() {
        let arena = Arena::default();
        let root = arena.sequence(vec![arena.reference(0), arena.known(1), arena.glue()]);
        assert_eq!(leaves(root.clone()), [0, 1]);
        assert_eq!(leaves(root.shifted(1).selected(&[7, 4, 6])), [4, 6]);
        assert_eq!(leaves(root.selected(&[3, 2]).shifted(5)), [8, 7]);
        assert_eq!(leaves(root.selected(&[1, 0]).selected(&[9, 8])), [8, 9]);
        assert_eq!(leaves(root.selected(&[0, 1])), [0, 1]);
        let unused_large_slot = arena.reference(0).selected(&[0, u32::MAX]).shifted(1);
        assert_eq!(leaves(arena.sequence(vec![unused_large_slot.clone()])), [1]);
        assert_eq!(leaves(unused_large_slot.selected(&[8, 9])), [9]);
        assert_eq!(
            leaves(arena.reference(0).selected(&[0, 99]).selected(&[4])),
            [4]
        );
        assert_eq!(
            leaves(root.selected(&[3, 3]).selected(&[8, 7, 6, 5])),
            [5, 5]
        );
        assert_eq!(leaves(root.shifted(2).selected(&[99, 98, 4, 6])), [4, 6]);
        let switch = arena.switch(discr(0), vec![None, Some(root)]).shifted(4);
        let View::Switch(d, arms) = switch.view() else {
            panic!("switch")
        };
        assert_eq!(d.value, 4);
        assert!(arms[0].is_none());
        assert_eq!(leaves(arms[1].clone().expect("payload")), [4, 5]);
    }
    #[test]
    fn foreign_arenas_import_shared_nodes_once_and_survive_source_drop() {
        let source = Arena::default();
        let lifetime = Arc::downgrade(&source.0);
        let child = source.switch(discr(0), vec![Some(source.reference(1)), None]);
        let root = source
            .sequence(vec![child.clone(), child, source.glue()])
            .selected(&[4, 3]);
        let target = Arena::default();
        let combined = target.sequence(vec![root.clone(), root.clone()]);
        let count = lock(&target.0.data).nodes.len();
        let repeated = target.sequence(vec![root.clone(), root]);
        assert_eq!(lock(&target.0.data).nodes.len(), count);
        drop(source);
        assert!(lifetime.upgrade().is_none());
        assert_eq!(leaves(combined), [3, 3, 3, 3]);
        assert_eq!(leaves(repeated), [3, 3, 3, 3]);
    }
    #[test]
    fn filtering_preserves_tree_semantics_and_mapped_discriminants() {
        use super::oracle::Walk;
        fn materialize(root: &Root) -> Walk {
            match root.view() {
                View::Ref(k) => Walk::Ref(k),
                View::Known(k) => Walk::Known(k),
                View::Glue => Walk::Glue,
                View::Seq(children) => Walk::Seq(children.iter().map(materialize).collect()),
                View::Switch(d, arms) => Walk::Switch(
                    Box::new(d),
                    arms.iter().map(|r| r.as_ref().map(materialize)).collect(),
                ),
            }
        }
        let arena = Arena::default();
        let recipes = [
            arena.reference(0),
            arena.known(1),
            arena.glue(),
            arena.sequence(vec![]),
            arena.sequence(vec![arena.reference(0)]),
            arena.sequence(vec![arena.glue(), arena.glue()]),
            arena.sequence(vec![arena.glue(), arena.reference(0), arena.known(1)]),
            arena.switch(discr(0), vec![None, Some(arena.reference(1))]),
            arena.switch(
                discr(1),
                vec![Some(arena.sequence(vec![arena.glue(), arena.known(0)]))],
            ),
        ];
        for root in recipes {
            for root in [root.clone(), root.shifted(2), root.selected(&[3, 3])] {
                for skip in [vec![], vec![0], vec![1], vec![0, 1, 2, 3], vec![3, 0, 3]] {
                    assert_eq!(
                        root.without(&skip).as_ref().map(materialize),
                        materialize(&root).without(&skip)
                    );
                }
            }
        }
    }
    #[test]
    fn filtering_shared_dags_and_deep_chains_visits_each_link_once() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                for (width, depth) in [(10usize, 20usize), (100, 20), (1, 20_000)] {
                    let arena = Arena::default();
                    let mut root = arena.reference(0);
                    for _ in 0..depth {
                        root = arena.switch(discr(1), vec![Some(root); width]);
                    }
                    let (filtered, work) = crate::work::count_walk_plans(|| root.without(&[0]));
                    assert!(
                        work <= (8 * width * depth + 16) as u64,
                        "{width}/{depth}: {work}"
                    );
                    let mut root = filtered.expect("switch survives removed payload");
                    for _ in 0..depth - 1 {
                        let View::Switch(d, arms) = root.view() else {
                            panic!("switch")
                        };
                        assert_eq!(d.value, 1);
                        assert_eq!(arms.len(), width);
                        root = arms[0].clone().expect("inner switch");
                    }
                    let View::Switch(_, arms) = root.view() else {
                        panic!("last switch")
                    };
                    assert!(arms.iter().all(Option::is_none));
                }
            })
            .expect("small-stack thread")
            .join()
            .expect("iterative filtering and destruction");
    }
    #[test]
    fn width_1000_depth_20_and_twenty_thousand_deep_recipes_have_linear_work() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                for (width, depth) in [(100u32, 20u32), (1000, 20), (1, 20_000)] {
                    for shared in [false, true] {
                        let arena = Arena::default();
                        let (roots, work) = crate::work::count_walk_plans(|| {
                            let mut roots = Vec::new();
                            for column in 0..width {
                                let mut root = arena.reference(if shared { 0 } else { column });
                                for _ in 0..depth {
                                    root =
                                        arena.switch(discr(0), vec![None, Some(root.shifted(1))]);
                                }
                                roots.push(root);
                            }
                            roots
                        });
                        assert!(
                            work <= 8 * u64::from(width * depth + width),
                            "{width}/{depth}: {work}"
                        );
                        let ((), clone_work) = crate::work::count_walk_plans(|| {
                            for root in &roots {
                                drop(root.clone());
                            }
                        });
                        assert_eq!(clone_work, 0);
                        for (root, column) in roots.into_iter().zip(0u32..) {
                            assert_eq!(leaves(root), [(if shared { 0 } else { column }) + depth]);
                        }
                        assert!(
                            lock(&arena.0.data).nodes.len()
                                <= if shared {
                                    (depth + 1) as usize
                                } else {
                                    (width * (depth + 1)) as usize
                                }
                        );
                        drop(arena);
                    }
                }
            })
            .expect("small-stack thread")
            .join()
            .expect("flat recipes do not consume native depth");
    }
    #[test]
    fn public_keys_and_indices_survive_cache_scopes_and_normalization() {
        use crate::layout::{Walk, WalkKind};
        fn tree(root: &Walk) -> super::oracle::Walk {
            use super::oracle::Walk as Tree;
            match root.kind() {
                WalkKind::Ref(k) => Tree::Ref(k),
                WalkKind::Known(k) => Tree::Known(k),
                WalkKind::Glue => Tree::Glue,
                WalkKind::Seq(children) => Tree::Seq(children.iter().map(tree).collect()),
                WalkKind::Switch(d, arms) => Tree::Switch(
                    Box::new(d),
                    arms.iter().map(|a| a.as_ref().map(tree)).collect(),
                ),
            }
        }
        let root = cached(|| {
            Walk::sequence(vec![
                Walk::Glue,
                Walk::Ref(1),
                Walk::Ref(1),
                Walk::switch(discr(0), vec![None, Some(Walk::Known(2))]),
            ])
        });
        let imported = cached(|| Walk::sequence(vec![root.clone(), Walk::Glue]));
        for addresses_only in [false, true] {
            let mut indices = imported.indices(addresses_only);
            indices.sort_stable();
            indices.dedup();
            assert_eq!(
                indices,
                if addresses_only {
                    vec![1, 2]
                } else {
                    vec![0, 1, 2]
                }
            );
        }
        assert!(Walk::Glue.has_glue());
        assert!(imported.has_glue());
        assert_eq!(Walk::Glue.without(&[1]), Some(Walk::Glue));
        let mut recipes = vec![Walk::Ref(0), Walk::Known(1), Walk::Glue, root, imported];
        recipes.push(recipes[3].without(&[1, 2]).expect("discriminant remains"));
        for a in &recipes {
            assert_eq!(format!("{a:?}"), format!("{:?}", tree(a)));
            for b in &recipes {
                assert_eq!(a.partial_cmp(b), Some(tree(a).cmp(&tree(b))));
            }
        }
    }
    #[test]
    fn filtering_untouched_shifted_prefixes_retains_shared_recipes() {
        for depth in [100usize, 1000] {
            let arena = Arena::default();
            let (kept, work) = crate::work::count_walk_plans(|| {
                let mut root = arena.reference(1);
                let mut kept = root.clone();
                for _ in 0..depth {
                    root = arena.switch(discr(0), vec![None, Some(root.shifted(1))]);
                    kept = root
                        .without(&[0])
                        .expect("no address occupies the tag slot");
                }
                kept
            });
            assert!(work <= 40 * depth as u64, "{depth}: {work}");
            assert!(lock(&arena.0.data).nodes.len() <= 2 * depth + 1);
            assert_eq!(
                leaves(kept),
                [u32::try_from(depth).expect("test depth") + 1]
            );
        }
    }
    #[test]
    fn a_panicking_client_does_not_make_the_arena_lock_unusable() {
        let data = Mutex::new(7);
        let failed = std::panic::catch_unwind(|| {
            let _guard = lock(&data);
            panic!("client panic while holding a cache guard");
        });
        assert!(failed.is_err());
        assert_eq!(*lock(&data), 7);
        *lock(&data) = 9;
        assert_eq!(*lock(&data), 9);
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use std::sync::{Barrier, mpsc};
    use std::time::Duration;

    #[test]
    fn opposite_direction_imports_do_not_hold_source_and_target_data_locks() {
        let a = Arena::default();
        let b = Arena::default();
        let first = a.sequence(vec![a.reference(0), a.known(1)]);
        let second = b.sequence(vec![b.reference(2), b.known(3)]);
        let barrier = Arc::new(Barrier::new(2));
        let (send, receive) = mpsc::channel();
        for (target, root) in [(a, second), (b, first)] {
            let barrier = barrier.clone();
            let send = send.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let imported = target.sequence(vec![root]);
                let View::Seq(outer) = imported.view() else {
                    panic!("outer sequence")
                };
                let View::Seq(inner) = outer[0].view() else {
                    panic!("inner sequence")
                };
                assert_eq!(inner.len(), 2);
                send.send(()).expect("receiver");
            });
        }
        for _ in 0..2 {
            receive
                .recv_timeout(Duration::from_secs(5))
                .expect("imports finish");
        }
    }
}

#[cfg(test)]
mod oracle;
