//! Stage B of header validation (resolution-and-interfaces.md §4.10.1;
//! trait-solver.md §1.1, §3.7, §5.1), the `HeaderCheck(F)` task's work:
//! the header checks that need goals. Written header types against their
//! declared bounds, each impl's supertraits, and delegation targets are
//! `Implements` goals asked through the solver, under the item's
//! environment (built as a body builds its own), with the folder's view
//! of the impls: its closure's tables and impl universe (§3.2). Each
//! item's goals share one fuel budget and one item memo.
//!
//! Compiler-supplied traits are not the solver's yet (§3.9), so a goal on
//! one of them reports nothing here, and neither does a goal the solver
//! cannot settle (`Overflow`, fuel): only `Fails` is an error.

use hd_base::{DefId, Fuel, StageResult, Symbol};
use hd_diag::Code;
use hd_resolve::{FnSig, Generic, ImplKind, Item, ItemData, Lookup, Names};
use hd_types::solver::{
    Answer, BodyMemo, GlobalMemo, Goal, ImplView, Impls, ParamEnv, SolveCx, Solver, TraitRef,
};
use hd_types::{ParamRef, Ty, TyData, TyList, Types};

use crate::body::{add_bound, trait_extends};

/// What a folder's header check sees: the items of its closure, its view
/// of the impls, the run memo and the solver.
pub struct HeaderCx<'a> {
    pub names: Names<'a>,
    pub lookup: &'a Lookup<'a>,
    pub impls: &'a ImplView<'a>,
    pub global: &'a GlobalMemo,
    pub solver: &'a dyn Solver,
}

/// One problem stage B found: the item, a code and a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub item: DefId,
    /// Which written position of the item (`anchor` slots): 0 is its header.
    pub slot: u32,
    pub code: Code,
    pub message: String,
}

/// Stage B over a folder's items, in item order: bounds of written header
/// types (`trait.bound.no-implied`), the supertraits of each impl
/// (`trait.super.impl-bounds`), and delegation targets.
pub fn stage_b(cx: &HeaderCx<'_>, items: &[Item]) -> StageResult<Vec<Finding>> {
    let mut out = Vec::new();
    for it in items {
        let mut env = environment(cx, it);
        env.key = Some(cx.global.env_key(&env));
        let mut hc = ItemCheck {
            cx,
            env,
            memo: BodyMemo::default(),
            fuel: Fuel::new(Fuel::BODY_DEFAULT),
            item: it.def,
            out: &mut out,
        };
        hc.item(it)?;
    }
    Ok(out)
}

/// The derived implementations of `items` whose type is a newtype: the base
/// type must implement the derived trait
/// (`trait.derive.newtype.requires`); one that does not is an error at the
/// base type, slot 1 of the derived implementation
/// (`trait.derive.newtype.requires.error`). `items` are a module's own
/// items, private ones included: a body check's view, not an interface's.
pub fn derived_newtypes(cx: &HeaderCx<'_>, items: &[Item]) -> StageResult<Vec<Finding>> {
    let mut out = Vec::new();
    for it in items {
        let ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            kind: ImplKind::Derived,
            ..
        } = &it.data
        else {
            continue;
        };
        let mut env = environment(cx, it);
        env.key = Some(cx.global.env_key(&env));
        let mut hc = ItemCheck {
            cx,
            env,
            memo: BodyMemo::default(),
            fuel: Fuel::new(Fuel::BODY_DEFAULT),
            item: it.def,
            out: &mut out,
        };
        let tv = hc.pool().intern_ty(&TyData::TraitValue {
            def: *trait_,
            args: *trait_args,
            bindings: vec![],
        });
        hc.derived_newtype(*self_ty, tv)?;
    }
    Ok(out)
}

/// The parameter environment of an item: its own parameters' bounds,
/// after its owner's (an impl's parameters, or a trait's `Self` bounded
/// by the trait and the trait's parameters).
fn environment(cx: &HeaderCx<'_>, it: &Item) -> ParamEnv {
    let pool = cx.names.pool.types();
    let mut env = ParamEnv::default();
    let generics = |owner: DefId, gs: &[Generic], offset: u16, env: &mut ParamEnv| {
        for (i, g) in gs.iter().enumerate() {
            if g.row {
                continue;
            }
            let index = offset + u16::try_from(i).unwrap_or(u16::MAX);
            let p = pool.intern_ty(&TyData::Param(ParamRef { owner, index }));
            for b in &g.bounds {
                add_bound(cx.lookup, cx.names.pool, pool, env, (p, *b), 0);
            }
        }
    };
    let owner = |owner: DefId, env: &mut ParamEnv| {
        let Some(o) = cx.lookup.item(owner) else {
            return;
        };
        if let ItemData::Trait(_) = o.data {
            let s = pool.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
            let args: Vec<Ty> = (1..=o.generics.len())
                .map(|i| {
                    pool.intern_ty(&TyData::Param(ParamRef {
                        owner,
                        index: u16::try_from(i).unwrap_or(u16::MAX),
                    }))
                })
                .collect();
            let tv = pool.intern_ty(&TyData::TraitValue {
                def: owner,
                args: pool.list(&args),
                bindings: vec![],
            });
            add_bound(cx.lookup, cx.names.pool, pool, env, (s, tv), 0);
            generics(owner, &o.generics, 1, env);
        } else {
            generics(owner, &o.generics, 0, env);
        }
    };
    match &it.data {
        ItemData::Method { owner: o, sig, .. } => {
            owner(*o, &mut env);
            generics(it.def, &sig.generics, 0, &mut env);
        }
        ItemData::Fn(sig) => generics(it.def, &sig.generics, 0, &mut env),
        ItemData::Trait(_) => owner(it.def, &mut env),
        _ => generics(it.def, &it.generics, 0, &mut env),
    }
    env
}

