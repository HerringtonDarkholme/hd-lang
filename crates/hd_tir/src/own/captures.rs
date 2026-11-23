//! The closure packets, from one scan of a body's closures.
//!
//! - A fresh environment copies the owned edges captured into it
//!   (`Operation`). Capture modes state which values transfer and which
//!   fields are copied. The ABI/layout lowers the copied packet: a
//!   transferred field takes no count, and a scalar or static frame cell
//!   lowers no count operation.
//! - A cold closure's frame copies the fields its environment keeps
//!   (`Cold`), its reconstructed self environment included, and the
//!   creator loan cells that counted-pointer captures may carry. Concrete
//!   layout selects those fields without another count of the fresh frame.
//! - A closure body takes copies of the captured edges it owns on entry
//!   (`Entry`). Its environment keeps those edges; borrowed captures and
//!   uncounted concrete fields lower no copy. This is apart from explicit
//!   value parameters.
//!
//! The cold and entry packets are per sub-body, stated once however many
//! constructions share it. A crossing closure's body (`sub_flags::ONCE`)
//! has neither: it takes the environment's counts, which the environment
//! then gives up.

use super::OwnCx;
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

/// The three packets, in that order. Cost: one pass over the body, plus
/// each closure's capture record.
pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let recursive = super::RecursiveBindings::new(b);
    let (mut fresh, mut cold, mut entry) = (Vec::new(), Vec::new(), Vec::new());
    let mut stated = vec![[false; 2]; b.sub_root.len()];
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::Closure) {
        let record = b.record(b.data[i][1]);
        let caps = record[0] as usize..(record[0] + record[1]) as usize;
        let own = recursive.self_local(i);
        let counted =
            |c: usize| b.cap_mode[c].shares() || cx.counted(b.local_ty[b.cap_local[c].idx()]);
        if caps
            .clone()
            .any(|c| Some(b.cap_local[c]) != own && counted(c))
        {
            let info = Info::new(At::Operation, Part::Captures, 0);
            fresh.push((Tag::Retain, u32_of(i), info));
        }
        let sub = b.data[i][0] as usize;
        // A body that runs once takes its environment's counts.
        let once = b.sub_flags[sub] & crate::ir::sub_flags::ONCE != 0;
        if sub == 0 || once || !(own.is_some() || caps.clone().any(counted)) {
            continue;
        }
        let suspends = cx.function_suspends(b.ty[i]);
        for (k, at, out) in [(0, At::Cold, &mut cold), (1, At::Entry, &mut entry)] {
            if (k == 1 || suspends) && !std::mem::replace(&mut stated[sub][k], true) {
                out.push((
                    Tag::Retain,
                    b.sub_root[sub],
                    Info::new(at, Part::Captures, 0),
                ));
            }
        }
    }
    fresh.extend(cold);
    fresh.extend(entry);
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, CaptureMode, TirBuilder, TirSink, Topology, local_flags, verify_owned,
    };
    use crate::own::{Pass, counts, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    fn body(caps: &[(Ty, CaptureMode)], function: Ty) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        let locals: Vec<_> = caps
            .iter()
            .map(|(ty, _)| b.local(*ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE))
            .collect();
        let sub = b.open_sub(&[]);
        let inner = b.open_block();
        let root = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
        for l in locals {
            b.capture(sub, l);
        }
        b.close_sub(sub, root, function, NodeIdx::NONE);
        let root = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &caps.iter().map(|c| c.1).collect::<Vec<_>>())
            .unwrap()
    }

    fn closure(b: &Body) -> usize {
        b.tags.iter().position(|&t| t == Tag::Closure).unwrap()
    }

    fn statement(b: &Body) -> Option<usize> {
        (0..b.len()).find(|&i| {
            Info::read(b, i).is_some_and(|c| c.at == At::Operation && c.part == Part::Captures)
        })
    }

    #[test]
    fn capture_modes_and_types_state_one_conditional_copy_packet() {
        for (caps, expected) in [
            (vec![], false),
            (vec![(Ty::I32, CaptureMode::Copy)], false),
            (vec![(Ty::I32, CaptureMode::Shared)], true),
            (vec![(Ty::I32, CaptureMode::Frame)], true),
            (vec![(Ty::STRING, CaptureMode::Copy)], true),
            // A moved field lowers no copy when its ABI transfers ownership;
            // a caller-lent field still needs its own environment copy.
            (vec![(Ty::STRING, CaptureMode::Move)], true),
            (
                vec![
                    (Ty::I32, CaptureMode::Copy),
                    (Ty::STRING, CaptureMode::Move),
                    (Ty::I32, CaptureMode::Shared),
                ],
                true,
            ),
        ] {
            let mut b = body(&caps, Ty(43));
            for _ in 0..2 {
                let topo = Topology::new(&b);
                counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
                let ds: Vec<_> = (0..b.len())
                    .filter(|&i| {
                        Info::read(&b, i)
                            .is_some_and(|c| c.at == At::Operation && c.part == Part::Captures)
                    })
                    .collect();
                assert_eq!(ds.len(), usize::from(expected));
                if let Some(&i) = ds.first() {
                    let point = closure(&b);
                    assert_eq!(b.tags[i], Tag::Retain);
                    assert_eq!(b.data[i][0], u32_of(point));
                    let topo = Topology::new(&b);
                    assert_eq!(
                        crate::ir::count::following(&b, &topo, u32_of(point), Tag::Retain),
                        Some(Info::new(At::Operation, Part::Captures, 0))
                    );
                }
                let errors = verify_owned(&b, &Cx);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn the_copy_requires_its_closure_action_subject_reason_and_adjacency() {
        let mut b = body(&[(Ty::I32, CaptureMode::Shared)], Ty(43));
        let topo = Topology::new(&b);
        counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
        let copy = statement(&b).unwrap();
        for info in [
            Info::new(At::Cold, Part::Captures, 0),
            Info::new(At::Operation, Part::Result, 0),
            Info::new(At::Operation, Part::Captures, 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[copy][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Cx).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[copy] = Tag::Release;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        wrong.data[copy][0] = wrong.sub_root[0];
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        let record = wrong.data[copy][1] as usize;
        wrong.extra[record + 3] = 1;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        for mode in 0..3 {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            let target = wrong.data[copy][0] as usize;
            if mode == 0 {
                edits.remove(u32_of(copy));
            } else {
                edits.insert(
                    if mode == 1 {
                        topo.blk[target]
                    } else {
                        wrong.sub_root[1]
                    },
                    if mode == 1 { topo.pos[target] + 1 } else { 0 },
                    Tag::Retain,
                    wrong.data[copy],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(
                verify_owned(&wrong, &Cx)
                    .iter()
                    .any(|e| e.what.contains("absent or repeated")
                        || e.what.contains("without its count fact"))
            );
        }
    }

    #[test]
    fn self_only_captures_make_no_copy_packet() {
        let mut b = body(&[(Ty(43), CaptureMode::Shared)], Ty(43));
        let ci = closure(&b);
        let syn = NodeIdx::from_raw(1);
        b.syn[ci] = syn;
        b.local_syn[0] = syn;
        b.local_flags[0] |= local_flags::SHORT;
        assert!(b.closure_self(ci).is_some());
        assert!(fresh(&b, &Facts).is_empty());
    }

    struct Facts;
    crate::own::test_cx::own_cx!(Facts, [temporary_owner_name, parts], {
        fn function_suspends(&self, ty: Ty) -> bool {
            ty == Ty(45)
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty == Ty(47)
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

        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    });

    fn declarations(b: &Body) -> Vec<usize> {
        (0..b.len())
            .filter(|&i| Info::read(b, i) == Some(Info::new(At::Cold, Part::Captures, 0)))
            .collect()
    }

    fn state(b: &mut Body) {
        crate::own::tests::state(b, &Facts);
    }

    #[test]
    fn cold_fields_use_types_and_capture_modes_not_the_makers_abi() {
        for (ty, mode, function, wanted) in [
            (Ty::STRING, CaptureMode::Copy, Ty(45), 1),
            (Ty::STRING, CaptureMode::Move, Ty(45), 1),
            (Ty::I32, CaptureMode::Shared, Ty(45), 1),
            (Ty::I32, CaptureMode::Frame, Ty(45), 1),
            (Ty::I32, CaptureMode::Copy, Ty(45), 0),
            (Ty(47), CaptureMode::Copy, Ty(45), 1),
            (Ty::STRING, CaptureMode::Copy, Ty(46), 0),
        ] {
            let mut b = body(&[(ty, mode)], function);
            for _ in 0..2 {
                state(&mut b);
                let ds = declarations(&b);
                assert_eq!(ds.len(), wanted);
                for i in ds {
                    assert_eq!(b.tags[i], Tag::Retain);
                    assert_eq!(b.data[i][0], b.sub_root[1]);
                    assert_eq!(Topology::new(&b).blk[i], b.sub_root[1]);
                }
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn two_constructions_share_one_cold_sub_body_declaration() {
        let mut b = body(&[(Ty::STRING, CaptureMode::Copy)], Ty(45));
        let ci = b.tags.iter().position(|&t| t == Tag::Closure).unwrap();
        let at = b.data[ci][1];
        let topo = Topology::new(&b);
        let mut edits = splice::Tables::default();
        edits.insert(
            topo.blk[ci],
            topo.pos[ci] + 1,
            Tag::Closure,
            [1, at],
            Ty(45),
            NodeIdx::NONE,
        );
        splice::apply(&mut b, &topo, &mut edits).unwrap();
        for _ in 0..2 {
            state(&mut b);
            assert_eq!(declarations(&b).len(), 1);
        }
    }

    #[test]
    fn reconstructed_self_has_a_cold_environment_count_without_a_constructor_field() {
        let mut b = body(&[(Ty(45), CaptureMode::Shared)], Ty(45));
        let ci = b.tags.iter().position(|&t| t == Tag::Closure).unwrap();
        b.syn[ci] = NodeIdx::from_raw(1);
        b.local_syn[0] = b.syn[ci];
        b.local_flags[0] |= local_flags::SHORT;
        assert!(b.closure_self(ci).is_some());
        assert!(fresh(&b, &Facts).is_empty());
        state(&mut b);
        assert_eq!(declarations(&b).len(), 1);
    }

    #[test]
    fn the_cold_copy_needs_its_own_action_phase_root_part_reason_and_multiplicity() {
        let mut b = body(&[(Ty::STRING, CaptureMode::Copy)], Ty(45));
        state(&mut b);
        let i = declarations(&b)[0];
        for info in [
            Info::new(At::Operation, Part::Captures, 0),
            Info::new(At::Cold, Part::Row, 0),
            Info::new(At::Cold, Part::Captures, 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[i] = Tag::Release;
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        let mut wrong = b.clone();
        wrong.data[i][0] = wrong.sub_root[0];
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        for duplicate in [false, true] {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            if duplicate {
                edits.insert(
                    wrong.sub_root[1],
                    0,
                    Tag::Retain,
                    wrong.data[i],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            } else {
                edits.remove(u32_of(i));
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
    }

    /// The fresh-environment packet alone.
    fn fresh(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
        let all = super::counts(b, cx);
        all.into_iter()
            .filter(|c| c.2.at == At::Operation)
            .collect()
    }
}
