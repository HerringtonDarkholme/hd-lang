//! Collection values (spec 04 "Collection Values", the tier-1 design `58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md`
//! §6.2): `List[T]` and `Map[K, V]` are values with copy-on-write. The
//! checker places no copies: every count of a collection the backend takes
//! runs its copy glue (`docs/compiler/tir-v2.md`, "List and Map buffers",
//! 3g).
//!
//! Changing a collection needs a writable place, as changing a struct's
//! field does.
//!
//! A map value is a place, as a list element is
//! (`types.coll.map-value.place`): a change written through a path
//! (`m[k].push(x)`) reaches the value in the map, since insertion makes
//! each collection on a change's path unique from the reads the change
//! goes through.

use hd_base::LocalId;
use hd_syntax::NodeRef;
use hd_tir::ir::{IntrinsicOp, NONE, Ref, Tag, local_flags};
use hd_types::{Ty, TyData};

use crate::access::Projection;
use crate::body::Ck;

impl Ck<'_, '_> {
    /// Whether `t` is a `List` or `Map` type.
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

    /// Whether `r` is a value the expression made itself, which no place
    /// holds: a literal, a constructor, a call's result, a copy.
    pub(crate) fn is_fresh(&mut self, r: Ref) -> bool {
        let Some(i) = r.as_inst() else {
            return true;
        };
        let (tag, data) = {
            let body = self.b.body_mut();
            (body.tags[i.0 as usize], body.data[i.0 as usize])
        };
        match tag {
            Tag::NewList | Tag::NewMap | Tag::NewStruct | Tag::CopyStruct | Tag::NewVariant => true,
            Tag::NewTuple | Tag::Closure | Tag::Interp | Tag::Prim => true,
            Tag::Call | Tag::CallDyn | Tag::DefaultCall => true,
            Tag::CallValue | Tag::Ref | Tag::RefDyn | Tag::RefIdentity => true,
            Tag::Intrinsic => IntrinsicOp::from_u32(data[0]) == Some(IntrinsicOp::Item),
            Tag::Coerce => self.is_fresh(Ref(data[0])),
            _ => false,
        }
    }

    /// Whether top-level binding `def` is a `let mut` binding, of this
    /// module's init or an earlier one.
    fn global_mutable(&self, def: hd_base::DefId) -> bool {
        let init = self.cx.init.borrow();
        init.globals.values().any(|g| g.def == def && g.mutable)
            || init.earlier.values().any(|g| g.def == def && g.mutable)
    }

    /// Whether the value `br` reads a place a change may write through
    /// (`types.coll.writable`), judged without reporting: a `let mut`
    /// local or `mut` parameter, a writable top-level binding, a value
    /// read through `&mut`, an element of a writable list or a value of a
    /// writable map, a struct field of any of these, or a value whose type
    /// already grants `mut` (a trait value read through `&mut`).
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
            if let Some(Projection::Field(inner)) = self.projection(cur) {
                let it = self.b.ty_of(inner);
                // A struct field is part of its struct's place.
                if !self.is_struct_ty(it) {
                    return self.is_struct_ty(ct) && self.has_mut_access(it);
                }
                cur = inner;
                continue;
            }
            if let Some((list, _)) = self.element_of(cur) {
                cur = list;
                continue;
            }
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::Read if let Some(l) = self.b.body_mut().local_read(i.0 as usize) => {
                    return self.mut_locals.contains(&l);
                }
                Tag::Read if let Some(def) = self.b.body_mut().global_read(i.0 as usize) => {
                    return self.global_mutable(def);
                }
                Tag::Read => {
                    return self.read_through(cur).is_some_and(|(_, m, _)| m);
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
            if let Some(Projection::Field(base)) = self.projection(cur) {
                cur = base;
                continue;
            }
            if let Some((list, _)) = self.element_of(cur) {
                cur = list;
                continue;
            }
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::Intrinsic => cur = Ref(*self.b.body_mut().record(data[1]).first()?),
                Tag::Read => {
                    let (src, m, _) = self.read_through(cur)?;
                    return (!m).then(|| self.b.ty_of(src));
                }
                _ => return None,
            }
        }
        None
    }

    /// The local a place `r` starts at, when it starts at one: the binding
    /// a fix-it makes writable.
    pub(crate) fn place_root_local(&mut self, r: Ref) -> Option<LocalId> {
        let mut cur = r;
        for _ in 0..64 {
            let i = cur.as_inst()?;
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            if let Some(Projection::Field(base)) = self.projection(cur) {
                cur = base;
                continue;
            }
            if let Some((list, _)) = self.element_of(cur) {
                cur = list;
                continue;
            }
            match tag {
                Tag::Coerce => cur = Ref(data[0]),
                Tag::Intrinsic => cur = Ref(*self.b.body_mut().record(data[1]).first()?),
                Tag::Read => return self.b.body_mut().local_read(i.0 as usize),
                _ => return None,
            }
        }
        None
    }

    /// A change to the collection `recv` of type `rt` (a `mut self`
    /// method such as `push`, an element store, a field write through an
    /// element): whether it may write (`types.coll.writable`,
    /// `types.coll.nested`). Reports when it may
    /// not, `not-writable`, with the fix-it that makes the binding
    /// writable. `what` names the change.
    pub(crate) fn coll_change(&mut self, recv: Ref, rt: Ty, what: &str, at: NodeRef<'_>) -> bool {
        let code = hd_diag::Code::NotWritable;
        if recv.0 == NONE || matches!(self.strip_mut(rt), Ty::NEVER | Ty::POISON) {
            return true;
        }
        if self.place_writable_quiet(recv) || self.is_fresh(recv) {
            if let Some(l) = self.place_root_local(recv) {
                self.note_capture_write_of(l);
                self.changed_locals.push(l);
                self.note_param_write(l, at.index());
            }
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
            .is_some_and(|i| self.b.body_mut().local_read(i.0 as usize).is_some());
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
}
