//! Copy-on-write collections on Wasm GC (`crates/hd_wasm/DESIGN.md` §15.2,
//! "Collection values"; spec/lang/04-type-system.md, "Collection
//! Values"). A list or map value is one header object over its arrays.
//! A copy (`ValueCopy`) makes a new header over the same arrays and sets
//! bit 0 of both headers' `cow` word; a `for` over the collection adds 2
//! to it for as long as it runs (`SnapBegin`, `SnapEnd`). A change to a
//! header whose `cow` is not 0 first copies its arrays, copying each
//! element that holds a collection in turn, and clears `cow`. With no
//! count of the headers sharing the arrays, the mark never clears on its
//! own: a list copied once copies its arrays at its next change even when
//! the other copy is gone.
//!
//! A list header also counts the element references into it, in its `pin`
//! word (§15.2, "Element references"): `ListPin` makes the arrays the
//! header's own and counts one more, `ListUnpin` one fewer, and a length
//! change while it is not 0 panics `collection-pinned`.

use hd_tir::ir::{IntrinsicOp, Tag};
use hd_types::{Ty, TyData};

use super::{Em, u32_of};
use crate::layout::{M_HASHES, M_KEYS, M_USED, Shape, list_cow, list_pin, map_cow, storage};
use crate::{VT, WTy, unsupported};
use hd_base::StageResult;

