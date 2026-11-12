//! A function-value spread copies the owned parameters of its concrete ABI.
//! One packet names those copies even when a generic input tuple's shape
//! stays open here. Matching arity and cold constructors use other paths.

use super::OwnCx;
use crate::ir::{
    Body, Ref, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::CallValue {
            continue;
        }
        let callee = Ref(b.data[i][0]);
        let ty = match callee.as_inst() {
            Some(v) => b.ty[v.0 as usize],
            None => b.consts[(callee.0 & !Ref::CONST_BIT) as usize].0,
        };
        let n = b.record(b.data[i][1]).len();
        if cx.function_spreads_arguments(ty, n) {
            out.push((
                Tag::Retain,
                u32_of(i),
                Info::new(At::Operation, Part::Arguments, 0),
            ));
        }
        if (0..n).any(|k| {
            cx.pass_value(ty, k, n) == super::Pass::Owned && cx.function_lends_counts(ty, k, n)
        }) {
            // An owning argument can retain a carrier of borrowed reference
            // counts after its native components transfer to the callback.
            out.push((
                Tag::Release,
                u32_of(i),
                Info::new(At::Operation, Part::Arguments, 2),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, ReadMode, TirBuilder, TirSink, Topology, count, local_flags, verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts;
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn function_spreads_arguments(&self, ty: Ty, args: usize) -> bool {
            match ty {
                Ty(43) => args != 2,
                Ty(44) => args != 1,
                Ty(45) => true, // Unresolved Tuple-bounded inputs.
                _ => false,     // A cold constructor or a non-function value.
            }
        }
        fn function_has_requirement_row(&self, ty: Ty) -> bool {
            ty == Ty(45)
        }
        fn counted(&self, ty: Ty) -> bool {
            Cx.counted(ty)
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

    fn body(ty: Ty, constant: bool, n: usize) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let callee = if constant {
            b.const_value(ty, 0)
        } else {
            let l = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
            b.get(l, ty, NodeIdx::NONE)
        };
        let arg = b.const_value(Ty::I32, 0);
        let args = b.refs_record(&vec![arg; n]);
        b.emit(Tag::CallValue, callee.0, args, Ty::I32, NodeIdx::NONE);
        let root = b.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        for (i, &tag) in b.tags.iter().enumerate() {
            if tag == Tag::Read {
                b.data[i][1] = ReadMode::Borrow as u32;
            }
        }
        b
    }

    fn state(b: &mut Body) {
        statements::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_spread_states_one_packet_from_its_input_shape() {
        for ty in [Ty(43), Ty(44), Ty(45), Ty(46)] {
            for constant in [false, true] {
                for n in 0..3 {
                    let mut b = body(ty, constant, n);
                    b.anchors = vec![37; b.len()];
                    for _ in 0..2 {
                        state(&mut b);
                        let ds: Vec<_> = (0..b.len())
                            .filter(|&i| {
                                Info::read(&b, i).is_some_and(|c| c.part == Part::Arguments)
                            })
                            .collect();
                        assert_eq!(
                            ds.len(),
                            usize::from(Facts.function_spreads_arguments(ty, n))
                        );
                        for i in ds {
                            let target = b.data[i][0];
                            assert_eq!(b.tags[i], Tag::Retain);
                            assert_eq!(b.tags[target as usize], Tag::CallValue);
                            assert_eq!(
                                Info::read(&b, i),
                                Some(Info::new(At::Operation, Part::Arguments, 0))
                            );
                            assert!(count::adjacent(&b, &Topology::new(&b), target, u32_of(i)));
                            assert_eq!(b.anchors[i], b.anchors[target as usize]);
                        }
                    }
                    let text = crate::ir::print(&b);
                    assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
                }
            }
        }
    }

    #[test]
    fn a_spread_and_its_row_keep_independent_action_and_part_readers() {
        let mut b = body(Ty(45), false, 1);
        state(&mut b);
        let target = u32_of(b.tags.iter().position(|&t| t == Tag::CallValue).unwrap());
        let topo = Topology::new(&b);
        for (action, part, reason) in [
            (Tag::Retain, Part::Row, 0),
            (Tag::Release, Part::Row, 2),
            (Tag::Retain, Part::Arguments, 0),
        ] {
            assert_eq!(
                count::operation(&b, &topo, target, action, part),
                Some(Info::new(At::Operation, part, reason))
            );
        }
        assert_eq!(
            count::operation(&b, &topo, target, Tag::Release, Part::Arguments),
            None
        );
    }

    #[test]
    fn a_spread_needs_its_packets_action_part_phase_reason_target_and_position() {
        let mut b = body(Ty(43), false, 1);
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        for info in [
            Info::new(At::Cold, Part::Arguments, 0),
            Info::new(At::Operation, Part::Row, 0),
            Info::new(At::Operation, Part::Arguments, 2),
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
        let record = wrong.data[i][1] as usize;
        wrong.extra[record + 3] = 1; // Arguments has no element index.
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
