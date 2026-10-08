//! The trait solver's types and entry points (trait-solver.md §1.3, §2,
//! §3, §6, §7, §8, §10, §12). The goal, candidate, memo, evidence and
//! failure records are real definitions; `solve` and `select` are the
//! table solver: impl heads matched with their bound plans, and the
//! `Instantiations` and `Methods` goals' candidate schemes (§6.5), over
//! canonical goals memoized per body and per run (§7: heights, cycles,
//! fuel charged per proof node); the projection goal is still not
//! implemented (the checker normalizes).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hd_base::{
    AppendVec, DefId, FolderId, Fuel, InferVar, ModuleId, NotImplemented, Stage, StageResult,
    Symbol,
};

pub use crate::lookup::{
    FolderImpls, ImplView, Impls, OWN_TABLE, OwnerMap, RefTable, UniverseImpls, UniverseReads,
    folder_table, open_arg,
};
use crate::pool::{ParamRef, Prim, Ty, TyData, TyList, Types};
use crate::unify::VarKind;

/// A trait reference: `self_ty: trait_[args]` (§2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TraitRef {
    pub trait_: DefId,
    pub self_ty: Ty,
    pub args: TyList,
}

/// A trait reference with no variable and no parameter: codegen's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConcreteTraitRef(pub TraitRef);

/// The four goals (§1.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Goal {
    Implements {
        tref: TraitRef,
        bindings: Vec<(DefId, Ty)>,
        mut_: bool,
    },
    Project {
        assoc: DefId,
        tref: TraitRef,
    },
    Instantiations {
        self_ty: Ty,
        trait_: DefId,
        mut_: bool,
    },
    /// The trait part of method lookup (§6.5): `traits` are the
    /// available traits that declare `name`, from the asking module's
    /// method index (what the memo key's availability part stands for),
    /// in content order. Inherent methods are the checker's.
    Methods {
        receiver: Ty,
        name: Symbol,
        traits: Vec<DefId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GoalKind {
    Implements,
    Project,
    Instantiations,
    Methods,
}

/// A canonical goal (§2.2): variables numbered as placeholders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CanonGoal {
    pub kind: GoalKind,
    pub mut_: bool,
    pub n_vars: u8,
    pub trait_: DefId,
    pub self_ty: Ty,
    pub args: TyList,
    pub extra: u32,
}

/// Maps placeholders back to the asking body's variables.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanonVars {
    pub map: Vec<InferVar>,
    pub kinds: Vec<VarKind>,
}

/// The memo scope of a canonical goal (§2.2 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Global,
    Env,
    Body,
}

/// An interned elaborated environment (§2.3); `EMPTY` names no parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EnvKey(pub u32);

impl EnvKey {
    pub const EMPTY: EnvKey = EnvKey(0);
    /// An environment the run memo has not interned yet (`GlobalMemo::env_key`).
    pub const UNSET: EnvKey = EnvKey(u32::MAX);
}

/// A declared bound, as written on an item (§4.2's input).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredBound {
    pub param: ParamRef,
    pub tref: TraitRef,
    pub bindings: Vec<(DefId, Ty)>,
    pub mut_: bool,
}

/// The elaborated parameter environment, as columns, frozen per item (§2.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParamEnv {
    pub key: Option<EnvKey>,
    pub clause_self: Vec<Ty>,
    pub clause_trait: Vec<DefId>,
    pub clause_args: Vec<TyList>,
    pub clause_bindings: Vec<Vec<(DefId, Ty)>>,
    pub clause_mut: Vec<bool>,
    pub clause_origin: Vec<u16>,
}

pub type ParamEnvBuilder = ParamEnv;

/// The impl universe of a solving context (§3.2; scheduler.md §6.1): the
/// sorted folders of its closure whose `arg_impls` section is not empty.
/// A run ID: it keys the memo, never a cache key or output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImplUniverseId(pub u32);

/// Interns impl universes once per run; equal lists share one id and one
/// merged view of their folders' unowned rows and directory.
#[derive(Default)]
pub struct ImplUniverses {
    by_list: Mutex<UniverseMap>,
}

type UniverseMap = HashMap<Vec<FolderId>, (ImplUniverseId, Arc<UniverseImpls>)>;

impl ImplUniverses {
    /// The universe of a context: `folders` are the folders of its closure
    /// that hold `arg_impls` or unowned rows, in any order.
    pub fn intern(
        &self,
        folders: &[(FolderId, &FolderImpls)],
    ) -> (ImplUniverseId, Arc<UniverseImpls>) {
        let mut list = folders.to_vec();
        list.sort_by_key(|(f, _)| f.raw());
        list.dedup_by_key(|(f, _)| *f);
        let key: Vec<FolderId> = list.iter().map(|(f, _)| *f).collect();
        let mut m = self.by_list.lock().expect("universes");
        let next = ImplUniverseId(u32::try_from(m.len()).expect("universes"));
        m.entry(key)
            .or_insert_with(|| (next, Arc::new(UniverseImpls::new(&list))))
            .clone()
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_list.lock().expect("universes").len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An impl head's index key (§3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeadKey {
    Ctor(DefId),
    Prim(Prim),
    Tuple(u16),
    TupleAny,
    Fn,
    SuspendFn,
    Param,
    Any,
}

impl HeadKey {
    /// The head key of a type: what a candidate lookup indexes by.
    #[must_use]
    pub fn of(pool: Types<'_>, t: Ty) -> HeadKey {
        match pool.get(t) {
            TyData::Adt { def, .. } | TyData::TraitValue { def, .. } => HeadKey::Ctor(def),
            TyData::Prim(p) => HeadKey::Prim(p),
            TyData::Tuple { elems, rest: None } => {
                HeadKey::Tuple(u16::try_from(pool.list_items(elems).len()).expect("arity"))
            }
            TyData::Tuple { rest: Some(_), .. } => HeadKey::TupleAny,
            TyData::Fn { suspends: true, .. } => HeadKey::SuspendFn,
            TyData::Fn { .. } => HeadKey::Fn,
            TyData::Mut(i) => HeadKey::of(pool, i),
            TyData::Param(_) => HeadKey::Param,
            _ => HeadKey::Any,
        }
    }

    /// A bucket code: rows of one trait are grouped by it (§3.3). It
    /// orders buckets inside a table only; candidates still come out in
    /// row order, so no raw id reaches an answer.
    #[must_use]
    pub fn code(self) -> u64 {
        let (tag, payload) = match self {
            HeadKey::Ctor(d) => (1, d.raw()),
            HeadKey::Prim(p) => (2, u32::from(p as u8)),
            HeadKey::Tuple(n) => (3, u32::from(n)),
            HeadKey::TupleAny => (4, 0),
            HeadKey::Fn => (5, 0),
            HeadKey::SuspendFn => (6, 0),
            HeadKey::Param => (7, 0),
            HeadKey::Any => (8, 0),
        };
        (tag << 32) | u64::from(payload)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImplOrigin {
    Written,
    Derived { template: DefId },
    Delegated { field: u16 },
    Error,
    NumericFamily,
    TupleTemplate,
}

/// A row of an impl table: module and row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImplRef {
    pub module: ModuleId,
    pub row: u32,
}

/// One step of an impl's bound plan (§3.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanStep {
    Bound {
        param: u8,
        trait_: DefId,
        args: TyList,
        mut_: bool,
    },
    /// `P < Tr[A, Name = Q]` with `Q` a bare impl parameter: `Q` is the
    /// normal form of `<P as Tr[A]>::Name` (`trait.overlap.constrained-binding`).
    Bind {
        param: u8,
        trait_: DefId,
        args: TyList,
        assoc: DefId,
        target: u8,
    },
}

/// Fills the impl parameters that `Bind` steps fix from the projections
/// of parameters the head fixed; a projection that is not concrete yet
/// leaves its target as it was.
pub fn apply_binds(
    pool: Types<'_>,
    impls: Impls<'_>,
    t: &ImplTable,
    row: usize,
    args: &mut [Option<Ty>],
) {
    apply_binds_in(pool, impls, t, row, args, &mut Reads::default());
}

fn apply_binds_in(
    pool: Types<'_>,
    impls: Impls<'_>,
    t: &ImplTable,
    row: usize,
    args: &mut [Option<Ty>],
    reads: &mut Reads,
) {
    let owner = t.def[row];
    for _ in 0..t.plan[row].len() {
        let mut changed = false;
        for step in &t.plan[row] {
            let PlanStep::Bind {
                param,
                trait_,
                args: targs,
                assoc,
                target,
            } = step
            else {
                continue;
            };
            let (Some(Some(base)), Some(None)) =
                (args.get(*param as usize), args.get(*target as usize))
            else {
                continue;
            };
            let known: Vec<Ty> = args.iter().map(|a| a.unwrap_or(Ty::POISON)).collect();
            let ta = pool.list(
                &pool
                    .list_items(*targs)
                    .iter()
                    .copied()
                    .map(|x| {
                        pool.subst(x, &|p: ParamRef| {
                            (p.owner == owner)
                                .then(|| known.get(p.index as usize).copied())
                                .flatten()
                        })
                    })
                    .collect::<Vec<_>>(),
            );
            if let Some(x) = project_concrete(pool, impls, *assoc, *trait_, *base, ta, reads) {
                args[*target as usize] = Some(norm_concrete(pool, impls, x, 0, reads));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// The solver's per-module impl table (§3.3), rows sorted by content rank.
#[derive(Clone, Debug, Default)]
pub struct ImplTable {
    pub trait_: Vec<DefId>,
    pub def: Vec<DefId>,
    pub head_key: Vec<HeadKey>,
    pub arg_key: Vec<[HeadKey; 2]>,
    pub n_params: Vec<u8>,
    pub head_self: Vec<Ty>,
    pub head_args: Vec<TyList>,
    pub plan: Vec<Vec<PlanStep>>,
    pub assoc: Vec<Vec<(DefId, Ty)>>,
    pub origin: Vec<ImplOrigin>,
    pub rank: Vec<u64>,
    pub by_trait: BTreeMap<u32, (u32, u32)>,
    /// The head index (§3.3): within each trait's row range, the rows
    /// sorted by `(head key code, row)`. Built by `index`.
    pub by_key: Vec<u32>,
}

impl ImplTable {
    /// Builds the head index once the rows are in place.
    pub fn index(&mut self) {
        let mut perm: Vec<u32> = (0..u32::try_from(self.def.len()).expect("impls")).collect();
        for &(s, e) in self.by_trait.values() {
            perm[s as usize..e as usize]
                .sort_unstable_by_key(|&r| (self.head_key[r as usize].code(), r));
        }
        self.by_key = perm;
    }

    /// The rows in `[s, e)` with one head key code, in row order.
    fn bucket(&self, s: u32, e: u32, code: u64) -> &[u32] {
        let rows = &self.by_key[s as usize..e as usize];
        let lo = rows.partition_point(|&r| self.head_key[r as usize].code() < code);
        let hi = rows.partition_point(|&r| self.head_key[r as usize].code() <= code);
        &rows[lo..hi]
    }

    /// Rows of one trait whose head key can match `key`, in row order: the
    /// exact bucket, the `Param` rows and, for a tuple, the `TupleAny`
    /// rows. A goal whose key is `Any` sees every row of the trait.
    #[must_use]
    pub fn candidates(&self, trait_: DefId, key: HeadKey) -> Rows<'_> {
        debug_assert_eq!(self.by_key.len(), self.def.len(), "ImplTable::index");
        let Some(&(s, e)) = self.by_trait.get(&trait_.raw()) else {
            return Rows::All(0..0);
        };
        if key == HeadKey::Any {
            return Rows::All(s..e);
        }
        let param = if key == HeadKey::Param {
            &[][..]
        } else {
            self.bucket(s, e, HeadKey::Param.code())
        };
        let tuple = if matches!(key, HeadKey::Tuple(_)) {
            self.bucket(s, e, HeadKey::TupleAny.code())
        } else {
            &[][..]
        };
        Rows::Buckets([self.bucket(s, e, key.code()), param, tuple])
    }
}

/// The rows a head-index probe returns, in row order (§3.3).
pub enum Rows<'a> {
    All(std::ops::Range<u32>),
    /// Up to three buckets, each in row order, merged.
    Buckets([&'a [u32]; 3]),
}

impl Iterator for Rows<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        match self {
            Rows::All(r) => r.next(),
            Rows::Buckets(b) => {
                let mut best: Option<usize> = None;
                for i in 0..3 {
                    if let Some(&x) = b[i].first()
                        && best.is_none_or(|j| x < b[j][0])
                    {
                        best = Some(i);
                    }
                }
                let i = best?;
                let x = b[i][0];
                b[i] = &b[i][1..];
                Some(x)
            }
        }
    }
}

/// Head matching (§3.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchResult {
    Yes(Vec<Ty>),
    No,
    Maybe(Vec<u8>),
}

/// A candidate of an `Instantiations` goal (§6.5): a scheme over the
/// impl's parameters. `impl_args` holds each impl parameter: the type the
/// target fixed, or the parameter itself when it is fresh (`n_fresh` of
/// them). `args` is the trait arguments over the same. `residual` lists
/// the plan steps the solver did not decide: those that read a fresh
/// parameter, and those that stalled on a variable of the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub row: ImplRef,
    pub n_fresh: u8,
    pub impl_args: TyList,
    pub args: TyList,
    pub residual: Vec<u16>,
}

/// One frame of the explicit search stack (§6.1), 24 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub goal: u32,
    pub impl_row: u32,
    pub step: u16,
    pub depth: u8,
    pub height: u8,
    pub heads: u32,
    pub subst: u32,
    pub seen: u32,
}
const _: () = assert!(core::mem::size_of::<Frame>() == 24);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MemoKind {
    Holds,
    Fails,
    Stalled,
    Overflow,
    AtLeast,
}

/// A memo entry (§7.1), 12 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct MemoEntry {
    pub answer: u32,
    pub children: u32,
    pub height: u8,
    pub kind: MemoKind,
    pub heads: u16,
}
const _: () = assert!(core::mem::size_of::<MemoEntry>() == 12);

/// The memo key: the canonical goal plus what its scope adds (§7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MemoKey {
    pub goal: CanonGoal,
    pub env: EnvKey,
    pub universe: Option<ImplUniverseId>,
    pub avail: u32,
}

impl MemoKey {
    /// The universe part of a goal's key (§2.2, §3.2): every
    /// `Instantiations` and `Methods` goal, and every goal with an open
    /// trait argument, may read the candidate directory, so its answer
    /// depends on the asking context's universe. Other goals key without it.
    #[must_use]
    pub fn universe_for(
        pool: Types<'_>,
        kind: GoalKind,
        args: TyList,
        universe: ImplUniverseId,
    ) -> Option<ImplUniverseId> {
        let reads = match kind {
            GoalKind::Instantiations | GoalKind::Methods => true,
            GoalKind::Implements | GoalKind::Project => {
                pool.list_items(args).iter().any(|a| open_arg(pool, *a))
            }
        };
        reads.then_some(universe)
    }

    /// A canonical goal's key in a context (§7.1).
    #[must_use]
    pub fn new(
        pool: Types<'_>,
        goal: CanonGoal,
        env: EnvKey,
        universe: ImplUniverseId,
        avail: u32,
    ) -> MemoKey {
        MemoKey {
            goal,
            env,
            universe: Self::universe_for(pool, goal.kind, goal.args, universe),
            avail,
        }
    }
}

/// A memo slot: a global entry index, or a body entry index with this bit
/// set (§7.1 "Children are entry indices").
pub const BODY_ENTRY: u32 = 1 << 31;

/// Levels a goal asked at depth 0 may use: depths 0 to 64 (§7.2, §7.3).
const LEVELS: u32 = 65;

/// The height of a cycle: no depth suffices (§7.3 point 4).
const CYCLE: u8 = u8::MAX;

/// An answer over placeholders and global types (§7.1 "Remapping"): what
/// a memo entry stores. It names no body-local type or variable, so a
/// trial's rollback cannot leave it dangling. `OutOfFuel` is never stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonAnswer {
    Holds {
        evidence: Evidence,
        learned: Vec<(u8, Ty)>,
    },
    Candidates(Vec<Candidate>),
    Fails(Box<FailInfo>),
    Stalled {
        on: Vec<u8>,
    },
    Overflow,
}

/// The cold part of a memo entry: its goal, answer and proof-DAG children
/// (memo slots, in plan order, repeats removed; §7.4), and the impl
/// probes of its subtree.
#[derive(Debug)]
pub struct MemoRecord {
    /// The goal's key as its context asked it. A global entry whose
    /// subtree is `scoped` is published under this key with the
    /// context's universe filled in.
    pub key: MemoKey,
    pub answer: CanonAnswer,
    pub children: Box<[u32]>,
    /// The `(trait, head key)` probes of the goal's whole subtree, as a
    /// set. A module whose own table has rows for one of them lists those
    /// rows first and names them by its own table (lookup.rs
    /// `OWN_TABLE`), so the entry is not that module's answer.
    pub reads: Box<[(DefId, HeadKey)]>,
    /// Some frame of the subtree read rows the impl universe decides: the
    /// candidate directory, or unowned rows (§3.2). The answer then holds
    /// for its universe only.
    pub scoped: bool,
}

