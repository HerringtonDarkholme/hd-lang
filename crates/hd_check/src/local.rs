//! Block-local declarations in a body (checking-and-tir.md "Local items
//! lift to hidden module items"). A local `data`, `enum`, `trait`, `type`
//! or `impl` is a hidden module item, lowered and checked as one; its
//! statement here executes nothing (`names.local-type.static`,
//! `names.local-impl.static`) and only checks what depends on where it
//! stands. A local `fn` is a named closure value whose name its own body
//! sees (`fn.local.capture-rules`, `names.local-fn.visible`).
//!
//! A generic local `fn` is checked once over its own type parameters, as a
//! named generic function is. Its closure body is a generic sub-body
//! (`sub_flags::GENERIC`): every use of its name instantiates it with the
//! captures of its declaration, and collection makes one code instance per
//! instantiation (`fn.type.generic.*` for a use as a value).

use hd_base::{DefId, LocalId, StageResult};
use hd_diag::Code;
use hd_resolve::{HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{NONE, Ref, Tag, TirSink, local_flags, sub_flags};
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, Ty, TyData, VarKind};

use crate::body::{Ck, PendingGoal, Waiting, unsupported};
use crate::expr::ClosureParts;
use crate::ty::NameRole;

/// A generic local function: what a use of its name instantiates.
#[derive(Clone, Debug)]
pub(crate) struct LocalGeneric {
    /// Its closure's sub-body.
    pub sub: u32,
    /// Its own type parameters: `Param`s of the body's generics owner
    /// numbered from `first`, after the owner's own and those of earlier
    /// generic local functions.
    pub first: u16,
    pub params: Vec<Ty>,
    pub bounds: Vec<Vec<Ty>>,
    pub defaults: Vec<Option<Ty>>,
    /// Its function type over them; while its body is checked, the type
    /// its written result and row give.
    pub fn_ty: Ty,
    /// Whether its body is being checked.
    pub open: bool,
    /// Whether its body uses its name: a recursive call checks against the
    /// type the header gives.
    pub recursive: bool,
    /// The outer locals it captures, once known: after its body, or on a
    /// second check of a body whose closure uses the function.
    pub caps: Option<Vec<LocalId>>,
}

/// A local function's parameters as its calls see them: the names, for
/// named arguments, and each default's hidden closure.
#[derive(Clone, Debug)]
pub(crate) struct LocalForm {
    pub name: String,
    pub names: Vec<hd_base::Symbol>,
    /// The hidden local holding the closure of each parameter's default,
    /// over the earlier parameters (`fn.default.eval`).
    pub defaults: Vec<Option<hd_base::Symbol>>,
}

/// The defaults of the local function a call is checking arguments for.
#[derive(Clone, Debug)]
pub(crate) struct LocalDefaults {
    pub defaults: Vec<Option<hd_base::Symbol>>,
    /// A generic local function's type arguments at this call, which its
    /// default closures are instantiated at too.
    pub vars: Option<Vec<Ty>>,
}

impl Ck<'_, '_> {
    /// A local type, trait or impl declaration statement.
    pub(crate) fn local_decl(&mut self, s: NodeRef<'_>) -> StageResult<()> {
        let lo = self.cx.src.span(s).lo;
        let Some(l) = self.cx.locals.iter().find(|l| l.lo == lo).copied() else {
            // A local derivation block with no trait is no item; lowering
            // reports it as `misplaced-derivation` (`annot.no-trait.local`).
            if hd_resolve::traitless_block(&self.cx.src, s) {
                return Ok(());
            }
            return unsupported(format!("statement {:?}", s.kind()));
        };
        if l.kind == HeadKind::Impl {
            return self.local_impl(l.def, s);
        }
        let name = self.cx.names.text(l.name).to_owned();
        // `names.type-param.no-redeclare.body`, `names.local-type.duplicate`.
        let earlier = self.cx.locals.iter().any(|o| {
            o.suite == l.suite && o.lo < l.lo && o.kind != HeadKind::Impl && o.name == l.name
        });
        let value = self.scopes.last().is_some_and(|m| m.contains_key(&l.name));
        if self.gens.iter().any(|(g, _)| *g == l.name) {
            let msg = format!("`{name}` is a type parameter in scope");
            self.err(Code::DuplicateBinding, s, &msg);
        } else if earlier || value {
            let msg = format!("`{name}` is already declared in this scope");
            self.err(Code::DuplicateBinding, s, &msg);
        }
        Ok(())
    }

