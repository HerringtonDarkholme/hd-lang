use hd_check::testing::{EscapeMemo, escape_in, hidden, holds_local_ref, lends};
use hd_types::{InternPool, Ty, TyData};

/// `lends` compares two types on its own stack: a pair of
/// types 1,000 levels deep, counted references against local ones at
/// each level, converts on a 96 KiB thread, and a pair that differs
/// only at the bottom does not. `hidden` finds a type
/// parameter, and `escape_in` a local reference, at the same
/// depth.
#[test]
fn walks_at_the_type_depth_fit_a_small_stack() {
    std::thread::Builder::new()
        // cost: one 96 KiB thread for this test.
        .stack_size(96 << 10)
        .spawn(|| {
            let pool = InternPool::new();
            let types = pool.types();
            let rf = |inner, counted| {
                pool.intern_ty(&TyData::Ref {
                    inner,
                    mutable: false,
                    counted,
                })
            };
            let pair = |a, b| pool.product(pool.list(&[a, b]), None);
            let (mut counted, mut local, mut wrong) = (Ty::I32, Ty::I32, rf(Ty::I32, false));
            for _ in 1..hd_base::limits::TYPE_DEPTH {
                counted = pair(rf(Ty::I32, true), counted);
                local = pair(rf(Ty::I32, false), local);
                wrong = pair(rf(Ty::I32, false), wrong);
            }
            assert!(lends(types, counted, local, 1));
            assert!(!lends(types, wrong, local, 1));
            // The same depth under a function type's parameter names
            // the type parameter in a position; beside it, it does not.
            let p = hd_types::ParamRef {
                owner: hd_base::DefId::from_raw(1),
                index: 0,
            };
            let mut deep = pool.intern_ty(&TyData::Param(p));
            for _ in 2..hd_base::limits::TYPE_DEPTH {
                deep = pair(rf(Ty::I32, false), deep);
            }
            let f = pool.intern_ty(&TyData::Fn {
                params: pool.list(&[deep]),
                result: Ty::I32,
                row: hd_types::RowId::EMPTY,
                suspends: false,
                inputs: hd_types::Inputs::Fixed,
                deps: hd_types::FnDeps::NONE,
            });
            assert!(hidden(types, f, p));
            assert!(!hidden(types, deep, p));
            // A local reference at the bottom of 1,000 levels of
            // tuples is held, and at the top it is no escape.
            let mut bottom = rf(Ty::I32, false);
            for _ in 1..hd_base::limits::TYPE_DEPTH {
                bottom = pair(Ty::I32, bottom);
            }
            assert!(holds_local_ref(types, bottom));
            let mut memo = EscapeMemo::default();
            assert!(escape_in(types, bottom, true, &mut memo).is_none());
        })
        .expect("thread")
        .join()
        .expect("no stack overflow");
}
