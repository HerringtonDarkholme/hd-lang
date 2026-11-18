//! Initialization state for inline Drop hooks. Zero is a valid payload,
//! so empty owned places need state independent of their value bits.
//!
//! A value root's flags are TIR locals (`hd_tir::own::drop_flags`): this
//! file lowers their `FlagSet` and `FlagClear` statements and guards each
//! hook by them. A cell keeps one flag per hooked node in its layout.

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

/// A cell's flag: the local of its address, and the flag's offset.
#[derive(Clone, Copy)]
pub(super) struct Flag(u32, u32);

/// Per value root, each flagged move path with its TIR flag local, read
/// from one sub-body's flag statements and sorted by [`path_order`], so the
/// paths under one path are one range.
pub(super) type TirFlags = hd_base::HashMap<Root, Vec<(Vec<Proj>, u32)>>;

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

#[derive(Clone)]
pub(super) struct State {
    paths: Vec<Vec<Proj>>,
    flags: Vec<Flag>,
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

    /// A cell's flags, one per hooked node of its type.
    fn drop_state(&mut self, root: Root, ty: Ty) -> Option<State> {
        let Root::Local(l) = root else {
            return None;
        };
        if !self.is_cell(l) {
            return None;
        }
        if let Some(state) = self.drop_states.get(&root) {
            return Some(state.clone());
        }
        if !matches!(self.kind(ty), Kind::Value)
            || !crate::layout::holds_drop(self.pool(), self.env(), ty)
        {
            return None;
        }
        let ty = self.b.local_ty[l];
        let paths = self.drop_node_paths(ty);
        if paths.is_empty() {
            return None;
        }
        let c = self.locals.get(&l)?[0];
        let base = crate::layout::mem(self.pool(), self.env(), ty)
            .ok()?
            .size
            .next_multiple_of(4);
        let flags = (0..paths.len())
            .map(|i| {
                Flag(
                    c,
                    base + u32::try_from(i).expect("ruled out: a type has under 2^32 nodes") * 4,
                )
            })
            .collect();
        let state = State { paths, flags };
        self.drop_states.insert(root, state.clone());
        Some(state)
    }