    /// A local impl: it must involve a local type or trait
    /// (`trait.impl.local.inherent`, `trait.impl.local.nonlocal`), and its
    /// target must implement the trait's supertraits through the impls
    /// known here (`names.member.known-impl`).
    fn local_impl(&mut self, def: DefId, s: NodeRef<'_>) -> StageResult<()> {
        let Some(it) = self.cx.lookup.item(def) else {
            return Ok(());
        };
        let ItemData::Impl {
            trait_, self_ty, ..
        } = &it.data
        else {
            return Ok(());
        };
        let pool = self.pool();
        let target = match pool.get(self.strip_mut(*self_ty)) {
            TyData::Adt { def, .. } => self.cx.names.is_local(def),
            _ => false,
        };
        let names = self.cx.names;
        let involved = target || (*trait_ != DefId::NONE && names.is_local(*trait_));
        if !involved {
            let msg = if *trait_ == DefId::NONE {
                "a local inherent implementation must target a local type".to_owned()
            } else {
                format!(
                    "implementing {} for {} belongs at module scope; a local implementation involves a local type or trait",
                    names.display_name(*trait_),
                    self.show(*self_ty)
                )
            };
            self.err(Code::LocalImplNonlocalPair, s, &msg);
            return Ok(());
        }
        let view = self.cx.impls.hiding(&self.hidden);
        let hcx = crate::header::HeaderCx {
            names,
            lookup: self.cx.lookup,
            impls: &view,
            global: self.cx.global,
            solver: self.cx.solver,
        };
        for f in crate::header::local_supertraits(&hcx, it)? {
            self.err(f.code, s, &f.message);
        }
        Ok(())
    }

