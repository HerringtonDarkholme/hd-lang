//! Lists (`crates/hd_wasm/DESIGN.md` §23.7, §23.17, job 9). A list value
//! is the address of its buffer, a counted object holding the capacity,
//! the length, a reserved word that stays 0 (§23.20), and the elements
//! inline. Every copy (`rt:coll_copy`) counts one more holder of the same
//! buffer. The ownership passes' `IsUnique` makes the buffer the list's
//! own before a change (`rt:list_unique`): a shared one is copied, and the
//! copied elements' collections are copied in turn, so a nested write
//! never reaches another list. A check or a change that replaces the
//! buffer stores the new one into the place it names
//! (`docs/compiler/tir-v2.md`, "List and Map buffers", 3f). A loop's
//! stable lists come as TIR values: the header a hoisted `IsUnique`
//! leaves, and the length a local holds (`hd_opt::prove`).

use crate::op::Op as I;
use hd_base::StageResult;
use hd_mono::layout::StdKind;
use hd_tir::ir::{Coercion, NONE, Ref, Tag};
use hd_types::{Prim, Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::value::{Dst, Src};
use crate::layout::{coll_leaves, mem as mem_of, strip_newtype};
use crate::module::{HEADER, HEAP, Val, mem};
use crate::rt::{Elem, Helper, LIST_ELEMS, LIST_LEN, empty_buffer};

impl Em<'_, '_, '_> {
    /// A nonempty list of scalar constants as one static count-zero
    /// buffer, for a top-level binding's initializer only
    /// ([`hd_tir::ir::Body::top_level_initializer`]). A change sees its
    /// count is not one and copies it through the ordinary copy-on-write
    /// path.
    fn static_scalar_list(&self, et: Ty, elem: &Elem, items: &[u32]) -> Option<Vec<u8>> {
        let prim = match self.pool().get(strip_newtype(self.pool(), self.env(), et)) {
            TyData::Prim(prim) if prim != Prim::String => prim,
            _ => return None,
        };
        let [leaf] = elem.leaves.as_slice() else {
            return None;
        };
        if leaf.offset != 0 || u32::from(leaf.bytes) != elem.size {
            return None;
        }
        let len = u32::try_from(items.len()).ok()?;
        let elem_bytes = len.checked_mul(elem.size)?;
        let total = HEADER.checked_add(LIST_ELEMS)?.checked_add(elem_bytes)?;
        let mut bytes = vec![0; usize::try_from(total).ok()?];
        let payload = usize::try_from(HEADER).ok()?;
        bytes[payload..payload + 4].copy_from_slice(&len.to_le_bytes());
        bytes[payload + 4..payload + 8].copy_from_slice(&len.to_le_bytes());

        for (k, mut r) in items.iter().copied().enumerate() {
            for _ in 0..32 {
                let Some(i) = Ref(r).as_inst() else { break };
                let [a, record] = self.b.data[i.0 as usize];
                if self.b.tags[i.0 as usize] != Tag::Coerce
                    || self.b.record(record).first() != Some(&(Coercion::Identity as u32))
                {
                    return None;
                }
                r = a;
            }
            if r == NONE || Ref(r).as_inst().is_some() {
                return None;
            }
            let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
            if self.vals(ty).ok()?.as_slice() != [leaf.val] {
                return None;
            }
            let raw = match prim {
                // A float literal holds f64 bits even when it settles to
                // f32, just as `konst_ops` documents and emits.
                Prim::F32 => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "this is the specified f64-to-f32 literal conversion"
                    )]
                    let value = f64::from_bits(bits) as f32;
                    u64::from(value.to_bits())
                }
                _ => bits,
            }
            .to_le_bytes();
            let width = usize::try_from(elem.size).ok()?;
            let at = usize::try_from(HEADER + LIST_ELEMS).ok()? + k * width;
            bytes[at..at + width].copy_from_slice(&raw[..width]);
        }
        Some(bytes)
    }

    /// The hoisted `IsUnique`s waiting for loop `l` (`hoisted`), run once
    /// before it under its entry test, so an empty loop copies nothing
    /// (`docs/compiler/tir-v2.md`, "Hoisting"): the statements before the
    /// test, which the pass found pure, run once more, then the test. With
    /// no loop (`None`), no test, or in a suspending body, they run
    /// unguarded, which costs only a copy an empty loop did not need.
    pub(super) fn hoisted_uniques(&mut self, l: Option<u32>) -> StageResult<()> {
        let uniques = std::mem::take(&mut self.hoisted);
        if uniques.is_empty() {
            return Ok(());
        }
        let test = l
            .filter(|_| {
                uniques.iter().any(|&i| {
                    !matches!(
                        self.b.data[i as usize][1],
                        hd_tir::ir::unique_word::DEFERRED_INIT
                            | hd_tir::ir::unique_word::DEFERRED_CACHE
                    )
                })
            })
            .filter(|_| self.susp.is_none())
            .and_then(|l| hd_tir::own::entry_test(self.b, &self.topo, l).map(|e| (l, e)));
        if let Some((l, e)) = test {
            let prefix: Vec<u32> = hd_tir::own::loop_statements(self.b, l)
                .take(e.prefix)
                .collect();
            for x in prefix {
                self.inst(x)?;
            }
            self.push(self.b.data[e.test as usize][0])?;
            if e.leaves_on_true {
                self.f.op(I::I32Eqz);
            }
            self.f.op(I::If(BlockType::Empty));
        }
        let was = self.f.reason;
        for i in uniques {
            if matches!(
                self.b.data[i as usize][1],
                hd_tir::ir::unique_word::DEFERRED_INIT | hd_tir::ir::unique_word::DEFERRED_CACHE
            ) {
                let flag = self.f.local(Val::I32);
                let cache = self.b.data[i as usize][1] == hd_tir::ir::unique_word::DEFERRED_CACHE;
                self.f.i32(0);
                self.f.set(flag);
                self.deferred_uniques
                    .insert(self.b.data[i as usize][0], (flag, cache));
                continue;
            }
            self.f.reason = hd_tir::own::reason(self.b, i as usize);
            let h = self.unique_at(i)?;
            // The header after the check, which the loop's element
            // accesses root at (`hd_opt::prove`); the place keeps its count.
            if self.b.ty[i as usize] != Ty::VOID {
                self.f.get(h);
                self.store(i)?;
            }
        }
        self.f.reason = was;
        if test.is_some() {
            self.f.op(I::End);
        }
        Ok(())
    }

    /// The element type of list type `t` and its layout for the glue.
    pub(super) fn elem(&self, t: Ty) -> StageResult<(Ty, Elem)> {
        // ruled out: the driver roots std runtime items before collecting list operations.
        // ruled out: list intrinsic signatures and checked projections supply List[T].
        let (def, args) = self
            .pool()
            .adt_parts(strip_newtype(self.pool(), self.env(), t));
        debug_assert_eq!(self.env().std_kind(def), StdKind::List);
        let items = self.pool().list_items(args);
        // ruled out: the seeded List[T] has one completed generic argument.
        debug_assert_eq!(items.len(), 1);
        let et = items[0];
        let m = mem_of(self.pool(), self.env(), et)?;
        let elem = Elem {
            size: m.size,
            leaves: m.leaves,
            walk: self.counted(et),
            // List elements are Copy, so only their referents may have hooks.
            drops: Vec::new(),
            colls: coll_leaves(self.pool(), self.env(), et),
            cyclic: self.cyclic_object(t),
        };
        Ok((et, elem))
    }

    /// A local holding the buffer of list or map value `r`.
    pub(super) fn buffer(&mut self, r: u32) -> StageResult<u32> {
        let b = self.f.local(Val::I32);
        self.comp(r, 0)?;
        self.f.set(b);
        Ok(b)
    }

    /// A local holding index `r`, checked against the length of the
    /// buffer in local `b`, or the length in local `len` where a proof
    /// keeps it there: an index past it is `index-out-of-bounds`, a
    /// 64-bit one before it is narrowed (`expr.index.list.range`).
    pub(super) fn index(&mut self, b: u32, r: u32, len: Option<u32>) -> StageResult<u32> {
        let ix = self.f.local(Val::I32);
        let wide = self.vals(self.ty_of(r))? == [Val::I64];
        let w = self.f.local(if wide { Val::I64 } else { Val::I32 });
        self.comp(r, 0)?;
        self.f.set(w);
        self.f.get(w);
        if let Some(len) = len {
            self.f.get(len);
        } else {
            self.f.get(b);
            self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        }
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
    pub(super) fn slot_at(&mut self, b: u32, ix: u32, size: u32) -> u32 {
        let at = self.f.local(Val::I32);
        self.f.get(b);
        self.f.get(ix);
        self.f.u32(size);
        self.f.op(I::I32Mul);
        self.f.op(I::I32Add);
        self.f.set(at);
        at
    }

    /// Source `s` of a `t` value for the place that consumes it: an operand
    /// at its only use moves its count there ([`Em::take`]), any other
    /// value is a copy with counts of its own ([`Em::copy_locals`]).
    pub(super) fn retain_src(&mut self, s: &Src, t: Ty) -> StageResult<Src> {
        match s {
            Src::Ref(r) => self.consumed(*r),
            Src::Locals(ls) => Ok(Src::Locals(self.copy_locals(ls, t)?)),
        }
    }

    /// `NewList`: a buffer sized exactly for the items and the spreads
    /// (`expr.list.spread.insert`), each item stored and each spread's
    /// elements copied in order. An empty literal shares the static empty
    /// buffer, and a top-level binding's literal of constants is static
    /// data.
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
            self.f.static_addr(at);
        } else if spreads.is_empty()
            && self.b.top_level_initializer(i as usize)
            && let Some(bytes) = self.static_scalar_list(et, &e, &items)
        {
            let at = self.m.static_bytes(&bytes, 8) + HEADER;
            self.m.label_static(at, "scalar-list-buffer");
            self.f.static_addr(at);
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
                let sb = self.buffer(*r)?;
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
                    let sb = next.next().expect("one buffer per spread");
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
                    self.f.push(crate::bulk::copy(HEAP, HEAP));
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
                    let s = self.retain_src(&Src::Ref(*r), et)?;
                    self.write_new(Dst::Mem(at, LIST_ELEMS, &e.leaves), &s)?;
                    self.f.get(at);
                    self.f.u32(e.size);
                    self.f.op(I::I32Add);
                    self.f.set(at);
                }
            }
            self.f.get(b);
        }
        self.store(i)
    }

    /// `ListLen`.
    pub(super) fn list_len(&mut self, i: u32, list: u32) -> StageResult<()> {
        self.elem(self.ty_of(list))?;
        let b = self.buffer(list)?;
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        self.store(i)
    }

    /// The element type of list type `t` and a local holding the address
    /// of its slot `ix` in the buffer in local `b`, the index checked
    /// against the length; for a map type, of the value of entry `ix`, a
    /// `MapEntry`'s number. Either address is `LIST_ELEMS` before the
    /// value.
    pub(super) fn elem_slot(&mut self, b: u32, t: Ty, ix: u32) -> StageResult<(Ty, u32)> {
        if self.cx().is_map(t) {
            // `LIST_ELEMS` before the value, as a list's slot is.
            let (vt, at) = self.map_value_at(b, t, ix)?;
            let x = self.f.local(Val::I32);
            self.f.get(at);
            self.f.u32(LIST_ELEMS);
            self.f.op(I::I32Sub);
            self.f.set(x);
            return Ok((vt, x));
        }
        let (et, e) = self.elem(t)?;
        let ix = self.index(b, ix, None)?;
        Ok((et, self.slot_at(b, ix, e.size)))
    }

    /// `ListPush`: the value is stored at the length, after the buffer,
    /// which the passes' `IsUnique` made the list's own, has room for it.
    /// A buffer with room left is the fast path. Any
    /// other goes through `rt:list_grow`, which moves a full buffer into
    /// the operand's place, keeping the length.
    pub(super) fn list_push(&mut self, args: &[u32]) -> StageResult<()> {
        let (et, e) = self.elem(self.ty_of(args[0]))?;
        let b = self.buffer(args[0])?;
        let n = self.f.local(Val::I32);
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        self.f.set(n);
        self.f.get(n);
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.op(I::I32Eq);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(b);
        self.f.cold_helper(
            Helper::ListGrow { elem: e.clone() },
            vec![Val::I32],
            vec![Val::I32],
        );
        self.f.set(b);
        self.put_back(args[0], b)?;
        self.f.op(I::End);
        let at = self.slot_at(b, n, e.size);
        let s = self.retain_src(&Src::Ref(args[1]), et)?;
        self.write_new(Dst::Mem(at, LIST_ELEMS, &e.leaves), &s)?;
        self.f.get(b);
        self.f.get(n);
        self.f.i32(1);
        self.f.op(I::I32Add);
        self.f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
        Ok(())
    }

    /// `ListReserve`: room for `count` more elements, through
    /// [`Helper::ListReserve`], which copies a shared buffer; the buffer
    /// it returns goes back into the operand's
    /// place. A count of 0 does nothing, so an empty list keeps the static
    /// empty buffer.
    pub(super) fn list_reserve(&mut self, args: &[u32]) -> StageResult<()> {
        let (_, e) = self.elem(self.ty_of(args[0]))?;
        let count = self.f.local(Val::I32);
        self.push(args[1])?;
        self.f.set(count);
        self.f.get(count);
        self.f.op(I::If(BlockType::Empty));
        let b = self.buffer(args[0])?;
        self.f.get(b);
        self.f.get(count);
        self.f.helper(Helper::ListReserve { elem: e });
        self.f.set(b);
        self.put_back(args[0], b)?;
        self.f.op(I::End);
        Ok(())
    }

    /// std's changes of a list through a reference: `list_truncate(items:
    /// &mut List[T], n)`, `list_extend(items, other)`,
    /// and `list_reserve(items, count)`. The list is read through its reference, changed
    /// by its helper, and its buffer stored back. The other argument is
    /// lent: the helper keeps no count of it. `list_reserve` is
    /// [`Helper::ListReserve`] itself, the routine `list_extend` reserves
    /// with, behind the same `count != 0` test.
    pub(super) fn list_ref_change(&mut self, key: &str, args: &[u32]) -> StageResult<()> {
        // ruled out: a first argument that is no reference. The intrinsic
        // table (`hd_resolve::intrinsics`) gives each a `&mut` first, and
        // std declares it `items: &mut List[T]`.
        let (lt, _) = self
            .cx()
            .reference(self.ty_of(args[0]))
            .expect("`&mut List[T]`");
        let (_, elem) = self.elem(lt)?;
        let helper = match key {
            "list_truncate" => Helper::ListTruncate { elem },
            "list_extend" => Helper::ListExtend { elem },
            // `list_reserve`, the one key left.
            _ => Helper::ListReserve { elem },
        };
        let reserve = key == "list_reserve";
        if reserve {
            self.push(args[1])?;
            self.f.op(I::If(BlockType::Empty));
        }
        let at = self.f.local(Val::I32);
        self.comp(args[0], 0)?;
        self.f.set(at);
        self.f.get(at);
        self.f.get(at);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.push(args[1])?;
        self.f.helper(helper);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        if reserve {
            self.f.op(I::End);
        }
        Ok(())
    }
}
