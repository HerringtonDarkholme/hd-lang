//! Persistent visible-unit sets, following containers' big-endian Patricia
//! `IntSet` (`Data.IntSet.Internal`, Okasaki/Gill's mergeable integer maps).
//! Leaves hold 64 adjacent unit bits. A union shares unchanged subtrees;
//! insertion copies at most 26 branch nodes and one bitmap, independently
//! of the unit graph's depth. Native recursion and drop depth are bounded
//! by twice (`u32::BITS - 6`) plus two calls (one operand swap per
//! branch level), never by source nesting or dependency-chain length.

use std::sync::{Arc, Mutex, OnceLock};

use hd_base::UnitId;
use hd_project::ModuleGraph;

#[derive(Clone, Default)]
pub struct UnitSet(Option<Arc<Node>>);

enum Node {
    Tip {
        base: u32,
        bits: u64,
        fingerprint: OnceLock<hd_base::Hash128>,
    },
    Branch {
        prefix: u32,
        mask: u32,
        left: Arc<Node>,
        right: Arc<Node>,
        fingerprint: OnceLock<hd_base::Hash128>,
    },
}

impl Node {
    fn key(&self) -> u32 {
        match self {
            Self::Tip { base, .. } => *base,
            Self::Branch { prefix, .. } => *prefix,
        }
    }

    fn mask(&self) -> u32 {
        match self {
            Self::Tip { .. } => 0,
            Self::Branch { mask, .. } => *mask,
        }
    }
    fn fingerprint(&self) -> hd_base::Hash128 {
        let cache = match self {
            Self::Tip { fingerprint, .. } | Self::Branch { fingerprint, .. } => fingerprint,
        };
        *cache.get_or_init(|| {
            super::work::context_tree_step();
            let mut h = hd_base::StableHasher::new("run-unit-set");
            match self {
                Self::Tip { base, bits, .. } => {
                    h.u8(0);
                    h.u32(*base);
                    h.u64(*bits);
                }
                Self::Branch {
                    prefix,
                    mask,
                    left,
                    right,
                    ..
                } => {
                    h.u8(1);
                    h.u32(*prefix);
                    h.u32(*mask);
                    h.hash(left.fingerprint());
                    h.hash(right.fingerprint());
                }
            }
            h.finish()
        })
    }
}

fn prefix(key: u32, mask: u32) -> u32 {
    key & !(mask | (mask - 1))
}

fn branch(p: u32, m: u32, left: Arc<Node>, right: Arc<Node>) -> Arc<Node> {
    Arc::new(Node::Branch {
        prefix: p,
        mask: m,
        left,
        right,
        fingerprint: OnceLock::new(),
    })
}

fn join(a: &Arc<Node>, b: &Arc<Node>) -> Arc<Node> {
    let different = a.key() ^ b.key();
    // ruled out: equal leaf bases or matching branch prefixes are merged
    // before join; mismatching prefixes differ in at least one high bit.
    debug_assert_ne!(different, 0);
    let mask = 1 << (u32::BITS - 1 - different.leading_zeros());
    let p = prefix(a.key(), mask);
    if a.key() & mask == 0 {
        branch(p, mask, Arc::clone(a), Arc::clone(b))
    } else {
        branch(p, mask, Arc::clone(b), Arc::clone(a))
    }
}

fn union(a: &Arc<Node>, b: &Arc<Node>) -> Arc<Node> {
    super::work::context_tree_step();
    if Arc::ptr_eq(a, b) {
        return Arc::clone(a);
    }
    let (pa, ma, pb, mb) = (a.key(), a.mask(), b.key(), b.mask());
    match (a.as_ref(), b.as_ref()) {
        (Node::Tip { base, bits: x, .. }, Node::Tip { bits: y, .. }) if pa == pb => {
            let bits = x | y;
            if bits == *x {
                Arc::clone(a)
            } else if bits == *y {
                Arc::clone(b)
            } else {
                Arc::new(Node::Tip {
                    base: *base,
                    bits,
                    fingerprint: OnceLock::new(),
                })
            }
        }
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
            let left = union(al, bl);
            let right = union(ar, br);
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
                let next = union(left, b);
                if Arc::ptr_eq(&next, left) {
                    Arc::clone(a)
                } else {
                    branch(pa, ma, next, Arc::clone(right))
                }
            } else {
                let next = union(right, b);
                if Arc::ptr_eq(&next, right) {
                    Arc::clone(a)
                } else {
                    branch(pa, ma, Arc::clone(left), next)
                }
            }
        }
        (_, Node::Branch { .. }) if mb > ma && prefix(pa, mb) == pb => union(b, a),
        _ => join(a, b),
    }
}

