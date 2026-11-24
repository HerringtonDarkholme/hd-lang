mod representation_conversion_tests {
    use hd_check::testing::with_checker;
    use hd_types::{Ty, TyData};

    #[test]
    fn representation_conversion_keeps_the_depth_rule_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for generated function-result chains.
            .spawn(|| {
                with_checker(|ck| {
                    let p = ck.pool();
                    for depth in [20, 1_000] {
                        let (mut got, mut want) = (Ty::I32, ck.optional(Ty::I32));
                        let fun = |result| {
                            p.intern_ty(&TyData::Fn {
                                params: hd_types::TyList::EMPTY,
                                result,
                                row: hd_types::RowId::EMPTY,
                                suspends: false,
                                inputs: hd_types::Inputs::Fixed,
                                deps: hd_types::FnDeps::NONE,
                            })
                        };
                        for _ in 0..depth {
                            got = fun(got);
                            want = fun(want);
                        }
                        assert!(ck.repr_change(got, want, 0));
                        assert!(!ck.repr_change(got, want, hd_base::limits::TYPE_DEPTH + 1));
                    }
                });
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

mod source_function_tests {
    use hd_base::{DefId, Stage, Symbol};
    use hd_check::testing::source_function;
    use hd_resolve::{CallAttrs, FnAttrs, FnSig, Item, ItemData, Src};
    use hd_syntax::SyntaxKind;
    use hd_types::Ty;

    fn callable() -> Item {
        Item::new(
            DefId::from_raw(1),
            Symbol::from_raw(1),
            false,
            ItemData::Fn(FnSig::simple(vec![], vec![], Ty::I32), FnAttrs::default()),
        )
    }

    #[test]
    fn selected_input_borrows_its_resolved_header_signature_and_block() {
        let parsed = hd_syntax::parse(b"fn ready() -> i32:\n    42\n");
        let node = parsed
            .tree
            .root()
            .children()
            .find(|n| n.kind() == SyntaxKind::FnDecl)
            .expect("function node");
        let item = callable();
        let (header, sig, body) =
            source_function(Some(&item), node).expect("selected source function");
        assert!(std::ptr::eq(header, &raw const item));
        assert_eq!(sig.ret, Ty::I32);
        assert_eq!(body.kind(), SyntaxKind::Block);
        assert_eq!(
            Some(body.index()),
            Src::child(node, SyntaxKind::Block).map(hd_syntax::NodeRef::index)
        );
    }

    #[test]
    fn missing_or_non_callable_headers_report_the_selection_invariant() {
        let parsed = hd_syntax::parse(b"fn ready() -> i32:\n    42\n");
        let node = parsed
            .tree
            .root()
            .children()
            .find(|n| n.kind() == SyntaxKind::FnDecl)
            .expect("function node");
        let non_callable = Item::new(
            DefId::from_raw(1),
            Symbol::from_raw(1),
            false,
            ItemData::Builtin,
        );
        for header in [None, Some(&non_callable)] {
            let error = source_function(header, node)
                .err()
                .expect("invalid selection");
            assert_eq!(error.stage, Stage::Body);
            assert!(error.bug);
            assert!(!error.out_of_fuel);
            assert!(error.what.starts_with("selected source function:"));
        }
    }

    #[test]
    fn a_bodyless_trait_method_cannot_be_selected_as_a_source_body() {
        let parsed = hd_syntax::parse(b"trait Work:\n    fn act() -> void\n");
        let declaration = parsed
            .tree
            .root()
            .children()
            .find(|n| n.kind() == SyntaxKind::TraitDecl)
            .expect("trait");
        let block = Src::child(declaration, SyntaxKind::Block).expect("trait body");
        let method = block
            .children()
            .find(|n| n.kind() == SyntaxKind::FnDecl)
            .expect("method");
        assert!(Src::child(method, SyntaxKind::Block).is_none());
        let item = Item::new(
            DefId::from_raw(1),
            Symbol::from_raw(1),
            false,
            ItemData::Method {
                owner: DefId::from_raw(2),
                sig: FnSig::simple(vec![], vec![], Ty::VOID),
                has_body: false,
                call: CallAttrs::default(),
            },
        );
        let error = source_function(Some(&item), method)
            .err()
            .expect("bodyless selection");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
    }
}

mod proof_answer_tests {
    use hd_base::{DefId, InferVar, Stage};
    use hd_check::testing::{Proof, is_out_of_fuel, proof_answer};
    use hd_types::solver::{Answer, CanonGoal, Evidence, FailInfo, FailReason, GoalKind};
    use hd_types::{Ty, TyList};

    #[test]
    fn proof_success_moves_its_exact_evidence_and_learned_buffer() {
        for evidence in [
            Evidence::Poison,
            Evidence::TraitValue {
                trait_: DefId::from_raw(1),
            },
        ] {
            let expected = evidence.clone();
            let learned = vec![
                (InferVar::from_raw(7), Ty::I32),
                (InferVar::from_raw(11), Ty::STRING),
            ];
            let ptr = learned.as_ptr();
            let capacity = learned.capacity();
            let proof = proof_answer(Answer::Holds {
                evidence,
                learned,
                obligations: hd_types::solver::RegionObligations::default(),
            })
            .expect("proof");
            let Proof::Holds {
                evidence, learned, ..
            } = proof
            else {
                panic!("successful proof lost its outcome");
            };
            assert_eq!(evidence, expected);
            assert_eq!(learned.as_ptr(), ptr);
            assert_eq!(learned.capacity(), capacity);
            assert_eq!(
                learned,
                [
                    (InferVar::from_raw(7), Ty::I32),
                    (InferVar::from_raw(11), Ty::STRING)
                ]
            );
        }
    }

    #[test]
    fn failure_stall_and_overflow_keep_their_proof_outcomes() {
        let failure = Answer::Fails(Box::new(FailInfo {
            leaf: CanonGoal {
                kind: GoalKind::Implements,
                mut_: false,
                n_vars: 0,
                trait_: DefId::from_raw(1),
                self_ty: Ty::I32,
                args: TyList::EMPTY,
                extra: 0,
            },
            chain: Vec::new(),
            reason: FailReason::NoImpl,
            near: Vec::new(),
        }));
        assert!(matches!(proof_answer(failure), Ok(Proof::Fails)));
        assert!(matches!(
            proof_answer(Answer::Stalled {
                on: vec![InferVar::from_raw(3), InferVar::from_raw(5)],
            }),
            Ok(Proof::Stalled)
        ));
        assert!(matches!(
            proof_answer(Answer::Overflow),
            Ok(Proof::Overflow)
        ));
    }

    #[test]
    fn fuel_exhaustion_and_wrong_goal_answers_are_distinct_errors() {
        let fuel = proof_answer(Answer::OutOfFuel).expect_err("fuel exhaustion");
        assert!(is_out_of_fuel(&fuel));
        assert!(!fuel.bug);
        let error = proof_answer(Answer::Candidates(Vec::new())).expect_err("not a proof");
        assert_eq!(error.stage, Stage::Body);
        assert!(error.bug);
        assert!(!error.out_of_fuel);
        assert_eq!(
            error.what,
            "Implements proof requires a proof outcome, not candidates"
        );
    }
}
