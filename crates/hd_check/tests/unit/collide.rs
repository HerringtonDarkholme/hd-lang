use hd_check::testing::{can_coincide, mentions_param};
use hd_types::{ParamRef, Ty, TyData};
#[test]
fn collision_conversions_keep_bindings_and_occurs_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(96 << 10)
        // cost: one 96 KiB thread for the generated chains.
        .spawn(|| {
            let global = hd_types::InternPool::new();
            let pool = global.types();
            let def = hd_base::DefId::from_raw(1);
            let param = ParamRef {
                owner: def,
                index: 0,
            };
            let ty = pool.intern_ty(&TyData::Param(param));
            let (mut left, mut right) = (ty, Ty::I32);
            for _ in 0..1_000 {
                left = pool.applied(def, left);
                right = pool.applied(def, right);
            }
            let mut subst = Vec::new();
            assert!(can_coincide(pool, &mut subst, left, right));
            assert_eq!(subst, [(param, Ty::I32)]);
            assert!(mentions_param(pool, &[], left, param));
            assert!(!can_coincide(pool, &mut Vec::new(), ty, left));
        })
        .unwrap()
        .join()
        .unwrap();
}
