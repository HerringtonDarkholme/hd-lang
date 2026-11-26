//! Where a goal's candidates come from (`crates/hd_check/DESIGN.md` (trait solver) §3.2, §3.3): the
//! tables of its owner units, one synthetic table of the impls that no
//! owner unit holds, and the candidate directory of the impls owned only
//! through a trait argument. The last two are merged per impl universe, so
//! a context sees only the units of its own closure.

use std::sync::{Arc, OnceLock};

use crate::AdtId;
use hd_base::SliceSort as _;
use hd_base::{HashMap, HashSet};

use hd_base::{DefId, ModuleId, PathId, UnitId};
use hd_intern::{PathKind, PathTable};

use crate::pool::{Ty, TyData, Types};
use crate::solver::{HeadKey, ImplRef, ImplTable, ImplUniverseId, MemberStep, TuplePlans};

/// The table id of the asking module's own impls. Its rows sit in its
/// unit's table too; this table is read first, as before owner lookup.
pub const OWN_TABLE: ModuleId = ModuleId::from_raw(u32::MAX - 1);

/// The table id of a unit's impl table.
#[must_use]
pub fn unit_table(f: UnitId) -> ModuleId {
    ModuleId::from_raw(f.raw())
}

/// Which unit declares what: module path node to unit, built once per
/// run from the module table.
#[derive(Debug, Default)]
pub struct OwnerMap {
    by_module: HashMap<PathId, UnitId>,
}

impl OwnerMap {
    pub fn insert(&mut self, module: PathId, unit: UnitId) {
        self.by_module.insert(module, unit);
    }

    /// The unit of the module that declares `d`.
    #[must_use]
    pub fn owner(&self, paths: &PathTable, d: DefId) -> Option<UnitId> {
        let mut p = PathId::from_raw(d.raw());
        while p.get().is_some() && !matches!(paths.kind(p), PathKind::Module | PathKind::Package) {
            p = paths.parent(p);
        }
        self.by_module.get(&p).copied()
    }

    /// The unit that owns a type's outer constructor: a declared type or
    /// a trait value's trait. Built-in heads, parameters and variables own
    /// nothing.
    #[must_use]
    pub fn owner_of_ty(&self, paths: &PathTable, pool: Types<'_>, t: Ty) -> Option<UnitId> {
        match pool.get(t) {
            TyData::Adt(AdtId::Declared(def), _) | TyData::TraitValue { def, .. } => {
                self.owner(paths, def)
            }
            _ => None,
        }
    }
}

/// Whether a goal's trait argument leaves its owner unknown: a variable,
/// a placeholder, or poison (which matches every head).
#[must_use]
pub fn open_arg(pool: Types<'_>, t: Ty) -> bool {
    matches!(
        pool.get(t),
        TyData::Infer(_) | TyData::Canon(_) | TyData::Poison
    )
}

/// One unit's impls, built once per run from its interface (§3.2).
#[derive(Debug, Default)]
pub struct UnitImpls {
    pub table: ImplTable,
    /// The `arg_impls` section: rows the unit owns only through a trait
    /// argument, in row order (trait, then impl, by content rank).
    pub arg_impls: Vec<u32>,
    /// Rows no owner unit holds: std's impls for built-in targets kept
    /// in another std unit, and impls the ownership check rejected
    /// (`nonlocal-impl`, `orphan-impl`), which stay visible so their uses
    /// add no second error.
    pub unowned: Vec<u32>,
    /// Head codes of rows owned through this unit's trait or self type.
    pub owner_codes: Vec<u64>,
}

impl UnitImpls {
    /// Sorts a unit's rows by how owner lookup reaches them.
    #[must_use]
    pub fn new(
        unit: UnitId,
        table: ImplTable,
        pool: Types<'_>,
        paths: &PathTable,
        owners: &OwnerMap,
    ) -> Self {
        let mine = |o: Option<UnitId>| o == Some(unit);
        let mut arg_impls = Vec::new();
        let mut unowned = Vec::new();
        let mut owner_codes = HashSet::default();
        for r in 0..table.def.len() {
            if mine(owners.owner(paths, table.trait_[r]))
                || mine(owners.owner_of_ty(paths, pool, table.head_self[r]))
            {
                owner_codes.insert(table.head_key[r].code());
                continue;
            }
            let row = crate::word(r);
            if pool
                .list_items(table.head_args[r])
                .iter()
                .any(|a| mine(owners.owner_of_ty(paths, pool, *a)))
            {
                arg_impls.push(row);
            } else {
                unowned.push(row);
            }
        }
        UnitImpls {
            table,
            arg_impls,
            unowned,
            owner_codes: owner_codes.into_iter().collect(),
        }
    }

