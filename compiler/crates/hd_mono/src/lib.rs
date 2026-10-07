#![forbid(unsafe_code)]
//! `hd_mono`: monomorphizing collection (codegen.md §13.1 to §13.3) over
//! `hd_tir::ir` bodies into the `InstanceTable`, with A1 classes and
//! instance keys from `layout`, and trait selection through the solver's
//! `select`. Collection records each call's target, so emission never
//! selects again. Reads TIR and interfaces only, never syntax.

pub mod layout;
pub mod passes;
pub mod suspend;

use std::collections::{BTreeSet, HashMap};

use hd_base::{DefId, Hash128, InstId, ModuleId, NotImplemented, StableHasher, Stage, StageResult};
use hd_tir::ir::{Body, Callee, ChoiceKind, Tag};
use hd_types::solver::{ConcreteTraitRef, ImplTable, Solver, TraitRef};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

use crate::layout::{A1Class, KeyArg, LayoutEnv, a1_class, instance_key};

/// What collection reads about the program (the driver implements it over
/// TIR and interfaces).
pub trait ProgramEnv: LayoutEnv {
    fn body(&self, def: DefId) -> Option<&Body>;
    /// The compiler-provided lowering of an instance whose item has no
    /// TIR of its own (`hd_host_abi::std_lowering`).
    fn lowering(&self, def: DefId, args: TyList) -> Option<u32>;
    /// Per type parameter: has a bound (A1, codegen.md §13.2: exact).
    fn bounded(&self, def: DefId) -> Option<Vec<bool>>;
    /// The declared result type.
    fn ret(&self, def: DefId) -> Option<Ty>;
    /// The method of `impl_` that implements the trait method `method`.
    fn impl_method(&self, impl_: DefId, method: DefId) -> Option<DefId>;
    /// Data fields, for the struct types a program needs.
    fn data_fields(&self, def: DefId) -> Option<Vec<Ty>>;
    fn path_hash(&self, def: DefId) -> Hash128;
    fn impl_tables(&self) -> Vec<(ModuleId, &ImplTable)>;
}

/// The A1 class `REF` as a type argument: a reserved canonical
/// placeholder, which never occurs in a concrete instance otherwise.
#[must_use]
pub fn class_ref(pool: &InternPool) -> Ty {
    pool.intern_ty(&TyData::Canon(0xF0))
}

#[must_use]
pub fn is_class_ref(pool: &InternPool, t: Ty) -> bool {
    pool.get(t) == TyData::Canon(0xF0)
}

/// Where one call goes: the callee instance's key and its result type
/// under its own arguments (an erased result is cast back at the caller).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallTarget {
    pub key: Hash128,
    pub ret: Ty,
    /// A compiler-provided lowering: the call is this host import
    /// (`hd_host_abi::STD_LOWERINGS` index) instead of an instance.
    pub import: Option<u32>,
}

/// The output of `Collect` (codegen.md §11.3).
#[derive(Debug, Default)]
pub struct Collected {
    pub table: InstanceTable,
    /// Per instance: each `Call` instruction's target.
    pub calls: Vec<HashMap<u32, CallTarget>>,
    /// Per instance: the hash of its callees' representation summaries,
    /// in call order (walking skeleton, SK-3); each code key holds it.
    pub callee_reps: Vec<Hash128>,
    pub imports: BTreeSet<u32>,
    /// Data types the program builds or reads.
    pub data: BTreeSet<DefIdOrd>,
}

pub use layout::InstanceTable;

/// A `DefId` ordered by its stable path hash, never by run ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefIdOrd(pub Hash128, pub u32);

impl DefIdOrd {
    #[must_use]
    pub fn def(self) -> DefId {
        DefId::from_raw(self.1)
    }
}

/// Substitutes an instance's arguments for its item's parameters.
#[must_use]
pub fn subst(pool: &InternPool, item: DefId, args: TyList, t: Ty) -> Ty {
    let a = pool.list_items(args);
    pool.subst(t, &|p: ParamRef| {
        (p.owner == item)
            .then(|| a.get(p.index as usize).copied())
            .flatten()
    })
}

fn key_args(pool: &InternPool, args: TyList) -> Vec<KeyArg> {
    pool.list_items(args)
        .into_iter()
        .map(|t| {
            if is_class_ref(pool, t) {
                KeyArg::Class(A1Class::Ref)
            } else {
                KeyArg::Canon(t)
            }
        })
        .collect()
}

fn note_data(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, out: &mut BTreeSet<DefIdOrd>) {
    if let TyData::Adt { def, .. } = pool.get(t)
        && out.insert(DefIdOrd(env.path_hash(def), def.raw()))
    {
        for f in env.data_fields(def).unwrap_or_default() {
            note_data(pool, env, f, out);
        }
    }
}

