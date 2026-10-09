//! The bodies of the implementations `@error` generates (spec 14 "Error
//! Derivation"; codegen.md §13.14). Header lowering declares the heads
//! (`hd_resolve::error_type`); each body here is checked as ordinary code
//! of the error type's module, so a message is type-checked in its
//! variant's scope (`annot.error.message.checked`). A cause member's type
//! must implement `Error`, checked as a bound before its conversion to
//! `dyn Error` (`annot.error.cause.type`); a transparent member's, by
//! the `Error.cause` call on it (`annot.error.transparent.type`).
//!
//! - `Display::to_string` matches the variant and evaluates its message
//!   with the members it interpolates bound by name; a transparent
//!   variant calls its member's `to_string`; a variant without a message
//!   is its name (`annot.error.message.*`, `annot.error.transparent.display`).
//! - `Error::cause` returns the `@from` or `@source` member as a
//!   `dyn Error`, an optional one only when present, a transparent
//!   member's own `cause`, or `.None` (`annot.error.cause.*`,
//!   `annot.error.transparent.cause`).
//! - `From[P]::from` builds the variant or data value holding the payload
//!   (`annot.error.from.generate`, `annot.error.from.data`).
//!
//! The method's parameter is never bound to a name, so `$self` in a
//! message names nothing (`annot.error.message.no-self`).

use std::collections::HashMap;

use hd_base::{DefId, LocalId, StageResult, Symbol};
use hd_diag::DiagBuf;
use hd_resolve::error_type::{ErrorShape, Generated, Message, interpolated, label};
use hd_resolve::{Field, FnSig, ItemData};
use hd_syntax::NodeRef;
use hd_tir::Body;
use hd_tir::ir::{BodyKind, NONE, Ref, Tag, TirSink, local_flags};
use hd_types::solver::TraitRef;
use hd_types::{ParamRef, RowId, Ty, TyData, TyList};

use crate::body::{BodyCx, Ck, new_ck, unsupported};

/// The error type a body is generated for.
struct Target<'s, 't> {
    ty: DefId,
    shape: &'s ErrorShape<'t>,
    /// Each variant's members over the type's parameters.
    fields: Vec<Vec<Field>>,
    /// Each enum variant's name and item (whose shared data globals it reads).
    variants: Vec<(Symbol, DefId)>,
    /// The enum's named shared fields (`annot.error.message.shared`).
    shared: Vec<Field>,
}

