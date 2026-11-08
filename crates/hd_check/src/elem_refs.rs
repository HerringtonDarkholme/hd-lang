//! Element references (spec/lang/13-values-and-references.md, "Element
//! References"; the tier-1 design `58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md` §6.3 and §6.4).
//!
//! `&xs[i]` and `&mut xs[i]` name a slot of the list's buffer, as Go's
//! `&s[i]` does, and `&m[k]` and `&mut m[k]` the value of a map's entry
//! (`ref.elem.map`), whose number `MapEntry` looks up just before the
//! reference. `for p in &xs` and `for p in &mut xs` bind an element
//! reference per step. An element reference is a `Ref` of the element
//! place and nothing else: the ownership passes make the collection unique
//! and pin it before the reference, and the reference's drop at its last
//! use ends the pin (`docs/compiler/tir-v2.md`, stage 2 commit 12b). The
//! pin only keeps the buffer the list's own while references count it; a
//! change of the collection's length while one lives is a compile-time
//! conflict, or a pointer-loan one behind a `*T` or a shared variable.

use hd_base::{HashSet, LocalId, NodeIdx, StageResult, Symbol};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{IntrinsicOp, NONE, Ref, Tag, TirSink};
use hd_types::{Prim, Ty};

use crate::body::Ck;
use crate::expr::Indexed;
use crate::refs::{Place, Root};
use hd_tir::ir::{Step as ReadStep, local_flags};

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
    /// A call argument, whose direct element access participates in the
    /// call's static conflict check (`ref.elem.conflict`).
    Arg,
    /// A method call's receiver, for the same check.
    Receiver,
    /// A dereference's operand or a field's base, which reads or writes
    /// through it at once: a loop binding's use (`flow.for.ref.binding.local`),
    /// never `&xs[i]` itself, whose place is written `xs[i]`.
    Through,
}

/// A collection place the checker sees whole (`ref.elem.unaliased`): a
/// local holding no reference, and the struct fields, list slots and map
/// values read from it in order.
#[derive(Clone)]
pub(crate) struct CollPath {
    root: LocalId,
    steps: Vec<PathStep>,
}

#[derive(Clone, Copy)]
enum PathStep {
    Field(u32),
    Slot(Key),
}

/// An index or a key on a collection path. Two keys name one slot only
/// when both are one constant or one read of a binding nothing reassigns;
/// any other key may differ, and leaves the check to the pin.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Const(Ty, u64),
    Text(Symbol),
    Local(LocalId),
    Unknown,
}

impl Key {
    fn same(self, other: Key) -> bool {
        self != Key::Unknown && self == other
    }
}

impl CollPath {
    /// Whether `self` is `c` or a place that holds it: a change of
    /// `self`'s length or of `self` may change `c`'s length.
    fn holds(&self, c: &CollPath) -> bool {
        self.root == c.root
            && self.steps.len() <= c.steps.len()
            && self.steps.iter().zip(&c.steps).all(|(a, b)| match (a, b) {
                (PathStep::Field(x), PathStep::Field(y)) => x == y,
                (PathStep::Slot(x), PathStep::Slot(y)) => x.same(*y),
                _ => false,
            })
    }
}

/// An entry of the open calls' exclusivity stack (`ref.elem.conflict`).
pub(crate) enum Excl {
    /// An access to a collection: an element reference that is an
    /// argument or a receiver of an open call, or an element reference
    /// loop whose body is being checked (`ref.elem.access.live`,
    /// `flow.for.ref.access`).
    Access(CollPath),
    /// A `&mut` argument or receiver of the innermost open call, which
    /// the callee holds while any access of the same call lasts.
    Held(CollPath, NodeIdx),
}

impl Ck<'_, '_> {
    /// Opens the exclusivity frame of a call or an element reference
    /// loop; [`Self::close_access_frame`] ends its accesses.
    pub(crate) fn open_access_frame(&mut self) {
        self.excl_frames.push(self.excl.len());
    }

    pub(crate) fn close_access_frame(&mut self) {
        if let Some(m) = self.excl_frames.pop() {
            self.excl.truncate(m);
        }
    }

