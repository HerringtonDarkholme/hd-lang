//! Where a goal's candidates come from (trait-solver.md §3.2, §3.3): the
//! tables of its owner folders, one synthetic table of the impls that no
//! owner folder holds, and the candidate directory of the impls owned only
//! through a trait argument. The last two are merged per impl universe, so
//! a context sees only the folders of its own closure.

use std::collections::HashMap;

use hd_base::{DefId, FolderId, ModuleId, PathId};
use hd_intern::{PathKind, PathTable};

use crate::pool::{Ty, TyData, Types};
use crate::solver::{HeadKey, ImplRef, ImplTable, ImplUniverseId};

/// The table id of the asking module's own impls. Its rows sit in its
/// folder's table too; this table is read first, as before owner lookup.
pub const OWN_TABLE: ModuleId = ModuleId::from_raw(u32::MAX - 1);

/// The table id of a folder's impl table.
#[must_use]
pub fn folder_table(f: FolderId) -> ModuleId {
    ModuleId::from_raw(f.raw())
}

/// Which folder declares what: module path node to folder, built once per
/// run from the module table.
#[derive(Debug, Default)]
pub struct OwnerMap {
    by_module: HashMap<PathId, FolderId>,
}

impl OwnerMap {
    pub fn insert(&mut self, module: PathId, folder: FolderId) {
        self.by_module.insert(module, folder);
    }

    /// The folder of the module that declares `d`.
    #[must_use]
    pub fn owner(&self, paths: &PathTable, d: DefId) -> Option<FolderId> {
        let mut p = PathId::from_raw(d.raw());
        while p.get().is_some() && !matches!(paths.kind(p), PathKind::Module | PathKind::Package) {
            p = paths.parent(p);
        }
        self.by_module.get(&p).copied()
    }

    /// The folder that owns a type's outer constructor: a declared type or
    /// a trait value's trait. Built-in heads, parameters and variables own
    /// nothing (§3.2 `owner_modules`).
    #[must_use]
    pub fn owner_of_ty(&self, paths: &PathTable, pool: Types<'_>, t: Ty) -> Option<FolderId> {
        match pool.get(strip_mut(pool, t)) {
            TyData::Adt { def, .. } | TyData::TraitValue { def, .. } => self.owner(paths, def),
            _ => None,
        }
    }
}

fn strip_mut(pool: Types<'_>, t: Ty) -> Ty {
    match pool.get(t) {
        TyData::Mut(i) => i,
        _ => t,
    }
}

/// Whether a goal's trait argument leaves its owner unknown: a variable,
/// a placeholder, or poison (which matches every head).
#[must_use]
pub fn open_arg(pool: Types<'_>, t: Ty) -> bool {
    matches!(
        pool.get(strip_mut(pool, t)),
        TyData::Infer(_) | TyData::Canon(_) | TyData::Poison
    )
}

/// One folder's impls, built once per run from its interface (§3.2).
#[derive(Debug, Default)]
pub struct FolderImpls {
    pub table: ImplTable,
    /// The `arg_impls` section: rows the folder owns only through a trait
    /// argument, in row order (trait, then impl, by content rank).
    pub arg_impls: Vec<u32>,
    /// Rows no owner folder holds: std's impls for built-in targets kept
    /// in another std folder, and impls the ownership check rejected
    /// (`nonlocal-impl`, `orphan-impl`), which stay visible so their uses
    /// add no second error.
    pub unowned: Vec<u32>,
}

