//! The body the compiler supplies for `std.ownership.drop[T]`
//! (spec/lang/13-ownership.md, Drops): the drop glue of `T`. Collection asks
//! for it at a concrete type, where the declaration is known, and every drop
//! of a value of that type, implicit or written, calls it.
//!
//! The body runs the type's `drop` when it implements `DropOwn`, then drops
//! each part in declaration order: a data type's fields, the payloads of the
//! variant an enum value holds, an optional's payload, a tuple's elements.
//! A part drops as a statement-end temporary (`DropTemp`), so collection
//! decides which parts have glue: a part without any drops nothing, and a
//! part with some calls its own type's `drop` body, so a recursive type
//! recurses through its own instance.

use hd_base::LocalId;
use hd_base::{DefId, StageResult};
use hd_resolve::ItemData;
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, ChoiceKind, Coercion, IntrinsicOp, NONE, PrimOp, Providers, Ref, Tag,
    TirBuilder, TirSink,
};
use hd_types::{ParamRef, Prim, Ty, TyData, TyList};

use super::{Cx, Gen, Handles, SYN};

/// The `DropOwn` implementation a type's glue calls first: the trait, its
/// `drop` method, and the implementation collection selected.
#[derive(Clone, Copy, Debug)]
pub struct DropImpl {
    pub trait_: DefId,
    pub method: DefId,
    pub impl_: DefId,
}

/// The body of `drop[ty]`. `drop_impl`: `ty`'s `DropOwn` implementation.
pub fn drop_body(
    cx: &Cx<'_, '_>,
    item: DefId,
    ty: Ty,
    drop_impl: Option<DropImpl>,
) -> StageResult<Body> {
    let names = cx.names;
    let mut g = Gen::new(cx, ty, item, Handles::Inline);
    g.b = TirBuilder::new(item, BodyKind::Fn);
    let value = g.b.local(
        ty,
        names.syms.intern("value"),
        hd_tir::ir::local_flags::PARAM,
        SYN,
    );
    let blk = g.b.open_block();
    let v = g.b.get(value, ty, SYN);
    let core = g.core(ty);
    if let Some(d) = drop_impl {
        let pool = names.pool;
        let lent_ty = pool.intern_ty(&TyData::Borrow {
            ty: core,
            mutable: true,
            lifetimes: TyList::EMPTY,
        });
        let lent = g.b.coerce(Coercion::LendMut, NONE, v, lent_ty, SYN);
        g.b.call(
            &Callee::TraitMethod {
                trait_: d.trait_,
                method: d.method,
                self_ty: core,
                targs: TyList::EMPTY,
                choice: (ChoiceKind::Impl, d.impl_.raw()),
            },
            &[lent],
            Providers::None,
            Ty::VOID,
            SYN,
        );
    }
    g.drop_parts((value, ty), v, core)?;
    let root = g.b.close_block(blk, None, Ty::VOID, SYN);
    g.finish(root).map(|(body, _)| body)
}

impl Gen<'_, '_> {
    /// `t` without its outer modes: the type whose declaration says what
    /// its value holds.
    fn core(&self, t: Ty) -> Ty {
        let pool = self.names.pool;
        let mut t = t;
        while let TyData::Mut(i) = pool.get(t) {
            t = i;
        }
        t
    }

    /// Whether a part of type `t` may hold an owned value; one that cannot
    /// is not read at all.
    fn may_own(&self, t: Ty) -> bool {
        !matches!(
            self.names.pool.get(t),
            TyData::Prim(_) | TyData::Never | TyData::Borrow { .. } | TyData::Fn { .. }
        )
    }

    /// Drops part `p`, of type `t`, as a temporary.
    fn drop_part(&mut self, p: Ref) {
        self.b.drop_temp(p, SYN);
    }

