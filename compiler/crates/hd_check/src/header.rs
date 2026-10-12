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
use hd_resolve::{FnSig, Generic, ImplKind, Item, ItemData, Lookup, LookupDecls, Names};
use hd_types::solver::{
    Answer, BodyMemo, GlobalMemo, Goal, ImplView, Impls, ParamEnv, SolveCx, Solver, TraitRef,
};
use hd_types::{ParamRef, RowId, Ty, TyData, TyList, Types};

use crate::body::add_bound;

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

/// Stage B over a folder's interface items that `HeaderCheck(F)` covers
/// ([`in_interface_check`]), in item order: bounds of written header types
/// (`trait.bound.no-implied`), the supertraits of each impl
/// (`trait.super.impl-bounds`), and delegation targets.
pub fn stage_b(cx: &HeaderCx<'_>, items: &[Item]) -> StageResult<Vec<Finding>> {
    stage_b_over(
        cx,
        items
            .iter()
            .filter(|it| in_interface_check(&cx.names, cx.lookup, it)),
    )
}

/// Stage B over the items of one module that `HeaderCheck` leaves out
/// ([`in_interface_check`]): its private items, and its impls that name
/// one. `cx`'s lookup holds the module's own items, private ones included:
/// a body check's view, not an interface's.
pub fn private_items(cx: &HeaderCx<'_>, items: &[Item]) -> StageResult<Vec<Finding>> {
    stage_b_over(
        cx,
        items
            .iter()
            .filter(|it| !in_interface_check(&cx.names, cx.lookup, it)),
    )
}

/// Whether `HeaderCheck(F)` checks `it`: an item that the interfaces of
/// `lookup` hold, whose header names only items they hold. Any other item,
/// a private one or an impl that names one, is checked with its module's
/// bodies ([`private_items`]), where its private names are seen.
#[must_use]
pub fn in_interface_check(names: &Names<'_>, lookup: &Lookup<'_>, it: &Item) -> bool {
    let held = |d: DefId| lookup.ifaces.iter().any(|f| f.item(d).is_some());
    held(it.def)
        && hd_resolve::mentioned_defs(names.pool, std::slice::from_ref(it), &[])
            .into_iter()
            .all(held)
}

fn stage_b_over<'i>(
    cx: &HeaderCx<'_>,
    items: impl Iterator<Item = &'i Item>,
) -> StageResult<Vec<Finding>> {
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

/// The supertrait check of a local trait impl, with `cx`'s view of the
/// impls as it is where the impl is declared: a supertrait impl declared
/// later is not known yet (`names.member.known-impl`,
/// `trait.impl.local.lookup`).
pub fn local_supertraits(cx: &HeaderCx<'_>, it: &Item) -> StageResult<Vec<Finding>> {
    let mut out = Vec::new();
    if !matches!(&it.data, ItemData::Impl { trait_, kind, .. }
        if *trait_ != DefId::NONE && kind.is_impl() && *kind != ImplKind::TupleTemplate)
    {
        return Ok(out);
    }
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
    hc.supertraits(it)?;
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

/// Where an entry-point finding sits in the declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryAt {
    /// The function's name.
    Name,
    /// The written result type.
    Result,
    /// A requirement key, by its trait.
    Key(DefId),
}

/// One problem with an entry point (`module.entry.*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryFinding {
    pub at: EntryAt,
    pub code: Code,
    pub message: String,
}

