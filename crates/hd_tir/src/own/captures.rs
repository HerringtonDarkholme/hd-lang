//! The owned edges copied into a closure's fresh environment.
//!
//! Capture modes state which values transfer and which fields are copied.
//! The ABI/layout lowers the copied packet: a transferred field takes no
//! count, and a scalar or static frame cell lowers no count operation.

use super::OwnCx;
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let recursive = super::RecursiveBindings::new(b);
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::Closure
            || (b.sub_is_generic(b.data[i][0] as usize) && !b.is_instantiation(i))
        {
            continue;
        }
        let record = b.record(b.data[i][1]);
        let (start, len) = (record[0] as usize, record[1] as usize);
        let own = recursive.self_local(i);
        let copied = (start..start + len).any(|c| {
            Some(b.cap_local[c]) != own
                && (b.cap_mode[c].shares() || cx.counted(b.local_ty[b.cap_local[c].idx()]))
        });
        if copied {
            out.push((
                Tag::Retain,
                u32_of(i),
                Info::new(At::Operation, Part::Captures, 0),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, CaptureMode, TirBuilder, TirSink, Topology, local_flags, sub_flags, verify_owned,
    };
    use crate::own::{counts, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    fn body(caps: &[(Ty, CaptureMode)]) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        let locals: Vec<_> = caps
            .iter()
            .map(|(ty, _)| b.local(*ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE))
            .collect();
        let sub = b.open_sub(&[]);
        let inner = b.open_block();
        let root = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
        for l in locals {
            b.capture(sub, l);
        }
        b.close_sub(sub, root, Ty(43), NodeIdx::NONE);
        let root = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &caps.iter().map(|c| c.1).collect::<Vec<_>>())
            .unwrap()
    }

    fn closure(b: &Body) -> usize {
        b.tags.iter().position(|&t| t == Tag::Closure).unwrap()
    }

    fn statement(b: &Body) -> Option<usize> {
        (0..b.len()).find(|&i| {
            Info::read(b, i).is_some_and(|c| c.at == At::Operation && c.part == Part::Captures)
        })
    }

    #[test]
    fn capture_modes_and_types_state_one_conditional_copy_packet() {
        for (caps, expected) in [
            (vec![], false),
            (vec![(Ty::I32, CaptureMode::Copy)], false),
            (vec![(Ty::I32, CaptureMode::Shared)], true),
            (vec![(Ty::I32, CaptureMode::Frame)], true),
            (vec![(Ty::STRING, CaptureMode::Copy)], true),
            // A moved field lowers no copy when its ABI transfers ownership;
            // a caller-lent field still needs its own environment copy.
            (vec![(Ty::STRING, CaptureMode::Move)], true),
            (
                vec![
                    (Ty::I32, CaptureMode::Copy),
                    (Ty::STRING, CaptureMode::Move),
                    (Ty::I32, CaptureMode::Shared),
                ],
                true,
            ),
        ] {
            let mut b = body(&caps);
            for _ in 0..2 {
                let topo = Topology::new(&b);
                counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
                let ds: Vec<_> = (0..b.len())
                    .filter(|&i| {
                        Info::read(&b, i)
                            .is_some_and(|c| c.at == At::Operation && c.part == Part::Captures)
                    })
                    .collect();
                assert_eq!(ds.len(), usize::from(expected));
                if let Some(&i) = ds.first() {
                    let point = closure(&b);
                    assert_eq!(b.tags[i], Tag::Retain);
                    assert_eq!(b.data[i][0], u32_of(point));
                    let topo = Topology::new(&b);
                    assert_eq!(
                        crate::ir::count::following(&b, &topo, u32_of(point), Tag::Retain),
                        Some(Info::new(At::Operation, Part::Captures, 0))
                    );
                }
                let errors = verify_owned(&b, &Cx);
                assert!(errors.is_empty(), "{errors:?}");
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn the_copy_requires_its_closure_action_subject_reason_and_adjacency() {
        let mut b = body(&[(Ty::I32, CaptureMode::Shared)]);
        let topo = Topology::new(&b);
        counts::run(&mut b, &Cx, &topo, &mut splice::Tables::default()).unwrap();
        let copy = statement(&b).unwrap();
        for info in [
            Info::new(At::Cold, Part::Captures, 0),
            Info::new(At::Operation, Part::Result, 0),
            Info::new(At::Operation, Part::Captures, 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[copy][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Cx).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[copy] = Tag::Release;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        wrong.data[copy][0] = wrong.sub_root[0];
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        let mut wrong = b.clone();
        let record = wrong.data[copy][1] as usize;
        wrong.extra[record + 3] = 1;
        assert!(!verify_owned(&wrong, &Cx).is_empty());
        for mode in 0..3 {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            let target = wrong.data[copy][0] as usize;
            if mode == 0 {
                edits.remove(u32_of(copy));
            } else {
                edits.insert(
                    if mode == 1 {
                        topo.blk[target]
                    } else {
                        wrong.sub_root[1]
                    },
                    if mode == 1 { topo.pos[target] + 1 } else { 0 },
                    Tag::Retain,
                    wrong.data[copy],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(
                verify_owned(&wrong, &Cx)
                    .iter()
                    .any(|e| e.what.contains("absent or repeated")
                        || e.what.contains("without its count fact"))
            );
        }
    }

    #[test]
    fn generic_declarations_and_self_only_captures_make_no_copy_packet() {
        let mut b = body(&[(Ty::STRING, CaptureMode::Shared)]);
        let ci = closure(&b);
        b.sub_flags[1] |= sub_flags::GENERIC;
        assert!(super::counts(&b, &Cx).is_empty());
        // The same capture layout at a concrete construction does own its
        // packet; only the uninstantiated definition is nonexecuting.
        let record = b.record(b.data[ci][1]).to_vec();
        let at = u32_of(b.extra.len());
        b.extra.extend([4, record[0], record[1], 0, 0]);
        b.data[ci][1] = at;
        assert_eq!(super::counts(&b, &Cx).len(), 1);
        let mut b = body(&[(Ty(43), CaptureMode::Shared)]);
        let ci = closure(&b);
        let syn = NodeIdx::from_raw(1);
        b.syn[ci] = syn;
        b.local_syn[0] = syn;
        b.local_flags[0] |= local_flags::SHORT;
        assert!(b.closure_self(ci).is_some());
        assert!(super::counts(&b, &Cx).is_empty());
    }
}