impl MemoRecord {
    /// The key a global entry computed in `universe` is published under
    /// (§3.2, §7.1): a scoped answer's key names the universe that
    /// decided it, even for a goal whose own key leaves it out. Any other
    /// answer is the same in every universe whose own and unowned rows
    /// add nothing to its probes, and keeps the asked key.
    #[must_use]
    pub fn global_key(&self, universe: ImplUniverseId) -> MemoKey {
        if self.scoped {
            MemoKey {
                universe: Some(universe),
                ..self.key
            }
        } else {
            self.key
        }
    }
}

/// Memo hits and misses of a run, over every eligible goal asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoStats {
    pub hits: u64,
    pub misses: u64,
}

/// An environment's clauses: what `EnvKey` interns.
type EnvClauses = (
    Vec<Ty>,
    Vec<DefId>,
    Vec<TyList>,
    Vec<Vec<(DefId, Ty)>>,
    Vec<bool>,
    Vec<u16>,
);

/// The run's global memo: completed, context-free answers only (rule
/// TS-4), shared by every body and thread. Entries and records are
/// append-only, so a published entry never changes and a reader holds it
/// without a lock; the shards map keys to entries, first writer wins.
#[derive(Default)]
pub struct GlobalMemo {
    shards: [Mutex<HashMap<MemoKey, u32>>; 16],
    entries: AppendVec<MemoEntry>,
    records: AppendVec<MemoRecord>,
    envs: Mutex<HashMap<EnvClauses, EnvKey>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl GlobalMemo {
    fn shard(&self, k: &MemoKey) -> &Mutex<HashMap<MemoKey, u32>> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        k.hash(&mut h);
        &self.shards[usize::try_from(h.finish() % 16).expect("shard")]
    }

    /// The entry published for `k`.
    #[must_use]
    pub fn lookup(&self, k: &MemoKey) -> Option<u32> {
        self.shard(k).lock().expect("memo").get(k).copied()
    }

    #[must_use]
    pub fn get(&self, k: &MemoKey) -> Option<MemoEntry> {
        self.lookup(k).map(|i| self.entry(i))
    }

    #[must_use]
    pub fn entry(&self, i: u32) -> MemoEntry {
        *self.entries.get(i).expect("memo entry")
    }

    #[must_use]
    pub fn record(&self, i: u32) -> &MemoRecord {
        self.records.get(self.entry(i).answer).expect("memo record")
    }

    /// Publishes an entry computed in `universe` under its global key and
    /// returns the index that holds that key: the first writer wins. Two
    /// writers of one key computed one answer (rule TS-1); debug builds
    /// check it.
    pub fn publish(&self, rec: MemoRecord, mut e: MemoEntry, universe: ImplUniverseId) -> u32 {
        let key = rec.global_key(universe);
        let mut shard = self.shard(&key).lock().expect("memo");
        if let Some(&i) = shard.get(&key) {
            debug_assert!(
                {
                    let (w, wr) = (self.entry(i), self.record(i));
                    (w.kind, w.height, w.heads, &wr.answer)
                        == (e.kind, e.height, e.heads, &rec.answer)
                },
                "rule TS-1: two writers of one memo key disagree: {key:?}"
            );
            return i;
        }
        e.answer = self.records.push(rec);
        let i = self.entries.push(e);
        assert!(i < BODY_ENTRY, "global memo over capacity");
        shard.insert(key, i);
        i
    }

    /// The interned key of an environment (§2.3): equal clause lists get
    /// equal keys; no clause is `EnvKey::EMPTY`. A run ID: it keys the
    /// memo, never a cache key or output.
    pub fn env_key(&self, env: &ParamEnv) -> EnvKey {
        if env.clause_self.is_empty() {
            return EnvKey::EMPTY;
        }
        let clauses = (
            env.clause_self.clone(),
            env.clause_trait.clone(),
            env.clause_args.clone(),
            env.clause_bindings.clone(),
            env.clause_mut.clone(),
            env.clause_origin.clone(),
        );
        let mut m = self.envs.lock().expect("envs");
        let next = EnvKey(u32::try_from(m.len() + 1).expect("envs"));
        *m.entry(clauses).or_insert(next)
    }

    fn count(&self, hit: bool) {
        let c = if hit { &self.hits } else { &self.misses };
        c.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn stats(&self) -> MemoStats {
        MemoStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
        }
    }
}

/// A body's own memo (§7.1): goals with placeholders, and the goals
/// whose answer is the body's alone (they met the depth cut, or read its
/// module's own impl rows). It also remembers the global entries the body
/// used, so a repeat takes no lock, and the proof nodes the body has been
/// charged for (`met`, rule TS-5). Nothing in it names a body-local type
/// or variable, so a trial's rollback leaves every entry valid (§2.2).
#[derive(Default, Debug)]
pub struct BodyMemo {
    table: HashMap<MemoKey, u32>,
    entries: Vec<MemoEntry>,
    records: Vec<MemoRecord>,
    met: HashSet<MemoKey>,
}

impl BodyMemo {
    /// The body's own entries (not the global ones it used).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// The body's own records.
    pub fn records(&self) -> impl Iterator<Item = &MemoRecord> {
        self.records.iter()
    }
    /// The slot a key maps to: a body entry (`BODY_ENTRY` set) or a
    /// global one.
    #[must_use]
    pub fn slot(&self, k: &MemoKey) -> Option<u32> {
        self.table.get(k).copied()
    }
}

/// A memo slot's entry and record.
fn slot_of<'x>(
    entries: &'x [MemoEntry],
    records: &'x [MemoRecord],
    global: &'x GlobalMemo,
    slot: u32,
) -> (MemoEntry, &'x MemoRecord) {
    if slot & BODY_ENTRY == 0 {
        (global.entry(slot), global.record(slot))
    } else {
        let e = entries[(slot & !BODY_ENTRY) as usize];
        (e, &records[e.answer as usize])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BuiltinImpl {
    Any,
    AnyVal,
    AnyRef,
    Inspectable,
    Tuple,
    Num,
    Integer,
    Float,
    Suspend,
}

/// What a `Holds` answer carries (§8.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    Impl { row: ImplRef, args: TyList },
    Bound { param: ParamRef, index: u16 },
    TraitValue { trait_: DefId },
    Builtin(BuiltinImpl),
    Poison,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainStep {
    pub impl_row: ImplRef,
    pub step: u16,
    pub origin: ImplOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailReason {
    NoImpl,
    Binding { name: Symbol, found: Ty },
    NotMutable,
    NotInspectable { arg: Ty },
    WrongCategory,
    Sealed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NearMiss {
    OtherInstantiation(ImplRef),
    NotAvailable(DefId),
    Derivable { trait_: DefId, target: DefId },
}

/// Why a goal failed: the root cause and the committed path (§10.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailInfo {
    pub leaf: CanonGoal,
    pub chain: Vec<ChainStep>,
    pub reason: FailReason,
    pub near: Vec<NearMiss>,
}

/// A solver answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Holds {
        evidence: Evidence,
        learned: Vec<(InferVar, Ty)>,
    },
    Normalized {
        ty: Ty,
    },
    Candidates(Vec<Candidate>),
    Fails(Box<FailInfo>),
    Stalled {
        on: Vec<InferVar>,
    },
    Overflow,
    OutOfFuel,
}

/// What codegen's `select` returns (§8.3): the impl and every impl argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub impl_row: ImplRef,
    pub args: TyList,
}

/// The solving context of one body, header check or derive instance.
pub struct SolveCx<'a> {
    pub pool: Types<'a>,
    pub env: &'a ParamEnv,
    /// The context's impl universe (§3.2): part of the key of every goal
    /// that may read the candidate directory.
    pub universe: ImplUniverseId,
    pub impls: Impls<'a>,
    pub body_memo: &'a mut BodyMemo,
    pub global: &'a GlobalMemo,
}

/// The solver's entry points (§1.3).
pub trait Solver: Sync {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: &Goal, fuel: &mut Fuel) -> StageResult<Answer>;
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey;
    fn select(
        &self,
        pool: Types<'_>,
        tables: &[(ModuleId, &ImplTable)],
        tref: ConcreteTraitRef,
    ) -> StageResult<Selection>;
    /// Codegen's entry for associated types at an instance: every
    /// projection in `t` with a concrete base, normalized.
    fn normalize_concrete(
        &self,
        pool: Types<'_>,
        tables: &[(ModuleId, &ImplTable)],
        t: Ty,
    ) -> StageResult<Ty>;
}

/// Canonicalizes a goal's self type and arguments (§2.2 steps 1 and 3):
/// variables become `Canon(i)` in first-occurrence order.
#[must_use]
pub fn canonicalize(
    pool: Types<'_>,
    kind: GoalKind,
    tref: TraitRef,
    mut_: bool,
) -> (CanonGoal, CanonVars) {
    let mut vars = CanonVars::default();
    let mut number = |t: Ty| {
        let TyData::Infer(v) = pool.get(t) else {
            return Some(t);
        };
        let i = vars.map.iter().position(|w| *w == v).unwrap_or_else(|| {
            vars.map.push(v);
            vars.kinds.push(VarKind::General);
            vars.map.len() - 1
        });
        Some(pool.intern_ty(&TyData::Canon(u8::try_from(i).unwrap_or(u8::MAX))))
    };
    let self_ty = fold_vars(pool, tref.self_ty, false, &mut number).expect("numbering");
    let args = fold_list(pool, tref.args, false, &mut number).expect("numbering");
    let goal = CanonGoal {
        kind,
        mut_,
        n_vars: u8::try_from(vars.map.len()).unwrap_or(u8::MAX),
        trait_: tref.trait_,
        self_ty,
        args,
        extra: 0,
    };
    (goal, vars)
}

/// Rebuilds `t` with every inference variable (`canon` false) or every
/// placeholder (`canon` true) replaced by `f`, in a fixed pre-order walk
/// over every type form; `None` when `f` refuses one.
fn fold_vars(
    pool: Types<'_>,
    t: Ty,
    canon: bool,
    f: &mut dyn FnMut(Ty) -> Option<Ty>,
) -> Option<Ty> {
    let has = if canon {
        pool.has_canon(t)
    } else {
        pool.has_infer(t)
    };
    if !has {
        return Some(t);
    }
    let d = match pool.get(t) {
        TyData::Infer(_) | TyData::Canon(_) => return f(t),
        TyData::Adt { def, args } => TyData::Adt {
            def,
            args: fold_list(pool, args, canon, f)?,
        },
        TyData::Tuple { elems, rest } => TyData::Tuple {
            elems: fold_list(pool, elems, canon, f)?,
            rest: match rest {
                Some(r) => Some(fold_vars(pool, r, canon, f)?),
                None => None,
            },
        },
        TyData::Option(i) => TyData::Option(fold_vars(pool, i, canon, f)?),
        TyData::Mut(i) => TyData::Mut(fold_vars(pool, i, canon, f)?),
        TyData::Fn {
            params,
            result,
            row,
            suspends,
        } => TyData::Fn {
            params: fold_list(pool, params, canon, f)?,
            result: fold_vars(pool, result, canon, f)?,
            row: fold_row(pool, row, canon, f)?,
            suspends,
        },
        TyData::TraitValue {
            def,
            args,
            bindings,
        } => TyData::TraitValue {
            def,
            args: fold_list(pool, args, canon, f)?,
            bindings: bindings
                .into_iter()
                .map(|(k, b)| Some((k, fold_vars(pool, b, canon, f)?)))
                .collect::<Option<Vec<_>>>()?,
        },
        TyData::Assoc {
            assoc,
            trait_,
            self_ty,
            args,
        } => TyData::Assoc {
            assoc,
            trait_,
            self_ty: fold_vars(pool, self_ty, canon, f)?,
            args: fold_list(pool, args, canon, f)?,
        },
        TyData::Row(r) => TyData::Row(fold_row(pool, r, canon, f)?),
        other => other,
    };
    Some(pool.intern_ty(&d))
}

fn fold_list(
    pool: Types<'_>,
    l: TyList,
    canon: bool,
    f: &mut dyn FnMut(Ty) -> Option<Ty>,
) -> Option<TyList> {
    let items = pool.list_items(l);
    let has = |t: &Ty| {
        if canon {
            pool.has_canon(*t)
        } else {
            pool.has_infer(*t)
        }
    };
    if !items.iter().any(has) {
        return Some(l);
    }
    let mut out = Vec::with_capacity(items.len());
    for t in items {
        out.push(fold_vars(pool, *t, canon, f)?);
    }
    Some(pool.list(&out))
}

fn fold_row(
    pool: Types<'_>,
    r: crate::pool::RowId,
    canon: bool,
    f: &mut dyn FnMut(Ty) -> Option<Ty>,
) -> Option<crate::pool::RowId> {
    let mut d = pool.row_data(r);
    let has = |t: &Ty| {
        if canon {
            pool.has_canon(*t)
        } else {
            pool.has_infer(*t)
        }
    };
    if !d.keys.iter().any(has) {
        return Some(r);
    }
    for k in &mut d.keys {
        *k = fold_vars(pool, *k, canon, f)?;
    }
    Some(pool.row(&d))
}

/// An answer's types over the goal's placeholders (§7.1 "Remapping");
/// `None` when it names a variable the goal does not, which no answer
/// does (the solver makes no variable).
fn canon_answer(pool: Types<'_>, a: &Answer, vars: &CanonVars) -> Option<CanonAnswer> {
    let index = |v: InferVar| {
        vars.map
            .iter()
            .position(|w| *w == v)
            .and_then(|i| u8::try_from(i).ok())
    };
    let mut place = |t: Ty| match pool.get(t) {
        TyData::Infer(v) => Some(pool.intern_ty(&TyData::Canon(index(v)?))),
        _ => Some(t),
    };
    Some(match a {
        Answer::Holds { evidence, learned } => CanonAnswer::Holds {
            evidence: match evidence {
                Evidence::Impl { row, args } => Evidence::Impl {
                    row: *row,
                    args: fold_list(pool, *args, false, &mut place)?,
                },
                other => other.clone(),
            },
            learned: learned
                .iter()
                .map(|(v, t)| Some((index(*v)?, fold_vars(pool, *t, false, &mut place)?)))
                .collect::<Option<_>>()?,
        },
        Answer::Candidates(cands) => CanonAnswer::Candidates(
            cands
                .iter()
                .map(|c| {
                    Some(Candidate {
                        impl_args: fold_list(pool, c.impl_args, false, &mut place)?,
                        args: fold_list(pool, c.args, false, &mut place)?,
                        ..c.clone()
                    })
                })
                .collect::<Option<_>>()?,
        ),
        Answer::Fails(info) => {
            // The leaf is canonical already; a reason's type is global.
            let global = match &info.reason {
                FailReason::Binding { found: t, .. } | FailReason::NotInspectable { arg: t } => {
                    !pool.has_infer(*t)
                }
                _ => true,
            };
            if !global {
                return None;
            }
            CanonAnswer::Fails(info.clone())
        }
        Answer::Stalled { on } => CanonAnswer::Stalled {
            on: on.iter().map(|v| index(*v)).collect::<Option<_>>()?,
        },
        Answer::Overflow => CanonAnswer::Overflow,
        Answer::Normalized { .. } | Answer::OutOfFuel => return None,
    })
}

/// A stored answer in the asking goal's variables.
fn decanon_answer(pool: Types<'_>, a: &CanonAnswer, vars: &CanonVars) -> Answer {
    let var = |i: u8| vars.map[usize::from(i)];
    let mut back = |t: Ty| match pool.get(t) {
        TyData::Canon(i) => Some(pool.intern_ty(&TyData::Infer(var(i)))),
        _ => Some(t),
    };
    let list = |l: TyList, f: &mut dyn FnMut(Ty) -> Option<Ty>| {
        fold_list(pool, l, true, f).expect("placeholders")
    };
    match a {
        CanonAnswer::Holds { evidence, learned } => Answer::Holds {
            evidence: match evidence {
                Evidence::Impl { row, args } => Evidence::Impl {
                    row: *row,
                    args: list(*args, &mut back),
                },
                other => other.clone(),
            },
            learned: learned
                .iter()
                .map(|(i, t)| {
                    let t = fold_vars(pool, *t, true, &mut back).expect("placeholders");
                    (var(*i), t)
                })
                .collect(),
        },
        CanonAnswer::Candidates(cands) => Answer::Candidates(
            cands
                .iter()
                .map(|c| Candidate {
                    impl_args: list(c.impl_args, &mut back),
                    args: list(c.args, &mut back),
                    ..c.clone()
                })
                .collect(),
        ),
        CanonAnswer::Fails(info) => Answer::Fails(info.clone()),
        CanonAnswer::Stalled { on } => Answer::Stalled {
            on: on.iter().map(|i| var(*i)).collect(),
        },
        CanonAnswer::Overflow => Answer::Overflow,
    }
}

impl CanonAnswer {
    fn kind(&self) -> MemoKind {
        match self {
            CanonAnswer::Holds { .. } | CanonAnswer::Candidates(_) => MemoKind::Holds,
            CanonAnswer::Fails(_) => MemoKind::Fails,
            CanonAnswer::Stalled { .. } => MemoKind::Stalled,
            CanonAnswer::Overflow => MemoKind::Overflow,
        }
    }
}

