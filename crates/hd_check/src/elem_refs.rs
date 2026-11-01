//! Element references (spec/lang/13-values-and-references.md, "Element
//! References"; the tier-1 design `58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md` §6.3 and §6.4).
//!
//! `&xs[i]` and `&mut xs[i]` name a slot of the list's buffer, as Go's
//! `&s[i]` does, and `&m[k]` and `&mut m[k]` the value of a map's entry
//! (`ref.elem.map`), whose number `MapEntry` looks up just before the
//! reference. A loop over a list, `for p in xs` or `for p in &mut xs`,
//! binds an element reference per step and takes a loan of the list for
//! its body (`flow.for.ref.access`). An element reference is a `Ref` of the element
//! place and nothing else: the ownership passes make the collection unique
//! and pin it before the reference, and the reference's drop at its last
//! use ends the pin (`docs/compiler/tir-v2.md`, stage 2 commit 12b; a
//! pinned list panics `collection-pinned` on a length change, a pinned map
//! on a store of a new key or a removal of a held one).

use hd_base::{HashSet, LocalId, NodeIdx, StageResult, Symbol};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{IntrinsicOp, NONE, PlaceRoot, Proj, Ref, Tag, TirSink};
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

/// The loan a loop over a list takes (`flow.for.loan`): the list value
/// the loop read, the body's instruction range, and whether the loop binds
/// `&mut` references.
pub(crate) struct LoopLoan {
    coll: Ref,
    span: (usize, usize),
    excl: bool,
}

/// The place that instruction `i` reads, refers to or writes, and whether
/// it writes it (`ref.excl.write`): a store, a `&mut`, a `&mut` binding a
/// call takes, or a built-in change of a collection operand.
fn touch(b: &hd_tir::ir::Body, i: usize) -> Option<(u32, bool)> {
    let [a, d] = b.data[i];
    match b.tags[i] {
        Tag::Store => Some((a, true)),
        Tag::Ref => Some((a, d & hd_tir::ir::ref_word::MUT != 0)),
        Tag::Read => Some((a, false)),
        Tag::Intrinsic
            if matches!(
                IntrinsicOp::from_u32(a),
                Some(
                    IntrinsicOp::ListPush
                        | IntrinsicOp::ListReserve
                        | IntrinsicOp::MapSet
                        | IntrinsicOp::MapRemove
                )
            ) =>
        {
            let c = *b.record(d).first()? as usize;
            (b.tags.get(c) == Some(&Tag::Read)).then(|| (b.data[c][0], true))
        }
        _ => None,
    }
}

/// The local and path of place `word`, through the reads its value root
/// is made of.
fn place_path(b: &hd_tir::ir::Body, word: u32) -> Option<(LocalId, Vec<Proj>)> {
    let p = b.decode_place(word)?;
    match p.root {
        PlaceRoot::Local(l) => Some((l, p.proj)),
        PlaceRoot::Value(v) => {
            let (l, mut path) = value_place(b, v.0)?;
            path.extend(p.proj);
            Some((l, path))
        }
        _ => None,
    }
}

/// The place the value `v` was read from, past its coercions.
fn value_place(b: &hd_tir::ir::Body, mut v: u32) -> Option<(LocalId, Vec<Proj>)> {
    while b.ty.get(v as usize).is_some() && b.tags[v as usize] == Tag::Coerce {
        v = b.data[v as usize][0];
    }
    if b.ty.get(v as usize).is_none() || b.tags[v as usize] != Tag::Read {
        return None;
    }
    place_path(b, b.data[v as usize][0])
}

