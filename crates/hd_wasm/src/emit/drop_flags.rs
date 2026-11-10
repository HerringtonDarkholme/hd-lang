//! Initialization state for inline Drop hooks. Zero is a valid payload,
//! so empty owned places need state independent of their value bits.

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

#[derive(Clone, Copy)]
pub(super) enum Flag {
    Local(u32),
    Memory(u32, u32),
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
            PlaceRoot::Local(l) => Some((Root::Local(l.idx()), self.sub(self.b.local_ty[l.idx()]))),
            PlaceRoot::Value(r) => Some((Root::Value(r.0), self.ty_of(r.0))),
            _ => None,
        }
    }

    fn drop_state(&mut self, root: Root, ty: Ty) -> Option<State> {
        if let Some(state) = self.drop_states.get(&root) {
            return Some(state.clone());
        }
        if !matches!(self.kind(ty), Kind::Value)
            || !crate::layout::holds_drop(self.pool(), self.env(), ty)
        {
            return None;
        }
        let ty = match root {
            Root::Local(l) if self.is_cell(l) => self.sub(self.b.local_ty[l]),
            _ => ty,
        };
        let paths = self.drop_node_paths(ty);
        if paths.is_empty() {
            return None;
        }
        let flags = match root {
            Root::Local(l) if self.is_cell(l) => {
                let c = self.locals.get(&l)?[0];
                let base = crate::layout::mem(self.pool(), self.env(), ty)
                    .ok()?
                    .size
                    .next_multiple_of(4);
                paths
                    .iter()
                    .enumerate()
                    .map(|(i, _)| Flag::Memory(c, base + u32::try_from(i).expect("flags") * 4))
                    .collect()
            }
            _ => paths
                .iter()
                .map(|_| Flag::Local(self.f.local(Val::I32)))
                .collect(),
        };
        let state = State { paths, flags };
        self.drop_states.insert(root, state.clone());
        Some(state)
    }

    pub(super) fn mark_drop_root(&mut self, root: Root, ty: Ty, live: bool) {
        if let Some(state) = self.drop_state(root, ty) {
            for flag in state.flags {
                self.set_drop_flag(flag, live);
            }
        }
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
        let state = root.and_then(|(root, ty)| self.drop_state(root, ty));
        let flags: Vec<_> = self
            .drop_paths(ty)
            .iter()
            .map(|(_, child)| {
                let path: Vec<_> = p.proj.iter().chain(child).copied().collect();
                state
                    .as_ref()
                    .and_then(|s| s.paths.iter().position(|p| *p == path).map(|i| s.flags[i]))
            })
            .collect();
        let flags: Vec<_> = flags
            .into_iter()
            .map(|flag| {
                flag.map(|flag| {
                    let local = self.f.local(Val::I32);
                    self.push_drop_flag(flag);
                    self.f.set(local);
                    local
                })
            })
            .collect();
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
                            offset: base + u32::try_from(i).expect("flags") * 4,
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
                Flag::Memory(at, base + u32::try_from(i).expect("flags") * 4),
                live,
            );
        }
    }

    /// The task's destructor is a separate function. Rebind the body's
    /// initialization locals to their saved values before releasing it.
    pub(super) fn reload_drop_flags(&mut self, area: &[(u32, u32, Val)]) {
        let mut roots: Vec<_> = self.drop_states.keys().copied().collect();
        roots.sort_stable_by_key(|root| match root {
            Root::Local(l) => (0, *l as u64),
            Root::Value(i) => (1, u64::from(*i)),
        });
        for root in roots {
            let mut state = self.drop_states[&root].clone();
            for flag in &mut state.flags {
                let old = match *flag {
                    Flag::Local(l) | Flag::Memory(l, _) => l,
                };
                let local = self.f.local(Val::I32);
                if let Some((_, offset, _)) = area.iter().find(|(l, _, _)| *l == old) {
                    self.f.get(super::susp::FRAME);
                    self.f.op(I::I32Load(mem(HEAP, u64::from(*offset), 2)));
                    self.f.set(local);
                }
                *flag = match *flag {
                    Flag::Local(_) => Flag::Local(local),
                    Flag::Memory(_, offset) => Flag::Memory(local, offset),
                };
            }
            self.drop_states.insert(root, state);
        }
    }

    pub(super) fn save_initial_drop_flags(
        &mut self,
        roots: &[usize],
        frame: u32,
        area: &[(u32, u32, Val)],
    ) {
        for &l in roots {
            let Some(state) = self.drop_states.get(&Root::Local(l)) else {
                continue;
            };
            for flag in &state.flags {
                if let Flag::Local(flag) = flag
                    && let Some((_, offset, _)) = area.iter().find(|(local, _, _)| local == flag)
                {
                    self.f.get(frame);
                    self.f.u32(1);
                    self.f.op(I::I32Store(mem(HEAP, u64::from(*offset), 2)));
                }
            }
        }
    }

    fn push_drop_flag(&mut self, flag: Flag) {
        match flag {
            Flag::Local(l) => self.f.get(l),
            Flag::Memory(at, offset) => {
                self.f.get(at);
                self.f.op(I::I32Load(mem(HEAP, u64::from(offset), 2)));
            }
        }
    }

    fn write_drop_flag(&mut self, flag: Flag) {
        match flag {
            Flag::Local(l) => self.f.set(l),
            Flag::Memory(at, offset) => {
                let value = self.f.local(Val::I32);
                self.f.set(value);
                self.f.get(at);
                self.f.get(value);
                self.f.op(I::I32Store(mem(HEAP, u64::from(offset), 2)));
            }
        }
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
