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

use crate::body::{BodyCx, new_ck};

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
            part_name: &part_name,
            def,
            tm,
        };
        out.extend(fwd.body(cx, &sig, node, diags)?);
    }
    Ok(out)
}

/// One forwarding method: `def` forwards to the trait method `tm` at the
/// part's type.
struct Forwarder<'s> {
    trait_: DefId,
    trait_args: TyList,
    part: Ty,
    part_name: &'s str,
    def: DefId,
    tm: DefId,
}

impl Forwarder<'_> {
    fn body(
        &self,
        cx: &BodyCx<'_>,
        sig: &FnSig,
        node: NodeRef<'_>,
        diags: &mut DiagBuf,
    ) -> StageResult<Option<Body>> {
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
        let blk = ck.b.open_block();
        let params: Vec<_> = sig
            .params
            .iter()
            .map(|&(n, t)| {
                let t = ck.norm_ty(t);
                (ck.b.local(t, n, local_flags::PARAM, at), t)
            })
            .collect();
        let Some(&(sl, st)) = params.first() else {
            return crate::body::unsupported("a forwarding method without a receiver");
        };
        // The receiver `self.E`, with `self`'s access.
        let sv = ck.b.get(sl, st, at);
        let Some((idx, ft)) = ck.field_of(st, self.part_name) else {
            return crate::body::unsupported("a forwarding method whose part is not a field");
        };
        let ft = ck.field_access(st, self.part_name, ft);
        let recv = ck.b.emit(hd_tir::ir::Tag::Field, sv.0, idx, ft, at);
        let mut refs = vec![recv];
        for &(l, t) in &params[1..] {
            refs.push(ck.b.get(l, t, at));
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
            return Ok(None);
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
        targs.extend(&own);
        let callee = Callee::TraitMethod {
            trait_,
            method: tm,
            self_ty: part,
            targs: pool.list(&targs),
            choice,
        };
        let (r, rt) = ck.emit_call(&callee, &refs, ret, tsig.suspends, tsig.suspends, node);
        let r = ck.coerce(r, rt, ret, node, "a forwarded result");
        let root = ck.b.close_block(blk, Some(r), ret, at);
        ck.finish_body(root).map(Some)
    }
}