    /// Whether the unit joins the impl universe of a closure holding it.
    #[must_use]
    pub fn in_universe(&self) -> bool {
        !self.arg_impls.is_empty() || !self.unowned.is_empty()
    }
}

/// Rows indexed by trait, head and stable unit/row identity. Contexts
/// share persistent roots rather than sorting/copying inherited rows.
#[derive(Clone, Debug, Default)]
pub struct RefTable {
    by_trait: crate::context_map::Map<HeadRows>,
    traits_by_key: crate::context_map::Map<crate::context_map::Map<DefId>>,
    names: Arc<OnceLock<crate::solver::NameIndex>>,
    named: Option<NamedTraits>,
}

type Rows = crate::context_map::Map<ImplRef>;
type HeadRows = crate::context_map::Map<Rows>;

/// The secondary name index shares the same persistent key paths as rows.
/// Each unit indexes its own contribution before dependency roots merge.
#[derive(Clone, Debug, Default)]
struct NamedTraits {
    by_name: crate::context_map::Map<crate::context_map::Map<DefId>>,
    unnamed: crate::context_map::Map<crate::context_map::Map<DefId>>,
}

impl NamedTraits {
    fn union(&self, other: &Self) -> Self {
        Self {
            by_name: self.by_name.nested_union(&other.by_name),
            unnamed: self.unnamed.nested_union(&other.unnamed),
        }
    }
}

impl RefTable {
    fn index_names(&mut self, names: &crate::solver::TraitNames<'_>) {
        let mut index = NamedTraits::default();
        let add =
            |map: &mut crate::context_map::Map<crate::context_map::Map<DefId>>, key, tr: DefId| {
                let mut traits = map.get(key).cloned().unwrap_or_default();
                traits.insert(u128::from(tr.raw()), tr);
                map.insert(key, traits);
            };
        for (code, traits) in &self.traits_by_key {
            for (_, &tr) in traits {
                let known = names(tr, &mut |name| {
                    add(
                        &mut index.by_name,
                        (code << 32) | u128::from(name.raw()),
                        tr,
                    );
                });
                if !known {
                    add(&mut index.unnamed, code, tr);
                }
            }
        }
        self.named = Some(index);
    }

    fn build(rows: impl IntoIterator<Item = (u32, u64, ImplRef)>) -> Self {
        let mut out = Self::default();
        for (tr, code, row) in rows {
            let mut heads = out
                .by_trait
                .get(u128::from(tr))
                .cloned()
                .unwrap_or_default();
            let mut rows = heads.get(u128::from(code)).cloned().unwrap_or_default();
            let order = (u128::from(row.module.raw()) << 32) | u128::from(row.row);
            rows.insert(order, row);
            heads.insert(u128::from(code), rows);
            out.by_trait.insert(u128::from(tr), heads);
            let mut traits = out
                .traits_by_key
                .get(u128::from(code))
                .cloned()
                .unwrap_or_default();
            traits.insert(u128::from(tr), DefId::from_raw(tr));
            out.traits_by_key.insert(u128::from(code), traits);
        }
        out
    }

    fn union(&self, other: &Self) -> Self {
        use crate::context_map::Merged;
        let by_trait = self.by_trait.union_by(&other.by_trait, |a, b| {
            let merged = a.nested_union(b);
            if merged.ptr_eq(a) {
                Merged::Left
            } else if merged.ptr_eq(b) {
                Merged::Right
            } else {
                Merged::Value(merged)
            }
        });
        let traits_by_key = self.traits_by_key.union_by(&other.traits_by_key, |a, b| {
            let merged = if b.len() >= a.len() {
                b.left_union(a)
            } else {
                a.left_union(b)
            };
            if merged.ptr_eq(a) {
                Merged::Left
            } else if merged.ptr_eq(b) {
                Merged::Right
            } else {
                Merged::Value(merged)
            }
        });
        if by_trait.ptr_eq(&self.by_trait) && traits_by_key.ptr_eq(&self.traits_by_key) {
            return self.clone();
        }
        if by_trait.ptr_eq(&other.by_trait) && traits_by_key.ptr_eq(&other.traits_by_key) {
            return other.clone();
        }
        let names = if traits_by_key.ptr_eq(&self.traits_by_key) {
            Arc::clone(&self.names)
        } else if traits_by_key.ptr_eq(&other.traits_by_key) {
            Arc::clone(&other.names)
        } else {
            Arc::default()
        };
        Self {
            by_trait,
            traits_by_key,
            names,
            named: match (&self.named, &other.named) {
                (Some(a), Some(b)) => Some(a.union(b)),
                _ => None,
            },
        }
    }

