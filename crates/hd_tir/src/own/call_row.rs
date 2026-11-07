//! A call's temporary context owns its row's provider data.
//! The copy packet and the temporary context's last use are separate
//! statements. An empty concrete row or a forwarded open context lowers
//! neither; a cold suspension constructor keeps its own row count.

use super::OwnCx;
use crate::ir::{
    Body, Callee, Ref, Tag,
    count::{At, Info, Part},
    u32_of,
};
use hd_base::DefId;
use hd_types::TyList;

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        let needed = match tag {
            Tag::CallValue => {
                let callee = Ref(b.data[i][0]);
                let ty = match callee.as_inst() {
                    Some(v) => b.ty[v.0 as usize],
                    None => b.consts[(callee.0 & !Ref::CONST_BIT) as usize].0,
                };
                cx.function_has_requirement_row(ty)
            }
            Tag::Call | Tag::Await => Callee::from_words(b.record(b.data[i][0]))
                .is_some_and(|c| cx.call_has_requirement_context(&c)),
            Tag::DefaultCall => match b.record(b.data[i][0]) {
                &[def, args, ..] => cx.call_has_requirement_context(&Callee::Item {
                    def: DefId::from_raw(def),
                    targs: TyList(args),
                }),
                _ => false,
            },
            _ => false,
        };
        if !needed {
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
        BodyKind, Callee, ChoiceKind, Providers, ReadMode, TirBuilder, TirSink, Topology, count,
        local_flags, verify_owned,
    };
    use crate::own::{Pass, counts as statements, splice, tests::Cx};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    struct Facts;
    impl OwnCx for Facts {
        fn function_has_requirement_row(&self, ty: Ty) -> bool {
            ty == Ty(43) || ty == Ty(45)
        }
        fn call_has_requirement_context(&self, c: &Callee) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(7))
                || matches!(c, Callee::TraitMethod { method, .. } if *method == DefId::from_raw(7))
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
    fn a_direct_default_or_awaited_call_uses_its_selected_signature_fact() {
        for tag in [Tag::Call, Tag::Await, Tag::DefaultCall] {
            for needed in [false, true] {
                for dynamic in [false, true] {
                    if dynamic && tag == Tag::DefaultCall {
                        continue;
                    }
                    let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
                    let block = builder.open_block();
                    let def = DefId::from_raw(if needed { 7 } else { 8 });
                    if tag == Tag::DefaultCall {
                        let call = builder.words_record(&[def.raw(), TyList::EMPTY.0]);
                        let args = builder.refs_record(&[]);
                        builder.emit(tag, call, args, Ty::I32, NodeIdx::NONE);
                    } else {
                        let callee = if dynamic {
                            Callee::TraitMethod {
                                trait_: DefId::from_raw(9),
                                method: def,
                                self_ty: Ty(46),
                                targs: TyList::EMPTY,
                                choice: (ChoiceKind::TraitValue, 0),
                            }
                        } else {
                            Callee::Item {
                                def,
                                targs: TyList::EMPTY,
                            }
                        };
                        builder.call(&callee, &[], Providers::None, Ty::I32, NodeIdx::NONE);
                    }
                    let root = builder.close_block(block, None, Ty::VOID, NodeIdx::NONE);
                    let mut b = builder.finish(root, &[]).unwrap();
                    b.tags[0] = tag;
                    b.anchors = vec![41; b.len()];
                    for _ in 0..2 {
                        state(&mut b);
                        let ds: Vec<_> = (0..b.len())
                            .filter(|&i| Info::read(&b, i).is_some_and(|c| c.part == Part::Row))
                            .collect();
                        assert_eq!(ds.len(), usize::from(needed) * 2);
                        for i in ds {
                            let target = b.data[i][0];
                            assert_eq!(b.tags[target as usize], tag);
                            assert!(count::adjacent(&b, &Topology::new(&b), target, u32_of(i)));
                            assert_eq!(b.anchors[i], b.anchors[target as usize]);
                            assert_eq!(
                                count::operation(
                                    &b,
                                    &Topology::new(&b),
                                    target,
                                    b.tags[i],
                                    Part::Row
                                ),
                                Info::read(&b, i),
                            );
                            if tag == Tag::Await {
                                assert!(
                                    count::operation(
                                        &b,
                                        &Topology::new(&b),
                                        target,
                                        Tag::Release,
                                        Part::Child,
                                    )
                                    .is_some()
                                );
                            }
                        }
                    }
                    let text = crate::ir::print(&b);
                    assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
                }
            }
        }
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
