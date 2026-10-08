//! Header validation stage B (§4.10.1) and the overlap check of coherence
//! (§4.12.3, trait-solver.md §5.2), over frozen interfaces.
//!
//! Stage B asks one question many times: does a type implement a trait,
//! under the declared bounds of the item being checked? `Universe::holds`
//! answers it from impl heads: an impl applies when its head matches the
//! type, and then its own bounds must hold. An answer the heads alone
//! cannot give (projections, compiler-supplied traits, `dyn` values) is
//! "unknown" and reports nothing; stage C settles those.

use std::collections::HashMap;

use hd_base::{DefId, Symbol};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

use crate::iface::{FnSig, Generic, ImplKind, Item, ItemData, Names};

/// Every item one check may read, and the trait impls by trait.
pub struct Universe<'a> {
    pub names: Names<'a>,
    pub items: HashMap<DefId, &'a Item>,
    pub impls: HashMap<DefId, Vec<&'a Item>>,
    /// Traits the compiler supplies or seals: answers about them are
    /// "unknown" here.
    pub opaque: Vec<DefId>,
}

/// One problem stage B found: the item, a code and a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub item: DefId,
    /// Which written position of the item (`anchor` slots): 0 is its header.
    pub slot: u32,
    pub code: hd_diag::Code,
    pub message: String,
}

type Env = Vec<(Ty, Vec<Ty>)>;

const DEPTH: u32 = 16;

impl<'a> Universe<'a> {
    #[must_use]
    pub fn new(names: Names<'a>, all: impl Iterator<Item = &'a Item>) -> Self {
        let mut items = HashMap::new();
        let mut impls: HashMap<DefId, Vec<&Item>> = HashMap::new();
        for it in all {
            items.insert(it.def, it);
            if let ItemData::Impl { trait_, kind, .. } = it.data
                && trait_ != DefId::NONE
                && kind.is_impl()
            {
                impls.entry(trait_).or_default().push(it);
            }
        }
        let k = names.known;
        let opaque = [
            k.any,
            k.any_val,
            k.any_ref,
            k.tuple,
            k.structure,
            k.inspectable,
        ]
        .into_iter()
        .collect();
        Self {
            names,
            items,
            impls,
            opaque,
        }
    }

    fn pool(&self) -> &InternPool {
        self.names.pool
    }

