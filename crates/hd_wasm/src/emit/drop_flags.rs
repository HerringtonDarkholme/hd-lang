//! Initialization state for inline Drop hooks. Zero is a valid payload,
//! so empty owned places need state independent of their value bits.
//!
//! The flags are TIR statements (`hd_tir::own::drop_flags`). A value
//! root's flag is a TIR local. A cell's flags are slots after its value,
//! one per node of its type in path order, which every body sharing the
//! cell reads, and a hook at node P runs when P's slot holds: a clear
//! empties the slots at, under and above its path, and a set fills the
//! slots at and under it, then each node above whose children all hold.

use std::rc::Rc;

use hd_base::SliceSort as _;
use hd_base::StageResult;
use hd_tir::ir::{Place, PlaceRoot, Proj, Ref, Tag};
use hd_types::Ty;

use super::Em;
use crate::layout::Kind;
use crate::layout::Leaf;
use crate::module::Val;
use crate::module::{HEAP, mem};
use crate::op::Op as I;
use crate::rt::DropCall;

/// Per value root, each flagged move path with its TIR flag local, read
/// from one sub-body's flag statements and sorted by [`path_order`], so the
/// paths under one path are one range.
pub(super) type TirFlags = hd_base::HashMap<Root, Vec<(Vec<Proj>, u32)>>;

/// A cell's flag slots after its value: their offset, and one node
/// path per slot.
pub(super) struct CellLayout {
    base: u32,
    pub(super) paths: Vec<Vec<Proj>>,
}

impl CellLayout {
    fn offset(&self, k: usize) -> u32 {
        self.base + u32::try_from(k).expect("ruled out: a type has under 2^32 nodes") * 4
    }

    /// The offset of node `path`'s slot: one binary search.
    fn slot(&self, path: &[Proj]) -> Option<u32> {
        let k = self.paths.binary_search_by(|p| path_order(p, path));
        Some(self.offset(k.ok()?))
    }

    /// The slots at and under `path`: one binary search, then the
    /// contiguous range of its subtree.
    fn under(&self, path: &[Proj]) -> std::ops::Range<usize> {
        let start = self.paths.partition_point(|p| path_order(p, path).is_lt());
        let len = self.paths[start..]
            .iter()
            .take_while(|p| p.starts_with(path))
            .count();
        start..start + len
    }
}

/// Lexicographic order of paths: a path sorts right before the paths
/// under it.
fn path_order(a: &[Proj], b: &[Proj]) -> std::cmp::Ordering {
    let step = |p: &Proj| match *p {
        Proj::Field(i) => (0, i),
        Proj::Variant(v) => (1, v),
        Proj::Deref => (2, 0),
        Proj::Elem(r) => (3, r.0),
    };
    a.iter().map(step).cmp(b.iter().map(step))
}

