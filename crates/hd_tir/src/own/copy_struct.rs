//! An in-place struct copy-update copies only fields its record keeps.
//! Replacement values transfer through their own operand contracts. Open
//! field types may count in an instance; a constant base has no count.

use super::OwnCx;
use crate::ir::{
    Body, Ref, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    let mut fields = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::CopyStruct
            || Ref(b.data[i][0]).as_inst().is_none()
            || !cx.struct_fields(b.ty[i], &mut fields)
        {
            continue;
        }
        let replaced = b.record(b.data[i][1]);
        if fields
            .iter()
            .enumerate()
            .any(|(f, &t)| cx.counted(t) && !replaced.chunks_exact(2).any(|w| w[0] as usize == f))
        {
            out.push((
                Tag::Retain,
                u32_of(i),
                Info::new(At::Operation, Part::Operand(0), 0),
            ));
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
        fn struct_fields(&self, ty: Ty, out: &mut Vec<Ty>) -> bool {
            out.clear();
            out.extend(match ty {
                Ty(43) => [Ty::STRING, Ty::I32].as_slice(),
                Ty(44) => [Ty::I32, Ty::I32].as_slice(),
                Ty(45) => [Ty(47), Ty::STRING].as_slice(),
                Ty(48) => [].as_slice(),
                _ => return false,
            });
            true
        }
        fn counted(&self, ty: Ty) -> bool {
            ty == Ty(47) || Cx.counted(ty)
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

    fn body(ty: Ty, replaced: &[u32], constant: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let base = if constant {
            b.const_value(ty, 0)
        } else {
            let l = b.local(ty, Symbol::from_raw(0), local_flags::PARAM, NodeIdx::NONE);
            b.get(l, ty, NodeIdx::NONE)
        };
        let mut fields = Vec::new();
        Facts.struct_fields(ty, &mut fields);
        let mut words = Vec::new();
        for &f in replaced {
            let v = b.const_value(fields[f as usize], 0);
            words.extend([f, v.0]);
        }
        let fields = b.words_record(&words);
        b.emit(Tag::CopyStruct, base.0, fields, ty, NodeIdx::NONE);
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
    fn only_kept_possibly_counted_fields_of_nonconstant_structs_need_a_packet() {
        for (ty, fields, needed) in [
            (Ty(43), vec![], true),
            (Ty(43), vec![0], false),
            (Ty(43), vec![1], true),
            (Ty(43), vec![0, 1], false),
            (Ty(44), vec![], false),
            (Ty(45), vec![], true),
            (Ty(45), vec![0], true),
            (Ty(45), vec![1], true),
            (Ty(45), vec![0, 1], false),
            (Ty(46), vec![], false),
            (Ty(48), vec![], false),
        ] {
            for constant in [false, true] {
                let mut b = body(ty, &fields, constant);
                b.anchors = vec![37; b.len()];
                for _ in 0..2 {
                    state(&mut b);
                    let ds: Vec<_> = (0..b.len())
                        .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Operand(0)))
                        .collect();
                    assert_eq!(ds.len(), usize::from(needed && !constant));
                    for i in ds {
                        let target = b.data[i][0];
                        assert_eq!(b.tags[i], Tag::Retain);
                        assert_eq!(b.tags[target as usize], Tag::CopyStruct);
                        assert_eq!(b.anchors[i], b.anchors[target as usize]);
                        assert!(count::adjacent(&b, &Topology::new(&b), target, u32_of(i)));
                        assert_eq!(
                            count::operation(
                                &b,
                                &Topology::new(&b),
                                target,
                                Tag::Retain,
                                Part::Operand(0)
                            ),
                            Info::read(&b, i)
                        );
                    }
                }
                let text = crate::ir::print(&b);
                assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
            }
        }
    }
    #[test]
    fn a_copy_needs_its_packets_action_part_phase_reason_target_and_position() {
        let mut b = body(Ty(43), &[], false);
        state(&mut b);
        let i = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        for info in [
            Info::new(At::Cold, Part::Operand(0), 0),
            Info::new(At::Operation, Part::Row, 0),
            Info::new(At::Operation, Part::Operand(0), 2),
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
        wrong.data[i][1] = Info::new(At::Operation, Part::Operand(1), 0).write(&mut wrong);
        assert!(!verify_owned(&wrong, &Facts).is_empty());
        for mode in 0..3 {
            let mut wrong = b.clone();
            let topo = Topology::new(&wrong);
            let target = wrong.data[i][0] as usize;
            let mut edits = splice::Tables::default();
            if mode != 1 {
                edits.remove(u32_of(i));
            }
            if mode != 0 {
                edits.insert(
                    topo.blk[target],
                    if mode == 1 {
                        topo.pos[i] + 1
                    } else {
                        topo.pos[target]
                    },
                    Tag::Retain,
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
