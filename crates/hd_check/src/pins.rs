//! Element references and pins (spec/lang/13-values-and-references.md,
//! "Element References"; `OBJECT_MODEL_TIER1.md` §6.3 and §6.4).
//!
//! `&xs[i]` and `&mut xs[i]` name a slot of the list's buffer, as Go's
//! `&s[i]` does. Taking one pins the list (`ListPin`): its buffer is made
//! unique, a length change panics `collection-pinned` while the pin is
//! held, and a copy of the list copies the buffer at once. `for p in &xs`
//! and `for p in &mut xs` bind an element reference per step and pin the
//! list for the whole loop.
//!
//! A pin is released after the reference's last use (`ListUnpin`), which
//! the checker finds at statement granularity in the suite that took it:
//! - a reference that a simple binding (`r := &mut xs[i]`, `let r = ...`)
//!   is initialized with lasts until the last statement of the suite that
//!   names the binding, at any depth, closures and interpolations
//!   included;
//! - any other reference, such as an argument or a loop's, lasts until
//!   the end of its statement;
//! - a pin is never held past the suite: an exit (`return`, `break`,
//!   `continue`, `?`) releases it through a `defer` suite of the suite's
//!   `Scope`, guarded by a flag that the release on the normal path
//!   clears.
//!
//! A `&self` or `&mut self` call on a list element (`xs[i].m()`) takes an
//! element reference as its receiver; that pin belongs to the call, not to
//! a suite, and is released right after the call (`release_call_pins`).
//!
//! Where no suite of the current body or closure holds the reference (an
//! expression-bodied closure, a top-level binding, a default), the pin is
//! released at once. A reference copied out of its binding (stored in a
//! field or a list, returned, captured by a closure that outlives the
//! statement) is released with the binding's last use on this backend
//! (`crates/hd_wasm_linear/DESIGN.md` §15.2, "Element references").
//!
//! All of this is the old path's. On the ownership passes' path
//! (`BodyCx::own_passes`; `docs/compiler/tir-v2.md`, stage 2 commit 12b)
//! an element reference is a `Ref` of the element place and nothing else:
//! the passes make the list unique and pin it before the reference, and
//! the reference's drop at its last use ends the pin.

use hd_base::{LocalId, NodeIdx, StageResult, Symbol};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{IntrinsicOp, NONE, Ref, Tag, TirSink, local_flags};
use hd_types::{Prim, Ty, TyData};

use crate::body::Ck;
use crate::expr::Indexed;
use crate::refs::{Place, Root};

/// A suite whose statements may take pins: the block, the open closures
/// when it began, and the statement being checked.
pub(crate) struct PinFrame {
    pub(crate) block: NodeIdx,
    pub(crate) subs: usize,
    pub(crate) stmt: usize,
}

/// A pin not yet released: its frame, the local holding the pinned list,
/// the flag saying the pin is held, the binding it belongs to, the
/// statement that took it, and the last statement that names its binding.
pub(crate) struct PinSlot {
    frame: usize,
    buf: LocalId,
    flag: LocalId,
    owner: Option<Symbol>,
    stmt: usize,
    until: Option<usize>,
}

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

/// Whether `n` is a reference expression that takes a pin: an element
/// reference (`&xs[i]`, `&mut xs[i].pos`), or the source of a loop over
/// element references (`for p in &mut xs`).
fn takes_pin(n: NodeRef<'_>) -> bool {
    if matches!(n.kind(), SyntaxKind::ForExpr | SyntaxKind::ComprehensionFor) {
        return n
            .children()
            .nth(1)
            .is_some_and(|src| strip_parens(src).kind() == SyntaxKind::RefExpr);
    }
    if n.kind() != SyntaxKind::RefExpr {
        return false;
    }
    let mut e = n.children().next();
    while let Some(x) = e {
        match x.kind() {
            SyntaxKind::ParenExpr | SyntaxKind::FieldExpr => e = x.children().next(),
            SyntaxKind::IndexExpr => return true,
            _ => return false,
        }
    }
    false
}

/// Whether `n` holds, below it and outside any nested suite, an
/// expression that takes a pin.
fn holds_pin(n: NodeRef<'_>) -> bool {
    n.children()
        .any(|c| c.kind() != SyntaxKind::Block && (takes_pin(c) || holds_pin(c)))
}

impl Ck<'_, '_> {
    /// Whether the statements of the suite `block` may take a pin, which
    /// makes it a cleanup scope that releases its pins on every exit.
    pub(crate) fn suite_takes_pins(block: NodeRef<'_>) -> bool {
        block.children().any(|s| takes_pin(s) || holds_pin(s))
    }

    /// Opens the pin frame of the suite `block`.
    pub(crate) fn open_pin_frame(&mut self, block: NodeRef<'_>) {
        self.pin_frames.push(PinFrame {
            block: block.index(),
            subs: self.subs.len(),
            stmt: 0,
        });
    }

    /// Closes the pin frame of the suite `block`: the pins still held were
    /// released on every path already, by their statements or the scope's
    /// exits.
    pub(crate) fn close_pin_frame(&mut self) {
        let depth = self.pin_frames.len();
        self.pins.retain(|p| p.frame + 1 < depth);
        self.pin_frames.pop();
    }