    /// Drops the parts of `v`, the value of local `value`, whose type
    /// without modes is `core`.
    fn drop_parts(&mut self, value: (LocalId, Ty), v: Ref, core: Ty) -> StageResult<()> {
        let names = self.names;
        let pool = names.pool;
        match pool.get(core) {
            TyData::Option(inner) => {
                if !self.may_own(inner) {
                    return Ok(());
                }
                let tb = self.b.open_block();
                let p = self.b.emit(Tag::Unwrap, v.0, NONE, inner, SYN);
                self.drop_part(p);
                let then = self.b.close_block(tb, None, Ty::VOID, SYN);
                let eb = self.b.open_block();
                let other = self.b.close_block(eb, None, Ty::VOID, SYN);
                // `.Some` is tag 1 of an optional.
                self.switch_tag(v, 1, then, other, Ty::VOID);
            }
            TyData::Tuple { elems, rest } => {
                let parts: Vec<Ty> = pool.list_items(elems).iter().copied().chain(rest).collect();
                for (k, e) in parts.into_iter().enumerate() {
                    if self.may_own(e) {
                        let k = u32::try_from(k).unwrap_or(0);
                        let p = self.b.emit(Tag::TupleGet, v.0, k, e, SYN);
                        self.drop_part(p);
                    }
                }
            }
            TyData::Adt { def, args } => {
                let known = names.known;
                let argv: Vec<Ty> = pool.list_items(args).to_vec();
                if def == known.list || def == known.map {
                    self.drop_elements(value, def == known.map, &argv);
                    return Ok(());
                }
                let sub = |t: Ty| {
                    pool.subst(t, &|p: ParamRef| {
                        (p.owner == def).then(|| argv.get(usize::from(p.index)).copied())?
                    })
                };
                let Some(item) = self.cx.items.item(def) else {
                    return self
                        .cx
                        .unsupported("drop glue of a type without its declaration");
                };
                match &item.data {
                    ItemData::Data(fields) => {
                        for (k, f) in fields.iter().enumerate() {
                            let t = sub(f.ty);
                            if self.may_own(t) {
                                let k = u32::try_from(k).unwrap_or(0);
                                let p = self.b.emit(Tag::Field, v.0, k, t, SYN);
                                self.drop_part(p);
                            }
                        }
                    }
                    // An erased newtype is its base value.
                    ItemData::Newtype(base) => {
                        let t = sub(*base);
                        if self.may_own(t) {
                            let p = self.b.emit(Tag::Field, v.0, 0, t, SYN);
                            self.drop_part(p);
                        }
                    }
                    ItemData::Enum { variants, .. } => {
                        let cases: Vec<(u32, Vec<(u32, Ty)>)> = variants
                            .iter()
                            .enumerate()
                            .map(|(k, vr)| {
                                let parts = vr
                                    .fields
                                    .iter()
                                    .enumerate()
                                    .map(|(i, f)| (u32::try_from(i).unwrap_or(0), sub(f.ty)))
                                    .filter(|(_, t)| self.may_own(*t))
                                    .collect::<Vec<_>>();
                                (u32::try_from(k).unwrap_or(0), parts)
                            })
                            .filter(|(_, parts)| !parts.is_empty())
                            .collect();
                        self.drop_variants(v, &cases);
                    }
                    _ => {
                        return self.cx.unsupported("drop glue of this kind of declaration");
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A list's elements, or a map's values, in iteration order: `pos`
    /// walks the list's indices or the map's live slots, as a `for` loop
    /// does, and each part drops as a temporary.
    fn drop_elements(&mut self, value: (LocalId, Ty), map: bool, args: &[Ty]) {
        let names = self.names;
        let pool = names.pool;
        let us = Ty::prim(Prim::Usize);
        let item = if map {
            pool.intern_ty(&TyData::Tuple {
                elems: pool.list(args),
                rest: None,
            })
        } else {
            args.first().copied().unwrap_or(Ty::VOID)
        };
        let intr = |g: &mut Self, op: IntrinsicOp, refs: &[Ref], t: Ty| {
            let rec = g.b.refs_record(refs);
            g.b.emit(Tag::Intrinsic, op as u32, rec, t, SYN)
        };
        let flags = hd_tir::ir::local_flags::ASSIGNED;
        let pos = self.b.local(us, names.syms.intern("$pos"), flags, SYN);
        let end = self.b.local(us, names.syms.intern("$end"), flags, SYN);
        let zero = self.b.const_value(us, 0);
        self.b.set(pos, zero, SYN);
        let c = self.b.get(value.0, value.1, SYN);
        let (len_op, read_op) = if map {
            (IntrinsicOp::MapSlots, IntrinsicOp::MapSlotEntry)
        } else {
            (IntrinsicOp::ListLen, IntrinsicOp::ListIndex)
        };
        let n = intr(self, len_op, &[c], us);
        self.b.set(end, n, SYN);
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        if map {
            let (c, p, d) = (
                self.b.get(value.0, value.1, SYN),
                self.b.get(pos, us, SYN),
                self.b.get(end, us, SYN),
            );
            let next = intr(self, IntrinsicOp::MapSlotNext, &[c, p, d], us);
            self.b.set(pos, next, SYN);
        }
        let (p, d) = (self.b.get(pos, us, SYN), self.b.get(end, us, SYN));
        let go = self.b.prim(PrimOp::Lt as u32, &[p, d], Ty::BOOL, SYN);
        let tb = self.b.open_block();
        let (c, p) = (self.b.get(value.0, value.1, SYN), self.b.get(pos, us, SYN));
        let read = intr(self, read_op, &[c, p], item);
        let part = if map {
            let v = args.get(1).copied().unwrap_or(Ty::VOID);
            self.b.emit(Tag::TupleGet, read.0, 1, v, SYN)
        } else {
            read
        };
        self.drop_part(part);
        let p = self.b.get(pos, us, SYN);
        let one = self.b.const_value(us, 1);
        let next = self.b.prim(PrimOp::Add as u32, &[p, one], us, SYN);
        self.b.set(pos, next, SYN);
        let then = self.b.close_block(tb, None, Ty::VOID, SYN);
        let eb = self.b.open_block();
        self.b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, SYN);
        let els = self.b.close_block(eb, None, Ty::VOID, SYN);
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, go.0, rec, Ty::VOID, SYN);
        let body = self.b.close_block(lb, None, Ty::VOID, SYN);
        self.b.close_loop(lp, body, Ty::VOID, SYN);
    }

    /// `SwitchTag` over the variants that hold parts, each dropping its
    /// payloads in declaration order; any other variant drops nothing.
    fn drop_variants(&mut self, v: Ref, cases: &[(u32, Vec<(u32, Ty)>)]) {
        let Some(((index, parts), rest)) = cases.split_first() else {
            return;
        };
        let tb = self.b.open_block();
        for (field, t) in parts {
            let rec = self.b.refs_record(&[Ref(*index), Ref(*field)]);
            let p = self.b.emit(Tag::Payload, v.0, rec, *t, SYN);
            self.drop_part(p);
        }
        let then = self.b.close_block(tb, None, Ty::VOID, SYN);
        let eb = self.b.open_block();
        self.drop_variants(v, rest);
        let other = self.b.close_block(eb, None, Ty::VOID, SYN);
        self.switch_tag(v, *index, then, other, Ty::VOID);
    }
}