    fn has_rows_at(&self, codes: [Option<u64>; 3]) -> bool {
        codes.into_iter().flatten().any(|c| {
            self.traits_by_key
                .get(u128::from(c))
                .is_some_and(|t| !t.is_empty())
        })
    }

    fn traits_at(&self, codes: [Option<u64>; 3], out: &mut Vec<DefId>) {
        for code in codes.into_iter().flatten() {
            if let Some(traits) = self.traits_by_key.get(u128::from(code)) {
                out.extend(traits.iter().map(|(_, &t)| t));
            }
        }
    }

    fn traits_named(
        &self,
        codes: [Option<u64>; 3],
        sym: hd_base::Symbol,
        names: &crate::solver::TraitNames<'_>,
        out: &mut Vec<DefId>,
    ) {
        if let Some(index) = &self.named {
            for code in codes.into_iter().flatten() {
                let code = u128::from(code);
                for traits in [
                    index.by_name.get((code << 32) | u128::from(sym.raw())),
                    index.unnamed.get(code),
                ]
                .into_iter()
                .flatten()
                {
                    out.extend(traits.iter().map(|(_, &tr)| tr));
                }
            }
            return;
        }
        // Legacy clients provide names on their first query. Driver
        // contexts instead index each unit once, before merging roots.
        let ix = self.names.get_or_init(|| {
            let by_key: HashMap<_, _> = self
                .traits_by_key
                .iter()
                .filter_map(|(code, traits)| {
                    Some((
                        u64::try_from(code).ok()?,
                        traits.iter().map(|(_, &t)| t).collect(),
                    ))
                })
                .collect();
            crate::solver::NameIndex::new(&by_key, names)
        });
        for code in codes.into_iter().flatten() {
            ix.at(code, sym, out);
        }
    }

    fn buckets(&self, trait_: DefId, key: HeadKey, visit: &mut dyn FnMut(&Rows)) {
        let Some(heads) = self.by_trait.get(u128::from(trait_.raw())) else {
            return;
        };
        if key == HeadKey::Any {
            for (_, rows) in heads {
                visit(rows);
            }
            return;
        }
        let codes = [
            Some(key.code()),
            (key != HeadKey::Param).then(|| HeadKey::Param.code()),
            matches!(key, HeadKey::Tuple(_)).then(|| HeadKey::TupleAny.code()),
        ];
        for code in codes.into_iter().flatten() {
            if let Some(rows) = heads.get(u128::from(code)) {
                visit(rows);
            }
        }
    }

    pub fn candidates(&self, trait_: DefId, key: HeadKey, out: &mut Vec<ImplRef>) {
        self.buckets(trait_, key, &mut |rows| {
            out.extend(rows.iter().map(|(_, &r)| r));
        });
    }

    #[must_use]
    pub fn has(&self, trait_: DefId, key: HeadKey) -> bool {
        let Some(heads) = self.by_trait.get(u128::from(trait_.raw())) else {
            return false;
        };
        if key == HeadKey::Any {
            return !heads.is_empty();
        }
        [
            Some(key.code()),
            (key != HeadKey::Param).then(|| HeadKey::Param.code()),
            matches!(key, HeadKey::Tuple(_)).then(|| HeadKey::TupleAny.code()),
        ]
        .into_iter()
        .flatten()
        .any(|code| {
            heads
                .get(u128::from(code))
                .is_some_and(|rows| !rows.is_empty())
        })
    }
}

/// The unowned rows and argument-owner directory contributed by a context.
#[derive(Clone, Debug, Default)]
pub struct UniverseImpls {
    pub unowned: RefTable,
    pub directory: RefTable,
}

