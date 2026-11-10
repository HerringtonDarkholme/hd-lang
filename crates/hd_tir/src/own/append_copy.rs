//! A place append can transfer its destination's owner instead of the
//! copied left read's. State cleanup of that single-use copy separately.
//! The existing place-transfer lowering conditions this cleanup; other
//! concatenations keep their ordinary operand drops.

use super::{OwnCx, Role, operands};
use crate::ir::{
    Body, IntrinsicOp, ReadMode, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let candidates: Vec<_> = b
        .tags
        .iter()
        .enumerate()
        .filter_map(|(i, &tag)| {
            if tag != Tag::Intrinsic
                || IntrinsicOp::from_u32(b.data[i][0]) != Some(IntrinsicOp::StrConcat)
            {
                return None;
            }
            let &[left, _] = b.record(b.data[i][1]) else {
                return None;
            };
            ((left as usize) < b.len()
                && b.tags[left as usize] == Tag::Read
                && b.data[left as usize][1] == ReadMode::Copy as u32
                && cx.counted(b.ty[left as usize]))
            .then_some((i, left))
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut uses = vec![0u32; b.len()];
    let mut ops = Vec::new();
    for i in 0..b.len() {
        operands(b, i, &mut ops);
        for op in &ops {
            if op.role != Role::Dropped {
                uses[op.value as usize] += 1;
            }
        }
    }
    candidates
        .into_iter()
        .filter(|&(_, left)| uses[left as usize] == 1)
        .map(|(i, _)| {
            (
                Tag::Release,
                u32_of(i),
                Info::new(At::Operation, Part::Operand(0), 2),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, Place, PlaceRoot, Providers, Ref, TirBuilder, TirSink, Topology, count,
        local_flags, verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::{Ty, TyList};

    struct Facts;
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING
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

    fn body(op: IntrinsicOp, mode: ReadMode, arity: usize, repeat: bool, extra: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let local = b.local(
            Ty::STRING,
            Symbol::from_raw(0),
            local_flags::PARAM,
            NodeIdx::NONE,
        );
        let left = b.get(local, Ty::STRING, NodeIdx::NONE);
        let suffix = b.const_str("suffix");
        if extra {
            b.call(
                &Callee::Item {
                    def: DefId::from_raw(8),
                    targs: TyList::EMPTY,
                },
                &[left],
                Providers::None,
                Ty::VOID,
                NodeIdx::NONE,
            );
        }
        let args: Vec<_> = (0..arity)
            .map(|k| if k == 0 || repeat { left } else { suffix })
            .collect();
        let args = b.refs_record(&args);
        let result = b.emit(Tag::Intrinsic, op as u32, args, Ty::STRING, NodeIdx::NONE);
        let place = b.place(&Place {
            root: PlaceRoot::Value(left),
            proj: vec![],
        });
        let root = b.close_block(block, Some(result), Ty::STRING, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        b.data[left.0 as usize][1] = mode as u32;
        // The builder checks pre-ownership TIR. Add this decided read's
        // ordinary drop only after finishing that input body.
        if mode != ReadMode::Borrow {
            let topo = Topology::new(&b);
            let mut edits = splice::Tables::default();
            edits.insert(
                topo.blk[result.0 as usize],
                topo.pos[result.0 as usize] + 1,
                Tag::Drop,
                [place, 2],
                Ty::VOID,
                NodeIdx::NONE,
            );
            splice::apply(&mut b, &topo, &mut edits).unwrap();
        }
        b
    }

    fn state(b: &mut Body) {
        statements::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn only_a_binary_concats_single_use_copied_read_has_conditional_cleanup() {
        for op in [IntrinsicOp::StrConcat, IntrinsicOp::StrEq] {
            for mode in [ReadMode::Copy, ReadMode::Borrow, ReadMode::Move] {
                for arity in 0..4 {
                    for repeat in [false, true] {
                        for extra in [false, true] {
                            let b = body(op, mode, arity, repeat, extra);
                            let expected = op == IntrinsicOp::StrConcat
                                && mode == ReadMode::Copy
                                && arity == 2
                                && !repeat
                                && !extra;
                            assert_eq!(counts(&b, &Facts).len(), usize::from(expected));
                        }
                    }
                }
            }
        }
        let mut b = body(IntrinsicOp::StrConcat, ReadMode::Copy, 2, false, false);
        let at = b.tags.iter().position(|&t| t == Tag::Intrinsic).unwrap();
        let constant = b.consts.len();
        b.consts.push((Ty::STRING, 0));
        let record = b.data[at][1] as usize;
        b.extra[record + 1] = Ref::CONST_BIT | u32_of(constant);
        assert!(counts(&b, &Facts).is_empty());
    }

    #[test]
    fn a_drop_is_not_a_second_use_and_the_packet_keeps_its_anchor_and_position() {
        let mut b = body(IntrinsicOp::StrConcat, ReadMode::Copy, 2, false, false);
        b.anchors = vec![37; b.len()];
        for _ in 0..2 {
            state(&mut b);
            let target = u32_of(b.tags.iter().position(|&t| t == Tag::Intrinsic).unwrap());
            let topo = Topology::new(&b);
            assert_eq!(
                count::operation(&b, &topo, target, Tag::Release, Part::Operand(0)),
                Some(Info::new(At::Operation, Part::Operand(0), 2))
            );
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Release).count(), 1);
            let i = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
            assert_eq!(b.anchors[i], b.anchors[target as usize]);
            assert!(count::adjacent(&b, &topo, target, u32_of(i)));
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn cleanup_requires_its_action_phase_operand_reason_target_and_position() {
        let mut b = body(IntrinsicOp::StrConcat, ReadMode::Copy, 2, false, false);
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        for info in [
            Info::new(At::Exit, Part::Operand(0), 2),
            Info::new(At::Operation, Part::Operand(1), 2),
            Info::new(At::Operation, Part::Operand(0), 0),
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
                    if mode == 2 { 0 } else { topo.pos[target] + 1 },
                    wrong.tags[i],
                    wrong.data[i],
                    Ty::VOID,
                    wrong.syn[i],
                );
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(!verify_owned(&wrong, &Facts).is_empty(), "mode {mode}");
        }
    }
}
