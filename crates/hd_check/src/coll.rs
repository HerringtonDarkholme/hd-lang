//! Collection values (spec 04 "Collection Values", `OBJECT_MODEL_TIER1.md`
//! §6.2): `List[T]` and `Map[K, V]` are values with copy-on-write. A
//! copy of a value that holds a collection is the TIR `ValueCopy`, which
//! the checker places where a value is stored: a binding, an assignment,
//! a field, an element, a payload, a return. A value made by the
//! expression itself (a literal, a call's result) is no copy of anything
//! and stores as is.
//!
//! Changing a collection needs a writable place, as changing a struct's
//! field does: its type is then `mut List[T]`. A legacy parameter, field
//! or annotation written `mut List[T]` keeps sharing the list it is given
//! until std migrates (`types.coll.legacy`).
//!
//! A change written through a path (`m[k].push(x)`, `xs[i].x = v`) makes
//! each list or map on the path its own first: its element read becomes
//! `ListIndexMut` or `MapIndexMut`, so the change is made in place and
//! never by reading the element out and storing it back.
//!
//! A value argument is the callee's own copy. It is passed as is, and
//! copied only where something besides the caller may change the place it
//! reads while the callee runs (`value_arg`). A closure that captures a
//! parameter, or a binding that took one as is, holds a copy of it
//! (`capture_copies`), since the caller may change its list once the call
//! returns.

use hd_base::{LocalId, NodeIdx};
use hd_syntax::NodeRef;
use hd_tir::ir::{Coercion, IntrinsicOp, NONE, Ref, Slot, SubMark, Tag, TirSink, local_flags};
use hd_types::{Ty, TyData};

use crate::body::Ck;

/// A value argument read from a writable local whose copy waits for the
/// end of the body (`Ck::arg_copies`): a later closure or `&mut` in the
/// same loop or closure may still let something else change the local.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArgCopy {
    slot: Slot,
    value: Ref,
    ty: Ty,
    local: LocalId,
    at: NodeIdx,
}

/// The place a value argument reads, for whether the callee may see it
/// change while it runs.
enum ArgRoot {
    /// No place, or one nothing changes in place: a `:=` or plain `let`
    /// binding, a parameter, a tuple element or a payload.
    Fixed,
    /// A place others may change: a legacy `mut` view, a mutable global,
    /// a value read through a reference, a `data` field.
    Shared,
    /// A `let mut` local or a `mut` parameter.
    Local(LocalId),
}

impl Ck<'_, '_> {
    /// Whether `t` is a `List` or `Map` value type: no `mut` view.
    pub(crate) fn is_coll_value(&self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let known = self.cx.names.known;
        matches!(pool.get(t), TyData::Adt { def, .. } if def == known.list || def == known.map)
    }

    /// Whether `t`, `mut` or not, is a `List` or `Map` type.
    pub(crate) fn is_coll_ty(&self, t: Ty) -> bool {
        self.is_coll_value(self.strip_mut(t))
    }

    /// Whether storing a value of type `t` may copy a collection: a `List`
    /// or `Map` value, a struct holding one in a field (at any depth of
    /// struct fields), or a type not known until instantiation. A tuple,
    /// an optional or an enum holds its collections in elements and
    /// payloads that are no places, so a copy of one shares them safely.
    pub(crate) fn may_hold_coll(&self, t: Ty) -> bool {
        self.may_hold_coll_at(t, 0)
    }

