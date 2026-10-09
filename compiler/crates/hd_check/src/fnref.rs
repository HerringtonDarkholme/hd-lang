//! Function references (spec/lang/07-functions.md, Generic Function
//! Values and Method References). A path used as a value is an `ItemRef`
//! of the item and its type arguments, instantiated from the expected
//! function type, an explicit list, or the call it is an argument of,
//! then from defaults (`fn.type.generic.*`). A bound reference
//! `value::name` is a closure whose body calls the method on the
//! receiver it captured once (codegen.md §13.11; `audit/muse_task.md`
//! Q-R3: `ItemRef` keeps its schema, a bound reference wraps it).
//!
//! The type arguments of an `ItemRef` are the owner's, then the
//! member's own: a function's own; an inherent member's impl arguments,
//! then its own; a trait member's `Self`, the trait's arguments, then its
//! own.

use hd_base::{DefId, StageResult};
use hd_diag::Code;
use hd_resolve::{FnSig, HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{Callee, ChoiceKind, NONE, Providers, Ref, Tag, TirSink, local_flags};
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, Prim, RowId, Ty, TyData, VarKind, with_assoc_args};

use crate::body::{Ck, unsupported};
use crate::call::{Hit, subst_owner};
use crate::ty::Named;

/// Whom a referenced function's other type parameters belong to.
enum Owner {
    /// A module-level function.
    Item,
    /// An inherent method or associated function: its impl's arguments.
    Impl { def: DefId, args: Vec<Ty> },
    /// A trait member: `Self`, the trait's arguments, and the evidence
    /// choice once it is known.
    Trait {
        def: DefId,
        self_ty: Ty,
        args: Vec<Ty>,
        choice: Option<(ChoiceKind, u32)>,
    },
}

/// A referenced function or member with fresh variables for its own
/// type parameters.
struct Member {
    def: DefId,
    sig: FnSig,
    vars: Vec<Ty>,
    owner: Owner,
}

/// `t` under the member's instantiation.
fn inst(pool: hd_types::Types<'_>, m: &Member, t: Ty) -> Ty {
    let t = subst_owner(pool, m.def, &m.vars, t);
    match &m.owner {
        Owner::Item => t,
        Owner::Impl { def, args } => subst_owner(pool, *def, args, t),
        Owner::Trait {
            def, self_ty, args, ..
        } => {
            let t = pool.subst(t, &|p: ParamRef| {
                if p.owner != *def {
                    None
                } else if p.index == 0 {
                    Some(*self_ty)
                } else {
                    args.get(p.index as usize - 1).copied()
                }
            });
            with_assoc_args(pool, t, *def, pool.list(args))
        }
    }
}

/// The member's row under its instantiation.
fn inst_row(pool: hd_types::Types<'_>, m: &Member, r: RowId) -> RowId {
    pool.subst_row(r, &|p: ParamRef| {
        if p.owner == m.def {
            return m.vars.get(p.index as usize).copied();
        }
        match &m.owner {
            Owner::Impl { def, args } if p.owner == *def => args.get(p.index as usize).copied(),
            _ => None,
        }
    })
}

