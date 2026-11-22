use hd_base::{DefId, Hash128};
use hd_mono::layout::{CanonMemo, canon};
use hd_types::{AdtId, InternPool, Ty, TyData, TyList};

// Frozen from the old tuple encoding before the representation migration.
#[test]
fn anonymous_products_preserve_tuple_digests() {
    let pool = InternPool::default();
    let path = |def: DefId| Hash128(u128::from(def.raw()));
    let mut memo = CanonMemo::default();
    for (name, elems, rest, expected) in [
        (
            "unit",
            TyList::EMPTY,
            None,
            17_974_520_238_679_138_432_039_580_537_221_994_580,
        ),
        (
            "single",
            pool.list(&[Ty::I32]),
            None,
            221_190_193_953_494_153_513_107_123_744_364_509_761,
        ),
        (
            "pair",
            pool.list(&[Ty::I32, Ty::STRING]),
            None,
            3_068_084_359_075_055_137_526_921_028_268_861_148,
        ),
        (
            "rest",
            pool.list(&[Ty::I32]),
            Some(Ty::STRING),
            144_233_481_973_758_477_859_635_119_758_774_272_015,
        ),
        (
            "rest-only",
            TyList::EMPTY,
            Some(Ty::STRING),
            121_689_676_849_439_223_114_246_548_249_636_043_955,
        ),
    ] {
        let product = pool.product(elems, rest);
        let b = canon(&pool, &path, &mut memo, product);
        assert_eq!(Hash128(expected), b, "{name}: pre-shared-ADT tuple digest");
        let sum = pool.intern_ty(&TyData::Adt(
            AdtId::Sum,
            match rest {
                None => elems,
                Some(rest) => pool.list(
                    &pool
                        .list_items(elems)
                        .iter()
                        .copied()
                        .chain([rest])
                        .collect::<Vec<_>>(),
                ),
            },
        ));
        assert_ne!(b, canon(&pool, &path, &mut memo, sum));
    }
}
