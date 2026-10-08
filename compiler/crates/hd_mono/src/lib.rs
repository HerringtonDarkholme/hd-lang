#![forbid(unsafe_code)]
//! `hd_mono`: monomorphizing collection (codegen.md §13.1 to §13.3) over
//! `hd_tir::ir` bodies into the `InstanceTable`, with A1 classes and
//! instance keys from `layout`, and trait selection through the solver's
//! `select` plus a head match. Collection records each call's target, each
//! closure's code instance and each trait-value coercion's vtable, so
//! emission never selects again. Reads TIR and interfaces only, never
//! syntax. Bodies of every package (std included) are collected alike.

pub mod layout;
pub mod passes;

use std::collections::{BTreeSet, HashMap};

use hd_base::{DefId, Hash128, InstId, ModuleId, NotImplemented, StableHasher, Stage, StageResult};
use hd_tir::ir::{Body, Callee, ChoiceKind, Coercion, Tag};
use hd_types::solver::{ConcreteTraitRef, ImplTable, Solver, TraitRef};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

use crate::layout::{A1Class, KeyArg, LayoutEnv, a1_class, canon, instance_key};

/// What collection reads about the program (the driver implements it over
/// TIR and interfaces).
pub trait ProgramEnv: LayoutEnv {
    fn body(&self, def: DefId) -> Option<&Body>;
    /// Per type parameter of the item itself: has a bound (A1, codegen.md
    /// §13.2: exact).
    fn bounded(&self, def: DefId) -> Option<Vec<bool>>;
    /// The declared result type.
    fn ret(&self, def: DefId) -> Option<Ty>;
    /// The declared parameter types (`self` first for a method).
    fn params(&self, def: DefId) -> Option<Vec<Ty>>;
    /// Whether the function suspends (`fn f!`).
    fn suspends(&self, def: DefId) -> bool;
    /// The requirement keys of an instance's row, as trait items, in key
    /// order (codegen.md §12.4): the declared keys, and the keys of each
    /// row parameter's argument, so providers are passed per instance.
    fn row_keys(&self, def: DefId, args: TyList) -> Vec<DefId>;
    /// A method's owner (impl or trait) and how many of the instance's
    /// arguments are the owner's: an impl's parameters, or a trait's
    /// `Self` and parameters.
    fn parent(&self, def: DefId) -> Option<(DefId, usize)>;
    /// An impl's head: self type, trait arguments, parameter count.
    fn impl_head(&self, impl_: DefId) -> Option<(Ty, TyList, usize)>;
    /// The method of `impl_` that implements the trait method `method`.
    fn impl_method(&self, impl_: DefId, method: DefId) -> Option<DefId>;
    /// A trait's own methods, in declaration order (the vtable shape).
    fn trait_methods(&self, trait_: DefId) -> Vec<DefId>;
    /// A trait's own parameter count, `Self` excluded.
    fn trait_arity(&self, trait_: DefId) -> usize;
    /// The compiler's lowering key of a body-less std function
    /// (`@intrinsic("key")`, or a compiler-supplied item by name).
    fn intrinsic(&self, def: DefId) -> Option<String>;
    /// Data fields, for the struct types a program needs.
    fn data_fields(&self, def: DefId) -> Option<Vec<Ty>>;
    fn path_hash(&self, def: DefId) -> Hash128;
    /// The item's stable path, for diagnostics.
    fn describe(&self, def: DefId) -> String;
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

/// How a call is lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// A direct call of a collected instance.
    Instance,
    /// A compiler lowering of a body-less std function, by intrinsic key.
    Intrinsic(String),
    /// A body-less method of a built-in family (`impl[N < Num] Display for
    /// N`): the method's name at a concrete self type.
    Builtin { method: String, self_ty: Ty },
}

/// Where one call goes: the callee instance's key, its item, arguments and
/// result type under its own arguments (an erased result is cast back at
/// the caller).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallTarget {
    pub key: Hash128,
    pub item: DefId,
    pub args: TyList,
    pub ret: Ty,
    pub kind: TargetKind,
}

