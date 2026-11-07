//! Where each local lives (`docs/future-work/OWNERSHIP_ARCHITECTURE.md`,
//! step 17): per sub-body, a frame slot or a cell for a local whose
//! address a thin reference takes or a closure shares, and Wasm locals
//! for every other (no row). Written right after `frame::run`, whose
//! `ref_word::FRAME` marks the slot rule reads.
//!
//! Two inputs arrive after the passes, so the emitter applies them where
//! it reads the rows: a promoted argument (`emit/promote.rs`) reads its
//! local's value, so a local only promoted references reach keeps Wasm
//! locals unless a closure shares it; and a build with no runtime, or a
//! cold constructor, has no slots, so a slot row is a cell there.

use super::OwnCx;
use crate::ir::{Body, CaptureMode, Proj, Root, Storage, Tag, Topology, ref_word};
use hd_base::{LocalId, SubId};

/// Whether `Ref` `i` of `b` is a thin reference to its local's own storage
/// or a field path of it, a root of a cell or a slot.
fn root_of(b: &Body, cx: &dyn OwnCx, i: usize) -> Option<usize> {
    if b.tags[i] != Tag::Ref {
        return None;
    }
    let p = b.view(b.data[i][0]);
    let Root::Local(l) = p.root else {
        return None;
    };
    let fields = p.steps(b).all(|s| matches!(s, Proj::Field(_)));
    let thin = !cx
        .reference_target(b.ty[i])
        .is_some_and(|t| cx.trait_value(t));
    (fields && thin).then_some(l as usize)
}

/// Whether local `l` of `b` is an immutable recursive function's own
/// name, which a nested closure copies rather than shares.
fn recursive_binding(b: &Body, l: usize) -> bool {
    b.local_flags[l] & crate::ir::local_flags::SHORT != 0
        && (0..b.len()).any(|ci| {
            b.tags[ci] == Tag::Closure
                && b.syn[ci] == b.local_syn[l]
                && b.closure_self(ci).map(LocalId::idx) == Some(l)
        })
}

/// The captures of closure `ci`: each local, and whether it is shared
/// through a cell.
pub fn captures(b: &Body, ci: usize) -> impl Iterator<Item = (usize, bool)> + '_ {
    let rec = b.record(b.data[ci][1]);
    let (start, n) = (rec[0] as usize, rec[1] as usize);
    let own = b.closure_self(ci).map(LocalId::idx);
    (start..start + n)
        .map(|c| (c, b.cap_local[c].idx()))
        .filter(move |&(_, l)| Some(l) != own)
        .map(|(c, l)| {
            let shares = b.cap_mode.get(c).is_some_and(|m| m.shares());
            (l, shares && !recursive_binding(b, l))
        })
}

/// The locals of `roots` that live in a slot of the root's frame: every
/// `Ref` of their storage is a local reference of a frame local in the
/// root, and no closure captures them; or every capture of them is a
/// frame cell's (`CaptureMode::Frame`) and every `Ref` of them is local
/// and in the root.
fn slots(b: &Body, topo: &Topology, roots: &[bool]) -> Vec<bool> {
    let n = b.local_ty.len();
    let (mut out, mut frame_cell) = (roots.to_vec(), vec![None::<bool>; n]);
    for (c, m) in b.cap_local.iter().zip(&b.cap_mode) {
        out[c.idx()] = false;
        let all = frame_cell[c.idx()].unwrap_or(true);
        frame_cell[c.idx()] = Some(all && *m == CaptureMode::Frame);
    }
    let mut cell: Vec<bool> = frame_cell.iter().map(|f| *f == Some(true)).collect();
    for i in 0..b.len() {
        if b.tags[i] != Tag::Ref {
            continue;
        }
        let Some(l) = b.local_in_place(b.data[i][0]).map(LocalId::idx) else {
            continue;
        };
        let w = b.data[i][1];
        let frame = ref_word::LOCAL | ref_word::FRAME;
        if w & frame != frame || topo.sub[i] != 0 {
            out[l] = false;
        }
        if w & ref_word::LOCAL == 0 || topo.sub[i] != 0 {
            cell[l] = false;
        }
    }
    (0..n).map(|l| out[l] || cell[l]).collect()
}

/// Writes `b.storage`.
pub(crate) fn run(b: &mut Body, topo: &Topology, cx: &dyn OwnCx, framed: bool) {
    let n = b.local_ty.len();
    let mut cell = vec![false; n];
    for i in 0..b.len() {
        if let Some(l) = root_of(b, cx, i) {
            cell[l] = true;
        }
    }
    let roots = cell.clone();
    for ci in (0..b.len()).filter(|&ci| b.tags[ci] == Tag::Closure) {
        for (l, shared) in captures(b, ci) {
            cell[l] |= shared;
        }
    }
    let mut rows = Vec::new();
    if cell.iter().any(|&c| c) {
        let slot = if framed {
            vec![false; n]
        } else {
            slots(b, topo, &roots)
        };
        for s in 0..b.sub_root.len() {
            // A closure's copy of a capture is a value of its own.
            let mut copy = vec![false; n];
            if s != 0
                && let Some(ci) = (0..b.len()).find(|&i| {
                    b.tags[i] == Tag::Closure
                        && b.data[i][0] as usize == s
                        && !b.is_instantiation(i)
                })
            {
                for (l, shared) in captures(b, ci) {
                    copy[l] |= !shared;
                }
            }
            for l in (0..n).filter(|&l| cell[l] && !copy[l]) {
                let storage = if s == 0 && slot[l] {
                    Storage::Slot
                } else {
                    Storage::Cell
                };
                let (s, l) = (crate::ir::u32_of(s), crate::ir::u32_of(l));
                rows.push((SubId::from_raw(s), LocalId::from_raw(l), storage));
            }
        }
    }
    b.storage = rows;
}
