//! ABI-owned parameter cleanup stated after ownership has decided the body.
//!
//! A release descriptor declares cleanup at the common function exit, after its
//! ordinary exit drops and loan cleanup. Its lexical position emits nothing.
//! It releases only when the resolved ABI supplies an owned argument; an
//! instance receiving it borrowed has no count to release.

use super::{OwnCx, Pass, params};
use crate::ir::{Body, BodyKind};

/// The root parameters whose declarations may supply a count the body
/// itself does not own. Closure and suspension ABIs are separate kinds.
pub(crate) fn parameters(b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
    if !matches!(
        b.kind,
        BodyKind::Fn | BodyKind::DeriveInstance | BodyKind::Default
    ) || b.kind != BodyKind::Default && cx.suspending(b.item)
    {
        return Vec::new();
    }
    let params = params(b);
    if cx.parameter_count(b.item) != Some(params.len()) {
        return Vec::new();
    }
    params
        .iter()
        .enumerate()
        .filter_map(|(k, &l)| {
            (cx.counted(b.local_ty[l as usize])
                && cx.received(b.item, k, params.len()) == Pass::Owned
                && !b
                    .exit_drops
                    .iter()
                    .any(|&(sub, local)| sub.raw() == 0 && local.raw() == l))
            .then_some(l)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::reason::LAST_USE;
    use super::super::{counts::run, splice};
    use super::*;
    use crate::ir::{Callee, Providers, TirBuilder, TirSink, local_flags, verify_owned};
    use crate::ir::{Tag, Topology};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;
    use hd_types::TyList;

    struct Facts;
    impl OwnCx for Facts {
        fn parameter_count(&self, _: DefId) -> Option<usize> {
            Some(1)
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

    fn body() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        let param = b.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, syn);
        let value = b.get(param, Ty::STRING, syn);
        b.call(
            &Callee::Item {
                def: DefId::from_raw(7),
                targs: TyList::EMPTY,
            },
            &[value],
            Providers::None,
            Ty::VOID,
            syn,
        );
        let root = b.close_block(block, None, Ty::VOID, syn);
        let mut b = b.finish(root, &[]).unwrap();
        b.exit_drops.clear();
        // The input is the ownership decision: this parameter's reads
        // borrow and it is outside the ordinary exit set.
        for i in 0..b.len() {
            if b.tags[i] == Tag::Read {
                b.data[i][1] = crate::ir::ReadMode::Borrow as u32;
            }
        }
        let topo = Topology::new(&b);
        run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        b
    }

    #[test]
    fn the_abi_cleanup_is_stated_once_without_reowning_the_body_parameter() {
        let mut b = body();
        assert!(b.exit_drops.is_empty());
        assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Release).count(), 1);
        assert!(verify_owned(&b, &Facts).is_empty());
        let topo = Topology::new(&b);
        run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Release).count(), 1);
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn malformed_records_and_a_release_at_a_value_are_rejected() {
        let mut b = body();
        let exit = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        let record = b.data[exit][1] as usize;
        b.extra[record] = 3;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("malformed count record"))
        );
        b.extra[record] = 4;
        b.extra[record + 1] = crate::ir::count::At::Value as u32;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("operation fact"))
        );
    }

    #[test]
    fn an_absent_or_mislabeled_abi_cleanup_fails_verification() {
        let mut b = body();
        let exit = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        b.extra[b.data[exit][1] as usize + 4] = 0;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("count fact"))
        );
        b.extra[b.data[exit][1] as usize + 4] = LAST_USE;
        let root = b.sub_root[0] as usize;
        let kept: Vec<u32> = b
            .record(b.data[root][0])
            .iter()
            .copied()
            .filter(|&i| i as usize != exit)
            .collect();
        let record = u32::try_from(b.extra.len()).unwrap();
        b.extra.push(u32::try_from(kept.len()).unwrap());
        b.extra.extend(kept);
        b.data[root][0] = record;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("boundary count"))
        );
    }
}
