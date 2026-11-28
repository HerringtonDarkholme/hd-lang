//! The old value a store replaces (`docs/compiler/ownership.md#ownership-per-instance`,
//! `Release p`, reason overwritten): a `Store` into a place that may hold a
//! value is followed by `Release %store`, which the backend lowers inside the
//! store's one walk of the place. A `Store` with none only writes.
//!
//! No statement follows a store of nothing that counts or a store known to
//! find its place empty ([`Stores::finds_empty`]): a local's first store,
//! and a module init's first store of a top-level binding, which declares
//! it (#2328).

use super::{OwnCx, splice};
use crate::ir::count::{At, Info, Part};

use crate::ir::{Body, BodyKind, NONE, Ref, Root, Tag, Topology, local_flags, u32_of};
use hd_base::{HashMap, HashSet};
use hd_types::Ty;

/// Where a body's stores stand, per local.
struct Stores {
    /// The first instruction that stores into the local, whole or in part.
    first: Vec<u32>,
    /// In a module init, the first instruction that stores into each
    /// top-level binding, whole or in part.
    first_global: HashMap<u32, u32>,
    /// Empty-place proofs from the existing known-null walk, after splicing.
    empty: HashSet<u32>,
    /// Whether a `Ref` or a capture reaches the local: it lives in a slot or
    /// a cell, which has a count or a value to end whatever its type.
    placed: Vec<bool>,
}

impl Stores {
    fn of(b: &Body, empty: &[u32]) -> Self {
        let locals = b.local_ty.len();
        let mut s = Stores {
            first: vec![NONE; locals],
            first_global: HashMap::default(),
            empty: empty.iter().copied().collect(),
            placed: vec![false; locals],
        };
        for i in 0..b.len() {
            match b.tags[i] {
                Tag::Store => {
                    let p = b.view(b.data[i][0]);
                    if let Some(l) = p.local() {
                        let first = &mut s.first[l as usize];
                        *first = (*first).min(u32_of(i));
                    } else if let (BodyKind::Init, Root::Global(d, _)) = (b.kind, p.root) {
                        s.first_global.entry(d.raw()).or_insert(u32_of(i));
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
        // Every store of a binding holds a borrow (invariant 23), so a
        // repeated frame-slot initialization owns no previous descriptor.
        if whole.is_some() && b.loan_value(value).is_some() {
            return false;
        }
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
        counts
            && !whole.is_some_and(|l| self.finds_empty(b, topo, i, l))
            && !self.declares(b, cx, topo, i)
    }

    /// Whether Store `i` is the declaring module init's first, whole
    /// store of a top-level binding, outside any loop: the binding's
    /// initialization, which finds its slot empty, since nothing reads or
    /// writes a binding before it is initialized (`module.init.definite`).
    /// Releasing the empty slot would run the glue or `Drop` hook of a
    /// value never made. A later REPL input that assigns an earlier one's
    /// binding belongs to another module, so its store still releases.
    fn declares(&self, b: &Body, cx: &dyn OwnCx, topo: &Topology, i: usize) -> bool {
        let p = b.view(b.data[i][0]);
        let Root::Global(d, _) = p.root else {
            return false;
        };
        p.whole()
            && self.first_global.get(&d.raw()) == Some(&u32_of(i))
            && topo.loop_of[i] == NONE
            && cx.declares_binding(b.item, d)
    }

    /// Whether Store i finds its whole local empty: the existing known-null
    /// walk proves it on every incoming path, or the first-store fallback
    /// proves it for storage the walk does not track. For the fallback,
    /// the local is no parameter, no store comes before this one (a local is
    /// always initialized where it is declared, so a closure's store comes
    /// after), and no loop of the sub-body runs it again with a value left in
    /// it ([`super::frame::store_finds_empty`]).
    fn finds_empty(&self, b: &Body, topo: &Topology, i: usize, l: u32) -> bool {
        if self.empty.contains(&u32_of(i)) {
            return true;
        }
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
pub(super) fn plan(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    empty: &[u32],
    edits: &mut splice::Tables,
) {
    if !b.tags.contains(&Tag::Store) {
        return;
    }
    let stores = Stores::of(b, empty);
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
