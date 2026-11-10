//! The count a table takes of a key, after its hash and probe.
//! `MapSet` puts and an inserting probe's miss pass a copied key to the table.
//! The miss also stores a copy of the default, which the intrinsic's body lends.

use super::OwnCx;
use crate::ir::{Body, Callee, IntrinsicOp, Ref, Tag};

/// Whether operand `k` (1, the key, or 2, a probe's default) is copied.
pub(super) fn needed(b: &Body, cx: &dyn OwnCx, i: usize, k: usize) -> bool {
    let args = match b.tags.get(i) {
        Some(Tag::Intrinsic)
            if k == 1 && IntrinsicOp::from_u32(b.data[i][0]) == Some(IntrinsicOp::MapSet) =>
        {
            b.record(b.data[i][1])
        }
        Some(Tag::Call)
            if Callee::from_words(b.record(b.data[i][0])).is_some_and(|c| cx.is_map_probe(&c)) =>
        {
            let record = b.record(b.data[i][1]);
            &record[..record.len().saturating_sub(3)]
        }
        _ => return false,
    };
    let [_, _, _] = *args else { return false };
    let r = Ref(args[k]);
    let ty = match r.as_inst() {
        Some(v) => b.ty.get(v.0 as usize).copied(),
        None => b.consts.get((r.0 & !Ref::CONST_BIT) as usize).map(|c| c.0),
    };
    ty.is_some_and(|ty| cx.counted(ty))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, Place, PlaceRoot, Proj, TirBuilder, TirSink, Topology,
        count::{At, Info, Part},
        local_flags, u32_of, verify_owned,
    };
    use crate::own::{Pass, counts, hoist, splice};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts;
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn is_map_probe(&self, callee: &Callee) -> bool {
            matches!(callee, Callee::Item { def, .. } if *def == DefId::from_raw(7))
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

    fn body(key: Ty, through: bool) -> (Body, u32) {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let local = b.local(
            Ty(42),
            Symbol::from_raw(0),
            local_flags::PARAM,
            NodeIdx::NONE,
        );
        let place = b.place(&Place {
            root: PlaceRoot::Local(local),
            proj: if through { vec![Proj::Deref] } else { vec![] },
        });
        let map = b.const_value(Ty(42), 0);
        let key = if key == Ty::STRING {
            b.const_str("key")
        } else {
            b.const_value(key, 0)
        };
        let value = b.const_value(Ty::I32, 0);
        let args = b.refs_record(&[map, key, value]);
        b.emit(
            Tag::Intrinsic,
            IntrinsicOp::MapSet as u32,
            args,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let root = b.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        b.exit_drops.clear();
        (b, place)
    }

    fn run(b: &mut Body) {
        counts::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
    }

    #[test]
    fn a_key_count_is_stated_once_only_when_the_key_counts() {
        for (ty, n) in [(Ty::STRING, 1), (Ty::I32, 0)] {
            let (mut b, _) = body(ty, false);
            b.anchors = vec![42; b.len()];
            run(&mut b);
            run(&mut b);
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), n);
            assert!(
                verify_owned(&b, &Facts).is_empty(),
                "{}",
                crate::ir::print(&b)
            );
            for (i, &tag) in b.tags.iter().enumerate() {
                if tag == Tag::Retain {
                    assert_eq!(b.anchors[i], b.anchors[b.data[i][0] as usize]);
                }
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn a_missing_mislabeled_or_repeated_key_count_is_rejected() {
        let (mut b, _) = body(Ty::STRING, false);
        run(&mut b);
        let retain = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        for (slot, wrong) in [(1, At::Value as u32), (3, 0), (4, 2)] {
            let at = b.data[retain][1] as usize + slot;
            let original = b.extra[at];
            b.extra[at] = wrong;
            assert!(!verify_owned(&b, &Facts).is_empty());
            b.extra[at] = original;
        }
        let topo = Topology::new(&b);
        let mut edits = splice::Tables::default();
        edits.insert(
            b.sub_root[0],
            2,
            Tag::Retain,
            b.data[retain],
            Ty::VOID,
            NodeIdx::NONE,
        );
        splice::apply(&mut b, &topo, &mut edits).unwrap();
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("absent or repeated"))
        );
        let topo = Topology::new(&b);
        edits.clear();
        for (i, &tag) in b.tags.iter().enumerate() {
            if tag == Tag::Retain {
                edits.remove(u32_of(i));
            }
        }
        splice::apply(&mut b, &topo, &mut edits).unwrap();
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("absent or repeated"))
        );
    }

    #[test]
    fn operation_bookkeeping_preserves_both_proofs_but_a_value_retain_breaks_them() {
        for through in [false, true] {
            let (mut b, word) = body(Ty::STRING, through);
            run(&mut b);
            let topo = Topology::new(&b);
            let root = b.sub_root[0];
            let mut edits = splice::Tables::default();
            edits.insert(root, 0, Tag::IsUnique, [word, 0], Ty::VOID, NodeIdx::NONE);
            edits.insert(root, 2, Tag::IsUnique, [word, 0], Ty::VOID, NodeIdx::NONE);
            splice::apply(&mut b, &topo, &mut edits).unwrap();
            let check = b.tags.iter().rposition(|&t| t == Tag::IsUnique).unwrap();
            assert!(
                hoist::repeats(&b, &Topology::new(&b), check),
                "through={through}"
            );
            let retain = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
            b.data[retain][1] = Info::new(At::Value, Part::Result, 0).write(&mut b);
            assert!(
                !hoist::repeats(&b, &Topology::new(&b), check),
                "through={through}"
            );
        }
    }
    fn probe_body(ty: Ty, arity: usize, intrinsic: bool) -> Body {
        use crate::ir::Providers;
        use hd_types::TyList;
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let map = b.const_value(Ty(42), 0);
        let key = if ty == Ty::STRING {
            b.const_str("key")
        } else {
            b.const_value(ty, 0)
        };
        let default = b.const_value(Ty::I32, 0);
        let mut args = vec![map, key];
        args.resize(arity, default);
        b.call(
            &Callee::Item {
                def: DefId::from_raw(if intrinsic { 7 } else { 8 }),
                targs: TyList::EMPTY,
            },
            &args,
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    #[test]
    fn only_an_inserting_probe_states_its_counted_key_copy() {
        for (ty, arity, intrinsic, n) in [
            (Ty::STRING, 3, true, 1),
            (Ty::I32, 3, true, 0),
            (Ty::STRING, 2, true, 0),
            (Ty::STRING, 4, true, 0),
            (Ty::STRING, 3, false, 0),
        ] {
            let mut b = probe_body(ty, arity, intrinsic);
            b.anchors = vec![78; b.len()];
            for _ in 0..2 {
                run(&mut b);
                let copies: Vec<_> = (0..b.len())
                    .filter(|&i| {
                        Info::read(&b, i) == Some(Info::new(At::Operation, Part::Operand(1), 0))
                    })
                    .collect();
                assert_eq!(copies.len(), n);
                for &i in &copies {
                    assert_eq!(b.tags[i], Tag::Retain);
                    let call = b.data[i][0];
                    assert_eq!(b.tags[call as usize], Tag::Call);
                    let topo = Topology::new(&b);
                    assert!(crate::ir::count::adjacent(&b, &topo, call, u32_of(i)));
                    assert_eq!(b.anchors[i], b.anchors[call as usize]);
                }
                let errors = verify_owned(&b, &Facts);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn a_probe_count_requires_the_insertion_fact_and_key_operand() {
        let mut b = probe_body(Ty::STRING, 3, true);
        run(&mut b);
        let copy = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        let point = b.data[copy][0] as usize;
        for info in [
            Info::new(At::Value, Part::Operand(1), 0),
            Info::new(At::Operation, Part::Operand(2), 0),
            Info::new(At::Operation, Part::Operand(1), 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[copy][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
        let mut wrong = b.clone();
        let callee = wrong.data[point][0] as usize;
        let c = Callee::from_words(wrong.record(wrong.data[point][0])).unwrap();
        assert!(Facts.is_map_probe(&c));
        // Item callees encode their def immediately after the record tag.
        let words = Callee::Item {
            def: DefId::from_raw(8),
            targs: hd_types::TyList::EMPTY,
        }
        .words();
        wrong.extra[callee + 1..callee + 1 + words.len()].copy_from_slice(&words);
        assert!(
            verify_owned(&wrong, &Facts)
                .iter()
                .any(|e| e.what.contains("without its count fact"))
        );
        for duplicate in [false, true] {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            if duplicate {
                edits.insert(
                    topo.blk[point],
                    topo.pos[point] + 1,
                    Tag::Retain,
                    wrong.data[copy],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            } else {
                edits.remove(u32_of(copy));
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(
                verify_owned(&wrong, &Facts)
                    .iter()
                    .any(|e| e.what.contains("absent or repeated"))
            );
        }
    }
}
