//! The methods of `@derive` on a newtype (spec 09
//! `trait.derive.newtype.base`): each applies the base type's method to the
//! wrapped values and wraps the result back. Header lowering declares one
//! method per method of the trait, with the trait's signature at `Self` =
//! the newtype (`hd_resolve::lower`, as for a delegation). Erased newtypes
//! forward to their base implementation directly, so no adapter bodies are
//! built here; only the position findings are reported, by the same
//! forwarder that writes a delegation's (`crate::delegate`).

use hd_base::{DefId, StageResult};
use hd_diag::DiagBuf;
use hd_intern::PathKind;
use hd_resolve::{ImplKind, ItemData};
use hd_syntax::NodeRef;
use hd_tir::ir::{IntrinsicOp, NONE, PrimOp, Ref, Tag, TirSink, local_flags};
use hd_types::{ParamRef, Prim, Ty, TyData};

use crate::body::{BodyCx, Ck, ExpectPosition, unsupported};
use crate::delegate::{Forwarded, Forwarder};

/// One problem with a derived newtype's forwarding: the derived
/// implementation, and what stops the method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeriveFinding {
    pub impl_: DefId,
    pub message: String,
}

/// The problems with the methods of every `@derive` on the newtype
/// `newtype`, declared at `node`: a method with `Self` anywhere but the
/// receiver, `Self`, `Self?`, `Result[Self, E]` or `List[Self]` is one
/// finding (`trait.derive.newtype.self-error`). Erased newtypes forward
/// to their base implementation directly, so no adapter bodies are
/// built; the position analysis still runs to report its findings.
pub fn derived_newtype_findings(
    cx: &BodyCx<'_>,
    newtype: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Vec<DeriveFinding>> {
    let names = &cx.names;
    let pool = names.pool;
    let mut findings = Vec::new();
    for it in cx.lookup.own {
        let ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            methods,
            kind: ImplKind::Derived,
            ..
        } = &it.data
        else {
            continue;
        };
        let TyData::Adt { def, args } = pool.get(*self_ty) else {
            continue;
        };
        if def != newtype {
            continue;
        }
        let Some(ItemData::Newtype(inner)) = cx.lookup.item(def).map(|i| &i.data) else {
            continue;
        };
        let ca = pool.list_items(args);
        let base = pool.subst(*inner, &|q: ParamRef| {
            (q.owner == newtype)
                .then(|| ca.get(q.index as usize).copied())
                .flatten()
        });
        for &(name, def) in methods {
            let Some(sig) = cx.lookup.item(def).and_then(|i| i.sig()).cloned() else {
                continue;
            };
            let fwd = Forwarder {
                trait_: *trait_,
                trait_args: *trait_args,
                part: base,
                part_name: None,
                newtype: Some(*self_ty),
                def,
                tm: names.member(*trait_, PathKind::Member, names.text(name)),
            };
            match fwd.body(cx, &sig, node, diags)? {
                Forwarded::Body(_) => {}
                Forwarded::Position(message) => findings.push(DeriveFinding {
                    impl_: it.def,
                    message,
                }),
                Forwarded::Nothing => {}
            }
        }
    }
    Ok(findings)
}

/// How a derived newtype's method values cross between the newtype and
/// its base type: the receiver and `Self` unwrap and wrap, and so do the
/// `Self` inside `Self?`, `Result[Self, E]` and `List[Self]`
/// (`trait.derive.newtype.self-positions`). Every other position holds
/// the same type on both sides.
pub(crate) struct Conversion {
    pub newtype: Ty,
    pub base: Ty,
}