/// What collection recorded for one instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Call(CallTarget),
    /// A coercion to a trait value: the trait and one target per vtable slot.
    VTable(DefId, Vec<CallTarget>),
    /// A closure's code instance.
    Closure(Hash128),
    /// A `$.with`'s providers: per key, the trait and its vtable slots.
    Withs(Vec<(DefId, Vec<CallTarget>)>),
}

/// The output of `Collect` (codegen.md §11.3).
#[derive(Debug, Default)]
pub struct Collected {
    pub table: InstanceTable,
    /// Per instance: each instruction's target.
    pub calls: Vec<HashMap<u32, Target>>,
    /// Per instance: the hash of what its code reads besides its own TIR:
    /// callees' representation summaries and signatures (walking skeleton,
    /// SK-3) and the layouts of its types; each code key holds it.
    pub callee_reps: Vec<Hash128>,
    pub imports: BTreeSet<u32>,
    /// Data types the program builds or reads.
    pub data: BTreeSet<DefIdOrd>,
    /// The init bodies' instances, in initialization order.
    pub inits: Vec<Hash128>,
    /// The extra roots' instances, in the order the caller gave them.
    pub extra: Vec<Hash128>,
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

/// Substitutes an instance's arguments for its item's parameters: the
/// owner's (impl or trait) first, then the item's own. A projection whose
/// base becomes concrete resolves to the impl's binding (codegen.md §13).
#[must_use]
pub fn subst(pool: &InternPool, env: &dyn ProgramEnv, item: DefId, args: TyList, t: Ty) -> Ty {
    let a = pool.list_items(args);
    let parent = env.parent(item);
    let n = parent.map_or(0, |p| p.1);
    let s = pool.subst(t, &|p: ParamRef| {
        if p.owner == item {
            a.get(n + p.index as usize).copied()
        } else if parent.is_some_and(|(o, _)| o == p.owner) {
            a.get(p.index as usize).copied()
        } else {
            None
        }
    });
    if pool.has_assoc(s) {
        hd_types::solver::normalize_concrete(pool, &env.impl_tables(), s)
    } else {
        s
    }
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

/// Matches an impl head's `pattern` (over the parameters of `owner`)
/// against a concrete type, filling `out`.
fn unify(pool: &InternPool, owner: DefId, pattern: Ty, t: Ty, out: &mut Vec<Option<Ty>>) {
    let strip = |x: Ty| match pool.get(x) {
        TyData::Mut(i) => i,
        _ => x,
    };
    let (pattern, t) = (strip(pattern), strip(t));
    match (pool.get(pattern), pool.get(t)) {
        (TyData::Param(p), _) if p.owner == owner => {
            let i = p.index as usize;
            if out.len() <= i {
                out.resize(i + 1, None);
            }
            out[i].get_or_insert(t);
        }
        (TyData::Adt { args: a, .. }, TyData::Adt { args: b, .. })
        | (TyData::TraitValue { args: a, .. }, TyData::TraitValue { args: b, .. })
        | (TyData::Tuple { elems: a, .. }, TyData::Tuple { elems: b, .. }) => {
            for (x, y) in pool.list_items(a).into_iter().zip(pool.list_items(b)) {
                unify(pool, owner, x, y, out);
            }
        }
        (TyData::Option(x), TyData::Option(y)) => unify(pool, owner, x, y, out),
        (
            TyData::Fn {
                params: a,
                result: r,
                ..
            },
            TyData::Fn {
                params: b,
                result: s,
                ..
            },
        ) => {
            for (x, y) in pool.list_items(a).into_iter().zip(pool.list_items(b)) {
                unify(pool, owner, x, y, out);
            }
            unify(pool, owner, r, s, out);
        }
        _ => {}
    }
}

/// The hash of a type's layout-relevant shape: its canonical form and,
/// for a declared type, its fields' (codegen.md §13.8 `layout_hash`).
fn layout_hash(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, h: &mut StableHasher, depth: u8) {
    let ph = |d: DefId| env.path_hash(d);
    canon(pool, &ph, t, h);
    if depth > 4 {
        return;
    }
    if let TyData::Adt { def, args } = pool.get(t) {
        for f in env.data_fields(def).unwrap_or_default() {
            layout_hash(pool, env, subst(pool, env, def, args, f), h, depth + 1);
        }
        for v in env.enum_variants(def, args).unwrap_or_default() {
            for f in v {
                layout_hash(pool, env, f, h, depth + 1);
            }
        }
    }
}

struct Cx<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    solver: &'a dyn Solver,
    tables: Vec<(ModuleId, &'a ImplTable)>,
    out: Collected,
    work: Vec<InstId>,
}

/// The type of a value operand: an instruction's, or a constant's when
/// the word has `Ref::CONST_BIT`.
fn value_ty(body: &Body, word: u32) -> Ty {
    let c = hd_tir::ir::Ref::CONST_BIT;
    let t = if word & c == 0 {
        body.ty.get(word as usize).copied()
    } else {
        body.consts.get((word & !c) as usize).map(|k| k.0)
    };
    t.unwrap_or(Ty::POISON)
}

fn err<T>(what: &str) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Collect, what))
}