impl Em<'_> {
    /// Whether copying a value of type `t` copies collections: a `List` or
    /// a `Map` value, or a struct that holds one in its fields, at any
    /// depth of struct fields. A legacy `mut List[T]` view shares its list
    /// and is no value.
    pub(super) fn holds_coll(&self, t: Ty) -> bool {
        hd_mono::holds_coll(self.pool(), self.env(), t)
    }

    /// Whether the operand `r` is a value its own instruction made, which
    /// no place holds: a literal, a constructor, a call's result whose
    /// type is no legacy `mut` view, a copy. Reading through a reference
    /// (`Ref[T]`'s `get`) is no such call.
    fn fresh_operand(&self, r: u32) -> bool {
        if hd_tir::ir::Ref(r).as_inst().is_none() {
            return true;
        }
        let i = r as usize;
        let legacy = matches!(self.pool().get(self.ty_of(r)), TyData::Mut(_));
        match self.b.tags[i] {
            Tag::NewList
            | Tag::NewMap
            | Tag::NewData
            | Tag::CopyData
            | Tag::NewVariant
            | Tag::NewTuple
            | Tag::Closure
            | Tag::Interp
            | Tag::Prim => true,
            Tag::Call | Tag::CallDyn | Tag::DefaultCall => !legacy,
            Tag::CallValue => {
                let f = self.b.data[i][0];
                let deref = hd_tir::ir::Ref(f).as_inst().is_some()
                    && self.b.tags[f as usize] == Tag::Field
                    && self.b.data[f as usize][1] == 0
                    && matches!(
                        self.pool().get(self.lay.strip(self.ty_of(self.b.data[f as usize][0]))),
                        TyData::Adt { def, .. } if def == self.env().ref_cell()
                    );
                !deref && !legacy
            }
            Tag::Intrinsic => {
                matches!(
                    IntrinsicOp::from_u32(self.b.data[i][0]),
                    Some(IntrinsicOp::ValueCopy | IntrinsicOp::Item)
                ) && !legacy
            }
            Tag::Coerce => self.fresh_operand(self.b.data[i][0]),
            _ => false,
        }
    }

    /// After the operand `r` is pushed for a slot of type `slot` (an
    /// element, a field, a payload), copies it when the slot holds a
    /// collection and `r` is held by some place (`types.coll.value`). A
    /// legacy data type's `List[T]` field shares its list, as a read-only
    /// view did; only a struct value in such a field is copied.
    pub(super) fn copy_slot(&mut self, r: u32, slot: Ty, in_data: bool) -> StageResult<()> {
        if !self.holds_coll(slot) || self.fresh_operand(r) {
            return Ok(());
        }
        if in_data
            && matches!(
                self.lay.shape(slot)?,
                Shape::List { .. } | Shape::Map { .. }
            )
        {
            return Ok(());
        }
        self.copy_top(slot)
    }

    /// The field types of the data or struct type `t`, substituted, and
    /// whether it is a legacy data type rather than a struct.
    pub(super) fn field_tys(&self, t: Ty) -> (Vec<Ty>, bool) {
        let (pool, env) = (self.pool(), self.env());
        let TyData::Adt { def, args } = pool.get(self.lay.erase(t)) else {
            return (Vec::new(), true);
        };
        let tys = env
            .data_fields(def)
            .unwrap_or_default()
            .iter()
            .map(|f| hd_mono::subst(pool, env, def, args, *f))
            .collect();
        (tys, !env.is_struct(def))
    }

    /// The slot type of each member of a tuple type, or each payload of
    /// variant `v` of an enum type.
    pub(super) fn member_tys(&self, t: Ty, variant: Option<u32>) -> Vec<Ty> {
        let (pool, env) = (self.pool(), self.env());
        match pool.get(self.lay.erase(t)) {
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().copied().chain(rest).collect()
            }
            TyData::Option(inner) => vec![inner],
            TyData::Adt { def, args } => variant
                .and_then(|v| env.enum_variants(def, args)?.get(v as usize).cloned())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Replaces the value of type `t` on the stack by its copy
    /// (`ValueCopy`): a list or a map gets a new header over the same
    /// arrays, both headers marked; a struct is rebuilt with each field
    /// that holds a collection copied. Any other value is its own copy.
    pub(super) fn copy_top(&mut self, t: Ty) -> StageResult<()> {
        if !self.holds_coll(t) {
            return Ok(());
        }
        match self.lay.shape(t)? {
            Shape::List { elem, ty } => self.share_header(&ty, 1, list_cow(elem.len()), 1, true),
            Shape::Map { key, val, ty } => {
                self.share_header(&ty, 1, map_cow(key.len() + val.len()), 1, false);
            }
            Shape::Data { ty, fields } => {
                let TyData::Adt { def, args } = self.pool().get(self.lay.erase(t)) else {
                    return unsupported("a struct copy of a type that is not data");
                };
                let ftys: Vec<Ty> = self
                    .env()
                    .data_fields(def)
                    .unwrap_or_default()
                    .iter()
                    .map(|f| hd_mono::subst(self.pool(), self.env(), def, args, *f))
                    .collect();
                let s = self.a.local(VT::r(ty.clone()));
                self.a.set(s);
                for ((first, vts), ft) in fields.iter().zip(ftys) {
                    for k in 0..vts.len() {
                        self.a.get(s);
                        self.a.struct_get(&ty, first + u32_of(k));
                    }
                    if vts.len() == 1 {
                        self.copy_top(ft)?;
                    }
                }
                self.a.struct_new(&ty);
            }
            _ => {}
        }
        Ok(())
    }

    /// Replaces the collection header on the stack by a new header over
    /// the same fields, its `cow` field set to `mark` (and a list's `pin`
    /// to 0), and adds `add` to the old header's `cow` (bit 0 for a copy, 2
    /// for a loop, which `share_header` sets with `add` 2 and `mark` 1).
    fn share_header(&mut self, ty: &WTy, add: i32, cow: u32, mark: i32, pin: bool) {
        let h = self.a.local(VT::r(ty.clone()));
        self.a.set(h);
        self.a.get(h);
        self.a.get(h);
        self.a.struct_get(ty, cow);
        self.a.i32(add);
        if add == 1 {
            self.a.s().i32_or();
        } else {
            self.a.s().i32_add();
        }
        self.a.struct_set(ty, cow);
        for f in 0..cow {
            self.a.get(h);
            self.a.struct_get(ty, f);
        }
        self.a.i32(mark);
        if pin {
            self.a.i32(0);
        }
        self.a.struct_new(ty);
    }

    /// Makes the arrays of the list in local `l` its own before a change
    /// (`ListSet`, `ListIndexMut`), or before `ListPush` stores at the
    /// length held in local `n`: when its `cow` is not 0, or when `push` is
    /// given and the arrays are full, each array is copied into a new one
    /// (twice as large plus 4 for a push), each element holding a
    /// collection is copied in turn, and `cow` is cleared.
    pub(super) fn list_reserve(
        &mut self,
        l: u32,
        list_ty: Ty,
        n: u32,
        push: bool,
    ) -> StageResult<()> {
        let Shape::List { elem, ty: lt } = self.lay.shape(list_ty)? else {
            return unsupported("a list change of a non-list");
        };
        if elem.is_empty() {
            return Ok(());
        }
        let cow = list_cow(elem.len());
        let cap = self.a.local(VT::I32);
        self.a.get(l);
        self.a.struct_get(&lt, cow);
        if push {
            self.a.get(n);
            self.a.get(l);
            self.a.struct_get(&lt, 1);
            self.a.array_len();
            self.a.s().i32_eq();
            self.a.s().i32_or();
        }
        self.a.if_();
        if push {
            self.a.get(n);
            self.a.i32(2);
            self.a.s().i32_mul();
            self.a.i32(4);
            self.a.s().i32_add();
        } else {
            self.a.get(l);
            self.a.struct_get(&lt, 1);
            self.a.array_len();
        }
        self.a.set(cap);
        for (k, v) in elem.iter().enumerate() {
            let arr = WTy::Array(storage(v).dflt());
            let fresh = self.a.local(VT::r(arr.clone()));
            self.a.get(cap);
            self.a.array_new_default(&arr);
            self.a.set(fresh);
            self.a.get(fresh);
            self.a.i32(0);
            self.a.get(l);
            self.a.struct_get(&lt, 1 + u32_of(k));
            self.a.i32(0);
            self.a.get(n);
            self.a.array_copy(&arr, &arr);
            self.a.get(l);
            self.a.get(fresh);
            self.a.struct_set(&lt, 1 + u32_of(k));
        }
        let et = self.elem_ty(list_ty, 0)?;
        if elem.len() == 1 && self.holds_coll(et) {
            self.a.get(l);
            self.a.struct_get(&lt, cow);
            self.a.if_();
            self.copy_slots(l, &lt, 1, n, None, (&elem[0], et))?;
            self.a.end();
        }
        self.a.get(l);
        self.a.i32(0);
        self.a.struct_set(&lt, cow);
        self.a.end();
        Ok(())
    }

    /// Copies each value in slots `0..n` (the local `n`) of array field
    /// `field` of the collection in local `c`, in place: the copy that
    /// copying the arrays owes each element holding a collection. With
    /// `live` (a map's hashes field), a removed entry's slot is skipped.
    fn copy_slots(
        &mut self,
        c: u32,
        ct: &WTy,
        field: u32,
        n: u32,
        live: Option<u32>,
        (v, et): (&VT, Ty),
    ) -> StageResult<()> {
        let st = storage(v).dflt();
        let arr = WTy::Array(st.clone());
        let j = self.a.local(VT::I32);
        self.a.i32(0);
        self.a.set(j);
        self.a.block();
        self.a.loop_();
        self.a.get(j);
        self.a.get(n);
        self.a.s().i32_ge_u();
        self.a.br_if(1);
        if let Some(h) = live {
            self.a.get(c);
            self.a.struct_get(ct, h);
            self.a.get(j);
            self.a.array_get(&WTy::Array(VT::I32));
            self.a.i32(0);
            self.a.s().i32_ge_s();
            self.a.if_();
        }
        self.a.get(c);
        self.a.struct_get(ct, field);
        self.a.get(j);
        self.a.get(c);
        self.a.struct_get(ct, field);
        self.a.get(j);
        self.a.array_get(&arr);
        self.a.conv(&st, v);
        self.copy_top(et)?;
        self.a.array_set(&arr);
        if live.is_some() {
            self.a.end();
        }
        self.a.get(j);
        self.a.i32(1);
        self.a.s().i32_add();
        self.a.set(j);
        self.a.br(0);
        self.a.end();
        self.a.end();
        Ok(())
    }

    /// Copies each value in slots `from..to` (two locals) of the array in
    /// local `arr` of type `at`, when its elements hold collections: a
    /// spread copied them out of another list.
    pub(super) fn copy_range(
        &mut self,
        arr: u32,
        at: &WTy,
        (from, to): (u32, u32),
        v: &VT,
        et: Ty,
    ) -> StageResult<()> {
        if !self.holds_coll(et) {
            return Ok(());
        }
        let st = storage(v).dflt();
        let j = self.a.local(VT::I32);
        self.a.get(from);
        self.a.set(j);
        self.a.block();
        self.a.loop_();
        self.a.get(j);
        self.a.get(to);
        self.a.s().i32_ge_u();
        self.a.br_if(1);
        self.a.get(arr);
        self.a.get(j);
        self.a.get(arr);
        self.a.get(j);
        self.a.array_get(at);
        self.a.conv(&st, v);
        self.copy_top(et)?;
        self.a.array_set(at);
        self.a.get(j);
        self.a.i32(1);
        self.a.s().i32_add();
        self.a.set(j);
        self.a.br(0);
        self.a.end();
        self.a.end();
        Ok(())
    }

    /// Type argument `k` of the collection type `t`, if it has one.
    pub(super) fn coll_arg(&self, t: Ty, k: usize) -> Option<Ty> {
        self.elem_ty(t, k).ok()
    }

    /// Type argument `k` of the collection type `t`.
    fn elem_ty(&self, t: Ty, k: usize) -> StageResult<Ty> {
        match self.pool().get(self.lay.erase(t)) {
            TyData::Adt { args, .. } => match self.pool().list_items(args).get(k) {
                Some(e) => Ok(*e),
                None => unsupported("a collection without its type argument"),
            },
            _ => unsupported("a collection type that is not one"),
        }
    }

    /// Makes the arrays of the map in local `m` its own before a change
    /// (`MapSet`, `MapRemove`, `MapIndexMut`): when its `cow` is not 0,
    /// every array is copied, each live value holding a collection is
    /// copied in turn, and `cow` is cleared.
    pub(super) fn map_unique(&mut self, m: u32, map_ty: Ty) -> StageResult<()> {
        let Shape::Map { key, val, ty: mt } = self.lay.shape(map_ty)? else {
            return unsupported("a map change of a non-map");
        };
        let cow = map_cow(key.len() + val.len());
        self.a.get(m);
        self.a.struct_get(&mt, cow);
        self.a.if_();
        let ints = WTy::Array(VT::I32);
        let mut arrays = vec![(crate::layout::M_INDEX, ints.clone()), (M_HASHES, ints)];
        for (k, v) in key.iter().chain(&val).enumerate() {
            arrays.push((M_KEYS + u32_of(k), WTy::Array(storage(v).dflt())));
        }
        for (f, arr) in &arrays {
            let (old, fresh) = (
                self.a.local(VT::r(arr.clone())),
                self.a.local(VT::r(arr.clone())),
            );
            self.a.get(m);
            self.a.struct_get(&mt, *f);
            self.a.set(old);
            self.a.get(old);
            self.a.array_len();
            self.a.array_new_default(arr);
            self.a.set(fresh);
            self.a.get(fresh);
            self.a.i32(0);
            self.a.get(old);
            self.a.i32(0);
            self.a.get(old);
            self.a.array_len();
            self.a.array_copy(arr, arr);
            self.a.get(m);
            self.a.get(fresh);
            self.a.struct_set(&mt, *f);
        }
        let vt = self.elem_ty(map_ty, 1)?;
        if val.len() == 1 && self.holds_coll(vt) {
            let used = self.a.local(VT::I32);
            self.a.get(m);
            self.a.struct_get(&mt, M_USED);
            self.a.set(used);
            let field = M_KEYS + u32_of(key.len());
            self.copy_slots(m, &mt, field, used, Some(M_HASHES), (&val[0], vt))?;
        }
        self.a.get(m);
        self.a.i32(0);
        self.a.struct_set(&mt, cow);
        self.a.end();
        Ok(())
    }

    /// `SnapBegin`: the header a counted `for` iterates, over the
    /// collection's arrays, whose `cow` gains 2 while the loop runs.
    pub(super) fn snap_begin(&mut self, i: u32, coll: u32) -> StageResult<()> {
        let t = self.ty_of(coll);
        match self.lay.shape(t)? {
            Shape::List { elem, ty } => {
                self.comp(coll, 0, &VT::r(ty.clone()))?;
                self.share_header(&ty, 2, list_cow(elem.len()), 1, true);
            }
            Shape::Map { key, val, ty } => {
                self.comp(coll, 0, &VT::r(ty.clone()))?;
                self.share_header(&ty, 2, map_cow(key.len() + val.len()), 1, false);
            }
            _ => return unsupported("a loop snapshot of a non-collection"),
        }
        self.store(i)
    }

    /// Panics `collection-pinned` when the list in local `l` is pinned
    /// (`ref.elem.pinned`): before a `push`, or before a truncation to the
    /// length in local `to` that changes the length.
    pub(super) fn pin_check(&mut self, l: u32, lt: &WTy, arrays: usize, to: Option<u32>) {
        self.a.get(l);
        self.a.struct_get(lt, list_pin(arrays));
        if let Some(to) = to {
            self.a.get(l);
            self.a.struct_get(lt, 0);
            self.a.get(to);
            self.a.s().i32_ne();
            self.a.s().i32_and();
        }
        self.a.if_();
        self.panic("collection-pinned: list length changed while pinned");
        self.a.end();
    }

    /// `ListPin`: the list an element reference points into, its index
    /// (when given) checked, its arrays made its own, and one more pin
    /// counted (`ref.elem.pin`).
    pub(super) fn list_pin(&mut self, i: u32, args: &[u32]) -> StageResult<()> {
        let lty = self.ty_of(args[0]);
        let Shape::List { elem, ty: lt } = self.lay.shape(lty)? else {
            return unsupported("a pin of a non-list");
        };
        let (l, n) = (self.a.local(VT::r(lt.clone())), self.a.local(VT::I32));
        self.comp(args[0], 0, &VT::r(lt.clone()))?;
        self.a.set(l);
        if let Some(&index) = args.get(1) {
            let ix = self.a.local(VT::I32);
            self.comp(index, 0, &VT::I32)?;
            self.a.set(ix);
            self.bounds(l, &lt, ix);
        }
        self.a.get(l);
        self.a.struct_get(&lt, 0);
        self.a.set(n);
        self.list_reserve(l, lty, n, false)?;
        let pin = list_pin(elem.len());
        self.a.get(l);
        self.a.get(l);
        self.a.struct_get(&lt, pin);
        self.a.i32(1);
        self.a.s().i32_add();
        self.a.struct_set(&lt, pin);
        self.a.get(l);
        self.store(i)
    }

    /// `ListUnpin`: one pin released (`ref.elem.release`).
    pub(super) fn list_unpin(&mut self, list: u32) -> StageResult<()> {
        let lty = self.ty_of(list);
        let Shape::List { elem, ty: lt } = self.lay.shape(lty)? else {
            return unsupported("an unpin of a non-list");
        };
        let (l, pin) = (self.a.local(VT::r(lt.clone())), list_pin(elem.len()));
        self.comp(list, 0, &VT::r(lt.clone()))?;
        self.a.set(l);
        self.a.get(l);
        self.a.get(l);
        self.a.struct_get(&lt, pin);
        self.a.i32(1);
        self.a.s().i32_sub();
        self.a.struct_set(&lt, pin);
        Ok(())
    }

    /// `SnapEnd`: the loop over `snap` ended. Unless a change copied the
    /// collection's arrays meanwhile, which cleared its `cow`, the loop's 2
    /// comes off again.
    pub(super) fn snap_end(&mut self, coll: u32, snap: u32) -> StageResult<()> {
        let t = self.ty_of(coll);
        let (ty, cow, probe) = match self.lay.shape(t)? {
            Shape::List { elem, ty } if elem.is_empty() => {
                let _ = ty;
                return Ok(());
            }
            Shape::List { elem, ty } => (ty, list_cow(elem.len()), 1),
            Shape::Map { key, val, ty } => (ty, map_cow(key.len() + val.len()), M_HASHES),
            _ => return unsupported("a loop snapshot of a non-collection"),
        };
        let h = self.a.local(VT::r(ty.clone()));
        self.comp(coll, 0, &VT::r(ty.clone()))?;
        self.a.set(h);
        self.a.get(h);
        self.a.struct_get(&ty, probe);
        self.comp(snap, 0, &VT::r(ty.clone()))?;
        self.a.struct_get(&ty, probe);
        self.a.s().ref_eq();
        self.a.if_();
        self.a.get(h);
        self.a.get(h);
        self.a.struct_get(&ty, cow);
        self.a.i32(2);
        self.a.s().i32_sub();
        self.a.struct_set(&ty, cow);
        self.a.end();
        Ok(())
    }
}
