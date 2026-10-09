//! Block-local declarations in a body (checking-and-tir.md "Local items
//! lift to hidden module items"). A local `data`, `enum`, `trait`, `type`
//! or `impl` is a hidden module item, lowered and checked as one; its
//! statement here executes nothing (`names.local-type.static`,
//! `names.local-impl.static`) and only checks what depends on where it
//! stands. A local `fn` is a named closure value whose name its own body
//! sees (`fn.local.capture-rules`, `names.local-fn.visible`).

use hd_base::{DefId, StageResult};
use hd_diag::Code;
use hd_resolve::{HeadKind, ItemData, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::TirSink;
use hd_types::TyData;

use crate::body::{Ck, unsupported};
use crate::expr::ClosureParts;

impl Ck<'_, '_> {
    /// A local type, trait or impl declaration statement.
    pub(crate) fn local_decl(&mut self, s: NodeRef<'_>) -> StageResult<()> {
        let lo = self.cx.src.span(s).lo;
        let Some(l) = self.cx.locals.iter().find(|l| l.lo == lo).copied() else {
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
        if let Some(gl) = Src::child(n, SyntaxKind::GenericParameterList) {
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
            return self.gap(n, "a generic local function");
        }
        let params = Src::child(n, SyntaxKind::ParameterList);
        if params
            .iter()
            .flat_map(|p| p.children())
            .any(|p| Src::child(p, SyntaxKind::DefaultValue).is_some())
        {
            return self.gap(n, "a local function with a parameter default");
        }
        // `names.local-fn.rules`: a local value's duplicate rule.
        if self.scopes.last().is_some_and(|m| m.contains_key(&name)) {
            let msg = format!("`{}` is already bound in this scope", src.text(t));
            self.err(Code::DuplicateBinding, n, &msg);
        }
        let Some(body) = Src::child(n, SyntaxKind::Block) else {
            return unsupported("a local function without a body");
        };
        let parts = ClosureParts {
            params,
            ret: n
                .children()
                .find(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow),
            row: Src::child(n, SyntaxKind::RequirementRow),
            body,
            suspends: n.direct_token(toks, TokenKind::Bang).is_some(),
            name: Some(name),
        };
        let (r, ft, own) = self.closure_with(n, &parts, None)?;
        let Some(l) = own else {
            return unsupported("a local function without its name");
        };
        self.b.body_mut().local_ty[l.idx()] = ft;
        self.b.set(l, r, n.index());
        Ok(())
    }
}
