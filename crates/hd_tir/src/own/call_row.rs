//! A function-value call's temporary context owns its row's provider data.
//! The copy packet and the temporary context's last use are separate
//! statements. An empty concrete row or a forwarded open context lowers
//! neither; a cold suspension constructor keeps its own row count.

use super::OwnCx;
use crate::ir::{
    Body, Ref, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::CallValue {
            continue;
        }
        let callee = Ref(b.data[i][0]);
        let ty = match callee.as_inst() {
            Some(v) => b.ty[v.0 as usize],
            None => b.consts[(callee.0 & !Ref::CONST_BIT) as usize].0,
        };
        if !cx.function_has_requirement_row(ty) {
            continue;
        }
        for (tag, reason) in [(Tag::Retain, 0), (Tag::Release, 2)] {
            out.push((tag, u32_of(i), Info::new(At::Operation, Part::Row, reason)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BodyKind, Callee, ReadMode, TirBuilder, TirSink, Topology, count, local_flags, verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts;
    impl OwnCx for Facts {
        fn function_has_requirement_row(&self, ty: Ty) -> bool {
            ty == Ty(43) || ty == Ty(45)
        }
        fn counted(&self, ty: Ty) -> bool {
            Cx.counted(ty)
        }
        fn pass(&self, c: &Callee, k: usize, n: usize) -> Pass {
            Cx.pass(c, k, n)
        }
        fn pass_value(&self, ty: Ty, k: usize, n: usize) -> Pass {
            Cx.pass_value(ty, k, n)
        }
        fn may_write(&self, c: &Callee) -> bool {
            Cx.may_write(c)
        }
        fn parts(&self, ty: Ty, v: Option<u32>, out: &mut Vec<Ty>) -> bool {
            Cx.parts(ty, v, out)
        }
        fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
            Cx.received(item, k, n)
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

    fn body(ty: Ty, constant: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let callee = if constant {
            b.const_value(ty, 0)
        } else {
            let l = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
            b.get(l, ty, NodeIdx::NONE)
        };
        let args = b.refs_record(&[]);
        b.emit(Tag::CallValue, callee.0, args, Ty::I32, NodeIdx::NONE);
        let root = b.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut b = b.finish(root, &[]).unwrap();
        for (i, &tag) in b.tags.iter().enumerate() {
            if tag == Tag::Read {
                b.data[i][1] = ReadMode::Borrow as u32;
            }
        }
        b
    }

    fn state(b: &mut Body) {
        statements::run(b, &Facts, &Topology::new(b), &mut splice::Tables::default()).unwrap();
        let errors = verify_owned(b, &Facts);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_call_states_both_row_counts_once_and_an_empty_row_needs_neither() {
        for ty in [Ty(43), Ty(44), Ty(45)] {
            for constant in [false, true] {
                let mut b = body(ty, constant);
                b.anchors = vec![37; b.len()];
                for _ in 0..2 {
                    state(&mut b);
                    let ds: Vec<_> = (0..b.len())
                        .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Row))
                        .collect();
                    assert_eq!(ds.len(), if ty == Ty(44) { 0 } else { 2 });
                    for i in ds {
                        let point = b.data[i][0];
                        assert_eq!(b.tags[point as usize], Tag::CallValue);
                        let reason = if b.tags[i] == Tag::Retain { 0 } else { 2 };
                        assert_eq!(
                            Info::read(&b, i),
                            Some(Info::new(At::Operation, Part::Row, reason))
                        );
                        assert!(count::adjacent(&b, &Topology::new(&b), point, u32_of(i)));
                        assert_eq!(b.anchors[i], b.anchors[point as usize]);
                    }
                }
                let text = crate::ir::print(&b);
                assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
            }
        }
    }

    #[test]
    fn the_call_needs_each_actions_phase_row_reason_and_target() {
        let mut b = body(Ty(43), false);
        state(&mut b);
        for action in [Tag::Retain, Tag::Release] {
            let i = b.tags.iter().position(|&t| t == action).unwrap();
            let reason = if action == Tag::Retain { 0 } else { 2 };
            for info in [
                Info::new(At::Cold, Part::Row, reason),
                Info::new(At::Operation, Part::Captures, reason),
                Info::new(At::Operation, Part::Row, if reason == 0 { 2 } else { 0 }),
            ] {
                let mut wrong = b.clone();
                wrong.data[i][1] = info.write(&mut wrong);
                assert!(!verify_owned(&wrong, &Facts).is_empty());
            }
            let mut wrong = b.clone();
            wrong.tags[i] = if action == Tag::Retain {
                Tag::Release
            } else {
                Tag::Retain
            };
            assert!(!verify_owned(&wrong, &Facts).is_empty());
            let mut wrong = b.clone();
            wrong.data[i][0] = wrong.sub_root[0];
            assert!(!verify_owned(&wrong, &Facts).is_empty());
            for mode in 0..3 {
                let mut wrong = b.clone();
                let topo = Topology::new(&wrong);
                let point = wrong.data[i][0] as usize;
                let mut edits = splice::Tables::default();
                if mode != 1 {
                    edits.remove(u32_of(i));
                }
                if mode != 0 {
                    edits.insert(
                        topo.blk[point],
                        if mode == 1 {
                            topo.pos[i] + 1
                        } else {
                            topo.pos[point]
                        },
                        action,
                        wrong.data[i],
                        Ty::VOID,
                        NodeIdx::NONE,
                    );
                }
                splice::apply(&mut wrong, &topo, &mut edits).unwrap();
                assert!(!verify_owned(&wrong, &Facts).is_empty());
            }
        }
    }
}
