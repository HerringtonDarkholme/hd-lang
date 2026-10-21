//! Lists (`crates/hd_wasm_linear/DESIGN.md` §23.7, §23.17, job 9). A list value
//! is the address of a counted header that holds its buffer's address and
//! its owner word (§23.20); the buffer is a counted object holding the
//! capacity, the length and the elements inline. A header never moves, so
//! a change through it reaches every place that holds the same list; a copy (`ValueCopy`) is a new header
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
use crate::rt::{Elem, Helper, LIST_ELEMS, LIST_EXPOSED, LIST_LEN, LIST_OWN, empty_buffer};
use crate::unsupported;

/// A stable list header, directly in a local or behind a reference local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ListRoot {
    local: u32,
    indirect: bool,
}

/// Borrowed physical projections for one proven loop interval.
#[derive(Clone, Copy)]
pub(super) struct ListCache {
    header: u32,
    buffer: u32,
    len: u32,
}

impl Em<'_, '_, '_> {
    fn list_root(&self, r: u32) -> Option<ListRoot> {
        let i = Ref(r).as_inst()?.0 as usize;
        let TyData::Adt { def, .. } =
            self.pool()
                .get(strip(self.pool(), self.env(), self.ty_of(r)))
        else {
            return None;
        };
        if self.env().std_kind(def) != hd_mono::layout::StdKind::List {
            return None;
        }
        let [a, _] = self.b.data[i];
        match self.b.tags[i] {
            Tag::LocalGet => Some(ListRoot {
                local: a,
                indirect: false,
            }),
            Tag::RefRead
                if Ref(a).as_inst().is_some() && self.b.tags[a as usize] == Tag::LocalGet =>
            {
                Some(ListRoot {
                    local: self.b.data[a as usize][0],
                    indirect: true,
                })
            }
            Tag::Coerce => self.list_root(a),
            _ => None,
        }
    }

    /// Reaching aliases, including provider operands. Scalar observations
    /// of the list are not aliases of its header.
    fn list_aliases(&self, root: ListRoot) -> Vec<bool> {
        let mut locals = vec![false; self.b.local_ty.len()];
        locals[root.local as usize] = true;
        let mut values = vec![false; self.b.len()];
        loop {
            let mut changed = false;
            for i in 0..self.b.len() {
                let [a, w] = self.b.data[i];
                let aliases = super::live::uses(self.b, i)
                    .iter()
                    .any(|r| values[*r as usize]);
                if self.b.tags[i] == Tag::LocalSet
                    && values.get(w as usize) == Some(&true)
                    && !locals[a as usize]
                {
                    locals[a as usize] = true;
                    changed = true;
                }
                let reaches = match self.b.tags[i] {
                    Tag::LocalGet => locals[a as usize],
                    Tag::RefPlace => self.b.place(i).is_some_and(|p| {
                        matches!(
                            p.kind,
                            hd_tir::ir::place_root::LOCAL | hd_tir::ir::place_root::DEREF
                        ) && locals[p.x as usize]
                    }),
                    Tag::RefRead | Tag::Coerce => values.get(a as usize) == Some(&true),
                    _ => {
                        aliases
                            && self
                                .counted(self.ty(u32::try_from(i).expect("insts")))
                                .is_some()
                    }
                };
                if reaches && !values[i] {
                    values[i] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        values
    }

    fn scalar_loop_test(&self, i: u32, budget: &mut usize) -> bool {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let [a, w] = self.b.data[i as usize];
        if self.counted(self.ty(i)).is_some() {
            return false;
        }
        match self.b.tags[i as usize] {
            Tag::LocalGet => !self.is_cell(a as usize),
            Tag::Prim | Tag::Field | Tag::Coerce => true,
            Tag::And | Tag::Or => self.scalar_loop_test(w, budget),
            Tag::Block => self
                .rec(a)
                .into_iter()
                .filter(|x| self.b.tags[*x as usize] != Tag::Block)
                .all(|x| self.scalar_loop_test(x, budget)),
            _ => false,
        }
    }

    /// A canonical loop test, before its body. Duplicating this scalar
    /// prefix lets an empty loop avoid the hoisted COW allocation.
    fn list_loop_guard(&self, loop_inst: u32, body: u32) -> Option<(Vec<u32>, u32, bool)> {
        let is_break = |blk: u32| {
            if blk == hd_tir::ir::NONE || self.b.tags[blk as usize] != Tag::Block {
                return false;
            }
            let mut items = self
                .b
                .record(self.b.data[blk as usize][0])
                .iter()
                .copied()
                .filter(|x| self.b.tags[*x as usize] != Tag::Block);
            let Some(item) = items.next() else {
                return false;
            };
            items.next().is_none()
                && self.b.tags[item as usize] == Tag::Break
                && self
                    .b
                    .label_inst
                    .get(self.b.data[item as usize][0] as usize)
                    == Some(&loop_inst)
        };
        let mut prefix = Vec::new();
        let mut budget = self.b.len();
        for x in self.rec(self.b.data[body as usize][0]) {
            if self.b.tags[x as usize] == Tag::Block {
                continue;
            }
            if self.b.tags[x as usize] == Tag::If {
                let branches = self.rec(self.b.data[x as usize][1]);
                let first = is_break(branches[0]);
                let second = branches.get(1).is_some_and(|b| is_break(*b));
                return (first != second).then_some((prefix, x, first));
            }
            if !self.scalar_loop_test(x, &mut budget) {
                return None;
            }
            prefix.push(x);
        }
        None
    }

    /// This body's proof never consults a callee. Unescaped locals may
    /// cross unrelated calls; parameters and escaped headers also reject
    /// unknown calls that could reach them through an external alias.
    fn list_loop_safe(&self, root: ListRoot, body: &[u32], aliases: &[bool]) -> bool {
        if self.is_cell(root.local as usize) {
            return false;
        }
        let external = root.indirect
            || self.b.local_flags[root.local as usize] & hd_tir::ir::local_flags::PARAM != 0
            || (0..self.b.len()).any(|i| {
                aliases[i]
                    && matches!(
                        self.b.tags[i],
                        Tag::RefPlace | Tag::Call | Tag::CallValue | Tag::CallDyn
                    )
            });
        for &x in body {
            let i = x as usize;
            let [a, w] = self.b.data[i];
            let reaches = super::live::uses(self.b, i)
                .iter()
                .any(|r| aliases[*r as usize]);
            if self.calls.contains_key(&x)
                || matches!(
                    self.b.tags[i],
                    Tag::Call | Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall
                )
            {
                if reaches {
                    return false;
                }
                let known_scalar = self.calls.get(&x).is_some_and(|target| {
                    let hd_mono::Target::Call(call) = target else {
                        return false;
                    };
                    match &call.kind {
                        hd_mono::TargetKind::Intrinsic(key) => {
                            matches!(key.as_str(), "sqrt" | "ref_serial")
                        }
                        hd_mono::TargetKind::Builtin { self_ty, .. } => {
                            matches!(self.pool().get(*self_ty), TyData::Prim(p) if p.is_float())
                                && (self.input.path)(call.item).rsplit(['.', '/']).next()
                                    == Some("sqrt")
                        }
                        hd_mono::TargetKind::Instance => false,
                    }
                });
                if external && !known_scalar {
                    return false;
                }
                continue;
            }
            match self.b.tags[i] {
                Tag::LocalSet if a == root.local || aliases.get(w as usize) == Some(&true) => {
                    return false;
                }
                Tag::LocalSet
                    if external
                        && self
                            .counted(self.sub(self.b.local_ty[a as usize]))
                            .is_some() =>
                {
                    return false;
                }
                Tag::GlobalGet if external => return false,
                Tag::RefWrite | Tag::FieldSet | Tag::GlobalSet if external => return false,
                Tag::Coerce
                | Tag::NewData
                | Tag::CopyData
                | Tag::NewTuple
                | Tag::NewVariant
                | Tag::NewList
                | Tag::NewMap
                | Tag::Interp
                    if external && self.counted(self.ty(x)).is_some() =>
                {
                    return false;
                }
                Tag::Await
                | Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
                | Tag::Scope
                | Tag::Defer
                | Tag::Hook
                | Tag::Closure => return false,
                Tag::Intrinsic if external && !reaches => match IntrinsicOp::from_u32(a) {
                    Some(IntrinsicOp::ListLen) => {}
                    Some(IntrinsicOp::ListIndex) if self.counted(self.ty(x)).is_none() => {}
                    Some(IntrinsicOp::ValueCopy)
                        if coll_leaves(self.pool(), self.env(), self.ty(x)).is_empty() => {}
                    // Distinct reference parameters can denote the same
                    // list. A mutation through either may replace its buffer.
                    _ => return false,
                },
                Tag::Intrinsic if reaches => match IntrinsicOp::from_u32(a) {
                    Some(
                        IntrinsicOp::ListLen
                        | IntrinsicOp::ListIndex
                        | IntrinsicOp::ListSet
                        | IntrinsicOp::ListFieldSet
                        | IntrinsicOp::ListIndexMut,
                    ) => {}
                    _ => return false,
                },
                Tag::LocalGet | Tag::RefRead | Tag::Field | Tag::Block => {}
                _ if reaches => return false,
                _ => {}
            }
        }
        true
    }

    pub(super) fn begin_list_loop(
        &mut self,
        loop_inst: u32,
        block: u32,
    ) -> StageResult<Vec<ListRoot>> {
        if self.susp.is_some() {
            return Ok(Vec::new());
        }
        let tree = super::live::Tree::new(self.b);
        let body: Vec<u32> = (0..self.b.len())
            .map(|i| u32::try_from(i).expect("insts"))
            .filter(|x| tree.in_loop(self.b, *x, loop_inst))
            .collect();
        let guard = self.list_loop_guard(loop_inst, block);
        let mut candidates: Vec<(ListRoot, Elem, bool, bool)> = Vec::new();
        for &x in &body {
            if self.b.tags[x as usize] != Tag::Intrinsic {
                continue;
            }
            let [op, record] = self.b.data[x as usize];
            let write = match IntrinsicOp::from_u32(op) {
                Some(IntrinsicOp::ListLen | IntrinsicOp::ListIndex) => false,
                Some(
                    IntrinsicOp::ListSet | IntrinsicOp::ListFieldSet | IntrinsicOp::ListIndexMut,
                ) => true,
                _ => continue,
            };
            let Some(list) = self.b.record(record).first().copied() else {
                continue;
            };
            let Some(root) = self.list_root(list) else {
                continue;
            };
            if self.list_caches.iter().any(|(key, _)| *key == root) {
                continue;
            }
            let (_, elem) = self.elem(self.ty_of(list))?;
            if elem.walk.is_some() {
                continue;
            }
            let definite = guard
                .as_ref()
                .is_some_and(|(_, g, _)| tree.iteration_write(self.b, x, loop_inst, *g));
            if let Some((_, _, writes, guaranteed)) =
                candidates.iter_mut().find(|(r, ..)| *r == root)
            {
                *writes |= write;
                *guaranteed |= write && definite;
            } else {
                candidates.push((root, elem, write, write && definite));
            }
        }
        candidates.retain(|(root, _, write, definite)| {
            (!write || *definite) && self.list_loop_safe(*root, &body, &self.list_aliases(*root))
        });
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        self.f.helper(Helper::RefViewsAbsent);
        self.f.op(I::If(BlockType::Empty));
        if let Some((prefix, test, invert)) = &guard {
            for &x in prefix {
                self.inst(x)?;
            }
            self.push(self.b.data[*test as usize][0])?;
            if *invert {
                self.f.op(I::I32Eqz);
            }
            self.f.op(I::If(BlockType::Empty));
        }
        let mut roots = Vec::new();
        for (root, elem, write, _) in candidates {
            let local = self.local_value(root.local)?[0];
            self.f.get(local);
            if root.indirect {
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
            }
            let header = self.f.local(Val::I32);
            self.f.set(header);
            if write {
                self.unique(header, &elem, false);
            }
            let buffer = self.buffer(header);
            let len = self.f.local(Val::I32);
            self.f.get(buffer);
            self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            self.f.set(len);
            self.list_caches.push((
                root,
                ListCache {
                    header,
                    buffer,
                    len,
                },
            ));
            roots.push(root);
        }
        if guard.is_some() {
            self.f.op(I::End);
        }
        self.f.op(I::End);
        Ok(roots)
    }

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
            cyclic: self.cyclic_object(t),
        };
        Ok((et, elem))
    }

    /// A local holding list value `r`'s header.
    pub(super) fn header(&mut self, r: u32) -> StageResult<u32> {
        let h = self.f.local(Val::I32);
        let cache = self
            .list_root(r)
            .and_then(|root| {
                self.list_caches
                    .iter()
                    .find(|(key, _)| *key == root)
                    .map(|(_, cache)| cache)
            })
            .copied();
        if let Some(cache) = cache {
            self.f.helper(Helper::RefViewsAbsent);
            self.f.op(I::If(BlockType::Result(Val::I32.wasm())));
            self.f.get(cache.header);
            self.f.op(I::Else);
            self.list_addr(r)?;
            self.f.op(I::End);
            self.cached_headers.push((h, cache));
        } else {
            self.list_addr(r)?;
        }
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
        if let Some(cache) = self
            .cached_headers
            .iter()
            .find(|(key, _)| *key == h)
            .map(|(_, cache)| *cache)
        {
            self.f.helper(Helper::RefViewsAbsent);
            self.f.op(I::If(BlockType::Result(Val::I32.wasm())));
            self.f.get(cache.buffer);
            self.f.op(I::Else);
            self.f.get(h);
            self.f.op(I::I32Load(mem(HEAP, 0, 2)));
            self.f.op(I::End);
            self.cached_buffers.push((b, cache.len));
        } else {
            self.f.get(h);
            self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        }
        self.f.set(b);
        b
    }

    fn buffer_len(&mut self, b: u32) {
        if let Some(len) = self
            .cached_buffers
            .iter()
            .find(|(key, _)| *key == b)
            .map(|(_, len)| *len)
        {
            self.f.helper(Helper::RefViewsAbsent);
            self.f.op(I::If(BlockType::Result(Val::I32.wasm())));
            self.f.get(len);
            self.f.op(I::Else);
            self.f.get(b);
            self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            self.f.op(I::End);
        } else {
            self.f.get(b);
            self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        }
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
        self.buffer_len(b);
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
    /// is no legacy `mut` view, or a copy.
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
        self.f.helper(Helper::ListHeader { cyclic: e.cyclic });
        self.store_owned(i)
    }

    /// `ListLen`.
    pub(super) fn list_len(&mut self, i: u32, list: u32) -> StageResult<()> {
        self.elem(self.ty_of(list))?;
        let h = self.header(list)?;
        let b = self.buffer(h);
        self.buffer_len(b);
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
            self.invariant_unique(args[0], h, &e);
            b = self.buffer(h);
        }
        let at = self.slot_at(b, ix, e.size);
        if !write && let Some((field, part)) = self.projected_field(i)? {
            for leaf in &e.leaves[part.first..part.first + part.count] {
                self.f.get(at);
                self.f.op(leaf.load(LIST_ELEMS));
            }
            self.store(field)?;
            self.precomputed_fields.insert(field);
            return Ok(());
        }
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

    /// No call or operation here can share a buffer after its first write.
    /// The one allowed call is the compiler's scalar square-root operation.
    fn cannot_share_buffers(&self) -> bool {
        for i in 0..self.b.len() {
            let tag = self.b.tags[i];
            let x = u32::try_from(i).expect("insts");
            if let Some(target) = self.calls.get(&x) {
                let hd_mono::Target::Call(call) = target else {
                    return false;
                };
                let sqrt = match &call.kind {
                    hd_mono::TargetKind::Intrinsic(key) => {
                        matches!(key.as_str(), "sqrt" | "ref_serial")
                    }
                    hd_mono::TargetKind::Builtin { self_ty, .. } => {
                        matches!(self.pool().get(*self_ty), TyData::Prim(p) if p.is_float())
                            && (self.input.path)(call.item).rsplit(['.', '/']).next()
                                == Some("sqrt")
                    }
                    hd_mono::TargetKind::Instance => false,
                };
                if !sqrt
                    || self.counted(self.ty(x)).is_some()
                    || self
                        .b
                        .value_operands(i)
                        .unwrap_or_default()
                        .iter()
                        .any(|r| self.counted(self.ty_of(*r)).is_some())
                {
                    return false;
                }
            }
            match tag {
                Tag::GlobalGet
                | Tag::GlobalSet
                | Tag::RefWrite
                | Tag::FieldSet
                | Tag::CallValue
                | Tag::CallDyn
                | Tag::CallHost
                | Tag::DefaultCall
                | Tag::Await
                | Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
                | Tag::Hook
                | Tag::Scope
                | Tag::Defer
                | Tag::Closure => return false,
                Tag::Call if !self.calls.contains_key(&x) => return false,
                Tag::NewData
                | Tag::CopyData
                | Tag::NewTuple
                | Tag::NewVariant
                | Tag::NewList
                | Tag::NewMap
                | Tag::Interp => {
                    if self.counted(self.ty(x)).is_some() {
                        return false;
                    }
                }
                Tag::Intrinsic => {
                    let [op, record] = self.b.data[i];
                    match IntrinsicOp::from_u32(op) {
                        Some(IntrinsicOp::ListLen | IntrinsicOp::ListIndex) => {}
                        Some(
                            IntrinsicOp::ListSet
                            | IntrinsicOp::ListFieldSet
                            | IntrinsicOp::ListIndexMut,
                        ) => {
                            let Some(&list) = self.b.record(record).first() else {
                                return false;
                            };
                            if !self
                                .elem(self.ty_of(list))
                                .is_ok_and(|(_, e)| e.walk.is_none())
                            {
                                return false;
                            }
                        }
                        Some(IntrinsicOp::ValueCopy)
                            if coll_leaves(self.pool(), self.env(), self.ty(x)).is_empty() => {}
                        _ => return false,
                    }
                }
                _ => {}
            }
        }
        true
    }

    fn invariant_unique(&mut self, list: u32, h: u32, e: &Elem) {
        if self
            .list_root(list)
            .and_then(|root| {
                self.list_caches
                    .iter()
                    .find(|(key, _)| *key == root)
                    .map(|(_, cache)| cache)
            })
            .is_some()
        {
            // The plan includes every write in this interval, so any
            // cached root used here already ran its uniqueness check.
            self.f.helper(Helper::RefViewsAbsent);
            self.f.op(I::If(BlockType::Empty));
            self.f.op(I::Else);
            self.unique(h, e, false);
            self.f.op(I::End);
            return;
        }

        let safe = if self.loops > 0 && e.walk.is_none() {
            self.unique_body.unwrap_or_else(|| {
                let safe = self.cannot_share_buffers();
                self.unique_body = Some(safe);
                safe
            })
        } else {
            false
        };
        let root = safe
            .then(|| {
                if self.b.tags.get(list as usize) != Some(&Tag::RefRead) {
                    return None;
                }
                let r = self.b.data[list as usize][0];
                if r == hd_tir::ir::NONE
                    || Ref(r).as_inst().is_none()
                    || self.b.tags[r as usize] != Tag::LocalGet
                {
                    return None;
                }
                let mut l = self.b.data[r as usize][0] as usize;
                for _ in 0..self.b.local_ty.len() {
                    if self.borrowed.contains(&l) {
                        return Some(l);
                    }
                    let &source = self.moves.loan_locals.get(&l)?;
                    if self.b.tags[source as usize] != Tag::LocalGet {
                        return None;
                    }
                    l = self.b.data[source as usize][0] as usize;
                }
                None
            })
            .flatten();
        let Some(root) = root else {
            self.unique(h, e, false);
            return;
        };
        let flag = *self
            .unique_once
            .entry(root)
            .or_insert_with(|| self.f.local(Val::I32));
        // A view's getter can make another buffer owner implicitly. The
        // link enables this cache only when no view constructor is reachable.
        self.f.helper(Helper::RefViewsAbsent);
        self.f.get(flag);
        self.f.op(I::I32And);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        self.unique(h, e, false);
        self.f.i32(1);
        self.f.set(flag);
        self.f.op(I::End);
    }

    /// `ListSet`: the element's old value is released after the new one
    /// is counted.
    pub(super) fn list_set(&mut self, args: &[u32]) -> StageResult<()> {
        let (et, e) = self.elem(self.ty_of(args[0]))?;
        let h = self.header(args[0])?;
        let b = self.buffer(h);
        let ix = self.index(b, args[1])?;
        self.invariant_unique(args[0], h, &e);
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

    /// `ListFieldSet`: stores a value into one field of an element, at the
    /// field's offset, after the bounds check and the uniqueness check:
    /// the field's old value is released after the new one is counted, and
    /// no other field is read or written (#655).
    pub(super) fn list_field_set(&mut self, args: &[u32]) -> StageResult<()> {
        let (et, e) = self.elem(self.ty_of(args[0]))?;
        // The field's values among the element's, and the field's type.
        let (mut first, mut count, mut ft) = (0, e.leaves.len(), et);
        for &f in &args[3..] {
            let r = f & !Ref::CONST_BIT;
            let k = usize::try_from(self.b.consts[r as usize].1).expect("field");
            let (_, parts) = crate::layout::struct_mem(self.pool(), self.env(), ft)?;
            let Some(part) = parts.get(k) else {
                return unsupported("a field store past the struct's fields");
            };
            first += part.first;
            count = part.count;
            ft = self.struct_fields(ft)[k];
        }
        let leaves = e.leaves[first..first + count].to_vec();
        let h = self.header(args[0])?;
        let b = self.buffer(h);
        let ix = self.index(b, args[1])?;
        self.invariant_unique(args[0], h, &e);
        let b = self.buffer(h);
        let at = self.slot_at(b, ix, e.size);
        let s = self.slot_src(args[2], ft, false)?;
        self.retain_src(&s, ft)?;
        if self.counted(ft).is_some() {
            let old: Vec<u32> = leaves.iter().map(|l| self.f.local(l.val)).collect();
            for (l, o) in leaves.iter().zip(&old) {
                self.f.get(at);
                self.f.op(l.load(LIST_ELEMS));
                self.f.set(*o);
            }
            self.release_locals(&old, ft)?;
        }
        for (k, l) in leaves.iter().enumerate() {
            self.store_leaf(
                &|em: &mut Self| {
                    em.f.get(at);
                    Ok(())
                },
                LIST_ELEMS,
                *l,
                &mut |em: &mut Self| em.src(&s, k),
            )?;
        }
        Ok(())
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
        self.f.static_addr(at);
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
        let exposed = !e.colls.is_empty();
        self.f.get(h);
        self.f
            .push(Ins::TableSlot(Sym::Helper(Helper::ListClone { elem: e })));
        self.f.i32(1);
        self.f.op(I::I32Add);
        if exposed {
            self.f.u32(LIST_EXPOSED);
            self.f.op(I::I32Or);
        }
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