    /// This sub-body's TIR flags, read on first use: one walk of its rows,
    /// then one sort per root, O(S log S) for S flag statements.
    fn tir_flags(&mut self) -> std::rc::Rc<TirFlags> {
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
        // The verifier gives each flag local one path: equal paths are one
        // flag's statements.
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

    /// `FlagSet` or `FlagClear` `i`: writes its flag local, when this
    /// instance reads it.
    pub(super) fn flag_statement(&mut self, i: u32) -> StageResult<()> {
        let [word, flag] = self.b.data[i as usize];
        let Some((root, ty)) = self.b.decode_place(word).and_then(|p| self.drop_root(&p)) else {
            return Ok(());
        };
        if self.tir_flagged(root, ty) {
            let local = self.local(flag)?[0];
            self.f
                .u32(u32::from(self.b.tags[i as usize] == Tag::FlagSet));
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

    pub(super) fn mark_drop_place(&mut self, p: &Place, live: bool) {
        let Some((root, ty)) = self.drop_root(p) else {
            return;
        };
        let Some(state) = self.drop_state(root, ty) else {
            return;
        };
        for (path, &flag) in state.paths.iter().zip(&state.flags) {
            if path.starts_with(&p.proj) || (!live && p.proj.starts_with(path)) {
                self.set_drop_flag(flag, live);
            }
        }
        // A parent becomes whole again after every immediate child was
        // restored. Keep nodes without hooks too: moving such a field
        // still makes the parent's own Drop hook ineligible.
        if live {
            for index in (0..state.paths.len()).rev() {
                let path = &state.paths[index];
                if p.proj.starts_with(path) && path.len() < p.proj.len() {
                    self.f.u32(1);
                    for (child, &flag) in state.paths.iter().zip(&state.flags) {
                        if child.len() == path.len() + 1 && child.starts_with(path) {
                            self.push_drop_flag(flag);
                            self.f.op(I::I32And);
                        }
                    }
                    self.write_drop_flag(state.flags[index]);
                }
            }
        }
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
            let cell = self.drop_state(root, rt);
            let tir = self.tir_flags();
            let paths = match (&cell, self.tir_flagged(root, rt)) {
                (None, true) => tir.get(&root).map_or(&[][..], Vec::as_slice),
                _ => &[],
            };
            for (_, child) in self.drop_paths(ty) {
                let full: Vec<_> = p.proj.iter().chain(&child).copied().collect();
                match &cell {
                    Some(state) => {
                        let i = state.paths.iter().position(|p| *p == full).ok_or_else(|| {
                            hd_base::NotImplemented::bug(
                                hd_base::Stage::Emit,
                                "a cell's hook at a node its flags lack",
                            )
                        })?;
                        self.push_drop_flag(state.flags[i]);
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
        self.mark_drop_place(p, false);
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

    pub(super) fn stored_drop(&mut self) {
        if let Some(p) = self.store_drop_place() {
            self.mark_drop_place(&p, true);
        }
    }

    pub(super) fn value_place(value: u32) -> Place {
        Place {
            root: PlaceRoot::Value(Ref(value)),
            proj: Vec::new(),
        }
    }

    pub(super) fn drop_node_paths(&self, ty: Ty) -> Vec<Vec<Proj>> {
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
                    crate::layout::strip(em.pool(), em.env(), ty),
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

    pub(super) fn cell_drop_calls(&self, ty: Ty) -> Vec<DropCall> {
        let paths = self.drop_node_paths(ty);
        let base = crate::layout::mem(self.pool(), self.env(), ty)
            .expect("cell layout")
            .size
            .next_multiple_of(4);
        self.drop_paths(ty)
            .into_iter()
            .map(|(mut call, path)| {
                if let Some(i) = paths.iter().position(|p| *p == path) {
                    call.guards.push((
                        Leaf {
                            offset: base
                                + u32::try_from(i).expect("ruled out: a type has under 2^32 nodes")
                                    * 4,
                            val: Val::I32,
                            bytes: 4,
                            signed: false,
                        },
                        crate::layout::Is::Eq(1),
                    ));
                }
                call
            })
            .collect()
    }

    pub(super) fn init_cell_drop(&mut self, at: u32, ty: Ty, live: bool) {
        let paths = self.drop_node_paths(ty);
        if paths.is_empty() {
            return;
        }
        let base = crate::layout::mem(self.pool(), self.env(), ty)
            .expect("cell layout")
            .size
            .next_multiple_of(4);
        for i in 0..paths.len() {
            self.set_drop_flag(
                Flag(
                    at,
                    base + u32::try_from(i).expect("ruled out: a type has under 2^32 nodes") * 4,
                ),
                live,
            );
        }
    }

    /// Scalar storage the separate frame destructor reads: each cell's
    /// address and each TIR flag local. Select these before assigning
    /// compact offsets.
    pub(super) fn select_frame_drop_flags(&mut self, selected: &mut [bool]) {
        for state in self.drop_states.values() {
            for flag in &state.flags {
                selected[flag.0 as usize] = true;
            }
        }
        for paths in self.tir_flags().values() {
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
        let mut roots: Vec<_> = self.drop_states.keys().copied().collect();
        roots.sort_stable_by_key(|root| match root {
            Root::Local(l) => (0, *l as u64),
            Root::Value(i) => (1, u64::from(*i)),
        });
        for root in roots {
            let mut state = self.drop_states[&root].clone();
            for flag in &mut state.flags {
                flag.0 = load(self, flag.0);
            }
            self.drop_states.insert(root, state);
        }
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

    fn push_drop_flag(&mut self, Flag(at, offset): Flag) {
        self.f.get(at);
        self.f.op(I::I32Load(mem(HEAP, u64::from(offset), 2)));
    }

    fn write_drop_flag(&mut self, Flag(at, offset): Flag) {
        let value = self.f.local(Val::I32);
        self.f.set(value);
        self.f.get(at);
        self.f.get(value);
        self.f.op(I::I32Store(mem(HEAP, u64::from(offset), 2)));
    }

    fn set_drop_flag(&mut self, flag: Flag, live: bool) {
        self.f.u32(u32::from(live));
        self.write_drop_flag(flag);
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
