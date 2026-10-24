//! Lists (`crates/hd_wasm_linear/DESIGN.md` §23.7, §23.17, job 9). A list value
//! is the address of its buffer, a counted object holding the capacity,
//! the length, the owner word (§23.20) and the elements inline; a copy
//! (`ValueCopy`, `rt:coll_copy`) counts one more holder of the same
//! buffer. The ownership passes' `IsUnique` makes the buffer the list's
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
use hd_tir::ir::{Coercion, IntrinsicOp, NONE, PrimOp, Ref, Step, Tag};
use hd_types::{Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::refs::{Head, Start};
use super::shape::Through;
use super::value::Src;
use crate::layout::{coll_leaves, mem as mem_of, strip};
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
        let TyData::Adt { def, .. } = self.pool().get(strip(self.pool(), self.env(), t)) else {
            return None;
        };
        (self.env().std_kind(def) == hd_mono::layout::StdKind::List).then_some(())
    }

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
                    Tag::Ref => self.spot(u32::try_from(i).expect("insts")).is_ok_and(|s| {
                        matches!(s.head, Head::Direct | Head::Deref)
                            && matches!(s.start, Start::Local(l) if locals[l as usize])
                    }),
                    Tag::Read
                        if self.projection(i).is_none() && self.b.element_read(i).is_none() =>
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
    fn list_loop_guard(&self, loop_inst: u32, body: u32) -> Option<(u32, bool)> {
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
        let mut budget = self.b.len();
        for x in self.rec(self.b.data[body as usize][0]) {
            if self.b.tags[x as usize] == Tag::Block {
                continue;
            }
            if self.b.tags[x as usize] == Tag::If {
                let branches = self.rec(self.b.data[x as usize][1]);
                let first = is_break(branches[0]);
                let second = branches.get(1).is_some_and(|b| is_break(*b));
                return (first != second).then_some((x, first));
            }
            if !self.scalar_loop_test(x, &mut budget) {
                return None;
            }
        }
        None
    }

    /// Follow immutable scalar aliases and fields of a just-built range.
    /// A changed or addressable binding never supplies a range fact.
    fn range_source(&self, mut r: u32, sets: &[Vec<(u32, u32)>], tree: &super::shape::Tree) -> u32 {
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
                    if !tree.dominates(self.b, *set, r) {
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
                    let base = self.range_source(from, sets, tree);
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
    fn range_indices(
        &self,
        loop_inst: u32,
        test: u32,
        tree: &super::shape::Tree,
        body: &[u32],
    ) -> Vec<u32> {
        // Every store into a local or a field of it, with the value of a
        // store into all of it (`NONE` for a field's).
        let mut sets = vec![Vec::new(); self.b.local_ty.len()];
        for i in 0..self.b.len() {
            if let Some((local, whole)) = self.b.stored_local(i) {
                let value = if whole { self.b.data[i][1] } else { NONE };
                sets[local.idx()].push((u32::try_from(i).expect("insts"), value));
            }
        }
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
        let (init, step) = if tree.in_loop(self.b, a.0, loop_inst) {
            (*b, *a)
        } else {
            (*a, *b)
        };
        if tree.in_loop(self.b, init.0, loop_inst)
            || !tree.in_loop(self.b, step.0, loop_inst)
            || !tree.dominates(self.b, init.0, loop_inst)
            || self.const_u32(self.range_source(init.1, &sets, tree)) != Some(0)
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
        if !tree.dominates(self.b, *bound_set, loop_inst) {
            return Vec::new();
        }
        let len = self.range_source(*value, &sets, tree);
        let Some(li) = Ref(len).as_inst() else {
            return Vec::new();
        };
        let [op, rec] = self.b.data[li.0 as usize];
        if self.b.tags[li.0 as usize] != Tag::Intrinsic
            || op != IntrinsicOp::ListLen as u32
            || !tree.dominates(self.b, len, loop_inst)
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
        let (blk, start) = tree.path(self.b, len).expect("dominating length")[0];
        let Some(finish) = tree
            .path(self.b, loop_inst)
            .and_then(|p| p.into_iter().find(|(k, _)| *k == blk))
            .map(|(_, p)| p)
        else {
            return Vec::new();
        };
        let prelude: Vec<_> = (0..self.b.len())
            .map(|i| u32::try_from(i).expect("insts"))
            .filter(|&x| {
                tree.path(self.b, x).is_some_and(|p| {
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
                let index = self.range_source(index, &sets, tree);
                super::shape::read_local(self.b, index) == Some(current)
                    && tree.dominates(self.b, cond, index)
                    && tree.dominates(self.b, index, step.0)
                    && tree.dominates(self.b, index, x)
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
                    Some(IntrinsicOp::ValueCopy)
                        if coll_leaves(self.pool(), self.env(), self.ty(x)).is_empty() => {}
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
            let body = self.b.data[l as usize][0];
            let prefix: Vec<u32> = self
                .rec(self.b.data[body as usize][0])
                .into_iter()
                .filter(|x| self.b.tags[*x as usize] != Tag::Block)
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
            let mut ty = self.start_ty(super::refs::Start::Local(local.raw()));
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

    /// Loop `loop_inst`, whose body is block `block`, is about to start:
    /// the hoisted `IsUnique`s waiting for it run, its exact range indices
    /// are proven (§23.37), and its stable lists keep their buffer and
    /// length in locals ([`Em::own_list_caches`]). The cached lists,
    /// which the loop's exit drops from the cache.
    pub(super) fn begin_list_loop(
        &mut self,
        loop_inst: u32,
        block: u32,
    ) -> StageResult<Vec<ListRoot>> {
        self.hoisted_uniques(Some(loop_inst))?;
        if self.susp.is_some() {
            return Ok(Vec::new());
        }
        let tree = super::shape::Tree::new(self.b);
        let body: Vec<u32> = (0..self.b.len())
            .map(|i| u32::try_from(i).expect("insts"))
            .filter(|x| tree.in_loop(self.b, *x, loop_inst))
            .collect();
        if let Some((test, false)) = self.list_loop_guard(loop_inst, block) {
            let proven = self.range_indices(loop_inst, test, &tree, &body);
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
            | Tag::NewStruct
            | Tag::CopyStruct
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
            self.f.helper(Helper::CollCopy);
            self.f.set(tmp);
            self.temps.push(tmp);
            ls[c as usize] = tmp;
        }
        Ok(Src::Locals(ls))
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
        self.store_owned(i)
    }

    /// `ListLen`.
    pub(super) fn list_len(&mut self, i: u32, list: u32) -> StageResult<()> {
        self.elem(self.ty_of(list))?;
        let b = self.buffer(list)?;
        self.buffer_len(b);
        self.store(i)
    }

    /// An element read, `Read [%xs [%i]]`, of the list `list` at `index`,
    /// or a map value's, `Read [%m [%e]]` at the number `%e` of its entry.
    pub(super) fn list_index(&mut self, i: u32, list: u32, index: u32) -> StageResult<()> {
        let t = self.ty_of(list);
        if self.is_map(t) {
            let m = self.buffer(list)?;
            let (vt, at) = self.map_value_at(m, t, index)?;
            for l in mem_of(self.pool(), self.env(), vt)?.leaves {
                self.f.get(at);
                self.f.op(l.load(0));
            }
            return self.store(i);
        }
        let (_, e) = self.elem(t)?;
        let b = self.buffer(list)?;
        let ix = if self.proven_list_indices.contains(&i) {
            // Typed views keep the ordinary checked path. The linker
            // folds this guard away in programs without any views.
            let ix = self.f.local(Val::I32);
            self.f.helper(Helper::RefViewsAbsent);
            self.f.op(I::If(BlockType::Result(Val::I32.wasm())));
            self.comp(index, 0)?;
            self.f.op(I::Else);
            let checked = self.index(b, index)?;
            self.f.get(checked);
            self.f.op(I::End);
            self.f.set(ix);
            ix
        } else {
            self.index(b, index)?
        };
        let at = self.slot_at(b, ix, e.size);
        for l in &e.leaves {
            self.f.get(at);
            self.f.op(l.load(LIST_ELEMS));
        }
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

    /// A `Store` into element `ix` of the list value `list`, or into the
    /// struct field `path` reaches in it (`[%xs [%i] .p .x]`), whose
    /// buffer the passes' `IsUnique` made the list's own; or into a map
    /// value's, `[%m [%e] .p .x]` at entry `%e`. After the bounds check,
    /// the place's old value is released after the new one is counted,
    /// and no other field is read or written (#655). A pin does not stop
    /// it: the length stays.
    pub(super) fn elem_store(
        &mut self,
        list: u32,
        ix: u32,
        path: &[u32],
        v: u32,
    ) -> StageResult<()> {
        let t = self.ty_of(list);
        let b = self.buffer(list)?;
        // The element's address, `off` bytes before it, and its leaves: a
        // list's slot after the bounds check, or a map's value at its
        // entry (`types.coll.map-value.place`).
        let (et, all, at, off) = if self.is_map(t) {
            let (vt, at) = self.map_value_at(b, t, ix)?;
            (vt, mem_of(self.pool(), self.env(), vt)?.leaves, at, 0)
        } else {
            let (et, e) = self.elem(t)?;
            let ix = self.index(b, ix)?;
            (et, e.leaves, self.slot_at(b, ix, e.size), LIST_ELEMS)
        };
        // The place's values among the element's, and its type.
        let (mut first, mut count, mut ft) = (0, all.len(), et);
        for &k in path {
            let (_, parts) = crate::layout::struct_mem(self.pool(), self.env(), ft)?;
            let Some(part) = parts.get(k as usize) else {
                return unsupported("a field store past the struct's fields");
            };
            first += part.first;
            count = part.count;
            ft = self.struct_fields(ft)[k as usize];
        }
        let s = self.slot_src(v, ft, false)?;
        self.retain_src(&s, ft)?;
        let leaves = all[first..first + count].to_vec();
        if self.counted(ft).is_some() {
            let old: Vec<u32> = leaves.iter().map(|l| self.f.local(l.val)).collect();
            for (l, o) in leaves.iter().zip(&old) {
                self.f.get(at);
                self.f.op(l.load(off));
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
                off,
                *l,
                &mut |em: &mut Self| em.src(&s, k),
            )?;
        }
        Ok(())
    }

    /// `ListPush`: a pinned list panics (`ref.elem.pinned`); otherwise the
    /// value is stored at the length, after the buffer, which the passes'
    /// `IsUnique` made the list's own, has room for it.
    pub(super) fn list_push(&mut self, args: &[u32]) -> StageResult<()> {
        let (et, e) = self.elem(self.ty_of(args[0]))?;
        let b = self.buffer(args[0])?;
        self.f.get(b);
        self.f.helper(Helper::ListPinned);
        self.f.op(I::If(BlockType::Empty));
        self.f.helper(Helper::Panic(
            "collection-pinned: list length changed while pinned".into(),
        ));
        self.f.op(I::End);
        // Only a full buffer moves (`rt:list_grow`), into the operand's
        // place.
        self.f.get(b);
        self.f.helper(Helper::ListGrow { elem: e.clone() });
        self.f.set(b);
        self.put_back(args[0], b)?;
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

    /// `ValueCopy`: each collection in the value becomes its copy
    /// (`rt:coll_copy`); a value holding none is itself.
    pub(super) fn value_copy(&mut self, i: u32, r: u32) -> StageResult<()> {
        // The copy's own type says what it copies. A list or a map copies
        // through its `mut` view, which only a `mut self` receiver has,
        // since no written type is one; a struct's `mut` view, a
        // provider's, stays shared.
        let mut t = self.ty(i);
        let s = strip(self.pool(), self.env(), t);
        if matches!(self.pool().get(s), TyData::Adt { def, .. }
            if matches!(self.env().std_kind(def), StdKind::List | StdKind::Map))
        {
            t = s;
        }
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
            self.f.helper(Helper::CollCopy);
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
        self.f.helper(Helper::CollCopy);
        self.store_owned(i)
    }

    /// `SnapEnd`: the loop over `snap` ended, so the snapshot gives up its
    /// buffer at once, and the collection's next change copies only if
    /// something else still shares it. A snapshot read with a count gives
    /// it up as any operand does; one only borrowed from its local leaves
    /// the static empty buffer there.
    pub(super) fn snap_end(&mut self, snap: u32) -> StageResult<()> {
        let i = snap as usize;
        if Ref(snap).as_inst().is_none()
            || self.b.tags[i] != Tag::Read
            || self.b.data[i][1] != hd_tir::ir::ReadMode::Borrow as u32
        {
            return Ok(());
        }
        let b = self.buffer(snap)?;
        let empty = self.f.local(Val::I32);
        let at = empty_buffer(self.m);
        self.f.static_addr(at);
        self.f.set(empty);
        self.put_back(snap, empty)?;
        self.f.get(b);
        self.f.helper(Helper::ReleaseRef);
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
