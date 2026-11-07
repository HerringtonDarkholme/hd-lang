//! The node a fixed-arity join owns from its fresh path until its end.
//! Ready, cancellation and frame teardown end the same owner, once.

use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body) -> impl Iterator<Item = (Tag, u32, Info)> + '_ {
    b.tags.iter().enumerate().filter_map(|(i, &tag)| {
        (tag == Tag::AwaitAll).then_some((
            Tag::Release,
            u32_of(i),
            Info::new(At::Operation, Part::Waker, 2),
        ))
    })
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
        for _ in 0..2 {
            let children = b.refs_record(&[]);
            b.emit(Tag::AwaitAll, NONE, children, Ty::VOID, NodeIdx::NONE);
        }
        b.call(
            &Callee::Item {
                def: DefId::from_raw(7),
                targs: TyList::EMPTY,
            },
            &[],
            Providers::None,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn stated() -> Body {
        let mut b = body();
        let topo = Topology::new(&b);
        counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
        b
    }

    #[test]
    fn each_join_states_its_waker_owner_once_even_for_an_uncounted_result() {
        let mut b = body();
        for _ in 0..2 {
            let topo = Topology::new(&b);
            counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
            let topo = Topology::new(&b);
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Release).count(), 2);
            assert!(!b.tags.contains(&Tag::Retain));
            for (i, &tag) in b.tags.iter().enumerate() {
                if !matches!(tag, Tag::AwaitAll | Tag::Call) {
                    continue;
                }
                let actual = crate::ir::count::following(&b, &topo, u32_of(i), Tag::Release);
                assert_eq!(
                    actual,
                    (tag == Tag::AwaitAll).then_some(Info::new(At::Operation, Part::Waker, 2))
                );
            }
            let errors = verify_owned(&b, &Cx);
            assert!(errors.is_empty(), "{errors:?}");
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn a_waker_is_not_the_child_result_or_a_siblings_node() {
        let b = stated();
        let release = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        for (slot, wrong) in [(1, At::Cold as u32), (2, 4), (3, 1), (4, 0)] {
            let mut wrong_body = b.clone();
            let record = wrong_body.data[release][1] as usize;
            wrong_body.extra[record + slot] = wrong;
            assert!(!verify_owned(&wrong_body, &Cx).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[release] = Tag::Retain;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        let sibling = b.tags.iter().rposition(|&t| t == Tag::AwaitAll).unwrap();
        wrong.data[release][0] = u32_of(sibling);
        assert!(!verify_owned(&wrong, &Cx).is_empty());
    }

    #[test]
    fn a_missing_duplicate_or_nonadjacent_waker_contract_is_rejected() {
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