impl UnitSet {
    #[must_use]
    pub fn singleton(unit: u32) -> Self {
        Self(Some(Arc::new(Node::Tip {
            base: unit & !63,
            bits: 1 << (unit & 63),
            fingerprint: OnceLock::new(),
        })))
    }

    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self(match (&self.0, &other.0) {
            (Some(a), Some(b)) => Some(union(a, b)),
            (a, None) | (None, a) => a.clone(),
        })
    }

    pub(crate) fn contains(&self, unit: u32) -> bool {
        let Some(mut node) = self.0.as_deref() else {
            return false;
        };
        loop {
            match node {
                Node::Tip { base, bits, .. } => {
                    return unit & !63 == *base && bits & (1 << (unit & 63)) != 0;
                }
                Node::Branch {
                    prefix: p,
                    mask,
                    left,
                    right,
                    ..
                } => {
                    if prefix(unit, *mask) != *p {
                        return false;
                    }
                    node = if unit & mask == 0 { left } else { right };
                }
            }
        }
    }

    fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }

    /// In-run content key, computed only for new persistent-tree nodes.
    /// This hashes run IDs and never enters a disk-cache key.
    #[must_use]
    pub fn fingerprint(&self) -> hd_base::Hash128 {
        self.0
            .as_ref()
            .map_or(hd_base::Hash128(0), |n| n.fingerprint())
    }

    /// Verify a content-key match exactly, pruning shared subtrees. A hash
    /// collision never makes two distinct impl universes share an ID.
    #[must_use]
    pub fn equivalent(&self, other: &Self) -> bool {
        let (Some(a), Some(b)) = (&self.0, &other.0) else {
            return self.0.is_none() && other.0.is_none();
        };
        let mut stack = vec![(a, b)];
        while let Some((a, b)) = stack.pop() {
            super::work::context_tree_step();
            if Arc::ptr_eq(a, b) {
                continue;
            }
            match (a.as_ref(), b.as_ref()) {
                (
                    Node::Tip {
                        base: x, bits: a, ..
                    },
                    Node::Tip {
                        base: y, bits: b, ..
                    },
                ) if x == y && a == b => {}
                (
                    Node::Branch {
                        prefix: x,
                        mask: m,
                        left: al,
                        right: ar,
                        ..
                    },
                    Node::Branch {
                        prefix: y,
                        mask: n,
                        left: bl,
                        right: br,
                        ..
                    },
                ) if x == y && m == n => {
                    stack.push((al, bl));
                    stack.push((ar, br));
                }
                _ => return false,
            }
        }
        true
    }

    #[must_use]
    pub fn iter(&self) -> Units<'_> {
        Units {
            stack: self.0.as_deref().into_iter().collect(),
            base: 0,
            bits: 0,
        }
    }
}

impl<'a> IntoIterator for &'a UnitSet {
    type Item = u32;
    type IntoIter = Units<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct Units<'a> {
    stack: Vec<&'a Node>,
    base: u32,
    bits: u64,
}

impl Iterator for Units<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        while self.bits == 0 {
            match self.stack.pop()? {
                Node::Tip { base, bits, .. } => {
                    self.base = *base;
                    self.bits = *bits;
                }
                Node::Branch { left, right, .. } => {
                    self.stack.push(right);
                    self.stack.push(left);
                }
            }
        }
        let unit = self.base | self.bits.trailing_zeros();
        self.bits &= self.bits - 1;
        super::work::context_item_step();
        Some(unit)
    }
}

/// Lazy dependency contexts: one fold per reached unit. The mutex keeps
/// concurrent header/body tasks from duplicating folds; no interface or
/// check runs while held. A warm edit does not eagerly fold unused units.
pub(crate) struct Contexts(Mutex<Vec<Option<UnitSet>>>);

impl Contexts {
    pub(crate) fn new(units: usize) -> Self {
        Self(Mutex::new(vec![None; units]))
    }

