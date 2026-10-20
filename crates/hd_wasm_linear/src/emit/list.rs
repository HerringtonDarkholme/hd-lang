//! Lists (`crates/hd_wasm/DESIGN.md` §23.7, §23.17, job 9). A list value
//! is the address of a counted header that holds its buffer's address and
//! its owner word (§23.20); the buffer is a counted object holding the
//! capacity, the length and the elements inline. A header never moves, so
//! a change through it reaches every place that holds the same list, as
//! the Wasm GC backend's header does; a copy (`ValueCopy`) is a new header
//! over the same buffer, whose count is its number of owners. A change
//! makes the buffer the list's own first (`rt:list_unique`): a shared one
//! is copied, and the copied elements' collections are copied in turn, so
//! a nested write never reaches another list.

use hd_base::StageResult;
use hd_tir::ir::{IntrinsicOp, Ref, Tag};
use hd_types::{Ty, TyData};
use wasm_encoder::{BlockType, Instruction as I};

use super::Em;
use super::value::Src;
use crate::layout::{coll_leaves, mem as mem_of, strip};
use crate::module::{HEAP, Ins, Sym, Val, mem};
use crate::rt::{Elem, Helper, LIST_ELEMS, LIST_LEN, LIST_OWN, empty_buffer};
use crate::unsupported;

