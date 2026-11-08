//! A map probe can need an unshared table before lending its value.
//! State that operation's action without moving its check ahead of the
//! hash/probe or out of its hit/miss and short-loan conditions.

use super::OwnCx;
use crate::ir::{
    Body, Callee, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    b.tags
        .iter()
        .enumerate()
        .filter_map(|(i, &tag)| {
            if tag != Tag::Call
                || !Callee::from_words(b.record(b.data[i][0]))
                    .is_some_and(|callee| cx.is_map_probe(&callee))
            {
                return None;
            }
            let args = b.record(b.data[i][1]).len().saturating_sub(3);
            matches!(args, 2 | 3).then_some((
                Tag::MakeUnique,
                u32_of(i),
                Info::new(At::Operation, Part::Operand(0), 5),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Providers, TirBuilder, TirSink, Topology, count, verify_owned};
    use crate::own::{Pass, counts as statements, splice};
    use hd_base::{DefId, NodeIdx};
    use hd_types::{Ty, TyList};

    struct Facts(bool);
    impl OwnCx for Facts {
        fn is_map_probe(&self, callee: &Callee) -> bool {
            self.0 && matches!(callee, Callee::Item { def, .. } if def.raw() == 7)
        }
        fn counted(&self, _: Ty) -> bool {
            false
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

    fn body(n: usize, probe: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let mut args = Vec::new();
        for _ in 0..n {
            args.push(b.call(
                &Callee::Item {
                    def: DefId::from_raw(8),
                    targs: TyList::EMPTY,
                },
                &[],
                Providers::None,
                Ty::I32,
                NodeIdx::NONE,
            ));
        }
        let call = b.call(
            &Callee::Item {
                def: DefId::from_raw(if probe { 7 } else { 9 }),
                targs: TyList::EMPTY,
            },
            &args,
            Providers::None,
            Ty::I32,
            NodeIdx::NONE,
        );
        let root = b.close_block(root, Some(call), Ty::I32, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn state(b: &mut Body) {
        statements::run(
            b,
            &Facts(true),
            &Topology::new(b),
            &mut splice::Tables::default(),
        )
        .unwrap();
        let errors = verify_owned(b, &Facts(true));
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn only_aligned_lookup_and_inserting_probes_state_this_conditional_action() {
        for n in 0..=4 {
            for probe in [false, true] {
                let mut b = body(n, probe);
                let expected = usize::from(probe && matches!(n, 2 | 3));
                assert_eq!(counts(&b, &Facts(true)).len(), expected);
                for _ in 0..2 {
                    state(&mut b);
                    assert_eq!(
                        b.tags.iter().filter(|&&t| t == Tag::MakeUnique).count(),
                        expected
                    );
                }
                let text = crate::ir::print(&b);
                assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
            }
        }
    }

    #[test]
    fn the_action_is_an_anchored_identity_link_not_an_evaluated_map_operand() {
        let mut b = body(3, true);
        b.anchors = vec![29; b.len()];
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::MakeUnique).unwrap();
        let target = b.data[i][0];
        assert!(count::boundary(&b, i));
        assert_eq!(
            crate::own::reason(&b, i),
            Some(crate::own::Reason::MayBeShared)
        );
        assert!(count::adjacent(&b, &Topology::new(&b), target, u32_of(i)));
        assert_eq!(b.value_operands(i), None);
        assert_eq!(
            crate::ir::slot_words(&b, i, |w| w == crate::ir::Word::Inst),
            vec![target]
        );
        assert!(
            crate::ir::slot_words(&b, i, |w| matches!(w, crate::ir::Word::Value(_))).is_empty()
        );
        assert_eq!(b.anchors[i], b.anchors[target as usize]);
        let topo = Topology::new(&b);
        let mut edits = splice::Tables::default();
        edits.insert(
            topo.blk[target as usize],
            0,
            b.tags[0],
            b.data[0],
            b.ty[0],
            b.syn[0],
        );
        splice::apply(&mut b, &topo, &mut edits).unwrap();
        let i = b.tags.iter().position(|&t| t == Tag::MakeUnique).unwrap();
        assert_eq!(b.data[i][0], target + 1);
        assert!(verify_owned(&b, &Facts(true)).is_empty());
    }

    #[test]
    fn the_probe_fact_action_phase_part_reason_target_and_position_are_required() {
        let mut b = body(3, true);
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::MakeUnique).unwrap();
        for info in [
            Info::new(At::Entry, Part::Operand(0), 5),
            Info::new(At::Operation, Part::Operand(1), 5),
            Info::new(At::Operation, Part::Operand(0), 0),
            Info::new(At::Value, Part::Result, 0),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
        }
        for tag in [Tag::Retain, Tag::Release] {
            let mut wrong = b.clone();
            wrong.tags[i] = tag;
            assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
        }
        assert!(!verify_owned(&b, &Facts(false)).is_empty());
        let mut wrong = b.clone();
        wrong.data[i][0] = 0;
        assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
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
                    b.tags[i],
                    b.data[i],
                    Ty::VOID,
                    b.syn[i],
                );
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(
                !verify_owned(&wrong, &Facts(true)).is_empty(),
                "mode {mode}"
            );
        }
    }
}
