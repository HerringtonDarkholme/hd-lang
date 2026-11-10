//! The two owned copies of a persistent Fact/Handle result cache.
//!
//! A hit copies the cache's value for its caller. A fresh evaluation owns
//! its result already and copies it into the cache before returning.
//! Declarations execute at those cache boundaries, not lexically.

use super::OwnCx;
use crate::ir::{Body, BodyKind};

/// The kind and result type determine whether the cache owns counted bits.
pub(crate) fn counted(b: &Body, cx: &dyn OwnCx) -> bool {
    matches!(b.kind, BodyKind::Fact | BodyKind::Handle) && cx.counted(b.ty[b.sub_root[0] as usize])
}

#[cfg(test)]
mod tests {
    use super::super::Pass;
    use super::super::{counts::run, splice};
    use super::*;
    use crate::ir::{Callee, ReadMode, TirBuilder, TirSink, local_flags, verify_owned};
    use crate::ir::{Tag, Topology};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts;
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn suspending(&self, _: DefId) -> bool {
            false
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

    fn body(kind: BodyKind, ty: Ty) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), kind);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        let p = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, syn);
        let v = b.get(p, ty, syn);
        let root = b.close_block(block, Some(v), ty, syn);
        let mut b = b.finish(root, &[]).unwrap();
        b.exit_drops.clear();
        let read = b.tags.iter().position(|&t| t == Tag::Read).unwrap();
        b.data[read][1] = ReadMode::Borrow as u32;
        b
    }

    #[test]
    fn counted_caches_declare_both_copies_once() {
        for (kind, ty, count) in [
            (BodyKind::Fact, Ty::STRING, 2),
            (BodyKind::Handle, Ty::STRING, 2),
            (BodyKind::Fn, Ty::STRING, 0),
            (BodyKind::Handle, Ty::I32, 0),
        ] {
            let mut b = body(kind, ty);
            for _ in 0..2 {
                let topo = Topology::new(&b);
                run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
                assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), count);
                let errors = verify_owned(&b, &Facts);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn a_cache_copy_needs_its_kind_phase_and_reason_proof() {
        let mut b = body(BodyKind::Handle, Ty::STRING);
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("boundary count"))
        );
        let topo = Topology::new(&b);
        run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        let copy = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        b.extra[b.data[copy][1] as usize + 4] = 2;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("count fact"))
        );
        b.extra[b.data[copy][1] as usize + 4] = 0;
        b.kind = BodyKind::Fn;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("count fact"))
        );
    }
}