impl UniverseImpls {
    /// Build each unit's contribution once. Rows retain their original
    /// table/row identities; combining contexts shares their indexes.
    #[must_use]
    pub fn new(units: &[(UnitId, &UnitImpls)]) -> Self {
        let mut out = Self::default();
        for (unit, fi) in units {
            let at = |row: u32| {
                let i = row as usize;
                (
                    fi.table.trait_[i].raw(),
                    fi.table.head_key[i].code(),
                    ImplRef {
                        module: unit_table(*unit),
                        row,
                    },
                )
            };
            let own = Self {
                unowned: RefTable::build(fi.unowned.iter().copied().map(at)),
                directory: RefTable::build(fi.arg_impls.iter().copied().map(at)),
            };
            out = out.union(&own);
        }
        out
    }

    /// Index this contribution's method names before it joins a context.
    /// Missing declarations remain conservative unnamed candidates.
    #[must_use]
    pub fn with_names(
        units: &[(UnitId, &UnitImpls)],
        names: &crate::solver::TraitNames<'_>,
    ) -> Self {
        let mut out = Self::new(units);
        out.unowned.index_names(names);
        out.directory.index_names(names);
        out
    }

    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self {
            unowned: self.unowned.union(&other.unowned),
            directory: self.directory.union(&other.directory),
        }
    }
}

/// The rows a probe read whose visibility the impl universe decides
/// (§3.2): the candidate directory, and the unowned rows. An answer that
/// read neither is the same in every universe.
#[derive(Clone, Copy, Debug, Default)]
pub struct UniverseReads {
    /// The probe read the candidate directory.
    pub dir: bool,
    /// The universe's unowned rows had some for the probe.
    pub unowned: bool,
}

/// The block-local impls a solving context at byte `at` of its module
/// does not see: those whose extent `[lo, hi)` does not hold it. One
/// hash probe per candidate row, however many local impls the module has.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unseen<'a> {
    /// Each block-local impl of the module with its extent; `None` when it
    /// has none.
    pub extents: Option<&'a HashMap<DefId, (u32, u32)>>,
    pub at: u32,
}

impl Unseen<'_> {
    /// Whether `def` is a local impl this context does not see.
    #[must_use]
    pub fn hides(self, def: DefId) -> bool {
        self.extents
            .and_then(|e| e.get(&def))
            .is_some_and(|&(lo, hi)| !(lo <= self.at && self.at < hi))
    }
}

/// Shared immutable owner-table access for a checking context. Empty
/// tables remain addressable; enumeration needs only nonempty tables.
pub trait UnitContext: Sync {
    fn get(&self, unit: usize) -> Option<&UnitImpls>;
    fn units(&self) -> Box<dyn Iterator<Item = (usize, &UnitImpls)> + '_>;
    /// Units with relevant owner rows. Shared contexts may omit rows
    /// already represented by their argument-owner/unowned directories.
    fn units_at(
        &self,
        codes: [Option<u64>; 3],
    ) -> Box<dyn Iterator<Item = (usize, &UnitImpls)> + '_> {
        Box::new(self.units().filter(move |(_, f)| {
            crate::work::view_slots(1);
            f.table.has_rows_at(codes)
        }))
    }
}

/// The impl tables of a closure's units, by unit id: the closure's
/// units alone, sorted by id, so a read is a binary search and a walk
/// visits the closure, not every unit of the program. A dense slot per
/// program unit made each module's view, and each walk of it, cost the
/// program's units: k modules in k units cost k^2 (Cranelift's
/// `SparseMap` keeps members apart from the key space the same way).
/// A program's view, whose closure is every unit, reads the run's own
/// per-unit table instead, so building one costs nothing per body.
#[derive(Clone, Copy)]
pub enum UnitSlots<'a> {
    /// The closure's units, sorted by unit id with no id twice.
    Closure(&'a [(u32, &'a UnitImpls)]),
    Shared(&'a dyn UnitContext),
    /// The run's table by unit id, and its built units' ids in order.
    Program(&'a [OnceLock<Option<Arc<UnitImpls>>>], &'a [u32]),
}

impl Default for UnitSlots<'_> {
    fn default() -> Self {
        UnitSlots::Closure(&[])
    }
}

