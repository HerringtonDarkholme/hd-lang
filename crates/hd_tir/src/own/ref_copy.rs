//! Whole-local references that copy an existing cell in a suspending body.
//! Copies of captures are values in the environment, not the maker's cell:
//! a counted reference to one allocates an owned object instead.

use super::{OwnCx, cold};
use crate::ir::{Body, Tag, Topology, local_flags, ref_word};
use hd_base::SliceSort as _;

pub(super) fn results(b: &Body, cx: &dyn OwnCx, topo: &Topology) -> Vec<bool> {
    let mut suspends = vec![false; b.sub_root.len()];
    for sub in cold::sub_bodies(b, cx) {
        suspends[sub as usize] = true;
    }
    // Match the raw capture representation: immutable recursive bindings
    // are copied as function values even when the capture mode is shared.
    let mut recursive = vec![false; b.local_ty.len()];
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure
            && let Some(l) = b.closure_self(i)
            && b.local_flags[l.idx()] & local_flags::SHORT != 0
            && b.syn[i] == b.local_syn[l.idx()]
        {
            recursive[l.idx()] = true;
        }
    }
    let mut copied = Vec::new();
    for i in 0..b.len() {
        if b.tags[i] != Tag::Closure || b.is_instantiation(i) {
            continue;
        }
        let own = b.closure_self(i);
        let record = b.record(b.data[i][1]);
        for c in record[0]..record[0] + record[1] {
            let c = c as usize;
            let l = b.cap_local[c];
            if !b.cap_mode[c].shares() || recursive[l.idx()] || own == Some(l) {
                copied.push((b.data[i][0], l.raw()));
            }
        }
    }
    copied.sort_stable();
    (0..b.len())
        .map(|i| {
            if b.tags[i] != Tag::Ref
                || b.data[i][1] & (ref_word::FRAME | ref_word::TAKES) != 0
                || !suspends.get(topo.sub[i] as usize).copied().unwrap_or(false)
                || !cx.thin_counted_reference(b.ty[i])
            {
                return false;
            }
            let p = b.view(b.data[i][0]);
            p.whole()
                && p.local()
                    .is_some_and(|l| copied.binary_search(&(topo.sub[i], l)).is_err())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, CaptureMode, Place, PlaceRoot, Proj, TirBuilder, TirSink,
        count::{At, Info, Part},
        verify_owned,
    };
    use crate::own::{Pass, retains, splice};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts(bool);
    impl OwnCx for Facts {
        fn suspending(&self, _: DefId) -> bool {
            self.0
        }
        fn function_suspends(&self, ty: Ty) -> bool {
            ty == Ty(45)
        }
        fn thin_counted_reference(&self, ty: Ty) -> bool {
            ty == Ty(43)
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty(43)
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn may_write(&self, _: &Callee) -> bool {
            false
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    }

    fn body(flags: u32, ty: Ty, proj: Vec<Proj>) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let l = b.local(
            Ty::I32,
            Symbol::from_raw(0),
            local_flags::PARAM,
            NodeIdx::NONE,
        );
        let p = b.place(&Place {
            root: PlaceRoot::Local(l),
            proj,
        });
        let r = b.emit(
            Tag::Ref,
            p,
            flags & !(ref_word::FRAME | ref_word::TAKES),
            ty,
            NodeIdx::NONE,
        );
        let root = b.close_block(root, Some(r), ty, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        // These two bits are written by ownership, after checked TIR.
        let i = b.tags.iter().position(|&t| t == Tag::Ref).unwrap();
        b.data[i][1] = flags;
        b
    }

    #[test]
    fn only_whole_thin_counted_cell_copies_in_a_suspending_body_qualify() {
        for (suspends, flags, ty, proj, wanted) in [
            (true, ref_word::LOCAL, Ty(43), vec![], true),
            (true, ref_word::LOCAL | ref_word::MUT, Ty(43), vec![], true),
            (false, ref_word::LOCAL, Ty(43), vec![], false),
            (
                true,
                ref_word::LOCAL | ref_word::FRAME,
                Ty(43),
                vec![],
                false,
            ),
            (true, ref_word::TAKES, Ty(43), vec![], false),
            (true, ref_word::LOCAL, Ty(44), vec![], false),
            (true, ref_word::LOCAL, Ty(43), vec![Proj::Field(0)], false),
            (true, ref_word::LOCAL, Ty(43), vec![Proj::Deref], false),
        ] {
            let b = body(flags, ty, proj);
            assert_eq!(
                results(&b, &Facts(suspends), &Topology::new(&b))
                    .into_iter()
                    .filter(|v| *v)
                    .count(),
                usize::from(wanted)
            );
        }
    }

    #[test]
    fn each_closure_uses_its_own_abi_and_distinguishes_copied_captures_from_cells() {
        for fn_ty in [Ty(45), Ty(46)] {
            for mode in [
                CaptureMode::Copy,
                CaptureMode::Move,
                CaptureMode::Shared,
                CaptureMode::Frame,
            ] {
                let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
                let outer = b.open_block();
                let cap = b.local(
                    Ty::I32,
                    Symbol::from_raw(0),
                    local_flags::PARAM,
                    NodeIdx::NONE,
                );
                let own = b.local(
                    Ty::I32,
                    Symbol::from_raw(0),
                    local_flags::PARAM,
                    NodeIdx::NONE,
                );
                let sub = b.open_sub(&[own]);
                let inner = b.open_block();
                let mut refs = Vec::new();
                for l in [cap, own] {
                    let p = b.place(&Place {
                        root: PlaceRoot::Local(l),
                        proj: vec![],
                    });
                    refs.push(b.emit(Tag::Ref, p, ref_word::LOCAL, Ty(43), NodeIdx::NONE));
                }
                let root = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
                b.capture(sub, cap);
                b.close_sub(sub, root, fn_ty, NodeIdx::NONE);
                let root = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
                let b = b.finish(root, &[mode]).unwrap();
                let facts = results(&b, &Facts(false), &Topology::new(&b));
                assert_eq!(facts[refs[0].0 as usize], fn_ty == Ty(45) && mode.shares());
                assert_eq!(facts[refs[1].0 as usize], fn_ty == Ty(45));
            }
        }
    }

    #[test]
    fn the_value_statement_is_idempotent_and_verifies_its_fact_phase_reason_and_adjacency() {
        let mut b = body(ref_word::LOCAL, Ty(43), vec![]);
        b.anchors = vec![17; b.len()];
        for _ in 0..2 {
            let topo = Topology::new(&b);
            retains::run(&mut b, &Facts(true), &topo, &mut splice::Tables::default()).unwrap();
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 1);
            assert!(verify_owned(&b, &Facts(true)).is_empty());
        }
        let i = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        assert_eq!(b.anchors[i], 17);
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        for info in [
            Info::new(At::Operation, Part::Result, 0),
            Info::new(At::Value, Part::Operand(0), 0),
            Info::new(At::Value, Part::Result, 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
        }
        assert!(!verify_owned(&b, &Facts(false)).is_empty());
        for duplicate in [false, true] {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            if duplicate {
                edits.insert(
                    topo.blk[i],
                    topo.pos[i] + 1,
                    Tag::Retain,
                    wrong.data[i],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            } else {
                edits.remove(crate::ir::u32_of(i));
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
        }
    }
}