/// The outcome of matching one impl head against a goal's type (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum M {
    Yes,
    Maybe,
    No,
}

fn both(a: M, b: M) -> M {
    match (a, b) {
        (M::No, _) | (_, M::No) => M::No,
        (M::Maybe, _) | (_, M::Maybe) => M::Maybe,
        _ => M::Yes,
    }
}

/// Matches an impl head pattern (its parameters owned by `owner`) against
/// a goal type, binding the parameters. A variable in the goal where the
/// pattern has structure is a "maybe": the goal stalls on it.
fn match_ty(pool: Types<'_>, owner: DefId, pat: Ty, t: Ty, binds: &mut Vec<Option<Ty>>) -> M {
    if let TyData::Param(p) = pool.get(pat)
        && p.owner == owner
    {
        let i = p.index as usize;
        if binds.len() <= i {
            binds.resize(i + 1, None);
        }
        return match binds[i] {
            None => {
                binds[i] = Some(t);
                M::Yes
            }
            Some(b) if b == t => M::Yes,
            // A bare goal variable learns the bound argument.
            Some(_) if matches!(pool.get(t), TyData::Infer(_)) => M::Yes,
            Some(b) if pool.has_infer(b) || pool.has_infer(t) => M::Maybe,
            Some(_) => M::No,
        };
    }
    if pat == t {
        return M::Yes;
    }
    match (pool.get(pat), pool.get(t)) {
        (_, TyData::Poison) => M::Yes,
        (_, TyData::Infer(_)) => M::Maybe,
        (TyData::Mut(p), _) => match_ty(pool, owner, p, t, binds),
        (_, TyData::Mut(x)) => match_ty(pool, owner, pat, x, binds),
        (TyData::Adt { def: d1, args: a1 }, TyData::Adt { def: d2, args: a2 }) if d1 == d2 => {
            match_list(pool, owner, a1, a2, binds)
        }
        (
            TyData::Tuple {
                elems: e1,
                rest: None,
            },
            TyData::Tuple {
                elems: e2,
                rest: None,
            },
        ) => match_list(pool, owner, e1, e2, binds),
        (TyData::Option(p), TyData::Option(x)) => match_ty(pool, owner, p, x, binds),
        (
            TyData::Fn {
                params: p1,
                result: r1,
                suspends: s1,
                ..
            },
            TyData::Fn {
                params: p2,
                result: r2,
                suspends: s2,
                ..
            },
        ) if s1 == s2 => both(
            match_list(pool, owner, p1, p2, binds),
            match_ty(pool, owner, r1, r2, binds),
        ),
        (
            TyData::TraitValue {
                def: d1, args: a1, ..
            },
            TyData::TraitValue {
                def: d2, args: a2, ..
            },
        ) if d1 == d2 => match_list(pool, owner, a1, a2, binds),
        _ => M::No,
    }
}

fn match_list(
    pool: Types<'_>,
    owner: DefId,
    l1: TyList,
    l2: TyList,
    binds: &mut Vec<Option<Ty>>,
) -> M {
    let (x, y) = (pool.list_items(l1), pool.list_items(l2));
    if x.len() != y.len() {
        return M::No;
    }
    let mut r = M::Yes;
    for (&p, &t) in x.iter().zip(y) {
        r = both(r, match_ty(pool, owner, p, t, binds));
        if r == M::No {
            return r;
        }
    }
    r
}

/// Whether a numeric-family head can match `self_ty` (§3.9). A target
/// that is a bare impl parameter, as in `impl[N < Num] Add for N`,
/// stands for one impl per type its bound's trait lists
/// (`trait.target.numeric-family.each`), so it matches only those types:
/// those a head of the bound's trait matches. A parameter of the asking
/// item matches, and the bound step asks its environment. Every other
/// head is decided by `match_ty` alone.
fn family_excludes(
    pool: Types<'_>,
    impls: Impls<'_>,
    t: &ImplTable,
    r: usize,
    self_ty: Ty,
    reads: &mut Reads,
) -> bool {
    let TyData::Param(p) = pool.get(t.head_self[r]) else {
        return false;
    };
    if p.owner != t.def[r] || matches!(pool.get(self_ty), TyData::Param(_)) {
        return false;
    }
    t.plan[r].iter().any(|step| match step {
        PlanStep::Bound { param, trait_, .. } if u16::from(*param) == p.index => !reads
            .candidates(impls, pool, *trait_, self_ty, &[], false)
            .into_iter()
            .any(|(at, bt)| {
                let br = at.row as usize;
                match_ty(pool, bt.def[br], bt.head_self[br], self_ty, &mut Vec::new()) == M::Yes
            }),
        _ => false,
    })
}

/// The table solver: poison holds; a parameter's bound is found in the
/// elaborated environment; otherwise impl heads are matched (§3.4) with
/// their bound plans (§3.6) solved recursively. A goal whose type is still
/// open where a head needs structure stalls. `Instantiations` and
/// `Methods` answer candidate schemes (§6.5); the projection goal is not
/// implemented.
#[derive(Default)]
pub struct TableSolver;

/// The impl arguments a head match bound, then the plan's bound steps
/// substituted with them, each with its step index in the plan.
#[must_use]
pub fn plan_goals(pool: Types<'_>, t: &ImplTable, row: usize, args: &[Ty]) -> Vec<(u16, TraitRef)> {
    let owner = t.def[row];
    let s = |x: Ty| {
        pool.subst(x, &|p: ParamRef| {
            (p.owner == owner)
                .then(|| args.get(p.index as usize).copied())
                .flatten()
        })
    };
    (0u16..)
        .zip(&t.plan[row])
        .filter_map(|(i, step)| match step {
            PlanStep::Bound {
                param,
                trait_,
                args: targs,
                ..
            } => Some((
                i,
                TraitRef {
                    trait_: *trait_,
                    self_ty: args.get(*param as usize).copied()?,
                    args: pool.list(
                        &pool
                            .list_items(*targs)
                            .iter()
                            .copied()
                            .map(s)
                            .collect::<Vec<_>>(),
                    ),
                },
            )),
            PlanStep::Bind { .. } => None,
        })
        .collect()
}

/// The impl probes of one frame (§3.2, §7.1): whether it read the
/// candidate directory or unowned rows, and which `(trait, head key)`
/// buckets it probed.
#[derive(Default)]
struct Reads {
    seen: UniverseReads,
    keys: Vec<(DefId, HeadKey)>,
}

impl Reads {
    fn candidates<'a>(
        &mut self,
        impls: Impls<'a>,
        pool: Types<'_>,
        trait_: DefId,
        self_ty: Ty,
        args: &[Ty],
        open: bool,
    ) -> Vec<(ImplRef, &'a ImplTable)> {
        self.keys.push((trait_, HeadKey::of(pool, self_ty)));
        impls.candidates(pool, trait_, self_ty, args, open, &mut self.seen)
    }
}

/// The goals the search memoizes, with their `mut_` flag.
#[derive(Clone, Copy)]
enum Ask {
    Implements(TraitRef, bool),
    /// The self type with its outer `mut` stripped, and the trait.
    Instantiations(Ty, DefId, bool),
}

/// A goal's outcome at one depth (§7.3).
struct Outcome {
    answer: Answer,
    /// Levels used, the goal itself included; `CYCLE` for a cycle.
    height: u8,
    /// The answer met the depth cut: an `Overflow` that more levels could
    /// change, so it is the body's alone (rule TS-4).
    cut: bool,
}

impl Outcome {
    fn leaf(answer: Answer) -> Self {
        Outcome {
            answer,
            height: 1,
            cut: false,
        }
    }
}

/// A parent's height from a child's (§7.3).
fn above(h: u8) -> u8 {
    if h == CYCLE {
        CYCLE
    } else {
        h.saturating_add(1).min(CYCLE - 1)
    }
}

/// One `solve` call (§6, §7): the goals in progress, for cycles (§6.3),
/// and the scratch lists of the children and probes of the frames on the
/// stack, each frame's at the end.
struct Search<'s, 'a> {
    cx: &'s mut SolveCx<'a>,
    fuel: &'s mut Fuel,
    env: EnvKey,
    stack: Vec<MemoKey>,
    children: Vec<u32>,
    reads: Vec<(DefId, HeadKey)>,
    /// How many frames so far read rows the impl universe decides, or
    /// reused an entry that did: a goal whose subtree changed it is
    /// `scoped`.
    scoped: u32,
    walk: Vec<u32>,
}

impl<'s, 'a> Search<'s, 'a> {
    fn new(cx: &'s mut SolveCx<'a>, fuel: &'s mut Fuel) -> Self {
        let env = match cx.env.key {
            Some(k) => {
                debug_assert_eq!(k, cx.global.env_key(cx.env), "a stale EnvKey");
                k
            }
            None => cx.global.env_key(cx.env),
        };
        Search {
            cx,
            fuel,
            env,
            stack: Vec::new(),
            children: Vec::new(),
            reads: Vec::new(),
            scoped: 0,
            walk: Vec::new(),
        }
    }

    /// Adds a frame's probes to its goal's subtree.
    fn read(&mut self, reads: &Reads) {
        self.reads.extend_from_slice(&reads.keys);
        if reads.seen.dir || reads.seen.unowned {
            self.scoped += 1;
        }
    }

    /// Asks one goal at `depth` (0 at the use): the memo first, else the
    /// goal is computed and its entry published (rule TS-4). `memo` false
    /// asks without a lookup or an entry.
    fn goal(&mut self, ask: Ask, depth: u32, root: bool, memo: bool) -> StageResult<Outcome> {
        let pool = self.cx.pool;
        let (kind, tref, mut_) = match ask {
            Ask::Implements(t, m) => (GoalKind::Implements, t, m),
            Ask::Instantiations(s, tr, m) => (
                GoalKind::Instantiations,
                TraitRef {
                    trait_: tr,
                    self_ty: s,
                    args: TyList::EMPTY,
                },
                m,
            ),
        };
        let (cg, vars) = canonicalize(pool, kind, tref, mut_);
        // More variables than placeholders: asked, never memoized (§7.5).
        let memo = memo && vars.map.len() < usize::from(u8::MAX);
        // §2.2's scopes: a goal with no placeholder is `Global`, or `Env`
        // when it names a parameter; only those may go global.
        let params = pool.has_param(cg.self_ty)
            || pool.list_items(cg.args).iter().any(|a| pool.has_param(*a));
        let env = if params { self.env } else { EnvKey::EMPTY };
        let key = MemoKey::new(pool, cg, env, self.cx.universe, 0);
        let global = cg.n_vars == 0;
        // Rule TS-7: a goal that meets itself on the stack is a cycle;
        // every frame from it up answers `Overflow`, at any depth.
        if self.stack.contains(&key) {
            return Ok(Outcome {
                answer: Answer::Overflow,
                height: CYCLE,
                cut: false,
            });
        }
        if depth >= LEVELS {
            return Ok(Outcome {
                answer: Answer::Overflow,
                height: 1,
                cut: true,
            });
        }
        if memo {
            if let Some(slot) = self.find(&key, global)
                && let Some(out) = self.reuse(slot, &vars, depth, root)
            {
                self.cx.global.count(true);
                return Ok(out);
            }
            self.cx.global.count(false);
        }
        let (cstart, rstart, sstart) = (self.children.len(), self.reads.len(), self.scoped);
        self.stack.push(key);
        let r = match ask {
            Ask::Implements(t, _) => self.implements(&key, cg, t, depth, root),
            Ask::Instantiations(s, tr, _) => self.instantiations(&key, s, tr, depth, root),
        };
        self.stack.pop();
        let (out, heads) = r?;
        let mut children: Vec<u32> = Vec::with_capacity(self.children.len() - cstart);
        for c in self.children.drain(cstart..) {
            if !children.contains(&c) {
                children.push(c);
            }
        }
        let mut reads = self.reads.split_off(rstart);
        reads.sort_unstable_by_key(|(t, k)| (t.raw(), k.code()));
        reads.dedup();
        self.reads.extend_from_slice(&reads);
        if !memo || matches!(out.answer, Answer::OutOfFuel) {
            return Ok(out);
        }
        let Some(answer) = canon_answer(pool, &out.answer, &vars) else {
            return Ok(out);
        };
        // §7.3: a goal that met the cut stores how many levels did not
        // suffice; any other stores its answer and height.
        let (kind, height) = if out.cut {
            let left = u8::try_from(LEVELS - depth).expect("levels");
            (MemoKind::AtLeast, left)
        } else {
            (answer.kind(), out.height)
        };
        let entry = MemoEntry {
            answer: 0,
            children: u32::try_from(children.len()).expect("children"),
            height,
            kind,
            heads,
        };
        let to_global = global && !out.cut && self.own_free(&reads);
        let rec = MemoRecord {
            key,
            answer,
            children: children.into(),
            reads: reads.into(),
            scoped: self.scoped != sstart,
        };
        let slot = if to_global {
            self.cx.global.publish(rec, entry, self.cx.universe)
        } else {
            let bm = &mut *self.cx.body_memo;
            bm.entries.push(MemoEntry {
                answer: u32::try_from(bm.records.len()).expect("memo"),
                ..entry
            });
            bm.records.push(rec);
            u32::try_from(bm.entries.len() - 1).expect("memo") | BODY_ENTRY
        };
        self.cx.body_memo.table.insert(key, slot);
        self.children.push(slot);
        Ok(out)
    }

    /// The slot that answers `key` here: the body's table, then, for a goal
    /// with no placeholder, the global memo, whose entry serves only when
    /// this module's own table has no row for its probes. A key without a
    /// universe finds an unscoped entry, which serves only when this
    /// universe's unowned rows have none for its probes either (an answer
    /// that read none is the same in every such universe), else the entry
    /// scoped to this universe (`MemoRecord::global_key`).
    fn find(&mut self, key: &MemoKey, global: bool) -> Option<u32> {
        if let Some(s) = self.cx.body_memo.table.get(key) {
            return Some(*s);
        }
        if !global {
            return None;
        }
        let memo = self.cx.global;
        let unscoped = key.universe.is_none();
        let serves = |i: u32| {
            let rec = memo.record(i);
            debug_assert!(!unscoped || !rec.scoped, "a scoped entry under {key:?}");
            self.own_free(&rec.reads) && (!unscoped || self.unowned_free(&rec.reads))
        };
        let i = match memo.lookup(key) {
            Some(i) if serves(i) => i,
            _ if unscoped => {
                let scoped = MemoKey {
                    universe: Some(self.cx.universe),
                    ..*key
                };
                memo.lookup(&scoped)
                    .filter(|i| self.own_free(&memo.record(*i).reads))?
            }
            _ => return None,
        };
        self.cx.body_memo.table.insert(*key, i);
        Some(i)
    }

    /// Whether this universe's unowned rows have none for any of `reads`.
    fn unowned_free(&self, reads: &[(DefId, HeadKey)]) -> bool {
        reads.iter().all(|(t, k)| !self.cx.impls.unowned_at(*t, *k))
    }

    /// Whether this module's own table has no row for any of `reads`: then
    /// an entry with those probes is its answer too (lookup.rs `OWN_TABLE`).
    fn own_free(&self, reads: &[(DefId, HeadKey)]) -> bool {
        let Impls::Owned(v) = self.cx.impls else {
            return true;
        };
        let Some(own) = v.own.filter(|o| !o.def.is_empty()) else {
            return true;
        };
        reads
            .iter()
            .all(|(t, k)| own.candidates(*t, *k).next().is_none())
    }

    /// An entry's outcome at `depth` (§7.3), its proof DAG charged (rule
    /// TS-5). `None` when the entry met the cut with fewer levels than this
    /// use has: the goal is computed again.
    fn reuse(&mut self, slot: u32, vars: &CanonVars, depth: u32, root: bool) -> Option<Outcome> {
        let pool = self.cx.pool;
        let global = self.cx.global;
        let bm = &*self.cx.body_memo;
        let (e, rec) = slot_of(&bm.entries, &bm.records, global, slot);
        let left = LEVELS - depth;
        let overflow = |height, cut| Outcome {
            answer: Answer::Overflow,
            height,
            cut,
        };
        let out = match e.kind {
            MemoKind::AtLeast if left > u32::from(e.height) => return None,
            MemoKind::AtLeast => overflow(1, true),
            _ if e.height == CYCLE => overflow(CYCLE, false),
            _ if depth + u32::from(e.height) > LEVELS => overflow(e.height, true),
            _ => Outcome {
                answer: decanon_answer(pool, &rec.answer, vars),
                height: e.height,
                cut: false,
            },
        };
        self.reads.extend_from_slice(&rec.reads);
        if rec.scoped {
            self.scoped += 1;
        }
        self.children.push(slot);
        if !self.charge_dag(slot, root) {
            return Some(Outcome::leaf(Answer::OutOfFuel));
        }
        Some(out)
    }