    /// The supertraits of a trait value, `Self` replaced by `self_ty`.
    fn supers(&self, tv: Ty, self_ty: Ty) -> Vec<Ty> {
        let TyData::TraitValue { def, args, .. } = self.pool().get(tv) else {
            return Vec::new();
        };
        let Some(Item {
            data: ItemData::Trait(t),
            ..
        }) = self.items.get(&def).copied()
        else {
            return Vec::new();
        };
        let args = self.pool().list_items(args);
        t.supers
            .iter()
            .map(|s| {
                self.pool().subst(*s, &|p: ParamRef| {
                    (p.owner == def).then(|| {
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

    fn trait_def(&self, tv: Ty) -> Option<DefId> {
        match self.pool().get(tv) {
            TyData::TraitValue { def, .. } => Some(def),
            _ => None,
        }
    }

    /// Does `bound` (a trait value) follow from `have`, through supertraits?
    fn entails(&self, have: Ty, want: DefId, self_ty: Ty, depth: u32) -> bool {
        if self.trait_def(have) == Some(want) {
            return true;
        }
        depth > 0
            && self
                .supers(have, self_ty)
                .into_iter()
                .any(|s| self.entails(s, want, self_ty, depth - 1))
    }

    /// One-way match of an impl head against a type; the impl's own
    /// parameters are the pattern's variables.
    fn matches(&self, pat: Ty, t: Ty, owner: DefId, sub: &mut HashMap<u16, Ty>) -> bool {
        let pool = self.pool();
        if pat == t {
            return true;
        }
        match (pool.get(pat), pool.get(t)) {
            (TyData::Param(p), _) if p.owner == owner => {
                if let Some(&b) = sub.get(&p.index) {
                    b == t
                } else {
                    sub.insert(p.index, t);
                    true
                }
            }
            (_, TyData::Poison) => true,
            (TyData::Adt { def: a, args: x }, TyData::Adt { def: b, args: y }) => {
                a == b && self.matches_list(x, y, owner, sub)
            }
            (TyData::Option(a), TyData::Option(b)) | (TyData::Mut(a), TyData::Mut(b)) => {
                self.matches(a, b, owner, sub)
            }
            (
                TyData::Tuple {
                    elems: x,
                    rest: None,
                },
                TyData::Tuple {
                    elems: y,
                    rest: None,
                },
            ) => self.matches_list(x, y, owner, sub),
            (
                TyData::Fn {
                    params: x,
                    result: r1,
                    suspends: s1,
                    ..
                },
                TyData::Fn {
                    params: y,
                    result: r2,
                    suspends: s2,
                    ..
                },
            ) => {
                s1 == s2 && self.matches_list(x, y, owner, sub) && self.matches(r1, r2, owner, sub)
            }
            _ => false,
        }
    }

    fn matches_list(&self, x: TyList, y: TyList, owner: DefId, sub: &mut HashMap<u16, Ty>) -> bool {
        let (x, y) = (self.pool().list_items(x), self.pool().list_items(y));
        x.len() == y.len()
            && x.iter()
                .zip(y)
                .all(|(a, b)| self.matches(*a, *b, owner, sub))
    }

    /// Whether `t` implements the trait value `tv` under `env`: `Some(true)`,
    /// `Some(false)`, or `None` when heads alone cannot tell.
    fn holds(&self, t: Ty, tv: Ty, env: &Env, depth: u32) -> Option<bool> {
        let pool = self.pool();
        let want = self.trait_def(tv)?;
        if depth == 0 || self.opaque.contains(&want) {
            return None;
        }
        match pool.get(t) {
            TyData::Poison | TyData::Never => return Some(true),
            TyData::Mut(i) => return self.holds(i, tv, env, depth),
            TyData::Param(_) => {
                let (_, bounds) = env.iter().find(|(p, _)| *p == t)?;
                if bounds.iter().any(|b| self.entails(*b, want, t, 8)) {
                    return Some(true);
                }
                // A bound that the impls of a numeric family or a blanket
                // impl could still satisfy is left to stage C.
                return if bounds.is_empty() { Some(false) } else { None };
            }
            TyData::TraitValue { .. } => {
                return self.entails(t, want, t, 8).then_some(true);
            }
            TyData::Assoc { .. } | TyData::Infer(_) | TyData::Canon(_) => return None,
            _ => {}
        }
        let mut unknown = false;
        for imp in self.impls.get(&want).into_iter().flatten() {
            let ItemData::Impl {
                self_ty: head,
                kind,
                ..
            } = &imp.data
            else {
                continue;
            };
            let mut sub = HashMap::new();
            let hit = if *kind == ImplKind::TupleTemplate {
                matches!(pool.get(t), TyData::Tuple { .. })
            } else {
                self.matches(*head, t, imp.def, &mut sub)
            };
            if !hit {
                continue;
            }
            if *kind == ImplKind::TupleTemplate {
                unknown = true;
                continue;
            }
            let mut all = Some(true);
            for (i, g) in imp.generics.iter().enumerate() {
                let Some(&arg) = sub.get(&u16::try_from(i).unwrap_or(u16::MAX)) else {
                    if !g.bounds.is_empty() {
                        all = None;
                    }
                    continue;
                };
                for b in &g.bounds {
                    let b = pool.subst(*b, &|p: ParamRef| {
                        (p.owner == imp.def).then(|| sub.get(&p.index).copied())?
                    });
                    match self.holds(arg, b, env, depth - 1) {
                        Some(true) => {}
                        Some(false) => all = all.and(Some(false)),
                        None => all = None,
                    }
                }
            }
            match all {
                Some(true) => return Some(true),
                None => unknown = true,
                Some(false) => {}
            }
        }
        if unknown { None } else { Some(false) }
    }

    fn env_of(&self, owner: DefId, generics: &[Generic], offset: u16, env: &mut Env) {
        for (i, g) in generics.iter().enumerate() {
            if g.row {
                continue;
            }
            let index = offset + u16::try_from(i).unwrap_or(u16::MAX);
            let p = self
                .pool()
                .intern_ty(&TyData::Param(ParamRef { owner, index }));
            env.push((p, g.bounds.clone()));
        }
    }

    /// The parameter environment of an item: its own parameters, plus its
    /// owner's for a member (a trait's `Self` bounded by the trait).
    fn env(&self, it: &Item) -> Env {
        let mut env = Env::new();
        let pool = self.pool();
        let own = |owner: DefId, env: &mut Env| {
            let Some(o) = self.items.get(&owner).copied() else {
                return;
            };
            match &o.data {
                ItemData::Trait(_) => {
                    let s = pool.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
                    let args: Vec<Ty> = (0..o.generics.len())
                        .map(|i| {
                            pool.intern_ty(&TyData::Param(ParamRef {
                                owner,
                                index: u16::try_from(i + 1).unwrap_or(u16::MAX),
                            }))
                        })
                        .collect();
                    let tv = pool.intern_ty(&TyData::TraitValue {
                        def: owner,
                        args: pool.list(&args),
                        bindings: vec![],
                    });
                    env.push((s, vec![tv]));
                    self.env_of(owner, &o.generics, 1, env);
                }
                _ => self.env_of(owner, &o.generics, 0, env),
            }
        };
        match &it.data {
            ItemData::Method { owner, sig, .. } => {
                own(*owner, &mut env);
                self.env_of(it.def, &sig.generics, 0, &mut env);
            }
            ItemData::Fn(sig) => self.env_of(it.def, &sig.generics, 0, &mut env),
            ItemData::Trait(_) => own(it.def, &mut env),
            _ => self.env_of(it.def, &it.generics, 0, &mut env),
        }
        env
    }

    /// Written types against their declared bounds (`trait.bound.no-implied`).
    fn check_ty(&self, t: Ty, env: &Env, at: (DefId, u32), out: &mut Vec<Finding>) {
        let item = at.0;
        let pool = self.pool();
        match pool.get(t) {
            TyData::Adt { def, args } => {
                let args_v = pool.list_items(args);
                if let Some(target) = self.items.get(&def).copied() {
                    for (i, g) in target.generics.iter().enumerate() {
                        let Some(&a) = args_v.get(i) else { continue };
                        for b in &g.bounds {
                            let b = pool.subst(*b, &|p: ParamRef| {
                                (p.owner == def)
                                    .then(|| args_v.get(usize::from(p.index)).copied())?
                            });
                            if self.holds(a, b, env, DEPTH) == Some(false) {
                                out.push(Finding {
                                    item,
                                    slot: at.1,
                                    code: hd_diag::Code::UnsatisfiedTraitBound,
                                    message: format!(
                                        "{} does not implement {} in `{}`",
                                        self.show(a),
                                        self.show(b),
                                        self.names.path(item)
                                    ),
                                });
                            }
                        }
                    }
                }
                for &a in args_v {
                    self.check_ty(a, env, at, out);
                }
            }
            TyData::Option(i) | TyData::Mut(i) => self.check_ty(i, env, at, out),
            TyData::Tuple { elems, rest } => {
                for e in pool.list_items(elems).iter().copied().chain(rest) {
                    self.check_ty(e, env, at, out);
                }
            }
            TyData::Fn { params, result, .. } => {
                for e in pool.list_items(params).iter().copied() {
                    self.check_ty(e, env, at, out);
                }
                self.check_ty(result, env, at, out);
            }
            _ => {}
        }
    }

    #[must_use]
    pub fn show(&self, t: Ty) -> String {
        crate::iface::show_ty(&self.names, t)
    }

    fn sig_tys(sig: &FnSig) -> Vec<Ty> {
        sig.params.iter().map(|p| p.1).chain([sig.ret]).collect()
    }

    /// Stage B over a folder's items (§4.10.1): bounds of written header
    /// types, the supertraits of each impl, and delegation targets.
    #[must_use]
    pub fn stage_b(&self, items: &[Item]) -> Vec<Finding> {
        let pool = self.pool();
        let mut out = Vec::new();
        for it in items {
            let env = self.env(it);
            let tys: Vec<Ty> = match &it.data {
                ItemData::Fn(s) | ItemData::Method { sig: s, .. } => Self::sig_tys(s),
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
            );
            for (i, t) in tys.into_iter().enumerate() {
                let slot = if positional {
                    u32::try_from(i + 1).unwrap_or(0)
                } else {
                    0
                };
                self.check_ty(t, &env, (it.def, slot), &mut out);
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
                continue;
            };
            if *trait_ == DefId::NONE || !kind.is_impl() || *kind == ImplKind::TupleTemplate {
                continue;
            }
            let tv = pool.intern_ty(&TyData::TraitValue {
                def: *trait_,
                args: *trait_args,
                bindings: vec![],
            });
            for s in self.supers(tv, *self_ty) {
                if self.holds(*self_ty, s, &env, DEPTH) == Some(false) {
                    out.push(Finding {
                        item: it.def,
                        slot: 0,
                        code: hd_diag::Code::MissingSupertraitImplementation,
                        message: format!(
                            "{} implements {} but not {}",
                            self.show(*self_ty),
                            self.names.path(*trait_),
                            self.show(s)
                        ),
                    });
                }
            }
            if *kind == ImplKind::Delegated
                && let Some(by) = by
            {
                self.delegation(it, *self_ty, tv, *by, &env, &mut out);
            }
        }
        out
    }

    fn delegation(
        &self,
        it: &Item,
        self_ty: Ty,
        tv: Ty,
        by: Symbol,
        env: &Env,
        out: &mut Vec<Finding>,
    ) {
        let pool = self.pool();
        let TyData::Adt { def, args } = pool.get(self_ty) else {
            return;
        };
        let Some(Item {
            data: ItemData::Data(fields),
            ..
        }) = self.items.get(&def).copied()
        else {
            return;
        };
        let args = pool.list_items(args);
        let Some(f) = fields.iter().find(|f| f.embedded && f.name == by) else {
            out.push(Finding {
                item: it.def,
                slot: 0,
                code: hd_diag::Code::InvalidDelegation,
                message: format!(
                    "`{}` has no embedded field `{}`",
                    self.show(self_ty),
                    self.names.text(by)
                ),
            });
            return;
        };
        let ft = pool.subst(f.ty, &|p: ParamRef| {
            (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
        });
        if self.holds(ft, tv, env, DEPTH) == Some(false) {
            out.push(Finding {
                item: it.def,
                slot: 0,
                code: hd_diag::Code::InvalidDelegation,
                message: format!(
                    "the field `{}` does not implement {}",
                    self.names.text(by),
                    self.show(tv)
                ),
            });
        }
    }

    /// The overlap check (§4.12.3): per trait, heads that unify after
    /// renaming apart overlap. A head whose target is a bare parameter
    /// bounded by a trait with only ground impls (a numeric family) is
    /// expanded to those targets first. Returns (earlier, later, witness)
    /// in content order.
    #[must_use]
    pub fn overlaps(&self, order: &dyn Fn(DefId) -> String) -> Vec<(DefId, DefId, String)> {
        let pool = self.pool();
        let mut out = Vec::new();
        let mut traits: Vec<&DefId> = self.impls.keys().collect();
        traits.sort_by_key(|d| self.names.path_hash(**d));
        for tr in traits {
            let mut heads: Vec<(DefId, Ty, TyList)> = Vec::new();
            let mut list: Vec<&&Item> = self.impls[tr].iter().collect();
            list.sort_by_key(|i| order(i.def));
            for imp in list {
                let ItemData::Impl {
                    self_ty,
                    trait_args,
                    kind,
                    ..
                } = &imp.data
                else {
                    continue;
                };
                if *kind == ImplKind::TupleTemplate {
                    continue;
                }
                match self.family(imp, *self_ty) {
                    Some(members) => {
                        for m in members {
                            heads.push((imp.def, m, *trait_args));
                        }
                    }
                    None => heads.push((imp.def, *self_ty, *trait_args)),
                }
            }
            let mut reported = std::collections::HashSet::new();
            for (j, b) in heads.iter().enumerate() {
                for a in &heads[..j] {
                    if a.0 == b.0 || reported.contains(&b.0) {
                        continue;
                    }
                    let mut sub = HashMap::new();
                    if self.unify(a.1, b.1, a.0, b.0, &mut sub)
                        && self.unify_list(a.2, b.2, a.0, b.0, &mut sub)
                    {
                        let w =
                            pool.subst(b.1, &|p: ParamRef| sub.get(&(p.owner, p.index)).copied());
                        out.push((a.0, b.0, self.show(w)));
                        reported.insert(b.0);
                    }
                }
            }
        }
        out
    }

    /// A numeric-family head: `impl[N < B] Tr for N` where every impl of
    /// `B` has a ground target.
    fn family(&self, imp: &Item, self_ty: Ty) -> Option<Vec<Ty>> {
        let pool = self.pool();
        let TyData::Param(p) = pool.get(self_ty) else {
            return None;
        };
        if p.owner != imp.def {
            return None;
        }
        let g = imp.generics.get(usize::from(p.index))?;
        let b = g.bound?;
        let members: Vec<Ty> = self
            .impls
            .get(&b)?
            .iter()
            .filter_map(|i| match &i.data {
                ItemData::Impl { self_ty, .. } if !pool.has_param(*self_ty) => Some(*self_ty),
                _ => None,
            })
            .collect();
        (!members.is_empty()).then_some(members)
    }

    fn unify(
        &self,
        a: Ty,
        b: Ty,
        oa: DefId,
        ob: DefId,
        sub: &mut HashMap<(DefId, u16), Ty>,
    ) -> bool {
        let pool = self.pool();
        let resolve = |t: Ty, sub: &HashMap<(DefId, u16), Ty>| -> Ty {
            let mut t = t;
            for _ in 0..64 {
                match pool.get(t) {
                    TyData::Param(p) if p.owner == oa || p.owner == ob => {
                        match sub.get(&(p.owner, p.index)) {
                            Some(&x) => t = x,
                            None => break,
                        }
                    }
                    _ => break,
                }
            }
            t
        };
        let (a, b) = (resolve(a, sub), resolve(b, sub));
        if a == b {
            return true;
        }
        match (pool.get(a), pool.get(b)) {
            (TyData::Param(p), _) if p.owner == oa || p.owner == ob => {
                sub.insert((p.owner, p.index), b);
                true
            }
            (_, TyData::Param(p)) if p.owner == oa || p.owner == ob => {
                sub.insert((p.owner, p.index), a);
                true
            }
            (TyData::Adt { def: x, args: xa }, TyData::Adt { def: y, args: ya }) => {
                x == y && self.unify_list(xa, ya, oa, ob, sub)
            }
            (TyData::Option(x), TyData::Option(y)) | (TyData::Mut(x), TyData::Mut(y)) => {
                self.unify(x, y, oa, ob, sub)
            }
            (
                TyData::Tuple {
                    elems: x,
                    rest: None,
                },
                TyData::Tuple {
                    elems: y,
                    rest: None,
                },
            ) => self.unify_list(x, y, oa, ob, sub),
            (
                TyData::TraitValue {
                    def: x, args: xa, ..
                },
                TyData::TraitValue {
                    def: y, args: ya, ..
                },
            ) => x == y && self.unify_list(xa, ya, oa, ob, sub),
            (
                TyData::Fn {
                    params: x,
                    result: rx,
                    suspends: sx,
                    ..
                },
                TyData::Fn {
                    params: y,
                    result: ry,
                    suspends: sy,
                    ..
                },
            ) => sx == sy && self.unify_list(x, y, oa, ob, sub) && self.unify(rx, ry, oa, ob, sub),
            _ => false,
        }
    }

    fn unify_list(
        &self,
        x: TyList,
        y: TyList,
        oa: DefId,
        ob: DefId,
        sub: &mut HashMap<(DefId, u16), Ty>,
    ) -> bool {
        let (x, y) = (self.pool().list_items(x), self.pool().list_items(y));
        x.len() == y.len()
            && x.iter()
                .zip(y)
                .all(|(a, b)| self.unify(*a, *b, oa, ob, sub))
    }
}
