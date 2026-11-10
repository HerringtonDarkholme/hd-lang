//! Results a fixed-arity join holds for its completed children.
//! Cancellation and frame teardown end this owner; the done mask selects
//! the completed fields. It is separate from the point's waker owner.

use super::OwnCx;
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts<'a>(
    b: &'a Body,
    cx: &'a dyn OwnCx,
) -> impl Iterator<Item = (Tag, u32, Info)> + 'a {
    b.tags.iter().enumerate().filter_map(move |(i, &tag)| {
        (tag == Tag::AwaitAll && cx.counted(b.ty[i])).then_some((
            Tag::Release,
            u32_of(i),
            Info::new(At::Operation, Part::Results, 2),
        ))
    })
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
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn counted(&self, ty: Ty) -> bool {
            matches!(ty, Ty(43 | 45 | 47)) || Cx.counted(ty)
        }
        fn pass(&self, c: &Callee, k: usize, n: usize) -> Pass {
            Cx.pass(c, k, n)
        }
        fn pass_value(&self, ty: Ty, k: usize, n: usize) -> Pass {
            Cx.pass_value(ty, k, n)
        }
        fn may_write(&self, c: &Callee) -> bool {
            Cx.may_write(c)
        }
        fn parts(&self, ty: Ty, v: Option<u32>, out: &mut Vec<Ty>) -> bool {
            Cx.parts(ty, v, out)
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

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
        statements::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Facts);
        assert!(errors.is_empty(), "{errors:?}");
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
        b.unwind_drops.push((second, a.0));
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
        for info in [
            Info::new(At::FrameDrop, Part::Results, 2),
            Info::new(At::Operation, Part::Waker, 2),
            Info::new(At::Operation, Part::Results, 0),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[i] = Tag::Retain;
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        let mut wrong = b.clone();
        wrong.data[i][0] = wrong.sub_root[0];
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        let mut wrong = b.clone();
        let record = wrong.data[i][1] as usize;
        wrong.extra[record + 3] = 1;
        assert!(Info::read(&wrong, i).is_none());
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        for mode in 0..3 {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let target = wrong.data[i][0] as usize;
            let mut edits = splice::Tables::default();
            if mode != 1 {
                edits.remove(u32_of(i));
            }
            if mode != 0 {
                edits.insert(
                    topo.blk[target],
                    if mode == 1 {
                        topo.pos[i] + 1
                    } else {
                        topo.pos[target]
                    },
                    Tag::Release,
                    wrong.data[i],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
    }
}
