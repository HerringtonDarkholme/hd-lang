use hd_base::Stage;
use hd_check::error::error_bodies;
use hd_check::testing::with_checker;
use hd_diag::DiagBuf;
use hd_resolve::error_type::ErrorShape;
use hd_types::{Ty, TyData, TyList};

#[test]
fn missing_target_metadata_returns_before_generating_bodies() {
    with_checker(|ck| {
        let shape = ErrorShape {
            line: ck.root(),
            is_enum: false,
            variants: Vec::new(),
        };
        let mut diags = DiagBuf::default();
        let error = error_bodies(ck.cx(), hd_base::DefId::from_raw(1), &shape, &mut diags)
            .expect_err("missing resolved declaration");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
        assert_eq!(
            error.what,
            "error derivation requires resolved @error target"
        );
        assert!(diags.is_empty());
    });
}

#[test]
fn malformed_source_result_propagates_without_emitting_an_arm() {
    with_checker(|ck| {
        let node = ck.root();
        let shape = ErrorShape {
            line: node,
            is_enum: false,
            variants: vec![hd_resolve::error_type::ErrVariant {
                node,
                message: hd_resolve::error_type::Message::Absent,
                members: Vec::new(),
            }],
        };
        let before = (ck.b().body().len(), ck.b().body().extra.len());
        let error = ck
            .error_cause_arm(
                (hd_base::DefId::from_raw(1), &shape, vec![Vec::new()]),
                (hd_base::LocalId::from_raw(0), Ty::I32),
                0,
                Ty::I32,
            )
            .expect_err("malformed source signature");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert_eq!(
            error.what,
            "error derivation requires optional Error.source result"
        );
        assert_eq!((ck.b().body().len(), ck.b().body().extra.len()), before);
    });
}

#[test]
fn optional_source_payload_is_preserved_and_malformed_metadata_is_fallible() {
    with_checker(|ck| {
        let pool = ck.cx().names.pool;
        let option = ck.cx().names.known.option;
        let erased = pool.intern_ty(&TyData::TraitValue {
            def: ck.cx().names.known.error,
            args: TyList::EMPTY,
            bindings: Vec::new(),
        });
        let reference = pool.intern_ty(&TyData::Ref {
            inner: erased,
            mutable: false,
            counted: false,
        });
        for payload in [Ty::I32, Ty::STRING, ck.optional(Ty::STRING), reference] {
            assert_eq!(
                ck.error_source_payload(ck.optional(payload))
                    .expect("optional result"),
                payload,
            );
        }
        let no_args = pool.intern_ty(&TyData::Adt(
            hd_types::AdtId::Declared(option),
            TyList::EMPTY,
        ));
        let two_args = pool.intern_ty(&TyData::Adt(
            hd_types::AdtId::Declared(option),
            pool.list(&[Ty::I32, Ty::STRING]),
        ));
        let foreign = pool.applied(ck.cx().names.known.string_builder, Ty::I32);
        let before = (ck.b().body().len(), ck.b().body().extra.len());
        for ret in [Ty::I32, Ty::POISON, no_args, two_args, foreign] {
            let error = ck
                .error_source_payload(ret)
                .expect_err("non-optional result");
            assert_eq!(error.stage, Stage::Body);
            assert!(error.bug);
            assert!(!error.out_of_fuel);
            assert_eq!(
                error.what,
                "error derivation requires optional Error.source result"
            );
        }
        assert_eq!((ck.b().body().len(), ck.b().body().extra.len()), before);
    });
}