impl<'a> UnitSlots<'a> {
    /// The slots `slots`, sorted by unit id with no id twice.
    #[must_use]
    pub fn new(slots: &'a [(u32, &'a UnitImpls)]) -> Self {
        debug_assert!(slots.windows(2).all(|w| w[0].0 < w[1].0), "sorted slots");
        UnitSlots::Closure(slots)
    }

    /// Unit `i`'s tables, when the closure holds it: a binary search,
    /// or a program view's index.
    #[must_use]
    pub fn get(self, i: usize) -> Option<&'a UnitImpls> {
        match self {
            UnitSlots::Closure(slots) => {
                let i = u32::try_from(i).ok()?;
                let at = slots.binary_search_by_key(&i, |s| s.0).ok()?;
                Some(slots[at].1)
            }
            UnitSlots::Program(table, _) => table.get(i)?.get()?.as_deref(),
            UnitSlots::Shared(context) => context.get(i),
        }
    }

    /// Probe a shared head index instead of scanning all visible tables.
    #[must_use]
    pub fn at(
        self,
        codes: [Option<u64>; 3],
    ) -> Box<dyn Iterator<Item = (usize, &'a UnitImpls)> + 'a> {
        match self {
            UnitSlots::Shared(context) => context.units_at(codes),
            _ => Box::new(self.iter().filter(move |(_, f)| {
                crate::work::view_slots(1);
                f.table.has_rows_at(codes)
            })),
        }
    }

    /// The closure's units in id order.
    #[must_use]
    pub fn iter(self) -> Box<dyn Iterator<Item = (usize, &'a UnitImpls)> + 'a> {
        match self {
            UnitSlots::Closure(slots) => Box::new(slots.iter().map(|&(i, f)| (i as usize, f))),
            UnitSlots::Program(_, built) => Box::new(
                built
                    .iter()
                    .filter_map(move |&i| Some((i as usize, self.get(i as usize)?))),
            ),
            UnitSlots::Shared(context) => context.units(),
        }
    }
}

/// A solving context's view of the impls (§3.2): its own module's table,
/// the frozen tables of its closure's units, read by owner, and its
/// universe's unowned rows and directory.
pub struct ImplView<'a> {
    pub paths: &'a PathTable,
    pub owners: &'a OwnerMap,
    pub own: Option<&'a ImplTable>,
    /// The units of the closure, by unit id ([`UnitSlots`]).
    pub units: UnitSlots<'a>,
    pub universe: ImplUniverseId,
    pub extra: &'a UniverseImpls,
    /// A trait's number of arguments: a projection that leaves them
    /// implicit (`Self::Out`) has open arguments.
    pub arity: &'a (dyn Fn(DefId) -> usize + Sync),
    /// Block-local impls this context does not see: those whose suite does
    /// not hold it, or that are declared after it
    /// (`trait.impl.local.lookup`, `crates/hd_tir/DESIGN.md` "Local items lift
    /// to hidden module items"). They stay rows of their tables, so a
    /// probe that meets one is never answered from the run's memo.
    pub hidden: Unseen<'a>,
    /// The member steps of the tuple templates (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
    /// `None` only where a tuple template's own plan is computed, under a
    /// memo of its own.
    pub plans: Option<&'a dyn TuplePlans>,
}

impl<'a> ImplView<'a> {
    /// This view with `hidden` as its unseen local impls.
    #[must_use]
    pub fn hiding(&self, hidden: Unseen<'a>) -> ImplView<'a> {
        ImplView {
            paths: self.paths,
            owners: self.owners,
            own: self.own,
            units: self.units,
            universe: self.universe,
            extra: self.extra,
            arity: self.arity,
            hidden,
            plans: self.plans,
        }
    }
}

/// Where the solver reads impls: owner lookup in one context (§3.2).
#[derive(Clone, Copy)]
pub enum Impls<'a> {
    Owned(&'a ImplView<'a>),
}

/// A table of an [`ImplView`] ([`Impls::tables_at`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAt {
    Own,
    /// The unit's table, by unit index.
    Unit(usize),
    Unowned,
    Directory,
}

/// A table's place in the candidate order: the asking module's own table,
/// then units in path order. Owner lookup reads only the owners' tables,
/// and this order keeps their rows where reading every table puts them.
fn position(m: ModuleId) -> u32 {
    if m == OWN_TABLE { 0 } else { m.raw() + 1 }
}

