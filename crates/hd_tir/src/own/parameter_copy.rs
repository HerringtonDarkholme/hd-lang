//! The copy a plain body owns when its explicit parameter arrives lent.
//! The exit set states that ownership; the concrete ABI either supplies
//! the count or lends the value and lowers this entry copy. Captures and
//! suspending boundaries have separate contracts.

use super::{OwnCx, Tri, cold};
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut suspended = vec![false; b.sub_root.len()];
    for sub in cold::sub_bodies(b, cx) {
        suspended[sub as usize] = true;
    }
    let mut out = Vec::new();
    for (sub, &suspends) in suspended.iter().enumerate() {
        if suspends {
            continue;
        }
        for l in cold::locals(b, crate::ir::u32_of(sub)) {
            let ty = b.local_ty[l as usize];
            if cx.counted(ty)
                && cx.holds_drop(ty) != Tri::Yes
                && b.exit_drops
                    .iter()
                    .any(|&(s, local)| s.idx() == sub && local.raw() == l)
            {
                out.push((
                    Tag::Retain,
                    b.sub_root[sub],
                    Info::new(At::Entry, Part::Local(l), 0),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, CaptureMode, TirBuilder, TirSink, Topology, count, local_flags,
        verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice};
    use hd_base::{DefId, LocalId, NodeIdx, SubId, Symbol};
    use hd_types::Ty;

    struct Facts {
        suspends: bool,
        drop: Tri,
    }
    impl OwnCx for Facts {
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING
        }
        fn holds_drop(&self, _: Ty) -> Tri {
            self.drop
        }
        fn suspending(&self, _: DefId) -> bool {
            self.suspends
        }
        fn function_suspends(&self, ty: Ty) -> bool {
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

    fn body(ty: Ty, owned: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let l = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        if owned {
            b.exit_drops.push((SubId::from_raw(0), l));
        }
        b
    }

    fn state(b: &mut Body, facts: &Facts) {
        statements::run(b, facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn an_owned_plain_value_parameter_declares_a_possible_copy_but_a_drop_value_never_does() {
        for ty in [Ty::STRING, Ty::I32] {
            for owned in [false, true] {
                for suspends in [false, true] {
                    for drop in [Tri::No, Tri::Maybe, Tri::Yes] {
                        let facts = Facts { suspends, drop };
                        let mut b = body(ty, owned);
                        assert_eq!(
                            counts(&b, &facts).len(),
                            usize::from(ty == Ty::STRING && owned && !suspends && drop != Tri::Yes)
                        );
                        for _ in 0..2 {
                            state(&mut b, &facts);
                        }
                        let text = crate::ir::print(&b);
                        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
                    }
                }
            }
        }
    }

    #[test]
    fn a_closures_explicit_parameters_are_separate_from_its_captures_and_cold_boundary() {
        for function in [Ty(43), Ty(44)] {
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let outer = b.open_block();
            let captured = b.local(
                Ty::STRING,
                Symbol::from_raw(0),
                local_flags::PARAM,
                NodeIdx::NONE,
            );
            let parameter = b.local(
                Ty::STRING,
                Symbol::from_raw(1),
                local_flags::PARAM,
                NodeIdx::NONE,
            );
            let sub = b.open_sub(&[parameter]);
            let inner = b.open_block();
            let inner = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
            b.capture(sub, captured);
            b.close_sub(sub, inner, function, NodeIdx::NONE);
            let outer = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
            let mut b = b.finish(outer, &[CaptureMode::Copy]).unwrap();
            b.exit_drops = vec![
                (SubId::from_raw(0), captured),
                (sub.sub(), captured),
                (sub.sub(), parameter),
            ];
            let facts = Facts {
                suspends: false,
                drop: Tri::No,
            };
            let copies = counts(&b, &facts);
            assert_eq!(copies.len(), if function == Ty(43) { 1 } else { 2 });
            assert!(copies.contains(&(
                Tag::Retain,
                b.sub_root[0],
                Info::new(At::Entry, Part::Local(captured.raw()), 0)
            )));
            assert!(!copies.contains(&(
                Tag::Retain,
                b.sub_root[sub.sub().idx()],
                Info::new(At::Entry, Part::Local(captured.raw()), 0)
            )));
            state(&mut b, &facts);
        }
    }

    #[test]
    fn the_owned_parameter_fact_and_the_exact_boundary_contract_are_verified() {
        let facts = Facts {
            suspends: false,
            drop: Tri::No,
        };
        let mut b = body(Ty::STRING, true);
        b.anchors = vec![37; b.len()];
        state(&mut b, &facts);
        let i = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        let target = b.data[i][0];
        assert_eq!(
            count::find(&b, Tag::Retain, target, At::Entry, Part::Local(0)),
            Some(Info::new(At::Entry, Part::Local(0), 0))
        );
        assert_eq!(b.syn[i], b.local_syn[0]);
        assert_eq!(b.anchors[target as usize], 37);
        for info in [
            Info::new(At::Cold, Part::Local(0), 0),
            Info::new(At::Entry, Part::Local(0), 2),
            Info::new(At::Entry, Part::Result, 0),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &facts).is_empty());
        }
        let mut wrong = b.clone();
        wrong.exit_drops.clear();
        assert!(!verify_owned(&wrong, &facts).is_empty());
        let mut wrong = b.clone();
        wrong.tags[i] = Tag::Release;
        assert!(!verify_owned(&wrong, &facts).is_empty());
        let mut wrong = b.clone();
        wrong.local_flags[LocalId::from_raw(0).idx()] = 0;
        assert!(!verify_owned(&wrong, &facts).is_empty());
        let topo = Topology::new(&b);
        for duplicate in [false, true] {
            let mut wrong = b.clone();
            let mut edits = splice::Tables::default();
            if duplicate {
                edits.insert(
                    topo.blk[i],
                    topo.pos[i],
                    b.tags[i],
                    b.data[i],
                    Ty::VOID,
                    b.syn[i],
                );
            } else {
                edits.remove(crate::ir::u32_of(i));
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(!verify_owned(&wrong, &facts).is_empty());
        }
    }
}
