//! A constructed context keeps a copy of its row's provider data.
//!
//! Its provider/spread pairs determine the row. The concrete row layout
//! lowers the packet once per final key, so shadowed pairs and empty
//! spreads add no count. This is separate from the new context's own count.

use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body) -> Vec<(Tag, u32, Info)> {
    b.tags
        .iter()
        .enumerate()
        .filter(|&(i, tag)| *tag == Tag::ContextNew && !b.record(b.data[i][0]).is_empty())
        .map(|(i, _)| {
            (
                Tag::Retain,
                u32_of(i),
                Info::new(At::Operation, Part::Row, 0),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, TirBuilder, TirSink, Topology, count, verify_owned};
    use crate::own::{counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx};
    use hd_types::Ty;

    fn body(entries: usize) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let value = b.const_value(Ty(43), 0);
        let pairs: Vec<_> = (0..entries).flat_map(|_| [Ty(43).0, value.0]).collect();
        let record = b.words_record(&pairs);
        b.emit(Tag::ContextNew, record, 0, Ty(44), NodeIdx::NONE);
        let root = b.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn state(b: &mut Body) {
        statements::run(b, &Cx, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Cx);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_context_keeps_one_provider_packet_and_an_empty_context_keeps_none() {
        for entries in [0, 1, 2] {
            let mut b = body(entries);
            b.anchors = vec![17; b.len()];
            for _ in 0..2 {
                state(&mut b);
                let copies: Vec<_> = (0..b.len())
                    .filter(|&i| Info::read(&b, i) == Some(Info::new(At::Operation, Part::Row, 0)))
                    .collect();
                assert_eq!(copies.len(), usize::from(entries != 0));
                for i in copies {
                    let target = b.data[i][0];
                    assert_eq!(b.tags[i], Tag::Retain);
                    assert_eq!(b.tags[target as usize], Tag::ContextNew);
                    assert!(count::adjacent(&b, &Topology::new(&b), target, u32_of(i)));
                    assert_eq!(b.anchors[i], b.anchors[target as usize]);
                }
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn a_context_copy_needs_its_action_phase_row_reason_and_operation() {
        let mut b = body(1);
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        for info in [
            Info::new(At::Cold, Part::Row, 0),
            Info::new(At::Operation, Part::Captures, 0),
            Info::new(At::Operation, Part::Row, 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Cx).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[i] = Tag::Release;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        wrong.data[i][0] = wrong.sub_root[0];
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        // Removing the provider packet removes the fact that justified
        // the statement, without changing the context's instruction.
        let mut wrong = b.clone();
        let target = wrong.data[i][0] as usize;
        let record = wrong.data[target][0] as usize;
        wrong.extra[record] = 0;
        assert!(!super::counts(&wrong).iter().any(|c| c.1 == u32_of(target)));
        assert!(!verify_owned(&wrong, &Cx).is_empty());
    }

    #[test]
    fn a_missing_duplicate_or_nonadjacent_context_copy_is_rejected() {
        let mut b = body(1);
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        for mode in 0..3 {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            if mode != 1 {
                edits.remove(u32_of(i));
            }
            if mode != 0 {
                let target = wrong.data[i][0] as usize;
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
            assert!(!verify_owned(&wrong, &Cx).is_empty());
        }
    }
}