    fn may_hold_coll_at(&self, t: Ty, depth: u32) -> bool {
        if depth > 16 {
            return false;
        }
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        match pool.get(t) {
            TyData::Param(_) => true,
            TyData::Adt { def, args } => {
                let known = self.cx.names.known;
                if def == known.list || def == known.map {
                    return true;
                }
                if !self.is_struct_def(def) {
                    return false;
                }
                let Some(hd_resolve::ItemData::Data(fields)) =
                    self.cx.lookup.item(def).map(|i| &i.data)
                else {
                    return false;
                };
                let argv = pool.list_items(args);
                fields.iter().any(|f| {
                    let ft = pool.subst(f.ty, &|p: hd_types::ParamRef| {
                        (p.owner == def)
                            .then(|| argv.get(p.index as usize).copied())
                            .flatten()
                    });
                    self.may_hold_coll_at(ft, depth + 1)
                })
            }
            _ => false,
        }
    }

    /// Whether `r` is a value the expression made itself, which no place
    /// holds: a literal, a constructor, a call's result whose type is no
    /// legacy `mut` view, a copy.
    pub(crate) fn is_fresh(&mut self, r: Ref) -> bool {
        let Some(i) = r.as_inst() else {
            return true;
        };
        let (tag, data) = {
            let body = self.b.body_mut();
            (body.tags[i.0 as usize], body.data[i.0 as usize])
        };
        let t = self.b.ty_of(r);
        let legacy = matches!(
            self.pool().get(self.infer.resolve(self.pool(), t)),
            TyData::Mut(_)
        );
        match tag {
            Tag::NewList | Tag::NewMap | Tag::NewData | Tag::CopyData | Tag::NewVariant => true,
            Tag::NewTuple | Tag::Closure | Tag::Interp | Tag::Prim => true,
            Tag::Call | Tag::CallDyn | Tag::DefaultCall => !legacy,
            Tag::CallValue => self.deref_source(Ref(data[0])).is_none() && !legacy,
            Tag::Intrinsic => {
                matches!(
                    IntrinsicOp::from_u32(data[0]),
                    Some(IntrinsicOp::ValueCopy | IntrinsicOp::Item)
                ) && !legacy
            }
            Tag::Coerce => self.is_fresh(Ref(data[0])),
            _ => false,
        }
    }

    /// Whether `r` reads a local that never changes in place: a `:=`, a
    /// plain `let` or a parameter, not a `let mut` and not a legacy `mut`
    /// view. A copy into another such binding may share it.
    fn reads_fixed_local(&mut self, r: Ref) -> bool {
        let Some(i) = r.as_inst() else {
            return false;
        };
        let (tag, data) = {
            let body = self.b.body_mut();
            (body.tags[i.0 as usize], body.data[i.0 as usize])
        };
        match tag {
            Tag::Coerce => self.reads_fixed_local(Ref(data[0])),
            // An element of a tuple or a payload is no place, so nothing
            // changes the collection it holds in place.
            Tag::TupleGet | Tag::Payload | Tag::Unwrap => true,
            Tag::LocalGet => {
                let l = LocalId::from_raw(data[0]);
                let lt = self.b.local_ty(l);
                !self.mut_locals.contains(&l)
                    && !matches!(
                        self.pool().get(self.infer.resolve(self.pool(), lt)),
                        TyData::Mut(_)
                    )
            }
            _ => false,
        }
    }

    /// The value `v` of type `t` as a store copies it (`types.coll.value`):
    /// itself when it holds no collection or was just made, else its
    /// `ValueCopy`.
    pub(crate) fn coll_copy(&mut self, v: Ref, t: Ty, at: hd_base::NodeIdx) -> Ref {
        if v.0 == NONE || !self.may_hold_coll(t) || self.is_fresh(v) {
            return v;
        }
        let rec = self.b.refs_record(&[v]);
        self.b
            .emit(Tag::Intrinsic, IntrinsicOp::ValueCopy as u32, rec, t, at)
    }

