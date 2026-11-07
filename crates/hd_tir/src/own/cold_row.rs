//! The ABI row captured by a suspension frame.
//!
//! Cold construction and fresh materialization each copy the frame-owned
//! portion of that ABI row at their separate allocation boundaries.
//! Its instance layout supplies provider data and any open-row context;
//! a layout that owns no counts lowers no copy.

use super::{OwnCx, cold};
use crate::ir::Body;

/// A root passes its declared or inferred ABI row. A closure always passes one
/// caller context, even if its source row has no keys. Multiple generic
/// instantiations of the same sub-body share its declaration.
pub(crate) fn roots(b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
    cold::sub_bodies(b, cx)
        .into_iter()
        .filter(|&sub| sub != 0 || cx.has_requirement_row(b.item))
        .map(|sub| b.sub_root[sub as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::Pass;
    use super::super::{counts::run, splice};
    use super::*;
    use crate::ir::{BodyKind, Callee, TirBuilder, TirSink, verify_owned};
    use crate::ir::{Tag, Topology};
    use hd_base::DefId;
    use hd_types::Ty;

    struct Facts {
        row: bool,
        suspends: bool,
        declared: bool,
    }
    impl OwnCx for Facts {
        fn parameter_count(&self, _: DefId) -> Option<usize> {
            self.declared.then_some(0)
        }
        fn suspending(&self, _: DefId) -> bool {
            self.suspends
        }
        fn function_suspends(&self, ty: Ty) -> bool {
            ty == Ty(43)
        }
        fn has_requirement_row(&self, _: DefId) -> bool {
            self.row
        }
        fn counted(&self, _: Ty) -> bool {
            false
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

    fn body(kind: BodyKind) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), kind);
        let block = b.open_block();
        let root = b.close_block(block, None, Ty::VOID, hd_base::NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn closures(types: &[Ty], repeat: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        for &ty in types {
            let sub = b.open_sub(&[]);
            let inner = b.open_block();
            let root = b.close_block(inner, None, Ty::VOID, hd_base::NodeIdx::NONE);
            b.close_sub(sub, root, ty, hd_base::NodeIdx::NONE);
            if repeat {
                b.close_sub(sub, root, ty, hd_base::NodeIdx::NONE);
            }
        }
        let root = b.close_block(outer, None, Ty::VOID, hd_base::NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    #[test]
    fn suspending_closures_state_rows_at_their_own_roots_without_duplicate_contracts() {
        let facts = Facts {
            row: false,
            suspends: false,
            declared: true,
        };
        let mut b = closures(&[Ty(43), Ty(43), Ty(44)], true);
        for _ in 0..2 {
            let topo = Topology::new(&b);
            run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
            let topo = Topology::new(&b);
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 4);
            for (i, &tag) in b.tags.iter().enumerate() {
                if tag == Tag::Retain {
                    assert!(b.sub_root[1..3].contains(&b.data[i][0]));
                    assert_eq!(topo.blk[i], b.data[i][0]);
                }
            }
            let errors = verify_owned(&b, &facts);
            assert!(errors.is_empty(), "{errors:?}");
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn a_siblings_or_creators_row_contract_cannot_satisfy_the_closure() {
        let facts = Facts {
            row: false,
            suspends: false,
            declared: true,
        };
        for target in [0, 2] {
            let mut b = closures(&[Ty(43), Ty(43)], false);
            let topo = Topology::new(&b);
            run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
            let retain = (0..b.len())
                .find(|&i| b.tags[i] == Tag::Retain && b.data[i][0] == b.sub_root[1])
                .unwrap();
            b.data[retain][0] = b.sub_root[target];
            let errors = verify_owned(&b, &facts);
            assert!(
                errors
                    .iter()
                    .any(|e| e.what.contains("without its count fact"))
            );
            assert!(errors.iter().any(|e| e.what.contains("absent or repeated")));
        }
    }

    #[test]
    fn an_awaited_input_states_its_inferred_row_but_a_closures_await_is_not_its_own() {
        use crate::ir::Providers;
        use hd_types::TyList;
        for inside in [false, true] {
            let facts = Facts {
                row: true,
                suspends: !inside,
                declared: false,
            };
            let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Init);
            let outer = builder.open_block();
            let sub = inside.then(|| builder.open_sub(&[]));
            let block = builder.open_block();
            builder.call(
                &Callee::Item {
                    def: DefId::from_raw(7),
                    targs: TyList::EMPTY,
                },
                &[],
                Providers::None,
                Ty::VOID,
                hd_base::NodeIdx::NONE,
            );
            let block = builder.close_block(block, None, Ty::VOID, hd_base::NodeIdx::NONE);
            if let Some(sub) = sub {
                builder.close_sub(sub, block, Ty(43), hd_base::NodeIdx::NONE);
            }
            let root = builder.close_block(outer, None, Ty::VOID, hd_base::NodeIdx::NONE);
            let mut b = builder.finish(root, &[]).unwrap();
            let call = b.tags.iter().position(|&t| t == Tag::Call).unwrap();
            b.tags[call] = Tag::Await;
            let topo = Topology::new(&b);
            run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
            let expected = b.sub_root[usize::from(inside)];
            assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 2);
            for (i, &tag) in b.tags.iter().enumerate() {
                if tag == Tag::Retain {
                    assert_eq!(b.data[i][0], expected);
                }
            }
            let errors = verify_owned(&b, &facts);
            assert!(errors.is_empty(), "{errors:?}");
        }
    }

    #[test]
    fn row_copies_follow_the_abi_instead_of_a_body_kind_whitelist() {
        for (kind, row, suspends, declared, count) in [
            (BodyKind::Fn, true, true, true, 2),
            (BodyKind::DeriveInstance, true, true, true, 2),
            (BodyKind::Fn, false, true, true, 0),
            (BodyKind::Fn, true, false, true, 0),
            (BodyKind::Fn, true, true, false, 2),
            (BodyKind::Default, true, true, true, 2),
            (BodyKind::TestCase, true, true, true, 2),
            // The checked input's ABI survives even when optimization erased its point.
            (BodyKind::Init, true, true, false, 2),
            (BodyKind::Init, true, false, false, 0),
            (BodyKind::Init, false, true, false, 0),
        ] {
            let mut b = body(kind);
            let facts = Facts {
                row,
                suspends,
                declared,
            };
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
    fn materialization_cannot_use_the_constructors_copy_contract() {
        use crate::ir::count::{At, Info, Part};
        let (mut b, facts) = (
            body(BodyKind::Fn),
            Facts {
                row: true,
                suspends: true,
                declared: true,
            },
        );
        let topo = Topology::new(&b);
        run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
        let retain = (0..b.len())
            .find(|&i| Info::read(&b, i) == Some(Info::new(At::Materialize, Part::Row, 0)))
            .unwrap();
        b.extra[b.data[retain][1] as usize + 1] = At::Cold as u32;
        assert!(
            verify_owned(&b, &facts)
                .iter()
                .any(|e| e.what.contains("absent or repeated"))
        );
    }

    #[test]
    fn a_cold_row_copy_needs_its_signature_and_stored_reason() {
        let mut b = body(BodyKind::Fn);
        let facts = Facts {
            row: true,
            suspends: true,
            declared: true,
        };
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
        let empty = Facts {
            row: false,
            ..facts
        };
        assert!(
            verify_owned(&b, &empty)
                .iter()
                .any(|e| e.what.contains("count fact"))
        );
    }
}