    /// The collection path of the value `r`: a read of a local that holds
    /// no reference, through struct fields, list elements and map values.
    /// `None` for anything a reference or a global may reach.
    fn value_path(&mut self, r: Ref) -> Option<CollPath> {
        let mut steps = Vec::new();
        let mut cur = r;
        for _ in 0..64 {
            let i = cur.as_inst()?.0 as usize;
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i], body.data[i])
            };
            if let Some(l) = self.b.body_mut().local_read(i) {
                let lt = self.b.local_ty(l);
                if self.ref_of(lt).is_some() && !self.exclusive_ref(l) {
                    return None;
                }
                steps.reverse();
                return Some(CollPath { root: l, steps });
            }
            // `*r` of a `&mut` binding `r` stands for `r` where its
            // referent is seen whole ([`Self::exclusive_ref`]).
            if tag == Tag::Read && self.mut_ref_loops > 0 {
                let body = self.b.body_mut();
                let p = body.view(data[0]);
                let mut s = p.steps(body);
                if let (hd_tir::ir::Root::Value(v), Some(hd_tir::ir::Proj::Deref), None) =
                    (p.root, s.next(), s.next())
                {
                    cur = Ref(v);
                    continue;
                }
            }
            if let Some((base, step)) = self.b.body_mut().read_step(i) {
                let ReadStep::Field(k) = step else {
                    return None;
                };
                steps.push(PathStep::Field(k));
                cur = Ref(base);
                continue;
            }
            if let Some((list, index)) = self.element_of(cur) {
                steps.push(PathStep::Slot(self.path_key(index)));
                cur = list;
                continue;
            }
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                _ => return None,
            }
        }
        None
    }

    /// Whether binding `l` is a `&mut T` that nothing reassigns, inside an
    /// element reference loop over what it refers to: `ref.excl.mut` makes
    /// its referent reachable through it alone, so the loop's body sees
    /// every change of that list's length (`flow.for.ref.conflict`).
    fn exclusive_ref(&mut self, l: LocalId) -> bool {
        if self.mut_ref_loops == 0 {
            return false;
        }
        let lt = self.b.local_ty(l);
        let f = self.b.body_mut().local_flags[l.idx()];
        let fixed = f & local_flags::SHORT != 0
            || (f & local_flags::PARAM != 0 && !self.mut_params.contains(&l));
        fixed && self.ref_counted(lt) == Some(false) && self.ref_of(lt).is_some_and(|(_, m)| m)
    }

    /// The collection path of the place `p`, or `None` when a reference
    /// or a global may reach it.
    fn place_path(&mut self, p: &Place) -> Option<CollPath> {
        let mut path = match p.root {
            Root::Local(l, _) => CollPath {
                root: l,
                steps: Vec::new(),
            },
            Root::Elem(coll, key, _) => {
                let mut c = self.value_path(coll)?;
                let k = self.path_key(key);
                c.steps.push(PathStep::Slot(k));
                c
            }
            _ => return None,
        };
        path.steps
            .extend(p.path.iter().map(|&(k, _)| PathStep::Field(k)));
        Some(path)
    }

    fn path_key(&mut self, mut r: Ref) -> Key {
        for _ in 0..8 {
            let Some(i) = r.as_inst() else {
                let body = self.b.body_mut();
                let Some(&(t, bits)) = body.consts.get((r.0 & !Ref::CONST_BIT) as usize) else {
                    return Key::Unknown;
                };
                if t == Ty::STRING {
                    let Some(text) = usize::try_from(bits).ok().and_then(|s| body.strings.get(s))
                    else {
                        return Key::Unknown;
                    };
                    let text = text.to_string();
                    return Key::Text(self.cx.names.syms.intern(&text));
                }
                return Key::Const(t, bits);
            };
            let i = i.0 as usize;
            if let Some(l) = self.b.body_mut().local_read(i) {
                let f = self.b.body_mut().local_flags[l.idx()];
                let fixed = f & local_flags::SHORT != 0
                    || (f & local_flags::PARAM != 0 && !self.mut_params.contains(&l));
                return if fixed { Key::Local(l) } else { Key::Unknown };
            }
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i], body.data[i])
            };
            // A map value's entry number stands for its key.
            r = match (tag, IntrinsicOp::from_u32(data[0])) {
                (Tag::Coerce, _) => Ref(data[0]),
                (Tag::Intrinsic, Some(IntrinsicOp::MapEntry)) => {
                    Ref(self.b.body_mut().record(data[1])[1])
                }
                _ => return Key::Unknown,
            };
        }
        Key::Unknown
    }

    /// An access to the collection of the element place `p` begins at
    /// `at`, an argument or a receiver of the innermost open call, or an
    /// element reference loop. A `&mut` argument or receiver of the same
    /// call that holds the collection conflicts with it
    /// (`ref.elem.conflict.reference`).
    pub(crate) fn begin_access(&mut self, coll: Ref) {
        let Some(&frame) = self.excl_frames.last() else {
            return;
        };
        let Some(c) = self.value_path(coll) else {
            return;
        };
        self.begin_access_path(c, frame);
    }

    fn begin_place_access(&mut self, place: &Place) {
        let Some(&frame) = self.excl_frames.last() else {
            return;
        };
        if let Some(path) = self.place_path(place) {
            self.begin_access_path(path, frame);
        }
    }

    fn begin_access_path(&mut self, c: CollPath, frame: usize) {
        let held: Vec<NodeIdx> = self.excl[frame..]
            .iter()
            .filter_map(|e| match e {
                Excl::Held(p, at) if p.holds(&c) => Some(*at),
                _ => None,
            })
            .collect();
        for at in held {
            self.conflicts.push((c.root, at));
        }
        self.excl.push(Excl::Access(c));
    }

    /// `&mut` of the place `p` is taken at `at`; `held` when it is an
    /// argument or a receiver of the innermost open call, which holds it
    /// until it returns. During an access to a collection that `p` holds,
    /// it may change the collection's length (`ref.elem.conflict`).
    pub(crate) fn mut_place_taken(&mut self, p: &Place, at: NodeIdx, held: bool) {
        if self.excl_frames.is_empty() {
            return;
        }
        let Some(path) = self.place_path(p) else {
            return;
        };
        self.length_change(&path, at);
        if held {
            self.excl.push(Excl::Held(path, at));
        }
    }

    /// A built-in method that changes its receiver's length, `push` or a
    /// map's `remove`, is called at `at` on the collection value `recv`
    /// (`ref.elem.conflict.method`).
    pub(crate) fn length_changed(&mut self, recv: Ref, at: NodeIdx) {
        if self.excl.is_empty() {
            return;
        }
        if let Some(path) = self.value_path(recv) {
            self.length_change(&path, at);
        }
    }

    fn length_change(&mut self, path: &CollPath, at: NodeIdx) {
        let hit = self
            .excl
            .iter()
            .any(|e| matches!(e, Excl::Access(c) if path.holds(c)));
        if hit {
            self.conflicts.push((path.root, at));
        }
    }

    /// Reports the conflicts found in the body. A collection a closure
    /// captures is checked too: a change the closure makes while an
    /// element reference lives meets the reference's loan on the
    /// variable's cell at run time (`ref.excl.alias.shared-variable`). The
    /// reported ones stay, so the exclusivity check skips their nodes.
    pub(crate) fn report_conflicts(&mut self) {
        let mut found = std::mem::take(&mut self.conflicts);
        if found.is_empty() {
            return;
        }
        let mut seen = HashSet::default();
        found.retain(|&(_, at)| seen.insert(at));
        for &(_, at) in &found {
            let node = self.cx.src.parse.tree.node(at);
            self.err(
                Code::ConflictingAccess,
                node,
                "this may change the length of a collection while an element reference into it is in use; make the change before or after that use",
            );
        }
        self.conflicts = found;
    }

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
        let Some((get @ (Indexed::List | Indexed::Map), kt, vt)) = builtin else {
            return self.trait_index_place(base, key, e, mutable, Some((br, bt)));
        };
        let (kr, ktt) = self.index_key_value(key, builtin)?;
        if get == Indexed::List && self.is_range_ty(ktt) {
            self.index_reference((br, bt), (kr, ktt), (key, e), mutable)?;
            return Ok(None);
        }
        let kr = self.index_key((kr, ktt), key, kt);
        let ct = self.strip_mut(self.infer.resolve(self.pool(), bt));
        // Insertion pins the collection before the reference.
        Ok(Some(crate::refs::Operand::Place(Place {
            root: Root::Elem(br, kr, ct),
            path: vec![],
            ty: vt,
        })))
    }

    /// `e` is used as `how`, where an element reference may stand.
    pub(crate) fn elem_may_stand(&mut self, e: NodeRef<'_>, how: ElemUse) {
        self.elem_ok.insert(strip_parens(e).index(), how);
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
        let (source, source_ty, place) = if mutable {
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
                Operand::Place(p) => Some(p),
                Operand::Value(mut r, mut t) => {
                    let mut found = None;
                    while let Some((inner, access)) = self.ref_of(t) {
                        if matches!(self.index_kind(inner), Some((Indexed::Map, ..))) {
                            found = Some(Place {
                                root: Root::Deref(r, t),
                                path: vec![],
                                ty: self.deref_ty(inner, access),
                            });
                            break;
                        }
                        (r, t) = self.emit_deref(r, t, src.index());
                    }
                    if found.is_none() {
                        fresh = true;
                        let (l, depth) = self.hold(r, t, operand);
                        self.add_flags(l, CAPTURED_ASSIGNED);
                        found = Some(Place {
                            root: Root::Local(l, depth),
                            path: vec![],
                            ty: t,
                        });
                    }
                    found
                }
            };
            let p = place.as_ref().expect("a map source place");
            if !fresh && !self.place_writable(p, operand) {
                let p = self
                    .b
                    .emit(Tag::Poison, NONE, NONE, Ty::POISON, src.index());
                return Ok((p, Ty::VOID));
            }
            self.mut_place_taken(p, src.index(), false);
            let rt = self.ref_ty(self.strip_mut(p.ty), true, false);
            (self.make_ref(p, true, rt, src), rt, place)
        } else {
            let (r, t) = self.expr(operand, None)?;
            let (r, t) = self.deref_index_base(r, t, operand.index());
            (r, t, None)
        };
        let mt = self.ref_of(source_ty).map_or(source_ty, |(inner, _)| inner);
        let Some((Indexed::Map, key, value)) = self.index_kind(mt) else {
            return crate::body::unsupported("a map loop source that is not a map");
        };
        let item = self.pool().intern_ty(&hd_types::TyData::Tuple {
            elems: self.pool().list(&[
                self.ref_ty(key, false, false),
                self.ref_ty(value, mutable, false),
            ]),
            rest: None,
        });
        self.collection_loop(
            pat,
            (source, source_ty, item, true),
            (loop_else, None),
            e,
            &mut |me| {
                me.open_access_frame();
                me.mut_ref_loops += u32::from(mutable);
                if let Some(p) = &place {
                    me.begin_place_access(p);
                }
                let result = body(me);
                me.mut_ref_loops -= u32::from(mutable);
                me.close_access_frame();
                result
            },
        )
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
                self.rollback(cp);
                return self
                    .map_ref_loop(pat, (src, operand, mutable), loop_else, e, body)
                    .map(Some);
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
        // An element reference loop's binding is local, as `&e` is
        // (`ref.expr.kind.always-local`).
        let rt = self.ref_ty(inner, mutable, false);
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
        // The loop is one access to the list (`flow.for.ref.access`). A
        // list reached through a `&mut` binding is seen whole.
        self.open_access_frame();
        self.mut_ref_loops += 1;
        self.begin_access(lr);
        let checked = body(self);
        self.mut_ref_loops -= 1;
        self.close_access_frame();
        checked?;
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
