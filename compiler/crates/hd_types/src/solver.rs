//! The trait solver's types and entry points (trait-solver.md §1.3, §2,
//! §3, §6, §7, §8, §10, §12). The goal, candidate, memo, evidence and
//! failure records are real definitions; `solve` and `select` are the
//! skeleton: they answer what needs no impl search and report the rest
//! as not implemented.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use hd_base::{DefId, FolderId, Fuel, InferVar, ModuleId, NotImplemented, Stage, StageResult, Symbol};

use crate::pool::{InternPool, ParamRef, Prim, Ty, TyData, TyList};
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
    Implements { tref: TraitRef, bindings: Vec<(DefId, Ty)>, mut_: bool },
    Project { assoc: DefId, tref: TraitRef },
    Instantiations { self_ty: Ty, trait_: DefId, mut_: bool },
    Methods { receiver: Ty, name: Symbol },
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

/// Interns impl universes once per run; equal lists share one id.
#[derive(Default)]
pub struct ImplUniverses {
    by_list: Mutex<HashMap<Vec<FolderId>, ImplUniverseId>>,
}

impl ImplUniverses {
    pub fn intern(&self, folders: &[FolderId]) -> ImplUniverseId {
        let mut list = folders.to_vec();
        list.sort_by_key(|f| f.raw());
        list.dedup();
        let mut m = self.by_list.lock().expect("universes");
        let next = ImplUniverseId(u32::try_from(m.len()).expect("universes"));
        *m.entry(list).or_insert(next)
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
    pub fn of(pool: &InternPool, t: Ty) -> HeadKey {
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
    Bound { param: u8, trait_: DefId, args: TyList, mut_: bool },
    Bind { param: u8, trait_: DefId, args: TyList, name: Symbol, target: u8 },
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
}

impl ImplTable {
    /// Rows of one trait whose head key can match `key`.
    pub fn candidates(&self, trait_: DefId, key: HeadKey) -> impl Iterator<Item = u32> + '_ {
        let (s, e) = self.by_trait.get(&trait_.raw()).copied().unwrap_or((0, 0));
        (s..e).filter(move |&r| {
            let h = self.head_key[r as usize];
            h == key || h == HeadKey::Param || key == HeadKey::Any
        })
    }
}

/// Head matching (§3.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchResult {
    Yes(Vec<Ty>),
    No,
    Maybe(Vec<u8>),
}

/// A candidate of an `Instantiations` goal (§6.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub row: ImplRef,
    pub n_fresh: u8,
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
    Holds { evidence: Evidence, learned: Vec<(InferVar, Ty)> },
    Normalized { ty: Ty },
    Candidates(Vec<Candidate>),
    Fails(Box<FailInfo>),
    Stalled { on: Vec<InferVar> },
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
    pub pool: &'a InternPool,
    pub env: &'a ParamEnv,
    pub universe: ImplUniverseId,
    pub tables: &'a [(ModuleId, &'a ImplTable)],
    pub body_memo: &'a mut BodyMemo,
    pub global: &'a GlobalMemo,
}

/// The solver's entry points (§1.3).
pub trait Solver: Sync {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: &Goal, fuel: &mut Fuel) -> StageResult<Answer>;
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey;
    fn select(&self, pool: &InternPool, tables: &[(ModuleId, &ImplTable)], tref: ConcreteTraitRef) -> StageResult<Selection>;
    fn normalize_concrete(&self, pool: &InternPool, base: Ty, trait_: DefId, args: TyList, name: Symbol) -> StageResult<Ty>;
}

/// Canonicalizes a goal's self type and arguments (§2.2 steps 1 and 3):
/// variables become `Canon(i)` in first-occurrence order.
pub fn canonicalize(pool: &InternPool, kind: GoalKind, tref: TraitRef, mut_: bool) -> (CanonGoal, CanonVars) {
    let mut vars = CanonVars::default();
    let self_ty = canon_ty(pool, tref.self_ty, &mut vars);
    let args: Vec<Ty> = pool.list_items(tref.args).into_iter().map(|t| canon_ty(pool, t, &mut vars)).collect();
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

fn canon_ty(pool: &InternPool, t: Ty, vars: &mut CanonVars) -> Ty {
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
            let a: Vec<Ty> = pool.list_items(args).into_iter().map(&mut c).collect();
            TyData::Adt { def, args: pool.list(&a) }
        }
        other => other,
    };
    pool.intern_ty(&d)
}

/// The skeleton solver: poison holds, a parameter's bound is found in the
/// environment, a head with exactly one matching row in the given tables
/// holds. Everything else is reported as not implemented.
#[derive(Default)]
pub struct SkeletonSolver;

