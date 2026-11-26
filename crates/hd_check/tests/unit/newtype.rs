use hd_check::testing::{Checker, NewtypePlan, with_checker};
use hd_tir::ir::{Tag, TirSink, local_flags};
use hd_types::{AdtId, Ty, TyData, TyList};

/// A newtype `newtype` over `base`.
#[derive(Clone, Copy)]
struct Conversion {
    base: Ty,
    newtype: Ty,
}

impl Conversion {
    fn can(self, ck: &Checker<'_, '_, '_>, from: Ty, to: Ty) -> Option<NewtypePlan> {
        ck.newtype_plan((self.base, self.newtype), from, to)
    }
}

fn conversion(ck: &Checker<'_, '_, '_>) -> Conversion {
    Conversion {
        base: Ty::I32,
        newtype: ck.pool().intern_ty(&TyData::Adt(
            AdtId::Declared(ck.cx().names.item("app.main", "Wrapped")),
            TyList::EMPTY,
        )),
    }
}

fn nest(ck: &Checker<'_, '_, '_>, mut t: Ty, depth: usize, form: usize) -> Ty {
    let p = ck.pool();
    let k = ck.cx().names.known;
    for i in 0..depth {
        let (def, args) = match if form == 3 { i % 3 } else { form } {
            0 => (k.option, p.list(&[t])),
            1 => (k.result, p.list(&[t, Ty::BOOL])),
            _ => (k.list, p.list(&[t])),
        };
        t = p.intern_ty(&TyData::Adt(AdtId::Declared(def), args));
    }
    t
}

#[test]
fn newtype_conversion_queries_at_depth_1000_fit_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for this generated depth test.
        .spawn(|| {
            with_checker(|ck| {
                let conv = conversion(ck);
                for depth in [100, 1000] {
                    for form in 0..4 {
                        let a = nest(ck, conv.base, depth, form);
                        let b = nest(ck, conv.newtype, depth, form);
                        let bad = nest(ck, Ty::BOOL, depth, form);
                        for (from, to, expected) in [(a, b, true), (b, a, true), (a, bad, false)] {
                            let (answer, work) =
                                hd_types::work::count_type_nodes(|| conv.can(ck, from, to));
                            assert_eq!(answer.is_some(), expected);
                            assert_eq!(work, u64::try_from(depth + 1).unwrap());
                        }
                    }
                }
                let p = ck.pool();
                let k = ck.cx().names.known;
                let result =
                    |x, e| p.intern_ty(&TyData::Adt(AdtId::Declared(k.result), p.list(&[x, e])));
                assert!(
                    conv.can(
                        ck,
                        result(conv.base, Ty::I32),
                        result(conv.newtype, Ty::BOOL)
                    )
                    .is_none()
                );
                for data in [
                    TyData::Adt(AdtId::Product { rest: false }, p.list(&[conv.newtype])),
                    TyData::sum(p.list(&[conv.newtype])),
                    TyData::Ref {
                        inner: conv.newtype,
                        mutable: false,
                        counted: false,
                    },
                ] {
                    assert!(conv.can(ck, p.intern_ty(&data), Ty::I32).is_none());
                }
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn checked_conversions_can_become_identity_after_inference() {
    for depth in [0, 20] {
        for wrap in [false, true] {
            with_checker(|ck| {
                let p = ck.pool();
                let var = ck.infer().fresh(p, hd_types::VarKind::General);
                let conv = if wrap {
                    Conversion {
                        base: var,
                        newtype: Ty::I32,
                    }
                } else {
                    Conversion {
                        base: Ty::I32,
                        newtype: var,
                    }
                };
                let from = nest(ck, var, depth, 0);
                let to = nest(ck, Ty::I32, depth, 0);
                let plan = conv.can(ck, from, to).unwrap();
                ck.infer().unify(p, var, Ty::I32).unwrap();
                let node = ck.root();
                let root = ck.b().open_block();
                let name = ck.cx().names.syms.intern("input");
                let local = ck.b().local(from, name, local_flags::PARAM, node.index());
                let input = ck.b().get(local, from, node.index());
                let output = ck.apply_newtype_plan(input, plan).unwrap();
                assert_eq!(output, input);
                ck.b().close_block(root, Some(output), to, node.index());
                assert_eq!(ck.b().body().tags.len(), 2);
                assert_eq!(ck.b().body().tags[1], Tag::Block);
            });
        }
    }
}

#[test]
fn applying_newtype_conversions_at_depth_1000_fits_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for this generated depth test.
        .spawn(|| {
            for depth in [100, 1000] {
                for form in 0..4 {
                    for wrap in [false, true] {
                        with_checker(|ck| {
                            let conv = conversion(ck);
                            let (from, to) = if wrap {
                                (conv.base, conv.newtype)
                            } else {
                                (conv.newtype, conv.base)
                            };
                            let (from, to) =
                                (nest(ck, from, depth, form), nest(ck, to, depth, form));
                            let node = ck.root();
                            let root = ck.b().open_block();
                            let sym = ck.cx().names.syms.intern("input");
                            let local = ck.b().local(from, sym, local_flags::PARAM, node.index());
                            let input = ck.b().get(local, from, node.index());
                            let plan = conv.can(ck, from, to).unwrap();
                            let (out, work) = hd_types::work::count_type_nodes(|| {
                                ck.apply_newtype_plan(input, plan).unwrap()
                            });
                            assert_eq!(work, u64::try_from(depth + 1).unwrap());
                            ck.b().close_block(root, Some(out), to, node.index());
                            let body = ck.b().body();
                            let count = |tag| body.tags.iter().filter(|&&t| t == tag).count();
                            let lists = if form == 2 {
                                depth
                            } else if form == 3 {
                                depth / 3
                            } else {
                                0
                            };
                            assert_eq!(count(Tag::Loop), lists);
                            assert_eq!(count(Tag::SwitchTag), depth - lists);
                            assert_eq!(count(Tag::NewVariant), 2 * (depth - lists));
                            assert_eq!(body.local_ty.len(), 1 + 4 * lists);
                            assert_eq!(count(Tag::NewStruct), usize::from(wrap));
                            assert!(body.tags.len() < 40 * depth + 10, "emitted work is linear");
                        });
                    }
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
