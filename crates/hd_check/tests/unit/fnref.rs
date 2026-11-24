use hd_base::{DefId, Stage};
use hd_check::testing::{Member, Owner, with_checker};
use hd_resolve::FnSig;
use hd_tir::ir::ChoiceKind;
use hd_types::{Inputs, RowId, Ty, TyData};

#[test]
fn retained_function_parts_match_the_interned_shape() {
    with_checker(|ck| {
        let list = ck.pool().applied(ck.cx().names.known.list, Ty::STRING);
        for skip in [0, 1] {
            for variadic in [false, true] {
                for suspends in [false, true] {
                    let params = vec![
                        (ck.cx().names.syms.intern("self"), Ty::I32),
                        (ck.cx().names.syms.intern("rest"), list),
                    ];
                    let mut sig = FnSig::simple(Vec::new(), params, Ty::BOOL);
                    sig.variadic = variadic;
                    sig.suspends = suspends;
                    let member = Member {
                        def: DefId::from_raw(1),
                        sig,
                        vars: Vec::new(),
                        owner: Owner::Item,
                    };
                    let function = ck.member_fn_ty(&member, skip).expect("function metadata");
                    assert_eq!(
                        ck.pool().list_items(function.params),
                        &[Ty::I32, list][skip..]
                    );
                    assert_eq!(function.result, Ty::BOOL);
                    assert_eq!(function.row, RowId::EMPTY);
                    assert_eq!(function.suspends, suspends);
                    assert_eq!(
                        function.inputs,
                        if variadic {
                            Inputs::Rest
                        } else {
                            Inputs::Fixed
                        }
                    );
                    assert_eq!(
                        ck.pool().get(function.ty),
                        TyData::Fn {
                            params: function.params,
                            result: function.result,
                            row: function.row,
                            suspends: function.suspends,
                            inputs: function.inputs,
                            deps: ck.sig_fn_deps(&member.sig, skip),
                        }
                    );
                }
            }
        }
    });
}

#[test]
fn trait_parts_preserve_arguments_and_current_evidence() {
    let def = DefId::from_raw(3);
    for choice in [None, Some((ChoiceKind::TraitValue, 7))] {
        let mut owner = Owner::Trait {
            def,
            self_ty: Ty::I32,
            args: vec![Ty::STRING, Ty::BOOL],
            choice,
        };
        let (selected, args, evidence) = owner.trait_parts().expect("trait owner");
        assert_eq!(selected, def);
        assert_eq!(args, [Ty::STRING, Ty::BOOL]);
        assert_eq!(evidence, choice);
        if let Owner::Trait { choice, .. } = &mut owner {
            *choice = Some((ChoiceKind::Impl, 11));
        }
        assert_eq!(
            owner.trait_parts().expect("updated choice").2,
            Some((ChoiceKind::Impl, 11))
        );
    }
    for owner in [
        Owner::Item,
        Owner::Impl {
            def,
            args: Vec::new(),
        },
    ] {
        let error = owner.trait_parts().expect_err("nontrait owner");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
    }
}

#[test]
fn dynamic_nontrait_selection_fails_before_emission() {
    with_checker(|ck| {
        let before = (
            ck.b().body().len(),
            ck.b().body().extra.len(),
            ck.b().body().local_ty.len(),
        );
        for owner in [
            Owner::Item,
            Owner::Impl {
                def: DefId::from_raw(1),
                args: Vec::new(),
            },
        ] {
            let member = Member {
                def: DefId::from_raw(2),
                sig: FnSig::simple(Vec::new(), Vec::new(), Ty::VOID),
                vars: Vec::new(),
                owner,
            };
            let error = ck
                .dynamic_member_ref(member, (Ty::I32, Ty::I32, false))
                .expect_err("invalid dynamic owner");
            assert_eq!(error.stage, Stage::Body);
            assert!(error.bug);
            assert_eq!(
                error.what,
                "dynamic member reference requires a trait owner"
            );
            assert_eq!(
                (
                    ck.b().body().len(),
                    ck.b().body().extra.len(),
                    ck.b().body().local_ty.len()
                ),
                before
            );
        }
    });
}
