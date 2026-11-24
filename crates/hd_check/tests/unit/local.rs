use hd_base::{LocalId, Stage};
use hd_check::testing::{LocalGeneric, named_closure_binding, with_checker};
use hd_types::Ty;

#[test]
fn named_binding_preserves_identity_and_rejects_absence() {
    for local in [0, 1, 1_000] {
        let local = LocalId::from_raw(local);
        assert_eq!(
            named_closure_binding(Some(local)).expect("named binding"),
            local
        );
    }
    let error = named_closure_binding(None).expect_err("absent binding");
    assert_eq!(error.stage, Stage::Body);
    assert!(error.bug);
    assert!(!error.out_of_fuel);
    assert_eq!(error.what, "local closure requires named closure binding");
}

#[test]
fn generic_entry_is_borrowed_and_missing_entry_changes_no_state() {
    with_checker(|ck| {
        let local = LocalId::from_raw(3);
        let missing = LocalId::from_raw(4);
        ck.local_generics().insert(
            local,
            LocalGeneric {
                sub: 11,
                first: 7,
                params: vec![Ty::I32],
                generics: Vec::new(),
                fn_ty: Ty::POISON,
                const_inputs: None,
                caps: None,
            },
        );
        let before = (
            ck.b().body().len(),
            ck.b().body().extra.len(),
            ck.b().body().local_ty.len(),
        );
        let entry = ck.local_generic_entry(local).expect("registered entry");
        assert_eq!(entry.sub, 11);
        assert_eq!(entry.first, 7);
        assert_eq!(entry.params, [Ty::I32]);
        entry.fn_ty = Ty::STRING;
        entry.caps = Some(vec![LocalId::from_raw(9)]);
        let original = ck.local_generics().get(&local).expect("same entry");
        assert_eq!(original.fn_ty, Ty::STRING);
        assert_eq!(original.caps, Some(vec![LocalId::from_raw(9)]));
        let error = ck.local_generic_entry(missing).expect_err("absent entry");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
        assert_eq!(
            error.what,
            "local closure requires registered generic entry"
        );
        assert_eq!(ck.local_generics().len(), 1);
        assert!(!ck.local_generics().contains_key(&missing));
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
