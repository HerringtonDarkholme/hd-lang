use hd_base::DefId;
use hd_check::testing::{KeyCandidates, key_is, with_checker};
use hd_types::{InternPool, Ty, TyData, TyList};

#[test]
fn nested_rows_pair_fixed_members_without_rewriting_rests() {
    with_checker(|ck| {
        let pool = ck.pool();
        for (fixed, rest) in [(true, false), (false, true), (true, true)] {
            let var = ck.infer().fresh(pool, hd_types::unify::VarKind::General);
            let row = pool.row(&hd_types::RowData {
                keys: vec![var],
                params: vec![],
            });
            let function = |row| {
                pool.intern_ty(&TyData::Fn {
                    params: TyList::EMPTY,
                    result: Ty::I32,
                    row,
                    suspends: false,
                    inputs: hd_types::Inputs::Fixed,
                    deps: hd_types::FnDeps::NONE,
                })
            };
            let left = function(row);
            let right = function(hd_types::RowId::EMPTY);
            let (left_members, right_members) = ([left], [right]);
            let got = pool.product(
                pool.list(if fixed { &left_members } else { &[] }),
                rest.then_some(left),
            );
            let want = pool.product(
                pool.list(if fixed { &right_members } else { &[] }),
                Some(right),
            );
            ck.solve_nested_rows(got, want, 0);
            assert_eq!(
                ck.infer().resolve_row(pool, row) == hd_types::RowId::EMPTY,
                fixed
            );
        }
    });
}

#[test]
fn candidate_binding_index_preserves_the_first_matching_supplier() {
    let pool = InternPool::new();
    let make = |def, bindings| {
        pool.intern_ty(&TyData::TraitValue {
            def: DefId::from_raw(def),
            args: TyList::EMPTY,
            bindings,
        })
    };
    let a = DefId::from_raw(50);
    let b = DefId::from_raw(51);
    let first = make(20, vec![(a, Ty::I32), (b, Ty::STRING)]);
    let second = make(20, vec![(a, Ty::I32), (b, Ty::I32)]);
    let h1 = make(21, vec![]);
    let h2 = make(22, vec![]);
    let plain = make(20, vec![]);
    let cases = [
        vec![(first, first), (h2, second), (h1, first)],
        vec![(h1, first), (h2, second), (plain, plain)],
        vec![(plain, plain), (h2, second), (h1, first)],
    ];
    let wants = [
        plain,
        first,
        second,
        make(20, vec![(a, Ty::I32)]),
        make(20, vec![(a, Ty::STRING)]),
        make(20, vec![(b, Ty::STRING)]),
    ];
    for values in cases {
        let mut index = KeyCandidates::default();
        for &(have, inst) in &values {
            index.push(pool.types(), have, inst);
        }
        for &want in &wants {
            let expected = values
                .iter()
                .find_map(|&(have, inst)| key_is(pool.types(), have, inst, want).then_some(have));
            assert_eq!(
                index.supplier(pool.types(), want),
                expected,
                "{values:?}, {want:?}"
            );
        }
    }
}