    /// `fn name(params) -> R: body` in a block: a closure bound to `name`
    /// (`fn.local.declare`, `fn.local.scope`).
    pub(crate) fn local_fn(&mut self, n: NodeRef<'_>) -> StageResult<()> {
        let src = self.cx.src;
        let toks = &src.parse.tokens;
        let Some(t) = n.name(toks).or_else(|| src.name_after(n, TokenKind::KwFn)) else {
            return unsupported("a local function without a name");
        };
        let name = self.cx.names.syms.intern(src.text(t));
        let generics = Src::child(n, SyntaxKind::GenericParameterList);
        if let Some(gl) = generics {
            // `names.type-param.no-redeclare.body`: `fn convert[T]` inside
            // `fn work[T]`.
            let mut reused = false;
            for g in gl.children() {
                if let Some(gt) = g.name(toks) {
                    let gs = self.cx.names.syms.intern(src.text(gt));
                    if self.gens.iter().any(|(x, _)| *x == gs) {
                        let msg = format!("`{}` is a type parameter in scope", src.text(gt));
                        self.err(Code::DuplicateBinding, g, &msg);
                        reused = true;
                    }
                }
            }
            if reused {
                return Ok(());
            }
        }
        let params = Src::child(n, SyntaxKind::ParameterList);
        // `names.local-fn.rules`: a local value's duplicate rule.
        if self.scopes.last().is_some_and(|m| m.contains_key(&name)) {
            let msg = format!("`{}` is already bound in this scope", src.text(t));
            self.err(Code::DuplicateBinding, n, &msg);
        }
        let Some(body) = Src::child(n, SyntaxKind::Block) else {
            return unsupported("a local function without a body");
        };
        let mut parts = ClosureParts {
            params,
            ret: n
                .children()
                .find(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow),
            row: Src::child(n, SyntaxKind::RequirementRow),
            body,
            suspends: n.direct_token(toks, TokenKind::Bang).is_some(),
            name: Some(name),
            reassignable: false,
            generic: None,
            upto: None,
            form: None,
        };
        let param_nodes: Vec<NodeRef<'_>> = params
            .iter()
            .flat_map(|p| p.children())
            .filter(|c| c.kind() == SyntaxKind::Parameter)
            .collect();
        self.default_order(&param_nodes);
        let mut form = LocalForm {
            name: src.text(t).to_owned(),
            names: Vec::new(),
            defaults: vec![None; param_nodes.len()],
        };
        for (i, p) in param_nodes.iter().enumerate() {
            let pname = p
                .name(toks)
                .map_or_else(|| format!("${i}"), |t| src.text(t).to_owned());
            form.names.push(self.cx.names.syms.intern(&pname));
        }
        if let Some(gl) = generics {
            return self.generic_local_fn(n, gl, parts, form, &param_nodes);
        }
        // `fn.default.eval`: each default is a closure of the earlier
        // parameters, declared before the function, so it sees the
        // enclosing locals as the function does.
        for (i, p) in param_nodes.iter().enumerate() {
            let Some(e) = Self::default_expr(*p) else {
                continue;
            };
            let hidden = self.hidden_default(&form.name, form.names[i]);
            self.default_closure(*p, i, hidden, e, params, None)?;
            form.defaults[i] = Some(hidden);
        }
        parts.form = Some(form);
        let (r, ft, own) = self.closure_with(n, &parts, None)?;
        let Some(l) = own else {
            return unsupported("a local function without its name");
        };
        self.b.body_mut().local_ty[l.idx()] = ft;
        self.b.set(l, r, n.index());
        Ok(())
    }

