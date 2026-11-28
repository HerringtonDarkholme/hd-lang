//! What a fixed-arity join owns, from one scan of the body's joins.
//!
//! - The results it holds for its completed children (`Results`), when
//!   they may be counted. Cancellation and frame teardown end this owner;
//!   the done mask selects the completed fields.
//! - The node it owns from its fresh path until its end (`Waker`). Ready,
//!   cancellation and frame teardown end the same owner, once.

use super::OwnCx;
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for i in (0..b.len()).filter(|&i| b.tags[i] == Tag::AwaitAll) {
        if cx.counted(b.ty[i]) {
            let info = Info::new(At::Operation, Part::Results, 2);
            out.push((Tag::Release, u32_of(i), info));
        }
        let info = Info::new(At::Operation, Part::Waker, 2);
        out.push((Tag::Release, u32_of(i), info));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, NONE, Providers, TirBuilder, TirSink, Topology, count, verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx};
    use hd_types::{Ty, TyList};

    struct Facts;
    crate::own::test_cx::own_cx!(
        Facts,
        [
            temporary_owner_name,
            pass,
            pass_value,
            may_write,
            parts,
            holds_coll
        ],
        {
            fn counted(&self, ty: Ty) -> bool {
                matches!(ty, Ty(43 | 45 | 47)) || Cx.counted(ty)
            }

            fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
                Pass::Lent
            }
        }
    );

    fn body(tag: Tag, ty: Ty) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let result = if tag == Tag::Call {
            b.call(
                &Callee::Item {
                    def: DefId::from_raw(7),
                    targs: TyList::EMPTY,
                },
                &[],
                Providers::None,
                ty,
                NodeIdx::NONE,
            )
        } else {
            let children = b.refs_record(&[]);
            b.emit(tag, NONE, children, ty, NodeIdx::NONE)
        };
        let root = b.close_block(root, Some(result), ty, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn state(b: &mut Body) {
        crate::own::tests::state(b, &Facts);
    }

    fn result_packet(b: &Body) -> usize {
        (0..b.len())
            .find(|&i| Info::read(b, i).is_some_and(|c| c.part == Part::Results))
            .unwrap()
    }

    #[test]
    fn only_a_join_with_possibly_counted_results_states_a_cleanup_packet() {
        for tag in [Tag::AwaitAll, Tag::AwaitRace, Tag::Call] {
            // Counted/open tuples, then scalar/empty tuples.
            for ty in [Ty(43), Ty(45), Ty::I32, Ty::VOID] {
                let mut b = body(tag, ty);
                b.anchors = vec![37; b.len()];
                for _ in 0..2 {
                    state(&mut b);
                    let packets: Vec<_> = (0..b.len())
                        .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Results))
                        .collect();
                    assert_eq!(
                        packets.len(),
                        usize::from(tag == Tag::AwaitAll && Facts.counted(ty))
                    );
                    let point = u32_of(b.tags.iter().position(|&t| t == tag).unwrap());
                    let topo = Topology::new(&b);
                    assert_eq!(
                        count::operation(&b, &topo, point, Tag::Release, Part::Waker),
                        (tag == Tag::AwaitAll).then_some(Info::new(At::Operation, Part::Waker, 2))
                    );
                    for i in packets {
                        assert_eq!(b.tags[i], Tag::Release);
                        assert_eq!(b.data[i][0], point);
                        assert_eq!(
                            Info::read(&b, i),
                            Some(Info::new(At::Operation, Part::Results, 2))
                        );
                        assert_eq!(b.anchors[i], b.anchors[point as usize]);
                        assert!(count::adjacent(&b, &topo, point, u32_of(i)));
                        assert_eq!(
                            count::operation(&b, &topo, point, Tag::Release, Part::Results),
                            Info::read(&b, i)
                        );
                    }
                }
                let text = crate::ir::print(&b);
                assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
            }
        }
    }

    #[test]
    fn separate_join_points_never_share_a_completed_result_owner() {
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = builder.open_block();
        let children = builder.refs_record(&[]);
        let a = builder.emit(Tag::AwaitAll, NONE, children, Ty(43), NodeIdx::NONE);
        let b = builder.emit(Tag::AwaitAll, NONE, children, Ty(43), NodeIdx::NONE);
        let values = builder.refs_record(&[a, b]);
        let result = builder.emit(Tag::NewTuple, NONE, values, Ty(45), NodeIdx::NONE);
        let root = builder.close_block(root, Some(result), Ty(45), NodeIdx::NONE);
        let second = b.0;
        let mut b = builder.finish(root, &[]).unwrap();
        // The first join's result is held across the second: the second's
        // cancel block releases it.
        crate::own::scopes::lower(&mut b).expect("lowers");
        let blk = b.cancel_block(second).unwrap();
        let record =
            Info::new(At::Value, Part::Result, crate::own::reason::CANCELLED).write(&mut b);
        let mut edits = splice::Tables::default();
        edits.insert(blk, 0, Tag::Release, [a.0, record], Ty::VOID, NodeIdx::NONE);
        let topo = Topology::new(&b);
        splice::apply(&mut b, &topo, &mut edits).unwrap();
        state(&mut b);
        let i = result_packet(&b);
        let target = b.data[i][0];
        let sibling = u32_of(b.tags.iter().rposition(|&t| t == Tag::AwaitAll).unwrap());
        assert_ne!(target, sibling);
        assert_eq!(
            (0..b.len())
                .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Results))
                .count(),
            2
        );
        b.data[i][0] = sibling;
        assert!(!verify_owned(&b, &Facts).is_empty());
    }

    #[test]
    fn a_join_requires_the_packets_action_part_phase_reason_index_and_position() {
        let mut b = body(Tag::AwaitAll, Ty(43));
        state(&mut b);
        let i = result_packet(&b);
        let infos = [
            Info::new(At::FrameDrop, Part::Results, 2),
            Info::new(At::Operation, Part::Waker, 2),
            Info::new(At::Operation, Part::Results, 0),
        ];
        crate::own::tests::assert_contract_enforced(&b, i, &infos, &Facts);
        let mut wrong = b.clone();
        let record = wrong.data[i][1] as usize;
        wrong.extra[record + 3] = 1;
        assert!(Info::read(&wrong, i).is_none());
        assert!(!verify_owned(&wrong, &Facts).is_empty());
    }

    /// Two joins, then a call.
    fn joins() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        for _ in 0..2 {
            let children = b.refs_record(&[]);
            b.emit(Tag::AwaitAll, NONE, children, Ty::VOID, NodeIdx::NONE);
        }
        b.call(
            &Callee::Item {
                def: DefId::from_raw(7),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn stated() -> Body {
        let mut b = joins();
        let topo = Topology::new(&b);
        statements::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
        b
    }

    #[test]
    fn each_join_states_its_waker_owner_once_even_for_an_uncounted_result() {
        let mut b = joins();
        for _ in 0..2 {
            let topo = Topology::new(&b);
            statements::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
            let topo = Topology::new(&b);
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Release).count(), 2);
            assert!(!b.tags.contains(&Tag::Retain));
            for (i, &tag) in b.tags.iter().enumerate() {
                if !matches!(tag, Tag::AwaitAll | Tag::Call) {
                    continue;
                }
                let actual = crate::ir::count::following(&b, &topo, u32_of(i), Tag::Release);
                assert_eq!(
                    actual,
                    (tag == Tag::AwaitAll).then_some(Info::new(At::Operation, Part::Waker, 2))
                );
            }
            let errors = verify_owned(&b, &Cx);
            assert!(errors.is_empty(), "{errors:?}");
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn a_waker_is_not_the_child_result_or_a_siblings_node() {
        let b = stated();
        let release = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        for (slot, wrong) in [(1, At::Cold as u32), (2, 4), (3, 1), (4, 0)] {
            let mut wrong_body = b.clone();
            let record = wrong_body.data[release][1] as usize;
            wrong_body.extra[record + slot] = wrong;
            assert!(!verify_owned(&wrong_body, &Cx).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[release] = Tag::Retain;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        let sibling = b.tags.iter().rposition(|&t| t == Tag::AwaitAll).unwrap();
        wrong.data[release][0] = u32_of(sibling);
        assert!(!verify_owned(&wrong, &Cx).is_empty());
    }

    #[test]
    fn a_missing_duplicate_or_nonadjacent_waker_contract_is_rejected() {
        let b = stated();
        let release = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        crate::own::tests::assert_bundle_adjacent(&b, release);
    }
}