    /// Rule TS-5: charges the proof DAG under `slot`, depth first in plan
    /// order. A node the body has not paid for costs one plus the heads its
    /// probe matched, and joins `met` before its children are walked; a
    /// node already met costs nothing and is not entered, except that the
    /// asked goal costs one. False when the fuel runs out.
    fn charge_dag(&mut self, slot: u32, root: bool) -> bool {
        let global = self.cx.global;
        let bm = &mut *self.cx.body_memo;
        self.walk.clear();
        self.walk.push(slot);
        let mut first = root;
        while let Some(s) = self.walk.pop() {
            let (e, rec) = slot_of(&bm.entries, &bm.records, global, s);
            let cost = if bm.met.insert(rec.key) {
                self.walk.extend(rec.children.iter().rev());
                1 + u64::from(e.heads)
            } else {
                u64::from(first)
            };
            first = false;
            if cost > 0 && !self.fuel.charge(cost) {
                return false;
            }
        }
        true
    }

    /// Rule TS-5 for a goal computed now: the walk's charge, made when its
    /// probe is done and before its children are asked.
    fn charge(&mut self, key: &MemoKey, heads: u16, root: bool) -> bool {
        let cost = if self.cx.body_memo.met.insert(*key) {
            1 + u64::from(heads)
        } else {
            u64::from(root)
        };
        cost == 0 || self.fuel.charge(cost)
    }

    /// A goal answered before any impl probe.
    fn leaf(&mut self, key: &MemoKey, root: bool, answer: Answer) -> (Outcome, u16) {
        let answer = if self.charge(key, 0, root) {
            answer
        } else {
            Answer::OutOfFuel
        };
        (Outcome::leaf(answer), 0)
    }

    /// `Implements` (§3): poison holds; a parameter's bound is found in the
    /// elaborated environment; otherwise impl heads are matched (§3.4) and
    /// the one committed head's bound plan is asked (§3.6). Returns the
    /// outcome and the heads the probe matched.
    fn implements(
        &mut self,
        key: &MemoKey,
        cg: CanonGoal,
        tref: TraitRef,
        depth: u32,
        root: bool,
    ) -> StageResult<(Outcome, u16)> {
        let pool = self.cx.pool;
        let impls = self.cx.impls;
        if pool.has_poison(tref.self_ty) {
            let holds = Answer::Holds {
                evidence: Evidence::Poison,
                learned: vec![],
            };
            return Ok(self.leaf(key, root, holds));
        }
        let self_ty = match pool.get(tref.self_ty) {
            TyData::Mut(i) => i,
            _ => tref.self_ty,
        };
        // The environment: a clause on the same parameter and trait.
        if let TyData::Param(param) = pool.get(self_ty) {
            let env = self.cx.env;
            for i in 0..env.clause_self.len() {
                if env.clause_self[i] == self_ty && env.clause_trait[i] == tref.trait_ {
                    let mut binds = Vec::new();
                    let m =
                        match_list(pool, DefId::NONE, env.clause_args[i], tref.args, &mut binds);
                    if m == M::No {
                        continue;
                    }
                    let learned = learned_from(pool, env.clause_args[i], tref.args);
                    let index = u16::try_from(i).unwrap_or(u16::MAX);
                    let holds = Answer::Holds {
                        evidence: Evidence::Bound { param, index },
                        learned,
                    };
                    return Ok(self.leaf(key, root, holds));
                }
            }
        }
        if let TyData::Infer(v) = pool.get(self_ty) {
            return Ok(self.leaf(key, root, Answer::Stalled { on: vec![v] }));
        }
        let goal_args = pool.list_items(tref.args);
        let open = goal_args.iter().any(|a| open_arg(pool, *a));
        // The frame's probes and its "read the directory" bit (§3.2).
        let mut reads = Reads::default();
        let mut yes = Vec::new();
        let mut maybes = 0usize;
        let cands = reads.candidates(impls, pool, tref.trait_, self_ty, goal_args, open);
        let heads = u16::try_from(cands.len()).unwrap_or(u16::MAX);
        for (at, t) in cands {
            let r = at.row as usize;
            if t.origin[r] == ImplOrigin::TupleTemplate {
                if matches!(pool.get(self_ty), TyData::Tuple { .. }) {
                    yes.push((at, Vec::new(), t));
                }
                continue;
            }
            if family_excludes(pool, impls, t, r, self_ty, &mut reads) {
                continue;
            }
            let owner = t.def[r];
            let mut binds = Vec::new();
            let a = match_ty(pool, owner, t.head_self[r], self_ty, &mut binds);
            // A bare variable among the goal's arguments learns the head's.
            let hs = pool.list_items(t.head_args[r]);
            let b = if hs.len() == goal_args.len() {
                hs.iter().zip(goal_args).fold(M::Yes, |acc, (h, g)| {
                    if matches!(pool.get(*g), TyData::Infer(_)) {
                        acc
                    } else {
                        both(acc, match_ty(pool, owner, *h, *g, &mut binds))
                    }
                })
            } else {
                M::No
            };
            match both(a, b) {
                M::Yes => {
                    let n = usize::from(t.n_params[r]).max(binds.len());
                    let mut fixed: Vec<Option<Ty>> =
                        (0..n).map(|i| binds.get(i).copied().flatten()).collect();
                    apply_binds_in(pool, impls, t, r, &mut fixed, &mut reads);
                    let args: Vec<Ty> =
                        fixed.into_iter().map(|a| a.unwrap_or(Ty::POISON)).collect();
                    yes.push((at, args, t));
                }
                M::Maybe => maybes += 1,
                M::No => {}
            }
        }
        if reads.seen.dir && key.universe.is_none() {
            return Err(NotImplemented::new(
                Stage::Body,
                "internal error: a solver frame read the candidate directory, but its memo key has no impl universe",
            ));
        }
        self.read(&reads);
        if !self.charge(key, heads, root) {
            return Ok((Outcome::leaf(Answer::OutOfFuel), heads));
        }
        // Committing (§3.5, rule TS-2): the heads alone pick the impl,
        // before any bound is solved. A goal with open variables that
        // more than one head could still answer stalls, so inference can
        // learn more; it never tries each head's bounds. Two heads that
        // match a goal with no open variable exist only beside an overlap
        // error (§5.4): the first in content order is taken.
        let open_goal = pool.has_infer(self_ty) || goal_args.iter().any(|a| pool.has_infer(*a));
        if yes.is_empty() && maybes == 0 && !pool.has_infer(self_ty) {
            let fails = Answer::Fails(Box::new(FailInfo {
                leaf: cg,
                chain: vec![],
                reason: FailReason::NoImpl,
                near: vec![],
            }));
            return Ok((Outcome::leaf(fails), heads));
        }
        let several = yes.len() + maybes > 1;
        match yes.into_iter().next() {
            Some(head) if maybes == 0 && !(open_goal && several) => {
                Ok((self.commit(tref, head, depth)?, heads))
            }
            _ => {
                // The goal's variables, in first-occurrence order (§3.4).
                let mut all = Vec::new();
                collect_vars(pool, self_ty, &mut all);
                for a in goal_args {
                    collect_vars(pool, *a, &mut all);
                }
                let mut on = Vec::with_capacity(all.len());
                for v in all {
                    if !on.contains(&v) {
                        on.push(v);
                    }
                }
                Ok((Outcome::leaf(Answer::Stalled { on }), heads))
            }
        }
    }

    /// Solves the bound plan of the one impl a goal committed to (§3.6).
    /// The first step that does not hold decides; a failing step fails
    /// the goal, with this impl added to the front of its chain (§10.1).
    fn commit(
        &mut self,
        tref: TraitRef,
        (row, args, t): (ImplRef, Vec<Ty>, &ImplTable),
        depth: u32,
    ) -> StageResult<Outcome> {
        let pool = self.cx.pool;
        let r = row.row as usize;
        let mut height = 1;
        for (step, sub) in plan_goals(pool, t, r, &args) {
            let o = self.goal(Ask::Implements(sub, false), depth + 1, false, true)?;
            height = height.max(above(o.height));
            match o.answer {
                Answer::Holds { .. } => {}
                Answer::Fails(mut info) => {
                    info.chain.insert(
                        0,
                        ChainStep {
                            impl_row: row,
                            step,
                            origin: t.origin[r],
                        },
                    );
                    return Ok(Outcome {
                        answer: Answer::Fails(info),
                        height,
                        cut: false,
                    });
                }
                other => {
                    return Ok(Outcome {
                        answer: other,
                        height,
                        cut: o.cut,
                    });
                }
            }
        }
        let owner = t.def[r];
        let learned = learned_from(pool, t.head_args[r], tref.args)
            .into_iter()
            .map(|(v, x)| {
                let x = pool.subst(x, &|p: ParamRef| {
                    (p.owner == owner)
                        .then(|| args.get(p.index as usize).copied())
                        .flatten()
                });
                (v, x)
            })
            .collect();
        Ok(Outcome {
            answer: Answer::Holds {
                evidence: Evidence::Impl {
                    row,
                    args: pool.list(&args),
                },
                learned,
            },
            height,
            cut: false,
        })
    }
}

fn collect_vars(pool: Types<'_>, t: Ty, out: &mut Vec<InferVar>) {
    if !pool.has_infer(t) {
        return;
    }
    match pool.get(t) {
        TyData::Infer(v) => out.push(v),
        TyData::Adt { args, .. } | TyData::TraitValue { args, .. } => {
            for a in pool.list_items(args).iter().copied() {
                collect_vars(pool, a, out);
            }
        }
        TyData::Tuple { elems, .. } => {
            for a in pool.list_items(elems).iter().copied() {
                collect_vars(pool, a, out);
            }
        }
        TyData::Option(i) | TyData::Mut(i) => collect_vars(pool, i, out),
        TyData::Fn { params, result, .. } => {
            for a in pool.list_items(params).iter().copied() {
                collect_vars(pool, a, out);
            }
            collect_vars(pool, result, out);
        }
        _ => {}
    }
}

/// Goal arguments that are bare variables learn the matching head
/// argument (§8.1 `learned`).
fn learned_from(pool: Types<'_>, head: TyList, goal: TyList) -> Vec<(InferVar, Ty)> {
    pool.list_items(goal)
        .iter()
        .copied()
        .zip(pool.list_items(head).iter().copied())
        .filter_map(|(g, h)| match pool.get(g) {
            TyData::Infer(v) => Some((v, h)),
            _ => None,
        })
        .collect()
}

/// Whether `t` names an impl parameter of `owner` that `fixed` leaves open.
fn reads_fresh(pool: Types<'_>, owner: DefId, t: Ty, fixed: &[Option<Ty>]) -> bool {
    let hit = std::cell::Cell::new(false);
    let _ = pool.subst(t, &|p: ParamRef| {
        if p.owner == owner && fixed.get(p.index as usize).copied().flatten().is_none() {
            hit.set(true);
        }
        None
    });
    hit.get()
}

impl Search<'_, '_> {
    /// `Instantiations { S, Tr }` (§6.5): every impl head of `Tr` that
    /// matches `S` with all trait arguments open, as schemes in content
    /// order. The target fixes some impl parameters; the plan steps over
    /// those alone are solved here, and a candidate whose step fails is
    /// dropped. Steps that read a fresh parameter, or that stall on a
    /// variable of `S`, are the candidate's residual obligations.
    fn instantiations(
        &mut self,
        key: &MemoKey,
        self_ty: Ty,
        trait_: DefId,
        depth: u32,
        root: bool,
    ) -> StageResult<(Outcome, u16)> {
        let pool = self.cx.pool;
        let impls = self.cx.impls;
        match pool.get(self_ty) {
            TyData::Poison => {
                let holds = Answer::Holds {
                    evidence: Evidence::Poison,
                    learned: vec![],
                };
                return Ok(self.leaf(key, root, holds));
            }
            TyData::Infer(v) => return Ok(self.leaf(key, root, Answer::Stalled { on: vec![v] })),
            TyData::Param(_) | TyData::TraitValue { .. } => {
                return Err(NotImplemented::new(
                    Stage::Body,
                    "Instantiations of a parameter or a trait value (the checker reads its clauses)",
                ));
            }
            _ => {}
        }
        let mut reads = Reads::default();
        let mut out = Vec::new();
        let mut maybe = false;
        let cands = reads.candidates(impls, pool, trait_, self_ty, &[], true);
        let heads = u16::try_from(cands.len()).unwrap_or(u16::MAX);
        // The rows that match, with their impl arguments, before any step
        // is asked: the probe is charged first (rule TS-5).
        let mut matched = Vec::new();
        for (at, t) in cands {
            let r = at.row as usize;
            if t.origin[r] == ImplOrigin::TupleTemplate {
                if matches!(pool.get(self_ty), TyData::Tuple { .. }) {
                    matched.push((at, t, None));
                }
                continue;
            }
            if family_excludes(pool, impls, t, r, self_ty, &mut reads) {
                continue;
            }
            let owner = t.def[r];
            let mut binds = Vec::new();
            match match_ty(pool, owner, t.head_self[r], self_ty, &mut binds) {
                M::No => continue,
                M::Maybe => {
                    maybe = true;
                    continue;
                }
                M::Yes => {}
            }
            let n = usize::from(t.n_params[r]).max(binds.len());
            let mut fixed: Vec<Option<Ty>> =
                (0..n).map(|i| binds.get(i).copied().flatten()).collect();
            apply_binds_in(pool, impls, t, r, &mut fixed, &mut reads);
            matched.push((at, t, Some(fixed)));
        }
        if reads.seen.dir && key.universe.is_none() {
            return Err(NotImplemented::new(
                Stage::Body,
                "internal error: an Instantiations frame read the directory without a universe",
            ));
        }
        self.read(&reads);
        if !self.charge(key, heads, root) {
            return Ok((Outcome::leaf(Answer::OutOfFuel), heads));
        }
        let mut height = 1;
        for (at, t, fixed) in matched {
            let r = at.row as usize;
            let Some(fixed) = fixed else {
                out.push(Candidate {
                    row: at,
                    n_fresh: 0,
                    impl_args: TyList::EMPTY,
                    args: t.head_args[r],
                    residual: vec![],
                });
                continue;
            };
            let owner = t.def[r];
            let impl_args: Vec<Ty> = (0u16..)
                .zip(&fixed)
                .map(|(index, a)| {
                    a.unwrap_or_else(|| pool.intern_ty(&TyData::Param(ParamRef { owner, index })))
                })
                .collect();
            let goals = plan_goals(pool, t, r, &impl_args);
            let mut residual = Vec::new();
            let mut dropped = false;
            for (i, step) in (0u16..).zip(&t.plan[r]) {
                let PlanStep::Bound {
                    param, args: sargs, ..
                } = step
                else {
                    continue;
                };
                let open = fixed.get(usize::from(*param)).copied().flatten().is_none()
                    || pool
                        .list_items(*sargs)
                        .iter()
                        .any(|a| reads_fresh(pool, owner, *a, &fixed));
                if open {
                    residual.push(i);
                    continue;
                }
                let Some(&(_, sub)) = goals.iter().find(|(k, _)| *k == i) else {
                    continue;
                };
                let o = self.goal(Ask::Implements(sub, false), depth + 1, false, true)?;
                height = height.max(above(o.height));
                match o.answer {
                    Answer::Holds { .. } => {}
                    Answer::Fails(_) => {
                        dropped = true;
                        break;
                    }
                    Answer::Stalled { .. } => residual.push(i),
                    other => {
                        let o = Outcome {
                            answer: other,
                            height,
                            cut: o.cut,
                        };
                        return Ok((o, heads));
                    }
                }
            }
            if dropped {
                continue;
            }
            let args: Vec<Ty> = pool
                .list_items(t.head_args[r])
                .iter()
                .map(|a| {
                    pool.subst(*a, &|p: ParamRef| {
                        (p.owner == owner)
                            .then(|| impl_args.get(p.index as usize).copied())
                            .flatten()
                    })
                })
                .collect();
            out.push(Candidate {
                row: at,
                n_fresh: u8::try_from(fixed.iter().filter(|a| a.is_none()).count())
                    .unwrap_or(u8::MAX),
                impl_args: pool.list(&impl_args),
                args: pool.list(&args),
                residual,
            });
        }
        let done = |answer| {
            let o = Outcome {
                answer,
                height,
                cut: false,
            };
            Ok((o, heads))
        };
        if maybe {
            let mut on = Vec::new();
            collect_vars(pool, self_ty, &mut on);
            let mut seen = Vec::with_capacity(on.len());
            on.retain(|v| {
                let first = !seen.contains(v);
                seen.push(*v);
                first
            });
            return done(Answer::Stalled { on });
        }
        if out.is_empty() {
            let tref = TraitRef {
                trait_,
                self_ty,
                args: TyList::EMPTY,
            };
            let (leaf, _) = canonicalize(pool, GoalKind::Instantiations, tref, false);
            return done(Answer::Fails(Box::new(FailInfo {
                leaf,
                chain: vec![],
                reason: FailReason::NoImpl,
                near: vec![],
            })));
        }
        done(Answer::Candidates(out))
    }

    /// The trait part of `Methods` (§6.5 step 3): one `Instantiations`
    /// goal per trait, in the given order; a trait the receiver does not
    /// implement adds nothing. The answer lists every candidate (its
    /// row's trait tells them apart); none at all is an empty list. The
    /// `Methods` goal itself is not memoized: its key would need the
    /// module's availability, and its `Instantiations` goals are.
    fn methods(&mut self, receiver: Ty, traits: &[DefId]) -> StageResult<Answer> {
        let s = strip_outer_mut(self.cx.pool, receiver);
        let mut all = Vec::new();
        for tr in traits {
            match self
                .goal(Ask::Instantiations(s, *tr, false), 0, true, true)?
                .answer
            {
                Answer::Candidates(c) => all.extend(c),
                Answer::Fails(_) => {}
                other => return Ok(other),
            }
        }
        Ok(Answer::Candidates(all))
    }
}