/// One item's goals: its environment, memo and fuel.
struct ItemCheck<'c, 'a> {
    cx: &'c HeaderCx<'a>,
    env: ParamEnv,
    memo: BodyMemo,
    fuel: Fuel,
    item: DefId,
    out: &'c mut Vec<Finding>,
}

impl<'a> ItemCheck<'_, 'a> {
    fn pool(&self) -> Types<'a> {
        self.cx.names.pool.types()
    }

    fn show(&self, t: Ty) -> String {
        hd_resolve::show_ty(&self.cx.names, t)
    }

    fn report(&mut self, slot: u32, code: Code, message: String) {
        self.out.push(Finding {
            item: self.item,
            slot,
            code,
            message,
        });
    }

    fn item(&mut self, it: &Item) -> StageResult<()> {
        let tys: Vec<Ty> = match &it.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => sig_tys(s),
            ItemData::Data(fs) => fs.iter().map(|f| f.ty).collect(),
            ItemData::Enum { shared, variants } => shared
                .iter()
                .chain(variants.iter().flat_map(|v| &v.fields))
                .map(|f| f.ty)
                .collect(),
            ItemData::Impl { self_ty, .. } => vec![*self_ty],
            ItemData::Alias(t) | ItemData::Newtype(t) => vec![*t],
            ItemData::Trait(_) | ItemData::AssocType { .. } => vec![],
        };
        // Slot 0 is the header; written types of functions, data and
        // enums have their own (`anchor`).
        let positional = matches!(
            it.data,
            ItemData::Fn(_)
                | ItemData::Method { .. }
                | ItemData::Data(_)
                | ItemData::Enum { .. }
                | ItemData::Newtype(_)
        );
        for (i, t) in tys.into_iter().enumerate() {
            let slot = if positional {
                u32::try_from(i + 1).unwrap_or(0)
            } else {
                0
            };
            self.check_ty(t, slot)?;
        }
        let ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            kind,
            by,
            ..
        } = &it.data
        else {
            return Ok(());
        };
        if *trait_ == DefId::NONE || !kind.is_impl() || *kind == ImplKind::TupleTemplate {
            return Ok(());
        }
        let pool = self.pool();
        let tv = pool.intern_ty(&TyData::TraitValue {
            def: *trait_,
            args: *trait_args,
            bindings: vec![],
        });
        for s in self.supers(*trait_, *trait_args, *self_ty) {
            if self.fails(*self_ty, s)? {
                let message = format!(
                    "{} implements {} but not {}",
                    self.show(*self_ty),
                    self.cx.names.display_name(*trait_),
                    self.show(s)
                );
                self.report(0, Code::MissingSupertraitImplementation, message);
            }
        }
        if *kind == ImplKind::Delegated
            && let Some(by) = by
        {
            self.delegation(*self_ty, tv, *by)?;
        }
        Ok(())
    }

    /// A derived newtype's base type must implement the derived trait
    /// (`trait.derive.newtype.requires`); one that does not is an error at
    /// the base type (`trait.derive.newtype.requires.error`).
    fn derived_newtype(&mut self, self_ty: Ty, tv: Ty) -> StageResult<()> {
        let pool = self.pool();
        let TyData::Adt { def, args } = pool.get(self_ty) else {
            return Ok(());
        };
        let Some(ItemData::Newtype(inner)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return Ok(());
        };
        let args = pool.list_items(args);
        let base = pool.subst(*inner, &|p: ParamRef| {
            (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
        });
        if !self.fails(base, tv)? {
            return Ok(());
        }
        let TyData::TraitValue { def: trait_, .. } = pool.get(tv) else {
            return Ok(());
        };
        let message = format!(
            "the base type {} of {} does not implement {}, so {} cannot derive it",
            self.show(base),
            self.show(self_ty),
            self.cx.names.display_name(trait_),
            self.show(self_ty)
        );
        // The derived implementation's slot 1 is the base type.
        self.report(1, Code::DeriveFieldMissingTrait, message);
        Ok(())
    }

    /// The supertraits of `trait_[args]`, `Self` replaced by `self_ty`.
    fn supers(&self, trait_: DefId, args: TyList, self_ty: Ty) -> Vec<Ty> {
        let pool = self.pool();
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(trait_).map(|i| &i.data) else {
            return Vec::new();
        };
        let args = pool.list_items(args);
        t.supers
            .iter()
            .map(|s| {
                pool.subst(*s, &|p: ParamRef| {
                    (p.owner == trait_).then(|| {
                        if p.index == 0 {
                            Some(self_ty)
                        } else {
                            args.get(usize::from(p.index) - 1).copied()
                        }
                    })?
                })
            })
            .collect()
    }

    /// Whether `t` provably does not implement the trait value `bound`
    /// under the item's environment: the solver answers `Fails`.
    fn fails(&mut self, t: Ty, bound: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let TyData::TraitValue { def, args, .. } = pool.get(bound) else {
            return Ok(false);
        };
        let k = self.cx.names.known;
        if [
            k.any,
            k.any_val,
            k.any_ref,
            k.tuple,
            k.structure,
            k.inspectable,
        ]
        .contains(&def)
        {
            return Ok(false);
        }
        let inner = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        match pool.get(inner) {
            TyData::Never => return Ok(false),
            // A trait value implements its trait and its supertraits.
            TyData::TraitValue { def: d, .. }
                if d == def || trait_extends(self.cx.lookup, pool, d, def, 0) =>
            {
                return Ok(false);
            }
            _ => {}
        }
        let tref = TraitRef {
            trait_: def,
            self_ty: t,
            args,
        };
        let goal = Goal::Implements {
            tref,
            bindings: vec![],
            mut_: false,
        };
        let mut scx = SolveCx {
            pool,
            env: &self.env,
            universe: self.cx.impls.universe,
            impls: Impls::Owned(self.cx.impls),
            body_memo: &mut self.memo,
            global: self.cx.global,
        };
        let answer = self.cx.solver.solve(&mut scx, &goal, &mut self.fuel)?;
        Ok(matches!(answer, Answer::Fails(_)))
    }

    /// Written types against their declared bounds (`trait.bound.no-implied`).
    fn check_ty(&mut self, t: Ty, slot: u32) -> StageResult<()> {
        let pool = self.pool();
        match pool.get(t) {
            TyData::Adt { def, args } => {
                let args_v = pool.list_items(args);
                if let Some(target) = self.cx.lookup.item(def) {
                    for (i, g) in target.generics.iter().enumerate() {
                        let Some(&a) = args_v.get(i) else { continue };
                        for b in &g.bounds {
                            let b = pool.subst(*b, &|p: ParamRef| {
                                (p.owner == def)
                                    .then(|| args_v.get(usize::from(p.index)).copied())?
                            });
                            if self.fails(a, b)? {
                                let message = format!(
                                    "{} does not implement {} in `{}`",
                                    self.show(a),
                                    self.show(b),
                                    self.cx.names.display_name(self.item)
                                );
                                self.report(slot, Code::UnsatisfiedTraitBound, message);
                            }
                        }
                    }
                }
                for &a in args_v {
                    self.check_ty(a, slot)?;
                }
            }
            TyData::Option(i) | TyData::Mut(i) => self.check_ty(i, slot)?,
            TyData::Tuple { elems, rest } => {
                for e in pool.list_items(elems).iter().copied().chain(rest) {
                    self.check_ty(e, slot)?;
                }
            }
            TyData::Fn { params, result, .. } => {
                for e in pool.list_items(params).iter().copied() {
                    self.check_ty(e, slot)?;
                }
                self.check_ty(result, slot)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// A delegation target: the embedded field `by` implements the trait.
    fn delegation(&mut self, self_ty: Ty, tv: Ty, by: Symbol) -> StageResult<()> {
        let pool = self.pool();
        let TyData::Adt { def, args } = pool.get(self_ty) else {
            return Ok(());
        };
        let Some(ItemData::Data(fields)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return Ok(());
        };
        let args = pool.list_items(args);
        let Some(f) = fields.iter().find(|f| f.embedded && f.name == by) else {
            let message = format!(
                "`{}` has no embedded field `{}`",
                self.show(self_ty),
                self.cx.names.text(by)
            );
            self.report(0, Code::InvalidDelegation, message);
            return Ok(());
        };
        let ft = pool.subst(f.ty, &|p: ParamRef| {
            (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
        });
        if self.fails(ft, tv)? {
            let message = format!(
                "the field `{}` does not implement {}",
                self.cx.names.text(by),
                self.show(tv)
            );
            self.report(0, Code::InvalidDelegation, message);
        }
        Ok(())
    }
}

fn sig_tys(sig: &FnSig) -> Vec<Ty> {
    sig.params.iter().map(|p| p.1).chain([sig.ret]).collect()
}
