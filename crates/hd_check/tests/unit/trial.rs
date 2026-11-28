use hd_base::{DefId, ModuleId, Stage};
use hd_check::testing::{candidate_table, with_checker};
use hd_types::solver::{Candidate, ImplOrigin, ImplRef, ImplTable};
use hd_types::{Ty, TyList};

fn table() -> ImplTable {
    ImplTable {
        def: vec![DefId::from_raw(3), DefId::from_raw(7)],
        plan: vec![Vec::new(), Vec::new()],
        origin: vec![ImplOrigin::Written, ImplOrigin::UnionTemplate],
        ..ImplTable::default()
    }
}

#[test]
fn candidate_rows_borrow_the_exact_table() {
    let table = table();
    for row in 0..2 {
        let selected = candidate_table(Some(&table), row).expect("complete row");
        assert!(std::ptr::eq(selected, &raw const table));
        assert_eq!(selected.def[row], table.def[row]);
        assert_eq!(selected.origin[row], table.origin[row]);
    }
}

#[test]
fn missing_tables_and_incomplete_candidate_columns_are_fallible() {
    let mut missing_def = table();
    missing_def.def.pop();
    let mut missing_plan = table();
    missing_plan.plan.pop();
    let mut missing_origin = table();
    missing_origin.origin.pop();
    let empty = ImplTable::default();
    let full = table();
    for (selected, row, needed) in [
        (None, 0, "registered impl table"),
        (Some(&empty), 0, "complete impl row"),
        (Some(&full), 2, "complete impl row"),
        (Some(&full), usize::MAX, "complete impl row"),
        (Some(&missing_def), 1, "complete impl row"),
        (Some(&missing_plan), 1, "complete impl row"),
        (Some(&missing_origin), 1, "complete impl row"),
    ] {
        let error = candidate_table(selected, row).expect_err("missing metadata");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
        assert_eq!(error.what, format!("solver candidate requires {needed}"));
    }
}

#[test]
fn a_missing_candidate_table_propagates_before_call_emission() {
    with_checker(|ck| {
        let candidate = Candidate {
            obligations: hd_types::solver::RegionObligations::default(),
            row: ImplRef {
                module: ModuleId::from_raw(0),
                row: 0,
            },
            n_fresh: 1,
            impl_args: ck.pool().list(&[Ty::I32]),
            args: TyList::EMPTY,
            residual: Vec::new(),
        };
        let vars_before = ck.infer().var_count();
        let before = (
            ck.b().body().len(),
            ck.b().body().extra.len(),
            ck.b().body().local_ty.len(),
        );
        let error = ck
            .call_candidate(
                (DefId::from_raw(1), DefId::from_raw(2), Ty::I32),
                &candidate,
            )
            .expect_err("missing module table");
        assert_eq!(ck.infer().var_count(), vars_before);
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert_eq!(
            error.what,
            "solver candidate requires registered impl table"
        );
        assert_eq!(
            (
                ck.b().body().len(),
                ck.b().body().extra.len(),
                ck.b().body().local_ty.len()
            ),
            before
        );
    });
}