fn strip_outer_mut(pool: Types<'_>, t: Ty) -> Ty {
    match pool.get(t) {
        TyData::Mut(i) => i,
        _ => t,
    }
}

impl Solver for TableSolver {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: &Goal, fuel: &mut Fuel) -> StageResult<Answer> {
        let pool = cx.pool;
        let mut s = Search::new(cx, fuel);
        match goal {
            // Bindings do not reach the answer yet, so a goal that has
            // some is asked without the memo, whose key leaves them out.
            Goal::Implements {
                tref,
                bindings,
                mut_,
            } => Ok(s
                .goal(Ask::Implements(*tref, *mut_), 0, true, bindings.is_empty())?
                .answer),
            Goal::Instantiations {
                self_ty,
                trait_,
                mut_,
            } => {
                let ask = Ask::Instantiations(strip_outer_mut(pool, *self_ty), *trait_, *mut_);
                Ok(s.goal(ask, 0, true, true)?.answer)
            }
            Goal::Methods {
                receiver, traits, ..
            } => s.methods(*receiver, traits),
            Goal::Project { .. } => Err(NotImplemented::new(
                Stage::Body,
                "the Project goal (the checker normalizes)",
            )),
        }
    }

    /// Elaborates the clauses. Their key is the run memo's
    /// (`GlobalMemo::env_key`), interned on the first solve; it is
    /// `EnvKey::EMPTY` for no bound and `EnvKey::UNSET` until then.
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey {
        for (i, b) in bounds.iter().enumerate() {
            out.clause_self.push(b.tref.self_ty);
            out.clause_trait.push(b.tref.trait_);
            out.clause_args.push(b.tref.args);
            out.clause_bindings.push(b.bindings.clone());
            out.clause_mut.push(b.mut_);
            out.clause_origin.push(u16::try_from(i).expect("bounds"));
        }
        out.key = None;
        if out.clause_self.is_empty() {
            EnvKey::EMPTY
        } else {
            EnvKey::UNSET
        }
    }

    fn select(
        &self,
        pool: Types<'_>,
        tables: &[(ModuleId, &ImplTable)],
        tref: ConcreteTraitRef,
    ) -> StageResult<Selection> {
        let env = ParamEnv::default();
        let global = GlobalMemo::default();
        let mut memo = BodyMemo::default();
        let mut cx = SolveCx {
            pool,
            env: &env,
            universe: ImplUniverseId(0),
            impls: Impls::All(tables),
            body_memo: &mut memo,
            global: &global,
        };
        let mut fuel = Fuel::new(Fuel::BODY_DEFAULT);
        let mut s = Search::new(&mut cx, &mut fuel);
        match s
            .goal(Ask::Implements(tref.0, false), 0, true, true)?
            .answer
        {
            Answer::Holds {
                evidence: Evidence::Impl { row, args },
                ..
            } => Ok(Selection {
                impl_row: row,
                args,
            }),
            other => Err(NotImplemented::new(
                Stage::Collect,
                format!("select found no impl: {other:?}"),
            )),
        }
    }

    fn normalize_concrete(
        &self,
        pool: Types<'_>,
        tables: &[(ModuleId, &ImplTable)],
        t: Ty,
    ) -> StageResult<Ty> {
        Ok(normalize_concrete(pool, Impls::All(tables), t))
    }
}

/// Replaces every projection in `t` whose base is concrete by the binding
/// of the impl whose head matches it (§4.3, codegen's mode): the checker
/// has proven the goal, so a head match is enough. A projection on a
/// parameter or a variable, or one no impl binds, is kept.
#[must_use]
pub fn normalize_concrete(pool: Types<'_>, impls: Impls<'_>, t: Ty) -> Ty {
    norm_concrete(pool, impls, t, 0, &mut Reads::default())
}

fn norm_list(
    pool: Types<'_>,
    impls: Impls<'_>,
    l: TyList,
    depth: u32,
    reads: &mut Reads,
) -> TyList {
    let mut out = Vec::with_capacity(pool.list_items(l).len());
    for x in pool.list_items(l).iter().copied() {
        out.push(norm_concrete(pool, impls, x, depth, reads));
    }
    pool.list(&out)
}

fn norm_concrete(pool: Types<'_>, impls: Impls<'_>, t: Ty, depth: u32, reads: &mut Reads) -> Ty {
    if !pool.has_assoc(t) || depth > 64 {
        return t;
    }
    let d = match pool.get(t) {
        TyData::Assoc {
            assoc,
            trait_,
            self_ty,
            args,
        } => {
            let s = norm_concrete(pool, impls, self_ty, depth, reads);
            let a = norm_list(pool, impls, args, depth, reads);
            if let Some(x) = project_concrete(pool, impls, assoc, trait_, s, a, reads) {
                return norm_concrete(pool, impls, x, depth + 1, reads);
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty: s,
                args: a,
            }
        }
        TyData::Adt { def, args } => TyData::Adt {
            def,
            args: norm_list(pool, impls, args, depth, reads),
        },
        TyData::Tuple { elems, rest } => TyData::Tuple {
            elems: norm_list(pool, impls, elems, depth, reads),
            rest: rest.map(|r| norm_concrete(pool, impls, r, depth, reads)),
        },
        TyData::Option(i) => TyData::Option(norm_concrete(pool, impls, i, depth, reads)),
        TyData::Mut(i) => TyData::Mut(norm_concrete(pool, impls, i, depth, reads)),
        TyData::Fn {
            params,
            result,
            row,
            suspends,
        } => TyData::Fn {
            params: norm_list(pool, impls, params, depth, reads),
            result: norm_concrete(pool, impls, result, depth, reads),
            row,
            suspends,
        },
        TyData::TraitValue {
            def,
            args,
            bindings,
        } => TyData::TraitValue {
            def,
            args: norm_list(pool, impls, args, depth, reads),
            bindings: bindings
                .into_iter()
                .map(|(k, b)| (k, norm_concrete(pool, impls, b, depth, reads)))
                .collect(),
        },
        _ => return t,
    };
    pool.intern_ty(&d)
}