    pub(crate) fn closure(
        &self,
        graph: &ModuleGraph,
        starts: impl IntoIterator<Item = UnitId>,
    ) -> UnitSet {
        let mut contexts = super::lock(&self.0);
        let mut out = UnitSet::default();
        for start in starts {
            let mut stack = vec![(start, false)];
            while let Some((unit, ready)) = stack.pop() {
                if contexts[unit.idx()].is_some() {
                    continue;
                }
                super::work::context_graph_step();
                if !ready {
                    stack.push((unit, true));
                    for &dep in graph.uses[unit.idx()].iter().rev() {
                        super::work::context_graph_step();
                        if contexts[dep.idx()].is_none() {
                            stack.push((dep, false));
                        }
                    }
                    continue;
                }
                let mut set = UnitSet::singleton(unit.raw());
                for dep in &graph.uses[unit.idx()] {
                    super::work::context_graph_step();
                    // ruled out: the module graph is the SCC-condensed DAG;
                    // each dependency was folded by this explicit stack.
                    if let Some(prior) = &contexts[dep.idx()] {
                        set = set.union(prior);
                    }
                }
                contexts[unit.idx()] = Some(set);
            }
            if let Some(set) = &contexts[start.idx()] {
                out = out.union(set);
            }
        }
        out
    }
}

/// The metadata contributed by a dependency context. Empty declarations
/// add no entries; inherited impl/method maps retain their shared roots.
#[derive(Clone)]
pub(crate) struct Metadata {
    pub(crate) complete: bool,
    pub(crate) providers: bool,
    pub(crate) reach: UnitSet,
    pub(crate) impls: UnitSet,
    pub(crate) owner_heads: hd_types::context_map::Map<UnitSet>,
    pub(crate) universe: UnitSet,
    pub(crate) universe_data: Arc<hd_types::lookup::UniverseImpls>,
    pub(crate) methods: Arc<hd_check::MethodIndex>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            complete: true,
            providers: false,
            reach: UnitSet::default(),
            impls: UnitSet::default(),
            owner_heads: hd_types::context_map::Map::default(),
            universe: UnitSet::default(),
            universe_data: Arc::default(),
            methods: Arc::default(),
        }
    }
}

impl Metadata {
    pub(crate) fn union(&self, other: &Self) -> Self {
        Self {
            complete: self.complete && other.complete,
            providers: self.providers || other.providers,
            reach: self.reach.union(&other.reach),
            impls: self.impls.union(&other.impls),
            owner_heads: self.owner_heads.union_by(&other.owner_heads, |a, b| {
                let merged = a.union(b);
                if merged.ptr_eq(a) {
                    hd_types::context_map::Merged::Left
                } else if merged.ptr_eq(b) {
                    hd_types::context_map::Merged::Right
                } else {
                    hd_types::context_map::Merged::Value(merged)
                }
            }),
            universe: self.universe.union(&other.universe),
            universe_data: Arc::new(self.universe_data.union(&other.universe_data)),
            methods: Arc::new(self.methods.union(&other.methods)),
        }
    }
}

pub(crate) struct MetadataContexts(Mutex<Vec<Option<Arc<Metadata>>>>);

impl MetadataContexts {
    pub(crate) fn new(units: usize) -> Self {
        Self(Mutex::new(vec![None; units]))
    }

    /// Register each unit's own metadata once, then merge dependency roots.
    /// The callback only reads published interfaces and builds indexes; it
    /// never checks a header/body or recursively asks for another context.
    pub(crate) fn closure(
        &self,
        graph: &ModuleGraph,
        starts: &[UnitId],
        providers: Option<&Metadata>,
        own: impl Fn(UnitId) -> Option<Metadata>,
    ) -> Option<Metadata> {
        let mut slots = super::lock(&self.0);
        let mut out = Metadata::default();
        for &start in starts {
            let mut stack = vec![(start, false)];
            while let Some((unit, ready)) = stack.pop() {
                if let Some(prior) = &slots[unit.idx()] {
                    if let Some(base) = providers.filter(|_| !prior.providers) {
                        slots[unit.idx()] = Some(Arc::new(prior.union(base)));
                    }
                    continue;
                }
                super::work::context_graph_step();
                if !ready {
                    stack.push((unit, true));
                    for &dep in graph.uses[unit.idx()].iter().rev() {
                        super::work::context_graph_step();
                        if slots[dep.idx()].is_none() {
                            stack.push((dep, false));
                        }
                    }
                    continue;
                }
                // Start from the deepest dependency: an imported unit
                // often already contains the other direct dependencies.
                // Reachability proves a skipped merge adds no metadata.
                let deps = &graph.uses[unit.idx()];
                let first = deps.iter().max_by_key(|u| graph.height[u.idx()]);
                let mut inherited = first
                    .and_then(|u| slots[u.idx()].as_deref())
                    .cloned()
                    .unwrap_or_default();
                for dep in deps {
                    super::work::context_graph_step();
                    if !inherited.reach.contains(dep.raw()) {
                        inherited = inherited.union(slots[dep.idx()].as_deref()?);
                    }
                }
                if let Some(base) = providers.filter(|_| !inherited.providers) {
                    inherited = inherited.union(base);
                }
                let metadata = own(unit)?.union(&inherited);
                slots[unit.idx()] = Some(Arc::new(metadata));
            }
            if !out.reach.contains(start.raw()) {
                out = out.union(slots[start.idx()].as_deref()?);
            }
        }
        Some(out)
    }
}