impl Em<'_, '_, '_> {
    /// The element type of list type `t` and its layout for the glue.
    pub(super) fn elem(&self, t: Ty) -> StageResult<(Ty, Elem)> {
        if self.input.runtime.is_none() {
            return unsupported("lists without std's allocator");
        }
        let TyData::Adt { args, .. } = self.pool().get(strip(self.pool(), self.env(), t)) else {
            return unsupported("a list operation on a value that is not a list");
        };
        let Some(&et) = self.pool().list_items(args).first() else {
            return unsupported("a list without its element type");
        };
        let m = mem_of(self.pool(), self.env(), et)?;
        let elem = Elem {
            size: m.size,
            leaves: m.leaves,
            walk: self.counted(et),
            colls: coll_leaves(self.pool(), self.env(), et),
        };
        Ok((et, elem))
    }

    /// A local holding list value `r`'s header.
    pub(super) fn header(&mut self, r: u32) -> StageResult<u32> {
        let h = self.f.local(Val::I32);
        self.list_addr(r)?;
        self.f.set(h);
        Ok(h)
    }

    /// Pushes the header address of list value `r`.
    pub(super) fn list_addr(&mut self, r: u32) -> StageResult<()> {
        self.comp(r, 0)
    }

    /// A local holding the buffer of the list header in local `h`.
    fn buffer(&mut self, h: u32) -> u32 {
        let b = self.f.local(Val::I32);
        self.f.get(h);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.set(b);
        b
    }

    /// A local holding index `r`, checked against the length of the
    /// buffer in local `b`: an index past it is `index-out-of-bounds`, a
    /// 64-bit one before it is narrowed (`expr.index.list.range`).
    fn index(&mut self, b: u32, r: u32) -> StageResult<u32> {
        let ix = self.f.local(Val::I32);
        let wide = self.vals(self.ty_of(r))? == [Val::I64];
        let w = self.f.local(if wide { Val::I64 } else { Val::I32 });
        self.comp(r, 0)?;
        self.f.set(w);
        self.f.get(w);
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        if wide {
            self.f.op(I::I64ExtendI32U);
            self.f.op(I::I64GeU);
        } else {
            self.f.op(I::I32GeU);
        }
        self.f.op(I::If(BlockType::Empty));
        self.f.helper(Helper::Panic(
            "index-out-of-bounds: list index out of bounds".into(),
        ));
        self.f.op(I::End);
        self.f.get(w);
        if wide {
            self.f.op(I::I32WrapI64);
        }
        self.f.set(ix);
        Ok(ix)
    }

    /// A local holding the address of element `ix` (a local) of the buffer
    /// in local `b`, `LIST_ELEMS` before the element.
    fn slot_at(&mut self, b: u32, ix: u32, size: u32) -> u32 {
        let at = self.f.local(Val::I32);
        self.f.get(b);
        self.f.get(ix);
        self.f.u32(size);
        self.f.op(I::I32Mul);
        self.f.op(I::I32Add);
        self.f.set(at);
        at
    }

    /// Stores the values of `s` as the element at local `at`.
    fn store_elem(&mut self, at: u32, e: &Elem, s: &Src) -> StageResult<()> {
        for (k, l) in e.leaves.iter().copied().enumerate() {
            self.store_leaf(
                &|em: &mut Self| {
                    em.f.get(at);
                    Ok(())
                },
                LIST_ELEMS,
                l,
                &mut |em: &mut Self| em.src(s, k),
            )?;
        }
        Ok(())
    }

    /// One more count on each counted reference of `s`, a value of type
    /// `t`, for the place that consumes it; an operand at its only use
    /// moves its count there instead ([`Em::take`]).
    pub(super) fn retain_src(&mut self, s: &Src, t: Ty) -> StageResult<()> {
        match s {
            Src::Ref(r) if self.take(*r) => Ok(()),
            Src::Ref(r) => self.retain_ref(*r),
            Src::Locals(ls) => self.retain_locals(ls, t),
        }
    }

    /// Whether operand `r` is a value its own instruction made, which no
    /// place holds: a literal, a constructor, a call's result whose type
    /// is no legacy `mut` view, a copy (as the Wasm GC backend's
    /// `fresh_operand` decides).
    fn fresh(&self, r: u32) -> bool {
        let Some(at) = Ref(r).as_inst() else {
            return true;
        };
        let i = at.0 as usize;
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
            Tag::Intrinsic => {
                matches!(
                    IntrinsicOp::from_u32(self.b.data[i][0]),
                    Some(IntrinsicOp::ValueCopy | IntrinsicOp::Item)
                ) && !legacy
            }
            Tag::Coerce => self.fresh(self.b.data[i][0]),
            _ => false,
        }
    }

    /// The source of operand `r` for a slot of type `slot` (an element, a
    /// field, a payload): its copy when the slot holds a collection and
    /// some place holds `r` (`types.coll.value`). A legacy data type's
    /// list field shares its list (`in_data`); only a struct value in
    /// such a field is copied.
    pub(super) fn slot_src(&mut self, r: u32, slot: Ty, in_data: bool) -> StageResult<Src> {
        if coll_leaves(self.pool(), self.env(), slot).is_empty() || self.fresh(r) {
            return Ok(Src::Ref(r));
        }
        if in_data
            && matches!(self.pool().get(strip(self.pool(), self.env(), slot)), TyData::Adt { def, .. }
                if self.env().std_kind(def) != hd_mono::layout::StdKind::Other)
        {
            return Ok(Src::Ref(r));
        }
        self.copy_src(r, slot)
    }

    /// Value `r` of type `t` with each collection in it replaced by its
    /// copy. Each copy's own count belongs to a temporary that the
    /// function's exit releases (in a loop, the next iteration's copy),
    /// so the place it is stored in takes a count of its own as for any
    /// value.
    fn copy_src(&mut self, r: u32, t: Ty) -> StageResult<Src> {
        let Some(mut ls) = self.vals.get(&r).cloned() else {
            return unsupported("a value its instruction did not compute");
        };
        for c in coll_leaves(self.pool(), self.env(), t) {
            let tmp = self.f.local(Val::I32);
            if self.loops > 0 {
                self.f.get(tmp);
                self.f.helper(Helper::ReleaseRef);
            }
            self.f.get(ls[c as usize]);
            self.f.helper(Helper::ListCopy);
            self.f.set(tmp);
            self.temps.push(tmp);
            ls[c as usize] = tmp;
        }
        Ok(Src::Locals(ls))
    }

    /// `NewList`: a buffer sized exactly for the items and the spreads
    /// (`expr.list.spread.insert`), each item stored and each spread's
    /// elements copied in order, and a new header over it. An empty
    /// literal shares the static empty buffer.
    pub(super) fn new_list(&mut self, i: u32, spreads: u32, items: u32) -> StageResult<()> {
        let (et, e) = self.elem(self.ty(i))?;
        let items = self.rec(items);
        let spreads = if spreads == hd_tir::ir::NONE {
            Vec::new()
        } else {
            self.rec(spreads)
        };
        let is_spread = |k: usize| spreads.iter().any(|s| *s as usize == k);
        if items.is_empty() {
            let at = empty_buffer(self.m);
            self.f.u32(at);
        } else {
            let n = self.f.local(Val::I32);
            self.f
                .u32(u32::try_from(items.len() - spreads.len()).expect("items"));
            self.f.set(n);
            let mut sources = Vec::new();
            for (k, r) in items.iter().enumerate() {
                if !is_spread(k) {
                    continue;
                }
                let h = self.header(*r)?;
                let sb = self.buffer(h);
                self.f.get(n);
                self.f.get(sb);
                self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
                self.f.op(I::I32Add);
                self.f.set(n);
                sources.push(sb);
            }
            let b = self.f.local(Val::I32);
            self.f.get(n);
            self.f.helper(Helper::ListBuf { elem: e.clone() });
            self.f.set(b);
            self.f.get(b);
            self.f.get(n);
            self.f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
            // `at` walks the elements, `LIST_ELEMS` before each.
            let at = self.f.local(Val::I32);
            self.f.get(b);
            self.f.set(at);
            let mut next = sources.into_iter();
            for (k, r) in items.iter().enumerate() {
                if is_spread(k) {
                    let Some(sb) = next.next() else {
                        return unsupported("a list spread without its list");
                    };
                    let bytes = self.f.local(Val::I32);
                    self.f.get(sb);
                    self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
                    self.f.u32(e.size);
                    self.f.op(I::I32Mul);
                    self.f.set(bytes);
                    self.f.get(at);
                    self.f.u32(LIST_ELEMS);
                    self.f.op(I::I32Add);
                    self.f.get(sb);
                    self.f.u32(LIST_ELEMS);
                    self.f.op(I::I32Add);
                    self.f.get(bytes);
                    self.f.op(I::MemoryCopy {
                        src_mem: HEAP,
                        dst_mem: HEAP,
                    });
                    if e.counts() {
                        self.f.get(at);
                        self.f.u32(LIST_ELEMS);
                        self.f.op(I::I32Add);
                        self.f.get(sb);
                        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
                        self.f.helper(Helper::CopyElems { elem: e.clone() });
                    }
                    self.f.get(at);
                    self.f.get(bytes);
                    self.f.op(I::I32Add);
                    self.f.set(at);
                } else {
                    let s = self.slot_src(*r, et, false)?;
                    self.retain_src(&s, et)?;
                    self.store_elem(at, &e, &s)?;
                    self.f.get(at);
                    self.f.u32(e.size);
                    self.f.op(I::I32Add);
                    self.f.set(at);
                }
            }
            self.f.get(b);
        }
        self.f.helper(Helper::ListHeader);
        self.store_owned(i)
    }

    /// `ListLen`.
    pub(super) fn list_len(&mut self, i: u32, list: u32) -> StageResult<()> {
        self.elem(self.ty_of(list))?;
        self.list_addr(list)?;
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        self.store(i)
    }

    /// `ListIndex`, and `ListIndexMut`, which first makes the buffer the
    /// list's own so that a write through the element reaches only it.
    pub(super) fn list_index(&mut self, i: u32, args: &[u32], write: bool) -> StageResult<()> {
        let (_, e) = self.elem(self.ty_of(args[0]))?;
        let h = self.header(args[0])?;
        let mut b = self.buffer(h);
        let ix = self.index(b, args[1])?;
        if write {
            self.unique(h, &e, false);
            b = self.buffer(h);
        }
        let at = self.slot_at(b, ix, e.size);
        for l in &e.leaves {
            self.f.get(at);
            self.f.op(l.load(LIST_ELEMS));
        }
        self.store(i)
    }

    /// Makes the buffer of the list header in local `h` its own.
    fn unique(&mut self, h: u32, e: &Elem, push: bool) {
        self.f.get(h);
        self.f.helper(Helper::ListUnique {
            elem: e.clone(),
            push,
        });
    }

    /// `ListSet`: the element's old value is released after the new one
    /// is counted.
    pub(super) fn list_set(&mut self, args: &[u32]) -> StageResult<()> {
        let (et, e) = self.elem(self.ty_of(args[0]))?;
        let h = self.header(args[0])?;
        let b = self.buffer(h);
        let ix = self.index(b, args[1])?;
        self.unique(h, &e, false);
        let b = self.buffer(h);
        let at = self.slot_at(b, ix, e.size);
        let s = self.slot_src(args[2], et, false)?;
        self.retain_src(&s, et)?;
        if e.walk.is_some() {
            let old: Vec<u32> = e.leaves.iter().map(|l| self.f.local(l.val)).collect();
            for (l, o) in e.leaves.iter().zip(&old) {
                self.f.get(at);
                self.f.op(l.load(LIST_ELEMS));
                self.f.set(*o);
            }
            self.release_locals(&old, et)?;
        }
        self.store_elem(at, &e, &s)
    }

    /// `ListPush`: a pinned list panics (`ref.elem.pinned`); otherwise the
    /// value is stored at the length, after the buffer is made the list's
    /// own with room for it.
    pub(super) fn list_push(&mut self, args: &[u32]) -> StageResult<()> {
        let (et, e) = self.elem(self.ty_of(args[0]))?;
        let h = self.header(args[0])?;
        self.f.get(h);
        self.f.helper(Helper::ListPinned);
        self.f.op(I::If(BlockType::Empty));
        self.f.helper(Helper::Panic(
            "collection-pinned: list length changed while pinned".into(),
        ));
        self.f.op(I::End);
        self.unique(h, &e, true);
        let b = self.buffer(h);
        let n = self.f.local(Val::I32);
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        self.f.set(n);
        let at = self.slot_at(b, n, e.size);
        let s = self.slot_src(args[1], et, false)?;
        self.retain_src(&s, et)?;
        self.store_elem(at, &e, &s)?;
        self.f.get(b);
        self.f.get(n);
        self.f.i32(1);
        self.f.op(I::I32Add);
        self.f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
        Ok(())
    }

    /// `ValueCopy`: each collection in the value becomes its copy, a new
    /// header over the same buffer; a value holding none is itself.
    pub(super) fn value_copy(&mut self, i: u32, r: u32) -> StageResult<()> {
        // The copy's own type says what it copies.
        let t = self.ty(i);
        let colls = coll_leaves(self.pool(), self.env(), t);
        if colls.is_empty() {
            self.push(r)?;
            return self.store(i);
        }
        if self.input.runtime.is_none() {
            return unsupported("lists without std's allocator");
        }
        if self.vals(t)?.len() == 1 {
            self.comp(r, 0)?;
            self.f.helper(Helper::ListCopy);
            return self.store_owned(i);
        }
        let s = self.copy_src(r, t)?;
        for k in 0..self.vals(t)?.len() {
            self.src(&s, k)?;
        }
        self.store(i)
    }

    /// `SnapBegin`: the list a counted `for` iterates, a copy of the
    /// collection, so that a change during the loop copies its buffer
    /// first (`flow.for.snapshot`).
    pub(super) fn snap_begin(&mut self, i: u32, coll: u32) -> StageResult<()> {
        if self.input.runtime.is_none() {
            return unsupported("collections without std's allocator");
        }
        self.comp(coll, 0)?;
        self.f.helper(Helper::ListCopy);
        self.store_owned(i)
    }

    /// `SnapEnd`: the loop over `snap` ended, so the snapshot gives up its
    /// buffer at once, and the collection's next change copies only if
    /// something else still shares it.
    pub(super) fn snap_end(&mut self, snap: u32) -> StageResult<()> {
        let h = self.header(snap)?;
        let b = self.buffer(h);
        let at = empty_buffer(self.m);
        self.f.get(h);
        self.f.u32(at);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        self.f.get(b);
        self.f.helper(Helper::ReleaseRef);
        Ok(())
    }

    /// `ListPin`: the list an element reference points into, its index
    /// (when given) checked and its buffer made its own (`ref.elem.pin`,
    /// §23.20). The header takes its owner word, so the buffer stays its
    /// own: a copy of the list copies the buffer while it is pinned. The
    /// pin is one count of the buffer, which `ListUnpin` gives back; each
    /// element reference into the buffer holds a count of its own, so the
    /// list stays pinned until the last copy of the last reference dies
    /// (`ref.elem.release.copies`). The value is the list.
    pub(super) fn list_pin(&mut self, i: u32, args: &[u32]) -> StageResult<()> {
        if self.input.runtime.is_none() {
            return unsupported("element references without std's allocator");
        }
        let (_, e) = self.elem(self.ty_of(args[0]))?;
        let h = self.header(args[0])?;
        if let Some(&ix) = args.get(1) {
            let b = self.buffer(h);
            self.index(b, ix)?;
        }
        self.unique(h, &e, false);
        self.f.get(h);
        self.f
            .body
            .push(Ins::TableSlot(Sym::Helper(Helper::ListClone { elem: e })));
        self.f.i32(1);
        self.f.op(I::I32Add);
        self.f.op(I::I32Store(mem(HEAP, LIST_OWN, 2)));
        let b = self.buffer(h);
        self.f.get(b);
        self.f
            .call(Sym::Inst(self.input.runtime.expect("checked above").retain));
        self.f.get(h);
        self.store(i)
    }

    /// `ListUnpin`: gives back the count of the buffer that `ListPin` took.
    pub(super) fn list_unpin(&mut self, list: u32) -> StageResult<()> {
        if self.input.runtime.is_none() {
            return Ok(());
        }
        let h = self.header(list)?;
        let b = self.buffer(h);
        self.f.get(b);
        self.f.helper(Helper::ReleaseRef);
        Ok(())
    }
}
