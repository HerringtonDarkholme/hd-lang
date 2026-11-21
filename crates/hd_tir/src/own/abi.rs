//! ABI-owned parameter cleanup stated after ownership has decided the body.
//!
//! A release descriptor declares cleanup at the common function exit, after its
//! ordinary exit drops and loan cleanup. Its lexical position emits nothing.
//! It releases only when the resolved ABI supplies an owned argument; an
//! instance receiving it borrowed has no count to release.

use super::{OwnCx, Pass, params};
use crate::ir::{Body, BodyKind};

/// The root parameters whose declarations may supply a count the body
/// itself does not own. Closure and suspension ABIs are separate kinds; a
/// default has no item signature, so its parameter locals are its own.
pub(crate) fn parameters(b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
    if !matches!(
        b.kind,
        BodyKind::Fn | BodyKind::DeriveInstance | BodyKind::Default
    ) || b.kind != BodyKind::Default && cx.suspending(b.item)
    {
        return Vec::new();
    }
    let params = params(b);
    if b.kind != BodyKind::Default && cx.parameter_count(b.item) != Some(params.len()) {
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
    crate::own::test_cx::own_cx!(Facts, [temporary_owner_name, parts, received], {
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

        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    });

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

    /// `body`'s parameter, then a `return` in an `If` arm: two ways out.
    /// Its exit release is stated at the root's start, on both.
    fn two_ways() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        b.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, syn);
        let yes = b.const_value(Ty::BOOL, 1);
        let then = b.open_block();
        b.emit(
            Tag::Return,
            crate::ir::NONE,
            crate::ir::NONE,
            Ty::NEVER,
            syn,
        );
        let then = b.close_block(then, None, Ty::NEVER, syn);
        let other = b.open_block();
        let other = b.close_block(other, None, Ty::VOID, syn);
        let arms = b.refs_record(&[then, other]);
        b.emit(Tag::If, yes.0, arms, Ty::VOID, syn);
        let root = b.close_block(block, None, Ty::VOID, syn);
        let mut b = b.finish(root, &[]).unwrap();
        b.exit_drops.clear();
        let topo = Topology::new(&b);
        run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        b
    }

    /// `two_ways` with its exit release moved: before the `return` when
    /// `at_return`, at the root's end when `at_end`, and left at the
    /// root's start when `keep`.
    fn moved(at_return: bool, at_end: bool, keep: bool) -> Body {
        let mut b = two_ways();
        let topo = Topology::new(&b);
        let exit = b.tags.iter().position(|&t| t == Tag::Release).unwrap();
        let ret = b.tags.iter().position(|&t| t == Tag::Return).unwrap();
        let root = b.sub_root[0];
        let mut edits = splice::Tables::default();
        let data = b.data[exit];
        if at_return {
            edits.insert(
                topo.blk[ret],
                topo.pos[ret],
                Tag::Release,
                data,
                Ty::VOID,
                NodeIdx::NONE,
            );
        }
        if at_end {
            let end = crate::ir::u32_of(b.record(b.data[root as usize][0]).len());
            edits.insert(root, end, Tag::Release, data, Ty::VOID, NodeIdx::NONE);
        }
        if !keep {
            edits.remove(crate::ir::u32_of(exit));
        }
        splice::apply(&mut b, &topo, &mut edits).unwrap();
        b
    }

    fn once_on_every_way_out(b: &Body) -> bool {
        !verify_owned(b, &Facts)
            .iter()
            .any(|e| e.what.contains("on a way out"))
    }

    /// An exit count runs once on every way out of its sub-body, wherever
    /// it stands (`own::scopes::exits` copies it to each way out).
    #[test]
    fn an_exit_release_runs_once_on_each_way_out() {
        assert!(once_on_every_way_out(&two_ways()));
        assert!(once_on_every_way_out(&moved(true, true, false)));
    }

    /// Missing on the `return`'s path, or run twice on it, is refused.
    #[test]
    fn an_exit_release_missing_or_doubled_on_one_way_out_is_refused() {
        assert!(!once_on_every_way_out(&moved(false, true, false)));
        assert!(!once_on_every_way_out(&moved(true, false, true)));
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
