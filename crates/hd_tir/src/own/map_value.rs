//! A map lookup/removal hit copies its table value into a Some payload.
//! A removal then ends the table's value and stored key counts. A miss
//! has no payload; none of these packets counts a fresh container's initial count.

use super::OwnCx;
use crate::ir::{
    Body, IntrinsicOp, Ref, Tag,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::Intrinsic {
            continue;
        }
        let op = IntrinsicOp::from_u32(b.data[i][0]);
        if !matches!(op, Some(IntrinsicOp::MapGet | IntrinsicOp::MapRemove)) {
            continue;
        }
        let remove = op == Some(IntrinsicOp::MapRemove);
        if cx.counted(b.ty[i]) {
            out.push((
                Tag::Retain,
                u32_of(i),
                Info::new(At::Operation, Part::Operand(0), 0),
            ));
            if remove {
                out.push((
                    Tag::Release,
                    u32_of(i),
                    Info::new(At::Operation, Part::Operand(0), 2),
                ));
            }
        }
        if remove && let Some(&key) = b.record(b.data[i][1]).get(1) {
            // The builtin's key argument has K, not &K. A constant lookup
            // key still ends the table's stored key, which can be dynamic.
            let key = Ref(key);
            let ty = match key.as_inst() {
                Some(v) => b.ty[v.0 as usize],
                None => b.consts[(key.0 & !Ref::CONST_BIT) as usize].0,
            };
            if cx.counted(ty) {
                out.push((
                    Tag::Release,
                    u32_of(i),
                    Info::new(At::Operation, Part::Operand(1), 2),
                ));
            }
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
        fn counted(&self, ty: Ty) -> bool {
            matches!(ty, Ty(43 | 45 | 47)) || Cx.counted(ty)
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
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn holds_coll(&self, ty: Ty) -> bool {
            Cx.holds_coll(ty)
        }
    }

    fn body(op: IntrinsicOp, ty: Ty) -> Body {
        body_with_key(op, ty, Ty::STRING, true)
    }

    fn body_with_key(op: IntrinsicOp, ty: Ty, key_ty: Ty, constant: bool) -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = b.open_block();
        let map = b.const_value(Ty(44), 0);
        let key = if constant {
            b.const_value(key_ty, 0)
        } else {
            let l = b.local(
                key_ty,
                Symbol::from_raw(0),
                local_flags::PARAM,
                NodeIdx::NONE,
            );
            b.get(l, key_ty, NodeIdx::NONE)
        };
        let args = b.refs_record(&[map, key]);
        let result = b.emit(Tag::Intrinsic, op as u32, args, ty, NodeIdx::NONE);
        let root = b.close_block(block, Some(result), ty, NodeIdx::NONE);
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
    fn a_get_or_remove_states_one_copy_packet_only_for_possibly_counted_results() {
        for op in [
            IntrinsicOp::MapGet,
            IntrinsicOp::MapRemove,
            IntrinsicOp::MapEntry,
        ] {
            // String/collection/open results, followed by scalar/no-value results.
            for ty in [Ty(43), Ty(45), Ty(47), Ty::I32, Ty::VOID] {
                let mut b = body(op, ty);
                b.anchors = vec![37; b.len()];
                for _ in 0..2 {
                    state(&mut b);
                    let ds: Vec<_> = (0..b.len())
                        .filter(|&i| {
                            b.tags[i] == Tag::Retain
                                && Info::read(&b, i).is_some_and(|c| c.part == Part::Operand(0))
                        })
                        .collect();
                    assert_eq!(
                        ds.len(),
                        usize::from(
                            matches!(op, IntrinsicOp::MapGet | IntrinsicOp::MapRemove)
                                && Facts.counted(ty)
                        )
                    );
                    for i in ds {
                        let target = b.data[i][0];
                        assert_eq!(b.tags[i], Tag::Retain);
                        assert_eq!(b.tags[target as usize], Tag::Intrinsic);
                        assert_eq!(
                            Info::read(&b, i),
                            Some(Info::new(At::Operation, Part::Operand(0), 0))
                        );
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
    fn a_remove_states_the_values_two_actions_and_the_stored_keys_last_use() {
        for ty in [Ty(43), Ty(45), Ty(47), Ty::I32] {
            for key in [Ty::STRING, Ty(47), Ty::I32] {
                for constant in [false, true] {
                    let mut b = body_with_key(IntrinsicOp::MapRemove, ty, key, constant);
                    b.anchors = vec![41; b.len()];
                    for _ in 0..2 {
                        state(&mut b);
                        let target =
                            u32_of(b.tags.iter().position(|&t| t == Tag::Intrinsic).unwrap());
                        let topo = Topology::new(&b);
                        let rows: Vec<_> = (0..b.len())
                            .filter_map(|i| {
                                Info::read(&b, i)
                                    .filter(|c| c.at == At::Operation)
                                    .map(|c| (i, c))
                            })
                            .collect();
                        assert_eq!(
                            rows.len(),
                            usize::from(Facts.counted(ty)) * 2 + usize::from(Facts.counted(key))
                        );
                        for (action, part, reason, needed) in [
                            (Tag::Retain, Part::Operand(0), 0, Facts.counted(ty)),
                            (Tag::Release, Part::Operand(0), 2, Facts.counted(ty)),
                            (Tag::Release, Part::Operand(1), 2, Facts.counted(key)),
                        ] {
                            assert_eq!(
                                count::operation(&b, &topo, target, action, part),
                                needed.then_some(Info::new(At::Operation, part, reason))
                            );
                        }
                        for (i, _) in rows {
                            assert_eq!(b.data[i][0], target);
                            assert_eq!(b.anchors[i], b.anchors[target as usize]);
                            assert!(count::adjacent(&b, &topo, target, u32_of(i)));
                        }
                    }
                    let text = crate::ir::print(&b);
                    assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
                }
            }
        }
    }

    #[test]
    fn each_get_or_remove_packet_needs_its_complete_contract() {
        for op in [IntrinsicOp::MapGet, IntrinsicOp::MapRemove] {
            let mut b = body(op, Ty(43));
            state(&mut b);
            for i in (0..b.len()).filter(|&i| Info::read(&b, i).is_some()) {
                let info = Info::read(&b, i).unwrap();
                let action = b.tags[i];
                for replacement in [
                    Info::new(At::Cold, info.part, info.reason),
                    Info::new(At::Operation, Part::Row, info.reason),
                    Info::new(At::Operation, info.part, 8),
                ] {
                    let mut wrong = b.clone();
                    wrong.data[i][1] = replacement.write(&mut wrong);
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
                let mut wrong = b.clone();
                let part = if info.part == Part::Operand(0) {
                    Part::Operand(1)
                } else {
                    Part::Operand(0)
                };
                wrong.data[i][1] = Info::new(At::Operation, part, info.reason).write(&mut wrong);
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
}