pub(crate) struct View<'r, 'a> {
    pub(crate) run: &'r super::Run<'a>,
    pub(crate) units: UnitSet,
    pub(crate) later: UnitSet,
    pub(crate) methods: Vec<Arc<hd_check::MethodIndex>>,
    pub(crate) metadata: Metadata,
}

impl hd_resolve::iface::InterfaceContext for View<'_, '_> {
    fn unit(&self, unit: UnitId) -> Option<&hd_resolve::UnitIface> {
        (self.units.contains(unit.raw()) || self.later.contains(unit.raw())).then_some(())?;
        self.run.iface.get(unit.idx())?.get()?.as_deref()
    }

    fn interfaces(&self) -> Box<dyn Iterator<Item = &hd_resolve::UnitIface> + '_> {
        Box::new(
            self.units
                .iter()
                .chain(self.later.iter().filter(|&u| !self.units.contains(u)))
                .filter_map(|u| self.run.iface.get(u as usize)?.get()?.as_deref()),
        )
    }
}

impl hd_types::lookup::UnitContext for View<'_, '_> {
    fn get(&self, unit: usize) -> Option<&hd_types::lookup::UnitImpls> {
        let id = u32::try_from(unit).ok()?;
        (self.units.contains(id) || self.later.contains(id)).then_some(())?;
        self.run.unit_impls(UnitId::from_raw(id))
    }

    fn units_at(
        &self,
        codes: [Option<u64>; 3],
    ) -> Box<dyn Iterator<Item = (usize, &hd_types::lookup::UnitImpls)> + '_> {
        let mut units = UnitSet::default();
        for code in codes.into_iter().flatten() {
            hd_types::work::view_slots(1);
            if let Some(found) = self.metadata.owner_heads.get(u128::from(code)) {
                units = units.union(found);
            }
        }
        Box::new(OwnedUnits {
            context: self,
            stack: units.0.into_iter().collect(),
            base: 0,
            bits: 0,
        })
    }

    fn units(&self) -> Box<dyn Iterator<Item = (usize, &hd_types::lookup::UnitImpls)> + '_> {
        Box::new(
            self.metadata
                .impls
                .iter()
                .filter_map(|u| Some((u as usize, self.run.unit_impls(UnitId::from_raw(u))?))),
        )
    }
}

struct OwnedUnits<'v, 'r, 'a> {
    context: &'v View<'r, 'a>,
    stack: Vec<Arc<Node>>,
    base: u32,
    bits: u64,
}
impl<'v> Iterator for OwnedUnits<'v, '_, '_> {
    type Item = (usize, &'v hd_types::lookup::UnitImpls);
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while self.bits == 0 {
                let node = self.stack.pop()?;
                match node.as_ref() {
                    Node::Tip { base, bits, .. } => {
                        self.base = *base;
                        self.bits = *bits;
                    }
                    Node::Branch { left, right, .. } => {
                        self.stack.push(Arc::clone(right));
                        self.stack.push(Arc::clone(left));
                    }
                }
            }
            let unit = self.base | self.bits.trailing_zeros();
            self.bits &= self.bits - 1;
            super::work::context_item_step();
            if let Some(table) = self.context.run.unit_impls(UnitId::from_raw(unit)) {
                return Some((unit as usize, table));
            }
        }
    }
}