impl Ck<'_, '_> {
    /// `Owner::name` or `value::name` as a value; `method_targs` are the
    /// member's written type arguments (`Counter::pick::[i32]`).
    pub(crate) fn path_value(
        &mut self,
        n: NodeRef<'_>,
        want: Option<Ty>,
        method_targs: &[Ty],
    ) -> StageResult<(Ref, Ty)> {
        let Some(base) = n.children().next() else {
            return unsupported("a path without a base");
        };
        let name = self.cx.src.text(self.cx.src.last(n)).to_owned();
        // `fn.ref.unbound`: the owner names a type, a trait or a type
        // parameter (the forms of a qualified call).
        if matches!(base.kind(), SyntaxKind::NameExpr | SyntaxKind::TypeArgsExpr)
            && (base.kind() == SyntaxKind::TypeArgsExpr
                || self.find_local(self.sym_of(base)).is_none())
        {
            let (bnode, explicit) = if base.kind() == SyntaxKind::TypeArgsExpr {
                let Some(inner) = base.children().next() else {
                    return unsupported("a type-argument base");
                };
                let mut ex = Vec::new();
                if let Some(tl) = Src::child(base, SyntaxKind::TypeArgumentList) {
                    for t in tl.children().filter(|c| c.kind().is_type()) {
                        ex.push(self.ty_node(t)?);
                    }
                }
                (inner, ex)
            } else {
                (base, vec![])
            };
            let text = self.cx.src.text(self.cx.src.first(bnode)).to_owned();
            if let Some(p) = self.type_param(&text) {
                return self.type_ref(p, &name, method_targs, want, n);
            }
            if let Some(p) = Prim::ALL.iter().find(|p| p.name() == text) {
                return self.type_ref(Ty::prim(*p), &name, method_targs, want, n);
            }
            match self.scope_name(&text) {
                Some(Named::Poison) => return Ok(self.poison_value(n)),
                Some(Named::Module(m)) => {
                    let Some(def) = self.export(m, &name) else {
                        self.missing_export(m, &name, n);
                        return Ok((Ref(NONE), Ty::NEVER));
                    };
                    if let Some(ItemData::Fn(sig)) = self.cx.lookup.item(def).map(|i| &i.data) {
                        let sig = sig.clone();
                        return self.item_value(def, &sig, method_targs, want, n);
                    }
                    return unsupported("a module member value that is not a function");
                }
                Some(Named::Item(def)) => match self.kind_of_item(def) {
                    Some(HeadKind::Trait) => {
                        return self.trait_ref(def, explicit, &name, method_targs, want, n);
                    }
                    Some(HeadKind::Enum | HeadKind::Alias)
                        if {
                            let et = self.ctor(def, &explicit)?;
                            self.variant_fields(et, &name).is_some()
                        } =>
                    {
                        return self.gap(n, "a variant constructor used as a value");
                    }
                    Some(k) if k.is_type() => {
                        let t = self.ctor(def, &explicit)?;
                        return self.type_ref(t, &name, method_targs, want, n);
                    }
                    _ => {}
                },
                None => {}
            }
        }
        // `fn.ref.bound`: the base names a value.
        let (recv, rt) = self.expr(base, None)?;
        self.bound_ref(recv, rt, &name, method_targs, want, n)
    }

