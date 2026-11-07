//! A waiting parent's count of its child frame, separate from the result.
//!
//! A direct/dynamic `Await` transfers its child's initial count. `AwaitValue`
//! borrows a stored suspension, so the parent takes its own count on Pending.
//! Completion, cancellation and shield exits end that same owner once.

use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if matches!(tag, Tag::Await | Tag::AwaitValue) {
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, NONE, Providers, TirBuilder, TirSink, Topology, verify_owned,
    };
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
        b.emit(Tag::AwaitValue, suspension.0, NONE, Ty::I32, NodeIdx::NONE);
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
                .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Child))
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
        let release = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
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
        for mode in 0..3 {
            let mut b = stated();
            let topo = Topology::new(&b);
            let release = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
            let point = b.data[release][0] as usize;
            let mut edits = splice::Tables::default();
            if mode == 0 {
                edits.remove(u32_of(release));
            } else if mode == 1 {
                edits.insert(
                    topo.blk[point],
                    topo.pos[point] + 1,
                    Tag::Release,
                    b.data[release],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            } else {
                let call = b.tags.iter().position(|&t| t == Tag::Call).unwrap();
                edits.insert(
                    topo.blk[point],
                    topo.pos[point] + 1,
                    Tag::Call,
                    b.data[call],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            }
            splice::apply(&mut b, &topo, &mut edits).unwrap();
            let errors = verify_owned(&b, &Cx);
            assert!(
                errors.iter().any(|e| e.what.contains("absent or repeated")),
                "{mode}: {errors:?}"
            );
        }
    }
}