impl Conversion {
    /// Whether a value of `from` converts to `to`.
    pub(crate) fn can(&self, ck: &Ck<'_, '_>, from: Ty, to: Ty) -> bool {
        let pool = ck.pool();
        let (from, to) = (ck.strip_mut(from), ck.strip_mut(to));
        if from == to || self.is_wrap(from, to) || self.is_unwrap(from, to) {
            return true;
        }
        let known = ck.cx.names.known;
        match (pool.get(from), pool.get(to)) {
            (TyData::Option(a), TyData::Option(b)) => self.can(ck, a, b),
            (TyData::Adt { def: d, args: a }, TyData::Adt { def: e, args: b }) if d == e => {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                if d == known.list {
                    self.can(ck, a[0], b[0])
                } else if d == known.result {
                    a[1] == b[1] && self.can(ck, a[0], b[0])
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn is_wrap(&self, from: Ty, to: Ty) -> bool {
        from == self.base && to == self.newtype
    }

    fn is_unwrap(&self, from: Ty, to: Ty) -> bool {
        from == self.newtype && to == self.base
    }

    /// The value `r` of type `from` as type `to`, when `can` holds.
    pub(crate) fn apply(
        &self,
        ck: &mut Ck<'_, '_>,
        r: Ref,
        (from, to): (Ty, Ty),
        node: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let pool = ck.pool();
        let at = node.index();
        let (f, t) = (ck.strip_mut(from), ck.strip_mut(to));
        if f == t {
            return Ok(ck.coerce(
                r,
                from,
                to,
                node,
                "a forwarded value",
                ExpectPosition::General,
            ));
        }
        if self.is_unwrap(f, t) {
            return Ok(ck.b.field(r, 0, to, at));
        }
        if self.is_wrap(f, t) {
            // The newtype's value, as its constructor builds it.
            let fresh = pool.intern_ty(&TyData::Mut(t));
            let rec = ck.b.refs_record(&[r]);
            let made = ck.b.emit(Tag::NewStruct, NONE, rec, fresh, at);
            return Ok(ck.coerce(
                made,
                fresh,
                to,
                node,
                "a wrapped value",
                ExpectPosition::General,
            ));
        }
        let known = ck.cx.names.known;
        match (pool.get(f), pool.get(t)) {
            (TyData::Option(a), TyData::Option(b)) => {
                let tb = ck.b.open_block();
                let u = ck.b.payload(r, 1, 0, a, at);
                let c = self.apply(ck, u, (a, b), node)?;
                let rec = ck.b.refs_record(&[c]);
                let some = ck.b.emit(Tag::NewVariant, 1, rec, t, at);
                let then = ck.b.close_block(tb, Some(some), t, at);
                let eb = ck.b.open_block();
                let empty = ck.b.refs_record(&[]);
                let absent = ck.b.emit(Tag::NewVariant, 0, empty, t, at);
                let other = ck.b.close_block(eb, Some(absent), t, at);
                let rec = ck.b.refs_record(&[Ref(1), then, other]);
                Ok(ck.b.emit(Tag::SwitchTag, r.0, rec, t, at))
            }
            (TyData::Adt { def, args: a }, TyData::Adt { args: b, .. }) if def == known.result => {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                let ob = ck.b.open_block();
                let v = ck.b.payload(r, 0, 0, a[0], at);
                let c = self.apply(ck, v, (a[0], b[0]), node)?;
                let rec = ck.b.refs_record(&[c]);
                let okv = ck.b.emit(Tag::NewVariant, 0, rec, t, at);
                let ok = ck.b.close_block(ob, Some(okv), t, at);
                let eb = ck.b.open_block();
                let e = ck.b.payload(r, 1, 0, a[1], at);
                let rec = ck.b.refs_record(&[e]);
                let errv = ck.b.emit(Tag::NewVariant, 1, rec, t, at);
                let err = ck.b.close_block(eb, Some(errv), t, at);
                let rec = ck.b.refs_record(&[Ref(0), ok, err]);
                Ok(ck.b.emit(Tag::SwitchTag, r.0, rec, t, at))
            }
            (TyData::Adt { def, args: a }, TyData::Adt { args: b, .. }) if def == known.list => {
                let (a, b) = (pool.list_items(a)[0], pool.list_items(b)[0]);
                self.list(ck, r, (f, t), (a, b), node)
            }
            _ => unsupported("a derived newtype value that does not convert"),
        }
    }

    /// A list of `from`'s elements as a list of `to`'s, by a counting
    /// loop: each element converts, then joins the new list.
    fn list(
        &self,
        ck: &mut Ck<'_, '_>,
        r: Ref,
        (list_from, list_to): (Ty, Ty),
        (from, to): (Ty, Ty),
        node: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let pool = ck.pool();
        let at = node.index();
        let usize_ty = Ty::prim(Prim::Usize);
        let out_ty = pool.intern_ty(&TyData::Mut(list_to));
        let sym = |ck: &Ck<'_, '_>, s: &str| ck.cx.names.syms.intern(s);
        let (src_s, out_s, i_s, n_s) = (
            sym(ck, "$src"),
            sym(ck, "$out"),
            sym(ck, "$i"),
            sym(ck, "$len"),
        );
        let src_l = ck.b.local(list_from, src_s, local_flags::ASSIGNED, at);
        let out_l = ck.b.local(out_ty, out_s, local_flags::ASSIGNED, at);
        let i_l = ck.b.local(usize_ty, i_s, local_flags::ASSIGNED, at);
        let n_l = ck.b.local(usize_ty, n_s, local_flags::ASSIGNED, at);
        ck.set_local(src_l, r, at);
        let empty = ck.b.refs_record(&[]);
        let fresh = ck.b.emit(Tag::NewList, NONE, empty, out_ty, at);
        ck.set_local(out_l, fresh, at);
        let zero = ck.b.const_value(usize_ty, 0);
        ck.set_local(i_l, zero, at);
        let s = ck.b.get(src_l, list_from, at);
        let rec = ck.b.refs_record(&[s]);
        let len = ck.b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListLen as u32,
            rec,
            usize_ty,
            at,
        );
        ck.set_local(n_l, len, at);
        let lp = ck.b.open_loop();
        let lb = ck.b.open_block();
        let (i, n) = (ck.b.get(i_l, usize_ty, at), ck.b.get(n_l, usize_ty, at));
        let below = ck.b.prim(PrimOp::Lt as u32, &[i, n], Ty::BOOL, at);
        let tb = ck.b.open_block();
        let s = ck.b.get(src_l, list_from, at);
        let i = ck.b.get(i_l, usize_ty, at);
        let item = ck.b.element(s, i, from, at);
        let c = self.apply(ck, item, (from, to), node)?;
        let o = ck.b.get(out_l, out_ty, at);
        let rec = ck.b.refs_record(&[o, c]);
        ck.b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListPush as u32,
            rec,
            Ty::VOID,
            at,
        );
        let i = ck.b.get(i_l, usize_ty, at);
        let one = ck.b.const_value(usize_ty, 1);
        let next = ck.b.prim(PrimOp::Add as u32, &[i, one], usize_ty, at);
        ck.set_local(i_l, next, at);
        let then = ck.b.close_block(tb, None, Ty::VOID, at);
        let eb = ck.b.open_block();
        ck.b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, at);
        let els = ck.b.close_block(eb, None, Ty::VOID, at);
        let rec = ck.b.refs_record(&[then, els]);
        ck.b.emit(Tag::If, below.0, rec, Ty::VOID, at);
        let body = ck.b.close_block(lb, None, Ty::VOID, at);
        ck.b.close_loop(lp, body, Ty::VOID, at);
        let o = ck.b.get(out_l, out_ty, at);
        Ok(ck.coerce(
            o,
            out_ty,
            list_to,
            node,
            "a rewrapped list",
            ExpectPosition::General,
        ))
    }
}