/// The bodies of every implementation `@error` generates for the data type
/// or enum `ty` with the error shape `shape`.
pub fn error_bodies(
    cx: &BodyCx<'_>,
    ty: DefId,
    shape: &ErrorShape<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Vec<Body>> {
    let names = &cx.names;
    let Some(item) = cx.lookup.item(ty) else {
        return unsupported("an error type outside the module");
    };
    let fields = hd_resolve::error_type::fields(&item.data)
        .into_iter()
        .map(<[Field]>::to_vec)
        .collect();
    let (variants, shared) = match &item.data {
        ItemData::Enum { shared, variants } => (
            variants.iter().map(|v| (v.name, v.def)).collect(),
            // Unnamed shared data is not in scope
            // (`annot.error.message.shared-unnamed`).
            shared
                .iter()
                .filter(|f| !names.text(f.name).starts_with(|c: char| c.is_ascii_digit()))
                .cloned()
                .collect(),
        ),
        _ => (vec![(item.name, ty)], Vec::new()),
    };
    let target = Target {
        ty,
        shape,
        fields,
        variants,
        shared,
    };
    let mut out = Vec::new();
    for g in shape.generated() {
        let def = g.def(names, ty);
        // A `@from` of a bare type parameter has no implementation, and an
        // `Error` without a cause no `cause`.
        let Some(ItemData::Impl { methods, .. }) = cx.lookup.item(def).map(|i| &i.data) else {
            continue;
        };
        let Some(&(_, method)) = methods.first() else {
            continue;
        };
        let Some(sig) = cx.lookup.item(method).and_then(|i| i.sig()).cloned() else {
            continue;
        };
        out.push(target.body(cx, g, def, method, &sig, diags)?);
    }
    Ok(out)
}

impl Target<'_, '_> {
    /// The body of `g`'s method `method`, of the implementation `def`.
    fn body(
        &self,
        cx: &BodyCx<'_>,
        g: Generated,
        def: DefId,
        method: DefId,
        sig: &FnSig,
        diags: &mut DiagBuf,
    ) -> StageResult<Body> {
        let local = hd_types::LocalPool::new();
        let mut ck = new_ck(
            cx,
            &local,
            method,
            method,
            BodyKind::Fn,
            (sig.ret, RowId::EMPTY),
            diags,
        );
        let at = self.shape.line;
        let Some(&(pname, pty)) = sig.params.first() else {
            return unsupported("a generated error method without a parameter");
        };
        let blk = ck.b.open_block();
        let p = ck.b.local(pty, pname, local_flags::PARAM, at.index());
        // The members' types over the implementation's own parameters.
        let own = |t: Ty| {
            cx.names.pool.subst(t, &|q: ParamRef| {
                (q.owner == self.ty).then(|| {
                    cx.names.pool.intern_ty(&TyData::Param(ParamRef {
                        owner: def,
                        index: q.index,
                    }))
                })
            })
        };
        let ret = sig.ret;
        let tail = match g {
            Generated::Display => {
                let arms: Vec<usize> = (0..self.shape.variants.len()).collect();
                self.dispatch(&mut ck, (p, pty), ret, &arms, &mut |me, ck, vi| {
                    me.display_arm(ck, (p, pty), vi, &own)
                })?
            }
            Generated::Error => {
                // The variants with a cause each test their tag; every other
                // variant shares one `.None` arm.
                let n = self.shape.variants.len();
                let mut arms: Vec<usize> = (0..n).filter(|vi| self.has_cause(*vi)).collect();
                if let Some(rest) = (0..n).find(|vi| !self.has_cause(*vi)) {
                    arms.push(rest);
                }
                self.dispatch(&mut ck, (p, pty), ret, &arms, &mut |me, ck, vi| {
                    me.cause_arm(ck, (p, pty), vi, ret, &own)
                })?
            }
            Generated::From { variant, .. } => {
                let v = ck.b.get(p, pty, at.index());
                let rec = ck.b.refs_record(&[v]);
                if self.shape.is_enum {
                    let index = u32::try_from(variant).unwrap_or(u32::MAX);
                    ck.b.emit(Tag::NewVariant, index, rec, ret, at.index())
                } else {
                    let fresh = cx.names.pool.intern_ty(&TyData::Mut(ret));
                    let r = ck.b.emit(Tag::NewData, NONE, rec, fresh, at.index());
                    ck.coerce(r, fresh, ret, at, "a conversion")
                }
            }
        };
        let root = ck.b.close_block(blk, Some(tail), ret, at.index());
        ck.finish_body(root)
    }

    /// The arms of `arms`, chosen by the value's tag: each but the last
    /// tests its variant, and the last is every other variant. A data type
    /// is its one arm.
    fn dispatch(
        &self,
        ck: &mut Ck<'_, '_>,
        p: (LocalId, Ty),
        ret: Ty,
        arms: &[usize],
        arm: &mut dyn FnMut(&Self, &mut Ck<'_, '_>, usize) -> StageResult<Ref>,
    ) -> StageResult<Ref> {
        let Some((&last, tested)) = arms.split_last() else {
            let at = self.shape.line.index();
            return Ok(ck.b.emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, at));
        };
        let Some((&k, remaining)) = tested.split_first() else {
            return arm(self, ck, last);
        };
        let at = self.shape.variants[k].node.index();
        let tb = ck.b.open_block();
        let t = arm(self, ck, k)?;
        let then = ck.b.close_block(tb, Some(t), ret, at);
        let eb = ck.b.open_block();
        let mut others = remaining.to_vec();
        others.push(last);
        let e = self.dispatch(ck, p, ret, &others, arm)?;
        let other = ck.b.close_block(eb, Some(e), ret, at);
        let v = ck.b.get(p.0, p.1, at);
        let index = u32::try_from(k).unwrap_or(u32::MAX);
        let rec = ck.b.refs_record(&[Ref(index), then, other]);
        Ok(ck.b.emit(Tag::SwitchTag, v.0, rec, ret, at))
    }

    /// Whether variant `vi` has a cause: a cause member, or a transparent
    /// member's own.
    fn has_cause(&self, vi: usize) -> bool {
        let v = &self.shape.variants[vi];
        self.members(vi).is_some()
            && (matches!(v.message, Message::Transparent) || v.cause().is_some())
    }

    /// Member `j` of variant `vi` read from the value in `p`.
    fn read(
        &self,
        ck: &mut Ck<'_, '_>,
        (p, pty): (LocalId, Ty),
        vi: usize,
        j: usize,
        t: Ty,
    ) -> Ref {
        let at = self.shape.variants[vi].members[j].node.index();
        let v = ck.b.get(p, pty, at);
        let field = u32::try_from(j).unwrap_or(u32::MAX);
        if self.shape.is_enum {
            let index = u32::try_from(vi).unwrap_or(u32::MAX);
            let rec = ck.b.refs_record(&[Ref(index), Ref(field)]);
            ck.b.emit(Tag::Payload, v.0, rec, t, at)
        } else {
            ck.b.emit(Tag::Field, v.0, field, t, at)
        }
    }

    /// The member types of variant `vi`, when its members line up with its
    /// declaration.
    fn members(&self, vi: usize) -> Option<&[Field]> {
        let fs = self.fields.get(vi)?;
        (fs.len() == self.shape.variants[vi].members.len()).then_some(fs.as_slice())
    }

    /// The text of variant `vi` (`annot.error.message.*`,
    /// `annot.error.transparent.display`).
    fn display_arm(
        &self,
        ck: &mut Ck<'_, '_>,
        p: (LocalId, Ty),
        vi: usize,
        own: &dyn Fn(Ty) -> Ty,
    ) -> StageResult<Ref> {
        let v = &self.shape.variants[vi];
        let cx = ck.cx;
        let names = &cx.names;
        match (v.message, self.members(vi)) {
            (Message::Text(msg), Some(fs)) => {
                // The members and shared fields the message names, bound by
                // name (`annot.error.message.scope`).
                let used = interpolated(&cx.src, msg);
                let mut scope = HashMap::new();
                for (j, f) in fs.iter().enumerate() {
                    let name = label(names, f);
                    if !used.contains(&name) {
                        continue;
                    }
                    let t = own(f.ty);
                    let r = self.read(ck, p, vi, j, t);
                    let sym = names.syms.intern(&name);
                    let at = v.members[j].node.index();
                    let l = ck.b.local(t, sym, local_flags::ASSIGNED, at);
                    ck.b.set(l, r, at);
                    scope.insert(sym, l);
                }
                if self.shape.is_enum
                    && let Some(&(_, vdef)) = self.variants.get(vi)
                {
                    for f in &self.shared {
                        let name = names.text(f.name).to_owned();
                        if !used.contains(&name) || scope.contains_key(&f.name) {
                            continue;
                        }
                        let g = crate::init::shared_global(names, vdef, &name);
                        let rec = ck.b.refs_record(&[Ref(g.raw())]);
                        let r = ck.b.emit(Tag::GlobalGet, rec, NONE, f.ty, msg.index());
                        let l = ck.b.local(f.ty, f.name, local_flags::ASSIGNED, msg.index());
                        ck.b.set(l, r, msg.index());
                        scope.insert(f.name, l);
                    }
                }
                ck.scopes.push(scope);
                let checked = ck.expr(msg, Some(Ty::STRING));
                ck.scopes.pop();
                let (r, t) = checked?;
                Ok(ck.coerce(r, t, Ty::STRING, msg, "an error message"))
            }
            (Message::Transparent, Some([f])) => {
                let node = v.members[0].node;
                let t = own(f.ty);
                let r = self.read(ck, p, vi, 0, t);
                let display = names.known.display;
                Ok(ck.trait_call(display, "to_string", r, t, &[], node)?.0)
            }
            _ => {
                let name = self
                    .variants
                    .get(vi)
                    .map_or("", |(s, _)| names.text(*s))
                    .to_owned();
                Ok(ck.b.const_str(&name))
            }
        }
    }

    /// The cause of variant `vi`, of type `ret` (`dyn Error?`)
    /// (`annot.error.cause.*`, `annot.error.transparent.cause`).
    fn cause_arm(
        &self,
        ck: &mut Ck<'_, '_>,
        p: (LocalId, Ty),
        vi: usize,
        ret: Ty,
        own: &dyn Fn(Ty) -> Ty,
    ) -> StageResult<Ref> {
        let v = &self.shape.variants[vi];
        let no_cause = |ck: &mut Ck<'_, '_>, at: NodeRef<'_>| {
            let rec = ck.b.refs_record(&[]);
            ck.b.emit(Tag::NewVariant, 0, rec, ret, at.index())
        };
        let Some(fs) = self.members(vi) else {
            return Ok(no_cause(ck, v.node));
        };
        let pool = ck.cx.names.pool;
        let TyData::Option(erased) = pool.get(ret) else {
            return unsupported("an `Error.cause` that does not return `dyn Error?`");
        };
        if let (Message::Transparent, [f]) = (v.message, fs) {
            let node = v.members[0].node;
            let t = own(f.ty);
            let r = self.read(ck, p, vi, 0, t);
            let error = ck.cx.names.known.error;
            let (c, ct) = ck.trait_call(error, "cause", r, t, &[], node)?;
            return Ok(ck.coerce(c, ct, ret, node, "a cause"));
        }
        let Some(c) = v.cause() else {
            return Ok(no_cause(ck, v.node));
        };
        let node = v.members[c].node;
        let t = own(fs[c].ty);
        // `annot.error.cause.type`: the cause member's type implements
        // `Error`, an unmet bound like any other; with the bound unmet
        // the member has no conversion to check.
        let cause_ty = match pool.get(t) {
            TyData::Option(inner) => inner,
            _ => t,
        };
        let before = ck.diags.len();
        let tref = TraitRef {
            trait_: ck.cx.names.known.error,
            self_ty: cause_ty,
            args: TyList::EMPTY,
        };
        ck.require_ref(tref, node)?;
        if ck.diags.len() > before {
            return Ok(no_cause(ck, node));
        }
        let r = self.read(ck, p, vi, c, t);
        // An optional `@source` gives a cause only when present
        // (`annot.error.cause.optional`).
        if let TyData::Option(inner) = pool.get(t) {
            let tb = ck.b.open_block();
            let u = ck.b.emit(Tag::Unwrap, r.0, NONE, inner, node.index());
            let d = ck.coerce(u, inner, erased, node, "a cause");
            let s = ck.coerce(d, erased, ret, node, "a cause");
            let then = ck.b.close_block(tb, Some(s), ret, node.index());
            let eb = ck.b.open_block();
            let n = no_cause(ck, node);
            let other = ck.b.close_block(eb, Some(n), ret, node.index());
            let rec = ck.b.refs_record(&[Ref(1), then, other]);
            return Ok(ck.b.emit(Tag::SwitchTag, r.0, rec, ret, node.index()));
        }
        let d = ck.coerce(r, t, erased, node, "a cause");
        Ok(ck.coerce(d, erased, ret, node, "a cause"))
    }
}