/// Whether two paths of one local overlap: no step of the one is a field
/// other than the other's at the same depth (`ref.excl.place`).
fn overlap(a: &[Proj], b: &[Proj]) -> bool {
    !a.iter()
        .zip(b)
        .any(|p| matches!(p, (Proj::Field(x), Proj::Field(y)) if x != y))
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
                if self.ref_of(lt).is_some() {
                    return None;
                }
                steps.reverse();
                return Some(CollPath { root: l, steps });
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

    /// Reports the conflicts found in the body on collections that stay
    /// unaliased: a closure that captures the collection's binding may
    /// change it where the checker cannot see, and the pin catches that
    /// (`ref.elem.unaliased`, `ref.elem.access.pin`). Any closure counts
    /// as escaping here, which only leaves more to the pin. The reported
    /// ones stay, so the exclusivity check skips their nodes.
    pub(crate) fn report_conflicts(&mut self) {
        let mut found = std::mem::take(&mut self.conflicts);
        if found.is_empty() {
            return;
        }
        let captured: HashSet<LocalId> = self.b.body_mut().cap_local.iter().copied().collect();
        let mut seen = HashSet::default();
        found.retain(|&(l, at)| !captured.contains(&l) && seen.insert(at));
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
    /// of the key `k`. `None` after an error; any other index is no place
    /// to refer to.
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
        let Some((get @ (Indexed::List | Indexed::Map), kt, vt)) = builtin else {
            self.not_yet_index(e);
            return Ok(None);
        };
        let (kr, ktt) = self.index_key_value(*key, builtin)?;
        if get == Indexed::List && self.is_range_ty(ktt) {
            self.not_yet_index(e);
            return Ok(None);
        }
        let kr = self.index_key((kr, ktt), *key, kt);
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

    /// `ref.check.not-yet-index`: a reference to an index result that is
    /// no list element, such as a slice or a user `Index` call.
    fn not_yet_index(&mut self, e: NodeRef<'_>) {
        self.err(
            Code::ReferencesNotYetChecked,
            e,
            "a reference to this index result is not checked yet; only a list element is a place",
        );
    }

    /// `for p in &xs` and `for p in &mut xs`: a list's loop
    /// ([`Self::slot_loop`]). `None` when the operand is no list or map,
    /// whose loop is the ordinary one.
    pub(crate) fn ref_loop(
        &mut self,
        pat: NodeRef<'_>,
        src: NodeRef<'_>,
        ends: crate::expr::LoopEnds,
        e: NodeRef<'_>,
        body: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<Option<(Ref, Ty)>> {
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
        match self.index_kind(lt) {
            Some((Indexed::List, _, vt)) => {
                self.commit(cp);
                let list = (lr, lt, vt);
                self.slot_loop(pat, (src, list, mutable), ends, e, body)
                    .map(Some)
            }
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
                Ok(Some((p, Ty::VOID)))
            }
            _ => {
                self.rollback(cp);
                Ok(None)
            }
        }
    }

    /// A loop over a list (`flow.for.list`): a counted loop over the slots
    /// of the list value `lr` of type `lt`, already read through a
    /// reference, that binds a `&T`, or for `mutable` a `&mut T`, to each
    /// slot. The loop is one loan of the list from its first step to its
    /// last (`flow.for.loan`), which [`Self::report_loop_loans`] checks
    /// against the body's writes.
    pub(crate) fn slot_loop(
        &mut self,
        pat: NodeRef<'_>,
        (src, (lr, lt, vt), mutable): (NodeRef<'_>, (Ref, Ty, Ty), bool),
        (loop_else, reserve): crate::expr::LoopEnds,
        e: NodeRef<'_>,
        body: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<(Ref, Ty)> {
        use hd_tir::ir::local_flags::ASSIGNED;
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
        if let Some(acc) = reserve {
            let count = self.b.get(len_l, us, at);
            self.reserve_room(acc, count, at);
        }
        let zero = self.b.const_value(us, 0);
        self.set_local(pos_l, zero, at);
        // A list inside a counted object is held by a hidden reference
        // binding, the backend's run-time loan (`flow.for.loan.alias`),
        // and each step reaches the list through it.
        let holder = self.counted_place(lr).map(|word| {
            let rt = self.ref_ty(lt, mutable, false);
            // `SLOT` marks the loop's loan, which only a development or
            // test build registers.
            let w = u32::from(mutable) | hd_tir::ir::ref_word::SLOT;
            let r = self.b.emit(Tag::Ref, word, w, rt, at);
            let l = self
                .b
                .local(rt, self.cx.names.syms.intern("$loan"), ASSIGNED, at);
            self.set_local(l, r, at);
            (l, rt)
        });
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
        let lr = match holder {
            Some((l, rt)) => {
                let r = self.b.get(l, rt, at);
                self.emit_deref(r, rt, at).0
            }
            None => lr,
        };
        let place = Place {
            root: Root::Elem(lr, index, lt),
            path: vec![],
            ty: vt,
        };
        let ok = !mutable || self.place_writable(&place, src);
        let inner = self.strip_mut(self.infer.resolve(self.pool(), vt));
        // A loop's binding is local, as `&e` is
        // (`ref.expr.kind.always-local`).
        let rt = self.ref_ty(inner, mutable, false);
        let item = if ok {
            let r = self.make_ref(&place, mutable, rt, src);
            // A shared loop's slot reference (`flow.for.ref.access`).
            if !mutable && let Some(i) = r.as_inst() {
                self.b.body_mut().data[i.0 as usize][1] |= hd_tir::ir::ref_word::SLOT;
            }
            r
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
        // The loop's loan covers its body (`flow.for.loan`).
        let lo = self.b.body_mut().len();
        body(self)?;
        let hi = self.b.body_mut().len();
        self.loop_loans.push(LoopLoan {
            coll: lr,
            span: (lo, hi),
            excl: mutable,
        });
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
        Ok((self.b.close_loop(lp, lbody, ty, at), ty))
    }

    /// The place of the list value `lr` when it is read from inside a
    /// counted object: one dereference of a counted reference, then
    /// fields.
    fn counted_place(&mut self, lr: Ref) -> Option<u32> {
        let i = lr.as_inst()?.0 as usize;
        let (word, root) = {
            let b = self.b.body_mut();
            if b.tags[i] != Tag::Read {
                return None;
            }
            let word = b.data[i][0];
            let p = b.decode_place(word)?;
            let [Proj::Deref, rest @ ..] = p.proj.as_slice() else {
                return None;
            };
            if !rest.iter().all(|s| matches!(s, Proj::Field(_))) {
                return None;
            }
            let t = match p.root {
                PlaceRoot::Local(l) => b.local_ty[l.idx()],
                PlaceRoot::Value(v) => *b.ty.get(v.0 as usize)?,
                _ => return None,
            };
            (word, t)
        };
        (self.ref_counted(root) == Some(true)).then_some(word)
    }

    /// `flow.for.loan.shared`, `flow.for.loan.mut`: in the body of a loop
    /// over a list place, a write to the list or to a place that holds it
    /// or lies in it is an error, and for a `&mut` loop any other use of
    /// it too. A list reached through a counted reference is left to the
    /// run-time checks; so is a closure that captures the list.
    pub(crate) fn report_loop_loans(&mut self) {
        let loans = std::mem::take(&mut self.loop_loans);
        if loans.is_empty() {
            return;
        }
        let pool = self.pool();
        // Each conflicting instruction's node, whether it writes, and the
        // loop's local.
        let mut found: Vec<(NodeIdx, bool, LocalId)> = Vec::new();
        {
            let b = self.b.body_mut();
            for loan in &loans {
                let Some((root, path)) = value_place(b, loan.coll.0) else {
                    continue;
                };
                let (lo, hi) = (loan.span.0, loan.span.1.min(b.len()));
                // A read that only roots a longer place is that place's use.
                let mut base = HashSet::default();
                for i in lo..hi {
                    if let Some((word, _)) = touch(b, i)
                        && let Some(p) = b.decode_place(word)
                        && let PlaceRoot::Value(mut v) = p.root
                    {
                        while b.ty.get(v.0 as usize).is_some()
                            && b.tags[v.0 as usize] == Tag::Coerce
                        {
                            v = Ref(b.data[v.0 as usize][0]);
                        }
                        base.insert(v.0 as usize);
                    }
                }
                for i in (lo..hi).filter(|i| !base.contains(i)) {
                    let Some((word, mut write)) = touch(b, i) else {
                        continue;
                    };
                    // A `&mut` binding read whole lends its target to a call.
                    if b.tags[i] == Tag::Read {
                        write |= matches!(
                            pool.get(b.ty[i]),
                            hd_types::TyData::Ref {
                                mutable: true,
                                counted: false,
                                ..
                            }
                        );
                    }
                    if !write && !loan.excl {
                        continue;
                    }
                    let Some((r, p)) = place_path(b, word) else {
                        continue;
                    };
                    let at = b.syn[i];
                    if r == root && overlap(&path, &p) && at != NodeIdx::NONE {
                        found.push((at, write, root));
                    }
                }
            }
        }
        // One error for a node, or for a node inside or around a reported
        // one.
        let mut done: Vec<(u32, u32)> = Vec::new();
        for (at, write, root) in found {
            let node = self.cx.src.parse.tree.node(at);
            let span = self.cx.src.span(node);
            if (done.iter())
                .any(|&(lo, hi)| lo <= span.lo && span.hi <= hi || span.lo <= lo && hi <= span.hi)
            {
                continue;
            }
            done.push((span.lo, span.hi));
            let name = self
                .cx
                .names
                .text(self.b.body_mut().local_name[root.idx()])
                .to_owned();
            let place = if name.starts_with('$') {
                "the list".to_owned()
            } else {
                format!("`{name}`")
            };
            let msg = if write {
                format!(
                    "this changes {place} while a `for` loop borrows it; change it after the loop, or loop over a copy or by index"
                )
            } else {
                format!(
                    "this uses {place} while a `for` loop holds it as `&mut`; reach its elements through the loop's binding"
                )
            };
            self.err(Code::ConflictingAccess, node, &msg);
        }
    }
}
