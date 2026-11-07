//! The count a table takes of a `MapSet` key, after its hash and probe.

use super::OwnCx;
use crate::ir::{Body, IntrinsicOp, Ref, Tag};

pub(super) fn needed(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    if b.tags.get(i) != Some(&Tag::Intrinsic)
        || IntrinsicOp::from_u32(b.data[i][0]) != Some(IntrinsicOp::MapSet)
    {
        return false;
    }
    let [_, key, _] = *b.record(b.data[i][1]) else {
        return false;
    };
    let key = Ref(key);
    let ty = match key.as_inst() {
        Some(v) => b.ty.get(v.0 as usize).copied(),
        None => b
            .consts
            .get((key.0 & !Ref::CONST_BIT) as usize)
            .map(|c| c.0),
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
}
