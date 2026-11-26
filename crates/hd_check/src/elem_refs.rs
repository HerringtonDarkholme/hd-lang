//! Element references (spec/lang/13-values-and-references.md, "Element
//! References"; the tier-1 design `58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md` §6.3 and §6.4).
//!
//! `&xs[i]` and `&mut xs[i]` name a slot of the list's buffer, as Go's
//! `&s[i]` does, and `&m[k]` and `&mut m[k]` the value of a map's entry
//! (`ref.elem.map`), whose number `MapEntry` looks up just before the
//! reference. `for p in &xs` and `for p in &mut xs` bind an element
//! reference per step. An element reference is a `Ref` of the element
//! place and nothing else: the ownership passes make the collection unique
//! before the reference (`docs/compiler/tir-v2.md`, "Insertion" rule 6),
//! and the reference counts nothing. A change of the collection's length
//! while one lives is a compile-time conflict, or a pointer-loan one
//! behind a `*T` or a shared variable.

use hd_base::{NodeIdx, StageResult};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{NONE, Ref, Tag, TirSink};
use hd_types::{Prim, Ty};

use crate::body::Ck;
use crate::expr::Indexed;
use crate::refs::{Place, Root};

/// `e` without its parentheses.
pub(crate) fn strip_parens(mut e: NodeRef<'_>) -> NodeRef<'_> {
    while e.kind() == SyntaxKind::ParenExpr {
        let Some(inner) = e.children().next() else {
            break;
        };
        e = inner;
    }
    e
}

impl Ck<'_, '_> {
    /// The place `&e[k]` names when `e` is a list (`ref.elem.place`), the
    /// slot `k` of the list's buffer, or a map (`ref.elem.map`), the value
    /// of the key `k`. Other types lend their place through `Index` or
    /// `IndexMut`. `None` after an error.
    pub(crate) fn elem_operand(
        &mut self,
        e: NodeRef<'_>,
        mutable: bool,
    ) -> StageResult<Option<crate::refs::Operand>> {
        let [base, key] = e.parts();
        if self.trait_index_base(base) {
            return self.trait_index_place(base, key, e, mutable, None);
        }
        let (br, bt) = self.expr(base, None)?;
        if matches!(self.infer.resolve(self.pool(), bt), Ty::POISON | Ty::NEVER) {
            return Ok(None);
        }
        let (br, bt) = self.deref_index_base(br, bt, base.index());
        let builtin = self.index_kind(bt);
        let Some((Indexed::Map, kt, vt)) = builtin else {
            return self.trait_index_place(base, key, e, mutable, Some((br, bt)));
        };
        let (kr, ktt) = self.index_key_value(key, builtin)?;
        let kr = self.index_key((kr, ktt), key, kt);
        let ct = self.resolved(self.infer.resolve(self.pool(), bt));
        // Insertion pins the collection before the reference.
        Ok(Some(crate::refs::Operand::Place(Place {
            root: Root::Elem(br, kr, ct),
            path: vec![],
            ty: vt,
        })))
    }

    fn map_ref_loop(
        &mut self,
        pat: NodeRef<'_>,
        (src, operand, mutable): (NodeRef<'_>, NodeRef<'_>, bool),
        loop_else: Option<(NodeIdx, Ty)>,
        e: NodeRef<'_>,
        body: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<(Ref, Ty)> {
        use crate::refs::{Operand, PlaceUse};
        use hd_tir::ir::local_flags::CAPTURED_ASSIGNED;
        let (source, source_ty) = if mutable {
            let Some(op) = self.place_of(operand, None, PlaceUse::Ref)? else {
                let p = self
                    .b
                    .emit(Tag::Poison, NONE, NONE, Ty::POISON, src.index());
                return Ok((p, Ty::VOID));
            };
            let op = match op {
                Operand::Place(p) if self.ref_of(p.ty).is_some() => {
                    let (r, t) = self.read_place(&p, operand);
                    Operand::Value(r, t)
                }
                op => op,
            };
            let mut fresh = false;
            let place = match op {
                Operand::Place(p) => p,
                Operand::Value(r, t) => {
                    // ruled out: a deeper reference. ref_loop selects Map after
                    // deref_index_base, which strips a reference only when its
                    // immediate inner type is directly indexed. The selected
                    // operand is therefore Map or Ref(Map).
                    if let Some((inner, _)) = self.ref_of(t)
                        && matches!(self.index_kind(inner), Some((Indexed::Map, ..)))
                    {
                        Place {
                            root: Root::Deref(r, t),
                            path: vec![],
                            ty: self.deref_ty(inner),
                        }
                    } else {
                        fresh = true;
                        let (l, depth) = self.hold(r, t, operand);
                        self.add_flags(l, CAPTURED_ASSIGNED);
                        Place {
                            root: Root::Local(l, depth),
                            path: vec![],
                            ty: t,
                        }
                    }
                }
            };
            if !fresh && !self.place_writable(&place, operand) {
                let p = self
                    .b
                    .emit(Tag::Poison, NONE, NONE, Ty::POISON, src.index());
                return Ok((p, Ty::VOID));
            }
            let rt = self.ref_ty(self.resolved(place.ty), true, false);
            (self.make_ref(&place, true, rt, src), rt)
        } else {
            let (r, t) = self.expr(operand, None)?;
            let (r, t) = self.deref_index_base(r, t, operand.index());
            (r, t)
        };
        let mt = self.ref_of(source_ty).map_or(source_ty, |(inner, _)| inner);
        let Some((Indexed::Map, key, value)) = self.index_kind(mt) else {
            unreachable!("for_loop selected an Indexed::Map source");
        };
        let item = self.pool().product(
            self.pool().list(&[
                self.ref_ty(key, false, false),
                self.ref_ty(value, mutable, false),
            ]),
            None,
        );
        self.collection_loop(pat, (source, source_ty, item), loop_else, e, body)
    }

    /// `for p in &xs` and `for p in &mut xs` over a list (`flow.for.ref`):
    /// `n := xs.len(); for i in 0..n: p := &xs[i]` (`&mut xs[i]`), the
    /// element reference through `Index` (`IndexMut`), whose loan borrows
    /// the whole list (`ref.elem.access.whole`). `None` when the operand
    /// is no list or map, whose loop is the ordinary one.
    pub(crate) fn ref_loop(
        &mut self,
        pat: NodeRef<'_>,
        src: NodeRef<'_>,
        loop_else: Option<(NodeIdx, Ty)>,
        e: NodeRef<'_>,
        body: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<Option<(Ref, Ty)>> {
        use hd_tir::ir::local_flags::ASSIGNED;
        let Some(operand) = src.children().next() else {
            return Ok(None);
        };
        let mutable = src
            .direct_token(&self.cx.src.parse.tokens, hd_syntax::TokenKind::KwMut)
            .is_some();
        // The operand's type picks the loop; it is evaluated again below.
        let cp = self.checkpoint();
        let (lr, lt) = self.expr(operand, None)?;
        // A reference to a list is read through (`flow.for.ref.through`).
        let (_, mut lt) = self.deref_index_base(lr, lt, operand.index());
        while let Some((inner, _)) = self.ref_of(lt) {
            lt = inner;
        }
        let lt = self.resolved(self.infer.resolve(self.pool(), lt));
        let map = matches!(self.index_kind(lt), Some((Indexed::Map, ..)));
        let list = (self.pool().adt(lt)).is_some_and(|(d, _)| d == self.cx.names.known.list);
        self.rollback(cp);
        if map {
            return self
                .map_ref_loop(pat, (src, operand, mutable), loop_else, e, body)
                .map(Some);
        }
        if !list {
            return Ok(None);
        }
        let receiver = if mutable {
            self.store_receiver(operand, "index_mut")?
        } else if self
            .peek_place_ty(operand)
            .is_some_and(|t| self.ref_of(t).is_none())
        {
            Some(self.address_of_place(operand, "index", false, vec![])?)
        } else {
            let (r, t) = self.expr(operand, None)?;
            Some(self.index_receiver(r, t, operand))
        };
        let Some((lr, lt)) = receiver.filter(|(_, t)| *t != Ty::POISON) else {
            let p = self
                .b
                .emit(Tag::Poison, NONE, NONE, Ty::POISON, src.index());
            return Ok(Some((p, Ty::VOID)));
        };
        let at = e.index();
        let us = Ty::prim(Prim::Usize);
        let result = loop_else.map(|(_, t)| t);
        let len_l = self
            .b
            .local(us, self.cx.names.syms.intern("$len"), ASSIGNED, at);
        let pos_l = self
            .b
            .local(us, self.cx.names.syms.intern("$pos"), ASSIGNED, at);
        let args = crate::call::Args::empty();
        let (n, _) = self.method_on(lr, lt, "len", &args, (operand, false), Some(us))?;
        self.set_local(len_l, n, at);
        let zero = self.b.const_value(us, 0);
        self.set_local(pos_l, zero, at);
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        let (p, d2) = (self.b.get(pos_l, us, at), self.b.get(len_l, us, at));
        let go = self
            .b
            .prim(hd_tir::ir::PrimOp::Lt as u32, &[p, d2], Ty::BOOL, at);
        let tb = self.b.open_block();
        self.loops.push((lp, result));
        self.scopes.push(hd_base::HashMap::default());
        // The step's index, read before the position moves on.
        let index = self.b.get(pos_l, us, at);
        let p = self.b.get(pos_l, us, at);
        let one = self.b.const_value(us, 1);
        let next = self
            .b
            .prim(hd_tir::ir::PrimOp::Add as u32, &[p, one], us, at);
        self.set_local(pos_l, next, at);
        let (item, rt) = self.index_reference((lr, lt), (index, us), (src, e), mutable)?;
        let rt = self.resolved(self.infer.resolve(self.pool(), rt));
        let mut binds = Vec::new();
        let before = self.diags.len();
        self.declare_pattern(pat, rt, &mut binds)?;
        if self.diags.len() == before {
            self.irrefutable_loop_pattern(pat, rt)?;
        }
        let ab = self.b.open_block();
        body(self)?;
        let arm = self.b.close_block(ab, None, Ty::VOID, at);
        let rows = vec![crate::pat::Row {
            pat: Some(pat),
            wrap_some: false,
            binds,
            guard: None,
            twins: Vec::new(),
            arm: 0,
        }];
        let db = self.b.open_block();
        self.decide(&rows, item, rt)?;
        let dec = self.b.close_block(db, None, Ty::NEVER, at);
        let rec = self.b.refs_record(&[dec, arm]);
        self.b.emit(Tag::Match, item.0, rec, Ty::VOID, at);
        self.scopes.pop();
        self.loops.pop();
        let then = self.b.close_block(tb, None, Ty::VOID, at);
        let eb = self.b.open_block();
        self.loop_exit(lp, loop_else, at, true)?;
        let els = self.b.close_block(eb, None, Ty::VOID, at);
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, go.0, rec, Ty::VOID, at);
        let lbody = self.b.close_block(lb, None, Ty::VOID, at);
        let ty = result.unwrap_or(Ty::VOID);
        Ok(Some((self.b.close_loop(lp, lbody, ty, at), ty)))
    }
}