impl FolderImpls {
    /// Sorts a folder's rows by how owner lookup reaches them.
    #[must_use]
    pub fn new(
        folder: FolderId,
        table: ImplTable,
        pool: Types<'_>,
        paths: &PathTable,
        owners: &OwnerMap,
    ) -> Self {
        let mine = |o: Option<FolderId>| o == Some(folder);
        let mut arg_impls = Vec::new();
        let mut unowned = Vec::new();
        for r in 0..table.def.len() {
            if mine(owners.owner(paths, table.trait_[r]))
                || mine(owners.owner_of_ty(paths, pool, table.head_self[r]))
            {
                continue;
            }
            let row = u32::try_from(r).expect("impls");
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
        FolderImpls {
            table,
            arg_impls,
            unowned,
        }
    }

    /// Whether the folder joins the impl universe of a closure holding it.
    #[must_use]
    pub fn in_universe(&self) -> bool {
        !self.arg_impls.is_empty() || !self.unowned.is_empty()
    }
}

/// Rows that point into folder tables, grouped by trait and head key: the
/// candidate directory, and the synthetic table of unowned rows.
#[derive(Debug, Default)]
pub struct RefTable {
    /// `(trait, head key code, row)`, sorted; the code groups, the row
    /// (folder, then row) orders within a group.
    rows: Vec<(u32, u64, ImplRef)>,
    by_trait: HashMap<u32, (u32, u32)>,
}

impl RefTable {
    fn build(mut rows: Vec<(u32, u64, ImplRef)>) -> Self {
        rows.sort_unstable_by_key(|(t, k, r)| (*t, *k, r.module.raw(), r.row));
        let mut by_trait: HashMap<u32, (u32, u32)> = HashMap::new();
        for (i, (t, _, _)) in rows.iter().enumerate() {
            let i = u32::try_from(i).expect("rows");
            by_trait
                .entry(*t)
                .and_modify(|e| e.1 = i + 1)
                .or_insert((i, i + 1));
        }
        RefTable { rows, by_trait }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn bucket(&self, s: u32, e: u32, code: u64) -> &[(u32, u64, ImplRef)] {
        let rows = &self.rows[s as usize..e as usize];
        let lo = rows.partition_point(|r| r.1 < code);
        let hi = rows.partition_point(|r| r.1 <= code);
        &rows[lo..hi]
    }

    /// The buckets of one trait whose head key can match `key` (those of
    /// `ImplTable::candidates`); a bucket that does not apply is empty.
    fn buckets(&self, trait_: DefId, key: HeadKey) -> [&[(u32, u64, ImplRef)]; 3] {
        let Some(&(s, e)) = self.by_trait.get(&trait_.raw()) else {
            return [&[], &[], &[]];
        };
        if key == HeadKey::Any {
            return [&self.rows[s as usize..e as usize], &[], &[]];
        }
        let param = if key == HeadKey::Param {
            &[][..]
        } else {
            self.bucket(s, e, HeadKey::Param.code())
        };
        let tuple = if matches!(key, HeadKey::Tuple(_)) {
            self.bucket(s, e, HeadKey::TupleAny.code())
        } else {
            &[]
        };
        [self.bucket(s, e, key.code()), param, tuple]
    }

    /// The rows of one trait whose head key can match `key`, appended
    /// unordered.
    pub fn candidates(&self, trait_: DefId, key: HeadKey, out: &mut Vec<ImplRef>) {
        for b in self.buckets(trait_, key) {
            out.extend(b.iter().map(|r| r.2));
        }
    }

    /// Whether a probe of `(trait_, key)` finds any row here.
    #[must_use]
    pub fn has(&self, trait_: DefId, key: HeadKey) -> bool {
        self.buckets(trait_, key).iter().any(|b| !b.is_empty())
    }
}

/// What an impl universe adds to owner lookup (§3.2): the unowned rows
/// (the synthetic `STD_BUILTIN_TABLE`, plus rejected impls) and the
/// candidate directory, each over the universe's folders only.
#[derive(Debug, Default)]
pub struct UniverseImpls {
    pub unowned: RefTable,
    pub directory: RefTable,
}

impl UniverseImpls {
    /// Merges the sections of a universe's folders, given in folder order.
    #[must_use]
    pub fn new(folders: &[(FolderId, &FolderImpls)]) -> Self {
        let mut unowned = Vec::new();
        let mut directory = Vec::new();
        for (f, fi) in folders {
            let t = &fi.table;
            let at = |row: u32| {
                let r = row as usize;
                (
                    t.trait_[r].raw(),
                    t.head_key[r].code(),
                    ImplRef {
                        module: folder_table(*f),
                        row,
                    },
                )
            };
            unowned.extend(fi.unowned.iter().map(|&r| at(r)));
            directory.extend(fi.arg_impls.iter().map(|&r| at(r)));
        }
        UniverseImpls {
            unowned: RefTable::build(unowned),
            directory: RefTable::build(directory),
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

/// A solving context's view of the impls (§3.2): its own module's table,
/// the frozen tables of its closure's folders, read by owner, and its
/// universe's unowned rows and directory.
pub struct ImplView<'a> {
    pub paths: &'a PathTable,
    pub owners: &'a OwnerMap,
    pub own: Option<&'a ImplTable>,
    /// By folder id: `Some` for the folders of the closure.
    pub folders: &'a [Option<&'a FolderImpls>],
    pub universe: ImplUniverseId,
    pub extra: &'a UniverseImpls,
    /// A trait's number of arguments: a projection that leaves them
    /// implicit (`Self::Out`) has open arguments.
    pub arity: &'a dyn Fn(DefId) -> usize,
}

/// Where the solver reads impls.
#[derive(Clone, Copy)]
pub enum Impls<'a> {
    /// Every table, in order: codegen's one program-wide table.
    All(&'a [(ModuleId, &'a ImplTable)]),
    /// Owner lookup in one context (§3.2).
    Owned(&'a ImplView<'a>),
}

