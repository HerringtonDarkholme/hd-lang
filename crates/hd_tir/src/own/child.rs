//! A waiting parent's count of its child frame, separate from the result.
//!
//! A direct/dynamic `Await` transfers its child's initial count. `AwaitValue`
//! borrows a stored suspension, so the parent takes its own count on Pending.
//! Completion, cancellation and shield exits end that same owner once.
//! A frame that can wait on one child also states its teardown owner.
//! Join points keep their children in the waker, never in that frame slot.

use super::{OwnCx, cold};
use crate::ir::{
    Body, Tag, Topology,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx, topo: &Topology) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    let mut child = vec![false; b.sub_root.len()];
    for (i, &tag) in b.tags.iter().enumerate() {
        if matches!(tag, Tag::Await | Tag::AwaitValue) {
            child[topo.sub[i] as usize] = true;
            out.push((
                Tag::Release,
                u32_of(i),
                Info::new(At::Operation, Part::Child, 2),
            ));
            if tag == Tag::AwaitValue {
                out.push((
                    Tag::Retain,
                    u32_of(i),
                    Info::new(At::Operation, Part::Child, 0),
                ));
            }
        }
    }
    // Only these points write a nonzero F_CHILD. AwaitAll suspends with
    // zero and owns its children through the separate waker contract.
    for sub in cold::sub_bodies(b, cx) {
        if child[sub as usize] {
            out.push((
                Tag::Release,
                b.sub_root[sub as usize],
                Info::new(At::FrameDrop, Part::Child, 2),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, NONE, Providers, TirBuilder, TirSink, verify_owned};
    use crate::own::{counts, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx};
    use hd_types::{Ty, TyList};

    fn body() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        let callee = Callee::Item {
            def: DefId::from_raw(7),
            targs: TyList::EMPTY,
        };
        b.call(&callee, &[], Providers::None, Ty::I32, NodeIdx::NONE);
        let suspension = b.const_value(Ty(42), 0);
        let args = b.call_args_record(&[], crate::ir::Providers::None);
        b.emit(Tag::AwaitValue, suspension.0, args, Ty::I32, NodeIdx::NONE);
        let children = b.refs_record(&[]);
        b.emit(Tag::AwaitAll, NONE, children, Ty::VOID, NodeIdx::NONE);
        b.call(&callee, &[], Providers::None, Ty::VOID, NodeIdx::NONE);
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        let first = b.tags.iter().position(|&t| t == Tag::Call).unwrap();
        b.tags[first] = Tag::Await;
        b
    }

    fn stated() -> Body {
        let mut b = body();
        let topo = Topology::new(&b);
        counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
        b
    }

    #[test]
    fn each_point_states_the_child_owner_separately_from_its_result() {
        let mut b = body();
        for _ in 0..2 {
            let topo = Topology::new(&b);
            counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
            let topo = Topology::new(&b);
            let actual = (0..b.len())
                .filter(|&i| {
                    Info::read(&b, i)
                        .is_some_and(|c| c.at == At::Operation && c.part == Part::Child)
                })
                .count();
            assert_eq!(actual, 3);
            for (i, &tag) in b.tags.iter().enumerate() {
                if !matches!(
                    tag,
                    Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::Call
                ) {
                    continue;
                }
                let release = crate::ir::count::following(&b, &topo, u32_of(i), Tag::Release)
                    .filter(|c| c.part == Part::Child);
                let retain = crate::ir::count::following(&b, &topo, u32_of(i), Tag::Retain)
                    .filter(|c| c.part == Part::Child);
                assert_eq!(
                    release,
                    matches!(tag, Tag::Await | Tag::AwaitValue).then_some(Info::new(
                        At::Operation,
                        Part::Child,
                        2
                    ))
                );
                assert_eq!(
                    retain,
                    (tag == Tag::AwaitValue).then_some(Info::new(At::Operation, Part::Child, 0))
                );
            }
            let errors = verify_owned(&b, &Cx);
            assert!(errors.is_empty(), "{errors:?}");
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn child_contracts_need_their_point_action_phase_subject_and_reason() {
        let b = stated();
        let release = (0..b.len())
            .find(|&i| {
                b.tags[i] == Tag::Release
                    && Info::read(&b, i)
                        .is_some_and(|c| c.at == At::Operation && c.part == Part::Child)
            })
            .unwrap();
        for (slot, wrong) in [(1, At::Cold as u32), (2, 0), (3, 1), (4, 0)] {
            let mut wrong_body = b.clone();
            let record = wrong_body.data[release][1] as usize;
            wrong_body.extra[record + slot] = wrong;
            assert!(!verify_owned(&wrong_body, &Cx).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[release] = Tag::Retain;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        wrong.data[release][0] = u32_of(b.tags.iter().position(|&t| t == Tag::AwaitValue).unwrap());
        assert!(!verify_owned(&wrong, &Cx).is_empty());
    }

    #[test]
    fn missing_duplicate_and_separated_bundles_are_rejected() {
        let b = stated();
        let release = (0..b.len())
            .find(|&i| {
                b.tags[i] == Tag::Release
                    && Info::read(&b, i)
                        .is_some_and(|c| c.at == At::Operation && c.part == Part::Child)
            })
            .unwrap();
        crate::own::tests::assert_bundle_adjacent(&b, release);
    }

    fn frame_release(b: &Body, sub: usize) -> Option<usize> {
        (0..b.len()).find(|&i| {
            b.tags[i] == Tag::Release
                && b.data[i][0] == b.sub_root[sub]
                && Info::read(b, i) == Some(Info::new(At::FrameDrop, Part::Child, 2))
        })
    }

    #[test]
    fn teardown_is_stated_once_and_cannot_use_the_point_or_exit_contract() {
        let mut b = body();
        for _ in 0..2 {
            let topo = Topology::new(&b);
            counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
            assert!(frame_release(&b, 0).is_some());
            assert!(verify_owned(&b, &Cx).is_empty());
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        let release = frame_release(&b, 0).unwrap();
        for info in [
            Info::new(At::Operation, Part::Child, 2),
            Info::new(At::Exit, Part::Child, 2),
            Info::new(At::Cold, Part::Child, 2),
            Info::new(At::FrameDrop, Part::Row, 2),
            Info::new(At::FrameDrop, Part::Child, 0),
        ] {
            let mut wrong = b.clone();
            wrong.data[release][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Cx).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[release] = Tag::Retain;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        for duplicate in [false, true] {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            if duplicate {
                edits.insert(
                    wrong.sub_root[0],
                    0,
                    Tag::Release,
                    wrong.data[release],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            } else {
                edits.remove(u32_of(release));
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(
                verify_owned(&wrong, &Cx)
                    .iter()
                    .any(|e| e.what.contains("absent or repeated"))
            );
        }
    }

    struct Frames(bool);
    crate::own::test_cx::own_cx!(
        Frames,
        [
            temporary_owner_name,
            counted,
            pass,
            pass_value,
            may_write,
            parts,
            received,
            holds_coll
        ],
        {
            fn suspending(&self, _: DefId) -> bool {
                self.0
            }
            fn function_suspends(&self, ty: Ty) -> bool {
                ty == Ty(43)
            }
        }
    );

    #[test]
    fn no_child_owner_for_join_only_no_point_or_nonsuspending_bodies() {
        for join in [false, true] {
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let root = b.open_block();
            if join {
                let children = b.refs_record(&[]);
                b.emit(Tag::AwaitAll, NONE, children, Ty::VOID, NodeIdx::NONE);
            }
            let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
            let mut b = b.finish(root, &[]).unwrap();
            let topo = Topology::new(&b);
            counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
            assert!(frame_release(&b, 0).is_none());
            assert!(verify_owned(&b, &Cx).is_empty());
        }
        let mut b = body();
        let topo = Topology::new(&b);
        counts::run(
            &mut b,
            &Frames(false),
            &topo,
            &mut splice::Tables::default(),
        )
        .unwrap();
        assert!(frame_release(&b, 0).is_none());
        assert!(verify_owned(&b, &Frames(false)).is_empty());
    }

    #[test]
    fn cleanup_waits_belong_to_their_closure_not_its_creator_or_sibling() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        for awaits in [true, false] {
            let sub = b.open_sub(&[]);
            let inner = b.open_block();
            if awaits {
                let scope = b.open_scope();
                let inside = b.open_block();
                let cleanup = b.open_block();
                let child = b.const_value(Ty(42), 0);
                let args = b.call_args_record(&[], crate::ir::Providers::None);
                b.emit(Tag::AwaitValue, child.0, args, Ty::VOID, NodeIdx::NONE);
                let cleanup = b.close_block(cleanup, None, Ty::VOID, NodeIdx::NONE);
                b.defer(cleanup, NodeIdx::NONE);
                let inside = b.close_block(inside, None, Ty::VOID, NodeIdx::NONE);
                b.close_scope(scope, inside, Ty::VOID, NodeIdx::NONE);
            }
            let root = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
            b.close_sub(sub, root, Ty(43), NodeIdx::NONE);
        }
        let root = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        for _ in 0..2 {
            let topo = Topology::new(&b);
            counts::run(&mut b, &Frames(true), &topo, &mut splice::Tables::default()).unwrap();
            assert!(frame_release(&b, 0).is_none());
            assert!(frame_release(&b, 1).is_some());
            assert!(frame_release(&b, 2).is_none());
            let errors = verify_owned(&b, &Frames(true));
            assert!(errors.is_empty(), "{errors:?}");
        }
        let release = frame_release(&b, 1).unwrap();
        for target in [0, 2] {
            let mut wrong = b.clone();
            wrong.data[release][0] = wrong.sub_root[target];
            let errors = verify_owned(&wrong, &Frames(true));
            assert!(
                errors
                    .iter()
                    .any(|e| e.what.contains("without its count fact"))
            );
            assert!(errors.iter().any(|e| e.what.contains("absent or repeated")));
        }
    }
}