/// The rules of a public `main` or `main!` in an entry module
/// (`module.entry.parameters`, `.result-termination`, `.row.host`). A
/// function with parameters or generic parameters is one error and nothing
/// more: its result and row are not checked. Otherwise each requirement
/// key must be one of the `host` capability traits of the runtime profile,
/// and the result must implement `std.process.Termination`, asked of the
/// solver as an ordinary trait bound.
pub fn entry_point(cx: &HeaderCx<'_>, it: &Item, host: &[DefId]) -> StageResult<Vec<EntryFinding>> {
    let mut out = Vec::new();
    let ItemData::Fn(sig) = &it.data else {
        return Ok(out);
    };
    let pool = cx.names.pool.types();
    let name = if sig.suspends { "main!" } else { "main" };
    if !sig.params.is_empty() || !sig.generics.is_empty() {
        out.push(EntryFinding {
            at: EntryAt::Name,
            code: Code::EntryPointParameters,
            message: format!(
                "the entry point `{name}` takes no parameters and no generic parameters"
            ),
        });
        return Ok(out);
    }
    for key in pool.row_data(sig.row).keys {
        let bare = match pool.get(key) {
            TyData::Mut(inner) => inner,
            _ => key,
        };
        let def = match pool.get(bare) {
            TyData::TraitValue { def, .. } => def,
            _ => DefId::NONE,
        };
        if def == DefId::NONE || !host.contains(&def) {
            out.push(EntryFinding {
                at: if def == DefId::NONE {
                    EntryAt::Name
                } else {
                    EntryAt::Key(def)
                },
                code: Code::NonhostEntryRequirement,
                message: format!(
                    "`{}` is not a host capability trait of the runtime profile, so the entry point `{name}` cannot require it",
                    hd_resolve::show_ty(&cx.names, key)
                ),
            });
        }
    }
    if pool.has_poison(sig.ret) {
        return Ok(out);
    }
    let mut env = environment(cx, it);
    env.key = Some(cx.global.env_key(&env));
    let mut findings = Vec::new();
    let mut hc = ItemCheck {
        cx,
        env,
        memo: BodyMemo::default(),
        fuel: Fuel::new(Fuel::BODY_DEFAULT),
        item: it.def,
        out: &mut findings,
    };
    let termination = pool.intern_ty(&TyData::TraitValue {
        def: cx.names.item("std.process", "Termination"),
        args: TyList::EMPTY,
        bindings: vec![],
    });
    if hc.fails(sig.ret, termination)? {
        out.push(EntryFinding {
            at: EntryAt::Result,
            code: Code::UnsatisfiedTraitBound,
            message: format!(
                "{} does not implement `Termination`, so the entry point `{name}` cannot return it",
                hd_resolve::show_ty(&cx.names, sig.ret)
            ),
        });
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
        let exposed = self.exposed(it);
        if exposed {
            self.header_leaks(it);
        }
        // Where the signature's row keys start in `tys`.
        let mut keys_from = usize::MAX;
        let tys: Vec<Ty> = match &it.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => {
                let mut tys = sig_tys(s);
                keys_from = tys.len();
                tys.extend(self.pool().row_data(s.row).keys.iter().copied());
                tys
            }
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
        // The positions that other modules can see: every position of a
        // function or enum, but of data only the `pub` fields and the
        // embedded fields (`module.vis.signature.coverage`).
        let open: Vec<bool> = match &it.data {
            ItemData::Data(fs) => fs.iter().map(|f| f.public || f.embedded).collect(),
            _ => vec![true; tys.len()],
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
                | ItemData::Alias(_)
        );
        // A method's `self` is the impl's or trait's own target, checked
        // once at the header.
        let skip_self = matches!(&it.data, ItemData::Method { sig, .. }
            if sig.params.first().is_some_and(|p| self.cx.names.text(p.0) == "self"));
        for (i, t) in tys.into_iter().enumerate() {
            if skip_self && i == 0 {
                continue;
            }
            let slot = if positional {
                u32::try_from(i + 1).unwrap_or(0)
            } else {
                0
            };
            if i >= keys_from {
                self.check_key(t, slot);
            }
            if exposed && open.get(i).copied().unwrap_or(true) {
                self.leak(t, slot);
            }
            self.check_ty(t, slot)?;
        }
        // The bounds the item writes are written types too
        // (`trait.bound.no-implied.positions`); they report at the header.
        let gens: &[Generic] = match &it.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => &s.generics,
            _ => &it.generics,
        };
        for b in gens.iter().filter(|g| !g.row).flat_map(|g| &g.bounds) {
            self.check_bound(*b)?;
        }
        match &it.data {
            ItemData::Trait(t) => {
                for s in &t.supers {
                    self.check_bound(*s)?;
                }
            }
            ItemData::Impl { trait_args, .. } => {
                for a in self.pool().list_items(*trait_args) {
                    self.check_ty(*a, 0)?;
                }
            }
            _ => {}
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
        // A local impl's supertraits are those implemented where it is
        // declared, which its body's checker asks (`local_supertraits`).
        if !self.cx.names.is_local(it.def) {
            self.supertraits(it)?;
        }
        if *kind == ImplKind::Delegated
            && let Some(by) = by
        {
            let tv = self.pool().intern_ty(&TyData::TraitValue {
                def: *trait_,
                args: *trait_args,
                bindings: vec![],
            });
            self.delegation(*self_ty, tv, *by)?;
        }
        Ok(())
    }

    /// Whether the item is part of what other modules see: a `pub`
    /// declaration, a member of a `pub` trait, or a `pub` inherent method of
    /// a type that is itself visible. An implementation is not a declaration
    /// of its own, and its signature is its trait's.
    fn exposed(&self, it: &Item) -> bool {
        let lookup = self.cx.lookup;
        match &it.data {
            ItemData::Impl { .. } => false,
            ItemData::AssocType { owner, .. } => lookup.item(*owner).is_some_and(|o| o.public),
            ItemData::Method { owner, .. } => match lookup.item(*owner).map(|o| &o.data) {
                Some(ItemData::Trait(_)) => it.public,
                Some(ItemData::Impl {
                    trait_, self_ty, ..
                }) if *trait_ == DefId::NONE => {
                    it.public
                        && !matches!(self.pool().get(*self_ty),
                            TyData::Adt { def, .. } if self.is_private(def))
                }
                _ => false,
            },
            _ => it.public,
        }
    }

    /// Whether `def` is a declaration of the module that no other module
    /// can use.
    fn is_private(&self, def: DefId) -> bool {
        self.cx.lookup.item(def).is_some_and(|i| !i.public)
    }

    /// The first module-private declaration that the written type `t`
    /// reaches, nested types, arguments and requirement keys included.
    fn private_in(&self, t: Ty) -> Option<DefId> {
        let pool = self.pool();
        let any = |ts: &[Ty]| ts.iter().find_map(|&x| self.private_in(x));
        let keys = |row: RowId| any(&pool.row_data(row).keys);
        match pool.get(t) {
            TyData::Adt { def, args } => self
                .is_private(def)
                .then_some(def)
                .or_else(|| any(pool.list_items(args))),
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => self.is_private(def).then_some(def).or_else(|| {
                any(pool.list_items(args))
                    .or_else(|| bindings.iter().find_map(|b| self.private_in(b.1)))
            }),
            TyData::Assoc {
                trait_,
                self_ty,
                args,
                ..
            } => self
                .is_private(trait_)
                .then_some(trait_)
                .or_else(|| self.private_in(self_ty))
                .or_else(|| any(pool.list_items(args))),
            TyData::Option(i) | TyData::Mut(i) => self.private_in(i),
            TyData::Tuple { elems, rest } => {
                any(pool.list_items(elems)).or_else(|| rest.and_then(|r| self.private_in(r)))
            }
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => any(pool.list_items(params))
                .or_else(|| self.private_in(result))
                .or_else(|| keys(row)),
            TyData::Row(row) | TyData::Context(row) => keys(row),
            _ => None,
        }
    }

    /// A written type of a public signature may not reach a module-private
    /// declaration (`module.vis.signature`): one error for the position, at
    /// the first such declaration.
    fn leak(&mut self, t: Ty, slot: u32) {
        let Some(def) = self.private_in(t) else {
            return;
        };
        let message = format!(
            "`{}` is private, so the public `{}` cannot name it in its signature",
            self.cx.names.display_name(def),
            self.cx.names.display_name(self.item)
        );
        self.report(slot, Code::PrivateTypeLeak, message);
    }

    /// The bounds, defaults and supertraits of a public declaration's
    /// header, each a position of its own at the header.
    fn header_leaks(&mut self, it: &Item) {
        let mut tys: Vec<Ty> = Vec::new();
        let mut gens = |gs: &[Generic]| {
            for g in gs {
                tys.extend(g.bounds.iter().copied());
                tys.extend(g.default);
            }
        };
        gens(&it.generics);
        match &it.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => gens(&s.generics),
            _ => {}
        }
        match &it.data {
            ItemData::Trait(t) => tys.extend(t.supers.iter().copied()),
            ItemData::AssocType {
                bounds, default, ..
            } => {
                tys.extend(bounds.iter().copied());
                tys.extend(*default);
            }
            _ => {}
        }
        for t in tys {
            self.leak(t, 0);
        }
    }

    /// An impl's target must implement each supertrait of its trait
    /// (`trait.super.impl-bounds`).
    fn supertraits(&mut self, it: &Item) -> StageResult<()> {
        let ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            ..
        } = &it.data
        else {
            return Ok(());
        };
        for s in self.supers(*trait_, *trait_args, *self_ty) {
            if self.fails(*self_ty, s)? {
                let message = format!(
                    "{} implements {} but not {}",
                    self.show(*self_ty),
                    self.cx.names.display_name(*trait_),
                    self.show(s)
                );
                self.report(0, Code::MissingSupertraitImplementation, message);
            } else {
                self.super_bindings(*trait_, *self_ty, s);
            }
        }
        Ok(())
    }

    /// The target's implementation of the supertrait `s` binds each
    /// associated type its supertrait list fixes to that type
    /// (`trait.binding.super.mismatch`). A binding that depends on a type
    /// parameter is left as it is.
    fn super_bindings(&mut self, trait_: DefId, self_ty: Ty, s: Ty) {
        let pool = self.pool();
        let TyData::TraitValue {
            def,
            args,
            bindings,
        } = pool.get(s)
        else {
            return;
        };
        let impls = Impls::Owned(self.cx.impls);
        for (assoc, want) in bindings {
            let proj = pool.intern_ty(&TyData::Assoc {
                assoc,
                trait_: def,
                self_ty,
                args,
            });
            let got = hd_types::solver::normalize_concrete(pool, impls, proj);
            let want = hd_types::solver::normalize_concrete(pool, impls, want);
            let open = |t: Ty| pool.has_assoc(t) || pool.has_param(t) || pool.has_poison(t);
            if got != want && !open(got) && !open(want) {
                let message = format!(
                    "{} implements {} with `{}` as {}, but {} requires {}",
                    self.show(self_ty),
                    self.cx.names.display_name(def),
                    self.cx.names.display_name(assoc),
                    self.show(got),
                    self.cx.names.display_name(trait_),
                    self.show(want)
                );
                self.report(0, Code::MissingSupertraitImplementation, message);
            }
        }
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
        let inner = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        if inner == Ty::NEVER {
            return Ok(false);
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
        let decls = LookupDecls {
            lookup: self.cx.lookup,
            sealed: self.cx.names.known.sealed(),
            paths: self.cx.names.paths,
            syms: self.cx.names.syms,
        };
        let mut scx = SolveCx {
            pool,
            env: &self.env,
            universe: self.cx.impls.universe,
            impls: Impls::Owned(self.cx.impls),
            decls: &decls,
            body_memo: &mut self.memo,
            global: self.cx.global,
        };
        let answer = self.cx.solver.solve(&mut scx, &goal, &mut self.fuel)?;
        Ok(matches!(answer, Answer::Fails(_)))
    }

    /// A written requirement key may not be `Inspectable` or extend it
    /// (`req.key.inspectable.error`).
    fn check_key(&mut self, key: Ty, slot: u32) {
        let pool = self.pool();
        if crate::body::is_inspectable_key(
            self.cx.lookup,
            pool,
            self.cx.names.known.inspectable,
            key,
        ) {
            let message = format!(
                "{} is `Inspectable` or extends it, so it cannot be a requirement key",
                self.show(key)
            );
            self.report(slot, Code::InspectableRequirement, message);
        }
    }

    /// The keys of a row written in a header type.
    fn check_row(&mut self, row: RowId, slot: u32) -> StageResult<()> {
        for key in self.pool().row_data(row).keys {
            self.check_key(key, slot);
            self.check_ty(key, slot)?;
        }
        Ok(())
    }

    /// A written bound: the types in its arguments and bindings are
    /// written types (`trait.bound.no-implied.positions`). Its own
    /// arguments may be left to defaults, so only they are checked.
    fn check_bound(&mut self, b: Ty) -> StageResult<()> {
        let pool = self.pool();
        if let TyData::TraitValue { args, bindings, .. } = pool.get(b) {
            for &a in pool.list_items(args) {
                self.check_ty(a, 0)?;
            }
            for (_, t) in bindings {
                self.check_ty(t, 0)?;
            }
        }
        Ok(())
    }

    /// Written types against their declared bounds (`trait.bound.no-implied`).
    fn check_ty(&mut self, t: Ty, slot: u32) -> StageResult<()> {
        let pool = self.pool();
        match pool.get(t) {
            TyData::Adt { def, args } => {
                let args_v = pool.list_items(args);
                if let Some(target) = self.cx.lookup.item(def) {
                    // `types.generic.default.written-missing`: a slot left
                    // out of a written type needs a default.
                    let slots: Vec<&Generic> = target.generics.iter().filter(|g| !g.row).collect();
                    // `types.generic.too-long`: no more arguments than parameters.
                    if args_v.len() > slots.len() {
                        let message = format!(
                            "`{}` takes {} type arguments, not {}",
                            self.cx.names.display_name(def),
                            slots.len(),
                            args_v.len()
                        );
                        self.report(slot, Code::ArgumentCount, message);
                    }
                    if let Some(g) = slots
                        .get(args_v.len()..)
                        .and_then(|rest| rest.iter().find(|g| g.default.is_none()))
                    {
                        let message = format!(
                            "`{}` is written without its parameter `{}`, which has no default",
                            self.cx.names.display_name(def),
                            self.cx.names.text(g.name)
                        );
                        self.report(slot, Code::PartialGenericArguments, message);
                    }
                    for (i, g) in target.generics.iter().enumerate() {
                        let Some(&a) = args_v.get(i) else { continue };
                        // `types.map-key.no-mut`: a `mut` key is its own error.
                        if def == self.cx.names.known.map
                            && i == 0
                            && matches!(pool.get(a), TyData::Mut(_))
                        {
                            let message = format!(
                                "{} is a `mut` map key type; a map key type is not `mut`",
                                self.show(a)
                            );
                            self.report(slot, Code::InvalidMapKey, message);
                            continue;
                        }
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
                                // One error per argument that misses a bound.
                                break;
                            }
                        }
                    }
                }
                for &a in args_v {
                    self.check_ty(a, slot)?;
                }
            }
            TyData::TraitValue { def, args, .. } => {
                // A trait value type or requirement key keeps only the
                // arguments written or filled from defaults; a default that
                // names `Self` cannot fill (`trait.dyn.generic-trait`).
                let have = pool.list_items(args).len();
                let want = self
                    .cx
                    .lookup
                    .item(def)
                    .map_or(0, |t| t.generics.iter().filter(|g| !g.row).count());
                if have < want {
                    let message = format!(
                        "`{}` is written without all of its generic arguments",
                        self.cx.names.display_name(def)
                    );
                    self.report(slot, Code::PartialGenericArguments, message);
                }
                self.check_bound(t)?;
            }
            TyData::Option(i) | TyData::Mut(i) => self.check_ty(i, slot)?,
            TyData::Tuple { elems, rest } => {
                for e in pool.list_items(elems).iter().copied().chain(rest) {
                    self.check_ty(e, slot)?;
                }
            }
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                for e in pool.list_items(params).iter().copied() {
                    self.check_ty(e, slot)?;
                }
                self.check_ty(result, slot)?;
                self.check_row(row, slot)?;
            }
            TyData::Row(row) | TyData::Context(row) => self.check_row(row, slot)?,
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

