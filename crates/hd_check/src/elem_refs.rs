//! Element references (spec/lang/13-values-and-references.md, "Element
//! References"; `OBJECT_MODEL_TIER1.md` §6.3 and §6.4).
//!
//! `&xs[i]` and `&mut xs[i]` name a slot of the list's buffer, as Go's
//! `&s[i]` does. `for p in &xs` and `for p in &mut xs` bind an element
//! reference per step. An element reference is a `Ref` of the element
//! place and nothing else: the ownership passes make the list unique and
//! pin it before the reference, and the reference's drop at its last use
//! ends the pin (`docs/compiler/tir-v2.md`, stage 2 commit 12b; a pinned
//! list panics `collection-pinned` on a length change).

use hd_base::{NodeIdx, StageResult};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{IntrinsicOp, NONE, Ref, Tag, TirSink};
use hd_types::{Prim, Ty, TyData};

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

/// What uses an expression where an element reference may stand.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElemUse {
    /// A call argument (`ref.elem.call`).
    Arg,
    /// A method call's receiver (`ref.elem.call`).
    Receiver,
    /// A dereference's operand or a field's base, which reads or writes
    /// through it at once: a loop binding's use (`flow.for.ref.binding`),
    /// never `&xs[i]` itself, whose place is written `xs[i]`.
    Through,
}

impl Ck<'_, '_> {
    /// The place `&e[k]` names when `e` is a list (`ref.elem.place`): the
    /// slot `k` of the list's buffer. `None` after an error; a map value or
    /// any other index is no place to refer to.
    pub(crate) fn elem_operand(
        &mut self,
        e: NodeRef<'_>,
    ) -> StageResult<Option<crate::refs::Operand>> {
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        let [base, key] = kids.as_slice() else {
            return crate::body::unsupported("an index shape");
        };
        let (br, bt) = self.expr(*base, None)?;
        if matches!(self.infer.resolve(self.pool(), bt), Ty::POISON | Ty::NEVER) {
            return Ok(None);
        }
        let (br, bt) = self.deref_index_base(br, bt, base.index());
        let builtin = self.index_kind(bt);
        match builtin {
            // `types.coll.map-value.ref`: a map value moves when the map
            // rehashes, so it is no place to refer to.
            Some((Indexed::Map, ..)) => {
                self.err(
                    Code::MapValueNotAddressable,
                    e,
                    "a map value is no place, so no reference refers to it; copy it into a `let mut` and refer to that, or keep `&mut` values in the map",
                );
                self.map_value_ref_fix(e);
                Ok(None)
            }
            Some((Indexed::List, kt, vt)) => {
                let (kr, ktt) = self.index_key_value(*key, builtin)?;
                if self.is_range_ty(ktt) {
                    self.not_yet_index(e);
                    return Ok(None);
                }
                let kr = self.index_key((kr, ktt), *key, kt);
                let lt = self.strip_mut(self.infer.resolve(self.pool(), bt));
                // Insertion pins the list before the reference; a map value
                // on the path is still marked.
                self.mark_write_path(br);
                Ok(Some(crate::refs::Operand::Place(Place {
                    root: Root::Elem(br, kr, lt),
                    path: vec![],
                    ty: vt,
                })))
            }
            _ => {
                self.not_yet_index(e);
                Ok(None)
            }
        }
    }

    /// `e` is used as `how`, where an element reference may stand.
    pub(crate) fn elem_may_stand(&mut self, e: NodeRef<'_>, how: ElemUse) {
        self.elem_ok.insert(strip_parens(e).index(), how);
    }

    /// `n` evaluates to an element reference: `&xs[i]`, or a name of an
    /// element reference loop's binding (`binding`). `&xs[i]` may stand
    /// only as an argument or a receiver (`ref.elem.call`); the binding
    /// also as a dereference's operand or a field's base
    /// (`flow.for.ref.binding`). Anywhere else it outlives its call
    /// (`ref.elem.kept`).
    pub(crate) fn elem_ref_value(&mut self, n: NodeRef<'_>, binding: bool) {
        match self.elem_ok.get(&n.index()) {
            Some(ElemUse::Arg) => {
                self.elem_args.insert(n.index());
            }
            Some(ElemUse::Receiver) => {}
            Some(ElemUse::Through) if binding => {}
            _ => self.elem_ref_kept(n),
        }
    }

    /// The argument `e`, when it is an element reference, passed for a
    /// parameter whose declared type is no reference: the callee stores
    /// a value, which is keeping it (`ref.elem.kept`). A reference
    /// parameter may keep a copy (`ref.elem.callee-copy`).
    pub(crate) fn elem_ref_arg(&mut self, e: NodeRef<'_>, ref_param: bool) {
        if !ref_param && self.elem_args.contains(&strip_parens(e).index()) {
            self.elem_ref_kept(e);
        }
    }

