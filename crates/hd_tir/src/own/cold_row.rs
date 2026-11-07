//! The requirement row captured by a source function's cold frame.
//!
//! The declaration copies the frame-owned portion of that ABI row.
//! Its instance layout supplies provider data and any open-row context;
//! a layout that owns no counts lowers no copy.

use super::{OwnCx, params, splice};
use crate::ir::{Body, BodyKind, Tag, Topology};
use hd_types::Ty;

pub(crate) fn needed(b: &Body, cx: &dyn OwnCx) -> bool {
    matches!(b.kind, BodyKind::Fn | BodyKind::DeriveInstance)
        && cx.suspending(b.item)
        && cx.has_requirement_row(b.item)
        && cx.parameter_count(b.item) == Some(params(b).len())
}

pub(super) fn run(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    if !needed(b, cx) {
        return Ok(());
    }
    let root = b.sub_root[0];
    if !b
        .record(b.data[root as usize][0])
        .iter()
        .any(|&i| b.tags[i as usize] == Tag::ColdRowCopy)
    {
        edits.insert(
            root,
            0,
            Tag::ColdRowCopy,
            [0, 0],
            Ty::VOID,
            b.syn[root as usize],
        );
    }
    splice::apply(b, topo, edits)
}

#[cfg(test)]
mod tests {
    use super::super::Pass;
    use super::*;
    use crate::ir::{Callee, TirBuilder, TirSink, verify_owned};
    use hd_base::DefId;

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

    #[test]
    fn a_source_cold_row_is_declared_once_and_other_abis_are_separate() {
        for (kind, row, suspends, declared, count) in [
            (BodyKind::Fn, true, true, true, 1),
            (BodyKind::DeriveInstance, true, true, true, 1),
            (BodyKind::Fn, false, true, true, 0),
            (BodyKind::Fn, true, false, true, 0),
            (BodyKind::Fn, true, true, false, 0),
            (BodyKind::Default, true, true, true, 0),
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
                assert_eq!(
                    b.tags.iter().filter(|&&t| t == Tag::ColdRowCopy).count(),
                    count
                );
                let errors = verify_owned(&b, &facts);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
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
                .any(|e| e.what.contains("row lacks its one copy"))
        );
        let topo = Topology::new(&b);
        run(&mut b, &facts, &topo, &mut splice::Tables::default()).unwrap();
        let copy = b.tags.iter().position(|&t| t == Tag::ColdRowCopy).unwrap();
        b.data[copy][1] = 2;
        assert!(
            verify_owned(&b, &facts)
                .iter()
                .any(|e| e.what.contains("source-requirement proof"))
        );
        b.data[copy][1] = 0;
        let empty = Facts {
            row: false,
            ..facts
        };
        assert!(
            verify_owned(&b, &empty)
                .iter()
                .any(|e| e.what.contains("source-requirement proof"))
        );
    }
}