/// A host capability trait of the run's runtime profile
/// (`module.profile.definition`): each method's arguments cross out of hd
/// and its result crosses in (`module.boundary.direction`). A value of a
/// data type with a field that is not `pub` crosses out only through
/// `std.serde.Serialize` and in only through `std.serde.Deserialize`
/// (`module.boundary.out`, `module.boundary.in`). A signature with a type
/// that would cross without it is an error there, once per signature
/// (`module.boundary.error`).
pub fn boundary(cx: &HeaderCx<'_>, trait_: &Item) -> StageResult<Vec<Finding>> {
    let mut out = Vec::new();
    let ItemData::Trait(t) = &trait_.data else {
        return Ok(out);
    };
    let serialize = cx.names.item("std.serde", "Serialize");
    let deserialize = cx.names.item("std.serde", "Deserialize");
    for &(_, m) in &t.methods {
        let Some(mi) = cx.lookup.item(m) else {
            continue;
        };
        let Some(sig) = mi.sig() else {
            continue;
        };
        let mut env = environment(cx, mi);
        env.key = Some(cx.global.env_key(&env));
        let mut hc = ItemCheck {
            cx,
            env,
            memo: BodyMemo::default(),
            fuel: Fuel::new(Fuel::BODY_DEFAULT),
            item: m,
            out: &mut out,
        };
        let crossing: Vec<(Ty, DefId)> = sig
            .params
            .iter()
            .filter(|p| cx.names.text(p.0) != "self")
            .map(|p| (p.1, serialize))
            .chain([(sig.ret, deserialize)])
            .collect();
        for (t, tr) in crossing {
            if let Some(bad) = hc.crosses_without(t, tr, &mut Vec::new())? {
                let way = if tr == serialize { "out of" } else { "into" };
                let message = format!(
                    "{} has a private field and does not implement {}, so it cannot cross {way} hd here",
                    hc.show(bad),
                    cx.names.display_name(tr)
                );
                hc.report(0, Code::BoundaryPrivateField, message);
                break;
            }
        }
    }
    Ok(out)
}

