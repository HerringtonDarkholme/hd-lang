//! Lists (`crates/hd_wasm_linear/DESIGN.md` §23.7, §23.17, job 9). A list value
//! is the address of its buffer, a counted object holding the capacity,
//! the length, the owner word (§23.20) and the elements inline; a copy,
//! every count of it (`rt:coll_copy`), counts one more holder of the same
//! buffer, or clones a pinned one. The ownership passes' `IsUnique` makes the buffer the list's
//! own before a change (`rt:list_unique`): a shared one is copied, and the
//! copied elements' collections are copied in turn, so a nested write
//! never reaches another list. A check or a change that replaces the
//! buffer stores the new one into the place it names
//! (`docs/compiler/tir-v2.md`, "List and Map buffers", 3f). A loop's
//! stable lists keep their buffer and length in locals
//! (`Em::own_list_caches`).

use crate::op::Op as I;
use hd_base::StageResult;
use hd_mono::layout::StdKind;
use hd_tir::ir::{Coercion, IntrinsicOp, NONE, PlaceRoot, PrimOp, Proj, Ref, Step, Tag};
use hd_types::{Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::shape::Through;
use super::value::{Dst, Src};
use crate::layout::{Kind, coll_kind, coll_leaves, mem as mem_of, strip};
use crate::module::{HEAP, Sym, Val, mem};
use crate::rt::{Elem, Helper, LIST_ELEMS, LIST_EXPOSED, LIST_LEN, LIST_OWN, empty_buffer};
use crate::unsupported;

/// A stable list, directly in a local or behind a reference local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ListRoot {
    local: u32,
    indirect: bool,
}

impl ListRoot {
    /// The list in local `local`, or behind the reference it holds.
    pub(super) fn new(local: u32, indirect: bool) -> ListRoot {
        ListRoot { local, indirect }
    }
}

/// Borrowed physical projections for one proven loop interval.
#[derive(Clone, Copy)]
pub(super) struct ListCache {
    pub(super) buffer: u32,
    pub(super) len: u32,
}

