//! Counts made explicit after an intrinsic constructs borrowed buffer bits.
//! The result is an owned value; its existing drops still end that owner.

use super::{OwnCx, splice};
use crate::ir::{Body, Callee, NONE, Tag, Topology, u32_of};
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
            return Err("a retained intrinsic result outside a block".into());
        }
        let position = topo.pos[i] + 1;
        let members = b.record(b.data[block as usize][0]);
        if members.get(position as usize).is_some_and(|&next| {
            b.tags[next as usize] == Tag::Retain && b.data[next as usize][0] == u32_of(i)
        }) {
            continue;
        }
        // Stored: the new string result keeps a reference to the buffer
        // the intrinsic borrowed. Its empty form points at static storage.
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
    b.tags.get(i) == Some(&Tag::Call)
        && b.ty[i] == Ty::STRING
        && Callee::from_words(b.record(b.data[i][0])).is_some_and(|c| cx.retains_result(&c))
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
                .any(|e| e.what.contains("buffer-result proof"))
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