impl ItemCheck<'_, '_> {
    /// The first type in `t` that has a field that is not `pub` and does
    /// not implement `tr`. Such a type that implements `tr` crosses as its
    /// implementation writes or builds it (`module.boundary.serialize.exact`,
    /// `module.boundary.deserialize.exact`). A data type whose fields are
    /// all `pub` crosses through its fields, an enum through its payloads
    /// (`module.boundary.public`), and the other types that may cross
    /// through their contents (`module.boundary.allowed`).
    fn crosses_without(&mut self, t: Ty, tr: DefId, seen: &mut Vec<Ty>) -> StageResult<Option<Ty>> {
        let pool = self.pool();
        if seen.contains(&t) {
            return Ok(None);
        }
        seen.push(t);
        let parts: Vec<Ty> = match pool.get(t) {
            TyData::Option(i) | TyData::Mut(i) => vec![i],
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().copied().chain(rest).collect()
            }
            TyData::Adt { def, args } => {
                let argv = pool.list_items(args);
                let sub = |x: Ty| {
                    pool.subst(x, &|p: ParamRef| {
                        (p.owner == def).then(|| argv.get(usize::from(p.index)).copied())?
                    })
                };
                let known = self.cx.names.known;
                match self.cx.lookup.item(def).map(|i| &i.data) {
                    _ if def == known.list || def == known.map => argv.to_vec(),
                    Some(ItemData::Data(fields)) => {
                        if fields.iter().any(|f| !f.public && !f.embedded) {
                            let bound = pool.intern_ty(&TyData::TraitValue {
                                def: tr,
                                args: TyList::EMPTY,
                                bindings: vec![],
                            });
                            return Ok(self.fails(t, bound)?.then_some(t));
                        }
                        fields.iter().map(|f| sub(f.ty)).collect()
                    }
                    Some(ItemData::Enum { shared, variants }) => shared
                        .iter()
                        .chain(variants.iter().flat_map(|v| &v.fields))
                        .map(|f| sub(f.ty))
                        .collect(),
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        for p in parts {
            if let Some(bad) = self.crosses_without(p, tr, seen)? {
                return Ok(Some(bad));
            }
        }
        Ok(None)
    }
}
