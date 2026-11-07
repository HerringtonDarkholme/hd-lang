//! Declared parameters copied into their cold suspension frame.
//!
//! A concrete `Drop` argument arrives owned instead; its physical ABI
//! transfers that count and the declaration lowers no copy.

use super::{OwnCx, Tri, params};
use crate::ir::{Body, BodyKind};

pub(crate) fn parameters(b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
    if !matches!(b.kind, BodyKind::Fn | BodyKind::DeriveInstance) || !cx.suspending(b.item) {
        return Vec::new();
    }
    let params = params(b);
    if cx.parameter_count(b.item) != Some(params.len()) {
        return Vec::new();
    }
    params
        .into_iter()
        .filter(|&l| {
            let ty = b.local_ty[l as usize];
            cx.counted(ty) && cx.holds_drop(ty) != Tri::Yes
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::Pass;
    use super::super::{counts::run, splice};
    use super::*;
    use crate::ir::{Callee, TirBuilder, TirSink, local_flags, verify_owned};
    use crate::ir::{Tag, Topology};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts {
        suspends: bool,
        drop: Tri,
        declared: bool,
    }
    impl OwnCx for Facts {
        fn parameter_count(&self, _: DefId) -> Option<usize> {
            self.declared.then_some(1)
        }
        fn suspending(&self, _: DefId) -> bool {
            self.suspends
        }
        fn holds_drop(&self, _: Ty) -> Tri {
            self.drop
        }
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
            Pass::Owned
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    }

    fn body(ty: Ty) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        b.local(ty, Symbol::from_raw(0), local_flags::PARAM, syn);
        let root = b.close_block(block, None, Ty::VOID, syn);
        let mut b = b.finish(root, &[]).unwrap();
        b.exit_drops.clear();
        b
    }

    #[test]
    fn copied_cold_arguments_are_declared_once_but_transferred_drop_arguments_are_not() {
        for (suspends, drop, declared, ty, count) in [
            (true, Tri::No, true, Ty::STRING, 1),
            (true, Tri::Maybe, true, Ty::STRING, 1),
            (true, Tri::Yes, true, Ty::STRING, 0),
            (false, Tri::No, true, Ty::STRING, 0),
            (true, Tri::No, false, Ty::STRING, 0),
            (true, Tri::No, true, Ty::I32, 0),
        ] {
            let facts = Facts {
                suspends,
                drop,
                declared,
            };
            let mut b = body(ty);
            let topo = Topology::new(&b);
            super::super::counts::run(&mut b, &facts, &topo, &mut splice::Tables::default())
                .unwrap();
            for _ in 0..2 {
                let topo = Topology::new(&b);
                run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
                assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), count);
                let errors = verify_owned(&b, &facts);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn a_cold_copy_needs_a_copied_parameter_and_stored_reason() {
        let facts = Facts {
            suspends: true,
            drop: Tri::No,
            declared: true,
        };
        let mut b = body(Ty::STRING);
        assert!(
            verify_owned(&b, &facts)
                .iter()
                .any(|e| e.what.contains("boundary count"))
        );
        let topo = Topology::new(&b);
        run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
        let copy = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        b.extra[b.data[copy][1] as usize + 4] = 2;
        assert!(
            verify_owned(&b, &facts)
                .iter()
                .any(|e| e.what.contains("count fact"))
        );
        b.extra[b.data[copy][1] as usize + 4] = 0;
        let moved = Facts {
            drop: Tri::Yes,
            ..facts
        };
        assert!(
            verify_owned(&b, &moved)
                .iter()
                .any(|e| e.what.contains("count fact"))
        );
    }
}