/// One projection step on a concrete base: a trait value's binding, or
/// the binding of the impl whose head matches. Trait arguments left
/// implicit (`Self::Out`) are open, so the directory is read.
fn project_concrete(
    pool: Types<'_>,
    impls: Impls<'_>,
    assoc: DefId,
    trait_: DefId,
    self_ty: Ty,
    args: TyList,
    reads: &mut Reads,
) -> Option<Ty> {
    let base = match pool.get(self_ty) {
        TyData::Mut(i) => i,
        _ => self_ty,
    };
    if pool.has_param(base) || pool.has_infer(base) || pool.has_assoc(base) {
        return None;
    }
    if let TyData::TraitValue { bindings, .. } = pool.get(base) {
        return bindings.iter().find(|(k, _)| *k == assoc).map(|(_, b)| *b);
    }
    let goal_args = pool.list_items(args);
    let open = goal_args.iter().any(|a| open_arg(pool, *a))
        || (goal_args.is_empty() && impls.implicit_args(trait_, 0));
    let cands = reads.candidates(impls, pool, trait_, base, goal_args, open);
    for (at, t) in cands {
        let r = at.row as usize;
        let Some((_, b)) = t.assoc[r].iter().find(|(k, _)| *k == assoc) else {
            continue;
        };
        if family_excludes(pool, impls, t, r, base, reads) {
            continue;
        }
        let owner = t.def[r];
        let mut binds = Vec::new();
        if match_ty(pool, owner, t.head_self[r], base, &mut binds) != M::Yes {
            continue;
        }
        // `Self::Out` may leave the trait's arguments implicit.
        let head_args = pool.list_items(t.head_args[r]);
        if !goal_args.is_empty()
            && (goal_args.len() != head_args.len()
                || head_args
                    .iter()
                    .zip(goal_args)
                    .any(|(h, g)| match_ty(pool, owner, *h, *g, &mut binds) != M::Yes))
        {
            continue;
        }
        return Some(pool.subst(*b, &|p: ParamRef| {
            (p.owner == owner)
                .then(|| binds.get(p.index as usize).copied().flatten())
                .flatten()
        }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        BodyMemo, ConcreteTraitRef, GlobalMemo, Goal, GoalKind, HeadKey, ImplOrigin, ImplTable,
        ImplUniverses, MemoEntry, MemoKey, MemoKind, ParamEnv, SolveCx, Solver, TableSolver,
        TraitRef, canonicalize,
    };
    use crate::pool::{InternPool, LocalPool, Prim, Ty, TyData, TyList, Types};
    use crate::unify::{InferTable, VarKind};
    use hd_base::{DefId, FolderId, Fuel, ModuleId};

    #[test]
    fn universes_dedup_sorted_lists() {
        let u = ImplUniverses::default();
        let f = FolderId::from_raw;
        let x = super::FolderImpls::default();
        let a = u.intern(&[(f(2), &x), (f(1), &x)]).0;
        assert_eq!(a, u.intern(&[(f(1), &x), (f(2), &x), (f(2), &x)]).0);
        assert_ne!(a, u.intern(&[(f(1), &x)]).0);
        assert_eq!(u.len(), 2);
    }

    #[test]
    fn head_index_returns_matching_rows_in_row_order() {
        let tr = DefId::from_raw(5);
        let mut t = ImplTable::default();
        let keys = [
            HeadKey::Prim(Prim::I32),
            HeadKey::Param,
            HeadKey::Tuple(2),
            HeadKey::Prim(Prim::Bool),
            HeadKey::TupleAny,
            HeadKey::Prim(Prim::I32),
        ];
        for k in keys {
            t.trait_.push(tr);
            t.head_key.push(k);
            t.def.push(DefId::from_raw(6));
        }
        t.by_trait.insert(tr.raw(), (0, 6));
        t.index();
        let rows = |k| t.candidates(tr, k).collect::<Vec<_>>();
        assert_eq!(rows(HeadKey::Prim(Prim::I32)), [0, 1, 5]);
        assert_eq!(rows(HeadKey::Tuple(2)), [1, 2, 4]);
        assert_eq!(rows(HeadKey::Param), [1]);
        assert_eq!(rows(HeadKey::Any), [0, 1, 2, 3, 4, 5]);
        assert!(
            t.candidates(DefId::from_raw(9), HeadKey::Any)
                .next()
                .is_none()
        );
    }

    #[test]
    fn canonical_goals_share_keys_across_bodies() {
        let gp = InternPool::new();
        // Two bodies, each with its own local pool.
        let (l1, l2) = (LocalPool::new(), LocalPool::new());
        let (p1, p2) = (Types::with_local(&gp, &l1), Types::with_local(&gp, &l2));
        let list = DefId::from_raw(1);
        let eq = DefId::from_raw(2);
        let mut t1 = InferTable::default();
        let _ = t1.fresh(p1, VarKind::General);
        let v3 = t1.fresh(p1, VarKind::General);
        let mut t2 = InferTable::default();
        let v0 = t2.fresh(p2, VarKind::General);
        let g = |p: Types<'_>, v| TraitRef {
            trait_: eq,
            self_ty: p.intern_ty(&TyData::Adt {
                def: list,
                args: p.list(&[v]),
            }),
            args: TyList::EMPTY,
        };
        let (a, _) = canonicalize(p1, GoalKind::Implements, g(p1, v3), false);
        let (b, vars) = canonicalize(p2, GoalKind::Implements, g(p2, v0), false);
        assert!(!a.self_ty.is_local(), "canonical goals are global");
        assert_eq!(a, b);
        assert_eq!(vars.map.len(), 1);
    }

    #[test]
    fn skeleton_solver_finds_one_exact_head_and_memo_publishes_once() {
        let gp = InternPool::new();
        let p = gp.types();
        let tr = DefId::from_raw(5);
        let mut t = ImplTable::default();
        t.trait_.push(tr);
        t.def.push(DefId::from_raw(6));
        t.head_key.push(HeadKey::Prim(Prim::I32));
        t.arg_key.push([HeadKey::Any; 2]);
        t.n_params.push(0);
        t.head_self.push(Ty::I32);
        t.head_args.push(TyList::EMPTY);
        t.plan.push(vec![]);
        t.assoc.push(vec![]);
        t.origin.push(ImplOrigin::Written);
        t.rank.push(0);
        t.by_trait.insert(tr.raw(), (0, 1));
        t.index();
        let m = ModuleId::from_raw(0);
        let tables = [(m, &t)];
        let env = ParamEnv::default();
        let global = GlobalMemo::default();
        let mut body = BodyMemo::default();
        let (u, _) = ImplUniverses::default().intern(&[]);
        let mut cx = SolveCx {
            pool: p,
            env: &env,
            universe: u,
            impls: super::Impls::All(&tables),
            body_memo: &mut body,
            global: &global,
        };
        let tref = TraitRef {
            trait_: tr,
            self_ty: Ty::I32,
            args: TyList::EMPTY,
        };
        let goal = Goal::Implements {
            tref,
            bindings: vec![],
            mut_: false,
        };
        let a = TableSolver
            .solve(&mut cx, &goal, &mut Fuel::new(10))
            .expect("holds");
        assert!(matches!(a, super::Answer::Holds { .. }));
        assert!(
            TableSolver
                .select(p, &tables, ConcreteTraitRef(tref))
                .is_ok()
        );
        let (cg, _) = canonicalize(p, GoalKind::Implements, tref, false);
        let key = MemoKey {
            goal: cg,
            env: super::EnvKey::EMPTY,
            universe: None,
            avail: 0,
        };
        // The solve published its completed, context-free answer.
        let i = global.lookup(&key).expect("published");
        let e = global.entry(i);
        assert_eq!((e.kind, e.height, e.heads), (MemoKind::Holds, 1, 1));
        let again = super::MemoRecord {
            key,
            answer: global.record(i).answer.clone(),
            children: Box::new([]),
            reads: Box::new([]),
            scoped: false,
        };
        let e2 = MemoEntry { children: 7, ..e };
        assert_eq!(global.publish(again, e2, u), i, "first writer wins");
        assert_eq!(global.entry(i).children, e.children);
    }

    /// Appends one impl row of `trait_` (rows of one trait stay together).
    fn push_row(
        t: &mut ImplTable,
        pool: Types<'_>,
        (trait_, def): (DefId, DefId),
        self_ty: Ty,
        args: TyList,
        (n_params, plan): (u8, Vec<super::PlanStep>),
    ) {
        let n = u32::try_from(t.def.len()).expect("rows");
        t.by_trait
            .entry(trait_.raw())
            .and_modify(|e| e.1 = n + 1)
            .or_insert((n, n + 1));
        t.trait_.push(trait_);
        t.def.push(def);
        t.head_key.push(HeadKey::of(pool, self_ty));
        t.arg_key.push([HeadKey::Any; 2]);
        t.n_params.push(n_params);
        t.head_self.push(self_ty);
        t.head_args.push(args);
        t.plan.push(plan);
        t.assoc.push(vec![]);
        t.origin.push(ImplOrigin::Written);
        t.rank.push(u64::from(n));
    }

    /// §14.2 memo invariance, §3.2's A/B pair: modules `a` and `b` see the
    /// same traits but different closures, and ask one open goal,
    /// `Receiver: Pick[?0]`. Only `a`'s closure holds the argument-owned
    /// `impl Pick[Product] for Receiver`. Each gets its own answer in both
    /// orders and on several threads, and their keys differ by universe.
    #[test]
    fn two_closures_asking_one_open_goal_keep_their_own_answers() {
        use super::{
            Answer, EnvKey, Evidence, FolderImpls, ImplRef, ImplView, Impls, OwnerMap, folder_table,
        };
        use hd_base::PathId;
        use hd_intern::{PathKind, PathTable};

        let gp = InternPool::new();
        let g = gp.types();
        let paths = PathTable::new();
        let pkg = paths.intern(PathId::NONE, PathKind::Package, "pkg");
        let mods = ["base", "prod", "a", "b"].map(|m| paths.intern(pkg, PathKind::Module, m));
        let item =
            |m: PathId, name: &str| DefId::from_raw(paths.intern(m, PathKind::Item, name).raw());
        let (pick, receiver, product) = (
            item(mods[0], "Pick"),
            item(mods[0], "Receiver"),
            item(mods[1], "Product"),
        );
        let f = FolderId::from_raw;
        let mut owners = OwnerMap::default();
        for (i, m) in (0u32..).zip(mods) {
            owners.insert(m, f(i));
        }
        let adt = |def| {
            g.intern_ty(&TyData::Adt {
                def,
                args: TyList::EMPTY,
            })
        };
        let (recv_ty, prod_ty) = (adt(receiver), adt(product));
        let mut t = ImplTable::default();
        let def = DefId::from_raw(paths.intern(mods[1], PathKind::Impl, "Pick").raw());
        push_row(
            &mut t,
            g,
            (pick, def),
            recv_ty,
            g.list(&[prod_ty]),
            (0, vec![]),
        );
        t.index();
        let prod = FolderImpls::new(f(1), t, g, &paths, &owners);
        assert_eq!(prod.arg_impls, [0], "owned only through `Product`");
        assert!(prod.unowned.is_empty());
        let empty = FolderImpls::default();
        let universes = ImplUniverses::default();
        let global = GlobalMemo::default();

        // One context: its closure's folders, one open goal.
        let ask = |closure: &[u32]| -> (Answer, MemoKey) {
            let mut folders: Vec<Option<&FolderImpls>> = vec![None; 4];
            let mut members = Vec::new();
            for &c in closure {
                let fi = if c == 1 { &prod } else { &empty };
                folders[c as usize] = Some(fi);
                if fi.in_universe() {
                    members.push((f(c), fi));
                }
            }
            let (universe, extra) = universes.intern(&members);
            let arity = |_: DefId| 1;
            let view = ImplView {
                paths: &paths,
                owners: &owners,
                own: None,
                folders: &folders,
                universe,
                extra: &extra,
                arity: &arity,
            };
            let local = LocalPool::new();
            let p = Types::with_local(&gp, &local);
            let v = InferTable::default().fresh(p, VarKind::General);
            let tref = TraitRef {
                trait_: pick,
                self_ty: recv_ty,
                args: p.list(&[v]),
            };
            let env = ParamEnv::default();
            let mut body = BodyMemo::default();
            let mut cx = SolveCx {
                pool: p,
                env: &env,
                universe,
                impls: Impls::Owned(&view),
                body_memo: &mut body,
                global: &global,
            };
            let goal = Goal::Implements {
                tref,
                bindings: vec![],
                mut_: false,
            };
            let answer = TableSolver
                .solve(&mut cx, &goal, &mut Fuel::new(100))
                .expect("solves");
            let (cg, _) = canonicalize(p, GoalKind::Implements, tref, false);
            (answer, MemoKey::new(p, cg, EnvKey::EMPTY, universe, 0))
        };
        let a = || match ask(&[0, 1, 2]) {
            (
                Answer::Holds {
                    evidence: Evidence::Impl { row, .. },
                    learned,
                },
                key,
            ) => {
                let at = ImplRef {
                    module: folder_table(f(1)),
                    row: 0,
                };
                assert_eq!(row, at);
                assert_eq!(learned.iter().map(|l| l.1).collect::<Vec<_>>(), [prod_ty]);
                key
            }
            other => panic!("`a` sees the impl: {other:?}"),
        };
        let b = || match ask(&[0, 3]) {
            (Answer::Fails(_), key) => key,
            other => panic!("`b` does not: {other:?}"),
        };
        let (ka, kb) = (a(), b());
        assert_eq!((b(), a()), (kb, ka), "the other order");
        assert!(ka.universe.is_some() && kb.universe.is_some());
        assert_ne!(ka, kb, "the universe keeps the two answers apart");
        assert_eq!(ka.goal, kb.goal);
        // Equal arg-impl folders share one universe: `a` without its own
        // (empty) folder keys the same.
        assert_eq!(ask(&[0, 1]).1, ka);
        std::thread::scope(|s| {
            for i in 0..8 {
                let (a, b) = (&a, &b);
                s.spawn(move || {
                    let keys = if i % 2 == 0 {
                        (a(), b())
                    } else {
                        let kb = b();
                        (a(), kb)
                    };
                    assert_eq!(keys, (ka, kb));
                });
            }
        });
    }

    /// A goal with known arguments keys without the universe, but its
    /// proof may read unowned rows: here the misplaced `impl Tag for i32`
    /// in `prod`, visible only where the closure holds `prod`. `with` and
    /// `without` each get their own answer in both orders and on several
    /// threads; `with`'s is published under its universe. A goal whose
    /// probes meet no unowned row (`Thing: Tag`, owned by `lib`) keeps
    /// one entry for every universe.
    #[test]
    fn a_known_goal_that_read_unowned_rows_is_scoped_to_its_universe() {
        use super::{Answer, EnvKey, FolderImpls, ImplView, Impls, OwnerMap};
        use hd_base::PathId;
        use hd_intern::{PathKind, PathTable};

        let gp = InternPool::new();
        let g = gp.types();
        let paths = PathTable::new();
        let pkg = paths.intern(PathId::NONE, PathKind::Package, "pkg");
        let mods =
            ["lib", "prod", "with", "without"].map(|m| paths.intern(pkg, PathKind::Module, m));
        let item =
            |m: PathId, name: &str| DefId::from_raw(paths.intern(m, PathKind::Item, name).raw());
        let (tag, thing) = (item(mods[0], "Tag"), item(mods[0], "Thing"));
        let f = FolderId::from_raw;
        let mut owners = OwnerMap::default();
        for (i, m) in (0u32..).zip(mods) {
            owners.insert(m, f(i));
        }
        let thing_ty = g.intern_ty(&TyData::Adt {
            def: thing,
            args: TyList::EMPTY,
        });
        let row = |m: PathId, self_ty: Ty| {
            let mut t = ImplTable::default();
            let def = DefId::from_raw(paths.intern(m, PathKind::Impl, "Tag").raw());
            push_row(&mut t, g, (tag, def), self_ty, TyList::EMPTY, (0, vec![]));
            t.index();
            t
        };
        let lib = FolderImpls::new(f(0), row(mods[0], thing_ty), g, &paths, &owners);
        assert!(!lib.in_universe(), "`lib` owns `impl Tag for Thing`");
        let prod = FolderImpls::new(f(1), row(mods[1], Ty::I32), g, &paths, &owners);
        assert_eq!(prod.unowned, [0], "`prod` owns neither `Tag` nor `i32`");
        let empty = FolderImpls::default();
        let universes = ImplUniverses::default();
        let global = GlobalMemo::default();

        let ask = |closure: &[u32], self_ty: Ty| -> (Answer, MemoKey, super::ImplUniverseId) {
            let mut folders: Vec<Option<&FolderImpls>> = vec![None; 4];
            let mut members = Vec::new();
            for &c in closure {
                let fi = match c {
                    0 => &lib,
                    1 => &prod,
                    _ => &empty,
                };
                folders[c as usize] = Some(fi);
                if fi.in_universe() {
                    members.push((f(c), fi));
                }
            }
            let (universe, extra) = universes.intern(&members);
            let arity = |_: DefId| 0;
            let view = ImplView {
                paths: &paths,
                owners: &owners,
                own: None,
                folders: &folders,
                universe,
                extra: &extra,
                arity: &arity,
            };
            let local = LocalPool::new();
            let p = Types::with_local(&gp, &local);
            let tref = TraitRef {
                trait_: tag,
                self_ty,
                args: TyList::EMPTY,
            };
            let env = ParamEnv::default();
            let mut body = BodyMemo::default();
            let mut cx = SolveCx {
                pool: p,
                env: &env,
                universe,
                impls: Impls::Owned(&view),
                body_memo: &mut body,
                global: &global,
            };
            let goal = Goal::Implements {
                tref,
                bindings: vec![],
                mut_: false,
            };
            let answer = TableSolver
                .solve(&mut cx, &goal, &mut Fuel::new(100))
                .expect("solves");
            let (cg, _) = canonicalize(p, GoalKind::Implements, tref, false);
            (
                answer,
                MemoKey::new(p, cg, EnvKey::EMPTY, universe, 0),
                universe,
            )
        };
        let with = || match ask(&[0, 1, 2], Ty::I32) {
            (Answer::Holds { .. }, key, u) => (key, u),
            other => panic!("`with` sees the misplaced impl: {other:?}"),
        };
        let without = || match ask(&[0, 3], Ty::I32) {
            (Answer::Fails(_), key, u) => (key, u),
            other => panic!("`without` does not: {other:?}"),
        };
        let (kw, ko) = (without(), with());
        assert_eq!((with(), without()), (ko, kw), "the other order");
        let (key, uw, uo) = (kw.0, kw.1, ko.1);
        assert_eq!(
            (key, key.universe),
            (ko.0, None),
            "the asked key has no universe"
        );
        assert_ne!(uw, uo);
        // `without` read no unowned row: its answer is every such
        // universe's, under the asked key. `with` read one: its answer is
        // scoped to its universe.
        let scoped = |u| MemoKey {
            universe: Some(u),
            ..key
        };
        let i = global.lookup(&key).expect("the unscoped entry");
        assert!(!global.record(i).scoped);
        assert_eq!(global.entry(i).kind, MemoKind::Fails);
        assert!(global.lookup(&scoped(uw)).is_none());
        let i = global.lookup(&scoped(uo)).expect("the scoped entry");
        assert!(global.record(i).scoped);
        assert_eq!(global.entry(i).kind, MemoKind::Holds);
        std::thread::scope(|s| {
            for i in 0..8 {
                let (with, without) = (&with, &without);
                s.spawn(move || {
                    if i % 2 == 0 {
                        assert_eq!((with(), without()), (ko, kw));
                    } else {
                        let o = without();
                        assert_eq!((with(), o), (ko, kw));
                    }
                });
            }
        });
        // Sharing is kept for a goal that read no unowned row.
        let (a, ka, _) = ask(&[0, 3], thing_ty);
        let (b, kb, _) = ask(&[0, 1, 2], thing_ty);
        assert!(matches!(
            (&a, &b),
            (Answer::Holds { .. }, Answer::Holds { .. })
        ));
        assert_eq!((ka, ka.universe), (kb, None));
        let i = global.lookup(&ka).expect("one unscoped entry");
        assert!(!global.record(i).scoped);
    }

    /// §3.2's debug check: a goal whose arguments are all known keys
    /// without a universe, so a frame of it that reads the directory (here
    /// through a `Bind` step whose projection leaves the trait's argument
    /// implicit) is an internal error, not a silently shared answer.
    #[test]
    fn a_directory_read_under_a_key_without_a_universe_is_an_internal_error() {
        use super::{FolderImpls, ImplView, Impls, MemoKey, OwnerMap, PlanStep, UniverseImpls};
        use crate::pool::ParamRef;
        use hd_base::PathId;
        use hd_intern::{PathKind, PathTable};

        let gp = InternPool::new();
        let g = gp.types();
        let paths = PathTable::new();
        let pkg = paths.intern(PathId::NONE, PathKind::Package, "pkg");
        let base = paths.intern(pkg, PathKind::Module, "base");
        let item = |name: &str| DefId::from_raw(paths.intern(base, PathKind::Item, name).raw());
        let (pick, sup, item_ty, receiver, product, wrap, imp) = (
            item("Pick"),
            item("Sup"),
            item("Sup.Item"),
            item("Receiver"),
            item("Product"),
            item("Wrap"),
            item("impl"),
        );
        let mut owners = OwnerMap::default();
        owners.insert(base, FolderId::from_raw(0));
        let adt = |def, args: &[Ty]| {
            g.intern_ty(&TyData::Adt {
                def,
                args: g.list(args),
            })
        };
        let param = g.intern_ty(&TyData::Param(ParamRef {
            owner: imp,
            index: 0,
        }));
        // `impl[I, Q] Pick[Product] for Wrap[I]` where `I < Sup[Item = Q]`,
        // the bound leaving `Sup`'s argument implicit.
        let mut t = ImplTable::default();
        let bind = PlanStep::Bind {
            param: 0,
            trait_: sup,
            args: TyList::EMPTY,
            assoc: item_ty,
            target: 1,
        };
        push_row(
            &mut t,
            g,
            (pick, imp),
            adt(wrap, &[param]),
            g.list(&[adt(product, &[])]),
            (2, vec![bind]),
        );
        t.index();
        let fi = FolderImpls::new(FolderId::from_raw(0), t, g, &paths, &owners);
        let folders = [Some(&fi)];
        let extra = UniverseImpls::default();
        let arity = |d: DefId| usize::from(d == sup || d == pick);
        let (universe, _) = ImplUniverses::default().intern(&[]);
        let view = ImplView {
            paths: &paths,
            owners: &owners,
            own: None,
            folders: &folders,
            universe,
            extra: &extra,
            arity: &arity,
        };
        let tref = TraitRef {
            trait_: pick,
            self_ty: adt(wrap, &[adt(receiver, &[])]),
            args: g.list(&[adt(product, &[])]),
        };
        assert_eq!(
            MemoKey::universe_for(g, GoalKind::Implements, tref.args, universe),
            None,
            "known arguments key without a universe"
        );
        assert_eq!(
            MemoKey::universe_for(g, GoalKind::Instantiations, TyList::EMPTY, universe),
            Some(universe)
        );
        let env = ParamEnv::default();
        let global = GlobalMemo::default();
        let mut body = BodyMemo::default();
        let mut cx = SolveCx {
            pool: g,
            env: &env,
            universe,
            impls: Impls::Owned(&view),
            body_memo: &mut body,
            global: &global,
        };
        let goal = Goal::Implements {
            tref,
            bindings: vec![],
            mut_: false,
        };
        let err = TableSolver
            .solve(&mut cx, &goal, &mut Fuel::new(100))
            .expect_err("internal error");
        assert!(err.what.starts_with("internal error"), "{}", err.what);
    }

    /// Solves `tref` over one table (module 0), with no environment.
    fn solve_in(p: Types<'_>, t: &ImplTable, tref: TraitRef) -> super::Answer {
        solve_over(p, &[(ModuleId::from_raw(0), t)], tref)
    }

    fn solve_over(
        p: Types<'_>,
        tables: &[(ModuleId, &ImplTable)],
        tref: TraitRef,
    ) -> super::Answer {
        let env = ParamEnv::default();
        let global = GlobalMemo::default();
        let mut body = BodyMemo::default();
        let (universe, _) = ImplUniverses::default().intern(&[]);
        let mut cx = SolveCx {
            pool: p,
            env: &env,
            universe,
            impls: super::Impls::All(tables),
            body_memo: &mut body,
            global: &global,
        };
        let goal = Goal::Implements {
            tref,
            bindings: vec![],
            mut_: false,
        };
        TableSolver
            .solve(&mut cx, &goal, &mut Fuel::new(100))
            .expect("solves")
    }

    /// A module's own impls are rows of its own table and of its folder's
    /// table: one impl, so one head, at its first place.
    #[test]
    fn one_impl_seen_in_two_tables_is_one_head() {
        let gp = InternPool::new();
        let local = LocalPool::new();
        let p = Types::with_local(&gp, &local);
        let [pick, money, imp] = [5, 6, 7].map(DefId::from_raw);
        let v = InferTable::default().fresh(p, VarKind::General);
        let money_ty = adt(p, money, &[]);
        let mut t = ImplTable::default();
        push_row(
            &mut t,
            p,
            (pick, imp),
            money_ty,
            p.list(&[Ty::I32]),
            (0, vec![]),
        );
        t.index();
        let tref = TraitRef {
            trait_: pick,
            self_ty: money_ty,
            args: p.list(&[v]),
        };
        let (own, folder) = (ModuleId::from_raw(0), ModuleId::from_raw(1));
        match solve_over(p, &[(own, &t), (folder, &t)], tref) {
            super::Answer::Holds {
                evidence: super::Evidence::Impl { row, .. },
                ..
            } => assert_eq!(row.module, own),
            other => panic!("one head commits: {other:?}"),
        }
    }

    fn adt(p: Types<'_>, def: DefId, args: &[Ty]) -> Ty {
        p.intern_ty(&TyData::Adt {
            def,
            args: p.list(args),
        })
    }

    fn param(p: Types<'_>, owner: DefId, index: u16) -> Ty {
        p.intern_ty(&TyData::Param(crate::pool::ParamRef { owner, index }))
    }

    /// `param < trait_` as plan step 0.
    fn bound(param: u8, trait_: DefId) -> Vec<super::PlanStep> {
        vec![super::PlanStep::Bound {
            param,
            trait_,
            args: TyList::EMPTY,
            mut_: false,
        }]
    }

    fn row_of(a: &super::Answer) -> Option<u32> {
        match a {
            super::Answer::Holds {
                evidence: super::Evidence::Impl { row, .. },
                ..
            } => Some(row.row),
            _ => None,
        }
    }

    /// Rule TS-2 (a): the one matching head is committed to; its failing
    /// bound fails the goal, and the failure names the bound's goal as the
    /// leaf and the committed impl and step as the chain (§10.1).
    #[test]
    fn a_failing_bound_of_the_committed_head_fails_the_goal_with_its_chain() {
        use super::{Answer, ChainStep, FailReason, ImplRef};
        let gp = InternPool::new();
        let p = gp.types();
        let [show, missing, boxed, imp] = [5, 6, 7, 8].map(DefId::from_raw);
        // impl[T < Missing] Show for Box[T]
        let mut t = ImplTable::default();
        let target = adt(p, boxed, &[param(p, imp, 0)]);
        push_row(
            &mut t,
            p,
            (show, imp),
            target,
            TyList::EMPTY,
            (1, bound(0, missing)),
        );
        t.index();
        let tref = TraitRef {
            trait_: show,
            self_ty: adt(p, boxed, &[Ty::I32]),
            args: TyList::EMPTY,
        };
        let Answer::Fails(info) = solve_in(p, &t, tref) else {
            panic!("the bound fails the goal");
        };
        let leaf = TraitRef {
            trait_: missing,
            self_ty: Ty::I32,
            args: TyList::EMPTY,
        };
        assert_eq!(
            info.leaf,
            canonicalize(p, GoalKind::Implements, leaf, false).0
        );
        assert_eq!(info.reason, FailReason::NoImpl);
        let at = ImplRef {
            module: ModuleId::from_raw(0),
            row: 0,
        };
        assert_eq!(
            info.chain,
            [ChainStep {
                impl_row: at,
                step: 0,
                origin: ImplOrigin::Written
            }]
        );
    }

    /// Rule TS-2 (b): two heads that an open goal could still pick
    /// between stall it on its variables; neither is tried. One head
    /// alone commits.
    #[test]
    fn several_possible_heads_stall_an_open_goal() {
        use super::Answer;
        let gp = InternPool::new();
        let local = LocalPool::new();
        let p = Types::with_local(&gp, &local);
        let [pick, money, boxed, d1, d2] = [5, 6, 7, 8, 9].map(DefId::from_raw);
        let mut infer = InferTable::default();
        let v = infer.fresh(p, VarKind::General);
        let TyData::Infer(var) = p.get(v) else {
            panic!("a variable");
        };
        let money_ty = adt(p, money, &[]);
        // impl Pick[i32] for Money; impl Pick[string] for Money
        let mut t = ImplTable::default();
        push_row(
            &mut t,
            p,
            (pick, d1),
            money_ty,
            p.list(&[Ty::I32]),
            (0, vec![]),
        );
        push_row(
            &mut t,
            p,
            (pick, d2),
            money_ty,
            p.list(&[Ty::STRING]),
            (0, vec![]),
        );
        t.index();
        let open = TraitRef {
            trait_: pick,
            self_ty: money_ty,
            args: p.list(&[v]),
        };
        assert_eq!(solve_in(p, &t, open), Answer::Stalled { on: vec![var] });
        // impl Pick for Box[i32]; impl Pick for Box[string]: two `Maybe`s.
        let mut t2 = ImplTable::default();
        let (b_i32, b_str) = (adt(p, boxed, &[Ty::I32]), adt(p, boxed, &[Ty::STRING]));
        push_row(&mut t2, p, (pick, d1), b_i32, TyList::EMPTY, (0, vec![]));
        push_row(&mut t2, p, (pick, d2), b_str, TyList::EMPTY, (0, vec![]));
        t2.index();
        let open_self = TraitRef {
            trait_: pick,
            self_ty: adt(p, boxed, &[v]),
            args: TyList::EMPTY,
        };
        assert_eq!(
            solve_in(p, &t2, open_self),
            Answer::Stalled { on: vec![var] }
        );
        // With one head, the open argument commits to it.
        let mut one = ImplTable::default();
        push_row(
            &mut one,
            p,
            (pick, d1),
            money_ty,
            p.list(&[Ty::I32]),
            (0, vec![]),
        );
        one.index();
        assert_eq!(row_of(&solve_in(p, &one, open)), Some(0));
    }

    /// Rule TS-2 (c): a head that matches by binding its parameters to
    /// the goal's variables, beside a head the variables could still
    /// become, stalls too. Once the goal is known, only one head matches.
    #[test]
    fn a_yes_head_beside_a_maybe_head_stalls_until_the_goal_is_known() {
        use super::Answer;
        let gp = InternPool::new();
        let local = LocalPool::new();
        let p = Types::with_local(&gp, &local);
        let [conv, boxed, d1, d2] = [5, 6, 7, 8].map(DefId::from_raw);
        let mut infer = InferTable::default();
        let (v0, v1) = (
            infer.fresh(p, VarKind::General),
            infer.fresh(p, VarKind::General),
        );
        let var = |t: Ty| match p.get(t) {
            TyData::Infer(x) => x,
            _ => panic!("a variable"),
        };
        // impl[T] Convert[T] for Box[T]; impl Convert[i32] for Box[string]
        let mut t = ImplTable::default();
        let tp = param(p, d1, 0);
        push_row(
            &mut t,
            p,
            (conv, d1),
            adt(p, boxed, &[tp]),
            p.list(&[tp]),
            (1, vec![]),
        );
        let b_str = adt(p, boxed, &[Ty::STRING]);
        push_row(
            &mut t,
            p,
            (conv, d2),
            b_str,
            p.list(&[Ty::I32]),
            (0, vec![]),
        );
        t.index();
        let goal = |self_ty, arg| TraitRef {
            trait_: conv,
            self_ty,
            args: p.list(&[arg]),
        };
        assert_eq!(
            solve_in(p, &t, goal(adt(p, boxed, &[v1]), v0)),
            Answer::Stalled {
                on: vec![var(v1), var(v0)]
            }
        );
        assert_eq!(
            solve_in(p, &t, goal(b_str, v0)),
            Answer::Stalled { on: vec![var(v0)] }
        );
        assert_eq!(row_of(&solve_in(p, &t, goal(b_str, Ty::I32))), Some(1));
        assert_eq!(row_of(&solve_in(p, &t, goal(b_str, Ty::STRING))), Some(0));
    }

    /// Rule TS-2 (d), §5.4: two heads match a known goal only beside an
    /// overlap error. The first in content order is committed to, even
    /// when its bound fails, on every ask and every thread.
    #[test]
    fn overlapping_heads_commit_to_the_first_in_content_order() {
        use super::Answer;
        let gp = InternPool::new();
        let p = gp.types();
        let [show, missing, boxed, generic, plain] = [5, 6, 7, 8, 9].map(DefId::from_raw);
        let b_i32 = adt(p, boxed, &[Ty::I32]);
        let generic_row = |t: &mut ImplTable| {
            let target = adt(p, boxed, &[param(p, generic, 0)]);
            push_row(
                t,
                p,
                (show, generic),
                target,
                TyList::EMPTY,
                (1, bound(0, missing)),
            );
        };
        let plain_row =
            |t: &mut ImplTable| push_row(t, p, (show, plain), b_i32, TyList::EMPTY, (0, vec![]));
        // impl[T < Missing] Show for Box[T], then impl Show for Box[i32]
        let mut first_generic = ImplTable::default();
        generic_row(&mut first_generic);
        plain_row(&mut first_generic);
        first_generic.index();
        // The same two in the other content order.
        let mut first_plain = ImplTable::default();
        plain_row(&mut first_plain);
        generic_row(&mut first_plain);
        first_plain.index();
        let tref = TraitRef {
            trait_: show,
            self_ty: b_i32,
            args: TyList::EMPTY,
        };
        let a = solve_in(p, &first_generic, tref);
        let Answer::Fails(info) = &a else {
            panic!("no backtracking to the second head: {a:?}");
        };
        assert_eq!(info.chain[0].impl_row.row, 0);
        let b = solve_in(p, &first_plain, tref);
        assert_eq!(row_of(&b), Some(0));
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    let p = gp.types();
                    assert_eq!(solve_in(p, &first_generic, tref), a);
                    assert_eq!(solve_in(p, &first_plain, tref), b);
                });
            }
        });
    }

    /// §3.9, `trait.target.numeric-family.each`: a numeric-family head
    /// stands for the types its bound lists, so it matches only those and
    /// never takes a goal from another head.
    #[test]
    fn a_numeric_family_head_matches_only_its_members() {
        use super::Answer;
        let gp = InternPool::new();
        let p = gp.types();
        let [eq, num, family, for_char, for_i32] = [5, 6, 7, 8, 9].map(DefId::from_raw);
        let char_ty = Ty::prim(Prim::Char);
        let mut t = ImplTable::default();
        // impl[N < Num] Eq for N; impl Eq for char; impl Num for i32
        let n = param(p, family, 0);
        push_row(
            &mut t,
            p,
            (eq, family),
            n,
            TyList::EMPTY,
            (1, bound(0, num)),
        );
        push_row(
            &mut t,
            p,
            (eq, for_char),
            char_ty,
            TyList::EMPTY,
            (0, vec![]),
        );
        push_row(
            &mut t,
            p,
            (num, for_i32),
            Ty::I32,
            TyList::EMPTY,
            (0, vec![]),
        );
        t.index();
        let goal = |self_ty| TraitRef {
            trait_: eq,
            self_ty,
            args: TyList::EMPTY,
        };
        assert_eq!(row_of(&solve_in(p, &t, goal(char_ty))), Some(1));
        assert_eq!(row_of(&solve_in(p, &t, goal(Ty::I32))), Some(0));
        let Answer::Fails(info) = solve_in(p, &t, goal(Ty::BOOL)) else {
            panic!("bool is no member");
        };
        assert!(info.chain.is_empty(), "no head matched: {info:?}");
    }

    fn ask(p: Types<'_>, t: &ImplTable, goal: &Goal) -> super::Answer {
        let env = ParamEnv::default();
        let global = GlobalMemo::default();
        let mut body = BodyMemo::default();
        let (universe, _) = ImplUniverses::default().intern(&[]);
        let tables = [(ModuleId::from_raw(0), t)];
        let mut cx = SolveCx {
            pool: p,
            env: &env,
            universe,
            impls: super::Impls::All(&tables),
            body_memo: &mut body,
            global: &global,
        };
        TableSolver
            .solve(&mut cx, goal, &mut Fuel::new(100))
            .expect("solves")
    }

    /// `Pick` over `Family` and `Box`, and `Show` over `Family`:
    /// row 0 `impl Pick[i32] for Family`, row 1 `impl[U < Show] Pick[U]
    /// for Family`, row 2 `impl[T < Missing] Pick[T] for Box[T]`, row 3
    /// `impl Show for Family`.
    fn pick_table(p: Types<'_>) -> (ImplTable, [DefId; 7]) {
        let ids = [10, 11, 12, 13, 14, 15, 16].map(DefId::from_raw);
        let [pick, show, missing, family, boxed, imp1, imp2] = ids;
        let fam = adt(p, family, &[]);
        let mut t = ImplTable::default();
        push_row(
            &mut t,
            p,
            (pick, DefId::from_raw(17)),
            fam,
            p.list(&[Ty::I32]),
            (0, vec![]),
        );
        push_row(
            &mut t,
            p,
            (pick, imp1),
            fam,
            p.list(&[param(p, imp1, 0)]),
            (1, bound(0, show)),
        );
        push_row(
            &mut t,
            p,
            (pick, imp2),
            adt(p, boxed, &[param(p, imp2, 0)]),
            p.list(&[param(p, imp2, 0)]),
            (1, bound(0, missing)),
        );
        push_row(
            &mut t,
            p,
            (show, DefId::from_raw(18)),
            fam,
            TyList::EMPTY,
            (0, vec![]),
        );
        t.index();
        (t, ids)
    }

    /// §6.5: the candidates are schemes in content order. A parameter the
    /// target does not fix stays fresh and its bound is a residual step;
    /// a step over fixed parameters runs, and one that fails drops its
    /// candidate; one that stalls on the target's variable is residual.
    #[test]
    fn instantiations_are_schemes_with_residual_bounds_in_content_order() {
        use super::{Answer, Candidate};
        let gp = InternPool::new();
        let local = LocalPool::new();
        let p = Types::with_local(&gp, &local);
        let (t, [pick, _, _, family, boxed, imp1, _]) = pick_table(p);
        let inst = |self_ty| Goal::Instantiations {
            self_ty,
            trait_: pick,
            mut_: false,
        };
        let Answer::Candidates(c) = ask(p, &t, &inst(adt(p, family, &[]))) else {
            panic!("Family has two instantiations");
        };
        let u = param(p, imp1, 0);
        assert_eq!(
            c.iter()
                .map(|c: &Candidate| (c.row.row, c.n_fresh, c.args, c.residual.clone()))
                .collect::<Vec<_>>(),
            [
                (0, 0, p.list(&[Ty::I32]), vec![]),
                (1, 1, p.list(&[u]), vec![0]),
            ]
        );
        assert_eq!(c[1].impl_args, p.list(&[u]), "U is fresh");
        // `Box[Family]`: T is fixed, `Family: Missing` fails.
        let plain = adt(p, boxed, &[adt(p, family, &[])]);
        assert!(matches!(ask(p, &t, &inst(plain)), Answer::Fails(_)));
        // `Box[?0]`: T is `?0`, and `?0: Missing` waits.
        let mut infer = InferTable::default();
        let v = infer.fresh(p, VarKind::General);
        let Answer::Candidates(c) = ask(p, &t, &inst(adt(p, boxed, &[v]))) else {
            panic!("one candidate with a residual");
        };
        assert_eq!((c.len(), c[0].residual.clone()), (1, vec![0]));
        assert_eq!(c[0].args, p.list(&[v]));
        // A variable self type stalls.
        assert!(matches!(ask(p, &t, &inst(v)), Answer::Stalled { .. }));
    }

    /// §6.5 `Methods`, trait part: each trait's candidates in the given
    /// order; a trait the receiver does not implement adds none.
    #[test]
    fn methods_lists_the_candidates_of_each_trait() {
        use super::Answer;
        let gp = InternPool::new();
        let p = gp.types();
        let (t, [pick, show, missing, family, ..]) = pick_table(p);
        let goal = Goal::Methods {
            receiver: adt(p, family, &[]),
            name: hd_base::Symbol::from_raw(0),
            traits: vec![show, missing, pick],
        };
        let Answer::Candidates(c) = ask(p, &t, &goal) else {
            panic!("candidates");
        };
        assert_eq!(c.iter().map(|c| c.row.row).collect::<Vec<_>>(), [3, 0, 1]);
    }

    /// One body of a memo test over every table: its pool, memo and fuel,
    /// and the run's shared global memo.
    fn solve_body(
        p: Types<'_>,
        tables: &[(ModuleId, &ImplTable)],
        env: &ParamEnv,
        (global, body, fuel): (&GlobalMemo, &mut BodyMemo, &mut Fuel),
        goal: &Goal,
    ) -> super::Answer {
        let (universe, _) = ImplUniverses::default().intern(&[]);
        let mut cx = SolveCx {
            pool: p,
            env,
            universe,
            impls: super::Impls::All(tables),
            body_memo: body,
            global,
        };
        TableSolver.solve(&mut cx, goal, fuel).expect("solves")
    }

    fn implements(tref: TraitRef) -> Goal {
        Goal::Implements {
            tref,
            bindings: vec![],
            mut_: false,
        }
    }

    /// Row 0 `impl Show for i32`, row 1 `impl[T < Show] Show for Box[T]`,
    /// row 2 `impl Pick[i32] for Money`.
    fn show_table(p: Types<'_>) -> (ImplTable, [DefId; 4]) {
        let ids = [20, 21, 22, 23].map(DefId::from_raw);
        let [show, pick, boxed, money] = ids;
        let mut t = ImplTable::default();
        let no_plan = (0, vec![]);
        push_row(
            &mut t,
            p,
            (show, DefId::from_raw(30)),
            Ty::I32,
            TyList::EMPTY,
            no_plan,
        );
        let imp = DefId::from_raw(31);
        let target = adt(p, boxed, &[param(p, imp, 0)]);
        push_row(
            &mut t,
            p,
            (show, imp),
            target,
            TyList::EMPTY,
            (1, bound(0, show)),
        );
        let m = adt(p, money, &[]);
        let row = (pick, DefId::from_raw(32));
        push_row(&mut t, p, row, m, p.list(&[Ty::I32]), (0, vec![]));
        t.index();
        (t, ids)
    }

    /// `Box` nested `n` deep over `i32`.
    fn boxes(p: Types<'_>, boxed: DefId, n: usize) -> Ty {
        (0..n).fold(Ty::I32, |t, _| adt(p, boxed, &[t]))
    }

    /// One body asking ground goals and goals with variables, each answer
    /// checked against a fresh solve; returns the fuel it spent.
    fn memo_body(gp: &InternPool, t: &ImplTable, global: &GlobalMemo) -> u64 {
        let [show, pick, boxed, money] = [20, 21, 22, 23].map(DefId::from_raw);
        let tables = [(ModuleId::from_raw(0), t)];
        let local = LocalPool::new();
        let p = Types::with_local(gp, &local);
        let mut infer = InferTable::default();
        let v = infer.fresh(p, VarKind::General);
        let w = infer.fresh(p, VarKind::General);
        let show_of = |self_ty| TraitRef {
            trait_: show,
            self_ty,
            args: TyList::EMPTY,
        };
        let goals = [
            show_of(boxes(p, boxed, 2)),
            show_of(boxes(p, boxed, 1)),
            show_of(adt(p, boxed, &[Ty::STRING])),
            show_of(adt(p, boxed, &[v])),
            TraitRef {
                trait_: pick,
                self_ty: adt(p, money, &[]),
                args: p.list(&[w]),
            },
            show_of(boxes(p, boxed, 2)),
        ];
        let env = ParamEnv::default();
        let mut body = BodyMemo::default();
        let mut fuel = Fuel::new(1000);
        for g in goals {
            let cx = (global, &mut body, &mut fuel);
            let a = solve_body(p, &tables, &env, cx, &implements(g));
            assert_eq!(a, solve_over(p, &tables, g), "{g:?}");
        }
        fuel.spent()
    }

    /// §14.2 memo invariance: a hit, from the body's memo or from another
    /// body's global entry, answers as a fresh solve, in the asking body's
    /// own variables. Ground goals are shared across bodies and threads;
    /// a warm memo charges a body the fuel a cold one does (rule TS-5).
    #[test]
    fn a_memo_hit_answers_as_a_fresh_solve() {
        let gp = InternPool::new();
        let (t, _) = show_table(gp.types());
        let global = GlobalMemo::default();
        let cold = memo_body(&gp, &t, &global);
        let stats = |hits, misses| super::MemoStats { hits, misses };
        assert_eq!(global.stats(), stats(2, 8));
        assert_eq!(
            memo_body(&gp, &t, &global),
            cold,
            "fuel does not see the memo"
        );
        // The second body found its three ground goals in the global memo
        // and computed again the two with variables (and a subgoal).
        assert_eq!(global.stats(), stats(6, 11));
        let shared = GlobalMemo::default();
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| assert_eq!(memo_body(&gp, &t, &shared), cold));
            }
        });
    }

    /// §14.2 memo invariance: contexts that differ only by their
    /// environment key a goal on a parameter apart, in both orders and on
    /// several threads; a goal that names no parameter has one entry.
    #[test]
    fn contexts_that_differ_only_by_environment_get_their_own_entries() {
        use super::{Answer, EnvKey};
        let gp = InternPool::new();
        let g = gp.types();
        let (t, [show, _, boxed, _]) = show_table(g);
        let tables = [(ModuleId::from_raw(0), &t)];
        let tp = param(g, DefId::from_raw(40), 0);
        let mut bounded = ParamEnv::default();
        bounded.clause_self.push(tp);
        bounded.clause_trait.push(show);
        bounded.clause_args.push(TyList::EMPTY);
        bounded.clause_bindings.push(vec![]);
        bounded.clause_mut.push(false);
        bounded.clause_origin.push(0);
        let plain = ParamEnv::default();
        let show_of = |self_ty| TraitRef {
            trait_: show,
            self_ty,
            args: TyList::EMPTY,
        };
        // `Box[T]: Show` asks `T: Show`, which only the bound answers.
        let on_param = show_of(adt(g, boxed, &[tp]));
        let ground = show_of(boxes(g, boxed, 1));
        let gp = &gp;
        let ask = |global: &GlobalMemo, env: &ParamEnv| {
            let mut body = BodyMemo::default();
            let mut fuel = Fuel::new(100);
            let mut one = |tref| {
                let cx = (global, &mut body, &mut fuel);
                solve_body(gp.types(), &tables, env, cx, &implements(tref))
            };
            (one(on_param), one(ground))
        };
        let yes = ask(&GlobalMemo::default(), &bounded);
        let no = ask(&GlobalMemo::default(), &plain);
        assert!(matches!(yes.0, Answer::Holds { .. }), "{yes:?}");
        assert!(matches!(no.0, Answer::Fails(_)), "{no:?}");
        assert_eq!(yes.1, no.1);
        let (universe, _) = ImplUniverses::default().intern(&[]);
        let key = |tref, env| {
            let (cg, _) = canonicalize(g, GoalKind::Implements, tref, false);
            MemoKey::new(g, cg, env, universe, 0)
        };
        for bounded_first in [true, false] {
            let global = GlobalMemo::default();
            let order = if bounded_first {
                [(&bounded, &yes), (&plain, &no)]
            } else {
                [(&plain, &no), (&bounded, &yes)]
            };
            for (env, want) in order {
                assert_eq!(&ask(&global, env), want);
            }
            let with_bound = global.get(&key(on_param, global.env_key(&bounded)));
            let without = global.get(&key(on_param, EnvKey::EMPTY));
            assert_eq!(with_bound.map(|e| e.kind), Some(MemoKind::Holds));
            assert_eq!(without.map(|e| e.kind), Some(MemoKind::Fails));
            assert!(global.get(&key(ground, EnvKey::EMPTY)).is_some());
            assert!(global.get(&key(ground, global.env_key(&bounded))).is_none());
        }
        let global = GlobalMemo::default();
        let envs = [(&bounded, &yes), (&plain, &no)];
        std::thread::scope(|s| {
            for i in 0..8 {
                let (global, ask) = (&global, &ask);
                s.spawn(move || {
                    for k in 0..2 {
                        let (env, want) = envs[(i + k) % 2];
                        assert_eq!(&ask(global, env), want);
                    }
                });
            }
        });
    }

    /// §2.2: body memo entries are canonical, so a trial that cached one
    /// and rolled back, dropping its variables (whose numbers come back),
    /// leaves no stale entry: a later variable reuses the entry under its
    /// own name, and no entry names a body-local type.
    #[test]
    fn a_rolled_back_trial_leaves_no_stale_body_entry() {
        use super::{Answer, CanonAnswer};
        let gp = InternPool::new();
        let local = LocalPool::new();
        let p = Types::with_local(&gp, &local);
        let (t, [_, pick, _, money]) = show_table(gp.types());
        let tables = [(ModuleId::from_raw(0), &t)];
        let (global, env) = (GlobalMemo::default(), ParamEnv::default());
        let mut body = BodyMemo::default();
        let mut fuel = Fuel::new(100);
        let mut infer = InferTable::default();
        let pick_of = |v| {
            implements(TraitRef {
                trait_: pick,
                self_ty: adt(p, money, &[]),
                args: p.list(&[v]),
            })
        };
        let var = |t: Ty| match p.get(t) {
            TyData::Infer(x) => x,
            _ => panic!("a variable"),
        };
        let learned = |a: Answer| match a {
            Answer::Holds { learned, .. } => learned,
            other => panic!("holds: {other:?}"),
        };
        let mark = infer.trial_mark();
        let v = infer.fresh(p, VarKind::General);
        let a = solve_body(
            p,
            &tables,
            &env,
            (&global, &mut body, &mut fuel),
            &pick_of(v),
        );
        assert_eq!(learned(a), [(var(v), Ty::I32)]);
        assert_eq!(body.len(), 1, "a goal with a variable is the body's");
        infer.rollback_trial(mark);
        let reused = infer.fresh(p, VarKind::General);
        assert_eq!(
            var(reused),
            var(v),
            "the trial's variable number comes back"
        );
        let w = infer.fresh(p, VarKind::General);
        let hits = global.stats().hits;
        let b = solve_body(
            p,
            &tables,
            &env,
            (&global, &mut body, &mut fuel),
            &pick_of(w),
        );
        assert_eq!(global.stats().hits, hits + 1);
        assert_eq!(learned(b), [(var(w), Ty::I32)]);
        assert_eq!(body.len(), 1);
        for r in body.records() {
            let g = r.key.goal;
            assert!(!g.self_ty.is_local());
            assert!(p.list_items(g.args).iter().all(|a| !a.is_local()));
            if let CanonAnswer::Holds { learned, .. } = &r.answer {
                assert!(learned.iter().all(|(_, t)| !t.is_local()));
            }
        }
    }

    /// Rule TS-7, §7.3: a goal that meets itself answers `Overflow` at once,
    /// not 64 levels down, and every goal of the cycle stores `Overflow`
    /// with the cycle height, which a later use in any body reuses.
    #[test]
    fn a_cycle_overflows_at_once_and_is_memoized() {
        use super::{Answer, EnvKey, PlanStep};
        let gp = InternPool::new();
        let g = gp.types();
        let [pair, imp] = [50, 51].map(DefId::from_raw);
        // impl[T, U < Pair[T]] Pair[U] for T
        let step = PlanStep::Bound {
            param: 1,
            trait_: pair,
            args: g.list(&[param(g, imp, 0)]),
            mut_: false,
        };
        let mut t = ImplTable::default();
        let (target, args) = (param(g, imp, 0), g.list(&[param(g, imp, 1)]));
        push_row(&mut t, g, (pair, imp), target, args, (2, vec![step]));
        t.index();
        let tables = [(ModuleId::from_raw(0), &t)];
        let goal = |self_ty, a| TraitRef {
            trait_: pair,
            self_ty,
            args: g.list(&[a]),
        };
        let (global, env) = (GlobalMemo::default(), ParamEnv::default());
        let mut body = BodyMemo::default();
        let mut fuel = Fuel::new(1000);
        let first = implements(goal(Ty::I32, Ty::BOOL));
        let a = solve_body(g, &tables, &env, (&global, &mut body, &mut fuel), &first);
        assert_eq!(a, Answer::Overflow);
        assert_eq!(
            fuel.spent(),
            4,
            "two goals of one head each; the repeat is met"
        );
        let (universe, _) = ImplUniverses::default().intern(&[]);
        for (s, x) in [(Ty::I32, Ty::BOOL), (Ty::BOOL, Ty::I32)] {
            let (cg, _) = canonicalize(g, GoalKind::Implements, goal(s, x), false);
            let key = MemoKey::new(g, cg, EnvKey::EMPTY, universe, 0);
            let e = global.get(&key).expect("memoized");
            assert_eq!((e.kind, e.height), (MemoKind::Overflow, u8::MAX));
        }
        let mut other = BodyMemo::default();
        let hits = global.stats().hits;
        let second = implements(goal(Ty::BOOL, Ty::I32));
        let b = solve_body(g, &tables, &env, (&global, &mut other, &mut fuel), &second);
        assert_eq!((b, global.stats().hits), (Answer::Overflow, hits + 1));
        let fresh = solve_over(g, &tables, goal(Ty::BOOL, Ty::I32));
        assert_eq!(fresh, Answer::Overflow);
    }

    /// §7.3: whether a goal overflows depends on the goal and the depth of
    /// its use, never on which ask computed it first: `Box` nested `n` deep
    /// over `i32` holds up to 64 and overflows from 65, in any order, in
    /// one body (whose `AtLeast` entries are reused or improved) or across
    /// bodies.
    #[test]
    fn heights_keep_overflow_independent_of_the_first_asker() {
        use super::Answer;
        let gp = InternPool::new();
        let g = gp.types();
        let (t, [show, _, boxed, _]) = show_table(g);
        let tables = [(ModuleId::from_raw(0), &t)];
        let show_of = |n| TraitRef {
            trait_: show,
            self_ty: boxes(g, boxed, n),
            args: TyList::EMPTY,
        };
        let env = ParamEnv::default();
        let fresh: Vec<Answer> = (0..=70)
            .map(|n| {
                let (global, mut body) = (GlobalMemo::default(), BodyMemo::default());
                let cx = (&global, &mut body, &mut Fuel::new(10_000));
                solve_body(g, &tables, &env, cx, &implements(show_of(n)))
            })
            .collect();
        assert!(matches!(fresh[64], Answer::Holds { .. }), "{:?}", fresh[64]);
        assert_eq!(fresh[65], Answer::Overflow);
        let orders = [
            [10, 70, 64, 65, 66, 63, 70, 1],
            [70, 66, 65, 64, 10, 63, 1, 70],
        ];
        for order in orders {
            let global = GlobalMemo::default();
            let mut shared = BodyMemo::default();
            for n in order {
                let mut fuel = Fuel::new(10_000);
                let goal = implements(show_of(n));
                let cx = (&global, &mut shared, &mut fuel);
                assert_eq!(solve_body(g, &tables, &env, cx, &goal), fresh[n], "n = {n}");
                let mut own = BodyMemo::default();
                let cx = (&global, &mut own, &mut fuel);
                let b = solve_body(g, &tables, &env, cx, &goal);
                assert_eq!(b, fresh[n], "a new body, n = {n}");
            }
        }
    }

    /// Rule TS-5, §7.4: a shared subgoal is charged once per body, so the
    /// DAG `P(k) = Pair[P(k-1), P(k-1)]` of 21 goals costs 42 steps, not
    /// two million, cold or warm; a repeat of the asked goal costs one.
    #[test]
    fn a_shared_subgoal_is_charged_once_per_body() {
        use super::{Answer, PlanStep};
        let gp = InternPool::new();
        let g = gp.types();
        let [eq, pair, imp, for_i32] = [60, 61, 62, 63].map(DefId::from_raw);
        let step = |param| PlanStep::Bound {
            param,
            trait_: eq,
            args: TyList::EMPTY,
            mut_: false,
        };
        // impl Eq for i32; impl[A < Eq, B < Eq] Eq for Pair[A, B]
        let mut t = ImplTable::default();
        push_row(
            &mut t,
            g,
            (eq, for_i32),
            Ty::I32,
            TyList::EMPTY,
            (0, vec![]),
        );
        let target = adt(g, pair, &[param(g, imp, 0), param(g, imp, 1)]);
        let plan = (2, vec![step(0), step(1)]);
        push_row(&mut t, g, (eq, imp), target, TyList::EMPTY, plan);
        t.index();
        let tables = [(ModuleId::from_raw(0), &t)];
        let deep = (0..20).fold(Ty::I32, |x, _| adt(g, pair, &[x, x]));
        let goal = implements(TraitRef {
            trait_: eq,
            self_ty: deep,
            args: TyList::EMPTY,
        });
        let (global, env) = (GlobalMemo::default(), ParamEnv::default());
        for _ in 0..2 {
            let mut body = BodyMemo::default();
            let mut fuel = Fuel::new(1000);
            let a = solve_body(g, &tables, &env, (&global, &mut body, &mut fuel), &goal);
            assert!(matches!(a, Answer::Holds { .. }), "{a:?}");
            assert_eq!(fuel.spent(), 42);
            let _ = solve_body(g, &tables, &env, (&global, &mut body, &mut fuel), &goal);
            assert_eq!(fuel.spent(), 43);
        }
    }

    /// lookup.rs `OWN_TABLE`: a module answers with its own impl rows
    /// first, so an entry that probed a bucket where the asking module
    /// has own rows is that module's alone, in either order; a goal whose
    /// probes miss its own table uses the global entry.
    #[test]
    fn a_module_with_its_own_rows_keeps_its_own_answer() {
        use super::{
            Answer, Evidence, FolderImpls, ImplRef, ImplView, Impls, OWN_TABLE, OwnerMap,
            UniverseImpls, folder_table,
        };
        use hd_base::PathId;
        use hd_intern::{PathKind, PathTable};

        let gp = InternPool::new();
        let g = gp.types();
        let paths = PathTable::new();
        let pkg = paths.intern(PathId::NONE, PathKind::Package, "pkg");
        let base = paths.intern(pkg, PathKind::Module, "base");
        let user = paths.intern(pkg, PathKind::Module, "user");
        let item = |name: &str| DefId::from_raw(paths.intern(base, PathKind::Item, name).raw());
        let imp = |name: &str| DefId::from_raw(paths.intern(base, PathKind::Impl, name).raw());
        let (show, foo, bar) = (item("Show"), item("Foo"), item("Bar"));
        let f0 = FolderId::from_raw(0);
        let mut owners = OwnerMap::default();
        owners.insert(base, f0);
        owners.insert(user, FolderId::from_raw(1));
        let (foo_ty, bar_ty) = (adt(g, foo, &[]), adt(g, bar, &[]));
        let row = |t: &mut ImplTable, d, s| {
            push_row(t, g, (show, d), s, TyList::EMPTY, (0, vec![]));
        };
        // `base` declares both impls; its own table holds only the first
        // here, so the probe for `Bar` misses it.
        let mut folder = ImplTable::default();
        row(&mut folder, imp("Foo"), foo_ty);
        row(&mut folder, imp("Bar"), bar_ty);
        folder.index();
        let fi = FolderImpls::new(f0, folder, g, &paths, &owners);
        let mut own = ImplTable::default();
        row(&mut own, imp("Foo"), foo_ty);
        own.index();
        let empty = FolderImpls::default();
        let folders = [Some(&fi), Some(&empty)];
        let extra = UniverseImpls::default();
        let arity = |_: DefId| 0;
        let (universe, _) = ImplUniverses::default().intern(&[]);
        let view = |own| ImplView {
            paths: &paths,
            owners: &owners,
            own,
            folders: &folders,
            universe,
            extra: &extra,
            arity: &arity,
        };
        let (in_base, in_user) = (view(Some(&own)), view(None));
        let ask = |global: &GlobalMemo, v: &ImplView<'_>, self_ty| {
            let env = ParamEnv::default();
            let mut body = BodyMemo::default();
            let mut cx = SolveCx {
                pool: g,
                env: &env,
                universe,
                impls: Impls::Owned(v),
                body_memo: &mut body,
                global,
            };
            let goal = implements(TraitRef {
                trait_: show,
                self_ty,
                args: TyList::EMPTY,
            });
            let a = TableSolver
                .solve(&mut cx, &goal, &mut Fuel::new(100))
                .expect("solves");
            let row = match a {
                Answer::Holds {
                    evidence: Evidence::Impl { row, .. },
                    ..
                } => row,
                other => panic!("holds: {other:?}"),
            };
            (row, body.len())
        };
        let mine = ImplRef {
            module: OWN_TABLE,
            row: 0,
        };
        let folder_row = |row| ImplRef {
            module: folder_table(f0),
            row,
        };
        for base_first in [true, false] {
            let global = GlobalMemo::default();
            let asks = if base_first {
                [(&in_base, (mine, 1)), (&in_user, (folder_row(0), 0))]
            } else {
                [(&in_user, (folder_row(0), 0)), (&in_base, (mine, 1))]
            };
            for (v, want) in asks {
                assert_eq!(ask(&global, v, foo_ty), want);
            }
            assert_eq!(ask(&global, &in_user, bar_ty), (folder_row(1), 0));
            let hits = global.stats().hits;
            assert_eq!(ask(&global, &in_base, bar_ty), (folder_row(1), 0));
            assert_eq!(global.stats().hits, hits + 1, "`base` shares `Bar`'s entry");
        }
    }
}
