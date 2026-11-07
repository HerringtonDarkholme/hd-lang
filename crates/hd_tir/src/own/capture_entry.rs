//! A closure body takes copies of the captured edges it owns on entry.
//! Its environment keeps those edges; borrowed captures and uncounted
//! concrete fields lower no copy. This is separate from the cold frame's
//! constructor packet and from explicit value parameters.

use super::OwnCx;
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut seen = vec![false; b.sub_root.len()];
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::Closure {
            continue;
        }
        let sub = b.data[i][0] as usize;
        if sub == 0 || seen[sub] {
            continue;
        }
        let record = b.record(b.data[i][1]);
        let own = b.closure_self(i);
        let copied = own.is_some()
            || (record[0]..record[0] + record[1]).any(|c| {
                let c = c as usize;
                b.cap_mode[c].shares() || cx.counted(b.local_ty[b.cap_local[c].idx()])
            });
        if copied {
            seen[sub] = true;
            out.push((
                Tag::Retain,
                b.sub_root[sub],
                Info::new(At::Entry, Part::Captures, 0),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, CaptureMode, TirBuilder, TirSink, Topology, local_flags, sub_flags,
        u32_of, verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts;
    impl OwnCx for Facts {
        fn function_suspends(&self, ty: Ty) -> bool {
            ty == Ty(45)
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty == Ty(47)
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
    }

    fn body(ty: Ty, mode: CaptureMode, function: Ty) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        let captured = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
        let sub = b.open_sub(&[]);
        let inner = b.open_block();
        let inner = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
        b.capture(sub, captured);
        b.close_sub(sub, inner, function, NodeIdx::NONE);
        let outer = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
        b.finish(outer, &[mode]).unwrap()
    }

    fn declarations(b: &Body) -> Vec<usize> {
        (0..b.len())
            .filter(|&i| Info::read(b, i) == Some(Info::new(At::Entry, Part::Captures, 0)))
            .collect()
    }

    fn state(b: &mut Body) {
        let topo = Topology::new(b);
        statements::run(b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        assert!(verify_owned(b, &Facts).is_empty());
    }

    #[test]
    fn a_counted_explicit_parameter_is_not_a_capture() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        let parameter = b.local(
            Ty::STRING,
            Symbol::from_raw(0),
            local_flags::PARAM,
            NodeIdx::NONE,
        );
        let sub = b.open_sub(&[parameter]);
        let inner = b.open_block();
        let inner = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
        b.close_sub(sub, inner, Ty(46), NodeIdx::NONE);
        let outer = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(outer, &[]).unwrap();
        state(&mut b);
        assert!(declarations(&b).is_empty());
    }

    #[test]
    fn loaded_captures_use_types_and_modes_in_both_plain_and_suspending_bodies() {
        for (ty, mode, function, wanted) in [
            (Ty::STRING, CaptureMode::Copy, Ty(45), 1),
            (Ty::STRING, CaptureMode::Move, Ty(45), 1),
            (Ty::I32, CaptureMode::Shared, Ty(45), 1),
            (Ty::I32, CaptureMode::Frame, Ty(45), 1),
            (Ty::I32, CaptureMode::Copy, Ty(45), 0),
            (Ty(47), CaptureMode::Copy, Ty(45), 1),
            (Ty::STRING, CaptureMode::Copy, Ty(46), 1),
            (Ty::I32, CaptureMode::Shared, Ty(46), 1),
            (Ty::I32, CaptureMode::Frame, Ty(46), 1),
            (Ty::I32, CaptureMode::Copy, Ty(46), 0),
        ] {
            let mut b = body(ty, mode, function);
            for _ in 0..2 {
                state(&mut b);
                let ds = declarations(&b);
                assert_eq!(ds.len(), wanted);
                for i in ds {
                    assert_eq!(b.tags[i], Tag::Retain);
                    assert_eq!(b.data[i][0], b.sub_root[1]);
                    assert_eq!(Topology::new(&b).blk[i], b.sub_root[1]);
                }
            }
            let text = crate::ir::print(&b);
            assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        }
    }

    #[test]
    fn generic_constructions_share_one_capture_entry_declaration() {
        let mut b = body(Ty::STRING, CaptureMode::Copy, Ty(45));
        b.sub_flags[1] |= sub_flags::GENERIC;
        let ci = b.tags.iter().position(|&t| t == Tag::Closure).unwrap();
        let rec = b.record(b.data[ci][1]).to_vec();
        let at = u32_of(b.extra.len());
        b.extra.extend([4, rec[0], rec[1], 0, 0]);
        let topo = Topology::new(&b);
        let mut edits = splice::Tables::default();
        edits.insert(
            topo.blk[ci],
            topo.pos[ci] + 1,
            Tag::Closure,
            [1, at],
            Ty(45),
            NodeIdx::NONE,
        );
        splice::apply(&mut b, &topo, &mut edits).unwrap();
        for _ in 0..2 {
            state(&mut b);
            assert_eq!(declarations(&b).len(), 1);
        }
    }

    #[test]
    fn reconstructed_self_has_an_entry_environment_count_without_a_constructor_field() {
        let mut b = body(Ty(45), CaptureMode::Shared, Ty(45));
        let ci = b.tags.iter().position(|&t| t == Tag::Closure).unwrap();
        b.syn[ci] = NodeIdx::from_raw(1);
        b.local_syn[0] = b.syn[ci];
        b.local_flags[0] |= local_flags::SHORT;
        assert!(b.closure_self(ci).is_some());
        assert!(crate::own::captures::counts(&b, &Facts).is_empty());
        state(&mut b);
        assert_eq!(declarations(&b).len(), 1);
    }

    #[test]
    fn the_entry_copy_needs_its_own_action_phase_root_part_reason_and_multiplicity() {
        let mut b = body(Ty::STRING, CaptureMode::Copy, Ty(45));
        state(&mut b);
        let i = declarations(&b)[0];
        for info in [
            Info::new(At::Operation, Part::Captures, 0),
            Info::new(At::Entry, Part::Row, 0),
            Info::new(At::Entry, Part::Captures, 2),
        ] {
            let mut wrong = b.clone();
            wrong.data[i][1] = info.write(&mut wrong);
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
        let mut wrong = b.clone();
        wrong.tags[i] = Tag::Release;
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        let mut wrong = b.clone();
        wrong.data[i][0] = wrong.sub_root[0];
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        let mut wrong = b.clone();
        wrong.local_ty[0] = Ty::I32;
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        for duplicate in [false, true] {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let mut edits = splice::Tables::default();
            if duplicate {
                edits.insert(
                    wrong.sub_root[1],
                    0,
                    Tag::Retain,
                    wrong.data[i],
                    Ty::VOID,
                    NodeIdx::NONE,
                );
            } else {
                edits.remove(u32_of(i));
            }
            splice::apply(&mut wrong, &topo, &mut edits).unwrap();
            assert!(!verify_owned(&wrong, &Facts).is_empty());
        }
    }
}