impl Cx<'_> {
    fn key(&self, item: DefId, sub: u16, args: TyList) -> Hash128 {
        let ph = |d: DefId| self.env.path_hash(d);
        instance_key(self.pool, &ph, item, sub, &key_args(self.pool, args))
    }

    /// Pushes an instance; returns its key.
    fn push(
        &mut self,
        item: DefId,
        sub: u16,
        args: TyList,
        depth: u8,
        parent: InstId,
    ) -> StageResult<Hash128> {
        let key = self.key(item, sub, args);
        let (id, new) =
            self.out
                .table
                .push(item, sub, args, depth.saturating_add(1), parent, key)?;
        if new {
            self.work.push(id);
        }
        Ok(key)
    }

    /// A1 (codegen.md §13.2): an unbounded, move-only argument of a
    /// reference layout is keyed and laid out as the class `REF`.
    fn classify(&self, def: DefId, args: &[Ty], own_from: usize) -> StageResult<TyList> {
        let bounded = self.env.bounded(def).unwrap_or_default();
        let mut a = Vec::new();
        for (k, &t) in args.iter().enumerate() {
            // A row argument is part of the instance (codegen.md §12.4:
            // its keys are the instance's provider parameters).
            let exact = k < own_from
                || bounded.get(k - own_from).copied().unwrap_or(true)
                || is_class_ref(self.pool, t)
                || matches!(self.pool.get(t), TyData::Row(_))
                || self.env.body(def).is_none();
            a.push(
                if !exact && a1_class(self.pool, self.env, t)? == A1Class::Ref {
                    class_ref(self.pool)
                } else {
                    t
                },
            );
        }
        Ok(self.pool.list(&a))
    }

    /// The target of a call of `def` at full arguments `args`.
    fn target(
        &mut self,
        def: DefId,
        args: TyList,
        depth: u8,
        parent: InstId,
    ) -> StageResult<CallTarget> {
        let ret = self.env.ret(def).unwrap_or(Ty::VOID);
        // A std function whose body the compiler supplies, though its
        // source has a placeholder body (`race!`'s frame, the categorized
        // `panic`, `std.rt`'s `entry_write`, the string byte primitives).
        if let Some(key) = self.env.intrinsic(def).filter(|k| {
            matches!(
                k.as_str(),
                "task_race_frame"
                    | "panic"
                    | "entry_write"
                    | "bytes_len"
                    | "bytes_at"
                    | "bytes_slice"
                    | "bytes_concat"
                    | "string_from_bytes"
                    | "char_scalar"
                    | "char_from_scalar"
            )
        }) {
            return Ok(CallTarget {
                key: Hash128(0),
                item: def,
                args,
                ret: subst(self.pool, self.env, def, args, ret),
                kind: TargetKind::Intrinsic(key),
            });
        }
        if self.env.body(def).is_none() {
            let Some(key) = self.env.intrinsic(def) else {
                return Err(NotImplemented::new(
                    Stage::Collect,
                    format!(
                        "a call of the body-less `{}`, which is not an intrinsic",
                        self.env.describe(def)
                    ),
                ));
            };
            return Ok(CallTarget {
                key: Hash128(0),
                item: def,
                args,
                ret: subst(self.pool, self.env, def, args, ret),
                kind: TargetKind::Intrinsic(key),
            });
        }
        let own_from = self.env.parent(def).map_or(0, |p| p.1);
        let args = self.classify(def, &self.pool.list_items(args), own_from)?;
        let key = self.push(def, 0, args, depth, parent)?;
        Ok(CallTarget {
            key,
            item: def,
            args,
            ret: subst(self.pool, self.env, def, args, ret),
            kind: TargetKind::Instance,
        })
    }

    /// Selects the implementation of `trait_` for `self_ty` (codegen.md
    /// §13.2 `select`): the impl and its arguments.
    fn select(
        &self,
        trait_: DefId,
        self_ty: Ty,
        trait_args: TyList,
        choice: Option<DefId>,
    ) -> StageResult<(DefId, Vec<Ty>)> {
        let impl_ = if let Some(d) = choice {
            d
        } else {
            {
                let tref = ConcreteTraitRef(TraitRef {
                    trait_,
                    self_ty,
                    args: trait_args,
                });
                let sel = self.solver.select(self.pool, &self.tables, tref)?;
                let Some((_, t)) = self.tables.iter().find(|(m, _)| *m == sel.impl_row.module)
                else {
                    return err("a selection outside the impl tables");
                };
                t.def[sel.impl_row.row as usize]
            }
        };
        let Some((head, targs, n)) = self.env.impl_head(impl_) else {
            return err("a selected impl without a head");
        };
        let mut out = vec![None; n];
        unify(self.pool, impl_, head, self_ty, &mut out);
        for (x, y) in self
            .pool
            .list_items(targs)
            .into_iter()
            .zip(self.pool.list_items(trait_args))
        {
            unify(self.pool, impl_, x, y, &mut out);
        }
        // Parameters that a bound's binding fixes (`Bind` steps).
        if out.iter().any(Option::is_none)
            && let Some((t, row)) = self
                .tables
                .iter()
                .find_map(|(_, t)| t.def.iter().position(|d| *d == impl_).map(|row| (*t, row)))
        {
            out.resize(out.len().max(n), None);
            hd_types::solver::apply_binds(self.pool, &self.tables, t, row, &mut out);
        }
        let mut args = Vec::new();
        for a in out.into_iter().take(n) {
            let Some(a) = a else {
                return err("an impl parameter that its head does not fix (a `Bind` step)");
            };
            args.push(a);
        }
        Ok((impl_, args))
    }

    /// The target of a trait method at a concrete self type: the impl's
    /// method, the trait's default body, or a built-in lowering.
    fn method_target(
        &mut self,
        trait_: DefId,
        method: DefId,
        self_ty: Ty,
        targs: &[Ty],
        choice: Option<DefId>,
        depth: u8,
        parent: InstId,
    ) -> StageResult<CallTarget> {
        let n_trait = self.env.trait_arity(trait_);
        let trait_args = self.pool.list(&targs[..n_trait.min(targs.len())]);
        let method_args = &targs[n_trait.min(targs.len())..];
        let (impl_, impl_args) = self.select(trait_, self_ty, trait_args, choice)?;
        match self.env.impl_method(impl_, method) {
            Some(m) if self.env.body(m).is_some() => {
                let mut all = impl_args;
                all.extend_from_slice(method_args);
                self.target(m, self.pool.list(&all), depth, parent)
            }
            Some(m) => Ok(CallTarget {
                key: Hash128(0),
                item: m,
                args: TyList::EMPTY,
                ret: subst(
                    self.pool,
                    self.env,
                    method,
                    self.pool.list(&[&[self_ty], targs].concat()),
                    self.env.ret(method).unwrap_or(Ty::VOID),
                ),
                kind: TargetKind::Builtin {
                    method: String::new(),
                    self_ty,
                },
            }),
            None => {
                let mut all = vec![self_ty];
                all.extend_from_slice(targs);
                self.target(method, self.pool.list(&all), depth, parent)
            }
        }
    }

    fn scan(&mut self, id: InstId) -> StageResult<()> {
        let (item, sub, args, depth) = (
            self.out.table.item[id.idx()],
            self.out.table.sub[id.idx()],
            self.out.table.args[id.idx()],
            self.out.table.depth[id.idx()],
        );
        let pool = self.pool;
        let env = self.env;
        let Some(body) = env.body(item) else {
            return err("an instance whose item has no TIR");
        };
        let mut calls = HashMap::new();
        let mut reps = StableHasher::new("callee-reps");
        let mut seen = std::collections::HashSet::new();
        let s = |t: Ty| subst(pool, env, item, args, t);
        for i in 0..body.len() {
            let ty = s(body.ty[i]);
            if seen.insert(ty) {
                layout_hash(pool, env, ty, &mut reps, 0);
            }
            note_data(pool, env, ty, &mut self.out.data);
            let ix = u32::try_from(i).expect("insts");
            let [a, b] = body.data[i];
            match body.tags[i] {
                Tag::CallHost => {
                    self.out.imports.insert(a);
                }
                Tag::Call | Tag::Await => {
                    let Some(c) = Callee::from_words(body.record(a)) else {
                        return err("a malformed callee record");
                    };
                    let t = match c {
                        Callee::Item { def, targs } => {
                            let targs: Vec<Ty> =
                                pool.list_items(targs).into_iter().map(s).collect();
                            self.target(def, pool.list(&targs), depth, id)?
                        }
                        Callee::TraitMethod {
                            trait_,
                            method,
                            self_ty,
                            targs,
                            choice,
                        } => {
                            let self_ty = s(self_ty);
                            let targs: Vec<Ty> =
                                pool.list_items(targs).into_iter().map(s).collect();
                            let pick = match choice.0 {
                                ChoiceKind::Impl => Some(DefId::from_raw(choice.1)),
                                ChoiceKind::Bound => None,
                                ChoiceKind::TraitValue => continue,
                                ChoiceKind::Builtin => {
                                    let name = env.path_hash(method);
                                    reps.hash(name);
                                    calls.insert(
                                        ix,
                                        Target::Call(CallTarget {
                                            key: Hash128(0),
                                            item: method,
                                            args: TyList::EMPTY,
                                            ret: ty,
                                            kind: TargetKind::Builtin {
                                                method: String::new(),
                                                self_ty,
                                            },
                                        }),
                                    );
                                    continue;
                                }
                            };
                            if matches!(pool.get(self_ty), TyData::TraitValue { .. }) {
                                continue;
                            }
                            self.method_target(trait_, method, self_ty, &targs, pick, depth, id)?
                        }
                    };
                    reps.hash(env.path_hash(t.item));
                    reps.hash(t.key);
                    let ph = |d: DefId| env.path_hash(d);
                    canon(pool, &ph, t.ret, &mut reps);
                    for p in env.params(t.item).unwrap_or_default() {
                        canon(pool, &ph, subst(pool, env, t.item, t.args, p), &mut reps);
                    }
                    for b in env.bounded(t.item).unwrap_or_default() {
                        reps.u8(u8::from(b));
                    }
                    for k in env.row_keys(t.item, t.args) {
                        reps.hash(env.path_hash(k));
                    }
                    calls.insert(ix, Target::Call(t));
                }
                Tag::Closure => {
                    let sub_k = u16::try_from(a).expect("subs");
                    let key = self.push(item, sub_k, args, depth, id)?;
                    calls.insert(ix, Target::Closure(key));
                }
                Tag::Coerce => {
                    let rec = body.record(b);
                    if rec.first().copied() != Some(Coercion::ToTraitValue as u32) {
                        continue;
                    }
                    let from = s(value_ty(body, a));
                    let from = match pool.get(from) {
                        TyData::Mut(x) => x,
                        _ => from,
                    };
                    let to = match pool.get(ty) {
                        TyData::Mut(x) => x,
                        _ => ty,
                    };
                    let TyData::TraitValue {
                        def: trait_,
                        args: trait_args,
                        ..
                    } = pool.get(to)
                    else {
                        return err("a trait-value coercion to a non-trait type");
                    };
                    let targs = pool.list_items(trait_args);
                    let mut slots = Vec::new();
                    for m in env.trait_methods(trait_) {
                        let t = self.method_target(trait_, m, from, &targs, None, depth, id)?;
                        reps.hash(t.key);
                        slots.push(t);
                    }
                    calls.insert(ix, Target::VTable(trait_, slots));
                }
                Tag::With => {
                    let rec = body.record(a).to_vec();
                    let mut withs = Vec::new();
                    for pair in rec.chunks(2) {
                        let [k, v] = pair else {
                            return err("a malformed `$.with` record");
                        };
                        let TyData::TraitValue {
                            def: trait_,
                            args: trait_args,
                            ..
                        } = pool.get(s(Ty(*k)))
                        else {
                            return err("a `$.with` key that is not a trait");
                        };
                        let from = s(value_ty(body, *v));
                        let from = match pool.get(from) {
                            TyData::Mut(x) => x,
                            _ => from,
                        };
                        let targs = pool.list_items(trait_args);
                        let mut slots = Vec::new();
                        for m in env.trait_methods(trait_) {
                            let t = self.method_target(trait_, m, from, &targs, None, depth, id)?;
                            reps.hash(t.key);
                            slots.push(t);
                        }
                        withs.push((trait_, slots));
                    }
                    calls.insert(ix, Target::Withs(withs));
                }
                _ => {}
            }
        }
        reps.u16(sub);
        if self.out.calls.len() <= id.idx() {
            self.out.calls.resize_with(id.idx() + 1, HashMap::new);
            self.out.callee_reps.resize(id.idx() + 1, Hash128(0));
        }
        self.out.calls[id.idx()] = calls;
        self.out.callee_reps[id.idx()] = reps.finish();
        Ok(())
    }
}

