//! A constructed context takes direct providers and copies spread entries.
//!
//! Its provider/spread pairs determine the row. The concrete row layout
//! lowers the copy packet once per final key. A selected direct entry
//! transfers its count; a shadowed direct entry uses its operand-release
//! packet. Spread contexts keep their owners. The new context has its own count.

use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn super::OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for (i, tag) in b.tags.iter().enumerate() {
        if *tag != Tag::ContextNew {
            continue;
        }
        let pairs = b.record(b.data[i][0]);
        if pairs.is_empty() {
            continue;
        }
        out.push((
            Tag::Retain,
            u32_of(i),
            Info::new(At::Operation, Part::Row, 0),
        ));
        for (k, pair) in pairs.chunks_exact(2).enumerate() {
            if cx.trait_value(hd_types::Ty(pair[0])) {
                out.push((
                    Tag::Release,
                    u32_of(i),
                    Info::new(At::Operation, Part::Operand(u32_of(k)), 2),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, TirBuilder, TirSink, Topology, count, verify_owned};
    use crate::own::{counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx};
    use hd_types::Ty;

    struct Providers;
    impl crate::own::OwnCx for Providers {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }
        fn trait_value(&self, ty: Ty) -> bool {
            ty == Ty(43)
        }
        fn counted(&self, ty: Ty) -> bool {
            Cx.counted(ty)
        }
        fn pass(&self, callee: &crate::ir::Callee, k: usize, n: usize) -> crate::own::Pass {
            Cx.pass(callee, k, n)
        }
        fn pass_value(&self, ty: Ty, k: usize, n: usize) -> crate::own::Pass {
            Cx.pass_value(ty, k, n)
        }
        fn may_write(&self, callee: &crate::ir::Callee) -> bool {
            Cx.may_write(callee)
        }
        fn parts(&self, ty: Ty, variant: Option<u32>, out: &mut Vec<Ty>) -> bool {
            Cx.parts(ty, variant, out)
        }
        fn received(&self, item: DefId, k: usize, n: usize) -> crate::own::Pass {
            Cx.received(item, k, n)
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

    #[test]
    fn direct_entries_are_consumed_and_have_conditional_release_packets() {
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = builder.open_block();
        let fields = builder.words_record(&[]);
        let direct = builder.emit(
            Tag::NewStruct,
            crate::ir::NONE,
            fields,
            Ty(43),
            NodeIdx::NONE,
        );
        let spread = builder.emit(
            Tag::NewStruct,
            crate::ir::NONE,
            fields,
            Ty(44),
            NodeIdx::NONE,
        );
        let pairs = builder.words_record(&[Ty(43).0, direct.0, Ty(44).0, spread.0]);
        let context = builder
            .emit(Tag::ContextNew, pairs, 0, Ty(45), NodeIdx::NONE)
            .0 as usize;
        let root = builder.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut b = builder.finish(root, &[]).unwrap();
        let mut ops = Vec::new();
        crate::own::operands(&b, context, &mut ops);
        assert_eq!(ops.len(), 2);
        assert!(crate::own::consumes(
            &b,
            &Providers,
            context,
            &ops[0],
            crate::ir::NONE
        ));
        assert!(!crate::own::consumes(
            &b,
            &Providers,
            context,
            &ops[1],
            crate::ir::NONE
        ));
        let topo = Topology::new(&b);
        statements::run(&mut b, &Providers, &topo, &mut splice::Tables::default()).unwrap();
        assert!(verify_owned(&b, &Providers).is_empty());
        let end = b
            .tags
            .iter()
            .enumerate()
            .position(|(i, &tag)| {
                tag == Tag::Release
                    && Info::read(&b, i).is_some_and(|info| info.part == Part::Operand(0))
            })
            .unwrap();
        b.tags[end] = Tag::Retain;
        assert!(!verify_owned(&b, &Providers).is_empty());
    }

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
        assert!(
            !super::counts(&wrong, &Cx)
                .iter()
                .any(|c| c.1 == u32_of(target))
        );
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
