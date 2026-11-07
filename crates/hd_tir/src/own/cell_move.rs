//! A whole-local move transfers its payload and ends its storage owner.
//! Storage rows are per sub-body. Their physical heap-cell lowering ends
//! one cell count, independently of whether the payload itself counts;
//! a stack slot or a promoted local lowers no cell release.

use crate::ir::{
    Body, ReadMode, Tag, Topology,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, topo: &Topology) -> Vec<(Tag, u32, Info)> {
    if b.storage.is_empty() {
        return Vec::new();
    }
    b.tags
        .iter()
        .enumerate()
        .filter_map(|(i, &tag)| {
            if tag != Tag::Read || b.data[i][1] != ReadMode::Move as u32 {
                return None;
            }
            let local = b.local_read(i)?.raw();
            // storage::run writes rows in (sub, local) order. A row says this
            // local has placed storage here, without repeating its inference.
            b.storage
                .binary_search_by_key(&(topo.sub[i], local), |&(s, l, _)| (s.raw(), l.raw()))
                .is_ok()
                .then_some((
                    Tag::Release,
                    u32_of(i),
                    Info::new(At::Operation, Part::Local(local), 2),
                ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, Place, PlaceRoot, Proj, Providers, Storage, TirBuilder, TirSink, count,
        verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice};
    use hd_base::{DefId, LocalId, NodeIdx, SubId, Symbol};
    use hd_types::{Ty, TyList};

    struct Facts;
    impl crate::own::OwnCx for Facts {
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

    fn local_value(b: &mut TirBuilder, ty: Ty) -> LocalId {
        let l = b.local(ty, Symbol::from_raw(0), 0, NodeIdx::NONE);
        let value = b.call(
            &Callee::Item {
                def: DefId::from_raw(7),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            ty,
            NodeIdx::NONE,
        );
        b.set(l, value, NodeIdx::NONE);
        l
    }

    fn body(ty: Ty, mode: ReadMode, storage: Option<Storage>, projected: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let l = local_value(&mut b, ty);
        b.local(ty, Symbol::from_raw(1), 0, NodeIdx::NONE); // A different valid local index.
        let read = if projected {
            let place = b.place(&Place {
                root: PlaceRoot::Local(l),
                proj: vec![Proj::Field(0)],
            });
            b.emit(Tag::Read, place, ReadMode::Use as u32, ty, NodeIdx::NONE)
        } else {
            b.get(l, ty, NodeIdx::NONE)
        };
        let root = b.close_block(root, Some(read), ty, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        b.data[read.0 as usize][1] = mode as u32;
        if let Some(storage) = storage {
            b.storage.push((SubId::from_raw(0), l, storage));
        }
        b
    }

    fn state(b: &mut Body) {
        statements::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_whole_move_ends_placed_storage_independently_of_its_payload_type() {
        for ty in [Ty::STRING, Ty::I32] {
            for mode in [ReadMode::Move, ReadMode::Copy, ReadMode::Borrow] {
                for storage in [None, Some(Storage::Cell), Some(Storage::Slot)] {
                    for projected in [false, true] {
                        let b = body(ty, mode, storage, projected);
                        assert_eq!(
                            counts(&b, &Topology::new(&b)).len(),
                            usize::from(mode == ReadMode::Move && storage.is_some() && !projected)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_storage_row_in_another_sub_body_does_not_own_this_moves_cell() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let outer = local_value(&mut b, Ty::STRING);
        let sub = b.open_sub(&[]);
        let inner = b.open_block();
        let local = local_value(&mut b, Ty::STRING);
        let read = b.get(local, Ty::STRING, NodeIdx::NONE);
        let inner = b.close_block(inner, Some(read), Ty::STRING, NodeIdx::NONE);
        b.close_sub(sub, inner, Ty(43), NodeIdx::NONE);
        let outer = b.get(outer, Ty::STRING, NodeIdx::NONE);
        let root = b.close_block(root, Some(outer), Ty::STRING, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        b.data[read.0 as usize][1] = ReadMode::Move as u32;
        b.data[outer.0 as usize][1] = ReadMode::Move as u32;
        b.storage.push((SubId::from_raw(0), local, Storage::Cell));
        assert!(counts(&b, &Topology::new(&b)).is_empty());
        b.storage.push((sub.sub(), local, Storage::Cell));
        assert_eq!(
            counts(&b, &Topology::new(&b)),
            vec![(
                Tag::Release,
                read.0,
                Info::new(At::Operation, Part::Local(local.raw()), 2)
            )]
        );
        state(&mut b);
    }

    #[test]
    fn the_cleanup_is_idempotent_adjacent_anchored_and_round_trips_with_storage() {
        for ty in [Ty::STRING, Ty::I32] {
            for storage in [Storage::Cell, Storage::Slot] {
                let mut b = body(ty, ReadMode::Move, Some(storage), false);
                b.anchors = vec![37; b.len()];
                for _ in 0..2 {
                    state(&mut b);
                    let i = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
                    let target = b.data[i][0];
                    let topo = Topology::new(&b);
                    assert_eq!(
                        count::operation(&b, &topo, target, Tag::Release, Part::Local(0)),
                        Some(Info::new(At::Operation, Part::Local(0), 2))
                    );
                    assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Release).count(), 1);
                    assert_eq!(b.anchors[i], b.anchors[target as usize]);
                    assert!(count::adjacent(&b, &topo, target, u32_of(i)));
                    assert_eq!(
                        b.storage,
                        vec![(SubId::from_raw(0), LocalId::from_raw(0), storage)]
                    );
                }
                let text = crate::ir::print(&b);
                assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
            }
        }
    }

    #[test]
    fn cleanup_requires_its_storage_fact_action_local_phase_reason_target_and_position() {
        let mut b = body(Ty::STRING, ReadMode::Move, Some(Storage::Cell), false);
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        for info in [
            Info::new(At::Exit, Part::Local(0), 2),
            Info::new(At::Operation, Part::Local(1), 2),
            Info::new(At::Operation, Part::Local(0), 0),
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
        wrong.storage.clear();
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