impl Em<'_, '_, '_> {
    /// `Some` when `t` is a `List`.
    fn list_root_ty(&self, t: Ty) -> Option<()> {
        (coll_kind(self.pool(), self.env(), t) == Some(StdKind::List)).then_some(())
    }

    fn list_root(&self, r: u32) -> Option<ListRoot> {
        let i = Ref(r).as_inst()?.0 as usize;
        if coll_kind(self.pool(), self.env(), self.ty_of(r)) != Some(StdKind::List) {
            return None;
        }
        let [a, _] = self.b.data[i];
        match self.b.tags[i] {
            Tag::Read if let Some(l) = self.b.local_read(i) => Some(ListRoot {
                local: l.raw(),
                indirect: false,
            }),
            Tag::Read => match self.read_through(i)? {
                (Through::Value(r), true) if let Some(l) = super::shape::read_local(self.b, r) => {
                    Some(ListRoot {
                        local: u32::try_from(l).expect("locals"),
                        indirect: true,
                    })
                }
                (Through::Local(l), true) => Some(ListRoot {
                    local: u32::try_from(l).expect("locals"),
                    indirect: true,
                }),
                _ => None,
            },
            Tag::Coerce => self.list_root(a),
            _ => None,
        }
    }

    /// Reaching aliases, including provider operands. Scalar observations
    /// of the list are not aliases of it. One flag per value, then
    /// one per local: a local that holds the list too, whose places rooted
    /// at it reach the list ([`Em::rooted_elem`]).
    fn list_aliases(&self, root: ListRoot) -> Vec<bool> {
        let mut locals = vec![false; self.b.local_ty.len()];
        locals[root.local as usize] = true;
        let mut values = vec![false; self.b.len()];
        loop {
            let mut changed = false;
            for i in 0..self.b.len() {
                let [a, _] = self.b.data[i];
                let aliases = super::shape::uses(self.b, i)
                    .iter()
                    .any(|r| values[*r as usize]);
                if let Some((l, v)) = super::shape::local_part_store(self.b, i)
                    && values.get(v as usize) == Some(&true)
                    && !locals[l]
                {
                    locals[l] = true;
                    changed = true;
                }
                let reaches = match self.b.tags[i] {
                    Tag::Read if let Some(l) = self.b.local_read(i) => locals[l.idx()],
                    // A reference to the list local's own place, or to
                    // what the reference it holds refers to.
                    // A data object's field is no such place.
                    Tag::Ref => self.b.decode_place(a).is_some_and(|p| {
                        let data = matches!(self.kind(self.at_root_ty(p.root)), Kind::Data(_));
                        let rest = match p.proj.as_slice() {
                            [Proj::Deref, rest @ ..] if !data || rest.is_empty() => rest,
                            rest => rest,
                        };
                        matches!(p.root, PlaceRoot::Local(l) if locals[l.idx()])
                            && rest.iter().all(|s| matches!(s, Proj::Field(_)))
                    }),
                    // An element is no alias of its list, read from a list
                    // value (`[%xs [%i]]`) or from a place rooted at a local
                    // (`[*$l [%i]]`, [`Em::rooted_elem`]).
                    Tag::Read
                        if self.projection(i).is_none()
                            && self.b.element_read(i).is_none()
                            && self.rooted_elem(i).is_none() =>
                    {
                        match self.read_through(i) {
                            Some((Through::Value(r), _)) => values.get(r as usize) == Some(&true),
                            Some((Through::Local(l), _)) => locals[l],
                            None => false,
                        }
                    }
                    Tag::Coerce => values.get(a as usize) == Some(&true),
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
        values.extend(locals);
        values
    }

    /// The list and index of `Read` or `Store` `i` whose place is rooted at
    /// a local and starts with an element of the local's list or of the
    /// list the reference it holds refers to (`[$l [%i] ..]`,
    /// `[*$l [%i] ..]`), as the checker roots places on the ownership
    /// passes' path (`hd_tir::ir::root_places`).
    pub(super) fn rooted_elem(&self, i: usize) -> Option<(ListRoot, u32)> {
        if !matches!(self.b.tags[i], Tag::Read | Tag::Store) {
            return None;
        }
        let p = self.b.decode_place(self.b.data[i][0])?;
        let hd_tir::ir::PlaceRoot::Local(l) = p.root else {
            return None;
        };
        let (indirect, ix) = match p.proj.as_slice() {
            [hd_tir::ir::Proj::Elem(ix), ..] => (false, *ix),
            [hd_tir::ir::Proj::Deref, hd_tir::ir::Proj::Elem(ix), ..] => (true, *ix),
            _ => return None,
        };
        Some((ListRoot::new(l.raw(), indirect), ix.0))
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
            Tag::Read if let Some(l) = self.b.local_read(i as usize) => !self.is_cell(l.idx()),
            Tag::Read
                if matches!(
                    self.projection(i as usize),
                    Some((_, super::shape::Projected::Field))
                ) =>
            {
                true
            }
            Tag::Prim | Tag::Coerce => true,
            Tag::And | Tag::Or => self.scalar_loop_test(w, budget),
            Tag::Block => self
                .rec(a)
                .into_iter()
                .filter(|x| self.b.tags[*x as usize] != Tag::Block)
                .all(|x| self.scalar_loop_test(x, budget)),
            _ => false,
        }
    }

    /// A canonical loop test, before its body: the `If` after a scalar
    /// prefix one of whose branches only breaks out of the loop, and
    /// whether that branch is the first.
    fn list_loop_guard(&self, l: u32) -> Option<(u32, bool)> {
        let (test, prefix, [first, second]) = hd_tir::own::loop_test(self.b, l, true)?;
        let mut budget = self.b.len();
        let scalar = hd_tir::own::loop_statements(self.b, l)
            .take(prefix)
            .all(|x| self.scalar_loop_test(x, &mut budget));
        (first != second && scalar).then_some((test, first))
    }

    /// Follow immutable scalar aliases and fields of a just-built range.
    /// A changed or addressable binding never supplies a range fact.
    fn range_source(&self, mut r: u32, sets: &[Vec<(u32, u32)>]) -> u32 {
        for _ in 0..32 {
            let Some(i) = Ref(r).as_inst() else { break };
            let [a, w] = self.b.data[i.0 as usize];
            match self.b.tags[i.0 as usize] {
                Tag::Read
                    if let Some(l) = super::shape::read_local(self.b, r)
                        && !self.is_cell(l)
                        && self.b.local_flags[l]
                            & (hd_tir::ir::local_flags::CAPTURED
                                | hd_tir::ir::local_flags::CAPTURED_ASSIGNED)
                            == 0 =>
                {
                    let [(set, value)] = sets[l].as_slice() else {
                        break;
                    };
                    if !self.dominates(*set, r) {
                        break;
                    }
                    r = *value;
                }
                Tag::Coerce if self.b.record(w).first() == Some(&(Coercion::Identity as u32)) => {
                    r = a;
                }
                Tag::Read
                    if let Some((from, super::shape::Projected::Field)) =
                        self.projection(i.0 as usize)
                        && let Some((_, Step::Field(k) | Step::DerefField(k))) =
                            self.b.read_step(i.0 as usize) =>
                {
                    let base = self.range_source(from, sets);
                    let Some(bi) = Ref(base).as_inst() else { break };
                    if self.b.tags[bi.0 as usize] != Tag::NewStruct {
                        break;
                    }
                    let Some(&value) = self.b.record(self.b.data[bi.0 as usize][1]).get(k as usize)
                    else {
                        break;
                    };
                    r = value;
                }
                _ => break,
            }
        }
        r
    }

    /// Exact usize induction from zero to an unchanged list length.
    /// This is a body-local proof; no callee summaries or unchecked TIR.
    fn range_indices(&self, loop_inst: u32, test: u32, body: &[u32]) -> Vec<u32> {
        let sets = self.local_sets();
        let cond = self.b.data[test as usize][0];
        if Ref(cond).as_inst().is_none()
            || self.b.tags[cond as usize] != Tag::Prim
            || self.b.data[cond as usize][0] != PrimOp::Lt as u32
        {
            return Vec::new();
        }
        let args = self.b.record(self.b.data[cond as usize][1]);
        let [cur, end] = args else { return Vec::new() };
        let (Some(current), Some(bound)) = (
            super::shape::read_local(self.b, *cur),
            super::shape::read_local(self.b, *end),
        ) else {
            return Vec::new();
        };
        if self.ty_of(*cur) != Ty::prim(hd_types::Prim::Usize)
            || self.ty_of(*end) != Ty::prim(hd_types::Prim::Usize)
        {
            return Vec::new();
        }
        if self.is_cell(current)
            || self.is_cell(bound)
            || [current, bound]
                .iter()
                .any(|l| self.b.local_flags[*l] & hd_tir::ir::local_flags::CAPTURED_ASSIGNED != 0)
        {
            return Vec::new();
        }
        let [a, b] = sets[current].as_slice() else {
            return Vec::new();
        };
        let (init, step) = if self.in_loop(a.0, loop_inst) {
            (*b, *a)
        } else {
            (*a, *b)
        };
        if self.in_loop(init.0, loop_inst)
            || !self.in_loop(step.0, loop_inst)
            || !self.dominates(init.0, loop_inst)
            || self.const_u32(self.range_source(init.1, &sets)) != Some(0)
            || Ref(step.1).as_inst().is_none()
            || self.b.tags[step.1 as usize] != Tag::Prim
            || self.b.data[step.1 as usize][0] != PrimOp::Add as u32
        {
            return Vec::new();
        }
        let add = self.b.record(self.b.data[step.1 as usize][1]);
        let [before, one] = add else {
            return Vec::new();
        };
        if super::shape::read_local(self.b, *before) != Some(current)
            || self.const_u32(*one) != Some(1)
        {
            return Vec::new();
        }
        let [(bound_set, value)] = sets[bound].as_slice() else {
            return Vec::new();
        };
        if !self.dominates(*bound_set, loop_inst) {
            return Vec::new();
        }
        let len = self.range_source(*value, &sets);
        let Some(li) = Ref(len).as_inst() else {
            return Vec::new();
        };
        let [op, rec] = self.b.data[li.0 as usize];
        if self.b.tags[li.0 as usize] != Tag::Intrinsic
            || op != IntrinsicOp::ListLen as u32
            || !self.dominates(len, loop_inst)
        {
            return Vec::new();
        }
        let Some(&list) = self.b.record(rec).first() else {
            return Vec::new();
        };
        let Some(root) = self.list_root(list) else {
            return Vec::new();
        };
        let aliases = self.list_aliases(root);
        if !self.list_loop_safe(root, body, &aliases) {
            return Vec::new();
        }
        // Length observation and loop entry can be separated by arbitrary
        // statements. A pop there invalidates the fact just as one in the
        // loop does. Include nested statements of that prelude.
        let (blk, start) = self.block_path(len).expect("dominating length")[0];
        let Some(finish) = self
            .block_path(loop_inst)
            .and_then(|p| p.into_iter().find(|(k, _)| *k == blk))
            .map(|(_, p)| p)
        else {
            return Vec::new();
        };
        let prelude: Vec<_> = (0..self.b.len())
            .map(|i| u32::try_from(i).expect("insts"))
            .filter(|&x| {
                self.block_path(x).is_some_and(|p| {
                    p.iter()
                        .any(|(k, at)| *k == blk && start < *at && *at <= finish)
                })
            })
            .collect();
        if !self.list_loop_safe(root, &prelude, &aliases) {
            return Vec::new();
        }
        body.iter()
            .copied()
            .filter(|&x| {
                let (list, index) = match self.b.element_read(x as usize) {
                    Some((list, index)) => (self.list_root(list), index),
                    None => match self.rooted_elem(x as usize) {
                        Some((list, index)) if self.b.tags[x as usize] == Tag::Read => {
                            (Some(list), index)
                        }
                        _ => return false,
                    },
                };
                if list != Some(root) || self.ty_of(index) != Ty::prim(hd_types::Prim::Usize) {
                    return false;
                }
                let index = self.range_source(index, &sets);
                super::shape::read_local(self.b, index) == Some(current)
                    && self.dominates(cond, index)
                    && self.dominates(index, step.0)
                    && self.dominates(index, x)
            })
            .collect()
    }

    /// Every store into each local or a field of it, with the value of a
    /// store into all of it (`NONE` for a field's).
    fn local_sets(&self) -> Vec<Vec<(u32, u32)>> {
        let mut sets = vec![Vec::new(); self.b.local_ty.len()];
        for i in 0..self.b.len() {
            if let Some((local, whole)) = self.b.stored_local(i) {
                let value = if whole { self.b.data[i][1] } else { NONE };
                sets[local.idx()].push((u32::try_from(i).expect("insts"), value));
            }
        }
        sets
    }

    /// Whether `usize` local `l` changes only by its own stores: no cell,
    /// no captured assignment and no reference to it.
    fn plain_index_local(&self, l: usize) -> bool {
        !self.is_cell(l)
            && self.b.local_flags[l]
                & (hd_tir::ir::local_flags::CAPTURED | hd_tir::ir::local_flags::CAPTURED_ASSIGNED)
                == 0
            && !(0..self.b.len()).any(|i| super::shape::ref_local(self.b, i) == Some(l))
    }

    /// The list and `usize` index of element access `x`: an element read
    /// or store of a list value (`[%xs [%i]]`), or of a local's list or
    /// the list its reference refers to ([`Em::rooted_elem`]).
    fn elem_access(&self, x: usize) -> Option<(ListRoot, u32)> {
        let (root, index) = if let Some((list, index)) = self.b.element_read(x) {
            (self.list_root(list)?, index)
        } else if let Some(list) = super::shape::elem_store_list(self.b, x) {
            let p = self.b.decode_place(self.b.data[x][0])?;
            let Some(&Proj::Elem(index)) = p.proj.first() else {
                return None;
            };
            (self.list_root(list)?, index.0)
        } else {
            let (root, index) = self.rooted_elem(x)?;
            let mut ty = self.sub(self.b.local_ty[root.local as usize]);
            if root.indirect {
                ty = self.ref_target(ty)?.0;
            }
            self.list_root_ty(ty)?;
            (root, index)
        };
        (self.ty_of(index) == Ty::prim(hd_types::Prim::Usize)).then_some((root, index))
    }

    /// The instructions that may run after `d` and before `j`, which `d`
    /// dominates: all of every statement of `d`'s block from `d`'s own
    /// through the one that holds `j`. A path from `d` to `j` that left
    /// those statements would reach `j` again only through the start of
    /// the block, and so through `d`.
    fn between(&self, d: u32, j: u32) -> Option<Vec<u32>> {
        let blk = self.topo.blk[d as usize];
        let from = self.topo.pos[d as usize] as usize;
        let to = self
            .block_path(j)?
            .into_iter()
            .find(|(k, _)| *k == blk)
            .map(|(_, at)| at)?;
        Some(
            (0..self.b.len())
                .map(|i| u32::try_from(i).expect("insts"))
                .filter(|&x| {
                    self.block_path(x).is_some_and(|p| {
                        p.iter().any(|&(k, at)| k == blk && from <= at && at <= to)
                    })
                })
                .collect(),
        )
    }

    /// Whether instruction `x` runs only once loop test `test`, an `If`
    /// whose second arm leaves the loop ([`Em::list_loop_guard`]), went
    /// on: in its first arm or after it.
    fn after_test(&self, test: u32, x: u32) -> bool {
        let go_on = self
            .b
            .record(self.b.data[test as usize][1])
            .first()
            .copied();
        self.dominates(test, x)
            || go_on.is_some_and(|arm| {
                self.block_path(x)
                    .is_some_and(|p| p.iter().any(|(k, _)| *k == arm))
            })
    }

    /// Falling indices: in loop `loop_inst`, whose `test` goes on while
    /// `v > 0` (or `v != 0`), a `usize` local `v` that the loop changes
    /// only by one `v = v - 1` after the test, and that is set once
    /// before the loop from a local `u` that a dominating access of list
    /// `xs` used as its index, unchanged since, never grows past that
    /// index. While `xs` keeps its length, from that access through the
    /// loop, `xs[v]` anywhere in the loop and `xs[v - 1]` read after the
    /// test and before the decrement are within bounds. This body's
    /// proof, as [`Em::range_indices`]'s, never consults a callee.
    fn falling_indices(&self, loop_inst: u32, test: u32, body: &[u32]) -> Vec<u32> {
        let usize_ty = Ty::prim(hd_types::Prim::Usize);
        let cond = self.b.data[test as usize][0];
        if Ref(cond).as_inst().is_none()
            || self.b.tags[cond as usize] != Tag::Prim
            || ![PrimOp::Gt as u32, PrimOp::Ne as u32].contains(&self.b.data[cond as usize][0])
        {
            return Vec::new();
        }
        let [cur, zero] = self.b.record(self.b.data[cond as usize][1]) else {
            return Vec::new();
        };
        let Some(v) = super::shape::read_local(self.b, *cur) else {
            return Vec::new();
        };
        if self.ty_of(*cur) != usize_ty
            || self.const_u32(*zero) != Some(0)
            || !self.plain_index_local(v)
        {
            return Vec::new();
        }
        let sets = self.local_sets();
        let [a, b] = sets[v].as_slice() else {
            return Vec::new();
        };
        let (init, dec) = if self.in_loop(a.0, loop_inst) {
            (*b, *a)
        } else {
            (*a, *b)
        };
        // The decrement runs once per iteration at most, after the test.
        if self.in_loop(init.0, loop_inst)
            || !self.dominates(init.0, loop_inst)
            || self.topo.loop_of[dec.0 as usize] != loop_inst
            || Ref(dec.1).as_inst().is_none()
            || self.b.tags[dec.1 as usize] != Tag::Prim
            || self.b.data[dec.1 as usize][0] != PrimOp::Sub as u32
        {
            return Vec::new();
        }
        let [before, one] = self.b.record(self.b.data[dec.1 as usize][1]) else {
            return Vec::new();
        };
        if super::shape::read_local(self.b, *before) != Some(v)
            || self.const_u32(*one) != Some(1)
            || self.topo.loop_of[*before as usize] != loop_inst
            || !self.after_test(test, *before)
        {
            return Vec::new();
        }
        // The value `v` enters with: local `u`, read where it is set.
        let Some(u) = super::shape::read_local(self.b, init.1) else {
            return Vec::new();
        };
        if self.ty_of(init.1) != usize_ty || !self.plain_index_local(u) {
            return Vec::new();
        }
        // The lists an access checked that value against: its index reads
        // `u`, which no store changes between that read and `v`'s.
        let mut roots: Vec<ListRoot> = Vec::new();
        for x in 0..self.b.len() {
            let Some((root, index)) = self.elem_access(x) else {
                continue;
            };
            let x = u32::try_from(x).expect("insts");
            if roots.contains(&root)
                || super::shape::read_local(self.b, index) != Some(u)
                || !self.dominates(x, init.1)
            {
                continue;
            }
            let (Some(to_init), Some(mut to_loop)) =
                (self.between(index, init.0), self.between(x, loop_inst))
            else {
                continue;
            };
            if to_init.iter().any(|&y| {
                self.b
                    .stored_local(y as usize)
                    .is_some_and(|(l, _)| l.idx() == u)
            }) {
                continue;
            }
            to_loop.extend_from_slice(body);
            let aliases = self.list_aliases(root);
            if self.list_loop_safe(root, &to_loop, &aliases) {
                roots.push(root);
            }
        }
        body.iter()
            .copied()
            .filter(|&y| {
                let Some((root, index)) = self.elem_access(y as usize) else {
                    return false;
                };
                if !roots.contains(&root) {
                    return false;
                }
                // A read of `v` in the loop, which never exceeds its entry
                // value.
                let index = self.range_source(index, &sets);
                if super::shape::read_local(self.b, index) == Some(v) {
                    return self.in_loop(index, loop_inst);
                }
                // `v - 1` where the test found `v` above 0.
                if Ref(index).as_inst().is_none()
                    || self.b.tags[index as usize] != Tag::Prim
                    || self.b.data[index as usize][0] != PrimOp::Sub as u32
                {
                    return false;
                }
                let [r, one] = self.b.record(self.b.data[index as usize][1]) else {
                    return false;
                };
                super::shape::read_local(self.b, *r) == Some(v)
                    && self.const_u32(*one) == Some(1)
                    && self.after_test(test, *r)
                    && self.dominates(*r, dec.0)
            })
            .collect()
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
                        Tag::Ref | Tag::Call | Tag::CallValue | Tag::CallDyn
                    )
            });
        let n = self.b.len();
        for &x in body {
            let i = x as usize;
            let [a, _] = self.b.data[i];
            // A place rooted at a local that holds the list reaches it as
            // a read of that local would.
            let rooted = self.rooted_elem(i);
            let reaches = super::shape::uses(self.b, i)
                .iter()
                .any(|r| aliases[*r as usize])
                || rooted.is_some_and(|(list, _)| aliases[n + list.local as usize]);
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
                        hd_mono::TargetKind::Intrinsic(key) => key == "sqrt",
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
            let store = super::shape::local_store(self.b, i);
            match self.b.tags[i] {
                Tag::Store
                    if store.is_some_and(|(l, v)| {
                        l == root.local as usize || aliases.get(v as usize) == Some(&true)
                    }) =>
                {
                    return false;
                }
                Tag::Store
                    if external
                        && store.is_some_and(|(l, _)| {
                            self.counted(self.sub(self.b.local_ty[l])).is_some()
                        }) =>
                {
                    return false;
                }
                Tag::Read if external && self.b.global_read(i).is_some() => return false,
                // An element store writes through its list, as the
                // intrinsics that change one in place do.
                Tag::Store
                    if super::shape::elem_store_list(self.b, i).is_some() || rooted.is_some() =>
                {
                    if external && !reaches {
                        return false;
                    }
                }
                Tag::Store if external && store.is_none() => return false,
                Tag::Coerce
                | Tag::NewStruct
                | Tag::CopyStruct
                | Tag::NewTuple
                | Tag::NewVariant
                | Tag::NewList
                | Tag::NewMap
                | Tag::Interp
                    if external && self.counted(self.ty(x)).is_some() =>
                {
                    return false;
                }
                // A `Scope` with no suites, an inlined call's, is a block.
                Tag::Scope if !self.rec(self.b.data[i][1]).is_empty() => return false,
                Tag::Await
                | Tag::AwaitValue
                | Tag::AwaitAll
                | Tag::AwaitRace
                | Tag::Defer
                | Tag::Hook
                | Tag::Closure => return false,
                // An element read is checked as the intrinsics that read
                // a list are.
                Tag::Read
                    if external
                        && !reaches
                        && (self.b.element_read(i).is_some() || rooted.is_some())
                        && self.counted(self.ty(x)).is_some() =>
                {
                    return false;
                }
                Tag::Intrinsic if external && !reaches => match IntrinsicOp::from_u32(a) {
                    Some(IntrinsicOp::ListLen) => {}
                    // Distinct reference parameters can denote the same
                    // list. A mutation through either may replace its buffer.
                    _ => return false,
                },
                Tag::Intrinsic if reaches => match IntrinsicOp::from_u32(a) {
                    Some(IntrinsicOp::ListLen) => {}
                    _ => return false,
                },
                // A tuple element or a payload is checked as any other
                // value that reaches the list.
                Tag::Read
                    if reaches
                        && self
                            .projection(i)
                            .is_some_and(|(_, p)| p != super::shape::Projected::Field) =>
                {
                    return false;
                }
                Tag::Read | Tag::Block => {}
                _ if reaches => return false,
                _ => {}
            }
        }
        true
    }

    /// The hoisted `IsUnique`s waiting for loop `l` (`hoisted`), run once
    /// before it under its entry test, so an empty loop copies nothing
    /// (`docs/compiler/tir-v2.md`, "Hoisting"): the statements before the
    /// test, which the pass found pure, run once more, then the test. With
    /// no loop (`None`), no test, or in a suspending body, they run
    /// unguarded, which costs only a copy an empty loop did not need. One
    /// behind a dereference runs only where no reference view is reachable;
    /// elsewhere the covered ones in the loop run instead.
    pub(super) fn hoisted_uniques(&mut self, l: Option<u32>) -> StageResult<()> {
        let uniques = std::mem::take(&mut self.hoisted);
        if uniques.is_empty() {
            return Ok(());
        }
        let test = l
            .filter(|_| self.susp.is_none())
            .and_then(|l| hd_tir::own::entry_test(self.b, l).map(|e| (l, e)));
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
        for i in uniques {
            let deref = self
                .b
                .decode_place(self.b.data[i as usize][0])
                .is_some_and(|p| p.proj.contains(&hd_tir::ir::Proj::Deref));
            if deref {
                self.f.helper(Helper::RefViewsAbsent);
                self.f.op(I::If(BlockType::Empty));
                self.deref_physical = true;
                let r = self.unique_at(i);
                self.deref_physical = false;
                r?;
                self.f.op(I::End);
            } else {
                self.unique_at(i)?;
            }
        }
        if test.is_some() {
            self.f.op(I::End);
        }
        Ok(())
    }

    /// The new path's loop caches: the stable list places the hoisting pass
    /// lists for loop `l` (`Body::stable_lists`), a local's list or the
    /// list a reference local refers to, with no counted leaf in its
    /// elements, keep their buffer and length in locals for the
    /// loop, where no reference view is reachable. Their uniqueness, if the
    /// loop changes them, was a hoisted `IsUnique`'s.
    fn own_list_caches(&mut self, l: u32) -> StageResult<Vec<ListRoot>> {
        let rows = &self.b.stable_lists;
        let from = rows.partition_point(|r| r.0 < l);
        let words: Vec<u32> = rows[from..]
            .iter()
            .take_while(|r| r.0 == l)
            .map(|r| r.1)
            .collect();
        let mut roots = Vec::new();
        for word in words {
            let Some(p) = self.b.decode_place(word) else {
                continue;
            };
            let hd_tir::ir::PlaceRoot::Local(local) = p.root else {
                continue;
            };
            let indirect = match p.proj.as_slice() {
                [] => false,
                [hd_tir::ir::Proj::Deref] => true,
                _ => continue,
            };
            let mut ty = self.at_root_ty(p.root);
            if indirect {
                match self.ref_target(ty) {
                    Some((target, false)) => ty = target,
                    _ => continue,
                }
            }
            if self.is_cell(local.idx()) || self.list_root_ty(ty).is_none() {
                continue;
            }
            let (_, elem) = self.elem(ty)?;
            if elem.walk.is_some() {
                continue;
            }
            let root = ListRoot {
                local: local.raw(),
                indirect,
            };
            if !self.list_caches.iter().any(|(key, _)| *key == root) && !roots.contains(&root) {
                roots.push(root);
            }
        }
        if roots.is_empty() {
            return Ok(roots);
        }
        self.f.helper(Helper::RefViewsAbsent);
        self.f.op(I::If(BlockType::Empty));
        for &root in &roots {
            let local = self.local_value(root.local)?[0];
            self.f.get(local);
            if root.indirect {
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
            }
            let buffer = self.f.local(Val::I32);
            self.f.set(buffer);
            let len = self.f.local(Val::I32);
            self.f.get(buffer);
            self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            self.f.set(len);
            self.list_caches.push((root, ListCache { buffer, len }));
        }
        self.f.op(I::End);
        Ok(roots)
    }

    /// Loop `loop_inst` is about to start: the hoisted `IsUnique`s waiting
    /// for it run, its exact range indices are proven (§23.37), and its
    /// stable lists keep their buffer and length in locals
    /// ([`Em::own_list_caches`]). The cached lists, which the loop's exit
    /// drops from the cache.
    pub(super) fn begin_list_loop(&mut self, loop_inst: u32) -> StageResult<Vec<ListRoot>> {
        self.hoisted_uniques(Some(loop_inst))?;
        if self.susp.is_some() {
            return Ok(Vec::new());
        }
        let body: Vec<u32> = (0..self.b.len())
            .map(|i| u32::try_from(i).expect("insts"))
            .filter(|x| self.in_loop(*x, loop_inst))
            .collect();
        if let Some((test, false)) = self.list_loop_guard(loop_inst) {
            let proven = self.range_indices(loop_inst, test, &body);
            self.proven_list_indices.extend(proven);
            let proven = self.falling_indices(loop_inst, test, &body);
            self.proven_list_indices.extend(proven);
        }
        self.own_list_caches(loop_inst)
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

    /// A local holding the buffer of list or map value `r`: a loop's cached
    /// one where no reference view is reachable.
    pub(super) fn buffer(&mut self, r: u32) -> StageResult<u32> {
        let b = self.f.local(Val::I32);
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
            self.f.get(cache.buffer);
            self.f.op(I::Else);
            self.comp(r, 0)?;
            self.f.op(I::End);
            self.cached_buffers.push((b, cache.len));
        } else {
            self.comp(r, 0)?;
        }
        self.f.set(b);
        Ok(b)
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
    pub(super) fn index(&mut self, b: u32, r: u32) -> StageResult<u32> {
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
    /// buffer.
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
            self.f.call_list_buf(&e);
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
        self.store_owned(i)
    }

    /// `ListLen`.
    pub(super) fn list_len(&mut self, i: u32, list: u32) -> StageResult<()> {
        self.elem(self.ty_of(list))?;
        let b = self.buffer(list)?;
        self.buffer_len(b);
        self.store(i)
    }

    /// The element type of list type `t` and a local holding the address
    /// of its slot `ix` in the buffer in local `b`, the index checked
    /// against the length; for a map type, of the value of entry `ix`, a
    /// `MapEntry`'s number. Either address is `LIST_ELEMS` before the
    /// value.
    pub(super) fn elem_slot(&mut self, b: u32, t: Ty, ix: u32) -> StageResult<(Ty, u32)> {
        if self.is_map(t) {
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
        let ix = self.index(b, ix)?;
        Ok((et, self.slot_at(b, ix, e.size)))
    }

    /// `ListPush`: a pinned list panics (`ref.elem.pinned`); otherwise the
    /// value is stored at the length, after the buffer, which the passes'
    /// `IsUnique` made the list's own, has room for it. A buffer with no
    /// owner word and room left is the fast path: an owner word is the
    /// only way to a pin, and only a full buffer moves. Any other goes
    /// through `rt:list_grow`, which checks the pin and moves a full
    /// buffer into the operand's place, keeping the length.
    pub(super) fn list_push(&mut self, args: &[u32]) -> StageResult<()> {
        let (et, e) = self.elem(self.ty_of(args[0]))?;
        let b = self.buffer(args[0])?;
        let n = self.f.local(Val::I32);
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
        self.f.set(n);
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, LIST_OWN, 2)));
        self.f.get(n);
        self.f.get(b);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.op(I::I32Eq);
        self.f.op(I::I32Or);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(b);
        self.f.call_list_grow(&e);
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
    /// [`Helper::ListReserve`], which panics on a pinned list and copies a
    /// shared buffer; the buffer it returns goes back into the operand's
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
    /// `list_reserve(items, count)` and `bytes_append(bytes: &mut
    /// List[u8], text)`. The list is read through its reference, changed
    /// by its helper, and its buffer stored back. The other argument is
    /// lent: the helper keeps no count of it. `list_reserve` is
    /// [`Helper::ListReserve`] itself, the routine `list_extend` and
    /// `bytes_append` reserve with, behind the `count != 0` test they
    /// make too.
    pub(super) fn list_ref_change(&mut self, key: &str, args: &[u32]) -> StageResult<()> {
        let Some((lt, false)) = self.ref_target(self.ty_of(args[0])) else {
            return unsupported("a list change through a value that is no reference");
        };
        let (_, elem) = self.elem(lt)?;
        let helper = match key {
            "list_truncate" => Helper::ListTruncate { elem },
            "list_extend" => Helper::ListExtend { elem },
            "list_reserve" => Helper::ListReserve { elem },
            _ => Helper::BytesAppend { elem },
        };
        let reserve = key == "list_reserve";
        if reserve {
            self.push(args[1])?;
            self.f.op(I::If(BlockType::Empty));
        }
        // A string argument is its view: the address and the length.
        let rest = |em: &mut Self| -> StageResult<()> {
            if key == "bytes_append" {
                em.comp(args[1], 0)?;
                em.comp(args[1], 1)
            } else {
                em.push(args[1])
            }
        };
        let at = self.f.local(Val::I32);
        self.comp(args[0], 0)?;
        self.f.set(at);
        // A field reference may be a representation view: its getter's
        // copy, which the change's unique copy replaces, and the new
        // buffer stored back by its setter.
        self.f.get(at);
        self.f.helper(Helper::RefIsView);
        self.f.op(I::If(BlockType::Empty));
        let value = self.ref_layout(lt)?;
        let [list] = crate::rt::ref_load(&mut self.f, at, &value)[..] else {
            return unsupported("a list that is not one address");
        };
        self.f.get(list);
        rest(self)?;
        self.f.helper(helper.clone());
        self.f.set(list);
        crate::rt::ref_store(&mut self.f, at, &[list], &value, true);
        self.f.op(I::Else);
        self.f.get(at);
        self.f.get(at);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        rest(self)?;
        self.f.helper(helper);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        self.f.op(I::End);
        if reserve {
            self.f.op(I::End);
        }
        Ok(())
    }

    /// Gives the buffer in local `b` of a list or a map of type `t` its
    /// owner word: the function-table slot of the collection's clone glue
    /// plus one, with bit 31 when its elements hold collections (§23.20).
    pub(super) fn pin_owner(&mut self, b: u32, t: Ty) -> StageResult<()> {
        // A map's table holds its owner word where a list buffer does
        // (`ref.elem.map`): its clone glue copies the table.
        let (clone, exposed) = if self.is_map(t) {
            let (_, _, entry) = self.entry(t)?;
            let exposed = !entry.colls.is_empty();
            (Helper::MapClone { entry }, exposed)
        } else {
            let (_, e) = self.elem(t)?;
            let exposed = !e.colls.is_empty();
            (Helper::ListClone { elem: e }, exposed)
        };
        self.f.get(b);
        self.f.table_slot(Sym::Helper(clone));
        self.f.i32(1);
        self.f.op(I::I32Add);
        if exposed {
            self.f.u32(LIST_EXPOSED);
            self.f.op(I::I32Or);
        }
        self.f.op(I::I32Store(mem(HEAP, LIST_OWN, 2)));
        Ok(())
    }
}