fn note_data(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, out: &mut BTreeSet<DefIdOrd>) {
    if let TyData::Adt { def, .. } = pool.get(t)
        && env.data_fields(def).is_some()
        && out.insert(DefIdOrd(env.path_hash(def), def.raw()))
    {
        for f in env.data_fields(def).unwrap_or_default() {
            note_data(pool, env, f, out);
        }
    }
}

/// Collection from `root` (codegen.md §13.1) and the init bodies `inits`
/// of the groups it reaches, in initialization order: every reachable
/// instance, each call's target, the imports and data types.
pub fn collect(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    solver: &dyn Solver,
    root: DefId,
    inits: &[DefId],
    extra: &[(DefId, TyList)],
) -> StageResult<Collected> {
    let mut cx = Cx {
        pool,
        env,
        solver,
        tables: env.impl_tables(),
        out: Collected::default(),
        work: Vec::new(),
    };
    cx.push(root, 0, TyList::EMPTY, 0, InstId::NONE)?;
    for i in inits {
        let k = cx.push(*i, 0, TyList::EMPTY, 0, InstId::NONE)?;
        cx.out.inits.push(k);
    }
    // Extra roots with type arguments: the test cases after the first and
    // the `std.rt` result functions (engines-and-test-runner.md §19.1).
    for (d, args) in extra {
        let t = cx.target(*d, *args, 0, InstId::NONE)?;
        cx.out.extra.push(t.key);
    }
    // Breadth-first in push order (codegen.md §13.4): the first instance
    // over a limit is the same on every run.
    let mut next = 0;
    while next < cx.work.len() {
        let id = cx.work[next];
        next += 1;
        cx.scan(id)?;
    }
    let mut out = cx.out;
    out.calls.resize_with(out.table.len(), HashMap::new);
    out.callee_reps.resize(out.table.len(), Hash128(0));
    Ok(out)
}
