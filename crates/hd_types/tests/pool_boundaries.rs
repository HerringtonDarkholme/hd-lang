//! Public type-pool queries and mapping contracts; no malformed pool IDs.
use hd_base::DefId;
use hd_types::{
    ConstValue, InferTable, InternPool, LocalPool, ParamRef, RowData, RowId, Ty, TyData, TyList,
    TyMap, VarKind, with_assoc_args,
};

#[test]
fn dependency_queries_and_referent_queries_keep_their_domains() {
    let pool = InternPool::new();
    let types = pool.types();
    assert_eq!(types.fn_ty_deps(Ty::I32), None);
    let tuple = types.product(types.list(&[Ty::I32, Ty::STRING]), None);
    assert_eq!(types.fn_ty_deps(tuple), None);
    for counted in [false, true] {
        for mutable in [false, true] {
            let reference = types.intern_ty(&TyData::Ref {
                inner: tuple,
                mutable,
                counted,
            });
            assert_eq!(types.ref_inner(reference), tuple);
            assert_eq!(types.fn_ty_deps(reference), None);
        }
    }
}

struct RowMap {
    types: Vec<Ty>,
    rows: Vec<RowId>,
    replacement: RowId,
}

impl TyMap for RowMap {
    fn ty(&mut self, t: Ty) -> Ty {
        self.types.push(t);
        t
    }
    fn row(&mut self, r: RowId) -> RowId {
        self.rows.push(r);
        self.replacement
    }
}

#[test]
fn child_mapping_treats_row_and_context_as_one_row_child() {
    let pool = InternPool::new();
    let types = pool.types();
    let key = |raw| {
        types.intern_ty(&TyData::TraitValue {
            def: DefId::from_raw(raw),
            args: TyList::EMPTY,
            bindings: vec![],
        })
    };
    let row = types.row(&RowData {
        keys: vec![key(1)],
        params: vec![],
    });
    let replacement = types.row(&RowData {
        keys: vec![key(2)],
        params: vec![],
    });
    for constructor in [TyData::Row, TyData::Context] {
        let before = types.intern_ty(&constructor(row));
        let expected = types.intern_ty(&constructor(replacement));
        let mut map = RowMap {
            types: vec![],
            rows: vec![],
            replacement,
        };
        assert_eq!(types.map_children(before, &mut map), expected);
        assert_eq!(map.rows, [row]);
        assert!(
            map.types.is_empty(),
            "the row mapper owns traversal of its keys"
        );
    }
}

#[test]
fn child_mapping_preserves_every_leaf_without_calling_the_mapper() {
    let pool = InternPool::new();
    let local = LocalPool::new();
    let types = hd_types::Types::with_local(&pool, &local);
    let mut infer = InferTable::default();
    let variable = infer.fresh(types, VarKind::General);
    let leaves = [
        Ty::I32,
        Ty::NEVER,
        Ty::POISON,
        variable,
        types.intern_ty(&TyData::Param(ParamRef {
            owner: DefId::from_raw(1),
            index: 0,
        })),
        types.intern_ty(&TyData::Const(ConstValue::Bool(true))),
        types.intern_ty(&TyData::Canon(0)),
    ];
    let mut map = RowMap {
        types: vec![],
        rows: vec![],
        replacement: RowId::EMPTY,
    };
    for leaf in leaves {
        assert_eq!(types.map_children(leaf, &mut map), leaf);
    }
    assert!(map.types.is_empty());
    assert!(map.rows.is_empty());
}

#[test]
fn associated_arguments_fill_union_members_and_preserve_written_arguments() {
    let pool = InternPool::new();
    let types = pool.types();
    let owner = DefId::from_raw(1);
    let other = DefId::from_raw(2);
    let this = types.intern_ty(&TyData::Param(ParamRef { owner, index: 0 }));
    let projection = |trait_, args| {
        types.intern_ty(&TyData::Assoc {
            assoc: DefId::from_raw(if trait_ == owner { 3 } else { 4 }),
            trait_,
            self_ty: this,
            args,
        })
    };
    let supplied = types.list(&[Ty::I32]);
    let written = types.list(&[Ty::STRING]);
    let union = |members: &[Ty]| types.intern_ty(&TyData::sum(types.list(members)));
    let untouched_other = projection(other, TyList::EMPTY);
    let explicit = projection(owner, written);
    let before = union(&[projection(owner, TyList::EMPTY), untouched_other, explicit]);
    let expected = union(&[projection(owner, supplied), untouched_other, explicit]);
    assert_eq!(with_assoc_args(types, before, owner, supplied), expected);
    assert_eq!(with_assoc_args(types, expected, owner, supplied), expected);
    assert_eq!(with_assoc_args(types, before, owner, TyList::EMPTY), before);
}
