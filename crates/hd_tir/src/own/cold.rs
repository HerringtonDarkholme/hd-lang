//! Explicit value parameters copied into a cold frame and its fresh body.
//!
//! A concrete `Drop` argument arrives owned instead; its physical ABI
//! transfers that count and the declaration lowers no copy.

use super::{OwnCx, Tri, params};
use crate::ir::{Body, Tag};

/// The ABI declaration survives optimization erasing a suspension point.
/// Generic instances of one closure share a single sub-body contract.
pub(super) fn sub_bodies(b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
    let mut seen = vec![false; b.sub_root.len()];
    seen[0] = cx.suspending(b.item);
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag == Tag::Closure
            && cx.function_suspends(b.ty[i])
            && let Some(seen) = seen
                .get_mut(b.data[i][0] as usize)
                .filter(|_| b.data[i][0] != 0)
        {
            *seen = true;
        }
    }
    seen.into_iter()
        .enumerate()
        .filter_map(|(sub, yes)| yes.then_some(crate::ir::u32_of(sub)))
        .collect()
}

/// The explicit parameters of this sub-body, excluding captured locals.
pub(super) fn locals(b: &Body, sub: u32) -> Vec<u32> {
    if sub == 0 {
        params(b)
    } else {
        let r = b.sub_params[sub as usize];
        b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize].to_vec()
    }
}

/// Each explicit value parameter belongs to its own body's boundary.
/// Capture locals and reference-object counts have separate contracts.
pub(crate) fn parameters(b: &Body, cx: &dyn OwnCx) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for sub in sub_bodies(b, cx) {
        let locals = locals(b, sub);
        out.extend(locals.into_iter().filter_map(|l| {
            let ty = b.local_ty[l as usize];
            (cx.counted(ty) && cx.holds_drop(ty) != Tri::Yes)
                .then_some((b.sub_root[sub as usize], l))
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::Pass;
    use super::super::{counts::run, splice};
    use super::*;
    use crate::ir::{BodyKind, Callee, TirBuilder, TirSink, local_flags, verify_owned};
    use crate::ir::{Tag, Topology};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts {
        suspends: bool,
        drop: Tri,
        declared: bool,
    }
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn parameter_count(&self, _: DefId) -> Option<usize> {
            self.declared.then_some(1)
        }
        fn suspending(&self, _: DefId) -> bool {
            self.suspends
        }
        fn function_suspends(&self, ty: Ty) -> bool {
            ty == Ty(43)
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
            (true, Tri::No, false, Ty::STRING, 1),
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
                for at in [crate::ir::count::At::Cold, crate::ir::count::At::Entry] {
                    assert_eq!(
                        (0..b.len())
                            .filter(|&i| b.tags[i] == Tag::Retain
                                && crate::ir::count::Info::read(&b, i).is_some_and(|c| c.at == at))
                            .count(),
                        count
                    );
                }
                let errors = verify_owned(&b, &facts);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn a_root_follows_its_suspension_abi_without_a_body_kind_or_signature_whitelist() {
        let facts = Facts {
            suspends: true,
            drop: Tri::No,
            declared: false,
        };
        for kind in [
            BodyKind::Fn,
            BodyKind::DeriveInstance,
            BodyKind::Default,
            BodyKind::TestCase,
            BodyKind::Init,
        ] {
            let mut b = body(Ty::STRING);
            b.kind = kind;
            let topo = Topology::new(&b);
            run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 2);
            assert!(verify_owned(&b, &facts).is_empty());
        }
    }

    fn closures() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::NONE;
        let outer = b.open_block();
        let captured = b.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, syn);
        for fn_ty in [Ty(43), Ty(43), Ty(44)] {
            let text = b.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, syn);
            let number = b.local(Ty::I32, Symbol::from_raw(0), local_flags::PARAM, syn);
            let sub = b.open_sub(&[text, number]);
            let inner = b.open_block();
            let root = b.close_block(inner, None, Ty::VOID, syn);
            b.capture(sub, captured);
            b.close_sub(sub, root, fn_ty, syn);
            // Generic instantiations share this sub's boundary.
            b.capture(sub, captured);
            b.close_sub(sub, root, fn_ty, syn);
        }
        let root = b.close_block(outer, None, Ty::VOID, syn);
        b.finish(root, &[]).unwrap()
    }

    #[test]
    fn suspending_closure_parameters_are_distinct_from_their_creator_siblings_and_captures() {
        use crate::ir::count::{At, Info, Part};
        let facts = Facts {
            suspends: false,
            drop: Tri::No,
            declared: false,
        };
        let mut b = closures();
        for _ in 0..2 {
            let topo = Topology::new(&b);
            run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
            let copies: Vec<_> = (0..b.len())
                .filter_map(|i| {
                    let info = Info::read(&b, i)?;
                    matches!(info.part, Part::Local(_)).then_some((b.data[i][0], info))
                })
                .collect();
            assert_eq!(copies.len(), 4);
            for sub in 1..=2 {
                let r = b.sub_params[sub];
                let l = b.extra[r.start as usize + 1];
                for at in [At::Cold, At::Entry] {
                    assert!(copies.contains(&(b.sub_root[sub], Info::new(at, Part::Local(l), 0))));
                }
            }
            assert!(verify_owned(&b, &facts).is_empty());
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        for target in [0, 2, 3] {
            let mut wrong = b.clone();
            let copy = (0..wrong.len())
                .find(|&i| {
                    Info::read(&wrong, i).is_some_and(|c| matches!(c.part, Part::Local(_)))
                        && wrong.data[i][0] == wrong.sub_root[1]
                })
                .unwrap();
            wrong.data[copy][0] = wrong.sub_root[target];
            let errors = verify_owned(&wrong, &facts);
            assert!(
                errors
                    .iter()
                    .any(|e| e.what.contains("without its count fact"))
            );
            assert!(errors.iter().any(|e| e.what.contains("absent or repeated")));
        }
        let transferred = Facts {
            drop: Tri::Yes,
            ..facts
        };
        assert!(parameters(&b, &transferred).is_empty());
        assert!(
            verify_owned(&b, &transferred)
                .iter()
                .any(|e| e.what.contains("without its count fact"))
        );
    }

    #[test]
    fn the_constructor_copy_cannot_substitute_for_the_entry_copy() {
        use crate::ir::count::{At, Info};
        let facts = Facts {
            suspends: true,
            drop: Tri::No,
            declared: true,
        };
        let mut b = body(Ty::STRING);
        let topo = Topology::new(&b);
        run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
        let entry = (0..b.len())
            .find(|&i| Info::read(&b, i).is_some_and(|c| c.at == At::Entry))
            .unwrap();
        b.extra[b.data[entry][1] as usize + 1] = At::Cold as u32;
        assert!(
            verify_owned(&b, &facts)
                .iter()
                .any(|e| e.what.contains("absent or repeated"))
        );
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
