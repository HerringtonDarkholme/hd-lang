//! Where each local lives (`docs/compiler/ownership.md#ownership-per-instance`,
//! "Ownership Per Instance"): per sub-body, a frame slot or a cell for a local whose
//! address a thin reference takes or a closure shares, and Wasm locals
//! for every other (no row). Migration step 17 is in
//! git history at 7ccbb4ea8, `docs/future-work/OWNERSHIP_ARCHITECTURE.md`.
//! Written right after `frame::run`, whose
//! `ref_word::FRAME` marks the slot rule reads. In a root that runs over
//! a suspension frame, a slot is a slot of that frame (#1707,
//! [`awaited_slots`]).
//!
//! Collection's choices then reach each instance body's rows before
//! emission ([`settle`], `docs/future-work/INSTANCE_TIR.md` step 7a): a
//! promoted argument reads its local's value, and a frame slot whose
//! `Await` calls no known body is a cell, as that await counts its
//! arguments. A build with no runtime has no slots; the emitter reads it.

use super::OwnCx;
use crate::ir::{Body, CaptureMode, Proj, Root, Storage, Tag, Topology, ref_word};
use hd_base::{LocalId, SubId};

/// The local `Ref` `i` of `b` takes a reference to the storage of, or to
/// a field path of.
fn field_ref(b: &Body, i: usize) -> Option<usize> {
    let p = (b.tags[i] == Tag::Ref).then(|| b.view(b.data[i][0]))?;
    let Root::Local(l) = p.root else {
        return None;
    };
    p.steps(b)
        .all(|s| matches!(s, Proj::Field(_)))
        .then_some(l as usize)
}

