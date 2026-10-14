//! The forwarding methods of a delegating implementation `impl Tr for C
//! by E` (spec 09 "Generated Methods", trait.by.generated; trait-solver.md
//! §3.10). Header lowering declares one per method of `Tr` with a receiver
//! that the body does not write, with the trait method's signature at
//! `Self = C[...]` (`hd_resolve::lower::delegations`). Each body here is
//! `Tr::m(self.E, arguments...)`: the part's implementation runs with the
//! part as its receiver, so a default body there calls the part's methods
//! (no overriding), and a `mut self` method forwards through `self.E`,
//! which has `mut` access through `mut self`. A rest parameter is already
//! the list the trait method takes, so it passes on as it is.

use hd_base::{DefId, StageResult};
use hd_diag::DiagBuf;
use hd_intern::PathKind;
use hd_resolve::{FnSig, ImplKind, ItemData};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::Body;
use hd_tir::ir::{BodyKind, Callee, TirSink, local_flags};
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, Ty, TyData, TyList, with_assoc_args};

use crate::body::{BodyCx, Ck, ExpectPosition, new_ck};
use crate::newtype::Conversion;

/// The bodies of the forwarding methods of the implementation `imp`,
/// declared at `node`. A part that is not an embedded field of a data
/// type, or does not implement the trait, has no forwarders; the header
/// check reports it (`invalid-delegation`).
pub fn forwarder_bodies(
    cx: &BodyCx<'_>,
    imp: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Vec<Body>> {
    let names = &cx.names;
    let pool = names.pool;
    let Some(ItemData::Impl {
        trait_,
        trait_args,
        self_ty,
        methods,
        by: Some(by),
        kind: ImplKind::Delegated,
        ..
    }) = cx.lookup.item(imp).map(|i| &i.data)
    else {
        return Ok(Vec::new());
    };
    let TyData::Adt {
        def: c,
        args: cargs,
    } = pool.get(*self_ty)
    else {
        return Ok(Vec::new());
    };
    let Some(ItemData::Data(fields)) = cx.lookup.item(c).map(|i| &i.data) else {
        return Ok(Vec::new());
    };
    let Some(field) = fields.iter().find(|f| f.embedded && f.name == *by) else {
        return Ok(Vec::new());
    };
    let ca = pool.list_items(cargs);
    let part = pool.subst(field.ty, &|q: ParamRef| {
        (q.owner == c)
            .then(|| ca.get(q.index as usize).copied())
            .flatten()
    });
    // The methods the body writes are checked as written.
    let written: Vec<&str> = node
        .children()
        .filter(|b| b.kind() == SyntaxKind::Block)
        .flat_map(NodeRef::children)
        .filter(|f| f.kind() == SyntaxKind::FnDecl)
        .filter_map(|f| {
            f.name(&cx.src.parse.tokens)
                .or_else(|| cx.src.name_after(f, hd_syntax::TokenKind::KwFn))
        })
        .map(|t| cx.src.text(t))
        .collect();
    let part_name = names.text(*by).to_owned();
    let mut out = Vec::new();
    for &(name, def) in methods {
        if written.contains(&names.text(name)) {
            continue;
        }
        let Some(sig) = cx.lookup.item(def).and_then(|i| i.sig()).cloned() else {
            continue;
        };
        let tm = names.member(*trait_, PathKind::Member, names.text(name));
        let fwd = Forwarder {
            trait_: *trait_,
            trait_args: *trait_args,
            part,
            part_name: Some(&part_name),
            newtype: None,
            def,
            tm,
        };
        if let Forwarded::Body(b) = fwd.body(cx, &sig, node, diags)? {
            out.push(*b);
        }
    }
    Ok(out)
}

/// One forwarding method: `def` forwards to the trait method `tm` at the
/// part's type.
pub(crate) struct Forwarder<'s> {
    pub(crate) trait_: DefId,
    pub(crate) trait_args: TyList,
    pub(crate) part: Ty,
    /// The embedded field a delegation forwards to. `None` for a derived
    /// newtype, whose part is its base type.
    pub(crate) part_name: Option<&'s str>,
    /// The derived newtype's own type, when this forwards for one.
    pub(crate) newtype: Option<Ty>,
    pub(crate) def: DefId,
    pub(crate) tm: DefId,
}

/// What a forwarding method comes to: its body, the derivation problem
/// that stops it (`newtype-derivation-self`), or nothing (the part does
/// not implement the trait, which the header check reports).
pub(crate) enum Forwarded {
    Body(Box<Body>),
    Position(String),
    Nothing,
}