impl<'a> Impls<'a> {
    /// The table an impl row lives in.
    #[must_use]
    pub fn table(self, m: ModuleId) -> Option<&'a ImplTable> {
        let Impls::Owned(v) = self;
        if m == OWN_TABLE {
            return v.own;
        }
        v.units.get(m.raw() as usize).map(|f| &f.table)
    }

    /// The member steps of a tuple template's plan (§3.9), when this
    /// context has them.
    #[must_use]
    pub fn tuple_plan(self, template: DefId) -> Option<std::sync::Arc<[MemberStep]>> {
        let Impls::Owned(v) = self;
        v.plans.and_then(|p| p.tuple_plan(template))
    }

    /// Whether `n` written arguments leave some of the trait's implicit.
    #[must_use]
    pub fn implicit_args(self, trait_: DefId, n: usize) -> bool {
        let Impls::Owned(v) = self;
        n < (v.arity)(trait_)
    }

    /// Whether the universe's unowned rows answer a probe of
    /// `(trait_, key)`: then what the probe sees depends on the universe.
    #[must_use]
    pub fn unowned_at(self, trait_: DefId, key: HeadKey) -> bool {
        let Impls::Owned(v) = self;
        v.extra.unowned.has(trait_, key)
    }

    /// The tables of this view with a row a goal on the head key codes
    /// `codes` ([`crate::solver::key_codes`]) may take, for
    /// [`Self::traits_named`].
    ///
    /// Cost: one probe per table of the view.
    #[must_use]
    pub fn tables_at(self, codes: [Option<u64>; 3]) -> Vec<TableAt> {
        let Impls::Owned(v) = self;
        let mut out = Vec::new();
        if v.own.is_some_and(|t| t.has_rows_at(codes)) {
            out.push(TableAt::Own);
        }
        for (i, _) in v.units.at(codes) {
            out.push(TableAt::Unit(i));
        }
        if v.extra.unowned.has_rows_at(codes) {
            out.push(TableAt::Unowned);
        }
        if v.extra.directory.has_rows_at(codes) {
            out.push(TableAt::Directory);
        }
        out
    }

    /// Appends the traits of table `at` with a row a goal on the head key
    /// codes `codes` ([`crate::solver::key_codes`]) may take that declare
    /// a method `sym`, and those `names` does
    /// not know, from the table's name index, made on the table's first
    /// ask: one probe per key code. A unit's table is shared by the run,
    /// so its index is made once per run.
    pub fn traits_named(
        self,
        at: TableAt,
        codes: [Option<u64>; 3],
        sym: hd_base::Symbol,
        names: &crate::solver::TraitNames<'_>,
        out: &mut Vec<DefId>,
    ) {
        let Impls::Owned(v) = self;
        match at {
            TableAt::Own => {
                if let Some(t) = v.own {
                    t.traits_named(codes, sym, names, out);
                }
            }
            TableAt::Unit(i) => {
                if let Some(f) = v.units.get(i) {
                    f.table.traits_named(codes, sym, names, out);
                }
            }
            TableAt::Unowned => v.extra.unowned.traits_named(codes, sym, names, out),
            TableAt::Directory => v.extra.directory.traits_named(codes, sym, names, out),
        }
    }

    /// Appends the traits of table `at` with a row a goal on the head key
    /// codes `codes` ([`crate::solver::key_codes`]) may take: one probe
    /// per key code.
    pub fn traits_at(self, at: TableAt, codes: [Option<u64>; 3], out: &mut Vec<DefId>) {
        let Impls::Owned(v) = self;
        match at {
            TableAt::Own => {
                if let Some(t) = v.own {
                    t.traits_at(codes, out);
                }
            }
            TableAt::Unit(i) => {
                if let Some(f) = v.units.get(i) {
                    f.table.traits_at(codes, out);
                }
            }
            TableAt::Unowned => v.extra.unowned.traits_at(codes, out),
            TableAt::Directory => v.extra.directory.traits_at(codes, out),
        }
    }

    /// Whether table `at` has a row of `trait_` that a goal on head key
    /// `key` may take: one probe of its head index.
    #[must_use]
    pub fn has_rows(self, at: TableAt, trait_: DefId, key: HeadKey) -> bool {
        let Impls::Owned(v) = self;
        match at {
            TableAt::Own => v
                .own
                .is_some_and(|t| t.candidates(trait_, key).next().is_some()),
            TableAt::Unit(i) => v
                .units
                .get(i)
                .is_some_and(|f| f.table.candidates(trait_, key).next().is_some()),
            TableAt::Unowned => v.extra.unowned.has(trait_, key),
            TableAt::Directory => v.extra.directory.has(trait_, key),
        }
    }

    /// The candidate rows of `self_ty: trait_[args]` in content order (§3.2,
    /// §3.3): the head-index buckets of the owner units' tables (the
    /// trait's, the self type's, each known argument's), the unowned
    /// rows and, when `open` (an argument's owner is unknown), the
    /// directory. `seen` records which universe-decided rows the probe read.
    pub fn candidates(
        self,
        pool: Types<'_>,
        trait_: DefId,
        self_ty: Ty,
        args: &[Ty],
        open: bool,
        seen: &mut UniverseReads,
    ) -> Vec<(ImplRef, &'a ImplTable)> {
        let key = HeadKey::of(pool, self_ty);
        let Impls::Owned(v) = self;
        let mut owners: Vec<UnitId> = Vec::with_capacity(2 + args.len());
        owners.extend(v.owners.owner(v.paths, trait_));
        owners.extend(v.owners.owner_of_ty(v.paths, pool, self_ty));
        for a in args {
            owners.extend(v.owners.owner_of_ty(v.paths, pool, *a));
        }
        owners.sort_stable_by_key(|f| f.raw());
        owners.dedup();
        let mut refs: Vec<ImplRef> = Vec::new();
        if let Some(t) = v.own {
            refs.extend(t.candidates(trait_, key).map(|row| ImplRef {
                module: OWN_TABLE,
                row,
            }));
        }
        for f in owners {
            if let Some(fi) = v.units.get(f.idx()) {
                refs.extend(fi.table.candidates(trait_, key).map(|row| ImplRef {
                    module: unit_table(f),
                    row,
                }));
            }
        }
        let before = refs.len();
        v.extra.unowned.candidates(trait_, key, &mut refs);
        seen.unowned |= refs.len() > before;
        if open {
            seen.dir = true;
            v.extra.directory.candidates(trait_, key, &mut refs);
        }
        refs.sort_stable_by_key(|r| (position(r.module), r.row));
        refs.dedup();
        let mut rows = one_row_per_impl(
            refs.into_iter()
                .filter_map(|r| Some((r, self.table(r.module)?)))
                .collect(),
        );
        if v.hidden.extents.is_some() {
            rows.retain(|(r, t)| !v.hidden.hides(t.def[r.row as usize]));
        }
        rows
    }
}

