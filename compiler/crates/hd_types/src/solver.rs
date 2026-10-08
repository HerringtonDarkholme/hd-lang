//! The trait solver's types and entry points (trait-solver.md §1.3, §2,
//! §3, §6, §7, §8, §10, §12). The goal, candidate, memo, evidence and
//! failure records are real definitions; `solve` and `select` are the
//! table solver: impl heads matched with their bound plans, and the
//! `Instantiations` and `Methods` goals' candidate schemes (§6.5); the
//! projection goal is still not implemented (the checker normalizes).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use hd_base::{
    DefId, FolderId, Fuel, InferVar, ModuleId, NotImplemented, Stage, StageResult, Symbol,
};

pub use crate::lookup::{
    FolderImpls, ImplView, Impls, OWN_TABLE, OwnerMap, RefTable, UniverseImpls, folder_table,
    open_arg,
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
    apply_binds_in(pool, impls, t, row, args, &mut false);
}

fn apply_binds_in(
    pool: Types<'_>,
    impls: Impls<'_>,
    t: &ImplTable,
    row: usize,
    args: &mut [Option<Ty>],
    read_dir: &mut bool,
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
            if let Some(x) = project_concrete(pool, impls, *assoc, *trait_, *base, ta, read_dir) {
                args[*target as usize] = Some(norm_concrete(pool, impls, x, 0, read_dir));
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

/// The run's global memo: completed, context-free answers only (rule TS-4).
#[derive(Default)]
pub struct GlobalMemo {
    shards: [Mutex<HashMap<MemoKey, MemoEntry>>; 16],
}

impl GlobalMemo {
    fn shard(&self, k: &MemoKey) -> &Mutex<HashMap<MemoKey, MemoEntry>> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        k.hash(&mut h);
        &self.shards[usize::try_from(h.finish() % 16).expect("shard")]
    }
    #[must_use]
    pub fn get(&self, k: &MemoKey) -> Option<MemoEntry> {
        self.shard(k).lock().expect("memo").get(k).copied()
    }
    /// Publishes an entry; the first writer wins (answers are pure).
    pub fn publish(&self, k: MemoKey, e: MemoEntry) -> MemoEntry {
        *self.shard(&k).lock().expect("memo").entry(k).or_insert(e)
    }
}

/// A body's own memo (§7.1): goals with placeholders or local types.
#[derive(Default, Debug)]
pub struct BodyMemo {
    pub table: HashMap<MemoKey, MemoEntry>,
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
    let self_ty = canon_ty(pool, tref.self_ty, &mut vars);
    let args: Vec<Ty> = pool
        .list_items(tref.args)
        .iter()
        .copied()
        .map(|t| canon_ty(pool, t, &mut vars))
        .collect();
    let goal = CanonGoal {
        kind,
        mut_,
        n_vars: u8::try_from(vars.map.len()).unwrap_or(u8::MAX),
        trait_: tref.trait_,
        self_ty,
        args: pool.list(&args),
        extra: 0,
    };
    (goal, vars)
}

fn canon_ty(pool: Types<'_>, t: Ty, vars: &mut CanonVars) -> Ty {
    if !pool.has_infer(t) {
        return t;
    }
    let mut c = |x| canon_ty(pool, x, vars);
    let d = match pool.get(t) {
        TyData::Infer(v) => {
            let i = vars.map.iter().position(|w| *w == v).unwrap_or_else(|| {
                vars.map.push(v);
                vars.kinds.push(VarKind::General);
                vars.map.len() - 1
            });
            TyData::Canon(u8::try_from(i).unwrap_or(u8::MAX))
        }
        TyData::Option(i) => TyData::Option(c(i)),
        TyData::Mut(i) => TyData::Mut(c(i)),
        TyData::Adt { def, args } => {
            let a: Vec<Ty> = pool.list_items(args).iter().copied().map(&mut c).collect();
            TyData::Adt {
                def,
                args: pool.list(&a),
            }
        }
        other => other,
    };
    pool.intern_ty(&d)
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
    read_dir: &mut bool,
) -> bool {
    let TyData::Param(p) = pool.get(t.head_self[r]) else {
        return false;
    };
    if p.owner != t.def[r] || matches!(pool.get(self_ty), TyData::Param(_)) {
        return false;
    }
    t.plan[r].iter().any(|step| match step {
        PlanStep::Bound { param, trait_, .. } if u16::from(*param) == p.index => !impls
            .candidates(pool, *trait_, self_ty, &[], false, read_dir)
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

impl TableSolver {
    fn implements(
        cx: &mut SolveCx<'_>,
        tref: TraitRef,
        fuel: &mut Fuel,
        depth: u32,
    ) -> StageResult<Answer> {
        let pool = cx.pool;
        if !fuel.charge(1) {
            return Ok(Answer::OutOfFuel);
        }
        if depth > 64 {
            return Ok(Answer::Overflow);
        }
        if pool.has_poison(tref.self_ty) {
            return Ok(Answer::Holds {
                evidence: Evidence::Poison,
                learned: vec![],
            });
        }
        let self_ty = match pool.get(tref.self_ty) {
            TyData::Mut(i) => i,
            _ => tref.self_ty,
        };
        // The environment: a clause on the same parameter and trait.
        if let TyData::Param(param) = pool.get(self_ty) {
            for i in 0..cx.env.clause_self.len() {
                if cx.env.clause_self[i] == self_ty && cx.env.clause_trait[i] == tref.trait_ {
                    let mut binds = Vec::new();
                    let m = match_list(
                        pool,
                        DefId::NONE,
                        cx.env.clause_args[i],
                        tref.args,
                        &mut binds,
                    );
                    if m == M::No {
                        continue;
                    }
                    let learned = learned_from(pool, cx.env.clause_args[i], tref.args);
                    let index = u16::try_from(i).unwrap_or(u16::MAX);
                    return Ok(Answer::Holds {
                        evidence: Evidence::Bound { param, index },
                        learned,
                    });
                }
            }
        }
        if let TyData::Infer(v) = pool.get(self_ty) {
            return Ok(Answer::Stalled { on: vec![v] });
        }
        let goal_args = pool.list_items(tref.args);
        let open = goal_args.iter().any(|a| open_arg(pool, *a));
        // The frame's "read the directory" bit (§3.2 debug check).
        let mut read_dir = false;
        let mut yes = Vec::new();
        let mut maybes = 0usize;
        let cands = cx
            .impls
            .candidates(pool, tref.trait_, self_ty, goal_args, open, &mut read_dir);
        for (at, t) in cands {
            let r = at.row as usize;
            if t.origin[r] == ImplOrigin::TupleTemplate {
                if matches!(pool.get(self_ty), TyData::Tuple { .. }) {
                    yes.push((at, Vec::new(), t));
                }
                continue;
            }
            if family_excludes(pool, cx.impls, t, r, self_ty, &mut read_dir) {
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
                    apply_binds_in(pool, cx.impls, t, r, &mut fixed, &mut read_dir);
                    let args: Vec<Ty> =
                        fixed.into_iter().map(|a| a.unwrap_or(Ty::POISON)).collect();
                    yes.push((at, args, t));
                }
                M::Maybe => maybes += 1,
                M::No => {}
            }
        }
        if read_dir
            && MemoKey::universe_for(pool, GoalKind::Implements, tref.args, cx.universe).is_none()
        {
            return Err(NotImplemented::new(
                Stage::Body,
                "internal error: a solver frame read the candidate directory, but its memo key has no impl universe",
            ));
        }
        // Committing (§3.5, rule TS-2): the heads alone pick the impl,
        // before any bound is solved. A goal with open variables that
        // more than one head could still answer stalls, so inference can
        // learn more; it never tries each head's bounds. Two heads that
        // match a goal with no open variable exist only beside an overlap
        // error (§5.4): the first in content order is taken.
        let open_goal = pool.has_infer(self_ty) || goal_args.iter().any(|a| pool.has_infer(*a));
        if yes.is_empty() && maybes == 0 && !pool.has_infer(self_ty) {
            let (leaf, _) = canonicalize(pool, GoalKind::Implements, tref, false);
            return Ok(Answer::Fails(Box::new(FailInfo {
                leaf,
                chain: vec![],
                reason: FailReason::NoImpl,
                near: vec![],
            })));
        }
        let several = yes.len() + maybes > 1;
        match yes.into_iter().next() {
            Some(head) if maybes == 0 && !(open_goal && several) => {
                Self::commit(cx, tref, head, fuel, depth)
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
                Ok(Answer::Stalled { on })
            }
        }
    }

    /// Solves the bound plan of the one impl a goal committed to (§3.6).
    /// The first step that does not hold decides; a failing step fails
    /// the goal, with this impl added to the front of its chain (§10.1).
    fn commit(
        cx: &mut SolveCx<'_>,
        tref: TraitRef,
        (row, args, t): (ImplRef, Vec<Ty>, &ImplTable),
        fuel: &mut Fuel,
        depth: u32,
    ) -> StageResult<Answer> {
        let pool = cx.pool;
        let r = row.row as usize;
        for (step, sub) in plan_goals(pool, t, r, &args) {
            match Self::implements(cx, sub, fuel, depth + 1)? {
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
                    return Ok(Answer::Fails(info));
                }
                other => return Ok(other),
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
        Ok(Answer::Holds {
            evidence: Evidence::Impl {
                row,
                args: pool.list(&args),
            },
            learned,
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

impl TableSolver {
    /// `Instantiations { S, Tr }` (§6.5): every impl head of `Tr` that
    /// matches `S` with all trait arguments open, as schemes in content
    /// order. The target fixes some impl parameters; the plan steps over
    /// those alone are solved here, and a candidate whose step fails is
    /// dropped. Steps that read a fresh parameter, or that stall on a
    /// variable of `S`, are the candidate's residual obligations.
    fn instantiations(
        cx: &mut SolveCx<'_>,
        self_ty: Ty,
        trait_: DefId,
        fuel: &mut Fuel,
        depth: u32,
    ) -> StageResult<Answer> {
        let pool = cx.pool;
        if !fuel.charge(1) {
            return Ok(Answer::OutOfFuel);
        }
        if depth > 64 {
            return Ok(Answer::Overflow);
        }
        let self_ty = match pool.get(self_ty) {
            TyData::Mut(i) => i,
            _ => self_ty,
        };
        match pool.get(self_ty) {
            TyData::Poison => {
                return Ok(Answer::Holds {
                    evidence: Evidence::Poison,
                    learned: vec![],
                });
            }
            TyData::Infer(v) => return Ok(Answer::Stalled { on: vec![v] }),
            TyData::Param(_) | TyData::TraitValue { .. } => {
                return Err(NotImplemented::new(
                    Stage::Body,
                    "Instantiations of a parameter or a trait value (the checker reads its clauses)",
                ));
            }
            _ => {}
        }
        let mut read_dir = false;
        let mut out = Vec::new();
        let mut maybe = false;
        let cands = cx
            .impls
            .candidates(pool, trait_, self_ty, &[], true, &mut read_dir);
        for (at, t) in cands {
            let r = at.row as usize;
            if t.origin[r] == ImplOrigin::TupleTemplate {
                if matches!(pool.get(self_ty), TyData::Tuple { .. }) {
                    out.push(Candidate {
                        row: at,
                        n_fresh: 0,
                        impl_args: TyList::EMPTY,
                        args: t.head_args[r],
                        residual: vec![],
                    });
                }
                continue;
            }
            if family_excludes(pool, cx.impls, t, r, self_ty, &mut read_dir) {
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
            apply_binds_in(pool, cx.impls, t, r, &mut fixed, &mut read_dir);
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
                match Self::implements(cx, sub, fuel, depth + 1)? {
                    Answer::Holds { .. } => {}
                    Answer::Fails(_) => {
                        dropped = true;
                        break;
                    }
                    Answer::Stalled { .. } => residual.push(i),
                    other => return Ok(other),
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
        if read_dir
            && MemoKey::universe_for(pool, GoalKind::Instantiations, TyList::EMPTY, cx.universe)
                .is_none()
        {
            return Err(NotImplemented::new(
                Stage::Body,
                "internal error: an Instantiations frame read the directory without a universe",
            ));
        }
        if maybe {
            let mut on = Vec::new();
            collect_vars(pool, self_ty, &mut on);
            let mut seen = Vec::with_capacity(on.len());
            on.retain(|v| {
                let first = !seen.contains(v);
                seen.push(*v);
                first
            });
            return Ok(Answer::Stalled { on });
        }
        if out.is_empty() {
            let tref = TraitRef {
                trait_,
                self_ty,
                args: TyList::EMPTY,
            };
            let (leaf, _) = canonicalize(pool, GoalKind::Instantiations, tref, false);
            return Ok(Answer::Fails(Box::new(FailInfo {
                leaf,
                chain: vec![],
                reason: FailReason::NoImpl,
                near: vec![],
            })));
        }
        Ok(Answer::Candidates(out))
    }

    /// The trait part of `Methods` (§6.5 step 3): one `Instantiations`
    /// goal per trait, in the given order; a trait the receiver does not
    /// implement adds nothing. The answer lists every candidate (its
    /// row's trait tells them apart); none at all is an empty list.
    fn methods(
        cx: &mut SolveCx<'_>,
        receiver: Ty,
        traits: &[DefId],
        fuel: &mut Fuel,
    ) -> StageResult<Answer> {
        let mut all = Vec::new();
        for tr in traits {
            match Self::instantiations(cx, receiver, *tr, fuel, 0)? {
                Answer::Candidates(c) => all.extend(c),
                Answer::Fails(_) => {}
                other => return Ok(other),
            }
        }
        Ok(Answer::Candidates(all))
    }
}

impl Solver for TableSolver {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: &Goal, fuel: &mut Fuel) -> StageResult<Answer> {
        match goal {
            Goal::Implements { tref, .. } => Self::implements(cx, *tref, fuel, 0),
            Goal::Instantiations {
                self_ty, trait_, ..
            } => Self::instantiations(cx, *self_ty, *trait_, fuel, 0),
            Goal::Methods {
                receiver, traits, ..
            } => Self::methods(cx, *receiver, traits, fuel),
            Goal::Project { .. } => Err(NotImplemented::new(
                Stage::Body,
                "the Project goal (the checker normalizes)",
            )),
        }
    }

    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey {
        for (i, b) in bounds.iter().enumerate() {
            out.clause_self.push(b.tref.self_ty);
            out.clause_trait.push(b.tref.trait_);
            out.clause_args.push(b.tref.args);
            out.clause_bindings.push(b.bindings.clone());
            out.clause_mut.push(b.mut_);
            out.clause_origin.push(u16::try_from(i).expect("bounds"));
        }
        let key = if bounds.is_empty() {
            EnvKey::EMPTY
        } else {
            EnvKey(u32::try_from(bounds.len()).expect("env"))
        };
        out.key = Some(key);
        key
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
        match Self::implements(&mut cx, tref.0, &mut fuel, 0)? {
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
    norm_concrete(pool, impls, t, 0, &mut false)
}

fn norm_list(
    pool: Types<'_>,
    impls: Impls<'_>,
    l: TyList,
    depth: u32,
    read_dir: &mut bool,
) -> TyList {
    let mut out = Vec::with_capacity(pool.list_items(l).len());
    for x in pool.list_items(l).iter().copied() {
        out.push(norm_concrete(pool, impls, x, depth, read_dir));
    }
    pool.list(&out)
}

fn norm_concrete(pool: Types<'_>, impls: Impls<'_>, t: Ty, depth: u32, read_dir: &mut bool) -> Ty {
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
            let s = norm_concrete(pool, impls, self_ty, depth, read_dir);
            let a = norm_list(pool, impls, args, depth, read_dir);
            if let Some(x) = project_concrete(pool, impls, assoc, trait_, s, a, read_dir) {
                return norm_concrete(pool, impls, x, depth + 1, read_dir);
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
            args: norm_list(pool, impls, args, depth, read_dir),
        },
        TyData::Tuple { elems, rest } => TyData::Tuple {
            elems: norm_list(pool, impls, elems, depth, read_dir),
            rest: rest.map(|r| norm_concrete(pool, impls, r, depth, read_dir)),
        },
        TyData::Option(i) => TyData::Option(norm_concrete(pool, impls, i, depth, read_dir)),
        TyData::Mut(i) => TyData::Mut(norm_concrete(pool, impls, i, depth, read_dir)),
        TyData::Fn {
            params,
            result,
            row,
            suspends,
        } => TyData::Fn {
            params: norm_list(pool, impls, params, depth, read_dir),
            result: norm_concrete(pool, impls, result, depth, read_dir),
            row,
            suspends,
        },
        TyData::TraitValue {
            def,
            args,
            bindings,
        } => TyData::TraitValue {
            def,
            args: norm_list(pool, impls, args, depth, read_dir),
            bindings: bindings
                .into_iter()
                .map(|(k, b)| (k, norm_concrete(pool, impls, b, depth, read_dir)))
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
    read_dir: &mut bool,
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
    let cands = impls.candidates(pool, trait_, base, goal_args, open, read_dir);
    for (at, t) in cands {
        let r = at.row as usize;
        let Some((_, b)) = t.assoc[r].iter().find(|(k, _)| *k == assoc) else {
            continue;
        };
        if family_excludes(pool, impls, t, r, base, read_dir) {
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
        let e = MemoEntry {
            answer: 1,
            children: 0,
            height: 0,
            kind: MemoKind::Holds,
            heads: 1,
        };
        global.publish(key, e);
        let e2 = MemoEntry { answer: 2, ..e };
        assert_eq!(global.publish(key, e2).answer, 1, "first writer wins");
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
}