    /// The value `v` a function returns as its result of type `ret`
    /// (`types.coll.value`): a copy, unless it is the function's
    /// own changeable local, which the return moves out, since no other
    /// place can hold its collection unmarked.
    pub(crate) fn return_copy(&mut self, v: Ref, ret: Ty, at: hd_base::NodeIdx) -> Ref {
        let mut cur = v;
        while let Some(i) = cur.as_inst() {
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::LocalGet => {
                    let l = LocalId::from_raw(data[0]);
                    let flags = self.b.body_mut().local_flags[l.idx()];
                    let lt = self.b.local_ty(l);
                    let legacy = matches!(
                        self.pool().get(self.infer.resolve(self.pool(), lt)),
                        TyData::Mut(_)
                    );
                    let own = flags & (local_flags::PARAM | local_flags::CAPTURED) == 0
                        && (self.mut_locals.contains(&l) || legacy);
                    if own {
                        return v;
                    }
                    break;
                }
                _ => break,
            }
        }
        // A legacy `mut List[T]` result shares (`types.coll.legacy`).
        if matches!(
            self.pool().get(self.infer.resolve(self.pool(), ret)),
            TyData::Mut(_)
        ) {
            return v;
        }
        self.coll_copy(v, ret, at)
    }

    /// The value `v` as a store into local `l` copies it: only a binding
    /// of `copy_locals`, and a binding that never changes takes a fixed
    /// local's value as is.
    pub(crate) fn copy_into_local(&mut self, l: LocalId, v: Ref, at: hd_base::NodeIdx) -> Ref {
        if !self.copy_locals.contains(&l) || v.0 == NONE {
            return v;
        }
        let lt = self.b.local_ty(l);
        if !self.mut_locals.contains(&l) && self.reads_fixed_local(v) {
            self.unowned.push(l);
            return v;
        }
        let t = self.b.ty_of(v);
        let t = if matches!(
            self.pool().get(self.infer.resolve(self.pool(), t)),
            TyData::Mut(_)
        ) {
            lt
        } else {
            t
        };
        self.coll_copy(v, t, at)
    }

    /// The entry copies of the `mut` parameters among `params` that hold
    /// collections (`fn.param.mut.copy`): the callee changes its own copy
    /// of the caller's value.
    pub(crate) fn mut_param_copies(&mut self, params: &[LocalId], at: hd_base::NodeIdx) {
        for &l in params {
            if !self.mut_params.contains(&l) {
                continue;
            }
            let lt = self.b.local_ty(l);
            if !self.may_hold_coll(lt) {
                continue;
            }
            let v = self.b.get(l, lt, at);
            let rec = self.b.refs_record(&[v]);
            let c = self
                .b
                .emit(Tag::Intrinsic, IntrinsicOp::ValueCopy as u32, rec, lt, at);
            self.b.set(l, c, at);
        }
    }

    /// Marks a store target `l` for copies (`types.coll.value`) unless its
    /// annotation is a legacy `mut List[T]` or `mut Map[K, V]` view.
    pub(crate) fn note_copy_local(&mut self, l: LocalId, annot: Option<Ty>) {
        if let Some(a) = annot {
            let pool = self.pool();
            if let TyData::Mut(i) = pool.get(self.infer.resolve(pool, a))
                && self.is_coll_value(i)
            {
                return;
            }
        }
        let flags = self.b.body_mut().local_flags[l.idx()];
        if flags & local_flags::PARAM != 0 {
            return;
        }
        self.copy_locals.push(l);
    }

    /// The value argument `r` for a parameter of type `param`
    /// (`types.coll.value`): the callee's own copy. It is passed as is
    /// unless something besides the caller may change the place it reads
    /// while the callee runs: a closure or a `&mut` that changes a writable
    /// local, a legacy `mut` view, a mutable global, a read through a
    /// reference, a `data` field. Then it is its `ValueCopy`, which marks
    /// the caller's collection, so such a change copies first. A legacy
    /// `mut` parameter, or a type parameter the call solved to a legacy
    /// view, shares instead (`types.coll.legacy`), and a reference
    /// parameter refers.
    pub(crate) fn value_arg(&mut self, r: Ref, param: Ty, at: NodeIdx) -> Ref {
        if r.0 == NONE {
            return r;
        }
        let pool = self.pool();
        if matches!(pool.get(self.infer.resolve(pool, param)), TyData::Mut(_))
            || self.ref_of(param).is_some()
            || self.ref_of(self.b.ty_of(r)).is_some()
        {
            return r;
        }
        let t = self.strip_mut(self.b.ty_of(r));
        if !self.may_hold_coll(t) || self.is_fresh(r) {
            return r;
        }
        match self.arg_root(r) {
            ArgRoot::Fixed => r,
            ArgRoot::Shared => self.value_copy(r, t, at),
            ArgRoot::Local(l) if self.local_aliased(l) => self.value_copy(r, t, at),
            // Straight-line code: whatever lets something else change the
            // local comes after the call, so it cannot run during it.
            ArgRoot::Local(_) if self.loops.is_empty() && self.subs.is_empty() => r,
            ArgRoot::Local(local) => {
                let slot = self.b.reserve();
                let s = slot.as_ref();
                self.b.body_mut().ty[s.0 as usize] = t;
                self.arg_copies.push(ArgCopy {
                    slot,
                    value: r,
                    ty: t,
                    local,
                    at,
                });
                s
            }
        }
    }

    /// The value receiver `r` of a method whose `self` is `param`: the
    /// callee's own copy, as an argument is (`value_arg`). A list or map
    /// receiver is passed as is: std's views and cursors over it (`view`,
    /// `iter`) keep the receiver they are given as a legacy view
    /// (`types.coll.legacy.data-field`) until std migrates.
    pub(crate) fn value_receiver(&mut self, r: Ref, param: Ty, at: NodeIdx) -> Ref {
        if r.0 == NONE || self.is_coll_ty(self.b.ty_of(r)) {
            return r;
        }
        self.value_arg(r, param, at)
    }

    fn value_copy(&mut self, v: Ref, t: Ty, at: NodeIdx) -> Ref {
        let rec = self.b.refs_record(&[v]);
        self.b
            .emit(Tag::Intrinsic, IntrinsicOp::ValueCopy as u32, rec, t, at)
    }

    /// Whether something besides the writable local `l` may change its
    /// collection: a closure or reference that assigns it, a closure that
    /// changes it in place, a legacy view sharing it.
    fn local_aliased(&mut self, l: LocalId) -> bool {
        self.alias_locals.contains(&l)
            || self.b.body_mut().local_flags[l.idx()] & local_flags::CAPTURED_ASSIGNED != 0
    }

    /// The place the value argument `r` reads.
    fn arg_root(&mut self, r: Ref) -> ArgRoot {
        let mut cur = r;
        for _ in 0..64 {
            let Some(i) = cur.as_inst() else {
                return ArgRoot::Fixed;
            };
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            let ct = self.b.ty_of(cur);
            let legacy = matches!(
                self.pool().get(self.infer.resolve(self.pool(), ct)),
                TyData::Mut(_)
            );
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::Field => {
                    let base = Ref(data[0]);
                    if !self.is_struct_ty(self.b.ty_of(base)) {
                        return ArgRoot::Shared;
                    }
                    cur = base;
                }
                Tag::Intrinsic => {
                    if !matches!(
                        IntrinsicOp::from_u32(data[0]),
                        Some(
                            IntrinsicOp::ListIndex
                                | IntrinsicOp::ListIndexMut
                                | IntrinsicOp::MapIndex
                                | IntrinsicOp::MapIndexMut
                        )
                    ) {
                        return if legacy {
                            ArgRoot::Shared
                        } else {
                            ArgRoot::Fixed
                        };
                    }
                    let Some(&base) = self.b.body_mut().record(data[1]).first() else {
                        return ArgRoot::Fixed;
                    };
                    cur = Ref(base);
                }
                Tag::LocalGet => {
                    let l = LocalId::from_raw(data[0]);
                    if self.mut_locals.contains(&l) {
                        return ArgRoot::Local(l);
                    }
                    let lt = self.b.local_ty(l);
                    let view = matches!(
                        self.pool().get(self.infer.resolve(self.pool(), lt)),
                        TyData::Mut(_)
                    );
                    return if view {
                        ArgRoot::Shared
                    } else {
                        ArgRoot::Fixed
                    };
                }
                Tag::GlobalGet => {
                    let Some(&def) = self.b.body_mut().record(data[0]).first() else {
                        return ArgRoot::Fixed;
                    };
                    let init = self.cx.init.borrow();
                    let mutable = init
                        .globals
                        .values()
                        .any(|g| g.def.raw() == def && g.mutable)
                        || init
                            .earlier
                            .values()
                            .any(|g| g.def.raw() == def && g.mutable);
                    return if mutable || legacy {
                        ArgRoot::Shared
                    } else {
                        ArgRoot::Fixed
                    };
                }
                Tag::CallValue if self.deref_source(Ref(data[0])).is_some() => {
                    return ArgRoot::Shared;
                }
                Tag::TupleGet | Tag::Payload | Tag::Unwrap => return ArgRoot::Fixed,
                _ => {
                    return if legacy {
                        ArgRoot::Shared
                    } else {
                        ArgRoot::Fixed
                    };
                }
            }
        }
        ArgRoot::Shared
    }

    /// Records that a change is written through the place `r` (a receiver,
    /// an index base, a legacy `mut` argument): when it starts at a
    /// writable local that a closure open here captures, something besides
    /// the local changes it (`local_aliased`).
    pub(crate) fn note_change_in_closure(&mut self, r: Ref) {
        let Some(l) = self.place_root_local(r) else {
            return;
        };
        let captured = self
            .subs
            .iter()
            .any(|s| self.b.open_captures(s.mark).contains(&l));
        if captured && !self.alias_locals.contains(&l) {
            self.alias_locals.push(l);
        }
    }

    /// The writable local `r` starts at, shared with a legacy `mut` view
    /// (`types.coll.legacy.argument`): the view may change it later.
    pub(crate) fn note_legacy_share(&mut self, r: Ref) {
        if let Some(l) = self.place_root_local(r)
            && self.mut_locals.contains(&l)
            && !self.alias_locals.contains(&l)
        {
            self.alias_locals.push(l);
        }
    }

    /// Settles each value argument that waited (`Ck::arg_copies`): its
    /// copy when its local turned out to be changed by something else,
    /// else the value as is.
    pub(crate) fn settle_arg_copies(&mut self) {
        for c in std::mem::take(&mut self.arg_copies) {
            if self.local_aliased(c.local) {
                let rec = self.b.refs_record(&[c.value]);
                self.b.fill(
                    c.slot,
                    Tag::Intrinsic,
                    IntrinsicOp::ValueCopy as u32,
                    rec,
                    c.ty,
                    c.at,
                );
            } else {
                let rec = self.b.words_record(&[Coercion::Identity as u32, NONE]);
                let ty = self.b.ty_of(c.value);
                self.b.fill(c.slot, Tag::Coerce, c.value.0, rec, ty, c.at);
            }
        }
    }

    /// Before the closure `mark` is made: each local it captures that may
    /// hold the caller's collection, a parameter that is not `mut` or a
    /// binding that took one as is, becomes its own copy, so the closure
    /// never sees the caller change it later (`fn.capture.copy.value`). A
    /// local an enclosing closure captures was copied when that closure
    /// was made.
    pub(crate) fn capture_copies(&mut self, mark: SubMark, at: NodeIdx) {
        let outer = self.subs.last().map(|s| s.mark);
        for l in self.b.open_captures(mark) {
            let flags = self.b.body_mut().local_flags[l.idx()];
            let param = flags & local_flags::PARAM != 0 && !self.mut_params.contains(&l);
            if !(param || self.unowned.contains(&l)) || self.mut_locals.contains(&l) {
                continue;
            }
            if outer.is_some_and(|m| self.b.open_captures(m).contains(&l)) {
                continue;
            }
            let lt = self.b.local_ty(l);
            let view = matches!(
                self.pool().get(self.infer.resolve(self.pool(), lt)),
                TyData::Mut(_)
            );
            if view || !self.may_hold_coll(lt) {
                continue;
            }
            let v = self.b.get(l, lt, at);
            let c = self.value_copy(v, lt, at);
            self.b.set(l, c, at);
        }
    }

    /// The element `item` a counted `for` binds, read from the snapshot,
    /// once the body is checked: its copy when a closure in the body
    /// captures one of the loop's bindings `binds`, so the closure keeps
    /// the element as it was when the collection changes after the loop.
    /// A map's entry is a tuple, rebuilt with its key and value copied.
    pub(crate) fn loop_binding_copy(
        &mut self,
        binds: &[(NodeIdx, LocalId)],
        item: Ref,
        item_ty: Ty,
        at: NodeIdx,
    ) -> Ref {
        let captured = binds
            .iter()
            .any(|(_, l)| self.b.body_mut().local_flags[l.idx()] & local_flags::CAPTURED != 0);
        if !captured {
            return item;
        }
        let pool = self.pool();
        if let TyData::Tuple { elems, rest: None } = pool.get(self.infer.resolve(pool, item_ty)) {
            let tys = pool.list_items(elems).to_vec();
            if !tys.iter().any(|t| self.may_hold_coll(*t)) {
                return item;
            }
            let mut parts = Vec::new();
            for (k, et) in tys.into_iter().enumerate() {
                let idx = u32::try_from(k).unwrap_or(0);
                let g = self.b.emit(Tag::TupleGet, item.0, idx, et, at);
                parts.push(if self.may_hold_coll(et) {
                    self.value_copy(g, et, at)
                } else {
                    g
                });
            }
            let rec = self.b.refs_record(&parts);
            return self.b.emit(Tag::NewTuple, NONE, rec, item_ty, at);
        }
        if !self.may_hold_coll(item_ty) {
            return item;
        }
        self.value_copy(item, item_ty, at)
    }

    /// The change about to be written through `r` (a receiver, an index
    /// base) reaches only the collections on its path: each element or
    /// map value read on it becomes `ListIndexMut` or `MapIndexMut`, which
    /// makes that collection's buffer its own first (`types.coll.nested`).
    pub(crate) fn mark_write_path(&mut self, r: Ref) {
        let mut cur = r;
        for _ in 0..64 {
            let Some(i) = cur.as_inst() else {
                return;
            };
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::Field => cur = Ref(data[0]),
                Tag::Intrinsic => {
                    let mutated = match IntrinsicOp::from_u32(data[0]) {
                        Some(IntrinsicOp::ListIndex | IntrinsicOp::ListIndexMut) => {
                            IntrinsicOp::ListIndexMut
                        }
                        Some(IntrinsicOp::MapIndex | IntrinsicOp::MapIndexMut) => {
                            IntrinsicOp::MapIndexMut
                        }
                        _ => return,
                    };
                    self.b.body_mut().data[i.0 as usize][0] = mutated as u32;
                    let args = self.b.body_mut().record(data[1]).to_vec();
                    let Some(&base) = args.first() else {
                        return;
                    };
                    cur = Ref(base);
                }
                _ => return,
            }
        }
    }

    /// Whether the value `br` reads a place a change may write through
    /// (`types.coll.writable`), judged without reporting: a `let mut`
    /// local or `mut` parameter, a writable top-level binding, a value
    /// read through `&mut`, an element of a writable list or a value of a
    /// writable map, a struct field of any of these, or a value whose type
    /// already grants `mut` (a legacy view, a data field declared `mut`).
    pub(crate) fn place_writable_quiet(&mut self, br: Ref) -> bool {
        let mut cur = br;
        for _ in 0..64 {
            let Some(i) = cur.as_inst() else {
                return false;
            };
            let ct = self.b.ty_of(cur);
            if matches!(
                self.pool().get(self.infer.resolve(self.pool(), ct)),
                TyData::Mut(_)
            ) {
                return true;
            }
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::Field => {
                    let inner = Ref(data[0]);
                    let it = self.b.ty_of(inner);
                    // A struct field is part of its struct's place. A
                    // struct in a field of a data value with mutable access
                    // is writable; a collection in a data field not declared
                    // `mut` is a legacy read-only edge.
                    if !self.is_struct_ty(it) {
                        return self.is_struct_ty(ct) && self.has_mut_access(it);
                    }
                    cur = inner;
                }
                Tag::CallValue => {
                    return self.deref_source(Ref(data[0])).is_some_and(|(_, m)| m);
                }
                Tag::Intrinsic => {
                    if !matches!(
                        IntrinsicOp::from_u32(data[0]),
                        Some(
                            IntrinsicOp::ListIndex
                                | IntrinsicOp::ListIndexMut
                                | IntrinsicOp::MapIndex
                                | IntrinsicOp::MapIndexMut
                        )
                    ) {
                        return false;
                    }
                    let args = self.b.body_mut().record(data[1]).to_vec();
                    let Some(&c) = args.first() else {
                        return false;
                    };
                    cur = Ref(c);
                }
                Tag::LocalGet => {
                    let l = LocalId::from_raw(data[0]);
                    return self.mut_locals.contains(&l);
                }
                Tag::GlobalGet => {
                    let Some(&def) = self.b.body_mut().record(data[0]).first() else {
                        return false;
                    };
                    let init = self.cx.init.borrow();
                    return init
                        .globals
                        .values()
                        .any(|g| g.def.raw() == def && g.mutable)
                        || init
                            .earlier
                            .values()
                            .any(|g| g.def.raw() == def && g.mutable);
                }
                _ => return false,
            }
        }
        false
    }

    /// The type of the read-only reference a place `r` is reached through,
    /// when it starts at a read through `&T`.
    fn readonly_ref_root(&mut self, r: Ref) -> Option<Ty> {
        let mut cur = r;
        for _ in 0..64 {
            let i = cur.as_inst()?;
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            match tag {
                Tag::Coerce | Tag::Field => cur = Ref(data[0]),
                Tag::Intrinsic => cur = Ref(*self.b.body_mut().record(data[1]).first()?),
                Tag::CallValue => {
                    let (src, m) = self.deref_source(Ref(data[0]))?;
                    return (!m).then(|| self.b.ty_of(src));
                }
                _ => return None,
            }
        }
        None
    }

    /// The local a place `r` starts at, when it starts at one: the binding
    /// a fix-it makes writable.
    fn place_root_local(&mut self, r: Ref) -> Option<LocalId> {
        let mut cur = r;
        for _ in 0..64 {
            let i = cur.as_inst()?;
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            match tag {
                Tag::Coerce | Tag::Field => cur = Ref(data[0]),
                Tag::Intrinsic => cur = Ref(*self.b.body_mut().record(data[1]).first()?),
                Tag::LocalGet => return Some(LocalId::from_raw(data[0])),
                _ => return None,
            }
        }
        None
    }

    /// A change to the collection `recv` of type `rt` (a `mut self`
    /// method such as `push`, an element store, a field write through an
    /// element): whether it may write, marking the path it writes through
    /// (`types.coll.writable`, `types.coll.nested`). Reports when it may
    /// not, with the fix-it that makes the binding writable: a call is
    /// `mutable-receiver-required`, a store `readonly-root`. `what` names
    /// the change.
    pub(crate) fn coll_change(&mut self, recv: Ref, rt: Ty, what: &str, at: NodeRef<'_>) -> bool {
        let code = if what == "this call" {
            hd_diag::Code::MutableReceiverRequired
        } else {
            hd_diag::Code::ReadonlyRoot
        };
        if recv.0 == NONE || matches!(self.strip_mut(rt), Ty::NEVER | Ty::POISON) {
            return true;
        }
        if self.place_writable_quiet(recv) || self.is_fresh(recv) {
            self.mark_write_path(recv);
            self.note_change_in_closure(recv);
            return true;
        }
        let shown = self.show(rt);
        // `ref.write.index`: through `&T` the referenced value is read-only.
        if let Some(reft) = self.readonly_ref_root(recv) {
            let msg = format!(
                "{what} changes this {shown} through a read-only reference {}; take the reference with `&mut` to write through it",
                self.show(reft)
            );
            self.err(code, at, &msg);
            return false;
        }
        let root = self.place_root_local(recv);
        let Some(l) = root else {
            let msg = format!(
                "{what} changes this {shown} in place, but it is not a variable or a field of one; bind it with `let mut` first"
            );
            self.err(code, at, &msg);
            return false;
        };
        let (sym, flags, syn) = {
            let body = self.b.body_mut();
            (
                body.local_name[l.idx()],
                body.local_flags[l.idx()],
                body.local_syn[l.idx()],
            )
        };
        let binding = self.cx.names.text(sym).to_owned();
        let short = flags & local_flags::SHORT != 0;
        let using = self.using_locals.contains(&l);
        let param = flags & local_flags::PARAM != 0;
        let direct = recv
            .as_inst()
            .is_some_and(|i| self.b.body_mut().tags[i.0 as usize] == Tag::LocalGet);
        let msg = if !direct {
            format!(
                "{what} changes a {shown} inside `{binding}`, but `{binding}` is not writable; bind it with `let mut`"
            )
        } else if param && binding == "self" {
            format!(
                "{what} changes `self`, which is this method's own read-only copy; take `&mut self` to change the caller's value"
            )
        } else if param {
            format!(
                "{what} changes parameter `{binding}`, which is the function's own copy and never changes; write `mut {binding}`"
            )
        } else if short || using {
            format!(
                "{what} changes `{binding}` in place, but `{binding}` never changes; bind it with `let mut`"
            )
        } else {
            format!(
                "{what} changes `{binding}` in place, but `{binding}` is not `let mut`; bind it with `let mut`"
            )
        };
        self.err(code, at, &msg);
        if !using {
            self.writable_binding_fix(&binding, short, param, syn);
        }
        false
    }

    /// The fix-it of `&m[k]` (`types.coll.map-value`): the statement is
    /// preceded by `let mut entry = m[k]` and refers to `entry` instead.
    pub(crate) fn map_value_ref_fix(&mut self, e: NodeRef<'_>) {
        let Some(stmt) = self.cur_stmt else {
            return;
        };
        let src = self.cx.src;
        let sp = src.span(src.parse.tree.node(stmt));
        let ep = src.span(e);
        let text: &str = src.text;
        let (lo, elo, ehi) = (sp.lo as usize, ep.lo as usize, ep.hi as usize);
        if elo < lo || ehi > text.len() {
            return;
        }
        let line = text[..lo].rfind('\n').map_or(0, |p| p + 1);
        let indent = &text[line..lo];
        if !indent.chars().all(|c| c == ' ') {
            return;
        }
        let at = |lo: u32, hi: u32| hd_base::Span {
            file: sp.file,
            lo,
            hi,
        };
        let first = format!("let mut entry = {}\n{indent}", &text[elo..ehi]);
        self.diags.add_fix(
            "copy the value into a `let mut` and refer to it",
            hd_diag::FixSafety::Suggestion,
            &[(at(sp.lo, sp.lo), first.as_str()), (ep, "entry")],
        );
    }
}