    /// A closure that captures an element reference loop's binding keeps
    /// it (`flow.for.ref.binding`). Reported once, by the outermost
    /// closure that captures it.
    pub(crate) fn elem_ref_captures(&mut self, mark: hd_tir::ir::SubMark, n: NodeRef<'_>) {
        for l in self.b.open_captures(mark) {
            if !self.elem_locals.contains(&l) {
                continue;
            }
            let depth = self
                .scopes
                .iter()
                .position(|m| m.values().any(|x| *x == l))
                .unwrap_or(0);
            if self.subs.iter().any(|s| s.depth > depth) {
                continue;
            }
            self.elem_ref_kept(n);
        }
    }

    fn elem_ref_kept(&mut self, n: NodeRef<'_>) {
        self.err(
            Code::ElementReferenceOutlivesCall,
            n,
            "an element reference lives for one call, so nothing may keep it; keep the index and name the element as `xs[i]` at each use",
        );
    }

    /// `ref.check.not-yet-non-list-elements`: a reference to an index result that is
    /// no list element, such as a slice or a user `Index` call.
    fn not_yet_index(&mut self, e: NodeRef<'_>) {
        self.err(
            Code::ReferencesNotYetChecked,
            e,
            "a reference to this index result is not checked yet; only a list element is a place",
        );
    }

    /// `for p in &xs` and `for p in &mut xs` over a list (`flow.for.ref`):
    /// a counted loop over the list's slots that binds an element
    /// reference per step. `None` when
    /// the operand is no list or map, whose loop is the ordinary one.
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
        // The operand is evaluated once; a value of another type is
        // evaluated again by the ordinary loop, so this one is undone.
        let cp = self.checkpoint();
        let (lr, lt) = self.expr(operand, None)?;
        // A reference to a list is read through (`flow.for.ref.through`).
        let (lr, lt) = self.deref_index_base(lr, lt, operand.index());
        let builtin = self.index_kind(lt);
        let vt = match builtin {
            Some((Indexed::List, _, vt)) => vt,
            Some((Indexed::Map, ..)) => {
                self.commit(cp);
                self.err(
                    Code::MapValueNotAddressable,
                    src,
                    "a map value is no place, so a loop gives no references to a map's values; loop over the map itself and store changed values with `m[k] = v`",
                );
                let p = self
                    .b
                    .emit(Tag::Poison, NONE, NONE, Ty::POISON, src.index());
                return Ok(Some((p, Ty::VOID)));
            }
            _ => {
                self.rollback(cp);
                return Ok(None);
            }
        };
        self.commit(cp);
        let at = e.index();
        let us = Ty::prim(Prim::Usize);
        let lt = self.strip_mut(self.infer.resolve(self.pool(), lt));
        let result = loop_else.map(|(_, t)| t);
        // Each step's reference pins the list (insertion's rule 6), from
        // the step's start to its end, where the reference's binding is
        // dropped: the loop holds no pin of its own.
        self.mark_write_path(lr);
        let len_l = self
            .b
            .local(us, self.cx.names.syms.intern("$len"), ASSIGNED, at);
        let pos_l = self
            .b
            .local(us, self.cx.names.syms.intern("$pos"), ASSIGNED, at);
        let rec = self.b.refs_record(&[lr]);
        let n = self
            .b
            .emit(Tag::Intrinsic, IntrinsicOp::ListLen as u32, rec, us, at);
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
        let place = Place {
            root: Root::Elem(lr, index, lt),
            path: vec![],
            ty: vt,
        };
        let ok = !mutable || self.place_writable(&place, src);
        let inner = self.strip_mut(self.infer.resolve(self.pool(), vt));
        let rt = self.pool().intern_ty(&TyData::Ref { inner, mutable });
        let item = if ok {
            self.make_ref(&place, mutable, rt, src)
        } else {
            self.b.emit(Tag::Poison, NONE, NONE, rt, src.index())
        };
        let mut binds = Vec::new();
        let before = self.diags.len();
        self.declare_pattern(pat, rt, &mut binds)?;
        let named: Vec<_> = binds
            .iter()
            .map(|b| b.1)
            .filter(|&l| self.b.local_ty(l) == rt)
            .collect();
        self.elem_locals.extend(named);
        if self.diags.len() == before {
            self.irrefutable_loop_pattern(pat, rt);
        }
        let ab = self.b.open_block();
        body(self)?;
        let arm = self.b.close_block(ab, None, Ty::VOID, at);
        let rows = vec![crate::pat::Row {
            pat: Some(pat),
            wrap_some: false,
            binds,
            guard: None,
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