/// The flag of `path` among one root's sorted flagged `paths`, and the
/// flagged paths under it: one binary search, then its subtree's range.
pub(super) fn path_flags<'p>(
    paths: &'p [(Vec<Proj>, u32)],
    path: &[Proj],
) -> (Option<u32>, &'p [(Vec<Proj>, u32)]) {
    let start = paths.partition_point(|(p, _)| path_order(p, path).is_lt());
    let len = paths[start..]
        .iter()
        .take_while(|(p, _)| p.starts_with(path))
        .count();
    match &paths[start..start + len] {
        [(p, flag), under @ ..] if p.len() == path.len() => (Some(*flag), under),
        under => (None, under),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Root {
    Local(usize),
    Value(u32),
}

impl Em<'_, '_, '_> {
    fn drop_root(&self, p: &Place) -> Option<(Root, Ty)> {
        if !p
            .proj
            .iter()
            .all(|p| matches!(p, Proj::Field(_) | Proj::Variant(_)))
        {
            return None;
        }
        match p.root {
            PlaceRoot::Local(l) => Some((Root::Local(l.idx()), self.b.local_ty[l.idx()])),
            PlaceRoot::Value(r) => Some((Root::Value(r.0), self.ty_of(r.0))),
            _ => None,
        }
    }

    /// This sub-body's TIR flags, read on first use: one walk of its rows,
    /// then one sort per root, O(S log S) for S flag statements.
    pub(super) fn tir_flags(&mut self) -> std::rc::Rc<TirFlags> {
        if let Some(flags) = &self.tir_flags {
            return std::rc::Rc::clone(flags);
        }
        let mut out = TirFlags::default();
        for &i in self.rows.iter() {
            if !matches!(self.b.tags[i], Tag::FlagSet | Tag::FlagClear) {
                continue;
            }
            let [word, flag] = self.b.data[i];
            if let Some(p) = self.b.decode_place(word)
                && let Some((root, _)) = self.drop_root(&p)
            {
                out.entry(root).or_default().push((p.proj, flag));
            }
        }
        // The flag pass (`hd_tir::own::drop_flags`) gives each flag local
        // one path: equal paths are one flag's statements.
        for paths in out.values_mut() {
            paths.sort_stable_by(|a, b| path_order(&a.0, &b.0));
            paths.dedup_by(|a, b| a.0 == b.0);
        }
        let out = std::rc::Rc::new(out);
        self.tir_flags = Some(std::rc::Rc::clone(&out));
        out
    }

    /// Whether this instance reads value root `root`'s TIR flags: its
    /// value holds a hook and the body owns it. A lent parameter's, a
    /// cell's and an address parameter's are never read.
    fn tir_flagged(&self, root: Root, ty: Ty) -> bool {
        // A local kept in memory reads its flags at its release
        // ([`Em::mem_release`]).
        if let Root::Local(l) = root
            && self.mem_locals.contains_key(&l)
        {
            return !self.borrowed.contains(&l) && self.released(ty);
        }
        matches!(self.kind(ty), Kind::Value)
            && crate::layout::holds_drop(self.pool(), self.env(), ty)
            && match root {
                Root::Local(l) => {
                    !self.is_cell(l)
                        && !self.borrowed.contains(&l)
                        && !self.mem_locals.contains_key(&l)
                }
                Root::Value(_) => true,
            }
    }

    /// Local `root`'s cell address local and slots, when it is a cell.
    fn cell_slots(&self, root: Root) -> Option<(u32, Rc<CellLayout>)> {
        let Root::Local(l) = root else {
            return None;
        };
        let c = *self.locals.get(&l)?.first()?;
        self.is_cell(l)
            .then(|| self.cell_layout(self.b.local_ty[l]))
            .flatten()
            .map(|layout| (c, layout))
    }

    /// A flag statement on the cell at `c`, for node `path` at depth d:
    /// O(d log N) searches of the type's N nodes, and for a set, one walk
    /// of each node above it, O(d N), bounded by the type, not the body.
    fn cell_statement(&mut self, c: u32, cell: &CellLayout, path: &[Proj], set: bool) {
        for k in cell.under(path) {
            self.write_slot(c, cell.offset(k), |em| em.f.u32(u32::from(set)));
        }
        let above = (0..path.len())
            .rev()
            .filter_map(|d| Some((d, cell.slot(&path[..d])?)));
        for (depth, at) in above {
            let children: Vec<u32> = (cell.under(&path[..depth]))
                .filter(|&k| cell.paths[k].len() == depth + 1)
                .map(|k| cell.offset(k))
                .collect();
            self.write_slot(c, at, |em| {
                em.f.u32(u32::from(set));
                for &child in children.iter().filter(|_| set) {
                    em.f.get(c);
                    em.f.op(I::I32Load(mem(HEAP, u64::from(child), 2)));
                    em.f.op(I::I32And);
                }
            });
        }
    }

    /// `FlagSet` or `FlagClear` `i`: writes its flag local, when this
    /// instance reads it, or a cell's slots under its path.
    pub(super) fn flag_statement(&mut self, i: u32) -> StageResult<()> {
        let [word, flag] = self.b.data[i as usize];
        let Some(p) = self.b.decode_place(word) else {
            return Ok(());
        };
        let Some((root, ty)) = self.drop_root(&p) else {
            return Ok(());
        };
        let set = u32::from(self.b.tags[i as usize] == Tag::FlagSet);
        ledger!(if self.cell_slots(root).is_some() => "flag-stmt/cell-slot"
            else if self.tir_flagged(root, ty) => "flag-stmt/local" else "flag-stmt/ignored-here");
        if let Some((c, cell)) = self.cell_slots(root) {
            // A slot's whole move zeroes its one holder's address.
            let slot = matches!(root, Root::Local(l) if self.slots.contains_key(&l));
            if set == 1 || !p.proj.is_empty() || !slot {
                self.cell_statement(c, &cell, &p.proj, set == 1);
            }
        } else if self.tir_flagged(root, ty) {
            let local = self.local(flag)?[0];
            self.f.u32(set);
            self.f.set(local);
        }
        Ok(())
    }

    /// Pushes the guard of a hook at path `full` of `root`: its nearest
    /// flagged path, at or above it, and each flagged path under it, all
    /// set, or the constant 1 when none is flagged. Binary searches of
    /// the root's sorted paths: O(d log P) for a hook at depth d, plus the
    /// paths under it.
    fn push_hook_guard(&mut self, paths: &[(Vec<Proj>, u32)], full: &[Proj]) -> StageResult<()> {
        let nearest = (0..=full.len()).rev().find_map(|k| {
            let at = paths.binary_search_by(|(p, _)| path_order(p, &full[..k]));
            at.ok().map(|i| paths[i].1)
        });
        let start = paths.partition_point(|(p, _)| path_order(p, full).is_le());
        let below = paths[start..]
            .iter()
            .take_while(|(p, _)| p.starts_with(full))
            .map(|&(_, f)| f);
        let mut first = true;
        for flag in nearest.into_iter().chain(below) {
            self.count_flag_read();
            let local = self.local(flag)?[0];
            self.f.get(local);
            if !std::mem::take(&mut first) {
                self.f.op(I::I32And);
            }
        }
        if first {
            self.f.u32(1);
        }
        Ok(())
    }

    /// Counts one read of a TIR drop flag for the ledger, by the
    /// statement whose lowering reads it: a `Drop`, a `Store` releasing
    /// what it overwrites, or another (an exit or unwind release).
    #[cfg_attr(not(feature = "ledger"), allow(clippy::unused_self))]
    pub(super) fn count_flag_read(&self) {
        ledger!(
            "drop-flag-read/{}",
            match self.emitting.last().map(|&i| self.b.tags[i as usize]) {
                Some(Tag::Drop) => "drop",
                Some(Tag::Store) => "overwrite",
                _ => "exit-or-unwind",
            }
        );
    }

    pub(super) fn release_drop_place(&mut self, ls: &[u32], ty: Ty, p: &Place) -> StageResult<()> {
        let root = self.drop_root(p);
        if let Some((root, _)) = &root {
            self.assert_cancelled(root);
        }
        // One flag per hook, whatever guards it: every release of a type
        // shares one drop helper, and the optimizer folds a constant 1.
        let mut flags = Vec::new();
        if let Some((root, rt)) = root
            && matches!(self.kind(rt), Kind::Value)
            && crate::layout::holds_drop(self.pool(), self.env(), rt)
        {
            let cell = self.cell_slots(root);
            let tir = self.tir_flags();
            let paths = match (&cell, self.tir_flagged(root, rt)) {
                (None, true) => tir.get(&root).map_or(&[][..], Vec::as_slice),
                _ => &[],
            };
            for (_, child) in self.drop_paths(ty) {
                let full: Vec<_> = p.proj.iter().chain(&child).copied().collect();
                match &cell {
                    Some((c, cell)) => {
                        let at = cell.slot(&full).ok_or_else(|| {
                            hd_base::NotImplemented::bug(
                                hd_base::Stage::Emit,
                                "a cell's hook at a node its flags lack",
                            )
                        })?;
                        self.f.get(*c);
                        self.f.op(I::I32Load(mem(HEAP, u64::from(at), 2)));
                    }
                    None => self.push_hook_guard(paths, &full)?,
                }
                let local = self.f.local(Val::I32);
                self.f.set(local);
                flags.push(Some(local));
            }
        }
        if !self.drop_inline_flags(ls, ty, &flags)? {
            self.release_locals(ls, ty)?;
        }
        Ok(())
    }

    pub(super) fn store_drop_place(&self) -> Option<Place> {
        let i = *self.emitting.last()? as usize;
        (self.b.tags[i] == Tag::Store)
            .then(|| self.b.decode_place(self.b.data[i][0]))
            .flatten()
    }

    pub(super) fn release_old_drop(&mut self, ls: &[u32], ty: Ty) -> StageResult<()> {
        self.because(hd_tir::own::Reason::Overwritten, |em| {
            match em.store_drop_place() {
                Some(p) => em.release_drop_place(ls, ty, &p),
                None => em.release_locals(ls, ty),
            }
        })
    }

    pub(super) fn value_place(value: u32) -> Place {
        Place {
            root: PlaceRoot::Value(Ref(value)),
            proj: Vec::new(),
        }
    }

    fn drop_node_paths(&self, ty: Ty) -> Vec<Vec<Proj>> {
        fn add(em: &Em<'_, '_, '_>, ty: Ty, path: &[Proj], out: &mut Vec<Vec<Proj>>) {
            out.push(path.to_vec());
            if !matches!(em.kind(ty), Kind::Value) {
                return;
            }
            if let Some(variants) = em.try_variant_tys(ty) {
                for (v, fields) in variants.iter().enumerate() {
                    let mut variant = path.to_vec();
                    variant.push(Proj::Variant(u32::try_from(v).expect("variant")));
                    out.push(variant.clone());
                    for (f, &field) in fields.iter().enumerate() {
                        let mut child = variant.clone();
                        child.push(Proj::Field(u32::try_from(f).expect("field")));
                        add(em, field, &child, out);
                    }
                }
            } else {
                for (f, field) in crate::layout::value_parts(
                    em.pool(),
                    em.env(),
                    crate::layout::strip_newtype(em.pool(), em.env(), ty),
                )
                .into_iter()
                .enumerate()
                {
                    let mut child = path.to_vec();
                    child.push(Proj::Field(u32::try_from(f).expect("field")));
                    add(em, field, &child, out);
                }
            }
        }
        if !matches!(self.kind(ty), Kind::Value)
            || !crate::layout::holds_drop(self.pool(), self.env(), ty)
        {
            return Vec::new();
        }
        let mut paths = Vec::new();
        add(self, ty, &[], &mut paths);
        paths
    }

    /// The flag slots of a cell holding a `ty`: their offset in the
    /// cell, and the node path of each in [`path_order`], which is the
    /// preorder [`Em::drop_node_paths`] walks. None when `ty` has no hook.
    /// Built once per type and emitter.
    pub(super) fn cell_layout(&self, ty: Ty) -> Option<Rc<CellLayout>> {
        if let Some(layout) = self.cell_layouts.borrow().get(&ty) {
            return layout.clone();
        }
        let paths = self.drop_node_paths(ty);
        let layout = crate::layout::mem(self.pool(), self.env(), ty)
            .ok()
            .filter(|_| !paths.is_empty())
            .map(|m| {
                Rc::new(CellLayout {
                    base: m.size.next_multiple_of(4),
                    paths,
                })
            });
        self.cell_layouts.borrow_mut().insert(ty, layout.clone());
        layout
    }

    /// The cell's release glue: each hook guarded by its node's slot.
    pub(super) fn cell_drop_calls(&self, ty: Ty) -> Vec<DropCall> {
        let cell = self.cell_layout(ty);
        let mut calls = self.drop_paths(ty);
        for (call, path) in &mut calls {
            if let Some(offset) = cell.as_ref().and_then(|cell| cell.slot(path)) {
                let leaf = Leaf {
                    offset,
                    val: Val::I32,
                    bytes: 4,
                    signed: false,
                };
                call.guards.push((leaf, crate::layout::Is::Eq(1)));
            }
        }
        calls.into_iter().map(|(call, _)| call).collect()
    }

    /// Sets every slot of a new cell at `at`: whole when `init` is
    /// true, empty when false, and none for `None`.
    pub(super) fn init_cell_drop(&mut self, at: u32, ty: Ty, init: Option<bool>) {
        if let Some(live) = init
            && let Some(cell) = self.cell_layout(ty)
        {
            self.cell_statement(at, &cell, &[], live);
        }
    }

    /// Stores the value `value` pushes into the slot `offset` bytes into
    /// the cell at `at`.
    fn write_slot(&mut self, at: u32, offset: u32, value: impl FnOnce(&mut Self)) {
        self.f.get(at);
        value(self);
        self.f.op(I::I32Store(mem(HEAP, u64::from(offset), 2)));
    }

    /// Scalar storage the frame destructor reads, selected before compact
    /// offsets: the TIR flags of `cold`, what a cold frame holds; the
    /// collector traces a started frame through it.
    pub(super) fn select_frame_drop_flags(&mut self, cold: &[usize], selected: &mut [bool]) {
        let flags = self.tir_flags();
        for paths in cold.iter().filter_map(|&l| flags.get(&Root::Local(l))) {
            for (_, flag) in paths {
                for &local in self.locals.get(&(*flag as usize)).into_iter().flatten() {
                    selected[local as usize] = true;
                }
            }
        }
    }

    /// The task's destructor is a separate function. Rebind the body's
    /// initialization locals to their saved values before releasing it;
    /// returns the TIR flag locals' body bindings, to restore after.
    pub(super) fn reload_drop_flags(
        &mut self,
        positions: &[Option<(u32, Val)>],
    ) -> Vec<(usize, Vec<u32>)> {
        let load = |em: &mut Self, old: u32| {
            let local = em.f.local(Val::I32);
            if let Some((offset, _)) = positions.get(old as usize).copied().flatten() {
                em.f.get(super::susp::FRAME);
                em.f.op(I::I32Load(mem(HEAP, u64::from(offset), 2)));
                em.f.set(local);
            }
            local
        };
        let mut flags: Vec<usize> = (self.tir_flags().values().flatten())
            .map(|&(_, f)| f as usize)
            .collect();
        flags.sort_stable();
        let mut body = Vec::new();
        for flag in flags {
            if let Some(old) = self.locals.get(&flag).cloned() {
                let new = old.iter().map(|&l| load(self, l)).collect();
                self.locals.insert(flag, new);
                body.push((flag, old));
            }
        }
        body
    }

    /// A cold frame's parameters are whole: their TIR flags start set.
    pub(super) fn save_initial_drop_flags(
        &mut self,
        roots: &[usize],
        frame: u32,
        positions: &[Option<(u32, Val)>],
    ) {
        let flags = self.tir_flags();
        for &l in roots {
            for &(_, flag) in flags.get(&Root::Local(l)).into_iter().flatten() {
                for &local in self.locals.get(&(flag as usize)).into_iter().flatten() {
                    if let Some((offset, _)) = positions.get(local as usize).copied().flatten() {
                        self.f.get(frame);
                        self.f.u32(1);
                        self.f.op(I::I32Store(mem(HEAP, u64::from(offset), 2)));
                    }
                }
            }
        }
    }

    pub(super) fn local_place(local: usize) -> Place {
        Place {
            root: PlaceRoot::Local(hd_base::LocalId::from_raw(
                u32::try_from(local).expect("local"),
            )),
            proj: Vec::new(),
        }
    }
}