/// A table's place in the candidate order: the asking module's own table,
/// then folders in path order. Owner lookup keeps the order a scan of
/// every table had.
fn position(m: ModuleId) -> u32 {
    if m == OWN_TABLE { 0 } else { m.raw() + 1 }
}

impl<'a> Impls<'a> {
    /// The table an impl row lives in.
    #[must_use]
    pub fn table(self, m: ModuleId) -> Option<&'a ImplTable> {
        match self {
            Impls::All(tables) => tables.iter().find(|(x, _)| *x == m).map(|(_, t)| *t),
            Impls::Owned(v) if m == OWN_TABLE => v.own,
            Impls::Owned(v) => v
                .folders
                .get(m.raw() as usize)
                .copied()
                .flatten()
                .map(|f| &f.table),
        }
    }

    /// Whether `n` written arguments leave some of the trait's implicit.
    #[must_use]
    pub fn implicit_args(self, trait_: DefId, n: usize) -> bool {
        match self {
            Impls::All(_) => false,
            Impls::Owned(v) => n < (v.arity)(trait_),
        }
    }

    /// Whether the universe's unowned rows answer a probe of
    /// `(trait_, key)`: then what the probe sees depends on the universe.
    #[must_use]
    pub fn unowned_at(self, trait_: DefId, key: HeadKey) -> bool {
        match self {
            Impls::All(_) => false,
            Impls::Owned(v) => v.extra.unowned.has(trait_, key),
        }
    }

    /// The candidate rows of `self_ty: trait_[args]` in content order (§3.2,
    /// §3.3): the head-index buckets of the owner folders' tables (the
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
        let v = match self {
            Impls::All(tables) => {
                return one_row_per_impl(
                    tables
                        .iter()
                        .flat_map(|(m, t)| {
                            t.candidates(trait_, key)
                                .map(move |row| (ImplRef { module: *m, row }, *t))
                        })
                        .collect(),
                );
            }
            Impls::Owned(v) => v,
        };
        let mut owners: Vec<FolderId> = Vec::with_capacity(2 + args.len());
        owners.extend(v.owners.owner(v.paths, trait_));
        owners.extend(v.owners.owner_of_ty(v.paths, pool, self_ty));
        for a in args {
            owners.extend(v.owners.owner_of_ty(v.paths, pool, *a));
        }
        owners.sort_unstable_by_key(|f| f.raw());
        owners.dedup();
        let mut refs: Vec<ImplRef> = Vec::new();
        if let Some(t) = v.own {
            refs.extend(t.candidates(trait_, key).map(|row| ImplRef {
                module: OWN_TABLE,
                row,
            }));
        }
        for f in owners {
            if let Some(Some(fi)) = v.folders.get(f.idx()) {
                refs.extend(fi.table.candidates(trait_, key).map(|row| ImplRef {
                    module: folder_table(f),
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
        refs.sort_unstable_by_key(|r| (position(r.module), r.row));
        refs.dedup();
        one_row_per_impl(
            refs.into_iter()
                .filter_map(|r| Some((r, self.table(r.module)?)))
                .collect(),
        )
    }
}

/// Keeps the first row of each impl. A module's own impls are rows of its
/// own table and again of its folder's frozen table; they are one head
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
