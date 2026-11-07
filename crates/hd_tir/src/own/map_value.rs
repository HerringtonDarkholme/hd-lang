//! A map lookup's hit copies its borrowed table value into a Some payload.
//! A miss has no payload; a short-loan result lowers no copy helper. The
//! packet counts copied fields, never a fresh enum container's initial count.

use super::OwnCx;
use crate::ir::{
    Body, IntrinsicOp, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    b.tags
        .iter()
        .enumerate()
        .filter(|&(i, &tag)| {
            tag == Tag::Intrinsic
                && IntrinsicOp::from_u32(b.data[i][0]) == Some(IntrinsicOp::MapGet)
                && cx.counted(b.ty[i])
        })
        .map(|(i, _)| {
            (
                Tag::Retain,
                u32_of(i),
                Info::new(At::Operation, Part::Operand(0), 0),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, TirBuilder, TirSink, Topology, count, verify_owned};
    use crate::own::{Pass, counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx};
    use hd_types::Ty;

    struct Facts;
    impl OwnCx for Facts {
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
        fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
            Cx.received(item, k, n)
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

    fn body(op: IntrinsicOp, ty: Ty) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let map = b.const_value(Ty(44), 0);
        let key = b.const_value(Ty::STRING, 0);
        let args = b.refs_record(&[map, key]);
        let result = b.emit(Tag::Intrinsic, op as u32, args, ty, NodeIdx::NONE);
        let root = b.close_block(block, Some(result), ty, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn state(b: &mut Body) {
        statements::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_get_states_one_hit_packet_only_for_possibly_counted_results() {
        for op in [
            IntrinsicOp::MapGet,
            IntrinsicOp::MapRemove,
            IntrinsicOp::MapEntry,
        ] {
            // String/collection/open results, followed by scalar/no-value results.
            for ty in [Ty(43), Ty(45), Ty(47), Ty::I32, Ty::VOID] {
                let mut b = body(op, ty);
                b.anchors = vec![37; b.len()];
                for _ in 0..2 {
                    state(&mut b);
                    let ds: Vec<_> = (0..b.len())
                        .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Operand(0)))
                        .collect();
                    assert_eq!(
                        ds.len(),
                        usize::from(op == IntrinsicOp::MapGet && Facts.counted(ty))
                    );
                    for i in ds {
                        let target = b.data[i][0];
                        assert_eq!(b.tags[i], Tag::Retain);
                        assert_eq!(b.tags[target as usize], Tag::Intrinsic);
                        assert_eq!(
                            Info::read(&b, i),
                            Some(Info::new(At::Operation, Part::Operand(0), 0))
                        );
                        assert_eq!(b.anchors[i], b.anchors[target as usize]);
                        assert!(count::adjacent(&b, &Topology::new(&b), target, u32_of(i)));
                        assert_eq!(
                            count::operation(
                                &b,
                                &Topology::new(&b),
                                target,
                                Tag::Retain,
                                Part::Operand(0)
                            ),
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
    fn a_get_needs_its_packets_action_part_phase_reason_target_and_position() {
        let mut b = body(IntrinsicOp::MapGet, Ty(43));
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        for info in [
            Info::new(At::Cold, Part::Operand(0), 0),
            Info::new(At::Operation, Part::Row, 0),
            Info::new(At::Operation, Part::Operand(0), 2),
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
        let mut wrong = b.clone();
        wrong.data[i][1] = Info::new(At::Operation, Part::Operand(1), 0).write(&mut wrong);
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
                    Tag::Retain,
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