/// Collection from `root` (codegen.md §13.1): every reachable instance,
/// each call's target, the imports and data types.
pub fn collect(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    solver: &dyn Solver,
    root: DefId,
) -> StageResult<Collected> {
    let ph = |d: DefId| env.path_hash(d);
    let mut out = Collected::default();
    let root_key = instance_key(pool, &ph, root, 0, &[]);
    let (first, _) = out
        .table
        .push(root, 0, TyList::EMPTY, 0, InstId::NONE, root_key)?;
    let mut work = vec![first];
    let tables = env.impl_tables();
    while let Some(id) = work.pop() {
        let (item, args, depth) = (
            out.table.item[id.idx()],
            out.table.args[id.idx()],
            out.table.depth[id.idx()],
        );
        let Some(body) = env.body(item) else {
            return Err(NotImplemented::new(
                Stage::Collect,
                "an instance whose item has no TIR",
            ));
        };
        let mut calls = HashMap::new();
        let mut reps = StableHasher::new("callee-reps");
        for i in 0..body.len() {
            note_data(
                pool,
                env,
                subst(pool, item, args, body.ty[i]),
                &mut out.data,
            );
            match body.tags[i] {
                Tag::CallHost => {
                    out.imports.insert(body.data[i][0]);
                }
                Tag::Call => {
                    let Some(c) = Callee::from_words(body.record(body.data[i][0])) else {
                        return Err(NotImplemented::new(
                            Stage::Collect,
                            "a malformed callee record",
                        ));
                    };
                    let (callee, cargs) = match c {
                        Callee::Item { def, targs } => {
                            let bounded = env.bounded(def).unwrap_or_default();
                            let mut a = Vec::new();
                            for (k, t) in pool.list_items(targs).into_iter().enumerate() {
                                let t = subst(pool, item, args, t);
                                let exact = bounded.get(k).copied().unwrap_or(true)
                                    || is_class_ref(pool, t);
                                a.push(if !exact && a1_class(pool, env, t)? == A1Class::Ref {
                                    class_ref(pool)
                                } else {
                                    t
                                });
                            }
                            (def, pool.list(&a))
                        }
                        Callee::TraitMethod {
                            trait_,
                            method,
                            self_ty,
                            choice,
                            ..
                        } => {
                            let self_ty = subst(pool, item, args, self_ty);
                            let impl_ = match choice.0 {
                                ChoiceKind::Impl => DefId::from_raw(choice.1),
                                ChoiceKind::Bound => {
                                    let tref = ConcreteTraitRef(TraitRef {
                                        trait_,
                                        self_ty,
                                        args: TyList::EMPTY,
                                    });
                                    let sel = solver.select(pool, &tables, tref)?;
                                    let Some((_, t)) =
                                        tables.iter().find(|(m, _)| *m == sel.impl_row.module)
                                    else {
                                        return Err(NotImplemented::new(
                                            Stage::Collect,
                                            "a selection outside the impl tables",
                                        ));
                                    };
                                    t.def[sel.impl_row.row as usize]
                                }
                                _ => {
                                    return Err(NotImplemented::new(
                                        Stage::Collect,
                                        "trait-value and builtin calls",
                                    ));
                                }
                            };
                            let Some(m) = env.impl_method(impl_, method) else {
                                return Err(NotImplemented::new(
                                    Stage::Collect,
                                    "an impl without the called method",
                                ));
                            };
                            (m, TyList::EMPTY)
                        }
                    };
                    if let Some(ix) = env.lowering(callee, cargs) {
                        out.imports.insert(ix);
                        calls.insert(
                            u32::try_from(i).expect("insts"),
                            CallTarget {
                                key: Hash128(0),
                                ret: Ty::VOID,
                                import: Some(ix),
                            },
                        );
                        reps.hash(env.path_hash(callee));
                        continue;
                    }
                    let key = instance_key(pool, &ph, callee, 0, &key_args(pool, cargs));
                    let (cid, new) =
                        out.table
                            .push(callee, 0, cargs, depth.saturating_add(1), id, key)?;
                    if new {
                        work.push(cid);
                    }
                    let ret = subst(pool, callee, cargs, env.ret(callee).unwrap_or(Ty::VOID));
                    calls.insert(
                        u32::try_from(i).expect("insts"),
                        CallTarget {
                            key,
                            ret,
                            import: None,
                        },
                    );
                    reps.hash(env.path_hash(callee));
                    for b in env.bounded(callee).unwrap_or_default() {
                        reps.u8(u8::from(b));
                    }
                }
                _ => {}
            }
        }
        if out.calls.len() <= id.idx() {
            out.calls.resize_with(id.idx() + 1, HashMap::new);
            out.callee_reps.resize(id.idx() + 1, Hash128(0));
        }
        out.calls[id.idx()] = calls;
        out.callee_reps[id.idx()] = reps.finish();
    }
    out.calls.resize_with(out.table.len(), HashMap::new);
    out.callee_reps.resize(out.table.len(), Hash128(0));
    Ok(out)
}
