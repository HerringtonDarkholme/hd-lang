//! Unification contracts for rest tuples and inferred data-row arguments.
use hd_base::DefId;
use hd_types::{
    InferTable, InternPool, LocalPool, RowData, RowId, Ty, TyData, TyList, Types, UnifyError,
    VarKind,
};

fn reference(p: Types<'_>, inner: Ty, mutable: bool) -> Ty {
    p.intern_ty(&TyData::Ref {
        inner,
        mutable,
        counted: false,
    })
}

fn rest_tuple(p: Types<'_>, fixed: Ty, item: Ty) -> Ty {
    let list = p.applied(DefId::from_raw(1), item);
    p.product(p.list(&[fixed]), Some(list))
}

fn key(p: Types<'_>, raw: u32) -> Ty {
    p.intern_ty(&TyData::TraitValue {
        def: DefId::from_raw(raw),
        args: TyList::EMPTY,
        bindings: vec![],
    })
}

fn row(p: Types<'_>, keys: &[Ty]) -> RowId {
    p.row(&RowData {
        keys: keys.to_vec(),
        params: vec![],
    })
}

#[test]
fn tuple_rest_joins_infer_both_parts_and_ignore_only_site_permissions() {
    let (global, local) = (InternPool::new(), LocalPool::new());
    let p = Types::with_local(&global, &local);
    let mut infer = InferTable::default();
    let fixed = infer.fresh(p, VarKind::General);
    let rest = infer.fresh(p, VarKind::General);
    let before = rest_tuple(p, reference(p, fixed, true), rest);
    let after = rest_tuple(p, reference(p, Ty::I32, false), Ty::BOOL);
    assert_eq!(infer.unify_join(p, before, after), Ok(()));
    assert_eq!(infer.resolve(p, fixed), Ty::I32);
    assert_eq!(infer.resolve(p, rest), Ty::BOOL);
    // Joining permissions does not rewrite either input's reference kind.
    assert_eq!(
        infer.resolve(p, before),
        rest_tuple(p, reference(p, Ty::I32, true), Ty::BOOL)
    );
    assert_eq!(infer.resolve(p, after), after);
}

#[test]
fn tuple_rest_joins_keep_generic_arguments_invariant_and_rest_shape_equal() {
    let (global, local) = (InternPool::new(), LocalPool::new());
    let p = Types::with_local(&global, &local);
    let mut infer = InferTable::default();
    let mutable = reference(p, Ty::I32, true);
    let readonly = reference(p, Ty::I32, false);
    assert!(matches!(
        infer.unify_join(p, rest_tuple(p, mutable, mutable), rest_tuple(p, readonly, readonly)),
        Err(UnifyError::Mismatch { expected, found }) if expected == mutable && found == readonly
    ));
    let with_rest = rest_tuple(p, Ty::I32, Ty::BOOL);
    let plain = p.product(p.list(&[Ty::I32]), None);
    for (a, b) in [(plain, with_rest), (with_rest, plain)] {
        assert_eq!(
            infer.unify_join(p, a, b),
            Err(UnifyError::Mismatch {
                expected: a,
                found: b
            })
        );
    }
}

#[test]
fn data_row_inference_binds_the_whole_row_in_both_orders_and_rolls_back() {
    let (global, local) = (InternPool::new(), LocalPool::new());
    let p = Types::with_local(&global, &local);
    let first = row(p, &[key(p, 2), key(p, 3)]);
    let second = row(p, &[key(p, 4)]);
    for reverse in [false, true] {
        let mut infer = InferTable::default();
        let variable = infer.fresh(p, VarKind::General);
        let open = row(p, &[variable]);
        let open_ty = p.intern_ty(&TyData::Row(open));
        let first_ty = p.intern_ty(&TyData::Row(first));
        let snapshot = infer.snapshot();
        let (a, b) = if reverse {
            (first_ty, open_ty)
        } else {
            (open_ty, first_ty)
        };
        assert_eq!(infer.unify(p, a, b), Ok(()));
        assert_eq!(
            infer.resolve(p, variable),
            first_ty,
            "the variable denotes a row, not a single key"
        );
        assert_eq!(infer.resolve_row(p, open), first);
        assert_eq!(
            infer.resolve_row(p, open),
            first,
            "warm row resolution agrees"
        );
        infer.rollback(snapshot);
        assert_eq!(infer.shallow(p, variable), variable);
        assert_eq!(infer.resolve_row(p, open), open);
        let second_ty = p.intern_ty(&TyData::Row(second));
        assert_eq!(infer.unify(p, open_ty, second_ty), Ok(()));
        assert_eq!(
            infer.resolve_row(p, open),
            second,
            "the old cached row must not survive rollback"
        );
    }
}

#[test]
fn data_row_equality_follows_an_already_bound_row_variable() {
    let (global, local) = (InternPool::new(), LocalPool::new());
    let p = Types::with_local(&global, &local);
    let mut infer = InferTable::default();
    let variable = infer.fresh(p, VarKind::General);
    let written = p.intern_ty(&TyData::Row(row(p, &[variable])));
    let closed = p.intern_ty(&TyData::Row(row(p, &[key(p, 2)])));
    assert_ne!(written, closed);
    assert_eq!(infer.unify(p, variable, closed), Ok(()));
    assert_eq!(infer.unify(p, written, closed), Ok(()));
    assert_eq!(infer.unify(p, closed, written), Ok(()));
}

#[test]
fn unequal_closed_data_rows_reject_without_changing_the_input_rows() {
    let global = InternPool::new();
    let p = global.types();
    let a = p.intern_ty(&TyData::Row(row(p, &[key(p, 2)])));
    let b = p.intern_ty(&TyData::Row(row(p, &[key(p, 3)])));
    let empty = p.intern_ty(&TyData::Row(RowId::EMPTY));
    let mut infer = InferTable::default();
    for (left, right) in [(a, b), (b, a), (empty, a), (a, empty)] {
        assert_eq!(
            infer.unify(p, left, right),
            Err(UnifyError::Mismatch {
                expected: left,
                found: right
            })
        );
        assert_eq!(infer.resolve(p, left), left);
        assert_eq!(infer.resolve(p, right), right);
    }
    assert_eq!(infer.var_count(), 0);
}