/// Keeps the first row of each impl. A module's own impls are rows of its
/// own table and again of its unit's frozen table; they are one head
/// (rule TS-2), found at its first place in content order.
fn one_row_per_impl(mut rows: Vec<(ImplRef, &ImplTable)>) -> Vec<(ImplRef, &ImplTable)> {
    let mut seen: Vec<DefId> = Vec::with_capacity(rows.len());
    rows.retain(|(r, t)| {
        let def = t.def[r.row as usize];
        if seen.contains(&def) {
            return false;
        }
        seen.push(def);
        true
    });
    rows
}

#[cfg(test)]
mod coverage_contracts {
    use super::{UnitImpls, UnitSlots};
    use std::sync::{Arc, OnceLock};

    #[test]
    fn program_unit_iteration_matches_the_built_closure_without_copying_tables() {
        let slots: Vec<OnceLock<Option<Arc<UnitImpls>>>> =
            (0..5).map(|_| OnceLock::new()).collect();
        let first = Arc::new(UnitImpls::default());
        let second = Arc::new(UnitImpls::default());
        slots[1]
            .set(Some(Arc::clone(&first)))
            .expect("publish unit 1");
        slots[2].set(None).expect("publish failed unit 2");
        slots[3]
            .set(Some(Arc::clone(&second)))
            .expect("publish unit 3");
        let built = [1, 3];
        let program = UnitSlots::Program(&slots, &built);
        let entries = [(1, first.as_ref()), (3, second.as_ref())];
        let closure = UnitSlots::new(&entries);
        let actual: Vec<_> = program.iter().collect();
        let expected: Vec<_> = closure.iter().collect();
        assert_eq!(actual.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [1, 3]);
        for ((a, table_a), (b, table_b)) in actual.iter().zip(expected.iter()) {
            assert_eq!(a, b);
            assert!(std::ptr::eq(*table_a, *table_b));
        }
        assert!(program.get(0).is_none());
        assert!(program.get(2).is_none());
        assert!(program.get(4).is_none());
        assert!(program.get(5).is_none());
        assert_eq!(Arc::strong_count(&first), 2);
        assert_eq!(Arc::strong_count(&second), 2);
    }
}