impl Solver for SkeletonSolver {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: &Goal, fuel: &mut Fuel) -> StageResult<Answer> {
        if !fuel.charge(1) {
            return Ok(Answer::OutOfFuel);
        }
        let Goal::Implements { tref, .. } = goal else {
            return Err(NotImplemented::new(Stage::Body, "solver goals other than Implements"));
        };
        if cx.pool.has_poison(tref.self_ty) {
            return Ok(Answer::Holds { evidence: Evidence::Poison, learned: vec![] });
        }
        for i in 0..cx.env.clause_self.len() {
            if cx.env.clause_self[i] == tref.self_ty
                && cx.env.clause_trait[i] == tref.trait_
                && let TyData::Param(param) = cx.pool.get(tref.self_ty)
            {
                let index = u16::try_from(i).expect("clauses");
                return Ok(Answer::Holds { evidence: Evidence::Bound { param, index }, learned: vec![] });
            }
        }
        let key = HeadKey::of(cx.pool, tref.self_ty);
        let mut found = Vec::new();
        for (m, t) in cx.tables {
            for row in t.candidates(tref.trait_, key) {
                if t.head_self[row as usize] == tref.self_ty {
                    found.push(ImplRef { module: *m, row });
                }
            }
        }
        match found.as_slice() {
            [one] => Ok(Answer::Holds { evidence: Evidence::Impl { row: *one, args: TyList::EMPTY }, learned: vec![] }),
            _ => Err(NotImplemented::new(Stage::Body, "impl search beyond one exact head")),
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
        let key = if bounds.is_empty() { EnvKey::EMPTY } else { EnvKey(u32::try_from(bounds.len()).expect("env")) };
        out.key = Some(key);
        key
    }

    fn select(&self, pool: &InternPool, tables: &[(ModuleId, &ImplTable)], tref: ConcreteTraitRef) -> StageResult<Selection> {
        let key = HeadKey::of(pool, tref.0.self_ty);
        for (m, t) in tables {
            for row in t.candidates(tref.0.trait_, key) {
                if t.head_self[row as usize] == tref.0.self_ty {
                    return Ok(Selection { impl_row: ImplRef { module: *m, row }, args: TyList::EMPTY });
                }
            }
        }
        Err(NotImplemented::new(Stage::Collect, "select for generic impl heads"))
    }

    fn normalize_concrete(&self, _: &InternPool, _: Ty, _: DefId, _: TyList, _: Symbol) -> StageResult<Ty> {
        Err(NotImplemented::new(Stage::Collect, "associated type normalization"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BodyMemo, ConcreteTraitRef, GlobalMemo, Goal, GoalKind, HeadKey, ImplOrigin, ImplTable, ImplUniverses,
        MemoEntry, MemoKey, MemoKind, ParamEnv, SkeletonSolver, SolveCx, Solver, TraitRef, canonicalize,
    };
    use crate::pool::{InternPool, Prim, Ty, TyData, TyList};
    use crate::unify::{InferTable, VarKind};
    use hd_base::{DefId, FolderId, Fuel, ModuleId};

    #[test]
    fn universes_dedup_sorted_lists() {
        let u = ImplUniverses::default();
        let f = FolderId::from_raw;
        let a = u.intern(&[f(2), f(1)]);
        assert_eq!(a, u.intern(&[f(1), f(2), f(2)]));
        assert_ne!(a, u.intern(&[f(1)]));
        assert_eq!(u.len(), 2);
    }

    #[test]
    fn canonical_goals_share_keys_across_bodies() {
        let p = InternPool::new();
        let list = DefId::from_raw(1);
        let eq = DefId::from_raw(2);
        let mut t1 = InferTable::default();
        let _ = t1.fresh(&p, VarKind::General);
        let v3 = t1.fresh(&p, VarKind::General);
        let mut t2 = InferTable::default();
        let v0 = t2.fresh(&p, VarKind::General);
        let mk = |v| p.intern_ty(&TyData::Adt { def: list, args: p.list(&[v]) });
        let g = |v| TraitRef { trait_: eq, self_ty: mk(v), args: TyList::EMPTY };
        let (a, _) = canonicalize(&p, GoalKind::Implements, g(v3), false);
        let (b, vars) = canonicalize(&p, GoalKind::Implements, g(v0), false);
        assert_eq!(a, b);
        assert_eq!(vars.map.len(), 1);
    }

    #[test]
    fn skeleton_solver_finds_one_exact_head_and_memo_publishes_once() {
        let p = InternPool::new();
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
        let m = ModuleId::from_raw(0);
        let tables = [(m, &t)];
        let env = ParamEnv::default();
        let global = GlobalMemo::default();
        let mut body = BodyMemo::default();
        let u = ImplUniverses::default().intern(&[]);
        let mut cx = SolveCx { pool: &p, env: &env, universe: u, tables: &tables, body_memo: &mut body, global: &global };
        let tref = TraitRef { trait_: tr, self_ty: Ty::I32, args: TyList::EMPTY };
        let goal = Goal::Implements { tref, bindings: vec![], mut_: false };
        let a = SkeletonSolver.solve(&mut cx, &goal, &mut Fuel::new(10)).expect("holds");
        assert!(matches!(a, super::Answer::Holds { .. }));
        assert!(SkeletonSolver.select(&p, &tables, ConcreteTraitRef(tref)).is_ok());
        let (cg, _) = canonicalize(&p, GoalKind::Implements, tref, false);
        let key = MemoKey { goal: cg, env: super::EnvKey::EMPTY, universe: None, avail: 0 };
        let e = MemoEntry { answer: 1, children: 0, height: 0, kind: MemoKind::Holds, heads: 1 };
        global.publish(key, e);
        let e2 = MemoEntry { answer: 2, ..e };
        assert_eq!(global.publish(key, e2).answer, 1, "first writer wins");
    }
}