impl Forwarder<'_> {
    pub(crate) fn body(
        &self,
        cx: &BodyCx<'_>,
        sig: &FnSig,
        node: NodeRef<'_>,
        diags: &mut DiagBuf,
    ) -> StageResult<Forwarded> {
        let local = hd_types::LocalPool::new();
        let mut ck = new_ck(
            cx,
            &local,
            self.def,
            self.def,
            BodyKind::Fn,
            (sig.ret, sig.row),
            diags,
        );
        let pool = ck.pool();
        let at = node.index();
        // As a written method's body: what is in scope where the
        // implementation is written, its suspension, and its signature's
        // projections normalized under the bounds.
        ck.move_to(cx.src.span(node).lo);
        ck.suspends = vec![sig.suspends];
        let ret = ck.norm_ty(sig.ret);
        ck.rets[0] = ret;
        let sig_ret = ret;
        let blk = ck.b.open_block();
        let params: Vec<_> = sig
            .params
            .iter()
            .map(|&(n, t)| {
                let t = ck.norm_ty(t);
                (ck.b.local(t, n, local_flags::PARAM, at), t)
            })
            .collect();
        let mut refs = Vec::new();
        if let Some(part_name) = self.part_name {
            let Some(&(sl, st)) = params.first() else {
                return crate::body::unsupported("a forwarding method without a receiver");
            };
            // The receiver `self.E`, with `self`'s access.
            let sv = ck.b.get(sl, st, at);
            let Some((idx, ft)) = ck.field_of(st, part_name) else {
                return crate::body::unsupported("a forwarding method whose part is not a field");
            };
            let ft = ck.field_access(st, part_name, ft);
            refs.push(ck.b.emit(hd_tir::ir::Tag::Field, sv.0, idx, ft, at));
            for &(l, t) in &params[1..] {
                refs.push(ck.b.get(l, t, at));
            }
        }
        // `Tr::m` at the part: the trait's arguments, then the method's
        // own parameters as the forwarder's.
        let mut targs = pool.list_items(self.trait_args).to_vec();
        let own: Vec<Ty> = (0..sig.generics.len())
            .map(|j| {
                pool.intern_ty(&TyData::Param(ParamRef {
                    owner: self.def,
                    index: u16::try_from(j).unwrap_or(u16::MAX),
                }))
            })
            .collect();
        let tref = TraitRef {
            trait_: self.trait_,
            self_ty: self.part,
            args: self.trait_args,
        };
        // A part that does not implement the trait is the header's
        // `invalid-delegation` (trait.by.part-impl): nothing to forward to.
        let hd_types::solver::Answer::Holds { evidence, .. } = ck.solve(tref)? else {
            return Ok(Forwarded::Nothing);
        };
        let choice = ck.choice_of(Some(&evidence));
        let tsig = ck.sig_of(self.tm)?;
        let ta = pool.list_items(self.trait_args);
        let (trait_, tm, part) = (self.trait_, self.tm, self.part);
        let inst = |x: Ty| {
            let x = pool.subst(x, &|q: ParamRef| {
                if q.owner == trait_ {
                    if q.index == 0 {
                        Some(part)
                    } else {
                        ta.get(usize::from(q.index) - 1).copied()
                    }
                } else if q.owner == tm {
                    own.get(usize::from(q.index)).copied()
                } else {
                    None
                }
            });
            with_assoc_args(pool, x, trait_, self.trait_args)
        };
        let ret = ck.normalize_deep(inst(tsig.ret))?;
        // A derived newtype applies the base type's method to the wrapped
        // values (trait.derive.newtype.base): each argument at its base
        // form, the result back at the newtype's.
        let conv = self.newtype.map(|newtype| Conversion {
            newtype,
            base: self.part,
        });
        if let Some(conv) = &conv {
            for (&(l, t), (_, want)) in params.iter().zip(&tsig.params) {
                let want = ck.normalize_deep(inst(*want))?;
                if !conv.can(&ck, t, want) {
                    return Ok(Forwarded::Position(self.position_message(&ck, t)));
                }
                let v = ck.b.get(l, t, at);
                refs.push(conv.apply(&mut ck, v, (t, want), node)?);
            }
            if !conv.can(&ck, ret, sig_ret) {
                return Ok(Forwarded::Position(self.position_message(&ck, sig_ret)));
            }
        }
        targs.extend(&own);
        let callee = Callee::TraitMethod {
            trait_,
            method: tm,
            self_ty: part,
            targs: pool.list(&targs),
            choice,
        };
        let (r, rt) = ck.emit_call(&callee, &refs, ret, tsig.suspends, tsig.suspends, node);
        let r = ck.coerce(
            r,
            rt,
            ret,
            node,
            "a forwarded result",
            ExpectPosition::General,
        );
        let (r, ret) = match &conv {
            Some(conv) => (conv.apply(&mut ck, r, (ret, sig_ret), node)?, sig_ret),
            None => (r, ret),
        };
        let root = ck.b.close_block(blk, Some(r), ret, at);
        ck.finish_body(root).map(|b| Forwarded::Body(Box::new(b)))
    }

    /// The error for a method position the derivation cannot forward
    /// (`trait.derive.newtype.self-error`).
    fn position_message(&self, ck: &Ck<'_, '_>, at: Ty) -> String {
        format!(
            "`{}` has `{}` where a newtype forwards only the receiver, Self, Self?, Result[Self, E] and List[Self]",
            ck.cx.names.display_name(self.tm),
            ck.show(at)
        )
    }
}
