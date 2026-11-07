//! Counts made explicit after a result copies borrowed bits.
//! The result is an owned value; its existing drops still end that owner.

use super::{OwnCx, splice};
use crate::ir::{Body, Callee, Coercion, NONE, Ref, Tag, Topology, u32_of};
use hd_types::Ty;

pub(super) fn run(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    for i in 0..b.len() {
        if !result(b, cx, i) {
            continue;
        }
        let block = topo.blk[i];
        if block == NONE {
            return Err("a retained borrowed result outside a block".into());
        }
        let position = topo.pos[i] + 1;
        let members = b.record(b.data[block as usize][0]);
        if members.get(position as usize).is_some_and(|&next| {
            b.tags[next as usize] == Tag::Retain && b.data[next as usize][0] == u32_of(i)
        }) {
            continue;
        }
        // Stored: the result keeps a reference to the borrowed source.
        // A string slice's empty form points at static storage.
        edits.insert(
            block,
            position,
            Tag::Retain,
            [u32_of(i), 0],
            Ty::VOID,
            b.syn[i],
        );
    }
    splice::apply(b, topo, edits)?;
    if !b.anchors.is_empty() {
        for i in 0..b.len() {
            if b.tags[i] == Tag::Retain {
                b.anchors[i] = b.anchors[b.data[i][0] as usize];
            }
        }
    }
    Ok(())
}

pub(crate) fn result(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    match b.tags.get(i) {
        Some(Tag::Call) => {
            b.ty[i] == Ty::STRING
                && Callee::from_words(b.record(b.data[i][0])).is_some_and(|c| cx.retains_result(&c))
        }
        Some(Tag::Coerce) if cx.counted(b.ty[i]) => {
            let kind = b.record(b.data[i][1]).first().copied();
            if kind == Some(Coercion::RefAsProvider as u32) {
                return true;
            }
            if ![
                Coercion::ToTraitValue,
                Coercion::Supertrait,
                Coercion::RefToTraitValue,
            ]
            .iter()
            .any(|c| kind == Some(*c as u32))
            {
                return false;
            }
            let value = Ref(b.data[i][0]);
            let source = match value.as_inst() {
                Some(inst) => b.ty.get(inst.0 as usize).copied().unwrap_or(Ty::POISON),
                None => b
                    .consts
                    .get((value.0 & !Ref::CONST_BIT) as usize)
                    .map_or(Ty::POISON, |c| c.0),
            };
            // A materialized trait value already has its data words;
            // widening only selects a different vtable path.
            if cx.trait_value(source) {
                return true;
            }
            let mut target = source;
            while let Some(inner) = cx.reference_target(target) {
                if cx.trait_value(inner) {
                    // Widening through references keeps the trait's data,
                    // including when reaching it needs loads first. Erasing
                    // the reference's identity into a box is a separate kind.
                    return cx.trait_reference_widens(target, b.ty[i]);
                }
                target = inner;
            }
            cx.single_counted_address(source)
        }
        // Lookup copies the caller-held provider into a result of its own.
        Some(Tag::ProviderGet) => cx.counted(b.ty[i]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Providers, TirBuilder, TirSink, verify_owned};
    use crate::own::{OwnScratch, Pass, consumers};
    use hd_base::{DefId, NodeIdx};
    use hd_types::TyList;

    struct Facts;

    impl OwnCx for Facts {
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty == Ty(43)
        }
        fn single_counted_address(&self, ty: Ty) -> bool {
            matches!(ty, Ty(42 | 44))
        }
        fn reference_target(&self, ty: Ty) -> Option<Ty> {
            match ty {
                Ty(44 | 46) => Some(Ty(43)),
                Ty(45) => Some(Ty(46)),
                _ => None,
            }
        }
        fn trait_reference_widens(&self, from: Ty, to: Ty) -> bool {
            from == Ty(46) && to == Ty(43)
        }
        fn trait_value(&self, ty: Ty) -> bool {
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
        fn retains_result(&self, c: &Callee) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(7))
        }
    }

    fn body() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        let text = b.const_str("abc");
        let zero = b.const_value(Ty::I32, 0);
        let value = b.call(
            &Callee::Item {
                def: DefId::from_raw(7),
                targs: TyList::EMPTY,
            },
            &[text, zero, zero],
            Providers::None,
            Ty::STRING,
            syn,
        );
        let root = b.close_block(block, Some(value), Ty::STRING, syn);
        b.finish(root, &[]).expect("slice body")
    }

    #[test]
    fn direct_trait_results_count_but_fresh_boxes_and_indirect_loads_do_not() {
        for (kind, source, counted) in [
            (Coercion::ToTraitValue, Ty(42), true),
            (Coercion::RefToTraitValue, Ty(42), true),
            (Coercion::Supertrait, Ty(43), true),
            (Coercion::ToTraitValue, Ty(43), true),
            (Coercion::ToTraitValue, Ty(44), false),
            (Coercion::RefToTraitValue, Ty(46), true),
            (Coercion::RefToTraitValue, Ty(45), true),
            (Coercion::ToTraitValue, Ty::I32, false),
            (Coercion::RefAsProvider, Ty(43), true),
        ] {
            let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let block = builder.open_block();
            let value = builder.const_value(source, 0);
            let value = builder.coerce(kind, NONE, value, Ty(43), NodeIdx::NONE);
            let root = builder.close_block(block, Some(value), Ty(43), NodeIdx::NONE);
            let mut body = builder.finish(root, &[]).unwrap();
            let topo = Topology::new(&body);
            run(&mut body, &Facts, &topo, &mut splice::Tables::default()).unwrap();
            assert_eq!(
                body.tags.contains(&Tag::Retain),
                counted,
                "{kind:?} {source:?}"
            );
            assert!(verify_owned(&body, &Facts).is_empty());
        }
    }

    #[test]
    fn the_count_statement_preserves_the_results_single_consumer() {
        let mut b = body();
        let mut out = Vec::new();
        consumers(&b, &Topology::new(&b), &Facts, &mut out);
        let call = b.tags.iter().position(|&t| t == Tag::Call).unwrap();
        assert_eq!(out[call], b.sub_root[0]);
        crate::own::run(&mut b, &Facts, &mut OwnScratch::default()).unwrap();
        consumers(&b, &Topology::new(&b), &Facts, &mut out);
        let call = b.tags.iter().position(|&t| t == Tag::Call).unwrap();
        assert_eq!(out[call], b.sub_root[0]);
        assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 1);
        let topo = Topology::new(&b);
        run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 1);
        assert!(verify_owned(&b, &Facts).is_empty());
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn the_verifier_rejects_a_missing_or_mislabeled_result_count() {
        let mut b = body();
        crate::own::run(&mut b, &Facts, &mut OwnScratch::default()).unwrap();
        let retain = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        b.data[retain][1] = 2;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("borrowed-result proof"))
        );
        b.data[retain][1] = 0;
        let root = b.sub_root[0] as usize;
        let list = b.data[root][0] as usize;
        assert_eq!(b.extra[list + b.extra[list] as usize], u32_of(retain));
        b.extra[list] -= 1;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("lacks its stored retain"))
        );
    }
}
