use hd_base::{DefId, NodeIdx};
use hd_check::results::Results;
use hd_check::testing::{MAX_KEY_DEPTH, row_changed, row_key_depth};
use hd_types::{AdtId, RowData, RowId, Ty, TyData};

#[test]
fn row_key_depth_at_1000_fits_a_small_stack() {
    std::thread::Builder::new()
        // cost: one 96 KiB thread for this test.
        .stack_size(96 << 10)
        .spawn(|| {
            let pool = hd_types::InternPool::new();
            let types = pool.types();
            for nesting in [10, 20, 100, 1_000] {
                let mut t = Ty::I32;
                for _ in 0..nesting {
                    t = pool.product(pool.list(&[t, t]), None);
                }
                let (answer, work) =
                    hd_types::work::count_type_nodes(|| row_key_depth(types, t, 0));
                let expected = (nesting + 1).min(MAX_KEY_DEPTH + 1);
                assert_eq!(answer, expected);
                assert_eq!(work, u64::from(expected));
            }
            let mut shared = Ty::I32;
            for _ in 1..20 {
                shared = pool.product(pool.list(&[shared, shared]), None);
            }
            for width in [100, 1_000] {
                let branches: Vec<_> = (0..width)
                    .map(|i| {
                        pool.intern_ty(&TyData::Adt(
                            AdtId::Declared(DefId::from_raw(i)),
                            pool.list(&[shared]),
                        ))
                    })
                    .collect();
                let root = pool.product(pool.list(&branches), None);
                let (answer, work) =
                    hd_types::work::count_type_nodes(|| row_key_depth(types, root, 0));
                assert_eq!(answer, 22);
                assert_eq!(
                    work,
                    u64::from(width) + 21,
                    "shared depth-20 parts are walked once"
                );
            }
            let tuple = |ts: &[Ty]| pool.product(pool.list(ts), None);
            let reference = |inner| {
                pool.intern_ty(&TyData::Ref {
                    inner,
                    mutable: false,
                    counted: false,
                })
            };
            let shared = tuple(&[Ty::I32]);
            assert_eq!(
                row_key_depth(types, tuple(&[shared, reference(shared)]), 0),
                4,
                "a later deeper visit must extend the answer"
            );
            for at in [
                0,
                MAX_KEY_DEPTH - 1,
                MAX_KEY_DEPTH,
                MAX_KEY_DEPTH + 1,
                1_000,
            ] {
                assert_eq!(
                    row_key_depth(types, Ty::I32, at),
                    if at > MAX_KEY_DEPTH { at } else { at + 1 }
                );
            }
            let owner = DefId::from_raw(1);
            let adt = pool.intern_ty(&TyData::Adt(AdtId::Declared(owner), pool.list(&[shared])));
            let value = pool.intern_ty(&TyData::TraitValue {
                def: owner,
                args: pool.list(&[Ty::I32]),
                bindings: vec![(owner, shared)],
            });
            let function = pool.intern_ty(&TyData::Fn {
                params: pool.list(&[shared]),
                result: Ty::I32,
                row: pool.row(&RowData {
                    keys: vec![adt],
                    params: vec![],
                }),
                suspends: false,
                inputs: hd_types::Inputs::Fixed,
                deps: hd_types::FnDeps::NONE,
            });
            for t in [adt, value, function] {
                assert_eq!(row_key_depth(types, t, 0), 3);
            }
            let opaque = pool.intern_ty(&TyData::Opaque {
                owner,
                site: 0,
                borrows: false,
                args: pool.list(&[shared]),
                bound: shared,
            });
            let projection = pool.intern_ty(&TyData::Assoc {
                trait_: owner,
                assoc: owner,
                self_ty: shared,
                args: pool.list(&[shared]),
            });
            let union = pool.intern_ty(&TyData::sum(pool.list(&[shared, Ty::BOOL])));
            for t in [opaque, projection, union] {
                assert_eq!(
                    row_key_depth(types, t, 0),
                    1,
                    "the existing row-key boundary stays a leaf"
                );
            }
        })
        .expect("thread")
        .join()
        .expect("no stack overflow");
}

#[test]
fn an_empty_row_solution_keeps_the_initial_approximation() {
    let row_changed = std::hint::black_box(row_changed);
    assert!(!row_changed(None, Some(RowId::EMPTY)));
    assert!(!row_changed(Some(RowId::EMPTY), None));
    assert!(row_changed(None, Some(RowId(1))));
    assert!(row_changed(Some(RowId(1)), Some(RowId::EMPTY)));
}

#[test]
fn a_signature_change_visits_only_its_readers() {
    for unrelated in [100, 1000] {
        let mut r = Results::default();
        for i in 0..unrelated {
            let def = DefId::from_raw(i);
            let position = usize::try_from(i).expect("position");
            r.solved_row(def, NodeIdx::from_raw(i), position);
            r.read_signature(def, def);
        }
        let def = DefId::from_raw(2);
        r.read_signature(def, DefId::from_raw(3));
        assert_eq!(r.signature_changed(def), 2, "unrelated state: {unrelated}");
        assert_eq!(
            r.dirty().iter().map(|d| d.raw()).collect::<Vec<_>>(),
            [2, 3]
        );
        // Several signature updates before checking still schedule
        // each affected body once.
        assert_eq!(r.signature_changed(def), 2);
        assert_eq!(r.dirty().len(), 2);
    }
}

#[test]
fn result_cycles_visit_each_node_and_edge_once_and_drop_finished_groups() {
    for (width, depth) in [(1_000, 20), (10_000, 20), (1, 20_000)] {
        let n = u64::from(width * depth);
        assert_eq!(
            hd_check::testing::result_cycle_work(width, depth, false),
            (depth as usize, 3 * n - u64::from(width), 0),
        );
        assert_eq!(
            hd_check::testing::result_cycle_work(width, depth, true),
            (1, 2 * n, 0),
        );
    }
}
