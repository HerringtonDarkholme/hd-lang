use hd_base::DefId;
use hd_check::testing::same_but_projections;
use hd_types::{Ty, TyData, TyList};

#[test]
fn nested_impl_projection_comparison_visits_shared_pairs_once() {
    let pool = hd_types::InternPool::new();
    let projection = pool.intern_ty(&TyData::Assoc {
        assoc: DefId::from_raw(1),
        trait_: DefId::from_raw(2),
        self_ty: Ty::I32,
        args: TyList::EMPTY,
    });
    let pair = |values: &[Ty]| pool.product(pool.list(values), None);
    let bound = |ty| (ty == projection).then_some(Ty::I32);
    let mut work = Vec::new();
    for width in [100, 1_000] {
        let (mut want, mut got) = (Ty::I32, projection);
        for _ in 0..20 {
            want = pair(&[want, want]);
            got = pair(&[got, got]);
        }
        want = pair(&vec![want; width]);
        got = pair(&vec![got; width]);
        let (same, visits) = same_but_projections(pool.types(), (got, want), &bound);
        assert!(same);
        work.push(visits);
    }
    assert!(work[0] > 100);
    assert!(work[1] <= work[0] * 10);
    assert!(!same_but_projections(pool.types(), (Ty::BOOL, projection), &bound).0);
}
