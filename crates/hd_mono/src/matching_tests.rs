use super::{DefId, InternPool, ParamRef, Ty, TyData, unify, unify_walk};

fn param(pool: &InternPool, owner: DefId, index: u16) -> Ty {
    pool.intern_ty(&TyData::Param(ParamRef { owner, index }))
}

#[test]
fn impl_arguments_follow_union_member_positions() {
    let pool = InternPool::new();
    let owner = DefId::from_raw(1);
    let a = param(&pool, owner, 0);
    let b = param(&pool, owner, 1);
    let pattern = pool.intern_ty(&TyData::Union(pool.list(&[a, b])));
    let actual = pool.intern_ty(&TyData::Union(pool.list(&[Ty::STRING, Ty::I32])));
    let mut out = Vec::new();
    unify(&pool, owner, pattern, actual, &mut out);
    assert_eq!(out, vec![Some(Ty::STRING), Some(Ty::I32)]);
}

#[test]
fn impl_arguments_include_a_tuple_rest_without_a_fixed_occurrence() {
    let pool = InternPool::new();
    let owner = DefId::from_raw(1);
    let a = param(&pool, owner, 0);
    let list = DefId::from_raw(2);
    let rest = |item| {
        pool.intern_ty(&TyData::Adt {
            def: list,
            args: pool.list(&[item]),
        })
    };
    let tuple = |item| {
        pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&[Ty::I32]),
            rest: Some(rest(item)),
        })
    };
    let mut out = Vec::new();
    unify(&pool, owner, tuple(a), tuple(Ty::STRING), &mut out);
    assert_eq!(out, vec![Some(Ty::STRING)]);
}

#[test]
fn shared_impl_head_walk_counts_width_1000_and_depth_20() {
    let pool = InternPool::new();
    let owner = DefId::from_raw(1);
    let mut pattern = param(&pool, owner, 0);
    let mut actual = Ty::STRING;
    let tuple = |members: &[Ty]| {
        pool.intern_ty(&TyData::Tuple {
            elems: pool.list(members),
            rest: None,
        })
    };
    for _ in 0..20 {
        pattern = tuple(&[pattern, pattern]);
        actual = tuple(&[actual, actual]);
    }
    pattern = tuple(&vec![pattern; 1000]);
    actual = tuple(&vec![actual; 1000]);
    let mut out = Vec::new();
    let (pairs, popped) = unify_walk(&pool, owner, pattern, actual, &mut out);
    assert_eq!(out, vec![Some(Ty::STRING)]);
    assert_eq!(
        pairs, 22,
        "one root, twenty shared tuple pairs, one parameter"
    );
    assert_eq!(
        popped, 1041,
        "one root, 1000 children, two edges at each of twenty levels"
    );
}