    /// A function item as a value (`fn.type.generic.*`).
    pub(crate) fn item_value(
        &mut self,
        def: DefId,
        sig: &FnSig,
        explicit: &[Ty],
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        // A registration function is never a value (`module.testing.direct-call`).
        if self.is_registration_fn(def) {
            let name = self
                .cx
                .lookup
                .item(def)
                .map(|i| self.cx.names.text(i.name).to_owned())
                .unwrap_or_default();
            let msg = format!("`{name}` is called only as a test registration in test position");
            self.err(Code::MisplacedTestCase, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        }
        let sig = self.with_result(def, sig.clone());
        let vars = self.fresh_generics(&sig, explicit, n);
        let m = Member {
            def,
            sig,
            vars,
            owner: Owner::Item,
        };
        self.unbound(m, want, n)
    }

    /// `Type::name`, `T::name`: an unbound reference through the lookup
    /// of a qualified call (`fn.ref.lookup`).
    fn type_ref(
        &mut self,
        t: Ty,
        name: &str,
        explicit: &[Ty],
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        match self.member_of(t, name, explicit, n)? {
            Some(m) => self.unbound(m, want, n),
            None => Ok((Ref(NONE), Ty::NEVER)),
        }
    }

    /// `Trait::name`: `Self` comes from the expected type or the call
    /// (`fn.ref.trait-self`); only the trait's own members
    /// (`trait.qualified.declaring`).
    fn trait_ref(
        &mut self,
        tr: DefId,
        mut trait_args: Vec<Ty>,
        name: &str,
        explicit: &[Ty],
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let Some(item) = self.cx.lookup.item(tr) else {
            return unsupported("a trait outside the closure");
        };
        let ItemData::Trait(td) = &item.data else {
            return unsupported("a trait path");
        };
        let n_trait = item.generics.len();
        let sym = self.cx.names.syms.intern(name);
        let Some(&(_, method)) = td.methods.iter().find(|(s, _)| *s == sym) else {
            let msg = format!(
                "no method `{name}` on trait {}",
                self.cx.names.display_name(tr)
            );
            self.err(Code::UnknownMethod, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        };
        while trait_args.len() < n_trait {
            trait_args.push(self.infer.fresh(pool, VarKind::General));
        }
        let self_ty = match self.template_self(tr) {
            Some(t) => t,
            None => self.infer.fresh(pool, VarKind::General),
        };
        let sig = self.sig_of(method)?;
        let vars = self.fresh_generics(&sig, explicit, n);
        let m = Member {
            def: method,
            sig,
            vars,
            owner: Owner::Trait {
                def: tr,
                self_ty,
                args: trait_args,
                choice: None,
            },
        };
        self.unbound(m, want, n)
    }

    /// The member `name` of `t` as a method call would find it, or
    /// `None` after reporting why there is none.
    fn member_of(
        &mut self,
        t: Ty,
        name: &str,
        explicit: &[Ty],
        n: NodeRef<'_>,
    ) -> StageResult<Option<Member>> {
        let pool = self.pool();
        let t = self.norm_ty(t);
        if matches!(self.strip_mut(t), Ty::NEVER | Ty::POISON) {
            return Ok(None);
        }
        let trait_member = |ck: &mut Self, trait_: DefId, method: DefId, mut args: Vec<Ty>| {
            let n_trait = ck.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
            while args.len() < n_trait {
                args.push(ck.infer.fresh(pool, VarKind::General));
            }
            (ck.strip_mut(t), args, method)
        };
        let m = match self.resolve_method(t, name)? {
            Some(Hit::Inherent {
                method,
                impl_def,
                impl_args,
            }) => {
                let sig = self.sig_of(method)?;
                let vars = self.fresh_generics(&sig, explicit, n);
                Member {
                    def: method,
                    sig,
                    vars,
                    owner: Owner::Impl {
                        def: impl_def,
                        args: impl_args,
                    },
                }
            }
            Some(Hit::Trait {
                trait_,
                method,
                args,
                choice,
            }) => {
                let (self_ty, args, method) = trait_member(self, trait_, method, args);
                let sig = self.sig_of(method)?;
                let vars = self.fresh_generics(&sig, explicit, n);
                Member {
                    def: method,
                    sig,
                    vars,
                    owner: Owner::Trait {
                        def: trait_,
                        self_ty,
                        args,
                        choice: Some(choice),
                    },
                }
            }
            // One trait with several instantiations: its arguments come
            // from the expected type, and the bound check picks the impl.
            Some(Hit::Choice { trait_, method, .. }) => {
                let (self_ty, args, method) = trait_member(self, trait_, method, vec![]);
                let sig = self.sig_of(method)?;
                let vars = self.fresh_generics(&sig, explicit, n);
                Member {
                    def: method,
                    sig,
                    vars,
                    owner: Owner::Trait {
                        def: trait_,
                        self_ty,
                        args,
                        choice: None,
                    },
                }
            }
            Some(Hit::Builtin { .. }) => return self.gap(n, "a reference to a built-in method"),
            Some(Hit::Ambiguous(traits)) => {
                let names: Vec<String> = self
                    .cx
                    .names
                    .display_names(&traits)
                    .iter()
                    .map(|d| format!("`{d}`"))
                    .collect();
                let msg = format!(
                    "`{name}` on {} is ambiguous: the traits {} each supply it; write `Trait::{name}`",
                    self.show(t),
                    names.join(" and ")
                );
                self.err(Code::AmbiguousMethod, n, &msg);
                return Ok(None);
            }
            None => {
                // `fn.ref.no-fields`: `::` never names a field.
                let msg = if self.field_of(t, name).is_some() {
                    format!(
                        "`{name}` is a field of {}; `::` names only methods and associated functions, so write a closure",
                        self.show(t)
                    )
                } else {
                    format!("no method `{name}` on {}", self.show(t))
                };
                self.no_method(t, name, n, &msg);
                return Ok(None);
            }
        };
        Ok(Some(m))
    }

    /// Whether the member takes `self`.
    fn has_self(&self, m: &Member) -> bool {
        m.sig
            .params
            .first()
            .is_some_and(|p| self.cx.names.text(p.0) == "self")
    }

    /// The member's function type without its first `skip` parameters.
    fn member_fn_ty(&mut self, m: &Member, skip: usize) -> StageResult<Ty> {
        let pool = self.pool();
        let mut params = Vec::with_capacity(m.sig.params.len());
        for p in m.sig.params.iter().skip(skip) {
            params.push(self.normalize_deep(inst(pool, m, p.1))?);
        }
        let result = self.normalize_deep(inst(pool, m, m.sig.ret))?;
        Ok(pool.intern_ty(&TyData::Fn {
            params: pool.list(&params),
            result,
            row: inst_row(pool, m, m.sig.row),
            suspends: m.sig.suspends,
        }))
    }

    /// Solves what the expected function type determines
    /// (`fn.type.generic.instantiate-from`): each parameter and the
    /// result, one at a time. A part that does not unify is left to the
    /// coercion at the use, which reports it or adapts the value.
    pub(crate) fn fit_expected(&mut self, ft: Ty, want: Option<Ty>) {
        let Some(w) = want else {
            return;
        };
        let pool = self.pool();
        let w = self.strip_mut(w);
        let (
            TyData::Fn {
                params: wp,
                result: wr,
                ..
            },
            TyData::Fn {
                params: fp,
                result: fr,
                ..
            },
        ) = (pool.get(w), pool.get(ft))
        else {
            return;
        };
        let wp = pool.list_items(wp).to_vec();
        let fp = pool.list_items(fp).to_vec();
        if wp.len() != fp.len() {
            return;
        }
        for (a, b) in fp.into_iter().zip(wp).chain([(fr, wr)]) {
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, a, b).is_err() {
                self.infer.rollback(snap);
            }
        }
    }

    /// The member's bounds, and for a trait member whose impl is not
    /// chosen yet, the trait itself; a bound on a type not known yet
    /// waits for the end of the body.
    fn member_bounds(&mut self, m: &mut Member, n: NodeRef<'_>) -> StageResult<()> {
        let pool = self.pool();
        if let Owner::Trait {
            def,
            self_ty,
            args,
            choice: choice @ None,
        } = &mut m.owner
        {
            let tref = TraitRef {
                trait_: *def,
                self_ty: self.infer.resolve(pool, *self_ty),
                args: pool.list(args),
            };
            let ev = self.require_ref(tref, n)?;
            *choice = Some(self.choice_of(ev.as_ref()));
        }
        let m = &*m;
        let inst = |x: Ty| inst(pool, m, x);
        self.bounds_of(&m.sig, &m.vars, &inst, n)
    }

    /// Records the reference's type variables, decided at the end of the
    /// statement (`fn.type.generic.default`, `fn.type.generic.unsolved`).
    fn open_ref_params(&mut self, m: &Member, n: NodeRef<'_>) {
        let pool = self.pool();
        let empty_row = pool.intern_ty(&TyData::Row(RowId::EMPTY));
        match &m.owner {
            Owner::Item => {}
            Owner::Impl { args, .. } => {
                for a in args {
                    self.ref_params.push((*a, None, n.index()));
                }
            }
            Owner::Trait { self_ty, args, .. } => {
                self.ref_params.push((*self_ty, None, n.index()));
                for a in args {
                    self.ref_params.push((*a, None, n.index()));
                }
            }
        }
        for (i, g) in m.sig.generics.iter().enumerate() {
            let default = if g.row {
                Some(empty_row)
            } else {
                g.default.map(|d| inst(pool, m, d))
            };
            self.ref_params.push((m.vars[i], default, n.index()));
        }
    }

    /// Steps 7 to 9 of a reference's instantiation for the variables
    /// recorded since `start`: an unsolved one takes its default, else it
    /// is `cannot-infer-type` (once per reference) and stands for poison.
    pub(crate) fn close_ref_params(&mut self, start: usize) {
        if self.ref_params.len() <= start {
            return;
        }
        let pool = self.pool();
        let open = self.ref_params.split_off(start);
        for (v, d, _) in &open {
            if let Some(d) = d
                && self.unsolved(*v)
            {
                let _ = self.infer.unify(pool, *v, *d);
            }
        }
        let mut reported = None;
        for (v, _, at) in open {
            if !self.unsolved(v) {
                continue;
            }
            if reported != Some(at) {
                reported = Some(at);
                let node = self.cx.src.parse.tree.node(at);
                self.err(
                    Code::CannotInferType,
                    node,
                    "this function reference's type arguments have no solution; give it an expected function type or write them explicitly",
                );
            }
            self.infer
                .rebind(pool, self.infer.shallow(pool, v), Ty::POISON);
        }
    }

    /// The rest of an unbound reference: fit, bounds, `ItemRef`.
    fn unbound(
        &mut self,
        mut m: Member,
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        self.note_call(m.def);
        let ft = self.member_fn_ty(&m, 0)?;
        self.fit_expected(ft, want);
        self.member_bounds(&mut m, n)?;
        self.open_ref_params(&m, n);
        let mut targs = match &m.owner {
            Owner::Item => vec![],
            Owner::Impl { args, .. } => args.clone(),
            Owner::Trait { self_ty, args, .. } => {
                let mut v = vec![*self_ty];
                v.extend(args);
                v
            }
        };
        targs.extend(&m.vars);
        let a = self.b.refs_record(&[Ref(m.def.raw())]);
        let l = pool.list(&targs);
        let bw = self.b.refs_record(&[Ref(l.0)]);
        Ok((self.b.emit(Tag::ItemRef, a, bw, ft, n.index()), ft))
    }

    /// `value::name`: the receiver is evaluated once, into a local the
    /// closure captures (`fn.ref.bound.capture`); calling the closure
    /// calls the method on it (`fn.ref.bound.type`).
    fn bound_ref(
        &mut self,
        recv: Ref,
        rt: Ty,
        name: &str,
        explicit: &[Ty],
        want: Option<Ty>,
        n: NodeRef<'_>,
    ) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let rt = self.norm_ty(rt);
        let Some(mut m) = self.member_of(rt, name, explicit, n)? else {
            return Ok((Ref(NONE), Ty::NEVER));
        };
        // `fn.ref.bound.associated`.
        if !self.has_self(&m) {
            let msg = format!(
                "`{name}` is an associated function; a bound reference names a method, so write `{}::{name}`",
                self.show(self.strip_mut(rt))
            );
            self.err(Code::UnknownMethod, n, &msg);
            return Ok((Ref(NONE), Ty::NEVER));
        }
        self.note_call(m.def);
        // `fn.ref.bound.mut`.
        let self_param = inst(pool, &m, m.sig.params[0].1);
        self.check_receiver(self_param, rt, n);
        let ft = self.member_fn_ty(&m, 1)?;
        self.fit_expected(ft, want);
        self.member_bounds(&mut m, n)?;
        self.open_ref_params(&m, n);
        let TyData::Fn {
            params,
            result,
            suspends,
            ..
        } = pool.get(ft)
        else {
            return unsupported("a bound reference's type");
        };
        let callee = match &m.owner {
            Owner::Item => Callee::Item {
                def: m.def,
                targs: pool.list(&m.vars),
            },
            Owner::Impl { args, .. } => {
                let mut all = args.clone();
                all.extend(&m.vars);
                Callee::Item {
                    def: m.def,
                    targs: pool.list(&all),
                }
            }
            Owner::Trait {
                def,
                self_ty,
                args,
                choice,
            } => {
                let mut all = args.clone();
                all.extend(&m.vars);
                Callee::TraitMethod {
                    trait_: *def,
                    method: m.def,
                    self_ty: self.infer.resolve(pool, *self_ty),
                    targs: pool.list(&all),
                    choice: choice.unwrap_or((ChoiceKind::Builtin, 0)),
                }
            }
        };
        let syn = n.index();
        let receiver = self.cx.names.syms.intern("$receiver");
        let rl = self.b.local(rt, receiver, local_flags::ASSIGNED, syn);
        self.b.set(rl, recv, syn);
        let ptys = pool.list_items(params).to_vec();
        let locals: Vec<_> = ptys
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let s = self.cx.names.syms.intern(&format!("${i}"));
                self.b.local(*t, s, local_flags::PARAM, syn)
            })
            .collect();
        let mark = self.b.open_sub(&locals);
        self.b.capture(mark, rl);
        self.b.body_mut().local_flags[rl.idx()] |= local_flags::READ | local_flags::CAPTURED;
        let blk = self.b.open_block();
        let mut refs = vec![self.b.get(rl, rt, syn)];
        for (l, t) in locals.iter().zip(&ptys) {
            refs.push(self.b.get(*l, *t, syn));
        }
        let r = self.b.call(&callee, &refs, Providers::None, result, syn);
        if suspends {
            self.b.body_mut().tags[r.0 as usize] = Tag::Await;
        }
        let root = self.b.close_block(blk, Some(r), result, syn);
        Ok((self.b.close_sub(mark, root, ft, syn), ft))
    }
}