    /// A generic local function: its own type parameters with their bounds
    /// and defaults are in scope for its header and body, which are checked
    /// once, as a named generic function's are (`fn.local.declare`).
    fn generic_local_fn<'t>(
        &mut self,
        n: NodeRef<'t>,
        gl: NodeRef<'t>,
        mut parts: ClosureParts<'t>,
        mut form: LocalForm,
        param_nodes: &[NodeRef<'t>],
    ) -> StageResult<()> {
        let src = self.cx.src;
        let toks = &src.parse.tokens;
        let pool = self.pool();
        let in_scope = self.gens.len();
        let first = self.next_gen;
        let mut params = Vec::new();
        let mut nodes = Vec::new();
        for g in gl
            .children()
            .filter(|c| c.kind() == SyntaxKind::GenericParameter)
        {
            let Some(gt) = g.name(toks) else {
                continue;
            };
            if g.direct_token(toks, TokenKind::Dollar).is_some() {
                return self.gap(g, "a row parameter of a local function");
            }
            let sym = self.cx.names.syms.intern(src.text(gt));
            self.check_prelude_shadow(sym, g);
            // `names.type-param.no-redeclare`: an earlier one of the list.
            if self.gens[in_scope..].iter().any(|(x, _)| *x == sym) {
                let msg = format!("`{}` is already a type parameter in scope", src.text(gt));
                self.err(Code::DuplicateBinding, g, &msg);
            }
            let index = self.next_gen;
            self.next_gen = self.next_gen.saturating_add(1);
            let p = pool.intern_ty(&TyData::Param(ParamRef {
                owner: self.gen_owner,
                index,
            }));
            self.gens.push((sym, p));
            params.push(p);
            nodes.push(g);
        }
        // Bounds and defaults, once every parameter has its name.
        let mut bounds = Vec::new();
        let mut defaults = Vec::new();
        for (&p, &g) in params.iter().zip(&nodes) {
            let mut bs = Vec::new();
            if let Some(bl) = Src::child(g, SyntaxKind::BoundList) {
                if bl.direct_token(toks, TokenKind::KwMut).is_some() {
                    return self.gap(bl, "a `mut` bound on a local function's type parameter");
                }
                for b in bl.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
                    let t = self.ty_node_as(b, NameRole::Trait)?;
                    if matches!(pool.get(t), TyData::TraitValue { .. }) {
                        self.add_bound(p, t, 0);
                        bs.push(t);
                    }
                }
            }
            bounds.push(bs);
            let written = Src::child(g, SyntaxKind::TypeDefault).and_then(|d| {
                d.children()
                    .find(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow)
            });
            defaults.push(match written {
                Some(d) => Some(self.ty_node(d)?),
                None => None,
            });
        }
        self.env.key = Some(self.cx.global.env_key(&self.env));
        parts.generic = Some(LocalGeneric {
            sub: NONE,
            first,
            params,
            bounds,
            defaults,
            fn_ty: Ty::POISON,
            open: true,
            recursive: false,
            caps: None,
        });
        // `fn.default.eval`: each default is a generic closure of the
        // earlier parameters over the function's type parameters.
        let generic = parts.generic.clone();
        for (i, p) in param_nodes.iter().enumerate() {
            let Some(e) = Self::default_expr(*p) else {
                continue;
            };
            let hidden = self.hidden_default(&form.name, form.names[i]);
            self.default_closure(*p, i, hidden, e, parts.params, generic.clone())?;
            form.defaults[i] = Some(hidden);
        }
        parts.form = Some(form);
        self.generic_closure(n, &mut parts)?;
        self.gens.truncate(in_scope);
        Ok(())
    }

    /// The closure of a generic local function, checked once, and its
    /// entry in `local_generics`.
    fn generic_closure(
        &mut self,
        n: NodeRef<'_>,
        parts: &mut ClosureParts<'_>,
    ) -> StageResult<LocalId> {
        let pool = self.pool();
        let cp = self.checkpoint();
        let (mut r, mut ft, mut own) = self.closure_with(n, parts, None)?;
        // A closure in the body used the function before its captures
        // were known: the body is checked again knowing them.
        if let Some(l) = own
            && let Some(at) = self.generic_recheck.iter().position(|x| *x == l)
        {
            self.generic_recheck.remove(at);
            let caps = self.closure_caps(r);
            self.rollback(cp);
            if let Some(g) = &mut parts.generic {
                g.caps = Some(caps);
            }
            (r, ft, own) = self.closure_with(n, parts, None)?;
        }
        let Some(l) = own else {
            return unsupported("a local function without its name");
        };
        let caps = self.closure_caps(r);
        let Some(g) = self.local_generics.get_mut(&l) else {
            return unsupported("a generic local function without its entry");
        };
        let row_of = |t: Ty| match pool.get(t) {
            TyData::Fn { row, .. } => Some(row),
            _ => None,
        };
        if g.recursive && row_of(g.fn_ty) != row_of(ft) {
            return unsupported(
                "a recursive local function whose requirements are inferred; write its `$` clause",
            );
        }
        g.fn_ty = ft;
        g.open = false;
        g.caps = Some(caps);
        let sub = g.sub as usize;
        let body = self.b.body_mut();
        body.local_ty[l.idx()] = ft;
        body.sub_flags[sub] |= sub_flags::GENERIC;
        Ok(l)
    }

    /// The expression of a parameter's default, if it has one.
    fn default_expr(p: NodeRef<'_>) -> Option<NodeRef<'_>> {
        Src::child(p, SyntaxKind::DefaultValue).and_then(|d| d.children().next())
    }

    /// The name of the hidden local holding a default's closure. It starts
    /// with `_`, which no read is warned about, and has a `.`, which no
    /// name written in source has.
    fn hidden_default(&self, func: &str, param: hd_base::Symbol) -> hd_base::Symbol {
        let p = self.cx.names.text(param).to_owned();
        self.cx.names.syms.intern(&format!("_default.{func}.{p}"))
    }

    /// `fn.default.order-final-function`: after the first parameter with a
    /// default, every following non-vararg parameter has one, but for a
    /// final parameter of function type.
    fn default_order(&mut self, params: &[NodeRef<'_>]) {
        let toks = &self.cx.src.parse.tokens;
        let mut defaulted = false;
        for (i, p) in params.iter().enumerate() {
            if Self::default_expr(*p).is_some() {
                defaulted = true;
                continue;
            }
            let exempt = p.direct_token(toks, TokenKind::Ellipsis).is_some()
                || (i + 1 == params.len()
                    && Src::type_child(*p).is_some_and(|t| t.kind() == SyntaxKind::FunctionType));
            if defaulted && !exempt {
                let name = p.name(toks).map_or("a parameter", |t| self.cx.src.text(t));
                let msg =
                    format!("`{name}` follows a parameter with a default, so it needs one too");
                self.err(Code::DefaultOrder, *p, &msg);
            }
        }
    }

    /// The default of parameter `i`, declared as the closure `hidden` of
    /// the parameters before it. A default is requirement-free
    /// (`fn.default.requirement-free`): it checks like a default body,
    /// and its row is empty.
    fn default_closure<'t>(
        &mut self,
        p: NodeRef<'t>,
        i: usize,
        hidden: hd_base::Symbol,
        expr: NodeRef<'t>,
        params: Option<NodeRef<'t>>,
        generic: Option<LocalGeneric>,
    ) -> StageResult<()> {
        let pool = self.pool();
        let mut parts = ClosureParts {
            params,
            ret: Src::type_child(p),
            row: None,
            body: expr,
            suspends: false,
            name: Some(hidden),
            reassignable: false,
            generic,
            upto: Some(i),
            form: None,
        };
        let saved = self.forbidden.replace("a default or fact expression");
        let saved_depth = std::mem::replace(&mut self.default_depth, self.suspends.len() + 1);
        let checked = if parts.generic.is_some() {
            self.generic_closure(p, &mut parts)
        } else {
            self.closure_with(p, &parts, None).and_then(|(r, ft, own)| {
                let Some(l) = own else {
                    return unsupported("a default without its name");
                };
                self.b.body_mut().local_ty[l.idx()] = ft;
                self.b.set(l, r, p.index());
                Ok(l)
            })
        };
        self.forbidden = saved;
        self.default_depth = saved_depth;
        let l = checked?;
        let ft = self.b.local_ty(l);
        if let TyData::Fn { row, .. } = pool.get(self.infer.resolve(pool, ft))
            && row != hd_types::RowId::EMPTY
        {
            self.err(
                Code::RequirementInDefault,
                expr,
                "a default cannot use a provider",
            );
        }
        Ok(())
    }

    /// The outer locals the closure `r` captures.
    fn closure_caps(&mut self, r: Ref) -> Vec<LocalId> {
        let body = self.b.body_mut();
        let Some(i) = r.as_inst() else {
            return Vec::new();
        };
        let rec = body.record(body.data[i.0 as usize][1]);
        let (start, len) = (rec[0] as usize, rec[1] as usize);
        body.cap_local[start..start + len].to_vec()
    }

    /// A use of the generic local function named by local `l`: its type
    /// arguments, the written ones and fresh variables after them, which
    /// the use solves by the end of its statement (`fn.type.generic.*`),
    /// and a closure of its body at them over its captures.
    pub(crate) fn local_generic_value(
        &mut self,
        l: LocalId,
        explicit: &[Ty],
        n: NodeRef<'_>,
    ) -> (Ref, Ty) {
        match self.local_generic_vars(l, explicit, n) {
            Some(vars) => self.local_generic_inst(l, &vars, n),
            None => (Ref(NONE), Ty::NEVER),
        }
    }

    /// The type arguments of a use of the generic local function `l`.
    pub(crate) fn local_generic_vars(
        &mut self,
        l: LocalId,
        explicit: &[Ty],
        n: NodeRef<'_>,
    ) -> Option<Vec<Ty>> {
        let g = self.local_generics.get(&l).cloned()?;
        let pool = self.pool();
        if explicit.len() > g.params.len() {
            let msg = format!(
                "{} type arguments are written, but the function takes {}",
                explicit.len(),
                g.params.len()
            );
            self.err(Code::ArgumentCount, n, &msg);
        }
        let vars: Vec<Ty> = (0..g.params.len())
            .map(|i| {
                explicit
                    .get(i)
                    .copied()
                    .unwrap_or_else(|| self.infer.fresh(pool, VarKind::General))
            })
            .collect();
        let (owner, first) = (self.gen_owner, g.first);
        let inst = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                let k = usize::from(p.index.checked_sub(first)?);
                (p.owner == owner).then(|| vars.get(k).copied()).flatten()
            })
        };
        for (i, &v) in vars.iter().enumerate() {
            if i >= explicit.len() {
                let d = g.defaults[i].map(inst);
                self.ref_params.push((v, d, n.index()));
            }
            for &b in &g.bounds[i] {
                if let TyData::TraitValue {
                    def: trait_, args, ..
                } = pool.get(inst(b))
                {
                    self.note_impl_use(trait_, None);
                    self.pending.push(PendingGoal {
                        tref: TraitRef {
                            trait_,
                            self_ty: v,
                            args,
                        },
                        at: n.index(),
                        kind: Waiting::Bound,
                    });
                }
            }
        }
        Some(vars)
    }

    /// A closure of the generic local function `l` at `vars`.
    pub(crate) fn local_generic_inst(
        &mut self,
        l: LocalId,
        vars: &[Ty],
        n: NodeRef<'_>,
    ) -> (Ref, Ty) {
        let Some(g) = self.local_generics.get(&l).cloned() else {
            return (Ref(NONE), Ty::NEVER);
        };
        let pool = self.pool();
        let (owner, first) = (self.gen_owner, g.first);
        let inst = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                let k = usize::from(p.index.checked_sub(first)?);
                (p.owner == owner).then(|| vars.get(k).copied()).flatten()
            })
        };
        let ft = inst(self.infer.resolve(pool, g.fn_ty));
        // Its captures cross every closure open around this use; inside
        // its own body they are its own.
        let nested = self
            .subs
            .last()
            .is_some_and(|s| s.mark.sub().raw() != g.sub);
        if let Some(caps) = &g.caps {
            for &c in caps {
                self.note_capture(c);
            }
        } else if g.open && nested && !self.generic_recheck.contains(&l) {
            self.generic_recheck.push(l);
        }
        if g.open
            && let Some(e) = self.local_generics.get_mut(&l)
        {
            e.recursive = true;
        }
        let list = pool.list(vars);
        let rec = self.b.words_record(&[0, 0, u32::from(first), list.0]);
        (self.b.emit(Tag::Closure, g.sub, rec, ft, n.index()), ft)
    }

    /// A read of outer local `l` that creates no value: it is captured
    /// into every open closure it crosses.
    fn note_capture(&mut self, l: LocalId) {
        let Some(depth) = self
            .scopes
            .iter()
            .rposition(|m| m.values().any(|v| *v == l))
        else {
            return;
        };
        let mut captured = false;
        for s in &self.subs {
            if s.depth > depth {
                self.b.capture(s.mark, l);
                captured = true;
            }
        }
        let flags = &mut self.b.body_mut().local_flags[l.idx()];
        *flags |= local_flags::READ;
        if captured {
            *flags |= local_flags::CAPTURED;
        }
    }
}