    /// The index of the pin frame of the suite `block`, when it is the
    /// innermost one.
    fn frame_of(&self, block: NodeRef<'_>) -> Option<usize> {
        let k = self.pin_frames.len().checked_sub(1)?;
        (self.pin_frames[k].block == block.index()).then_some(k)
    }

    /// Before statement `i` of the suite `block`.
    pub(crate) fn pins_before(&mut self, block: NodeRef<'_>, i: usize) {
        if let Some(k) = self.frame_of(block) {
            self.pin_frames[k].stmt = i;
        }
    }

    /// After statement `i` of the suite `block`, of the statements
    /// `stmts`: releases the pins whose last use it was. A pin taken in it
    /// for a binding lasts until the last later statement that names the
    /// binding (`ref.elem.release`). A statement that cannot complete left
    /// through an exit, which released its pins.
    pub(crate) fn pins_after(
        &mut self,
        block: NodeRef<'_>,
        i: usize,
        stmts: &[NodeRef<'_>],
        diverged: bool,
    ) {
        let Some(k) = self.frame_of(block) else {
            return;
        };
        for p in 0..self.pins.len() {
            let slot = &self.pins[p];
            if slot.frame != k || slot.stmt != i || slot.until.is_some() {
                continue;
            }
            let last = slot.owner.and_then(|s| {
                stmts
                    .iter()
                    .enumerate()
                    .skip(i + 1)
                    .rev()
                    .find(|(_, st)| self.mentions(**st, s))
                    .map(|(j, _)| j)
            });
            self.pins[p].until = Some(last.unwrap_or(i));
        }
        let at = stmts.get(i).map_or(block.index(), |s| s.index());
        let mut p = 0;
        while p < self.pins.len() {
            if self.pins[p].frame == k && self.pins[p].until == Some(i) {
                let slot = self.pins.remove(p);
                if !diverged {
                    self.emit_unpin(slot.buf, slot.flag, at);
                }
            } else {
                p += 1;
            }
        }
    }

    /// Whether the syntax `n` names `s` anywhere below it.
    fn mentions(&self, n: NodeRef<'_>, s: Symbol) -> bool {
        (n.kind() == SyntaxKind::NameExpr && self.sym_of(n) == s)
            || n.children().any(|c| self.mentions(c, s))
    }

    /// Releases the pin held in `buf` when `flag` says it is held, and
    /// clears `flag`.
    fn emit_unpin(&mut self, buf: LocalId, flag: LocalId, at: NodeIdx) {
        let held = self.b.get(flag, Ty::BOOL, at);
        let tb = self.b.open_block();
        let lt = self.b.local_ty(buf);
        let l = self.b.get(buf, lt, at);
        let rec = self.b.refs_record(&[l]);
        self.b.emit(
            Tag::Intrinsic,
            IntrinsicOp::ListUnpin as u32,
            rec,
            Ty::VOID,
            at,
        );
        let no = self.b.const_value(Ty::BOOL, 0);
        self.b.set(flag, no, at);
        let then = self.b.close_block(tb, None, Ty::VOID, at);
        let eb = self.b.open_block();
        let els = self.b.close_block(eb, None, Ty::VOID, at);
        let rec = self.b.refs_record(&[then, els]);
        self.b.emit(Tag::If, held.0, rec, Ty::VOID, at);
    }

    /// Pins the list `list` of value type `lt` for an element reference,
    /// checking `index` when given (`ref.elem.pin`): returns the local
    /// that holds the pinned list, with its scope depth. The pin belongs
    /// to the innermost suite of this body or closure, and to `owner`, the
    /// binding the reference initializes, when there is one.
    pub(crate) fn pin_list(
        &mut self,
        list: Ref,
        lt: Ty,
        index: Option<Ref>,
        owner: Option<Symbol>,
        n: NodeRef<'_>,
    ) -> (LocalId, usize) {
        let at = n.index();
        self.expose_list_path(list, at);
        let args: Vec<Ref> = std::iter::once(list).chain(index).collect();
        let rec = self.b.refs_record(&args);
        let pinned = self
            .b
            .emit(Tag::Intrinsic, IntrinsicOp::ListPin as u32, rec, lt, at);
        let sym = self.cx.names.syms.intern("$pinned");
        let buf = self.b.local(lt, sym, local_flags::ASSIGNED, at);
        self.b.set(buf, pinned, at);
        let depth = self.scopes.len() - 1;
        // A call's receiver: the call releases it (`release_call_pins`).
        if std::mem::take(&mut self.pin_for_call) {
            self.call_pins.push(buf);
            return (buf, depth);
        }
        let frame = self
            .pin_frames
            .len()
            .checked_sub(1)
            .filter(|k| self.pin_frames[*k].subs == self.subs.len());
        let Some(frame) = frame else {
            // No suite of this body holds the reference: released at once.
            let l = self.b.get(buf, lt, at);
            let rec = self.b.refs_record(&[l]);
            self.b.emit(
                Tag::Intrinsic,
                IntrinsicOp::ListUnpin as u32,
                rec,
                Ty::VOID,
                at,
            );
            return (buf, depth);
        };
        let sym = self.cx.names.syms.intern("$pin");
        let flag = self.b.local(Ty::BOOL, sym, local_flags::ASSIGNED, at);
        let yes = self.b.const_value(Ty::BOOL, 1);
        self.b.set(flag, yes, at);
        // Every exit from the suite releases a pin still held.
        let m = self.b.open_block();
        self.emit_unpin(buf, flag, at);
        let suite = self.b.close_block(m, None, Ty::VOID, at);
        self.b.defer(suite, at);
        let stmt = self.pin_frames[frame].stmt;
        self.pins.push(PinSlot {
            frame,
            buf,
            flag,
            owner,
            stmt,
            until: None,
        });
        (buf, depth)
    }

    /// Releases the pins that the receivers of calls took since there were
    /// `mark` of them, once the call is emitted
    /// (`fn.method.auto-address.element`).
    pub(crate) fn release_call_pins(&mut self, mark: usize, n: NodeRef<'_>) {
        let at = n.index();
        while self.call_pins.len() > mark {
            let Some(buf) = self.call_pins.pop() else {
                break;
            };
            let lt = self.b.local_ty(buf);
            let l = self.b.get(buf, lt, at);
            let rec = self.b.refs_record(&[l]);
            self.b.emit(
                Tag::Intrinsic,
                IntrinsicOp::ListUnpin as u32,
                rec,
                Ty::VOID,
                at,
            );
        }
    }

    /// The place `&e[k]` names when `e` is a list (`ref.elem.place`): the
    /// slot `k` of the list's buffer, which is pinned. `None` after an
    /// error; a map value or any other index is no place to refer to.
    pub(crate) fn elem_operand(
        &mut self,
        e: NodeRef<'_>,
        owner: Option<Symbol>,
    ) -> StageResult<Option<crate::refs::Operand>> {
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        let [base, key] = kids.as_slice() else {
            return crate::body::unsupported("an index shape");
        };
        // The base and the key may take pins of their own first.
        let for_call = std::mem::take(&mut self.pin_for_call);
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
                let pinned = if self.cx.own_passes {
                    // Insertion pins the list before the reference
                    // (`docs/compiler/tir-v2.md`, stage 2 commit 12b); a
                    // map value on the path is still marked.
                    self.mark_write_path(br);
                    None
                } else {
                    self.pin_for_call = for_call;
                    Some(self.pin_list(br, lt, Some(kr), owner, e))
                };
                Ok(Some(crate::refs::Operand::Place(Place {
                    root: Root::Elem(br, pinned, kr, lt),
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

    /// Detach shared ancestors before their element headers are read, then
    /// expose each ancestor without keeping its buffer pinned. The leaf's
    /// reference may escape this scope; exposure therefore belongs to the
    /// header rather than to a lexical cleanup. Reuse evaluated operands so
    /// no index or receiver is evaluated twice.
    fn expose_list_path(&mut self, r: Ref, at: NodeIdx) {
        self.mark_write_path(r);
        let mut cur = r;
        let mut ancestors = Vec::new();
        while let Some(i) = cur.as_inst() {
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            if let Some(crate::access::Projection::Field(base)) = self.projection(cur) {
                cur = base;
                continue;
            }
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::Intrinsic
                    if matches!(
                        IntrinsicOp::from_u32(data[0]),
                        Some(IntrinsicOp::ListIndexMut)
                    ) =>
                {
                    let args = self.b.body_mut().record(data[1]);
                    let (base, index) = (Ref(args[0]), Ref(args[1]));
                    ancestors.push((base, index));
                    cur = base;
                }
                _ => break,
            }
        }
        for (base, index) in ancestors.into_iter().rev() {
            let ty = self.b.ty_of(base);
            let args = self.b.refs_record(&[base, index]);
            let pinned = self
                .b
                .emit(Tag::Intrinsic, IntrinsicOp::ListPin as u32, args, ty, at);
            let args = self.b.refs_record(&[pinned]);
            self.b.emit(
                Tag::Intrinsic,
                IntrinsicOp::ListUnpin as u32,
                args,
                Ty::VOID,
                at,
            );
        }
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
    /// reference per step and pins the list while it runs. `None` when
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
        // On the ownership passes' path each step's reference pins the list
        // (insertion's rule 6), from the step's start to its end, where the
        // reference's binding is dropped: the loop holds no pin of its own.
        let pinned = if self.cx.own_passes {
            self.mark_write_path(lr);
            None
        } else {
            Some(self.pin_list(lr, lt, None, None, src))
        };
        let len_l = self
            .b
            .local(us, self.cx.names.syms.intern("$len"), ASSIGNED, at);
        let pos_l = self
            .b
            .local(us, self.cx.names.syms.intern("$pos"), ASSIGNED, at);
        let c = match pinned {
            Some((buf, _)) => self.b.get(buf, lt, at),
            None => lr,
        };
        let rec = self.b.refs_record(&[c]);
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
            root: Root::Elem(lr, pinned, index, lt),
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
