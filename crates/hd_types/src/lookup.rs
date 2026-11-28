//! Where a goal's candidates come from (`crates/hd_check/DESIGN.md` (trait solver) §3.2, §3.3): the
//! tables of its owner units, one synthetic table of the impls that no
//! owner unit holds, and the candidate directory of the impls owned only
//! through a trait argument. The last two are merged per impl universe, so
//! a context sees only the units of its own closure.

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

/// Persistent secondary indexes. Packed integer prefixes group rows by
/// (trait,head), traits by head, and names by (head,symbol), in stable order.
#[derive(Clone, Debug, Default)]
pub struct RefTable {
    by_trait: crate::context_map::Map<Rows>,
    traits_by_key: crate::context_map::Map<DefId>,
    names: crate::context_map::Map<DefId>,
    unnamed: crate::context_map::Map<DefId>,
}
type Rows = crate::context_map::Map<ImplRef>;
impl RefTable {
    fn build(
        rows: impl IntoIterator<Item = (u32, u64, ImplRef)>,
        names: &crate::solver::TraitNames<'_>,
    ) -> Self {
        let mut out = Self::default();
        for (tr, code, row) in rows {
            let def = DefId::from_raw(tr);
            let (tr, code) = (u128::from(tr), u128::from(code));
            let key = (tr << 64) | code;
            let mut entries = out.by_trait.get(key).cloned().unwrap_or_default();
            entries.insert(
                (u128::from(row.module.raw()) << 32) | u128::from(row.row),
                row,
            );
            out.by_trait.insert(key, entries);
            let key = (code << 32) | tr;
            if out.traits_by_key.get(key).is_none() {
                out.traits_by_key.insert(key, def);
                if !names(def, &mut |name| {
                    out.names
                        .insert((code << 64) | (u128::from(name.raw()) << 32) | tr, def);
                }) {
                    out.unnamed.insert(key, def);
                }
            }
        }
        out
    }
    fn union(&self, other: &Self) -> Self {
        Self {
            by_trait: self.by_trait.nested_union(&other.by_trait),
            traits_by_key: self.traits_by_key.left_union(&other.traits_by_key),
            names: self.names.left_union(&other.names),
            unnamed: self.unnamed.left_union(&other.unnamed),
        }
    }
    fn has_rows_at(&self, codes: [Option<u64>; 3]) -> bool {
        codes.into_iter().flatten().any(|c| {
            self.traits_by_key
                .prefix(u128::from(c) << 32, 32)
                .next()
                .is_some()
        })
    }
    fn traits_at(&self, codes: [Option<u64>; 3], out: &mut Vec<DefId>) {
        for code in codes.into_iter().flatten() {
            out.extend(
                self.traits_by_key
                    .prefix(u128::from(code) << 32, 32)
                    .map(|(_, &tr)| tr),
            );
        }
    }
    fn traits_named(&self, codes: [Option<u64>; 3], sym: hd_base::Symbol, out: &mut Vec<DefId>) {
        for code in codes.into_iter().flatten() {
            let code = u128::from(code);
            out.extend(
                self.names
                    .prefix((code << 64) | (u128::from(sym.raw()) << 32), 32)
                    .chain(self.unnamed.prefix(code << 32, 32))
                    .map(|(_, &tr)| tr),
            );
        }
    }
    fn buckets(&self, trait_: DefId, key: HeadKey) -> impl Iterator<Item = &Rows> {
        let base = u128::from(trait_.raw()) << 64;
        let all = (key == HeadKey::Any).then(|| self.by_trait.prefix(base, 64));
        let codes = [
            (key != HeadKey::Any).then(|| key.code()),
            (key != HeadKey::Any && key != HeadKey::Param).then(|| HeadKey::Param.code()),
            matches!(key, HeadKey::Tuple(_)).then(|| HeadKey::TupleAny.code()),
        ];
        all.into_iter().flatten().map(|(_, rows)| rows).chain(
            codes
                .into_iter()
                .flatten()
                .filter_map(move |code| self.by_trait.get(base | u128::from(code))),
        )
    }
    pub fn candidates(&self, trait_: DefId, key: HeadKey, out: &mut Vec<ImplRef>) {
        out.extend(
            self.buckets(trait_, key)
                .flat_map(|rows| rows.iter().map(|(_, &r)| r)),
        );
    }
    #[must_use]
    pub fn has(&self, trait_: DefId, key: HeadKey) -> bool {
        self.buckets(trait_, key).next().is_some()
    }
}

/// The unowned rows and argument-owner directory contributed by a context.
#[derive(Clone, Debug, Default)]
pub struct UniverseImpls {
    pub unowned: RefTable,
    pub directory: RefTable,
}

impl UniverseImpls {
    /// One unit's contribution, indexed before dependency roots merge.
    #[must_use]
    pub fn unit(unit: UnitId, fi: &UnitImpls, names: &crate::solver::TraitNames<'_>) -> Self {
        let at = |row: u32| {
            let i = row as usize;
            (
                fi.table.trait_[i].raw(),
                fi.table.head_key[i].code(),
                ImplRef {
                    module: unit_table(unit),
                    row,
                },
            )
        };
        Self {
            unowned: RefTable::build(fi.unowned.iter().copied().map(at), names),
            directory: RefTable::build(fi.arg_impls.iter().copied().map(at), names),
        }
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
    fn units_at(
        &self,
        codes: [Option<u64>; 3],
    ) -> Box<dyn Iterator<Item = (usize, &UnitImpls)> + '_>;
}
impl UnitContext for () {
    fn get(&self, _: usize) -> Option<&UnitImpls> {
        None
    }
    fn units_at(&self, _: [Option<u64>; 3]) -> Box<dyn Iterator<Item = (usize, &UnitImpls)> + '_> {
        Box::new(std::iter::empty())
    }
}
#[cfg(test)]
#[path = "../tests/support/contexts.rs"]
mod context_fixtures;

/// A solving context's view of the impls (§3.2): its own module's table,
/// the frozen tables of its closure's units, read by owner, and its
/// universe's unowned rows and directory.
pub struct ImplView<'a> {
    pub paths: &'a PathTable,
    pub owners: &'a OwnerMap,
    pub own: Option<&'a ImplTable>,
    /// The units of the closure, by unit id ([`UnitContext`]).
    pub units: &'a dyn UnitContext,
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
        for (i, _) in v.units.units_at(codes) {
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
            TableAt::Unowned => v.extra.unowned.traits_named(codes, sym, out),
            TableAt::Directory => v.extra.directory.traits_named(codes, sym, out),
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
    use super::{UnitContext, UnitImpls};
    use std::sync::{Arc, OnceLock};

    #[test]
    fn borrowed_unit_context_keeps_sparse_published_tables_and_failed_units() {
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
        let published: Vec<_> = built
            .iter()
            .filter_map(|&id| Some((id, slots[id as usize].get()?.as_deref()?)))
            .collect();
        let program: &dyn UnitContext = &published;
        let entries = [(1, first.as_ref()), (3, second.as_ref())];
        let closure: &dyn UnitContext = &entries;
        let actual: Vec<_> = built
            .iter()
            .filter_map(|&id| program.get(id as usize).map(|t| (id, t)))
            .collect();
        let expected: Vec<_> = built
            .iter()
            .filter_map(|&id| closure.get(id as usize).map(|t| (id, t)))
            .collect();
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
