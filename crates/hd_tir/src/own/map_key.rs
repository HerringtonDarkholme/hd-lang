//! The key count taken by a map put, after its hash and probe.
//!
//! This declaration names the key-taking boundary of its `MapSet`. A
//! lowering that only replaces an already found entry's value enters
//! no such boundary and therefore takes no key count. Each declaration
//! sits at its sub-body root and names its operation by identity.

use super::{OwnCx, splice};
use crate::ir::{Body, IntrinsicOp, NONE, Ref, Tag, Topology, u32_of};
use hd_types::Ty;

pub(crate) fn needed(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    if b.tags.get(i) != Some(&Tag::Intrinsic)
        || IntrinsicOp::from_u32(b.data[i][0]) != Some(IntrinsicOp::MapSet)
    {
        return false;
    }
    let args = b.record(b.data[i][1]);
    if args.len() != 3 {
        return false;
    }
    let key = Ref(args[1]);
    let ty = match key.as_inst() {
        Some(key) => b.ty.get(key.0 as usize).copied(),
        None => b
            .consts
            .get((key.0 & !Ref::CONST_BIT) as usize)
            .map(|c| c.0),
    };
    ty.is_some_and(|ty| cx.counted(ty))
}

pub(super) fn run(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    for i in 0..b.len() {
        if !needed(b, cx, i) {
            continue;
        }
        let block = topo
            .sub
            .get(i)
            .and_then(|&sub| b.sub_root.get(sub as usize))
            .copied()
            .filter(|&root| root != NONE)
            .ok_or("a map key copy outside a sub-body")?;
        if b.record(b.data[block as usize][0]).iter().any(|&next| {
            b.tags[next as usize] == Tag::MapKeyCopy && b.data[next as usize][0] == u32_of(i)
        }) {
            continue;
        }
        edits.insert(
            block,
            0,
            Tag::MapKeyCopy,
            [u32_of(i), 0],
            Ty::VOID,
            b.syn[i],
        );
    }
    splice::apply(b, topo, edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, TirBuilder, TirSink, verify_owned};
    use crate::own::Pass;
    use hd_base::{DefId, NodeIdx};

    struct Facts;
    impl OwnCx for Facts {
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty == Ty(78)
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
            Pass::Owned
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    }

    fn body(key_ty: Ty) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let map = b.const_value(Ty::I32, 0);
        let key = b.const_value(key_ty, 0);
        let value = b.const_value(Ty::I32, 1);
        let args = b.refs_record(&[map, key, value]);
        b.emit(
            Tag::Intrinsic,
            IntrinsicOp::MapSet as u32,
            args,
            Ty::VOID,
            NodeIdx::NONE,
        );
        let root = b.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    #[test]
    fn a_put_declares_only_counted_keys_without_a_new_data_use() {
        for (ty, count) in [(Ty::STRING, 1), (Ty(78), 1), (Ty::I32, 0)] {
            let mut b = body(ty);
            for _ in 0..2 {
                let topo = Topology::new(&b);
                run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
                assert_eq!(
                    b.tags.iter().filter(|&&t| t == Tag::MapKeyCopy).count(),
                    count
                );
                for (i, &tag) in b.tags.iter().enumerate() {
                    if tag == Tag::MapKeyCopy {
                        let mut uses = Vec::new();
                        crate::ir::slots(&b, i, |_, _, word| {
                            if matches!(word, crate::ir::Word::Value(_)) {
                                uses.push(word);
                            }
                        });
                        assert!(uses.is_empty());
                    }
                }
                let errors = verify_owned(&b, &Facts);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn a_key_copy_needs_its_put_and_stored_reason() {
        let mut b = body(Ty::STRING);
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("lacks its stored key copy"))
        );
        let topo = Topology::new(&b);
        run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        let copy = b.tags.iter().position(|&t| t == Tag::MapKeyCopy).unwrap();
        b.data[copy][1] = 2;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("root map-put proof"))
        );
        b.data[copy][1] = 0;
        let put = b.tags.iter().position(|&t| t == Tag::Intrinsic).unwrap();
        b.data[put][0] = IntrinsicOp::MapGet as u32;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("root map-put proof"))
        );
    }
}
