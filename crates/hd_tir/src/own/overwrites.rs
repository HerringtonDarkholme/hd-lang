//! The old value a store replaces (`docs/future-work/OWNERSHIP_ARCHITECTURE.md`,
//! `Release p`, reason overwritten): a `Store` into a place that may hold a
//! value is followed by `Release %store`, which the backend lowers inside the
//! store's one walk of the place. A `Store` with none only writes.
//!
//! No statement follows a store of nothing that counts or a store known to
//! find its place empty ([`Stores::finds_empty`]).

use super::{OwnCx, splice};
use crate::ir::count::{At, Info, Part};
use crate::ir::{Body, NONE, Ref, Tag, Topology, local_flags, u32_of};
use hd_types::Ty;

/// Where a body's stores stand, per local.
struct Stores {
    /// The first instruction that stores into the local, whole or in part.
    first: Vec<u32>,
    /// Whether a `Ref` or a capture reaches the local: it lives in a slot or
    /// a cell, which has a count or a value to end whatever its type.
    placed: Vec<bool>,
}

impl Stores {
    fn of(b: &Body) -> Self {
        let locals = b.local_ty.len();
        let mut s = Stores {
            first: vec![NONE; locals],
            placed: vec![false; locals],
        };
        for i in 0..b.len() {
            match b.tags[i] {
                Tag::Store => {
                    if let Some(l) = b.view(b.data[i][0]).local() {
                        let first = &mut s.first[l as usize];
                        *first = (*first).min(u32_of(i));
                    }
                }
                Tag::Ref => {
                    if let Some(l) = b.local_in_place(b.data[i][0]) {
                        s.placed[l.idx()] = true;
                    }
                }
                _ => {}
            }
        }
        for l in &b.cap_local {
            s.placed[l.idx()] = true;
        }
        s
    }

    /// Whether `Store` `i` releases what its place holds: its place's type
    /// has something to end, and the store is not known to find it empty.
    fn releases(&self, b: &Body, cx: &dyn OwnCx, topo: &Topology, i: usize) -> bool {
        let [word, value] = b.data[i];
        let p = b.view(word);
        let whole = p.whole().then(|| p.local()).flatten();
        let counts = match whole {
            Some(l) => self.placed[l as usize] || cx.counted(b.local_ty[l as usize]),
            None => match Ref(value).as_inst() {
                Some(v) => value != NONE && cx.counted(b.ty[v.0 as usize]),
                None => b
                    .consts
                    .get((value & !Ref::CONST_BIT) as usize)
                    .is_some_and(|c| cx.counted(c.0)),
            },
        };
        counts && !whole.is_some_and(|l| self.finds_empty(b, topo, i, l))
    }

    /// Whether `Store` `i`, of all of local `l`, finds it holding nothing:
    /// the local is no parameter, no store comes before this one (a local is
    /// always initialized where it is declared, so a closure's store comes
    /// after), and no loop of the sub-body runs it again with a value left in
    /// it ([`super::frame::store_finds_empty`]).
    fn finds_empty(&self, b: &Body, topo: &Topology, i: usize, l: u32) -> bool {
        let l = l as usize;
        if b.local_flags[l] & local_flags::PARAM != 0 || self.first[l] != u32_of(i) {
            return false;
        }
        let lp = topo.loop_of[i];
        lp == NONE
            || topo.sub[lp as usize] != topo.sub[i]
            || super::frame::store_finds_empty(b, topo, i)
    }
}

/// Plans a `Release` right after each store that releases what its place
/// holds. The caller applies the edits.
pub(super) fn plan(b: &mut Body, cx: &dyn OwnCx, topo: &Topology, edits: &mut splice::Tables) {
    if !b.tags.contains(&Tag::Store) {
        return;
    }
    let stores = Stores::of(b);
    for i in 0..b.len() {
        if b.tags[i] == Tag::Store && stores.releases(b, cx, topo, i) {
            let record = Info::new(At::Operation, Part::Result, 3).write(b);
            edits.insert(
                topo.blk[i],
                topo.pos[i] + 1,
                Tag::Release,
                [u32_of(i), record],
                Ty::VOID,
                b.syn[i],
            );
        }
    }
}

/// A `Release` emits at its store: it takes the store's anchor.
pub(super) fn anchor(b: &mut Body) {
    if b.anchors.is_empty() {
        return;
    }
    for i in 0..b.len() {
        if b.tags[i] == Tag::Release {
            b.anchors[i] = b.anchors[b.data[i][0] as usize];
        }
    }
}

/// Invariant 23's part: each `Release` follows the `Store` it names, and each
/// store that releases what its place holds has one.
pub(crate) fn verify(b: &Body, cx: &dyn OwnCx, topo: &Topology, mut err: impl FnMut(u32, String)) {
    let stores = Stores::of(b);
    for i in 0..b.len() {
        let next = || {
            b.record(b.data[topo.blk[i] as usize][0])
                .get(topo.pos[i] as usize + 1)
                .copied()
        };
        match b.tags[i] {
            Tag::Store if topo.blk[i] != NONE && stores.releases(b, cx, topo, i) => {
                if !next().is_some_and(|x| {
                    b.tags[x as usize] == Tag::Release && b.data[x as usize][0] == u32_of(i)
                }) {
                    err(u32_of(i), "a store that overwrites has no Release".into());
                }
            }
            Tag::Release if Info::read(b, i).is_none_or(|c| c.at == At::Operation) => {
                let store = b.data[i][0] as usize;
                if store >= i
                    || b.tags[store] != Tag::Store
                    || topo.blk[store] != topo.blk[i]
                    || topo.pos[store] + 1 != topo.pos[i]
                    || b.ty[i] != Ty::VOID
                {
                    err(u32_of(i), "a Release away from its store".into());
                }
            }
            _ => {}
        }
    }
}
