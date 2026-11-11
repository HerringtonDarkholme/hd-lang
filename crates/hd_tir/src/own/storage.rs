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

/// Immutable recursive names are reconstructed function values, including
/// when another closure captures them. Their initialization capture does
/// not imply a shared cell. Build once for an immutable body, then query in
/// constant time. Construction is linear in instructions, referenced capture
/// rows, Ref path words, retained shared rows and locals, with no input-depth
/// recursion or repeated body search.
pub struct RecursiveBindings {
    recursive: Vec<bool>,
    own: Vec<Option<LocalId>>,
    #[cfg(test)]
    work: usize,
}

impl RecursiveBindings {
    #[must_use]
    pub fn new(b: &Body) -> Self {
        let mut addressed = vec![false; b.local_ty.len()];
        #[cfg(test)]
        let mut work = b.local_ty.len();
        for &(_, capture) in &b.inlined_shared {
            addressed[b.cap_local[capture as usize].idx()] = true;
            #[cfg(test)]
            {
                work += 1;
            }
        }
        for i in 0..b.len() {
            #[cfg(test)]
            {
                work += 1;
            }
            if b.tags[i] != Tag::Ref {
                continue;
            }
            let Some(place) = b.decode_place(b.data[i][0]) else {
                continue;
            };
            #[cfg(test)]
            {
                work += place.proj.len();
            }
            if let crate::ir::PlaceRoot::Local(l) = place.root
                && place.proj.iter().all(|s| matches!(s, Proj::Field(_)))
            {
                addressed[l.idx()] = true;
            }
        }
        let mut recursive = vec![false; b.local_ty.len()];
        let mut own = vec![None; b.len()];
        for (ci, slot) in own.iter_mut().enumerate() {
            #[cfg(test)]
            {
                work += 1;
            }
            if b.tags[ci] != Tag::Closure || b.syn[ci] == hd_base::NodeIdx::NONE {
                continue;
            }
            let rec = b.record(b.data[ci][1]);
            let (start, n) = (rec[0] as usize, rec[1] as usize);
            for &l in &b.cap_local[start..start + n] {
                #[cfg(test)]
                {
                    work += 1;
                }
                if b.local_flags[l.idx()] & crate::ir::local_flags::SHORT != 0
                    && b.local_syn[l.idx()] == b.syn[ci]
                    && !addressed[l.idx()]
                {
                    *slot = Some(l);
                    recursive[l.idx()] = true;
                    break;
                }
            }
        }
        Self {
            recursive,
            own,
            #[cfg(test)]
            work,
        }
    }

    #[must_use]
    pub fn self_local(&self, ci: usize) -> Option<LocalId> {
        self.own.get(ci).copied().flatten()
    }

    #[must_use]
    pub fn capture_mode(&self, local: LocalId, mode: CaptureMode) -> CaptureMode {
        if mode.shares() && self.recursive[local.idx()] {
            CaptureMode::Copy
        } else {
            mode
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn work_units(&self) -> usize {
        self.work
    }
}

/// The captures of closure `ci`: each local, and whether it is shared
/// through a cell.
pub fn captures<'a>(
    b: &'a Body,
    ci: usize,
    recursive: &'a RecursiveBindings,
) -> impl Iterator<Item = (usize, bool)> + 'a {
    let rec = b.record(b.data[ci][1]);
    let (start, n) = (rec[0] as usize, rec[1] as usize);
    let own = recursive.self_local(ci).map(LocalId::idx);
    (start..start + n)
        .map(|c| (c, b.cap_local[c].idx()))
        .filter(move |&(_, l)| Some(l) != own)
        .map(|(c, l)| {
            let shares = b.cap_mode.get(c).is_some_and(|&m| {
                recursive
                    .capture_mode(LocalId::from_raw(crate::ir::u32_of(l)), m)
                    .shares()
            });
            (l, shares)
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
    let recursive = RecursiveBindings::new(b);
    let mut cell = vec![false; n];
    for i in 0..b.len() {
        if let Some(l) = root_of(b, cx, i) {
            cell[l] = true;
        }
    }
    let roots = cell.clone();
    for ci in (0..b.len()).filter(|&ci| b.tags[ci] == Tag::Closure) {
        for (l, shared) in captures(b, ci, &recursive) {
            cell[l] |= shared;
        }
    }
    for &(_, capture) in &b.inlined_shared {
        cell[b.cap_local[capture as usize].idx()] = true;
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
                for (l, shared) in captures(b, ci, &recursive) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, local_flags, place};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    fn recursive_body(width: usize) -> Body {
        let mut b = Body::new(DefId::from_raw(1), BodyKind::Fn);
        for l in 0..width {
            let l = crate::ir::u32_of(l);
            let syn = NodeIdx::from_raw(l);
            b.local_ty.push(Ty::VOID);
            b.local_name.push(Symbol::from_raw(0));
            b.local_syn.push(syn);
            b.local_flags.push(local_flags::SHORT);
            b.cap_local.push(LocalId::from_raw(l));
            b.cap_mode.push(CaptureMode::Shared);
            let at = crate::ir::u32_of(b.extra.len());
            b.extra.extend([2, l, 1]);
            b.tags.push(Tag::Closure);
            b.data.push([0, at]);
            b.ty.push(Ty::VOID);
            b.syn.push(syn);
        }
        b
    }

    #[test]
    fn recursive_capture_modes_preserve_addressed_and_mutable_cells() {
        let mut b = recursive_body(4);
        // A mutable binding and an explicitly addressed immutable binding
        // keep the ordinary shared-cell representation.
        b.local_flags[1] = 0;
        b.tags.push(Tag::Ref);
        b.data.push([place::BARE_LOCAL | 2, 0]);
        b.ty.push(Ty::VOID);
        b.syn.push(NodeIdx::NONE);
        let index = RecursiveBindings::new(&b);
        for ci in 0..4 {
            assert_eq!(index.own[ci], b.closure_self(ci));
        }
        for mode in [CaptureMode::Shared, CaptureMode::Frame] {
            assert_eq!(
                index.capture_mode(LocalId::from_raw(0), mode),
                CaptureMode::Copy
            );
            assert_eq!(index.capture_mode(LocalId::from_raw(1), mode), mode);
            assert_eq!(index.capture_mode(LocalId::from_raw(2), mode), mode);
        }
        assert_eq!(
            index.capture_mode(LocalId::from_raw(0), CaptureMode::Move),
            CaptureMode::Move
        );
        assert_eq!(captures(&b, 0, &index).count(), 0);
        assert_eq!(captures(&b, 1, &index).collect::<Vec<_>>(), [(1, true)]);
        assert_eq!(captures(&b, 2, &index).collect::<Vec<_>>(), [(2, true)]);
        // An eliminated closure's retained cell is still addressable even
        // after its executable Ref has vanished.
        b.inlined_shared.push((SubId::from_raw(0), 3));
        let index = RecursiveBindings::new(&b);
        assert_eq!(index.self_local(3), None);
        assert_eq!(b.closure_self(3), None);
        assert_eq!(
            index.capture_mode(LocalId::from_raw(3), CaptureMode::Shared),
            CaptureMode::Shared
        );
    }

    #[test]
    fn recursive_capture_census_counts_linear_work() {
        for width in [100, 1_000] {
            let b = recursive_body(width);
            let index = RecursiveBindings::new(&b);
            assert_eq!(index.work_units(), 4 * width);
            for l in 0..width {
                assert_eq!(
                    index
                        .capture_mode(LocalId::from_raw(crate::ir::u32_of(l)), CaptureMode::Shared),
                    CaptureMode::Copy
                );
            }
        }
    }
}
