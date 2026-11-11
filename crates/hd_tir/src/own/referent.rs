//! A cold frame's saved counts of local-reference objects held by a parameter.
//! A generic body's conditional contract lowers for each active by-value
//! reference leaf, including enum payloads selected by their discriminants.
//! Normal exit releases the live slot; teardown releases it if not EXITED.

use super::{OwnCx, cold};
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for sub in cold::sub_bodies(b, cx) {
        let root = b.sub_root[sub as usize];
        for l in cold::locals(b, sub) {
            if cx.may_frame_referent(b.local_ty[l as usize]) {
                out.push((Tag::Retain, root, Info::new(At::Cold, Part::Referent(l), 8)));
                out.push((
                    Tag::Release,
                    root,
                    Info::new(At::FrameDrop, Part::Referent(l), 2),
                ));
                out.push((
                    Tag::Release,
                    root,
                    Info::new(At::Exit, Part::Referent(l), 8),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, TirBuilder, TirSink, Topology, local_flags, verify_owned};
    use crate::own::{Pass, counts, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts(bool);
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn suspending(&self, _: DefId) -> bool {
            self.0
        }
        fn function_suspends(&self, ty: Ty) -> bool {
            ty == Ty(45)
        }
        fn may_frame_referent(&self, ty: Ty) -> bool {
            matches!(ty, Ty(43 | 44))
        }
        fn counted(&self, ty: Ty) -> bool {
            Cx.counted(ty) || ty == Ty(43)
        }
        fn pass(&self, c: &Callee, k: usize, n: usize) -> Pass {
            Cx.pass(c, k, n)
        }
        fn pass_value(&self, t: Ty, k: usize, n: usize) -> Pass {
            Cx.pass_value(t, k, n)
        }
        fn may_write(&self, c: &Callee) -> bool {
            Cx.may_write(c)
        }
        fn parts(&self, t: Ty, f: Option<u32>, out: &mut Vec<Ty>) -> bool {
            Cx.parts(t, f, out)
        }
        fn received(&self, d: DefId, k: usize, n: usize) -> Pass {
            Cx.received(d, k, n)
        }
        fn holds_coll(&self, t: Ty) -> bool {
            Cx.holds_coll(t)
        }
    }

    fn body() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        // A thin reference, an open instance type, a string, a scalar and
        // a fat reference. Only the first two can need a frame-object count.
        for ty in [Ty(43), Ty(44), Ty::STRING, Ty::I32, Ty(46)] {
            b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
        }
        let root = b.close_block(root, None, Ty::VOID, NodeIdx::NONE);
        b.finish(root, &[]).unwrap()
    }

    fn stated(b: &mut Body, facts: &Facts) {
        let topo = Topology::new(b);
        counts::run(b, facts, &topo, &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    fn declarations(b: &Body) -> Vec<(Tag, u32, Info)> {
        (0..b.len())
            .filter_map(|i| {
                let info = Info::read(b, i)?;
                matches!(info.part, Part::Referent(_)).then_some((b.tags[i], b.data[i][0], info))
            })
            .collect()
    }

    #[test]
    fn each_possible_referent_has_separate_cold_and_teardown_contracts() {
        let mut b = body();
        for _ in 0..2 {
            stated(&mut b, &Facts(true));
            let ds = declarations(&b);
            assert_eq!(ds.len(), 6);
            for l in [0, 1] {
                assert!(ds.contains(&(
                    Tag::Retain,
                    b.sub_root[0],
                    Info::new(At::Cold, Part::Referent(l), 8)
                )));
                assert!(ds.contains(&(
                    Tag::Release,
                    b.sub_root[0],
                    Info::new(At::FrameDrop, Part::Referent(l), 2)
                )));
                assert!(ds.contains(&(
                    Tag::Release,
                    b.sub_root[0],
                    Info::new(At::Exit, Part::Referent(l), 8)
                )));
            }
            // The reference value's count is a different owner, not a
            // substitute for the object the frame must keep alive.
            assert!(
                crate::ir::count::find(&b, Tag::Retain, b.sub_root[0], At::Cold, Part::Local(0))
                    .is_some()
            );
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        let mut ordinary = body();
        stated(&mut ordinary, &Facts(false));
        assert!(declarations(&ordinary).is_empty());
    }

    #[test]
    fn the_parameter_object_contract_requires_its_own_action_phase_local_and_reason() {
        let mut b = body();
        stated(&mut b, &Facts(true));
        for (tag, at, reason) in [
            (Tag::Retain, At::Cold, 8),
            (Tag::Release, At::FrameDrop, 2),
            (Tag::Release, At::Exit, 8),
        ] {
            let i = (0..b.len())
                .find(|&i| {
                    b.tags[i] == tag
                        && Info::read(&b, i) == Some(Info::new(at, Part::Referent(0), reason))
                })
                .unwrap();
            for info in [
                Info::new(At::Entry, Part::Referent(0), reason),
                Info::new(at, Part::Local(0), reason),
                Info::new(at, Part::Referent(2), reason),
                Info::new(at, Part::Referent(0), 0),
            ] {
                let mut wrong = b.clone();
                wrong.data[i][1] = info.write(&mut wrong);
                assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
            }
            let mut wrong = b.clone();
            wrong.tags[i] = if tag == Tag::Release {
                Tag::Retain
            } else {
                Tag::Release
            };
            assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
            let mut wrong = b.clone();
            let record = wrong.data[i][1] as usize;
            wrong.extra[record + 3] = u32::MAX;
            assert!(!verify_owned(&wrong, &Facts(true)).is_empty());
            for duplicate in [false, true] {
                let mut wrong = b.clone();
                let topo = Topology::new(&wrong);
                let mut edits = splice::Tables::default();
                if duplicate {
                    edits.insert(
                        wrong.sub_root[0],
                        0,
                        tag,
                        wrong.data[i],
                        Ty::VOID,
                        NodeIdx::NONE,
                    );
                } else {
                    edits.remove(crate::ir::u32_of(i));
                }
                splice::apply(&mut wrong, &topo, &mut edits).unwrap();
                assert!(
                    verify_owned(&wrong, &Facts(true))
                        .iter()
                        .any(|e| e.what.contains("absent or repeated"))
                );
            }
        }
    }

    #[test]
    fn closure_parameters_exclude_captures_and_belong_only_to_their_own_frame() {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let outer = b.open_block();
        let captured = b.local(
            Ty(43),
            Symbol::from_raw(0),
            local_flags::PARAM,
            NodeIdx::NONE,
        );
        for ty in [Ty(45), Ty(45), Ty(46)] {
            let param = b.local(
                Ty(43),
                Symbol::from_raw(0),
                local_flags::PARAM,
                NodeIdx::NONE,
            );
            let sub = b.open_sub(&[param]);
            let inner = b.open_block();
            let root = b.close_block(inner, None, Ty::VOID, NodeIdx::NONE);
            b.capture(sub, captured);
            b.close_sub(sub, root, ty, NodeIdx::NONE);
            // Repeated generic instances keep one sub-body contract.
            b.capture(sub, captured);
            b.close_sub(sub, root, ty, NodeIdx::NONE);
        }
        let root = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        for _ in 0..2 {
            stated(&mut b, &Facts(false));
            let ds = declarations(&b);
            assert_eq!(ds.len(), 6);
            for sub in [1, 2] {
                let r = b.sub_params[sub];
                let l = b.extra[r.start as usize + 1];
                assert!(ds.contains(&(
                    Tag::Retain,
                    b.sub_root[sub],
                    Info::new(At::Cold, Part::Referent(l), 8)
                )));
                assert!(ds.contains(&(
                    Tag::Release,
                    b.sub_root[sub],
                    Info::new(At::FrameDrop, Part::Referent(l), 2)
                )));
                assert!(ds.contains(&(
                    Tag::Release,
                    b.sub_root[sub],
                    Info::new(At::Exit, Part::Referent(l), 8)
                )));
            }
        }
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
        let release = (0..b.len())
            .find(|&i| {
                b.tags[i] == Tag::Release
                    && b.data[i][0] == b.sub_root[1]
                    && Info::read(&b, i).is_some_and(|c| matches!(c.part, Part::Referent(_)))
            })
            .unwrap();
        for sub in [0, 2, 3] {
            let mut wrong = b.clone();
            wrong.data[release][0] = wrong.sub_root[sub];
            let errors = verify_owned(&wrong, &Facts(false));
            assert!(
                errors
                    .iter()
                    .any(|e| e.what.contains("without its count fact"))
            );
            assert!(errors.iter().any(|e| e.what.contains("absent or repeated")));
        }
    }
}
