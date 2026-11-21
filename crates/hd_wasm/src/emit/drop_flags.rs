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
use crate::layout::Mem;
use crate::module::{HEAP, mem};
use crate::module::{Sym, Val};
use crate::op::Op as I;
use crate::rt::Helper;

/// Per value root, each flagged move path with its TIR flag local, read
/// from one sub-body's flag statements and sorted by [`path_order`], so the
/// paths under one path are one range.
pub(super) type TirFlags = hd_base::HashMap<Root, Vec<(Vec<Proj>, u32)>>;

/// A cell's flag slots after its value: their offset, and one node
/// path per slot.
/// Where the open drop reads whether a place is whole: TIR flag locals
/// by path, or a cell's slot words (`cell_statement`), each AND the
/// cell's whole flag.
pub(super) enum Guards {
    Tir(Vec<(Vec<Proj>, u32)>),
    Cell {
        c: u32,
        cell: Rc<CellLayout>,
        whole: Option<u32>,
        /// In a cell's glue, each loaded leaf's local and its offset in
        /// the cell: a whole node whose release runs in place gets its
        /// address there.
        at: Option<Rc<hd_base::HashMap<u32, u32>>>,
    },
}

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

    /// The flag local of slot `root`'s whole path, when a whole move may
    /// empty it: the slot keeps its address, and this says whether it
    /// holds a value, as rustc's drop flag of a maybe-moved place.
    pub(super) fn slot_flag(&mut self, root: Root) -> StageResult<Option<u32>> {
        let flags = self.tir_flags();
        match (root, flags.get(&root).and_then(|ps| ps.first())) {
            (Root::Local(l), Some((p, flag))) if p.is_empty() && self.slots.contains_key(&l) => {
                Ok(Some(self.local(*flag)?[0]))
            }
            _ => Ok(None),
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
        // A slot keeps its address after a whole move: its whole path's
        // flag local says whether it holds a value ([`Em::slot_flag`]).
        let slot =
            p.proj.is_empty() && matches!(root, Root::Local(l) if self.slots.contains_key(&l));
        ledger!(if slot => "flag-stmt/slot-whole" else if self.cell_slots(root).is_some() => "flag-stmt/cell-slot"
            else if self.tir_flagged(root, ty) => "flag-stmt/local" else "flag-stmt/ignored-here");
        if let Some((c, cell)) = self.cell_slots(root).filter(|_| set == 1 || !slot) {
            self.cell_statement(c, &cell, &p.proj, set == 1);
        }
        if slot || self.tir_flagged(root, ty) {
            let local = self.local(flag)?[0];
            self.f.u32(set);
            self.f.set(local);
        }
        Ok(())
    }

    /// Whether [`Em::open_drop`] descends the value: held in locals, one
    /// per value of `ty` (not by address), with every flag at or below
    /// `at` under fields and variants, each part's values among them.
    fn open_drop_fits(&self, g: &Guards, at: &[Proj], ty: Ty, n: usize) -> bool {
        let flagged = |p: &[Proj]| match g {
            Guards::Tir(paths) => paths.iter().any(|(q, _)| q.starts_with(p)),
            Guards::Cell { cell, .. } => cell.paths.iter().any(|q| q.starts_with(p)),
        };
        if !flagged(at) {
            return true;
        }
        // The open drop walks fields and variants only.
        let walked = match g {
            Guards::Tir(paths) => paths
                .iter()
                .filter(|(q, _)| q.starts_with(at))
                .all(|(q, _)| {
                    q[at.len()..]
                        .iter()
                        .all(|x| matches!(x, Proj::Field(_) | Proj::Variant(_)))
                }),
            Guards::Cell { .. } => true,
        };
        if !walked
            || crate::layout::vals(self.pool(), self.env(), ty).map_or(true, |v| v.len() != n)
        {
            return false;
        }
        self.open_parts(ty).is_some_and(|parts| {
            parts.iter().all(|(step, part, at_values)| {
                let sub: Vec<Proj> = at.iter().copied().chain(step.iter().copied()).collect();
                at_values.iter().all(|&k| (k as usize) < n)
                    && self.open_drop_fits(g, &sub, *part, at_values.len())
            })
        })
    }

    /// The parts the open drop descends into, each with the projections
    /// that reach it, its type and its values' indices among `ty`'s: a
    /// struct's or tuple's fields, or each variant's payload fields
    /// (behind `Proj::Variant`). `None` for a type with no parts to open.
    #[allow(clippy::type_complexity)]
    fn open_parts(&self, ty: Ty) -> Option<Vec<(Vec<Proj>, Ty, Vec<u32>)>> {
        let s = crate::layout::strip_newtype(self.pool(), self.env(), ty);
        if let Some(vs) = crate::layout::variants(self.pool(), self.env(), s) {
            let flat = crate::layout::flat(self.pool(), self.env(), s).ok()??;
            let mut out = Vec::new();
            for (v, fields) in (0u32..).zip(vs.iter()) {
                for (k, &f) in (0u32..).zip(fields) {
                    let at = flat.positions.get(v as usize)?.get(k as usize)?.clone();
                    out.push((vec![Proj::Variant(v), Proj::Field(k)], f, at));
                }
            }
            return Some(out);
        }
        let parts = crate::layout::value_parts(self.pool(), self.env(), s);
        let (_, places) = crate::layout::payload_mem(self.pool(), self.env(), &parts).ok()?;
        Some(
            (0u32..)
                .zip(parts.iter().zip(&places))
                .map(|(k, (&part, place))| {
                    let first = u32::try_from(place.first).unwrap_or(u32::MAX);
                    let count = u32::try_from(place.count).unwrap_or(0);
                    (vec![Proj::Field(k)], part, (first..first + count).collect())
                })
                .collect(),
        )
    }

    /// rustc's open drop (elaborate drops) of the value `ls` of type `ty`
    /// at `at`, whose drop flags (`paths`) say which places are still
    /// initialized. The flag at `at`, when there is one, guards it all: a
    /// place moved whole leaves nothing. Under it, with no flag below, the
    /// value releases whole: its shim, its hooks and fields. With flags
    /// below, it releases whole when every one is set, as
    /// `push_hook_guard` guards a hook; else each field drops the same
    /// way, and the hook at `at` does not run. O(the flags and the fields
    /// on their paths).
    pub(super) fn open_drop(
        &mut self,
        ls: &[u32],
        ty: Ty,
        at: &[Proj],
        g: &Guards,
    ) -> StageResult<()> {
        crate::work::moved_paths(1);
        // TIR flags: the flag at `at` guards it all, then the flags below.
        // A cell's slot at `at` is both already (`cell_statement`).
        let mut own = false;
        let guarded = match g {
            Guards::Tir(paths) => {
                if let Some(flag) = paths
                    .iter()
                    .find(|(p, _)| p.as_slice() == at)
                    .map(|&(_, f)| f)
                {
                    self.push_flags(&[flag])?;
                    self.f.op(I::If(wasm_encoder::BlockType::Empty));
                    own = true;
                }
                let below: Vec<u32> = paths
                    .iter()
                    .filter(|(p, _)| p.len() > at.len() && p.starts_with(at))
                    .map(|&(_, f)| f)
                    .collect();
                if below.is_empty() {
                    false
                } else {
                    self.push_flags(&below)?;
                    true
                }
            }
            Guards::Cell { c, cell, whole, .. } => match cell.slot(at) {
                None => false,
                Some(off) => {
                    self.count_flag_read();
                    self.f.get(*c);
                    self.f.op(I::I32Load(mem(HEAP, u64::from(off), 2)));
                    if let Some(w) = whole {
                        self.f.get(*w);
                        self.f.op(I::I32And);
                    }
                    true
                }
            },
        };
        // Below the node: TIR flags guard it when there are any; a cell
        // has its slots, and a node with no slot of its own but slots
        // below (a struct of hooked fields) opens unguarded. A cell's
        // slot with none below is a leaf: clear, the place moved whole.
        let below = match g {
            Guards::Tir(_) => guarded,
            Guards::Cell { cell, .. } => cell
                .paths
                .iter()
                .any(|p| p.len() > at.len() && p.starts_with(at)),
        };
        if guarded {
            self.f.op(I::If(wasm_encoder::BlockType::Empty));
            self.release_node(ls, ty, g)?;
            if below {
                self.f.op(I::Else);
                self.open_below(ls, ty, at, g)?;
            }
            self.f.op(I::End);
        } else if below {
            self.open_below(ls, ty, at, g)?;
        } else {
            self.release_node(ls, ty, g)?;
        }
        if own {
            self.f.op(I::End);
        }
        Ok(())
    }

    /// One count less on the node `ls` of type `ty` whole: in a cell's
    /// glue, a release that runs in place gets the node's address in the
    /// cell; else from its values.
    fn release_node(&mut self, ls: &[u32], ty: Ty, g: &Guards) -> StageResult<()> {
        let base = match g {
            Guards::Cell {
                c, at: Some(at), ..
            } => Some((*c, Rc::clone(at))),
            _ => self.node_base.clone(),
        };
        if let Some((c, at)) = &base
            && let Some(&off) = ls.first().and_then(|l| at.get(l))
        {
            let counted = crate::rt::counted_of(self.input, self.m, ty)?;
            if let crate::rt::Counted::Shims(s) = &counted
                && s.release_at
            {
                let (c, off) = (*c, off - counted.first_offset());
                counted.emit_at(
                    &mut self.f,
                    self.m,
                    &|f| {
                        f.get(c);
                        if off != 0 {
                            f.u32(off);
                            f.op(I::I32Add);
                        }
                    },
                    &|_, _| {},
                    false,
                );
                return Ok(());
            }
        }
        self.release_locals(ls, ty)
    }

    /// The open drop's parts of the value `ls` of type `ty` at `at`: an
    /// enum's variant it holds, one test per variant, or each field.
    fn open_below(&mut self, ls: &[u32], ty: Ty, at: &[Proj], g: &Guards) -> StageResult<()> {
        let s = crate::layout::strip_newtype(self.pool(), self.env(), ty);
        // An enum opens the variant it holds: one test per variant,
        // each opening that variant's payload fields.
        let disc = crate::layout::variants(self.pool(), self.env(), s)
            .and_then(|_| {
                crate::layout::flat(self.pool(), self.env(), s)
                    .ok()
                    .flatten()
            })
            .map(|flat| flat.disc);
        let parts = self.open_parts(ty).unwrap_or_default();
        let mut tested: Option<u32> = None;
        for (step, part, at_values) in parts {
            if let (Some(Proj::Variant(v)), Some(crate::layout::Disc::Cell(d))) =
                (step.first().copied(), disc)
                && tested != Some(v)
            {
                if tested.take().is_some() {
                    self.f.op(I::End);
                }
                let is = d.enc.is(v);
                if matches!(is, crate::layout::Is::Never) {
                    continue;
                }
                self.f.get(ls[d.value as usize]);
                for op in crate::layout::IsOps::ops(is) {
                    self.f.op(op);
                }
                self.f.op(I::If(wasm_encoder::BlockType::Empty));
                tested = Some(v);
            }
            if let Some(Proj::Variant(v)) = step.first().copied()
                && matches!(disc, Some(crate::layout::Disc::Cell(_)))
                && tested != Some(v)
            {
                continue;
            }
            let sub: Vec<Proj> = at.iter().copied().chain(step.iter().copied()).collect();
            let part_ls: Vec<u32> = at_values.iter().map(|&k| ls[k as usize]).collect();
            self.open_drop(&part_ls, part, &sub, g)?;
        }
        if tested.is_some() {
            self.f.op(I::End);
        }
        Ok(())
    }

    /// Pushes the AND of the drop flags `flags`, each a TIR local.
    fn push_flags(&mut self, flags: &[u32]) -> StageResult<()> {
        for (k, &flag) in flags.iter().enumerate() {
            self.count_flag_read();
            let local = self.local(flag)?[0];
            self.f.get(local);
            if k > 0 {
                self.f.op(I::I32And);
            }
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
        if let Some((root, rt)) = root
            && matches!(self.kind(rt), Kind::Value)
            && crate::layout::holds_drop(self.pool(), self.env(), rt)
        {
            let cell = self.cell_slots(root);
            let whole = self.slot_flag(root)?;
            let tir = self.tir_flags();
            let paths = match (&cell, self.tir_flagged(root, rt)) {
                (None, true) => tir.get(&root).map_or(&[][..], Vec::as_slice),
                _ => &[],
            };
            // No flag guards a hook: the value is whole, and its release
            // shim runs its hooks (`hd_layout::shims`). Flags on parts of
            // a struct or tuple: the open drop.
            if cell.is_none() && paths.is_empty() {
                return self.release_locals(ls, ty);
            }
            let guards = match &cell {
                Some((c, cell)) => Guards::Cell {
                    c: *c,
                    cell: Rc::clone(cell),
                    whole,
                    at: None,
                },
                None => Guards::Tir(paths.to_vec()),
            };
            // ruled out: drop flags name field and variant paths only (the
            // checker refuses a move out of an element or through a
            // reference), and `ls` holds the place's values: the open drop
            // takes every flagged place.
            if !self.open_drop_fits(&guards, &p.proj, ty, ls.len()) {
                return Err(hd_base::NotImplemented::bug(
                    hd_base::Stage::Emit,
                    "a drop flag past the fields and variants the open drop opens",
                ));
            }
            return self.open_drop(ls, ty, &p.proj, &guards);
        }
        self.release_locals(ls, ty)
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

    /// The release glue of a heap cell holding a `ty` laid out as `m`.
    /// With drop slots (`cell_layout`), the open drop over them, as a
    /// `Drop` statement on the cell runs it, made once per key as its own
    /// function: its parameter is the payload's address. Else one count
    /// less on the value whole ([`Helper::DropParts`]).
    pub(super) fn cell_glue(&mut self, ty: Ty, m: &Mem) -> StageResult<Option<Sym>> {
        let parts = self.typed_parts(ty)?;
        let cell = self
            .cell_layout(ty)
            .filter(|_| matches!(self.kind(ty), Kind::Value));
        let (Some(cell), [(_, crate::rt::Counted::Shims(shims))]) = (cell, parts.as_slice()) else {
            return Ok((!parts.is_empty()).then(|| {
                Sym::Helper(Helper::DropParts {
                    leaves: m.leaves.clone(),
                    parts,
                })
            }));
        };
        let mut h = hd_base::StableHasher::new("cell-drop");
        h.hash(shims.release);
        h.u32(cell.base);
        for path in &cell.paths {
            h.u32(u32::try_from(path.len()).expect("ruled out: a path is under 2^32 steps"));
            for p in path {
                h.u32(match p {
                    Proj::Field(k) => k.wrapping_mul(2),
                    Proj::Variant(v) => v.wrapping_mul(2) | 1,
                    _ => u32::MAX,
                });
            }
        }
        let sym = Sym::CellDrop(h.finish());
        if !self.m.is_defined(sym.clone()) {
            let glue = self.m.func("rt:cell_drop", vec![Val::I32], vec![]);
            let body = std::mem::replace(&mut self.f, glue);
            let ls = self.f.load_locals(0, &m.leaves);
            let at = ls
                .iter()
                .copied()
                .zip(m.leaves.iter().map(|l| l.offset))
                .collect();
            let guards = Guards::Cell {
                c: 0,
                cell,
                whole: None,
                at: Some(Rc::new(at)),
            };
            let made = if self.open_drop_fits(&guards, &[], ty, ls.len()) {
                self.open_drop(&ls, ty, &[], &guards)
            } else {
                Err(hd_base::NotImplemented::bug(
                    hd_base::Stage::Emit,
                    "a cell's open drop past fields and variants",
                ))
            };
            let glue = std::mem::replace(&mut self.f, body);
            made?;
            self.m.define(sym.clone(), glue);
        }
        Ok(Some(sym))
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
