//! Dependency folds and borrowed views over one shared persistent map.
//! Unit sets use the same integer trie as methods and impl directories.
use hd_base::UnitId;
use hd_project::ModuleGraph;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct UnitSet(pub(super) hd_types::context_map::Map<()>);
impl UnitSet {
    #[must_use]
    pub fn singleton(unit: u32) -> Self {
        let mut map = hd_types::context_map::Map::default();
        map.insert(u128::from(unit), ());
        Self(map)
    }
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self(self.0.left_union(&other.0))
    }
    pub(crate) fn contains(&self, unit: u32) -> bool {
        self.0.get(u128::from(unit)).is_some()
    }
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().filter_map(|(unit, ())| {
            super::work::context_item_step();
            // UnitSet inserts only u32 unit IDs.
            u32::try_from(unit).ok()
        })
    }
}

/// One lazy DAG fold serves reachability and published checking metadata.
/// The mutex prevents duplicate folds; callbacks never run a check.
pub(crate) struct Fold<T>(Mutex<Vec<Option<T>>>);
pub(crate) type Contexts = Fold<UnitSet>;
pub(crate) type MetadataContexts = Fold<Metadata>;
pub(crate) trait Folded: Clone + Default {
    fn reach(&self) -> &UnitSet;
    fn union(&self, other: &Self) -> Self;
}
impl<T: Folded> Fold<T> {
    pub(crate) fn new(units: usize) -> Self {
        Self(Mutex::new(vec![None; units]))
    }
    fn fold(
        &self,
        graph: &ModuleGraph,
        starts: impl IntoIterator<Item = UnitId>,
        own: impl Fn(UnitId) -> Option<T>,
        upgrade: impl Fn(&mut T),
    ) -> Option<T> {
        let mut slots = super::lock(&self.0);
        let mut out = T::default();
        for start in starts {
            let mut stack = vec![(start, false)];
            while let Some((unit, ready)) = stack.pop() {
                if let Some(prior) = &mut slots[unit.idx()] {
                    upgrade(prior);
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
                let deps = &graph.uses[unit.idx()];
                // The deepest dependency often already reaches the others.
                let mut inherited = deps
                    .iter()
                    .max_by_key(|u| graph.height.get(u.idx()).copied().unwrap_or(0))
                    .and_then(|u| slots[u.idx()].as_ref())
                    .cloned()
                    .unwrap_or_default();
                for dep in deps {
                    super::work::context_graph_step();
                    if !inherited.reach().contains(dep.raw()) {
                        inherited = inherited.union(slots[dep.idx()].as_ref()?);
                    }
                }
                let mut value = own(unit)?.union(&inherited);
                upgrade(&mut value);
                slots[unit.idx()] = Some(value);
            }
            if !out.reach().contains(start.raw()) {
                out = out.union(slots[start.idx()].as_ref()?);
            }
        }
        Some(out)
    }
}
impl Folded for UnitSet {
    fn reach(&self) -> &UnitSet {
        self
    }
    fn union(&self, other: &Self) -> Self {
        UnitSet::union(self, other)
    }
}
impl Fold<UnitSet> {
    pub(crate) fn closure(
        &self,
        graph: &ModuleGraph,
        starts: impl IntoIterator<Item = UnitId>,
    ) -> UnitSet {
        // Structural reach has no unpublished metadata to return None.
        self.fold(graph, starts, |u| Some(UnitSet::singleton(u.raw())), |_| {})
            .unwrap_or_default()
    }
}

/// The metadata contributed by a dependency context. Empty declarations
/// add no entries; inherited impl/method maps retain their shared roots.
#[derive(Clone, Default)]
pub(crate) struct Metadata {
    pub(crate) missing: bool,
    pub(crate) providers: bool,
    pub(crate) reach: UnitSet,
    pub(crate) owner_heads: hd_types::context_map::Map<hd_types::context_map::Map<()>>,
    pub(crate) universe: UnitSet,
    pub(crate) universe_data: Arc<hd_types::lookup::UniverseImpls>,
    pub(crate) methods: Arc<hd_check::MethodIndex>,
}

impl Metadata {
    pub(crate) fn union(&self, other: &Self) -> Self {
        Self {
            missing: self.missing || other.missing,
            providers: self.providers || other.providers,
            reach: self.reach.union(&other.reach),
            owner_heads: self.owner_heads.nested_union(&other.owner_heads),
            universe: self.universe.union(&other.universe),
            universe_data: Arc::new(self.universe_data.union(&other.universe_data)),
            methods: Arc::new(self.methods.union(&other.methods)),
        }
    }
}

impl Folded for Metadata {
    fn reach(&self) -> &UnitSet {
        &self.reach
    }
    fn union(&self, other: &Self) -> Self {
        Metadata::union(self, other)
    }
}
impl Fold<Metadata> {
    pub(crate) fn closure(
        &self,
        graph: &ModuleGraph,
        starts: &[UnitId],
        providers: Option<&Metadata>,
        own: impl Fn(UnitId) -> Option<Metadata>,
    ) -> Option<Metadata> {
        self.fold(graph, starts.iter().copied(), own, |value| {
            if let Some(base) = providers.filter(|_| !value.providers) {
                *value = value.union(base);
            }
        })
    }
}

pub(crate) struct View<'r, 'a> {
    pub(crate) run: &'r super::Run<'a>,
    pub(crate) units: UnitSet,
    pub(crate) methods: Vec<Arc<hd_check::MethodIndex>>,
    pub(crate) metadata: Metadata,
}

impl hd_resolve::iface::InterfaceContext for View<'_, '_> {
    fn unit(&self, unit: UnitId) -> Option<&hd_resolve::UnitIface> {
        self.metadata.reach.contains(unit.raw()).then_some(())?;
        self.run.iface.get(unit.idx())?.get()?.as_deref()
    }

    fn iter(&self) -> Box<dyn Iterator<Item = &hd_resolve::UnitIface> + '_> {
        Box::new(
            self.units
                .iter()
                .chain(
                    (!self.units.0.ptr_eq(&self.metadata.reach.0))
                        .then(|| {
                            self.metadata
                                .reach
                                .iter()
                                .filter(|&u| !self.units.contains(u))
                        })
                        .into_iter()
                        .flatten(),
                )
                .filter_map(|u| self.run.iface.get(u as usize)?.get()?.as_deref()),
        )
    }
}

impl hd_types::lookup::UnitContext for View<'_, '_> {
    fn get(&self, unit: usize) -> Option<&hd_types::lookup::UnitImpls> {
        let id = u32::try_from(unit).ok()?;
        self.metadata.reach.contains(id).then_some(())?;
        self.run.unit_impls(UnitId::from_raw(id))
    }

    fn units_at(
        &self,
        codes: [Option<u64>; 3],
    ) -> Box<dyn Iterator<Item = (usize, &hd_types::lookup::UnitImpls)> + '_> {
        let mut units = hd_types::context_map::Map::default();
        for code in codes.into_iter().flatten() {
            hd_types::work::view_slots(1);
            if let Some(found) = self.metadata.owner_heads.get(u128::from(code)) {
                units = units.left_union(found);
            }
        }
        Box::new(units.into_entries().filter_map(|(unit, ())| {
            super::work::context_item_step();
            let unit = u32::try_from(unit).ok()?;
            Some((unit as usize, self.run.unit_impls(UnitId::from_raw(unit))?))
        }))
    }
}

impl<'a> IntoIterator for &'a UnitSet {
    type Item = u32;
    type IntoIter = Box<dyn Iterator<Item = u32> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}
