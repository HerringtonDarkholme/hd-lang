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
//! use ends the pin (`docs/compiler/tir-v2.md`, stage 2 commit 12b; a
//! pinned list panics `collection-pinned` on a length change, a pinned map
//! on a store of a new key or a removal of a held one).

use hd_base::{HashSet, LocalId, NodeIdx, StageResult, Symbol};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_tir::ir::{IntrinsicOp, NONE, Ref, Tag, TirSink};
use hd_types::{Prim, Ty, TyData};

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
    /// A call argument (`ref.elem.call`).
    Arg,
    /// A method call's receiver (`ref.elem.call`).
    Receiver,
    /// A dereference's operand or a field's base, which reads or writes
    /// through it at once: a loop binding's use (`flow.for.ref.binding`),
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
    /// loop whose body is being checked (`ref.elem.access`,
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
    /// as escaping here, which only leaves more to the pin.
    pub(crate) fn report_conflicts(&mut self) {
        let mut found = std::mem::take(&mut self.conflicts);
        if found.is_empty() {
            return;
        }
        let captured: HashSet<LocalId> = self.b.body_mut().cap_local.iter().copied().collect();
        let mut seen = HashSet::default();
        found.retain(|&(l, at)| !captured.contains(&l) && seen.insert(at));
        for (_, at) in found {
            let node = self.cx.src.parse.tree.node(at);
            self.err(
                Code::ConflictingAccess,
                node,
                "this may change the length of a collection while an element reference into it is in use; make the change before or after that use",
            );
        }
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
        // The loop is one access to the list (`flow.for.ref.access`).
        self.open_access_frame();
        self.begin_access(lr);
        let checked = body(self);
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