/// [`field_ref`] of a thin reference, a root of a cell or a slot.
fn root_of(b: &Body, cx: &dyn OwnCx, i: usize) -> Option<usize> {
    let thin = |_: &usize| {
        !cx.reference_target(b.ty[i])
            .is_some_and(|t| cx.trait_value(t))
    };
    field_ref(b, i).filter(thin)
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

/// The locals of `roots` that live in a slot of a suspending root's
/// frame object (#1707): locals of the root, not parameters, that no
/// closure captures, and whose every `Ref` of their storage is a local
/// reference in the root that one `Await` takes as an argument and that
/// is otherwise only dropped. The await calls its callee's body directly,
/// which keeps the reference uncounted in its own frame, and the caller's
/// frame, which holds the slot, outlives that frame: the caller tears the
/// child down before its own locals' ends, on cancellation too. One scan
/// of the body after a scan of its locals, and none when no local is a
/// candidate.
fn awaited_slots(b: &Body, topo: &Topology, roots: &[bool]) -> Vec<bool> {
    use crate::ir::local_flags;
    // Each `Ref` of a candidate's storage: unseen, then awaited once, or
    // used otherwise (any other use or a second one).
    const UNTRACKED: u8 = 0;
    const UNSEEN: u8 = 1;
    const AWAITED: u8 = 2;
    const OTHER: u8 = 3;
    let mut out: Vec<bool> = (0..b.local_ty.len())
        .map(|l| roots[l] && b.local_flags[l] & local_flags::PARAM == 0)
        .collect();
    for c in &b.cap_local {
        out[c.idx()] = false;
    }
    if !out.iter().any(|&o| o) {
        return out;
    }
    let mut state = vec![UNTRACKED; b.len()];
    for (i, st) in state.iter_mut().enumerate() {
        if b.tags[i] != Tag::Ref {
            continue;
        }
        let Some(l) = b.local_in_place(b.data[i][0]).map(LocalId::idx) else {
            continue;
        };
        if b.data[i][1] & ref_word::LOCAL == 0 || topo.sub[i] != 0 {
            out[l] = false;
        } else if out[l] {
            *st = UNSEEN;
        }
    }
    // Insertion calls this before emitting ownership statements (invariant 22).
    let mut ops = Vec::new();
    for j in 0..b.len() {
        super::operands(b, j, &mut ops);
        for op in &ops {
            let st = &mut state[op.value as usize];
            if *st == UNTRACKED {
                continue;
            }
            let awaits = b.tags[j] == Tag::Await
                && topo.sub[j] == 0
                && matches!(op.role, crate::ir::Role::Arg(_));
            *st = if awaits && *st == UNSEEN {
                AWAITED
            } else {
                OTHER
            };
        }
    }
    for (i, &st) in state.iter().enumerate() {
        if matches!(st, UNSEEN | OTHER)
            && let Some(l) = b.local_in_place(b.data[i][0]).map(LocalId::idx)
        {
            out[l] = false;
        }
    }
    out
}

/// How many roots of `b` take each local, and whether a closure shares
/// it. One walk of the body and of its captures.
fn census(
    b: &Body,
    cx: &dyn OwnCx,
    recursive: &RecursiveBindings,
    mut instruction: impl FnMut(usize),
) -> (Vec<u32>, Vec<bool>) {
    let (mut refs, mut shared) = (vec![0; b.local_ty.len()], vec![false; b.local_ty.len()]);
    for i in 0..b.len() {
        instruction(i);
        if let Some(l) = root_of(b, cx, i) {
            refs[l] += 1;
        }
        if b.tags[i] == Tag::Closure {
            for (l, s) in captures(b, i, recursive) {
                shared[l] |= s;
            }
        }
    }
    for &(_, capture) in &b.inlined_shared {
        shared[b.cap_local[capture as usize].idx()] = true;
    }
    (refs, shared)
}

/// Writes `b.storage` in O(instructions + locals + captures + storage rows),
/// plus the existing reference-path walks.
/// Copy marks reset only touched entries, not the whole local universe.
pub(crate) fn run(b: &mut Body, topo: &Topology, cx: &dyn OwnCx, framed: bool) {
    run_with(b, topo, cx, framed, |_| {});
}

fn run_with(
    b: &mut Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    framed: bool,
    mut work: impl FnMut(usize),
) {
    let n = b.local_ty.len();
    let recursive = RecursiveBindings::new(b);
    let mut closures = vec![None; b.sub_root.len()];
    work(n + closures.len());
    let (refs, shared) = census(b, cx, &recursive, |i| {
        work(1);
        if b.tags[i] == Tag::Closure {
            closures[b.data[i][0] as usize].get_or_insert(i);
        }
    });
    let roots: Vec<bool> = refs.iter().map(|&r| r > 0).collect();
    let cells: Vec<usize> = (0..n).filter(|&l| roots[l] || shared[l]).collect();
    let mut rows = Vec::new();
    if !cells.is_empty() {
        let slot = if framed {
            awaited_slots(b, topo, &roots)
        } else {
            slots(b, topo, &roots)
        };
        let mut copy = vec![false; n];
        let mut touched = Vec::new();
        for (s, ci) in closures.into_iter().enumerate() {
            work(1);
            // A closure's copy of a capture is a value of its own.
            if s != 0
                && let Some(ci) = ci
            {
                for (l, shared) in captures(b, ci, &recursive) {
                    work(1);
                    if !shared && !copy[l] {
                        copy[l] = true;
                        touched.push(l);
                    }
                }
            }
            for &l in &cells {
                work(1);
                if copy[l] {
                    continue;
                }
                let storage = if s == 0 && slot[l] {
                    Storage::Slot
                } else {
                    Storage::Cell
                };
                let (s, l) = (crate::ir::u32_of(s), crate::ir::u32_of(l));
                rows.push((SubId::from_raw(s), LocalId::from_raw(l), storage));
            }
            for l in touched.drain(..) {
                work(1);
                copy[l] = false;
            }
        }
    }
    b.storage = rows;
}

/// `b`'s final rows (`docs/future-work/INSTANCE_TIR.md` step 7a), from
/// collection's choices for the instances that emit its sub-bodies:
/// `args`, each sub-body's `Ref`s passed by value (§23.40), and `counted`,
/// the root's locals an await of no known body takes. A local that only
/// promoted references of a sub-body reach, and that no closure shares,
/// keeps Wasm locals there; a counted local's slot is a cell. The body
/// with those rows and its promoted `Ref`s marked, or `None` when nothing
/// changes. One walk of the body when an argument is promoted, as rustc's
/// `non_ssa_locals` picks each local's backing before codegen.
pub fn settle(b: &Body, args: &[(u16, u32)], counted: &[bool], cx: &dyn OwnCx) -> Option<Body> {
    let mut rows = b.storage.clone();
    for r in rows
        .iter_mut()
        .filter(|r| r.0.idx() == 0 && counted.get(r.1.idx()) == Some(&true))
    {
        r.2 = Storage::Cell;
    }
    if args.is_empty() && rows == b.storage {
        return None;
    }
    let (refs, shared) = census(b, cx, &RecursiveBindings::new(b), |_| {});
    let mut promoted = hd_base::HashMap::<(usize, usize), u32>::default();
    for &(s, i) in args {
        if let Some(l) = root_of(b, cx, i as usize) {
            *promoted.entry((usize::from(s), l)).or_default() += 1;
        }
    }
    rows.retain(|&(s, l, _)| {
        let p = promoted.get(&(s.idx(), l.idx())).copied().unwrap_or(0);
        p == 0 || shared[l.idx()] || refs[l.idx()] > p
    });
    let mut out = b.clone();
    for &(_, i) in args {
        out.data[i as usize][1] |= ref_word::PROMOTED;
    }
    out.storage = rows;
    Some(out)
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

    /// Counts its reference-type queries: one per unpromoted `Ref`.
    struct Counting(std::cell::Cell<usize>);
    impl OwnCx for Counting {
        fn temporary_owner_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn reference_target(&self, _: Ty) -> Option<Ty> {
            self.0.set(self.0.get() + 1);
            None
        }
        fn counted(&self, _: Ty) -> bool {
            false
        }
        fn pass(&self, _: &crate::ir::Callee, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Lent
        }
        fn may_write(&self, _: &crate::ir::Callee) -> bool {
            false
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> crate::own::Pass {
            crate::own::Pass::Owned
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    }

    /// `n` cell locals, each referenced once by a call's argument.
    fn referenced_calls(n: u32) -> Body {
        use crate::ir::{Callee, Place, Providers, TirBuilder, TirSink};
        let (mut tb, none) = (
            TirBuilder::new(DefId::from_raw(1), BodyKind::Fn),
            NodeIdx::NONE,
        );
        let blk = tb.open_block();
        let callee = Callee::Item {
            def: DefId::from_raw(2),
            targs: hd_types::TyList::EMPTY,
        };
        for _ in 0..n {
            let l = tb.local(Ty::I32, Symbol::from_raw(0), local_flags::PARAM, none);
            let p = tb.place(&Place::local(l));
            let r = tb.emit(Tag::Ref, p, ref_word::LOCAL, Ty::VOID, none);
            tb.call(&callee, &[r], Providers::None, Ty::VOID, none);
        }
        let root = tb.close_block(blk, None, Ty::VOID, none);
        let mut b = tb.finish(root, &[]).expect("a verified body");
        b.storage = (0..n)
            .map(|l| (SubId::from_raw(0), LocalId::from_raw(l), Storage::Cell))
            .collect();
        b
    }

    #[test]
    fn settle_drops_promoted_rows_in_linear_work() {
        for n in [100, 1_000] {
            let b = referenced_calls(n);
            let cx = Counting(std::cell::Cell::new(0));
            // Every other call passes its argument by value.
            let args: Vec<(u16, u32)> = (0..b.len())
                .filter(|&i| b.tags[i] == Tag::Call)
                .map(|i| b.call_args(i)[0])
                .filter(|&r| b.data[r as usize][0] & 1 == 0)
                .map(|r| (0, r))
                .collect();
            let out = settle(&b, &args, &[], &cx).expect("a changed body");
            let kept: Vec<u32> = out.storage.iter().map(|r| r.1.raw()).collect();
            assert_eq!(kept, (0..n).filter(|l| l & 1 == 1).collect::<Vec<_>>());
            let marked = (0..b.len())
                .filter(|&i| b.tags[i] == Tag::Ref && out.data[i][1] & ref_word::PROMOTED != 0);
            assert_eq!(marked.count(), n as usize / 2);
            // One type query per `Ref` in the census, one per argument.
            assert_eq!(cx.0.get(), n as usize * 3 / 2);
            assert!(settle(&b, &[], &[], &cx).is_none());
        }
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

    #[test]
    fn storage_indexes_closures_and_resets_only_copied_captures() {
        use crate::ir::{TirBuilder, TirSink};
        let mut costs = Vec::new();
        for width in [100, 1_000] {
            let mut tb = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let none = NodeIdx::NONE;
            let outer = tb.open_block();
            let locals: Vec<_> = (0..width + 2)
                .map(|_| tb.local(Ty::I32, Symbol::NONE, local_flags::PARAM, none))
                .collect();
            let mut modes = Vec::new();
            for i in 0..width {
                let sub = tb.open_sub(&[]);
                let inner = tb.open_block();
                let root = tb.close_block(inner, None, Ty::VOID, none);
                tb.capture(sub, locals[0]);
                tb.capture(sub, locals[1]);
                tb.close_sub(sub, root, Ty(43), none);
                modes.push(if i % 2 == 0 {
                    CaptureMode::Shared
                } else {
                    CaptureMode::Copy
                });
                modes.push(CaptureMode::Shared);
            }
            let root = tb.close_block(outer, None, Ty::VOID, none);
            let mut b = tb.finish(root, &modes).unwrap();
            let topo = Topology::new(&b);
            let cx = Counting(std::cell::Cell::new(0));
            let expected: Vec<_> = (0..=width)
                .flat_map(|s| {
                    [0, 1].into_iter().filter_map(move |l| {
                        (s == 0 || s % 2 == 1 || l == 1).then_some((
                            SubId::from_raw(crate::ir::u32_of(s)),
                            LocalId::from_raw(l),
                            Storage::Cell,
                        ))
                    })
                })
                .collect();
            for framed in [false, true] {
                let mut work = 0;
                run_with(&mut b, &topo, &cx, framed, |n| work += n);
                assert_eq!(b.storage, expected);
                assert!(work <= 12 * width + 16, "{width} closures: {work}");
                if !framed {
                    costs.push(work);
                }
            }
            b.cap_mode.fill(CaptureMode::Copy);
            run(&mut b, &topo, &cx, false);
            assert!(b.storage.is_empty());
        }
        assert!(costs[1] <= 10 * costs[0]);
    }
}
