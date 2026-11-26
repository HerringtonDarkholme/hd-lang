mod bounds;
mod constant;
mod empty_loops;
mod live_set;

mod origin_tests {
    use hd_wasm::testing::opt::live_set::Sets;
    use hd_wasm::testing::opt::*;

    use hd_base::Hash128;
    use hd_wasm::testing::module::{Func, Global, Ins, Sym, SymId, Symbols};
    use hd_wasm::testing::op::{LabelListId, Op as I};

    use hd_wasm::testing::module::{HEAP, Val, mem};
    use hd_wasm::testing::rt::Helper;
    use wasm_encoder::BlockType;

    fn batch_branch_shapes(f: &mut Func) -> bool {
        let sites = ifs(&f.body);
        branch_shapes::run(f, &sites)
    }

    #[test]
    fn an_ungenerated_helper_has_enough_shape_to_remove_fallthrough_controls() {
        let symbols = Symbols::default();
        let target = symbols.intern(Sym::Helper(Helper::NewFixed { size: 4 }));
        let descriptor = symbols.intern(Sym::Helper(Helper::FreeFrame));
        let mut function = Func::default();
        function.symbols = symbols.clone();
        function.body = vec![
            Ins::Op(I::Block(BlockType::Empty)),
            Ins::Op(I::I32Const(4)),
            Ins::Descriptor(
                descriptor,
                hd_wasm::testing::module::DescriptorKey::default(),
            ),
            Ins::Call(target, None),
            Ins::Op(I::Drop),
            Ins::Op(I::Br(0)),
            Ins::Op(I::End),
        ];
        let mut unknown = function.clone();
        blocks(&mut unknown, &vec![]);
        assert!(
            unknown
                .body
                .iter()
                .any(|instruction| matches!(instruction, Ins::Op(I::Br(0))))
        );
        let arities = symbols.call_arities(&[]);
        assert_eq!(arities[target.idx()], Some((2, 1)));
        assert!(blocks(&mut function, &arities));
        assert!(
            function
                .body
                .iter()
                .any(|instruction| matches!(instruction, Ins::Call(id, _) if *id == target))
        );
        assert!(
            !function
                .body
                .iter()
                .any(|instruction| matches!(instruction, Ins::Op(I::Br(_) | I::Block(_) | I::End)))
        );
    }

    fn call(symbols: &Symbols, symbol: Sym) -> Ins {
        Ins::Call(symbols.intern(symbol), None)
    }

    fn tracked(symbols: &Symbols, params: usize, locals: usize, body: Vec<Ins>) -> Func {
        let mut f = hd_wasm::testing::module::Module::default().func(
            "origins",
            vec![Val::I32; params],
            vec![],
        );
        f.symbols = symbols.clone();
        f.locals = vec![Val::I32; locals];
        f.origins = Some(
            (0..body.len())
                .map(|i| Some(10 + u32::try_from(i).unwrap()))
                .collect(),
        );
        f.body = body;
        f
    }

    fn run(mut f: Func, pass: impl Fn(&mut Func)) -> Func {
        let mut plain = f.clone();
        plain.origins = None;
        pass(&mut plain);
        pass(&mut f);
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
        assert_eq!(f.origins.as_ref().unwrap().len(), f.body.len());
        f
    }

    fn spans(f: &Func) -> Vec<u32> {
        f.origins
            .as_ref()
            .unwrap()
            .iter()
            .map(|s| s.expect("retained source"))
            .collect()
    }

    #[test]
    fn call_anchors_do_not_block_local_rules_and_follow_surviving_calls() {
        let module = hd_wasm::testing::module::Module::default();
        let mut f = module.func("anchored", vec![Val::I32], vec![]);
        let temporary = f.local(Val::I32);
        f.get(0);
        f.set(temporary);
        f.get(temporary);
        f.at = Some(99);
        f.helper(Helper::ReleaseRef);
        let mut plain = f.clone();
        for ins in &mut plain.body {
            if let Ins::Call(_, anchor) | Ins::CallIndirect(_, anchor) = ins {
                *anchor = None;
            }
        }
        let arity = f.symbols.call_arities(&[]);
        for body in [&mut f, &mut plain] {
            locals(body, &arity);
            coalesce(body);
        }
        assert!(
            !f.body
                .iter()
                .any(|ins| matches!(ins, Ins::Op(I::LocalTee(_))))
        );
        assert_eq!(f.body.len(), 2);
        assert!(f.body.iter().any(|ins| ins.call_anchor() == Some(99)));
        for ins in &mut f.body {
            if let Ins::Call(_, anchor) | Ins::CallIndirect(_, anchor) = ins {
                *anchor = None;
            }
        }
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
    }

    #[test]
    fn count_expansion_preserves_the_call_anchor_and_restores_the_emitter() {
        let module = hd_wasm::testing::module::Module::default();
        let mut f = module.func("count", vec![Val::I32], vec![]);
        f.get(0);
        f.at = Some(37);
        f.helper(Helper::ReleaseKnown);
        f.at = Some(85);
        assert!(inline_counts(&mut f, false));
        assert_eq!(f.at, Some(85));
        let calls: Vec<_> = f
            .body
            .iter()
            .filter_map(|ins| match ins {
                Ins::Call(_, anchor) | Ins::CallIndirect(_, anchor) => Some(*anchor),
                _ => None,
            })
            .collect();
        assert_eq!(calls, [Some(37)]);
    }

    #[test]
    fn count_guards_copy_the_call_anchor_to_skipped_paths() {
        let module = hd_wasm::testing::module::Module::default();
        let mut f = module.func("guard", vec![Val::I32], vec![]);
        f.get(0);
        f.at = Some(37);
        f.helper(Helper::RetainKnown);
        f.at = Some(85);
        assert!(hd_wasm::testing::count_tests::lower(
            &mut f,
            hd_wasm::testing::count_tests::Counts {
                retain: Hash128(1),
                acyclic: false,
                known_inlined: false,
                skipped: Some(Hash128(2)),
            }
        ));
        assert_eq!(f.at, Some(85));
        let calls: Vec<_> = f
            .body
            .iter()
            .filter_map(|ins| match ins {
                Ins::Call(_, anchor) | Ins::CallIndirect(_, anchor) => Some(*anchor),
                _ => None,
            })
            .collect();
        assert_eq!(calls, [Some(37), Some(37), Some(37)]);
    }

    #[test]
    fn anchors_add_no_instruction_nodes_at_width_1000() {
        for width in [100, 1000] {
            let module = hd_wasm::testing::module::Module::default();
            let mut f = module.func("width", vec![], vec![]);
            for at in 0..width {
                f.at = Some(at);
                f.helper(Helper::ReleaseKnown);
            }
            assert_eq!(f.body.len(), width as usize);
            assert!(f.body.iter().enumerate().all(|(at, ins)| {
                ins.call_anchor().and_then(|a| usize::try_from(a).ok()) == Some(at)
            }));
        }
    }

    #[test]
    fn a_constant_bulk_width_survives_empty_inlining_blocks() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for bulk in [
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            I::MemoryFill(0),
        ] {
            let f = run(
                tracked(
                    &symbols,
                    2,
                    1,
                    vec![
                        op(I::I32Const(1)),
                        op(I::LocalSet(2)),
                        op(I::Block(wasm_encoder::BlockType::Empty)),
                        op(I::Block(wasm_encoder::BlockType::Empty)),
                        op(I::LocalGet(0)),
                        op(I::LocalGet(1)),
                        op(I::LocalGet(2)),
                        op(bulk),
                        op(I::End),
                        op(I::End),
                    ],
                ),
                |f| {
                    assert!(forward(f, 2));
                    assert!(hd_wasm::testing::bulk::lower(f, |_, _| None));
                },
            );
            assert!(f.body.iter().any(|i| matches!(i, Ins::Op(I::I32Store8(_)))));
            assert!(!f.body.iter().any(|i| matches!(
                i,
                Ins::Call(_, _) | Ins::Op(I::MemoryCopy { .. } | I::MemoryFill(_))
            )));
        }
    }

    #[test]
    fn forwarding_work_is_linear_at_width_1000_depth_20() -> Result<(), String> {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for width in [1usize, 1000] {
            for depth in [1usize, 20] {
                let local = |lane: usize, level: usize| {
                    u32::try_from(1 + lane * (depth + 1) + level).map_err(|e| e.to_string())
                };
                let mut body = Vec::new();
                for lane in 0..width {
                    for level in 0..=depth {
                        let source = if level == 0 {
                            0
                        } else {
                            local(lane, level - 1)?
                        };
                        body.push(op(I::LocalGet(source)));
                        body.push(op(I::LocalSet(local(lane, level)?)));
                    }
                }
                for lane in 0..width {
                    body.push(op(I::LocalGet(local(lane, depth)?)));
                    body.push(op(I::Drop));
                }
                let n = body.len();
                let mut f = tracked(&symbols, 1, width * (depth + 1), body);
                let mut work = 0;
                assert!(forward_with(&mut f, 1, || work += 1));
                // Four instruction walks and at most one query per position.
                // Long-distance uses and copy-chain depth add no rescans.
                assert!(
                    work <= 5 * n,
                    "{width} x {depth}: {work} visits / {n} instructions"
                );
                assert_eq!(f.body.len(), 2 * width);
                for chunk in f.body.chunks_exact(2) {
                    assert!(matches!(chunk, [Ins::Op(I::LocalGet(0)), Ins::Op(I::Drop)]));
                }
                let origins = spans(&f);
                for lane in 0..width {
                    assert_eq!(
                        origins[2 * lane],
                        10 + u32::try_from(2 * lane * (depth + 1)).map_err(|e| e.to_string())?
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn forwarding_relinks_newly_adjacent_copies() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                3,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(3)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(1)),
                    op(I::LocalSet(2)),
                    op(I::LocalSet(3)),
                    op(I::LocalGet(2)),
                    op(I::Drop),
                    op(I::LocalGet(3)),
                    op(I::Drop),
                ],
            ),
            |f| {
                assert!(forward(f, 1));
            },
        );
        assert!(matches!(
            f.body.as_slice(),
            [
                Ins::Op(I::I32Const(3)),
                Ins::Op(I::Drop),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::Drop)
            ]
        ));
        assert_eq!(spans(&f), [11, 17, 10, 19]);
    }

    #[test]
    fn forwarding_preserves_local_and_global_clobbers() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let global = symbols.intern_global(Global::HashSeed);
        let other = symbols.intern_global(Global::Forbid);
        let effect = call(
            &symbols,
            Sym::Import(hd_wasm::testing::module::Import::new(
                "test",
                "change",
                &[],
                &[],
            )),
        );
        for (source, between, allowed) in [
            (
                op(I::LocalGet(0)),
                vec![op(I::I32Const(9)), op(I::LocalSet(0))],
                false,
            ),
            (
                op(I::LocalGet(0)),
                vec![op(I::I32Const(9)), op(I::LocalSet(2))],
                true,
            ),
            (
                Ins::GlobalGet(global),
                vec![op(I::I32Const(9)), Ins::GlobalSet(global)],
                false,
            ),
            (
                Ins::GlobalGet(global),
                vec![op(I::I32Const(9)), Ins::GlobalSet(other)],
                true,
            ),
            (Ins::GlobalGet(global), vec![effect], false),
        ] {
            let mut body = vec![source, op(I::LocalSet(1))];
            body.extend(between);
            body.extend([op(I::LocalGet(1)), op(I::Drop)]);
            let original = format!("{body:?}");
            let mut f = tracked(&symbols, 1, 2, body);
            assert_eq!(forward(&mut f, 1), allowed);
            if allowed {
                assert!(matches!(f.body.last(), Some(Ins::Op(I::Drop))));
                assert_eq!(
                    format!("{:?}", f.body[f.body.len() - 2]),
                    format!("{source:?}")
                );
            } else {
                assert_eq!(format!("{:?}", f.body), original);
            }
        }
    }

    #[test]
    fn an_ordinary_store_keeps_its_constant_outside_the_block() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::I32Const(1 << 30)),
                    op(I::LocalSet(1)),
                    op(I::Block(wasm_encoder::BlockType::Empty)),
                    op(I::Block(wasm_encoder::BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    })),
                    op(I::End),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(!forward(f, 1));
            },
        );
        assert_eq!(spans(&f), (10..19).collect::<Vec<_>>());
    }

    #[test]
    fn constant_forwarding_still_stops_at_conditional_and_loop_edges() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for (edge, conditional, produces) in [
            (I::If(wasm_encoder::BlockType::Empty), true, false),
            (I::Loop(wasm_encoder::BlockType::Empty), false, false),
            (
                I::Block(wasm_encoder::BlockType::Result(wasm_encoder::ValType::I32)),
                false,
                true,
            ),
        ] {
            let mut body = vec![op(I::I32Const(1)), op(I::LocalSet(1))];
            if conditional {
                body.push(op(I::LocalGet(0)));
            }
            body.extend([op(edge), op(I::LocalGet(1))]);
            if !produces {
                body.push(op(I::Drop));
            }
            body.push(op(I::End));
            if produces {
                body.push(op(I::Drop));
            }
            let mut f = tracked(&symbols, 1, 1, body);
            assert!(!forward(&mut f, 1));
        }
        // A read after the end of a conditional store may still see zero.
        let mut f = tracked(
            &symbols,
            1,
            1,
            vec![
                op(I::LocalGet(0)),
                op(I::If(wasm_encoder::BlockType::Empty)),
                op(I::I32Const(1)),
                op(I::LocalSet(1)),
                op(I::End),
                op(I::LocalGet(1)),
                op(I::Drop),
            ],
        );
        assert!(!forward(&mut f, 1));
    }

    #[test]
    fn origins_follow_local_cleanup_forwarding_and_scheduling() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![op(I::LocalGet(0)), op(I::LocalSet(1)), op(I::LocalGet(1))],
            ),
            |f| locals(f, &Arity::new()),
        );
        assert_eq!(spans(&f), [10]);
        let f = run(
            tracked(&symbols, 0, 1, vec![op(I::I32Const(7)), op(I::LocalSet(0))]),
            |f| locals(f, &Arity::new()),
        );
        assert!(spans(&f).is_empty());
        let f = run(
            tracked(&symbols, 0, 1, vec![op(I::I32Const(7)), op(I::LocalTee(0))]),
            |f| locals(f, &Arity::new()),
        );
        assert_eq!(spans(&f), [10]);
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::I32Const(1)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(1)),
                    op(I::LocalGet(1)),
                    op(I::I32Add),
                ],
            ),
            |f| locals(f, &Arity::new()),
        );
        assert!(
            f.body
                .iter()
                .zip(f.origins.as_ref().unwrap())
                .any(|(i, s)| matches!(i, Ins::Op(I::LocalTee(_))) && *s == Some(14))
        );
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::LocalSet(1)),
                    op(I::I32Const(2)),
                    op(I::Drop),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(forward(f, 1));
            },
        );
        assert_eq!(spans(&f), [12, 13, 10]);
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0, 2))),
                ],
            ),
            |f| {
                assert!(schedule(f, 1, &Arity::new()));
            },
        );
        assert_eq!(spans(&f), [10, 13, 11]);
        let f = run(
            tracked(
                &symbols,
                0,
                2,
                vec![
                    op(I::I32Const(1)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(2)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32Add),
                ],
            ),
            |f| {
                assert!(schedule(f, 0, &Arity::new()));
            },
        );
        // Rule 9 takes precedence here, moving the second value before its producer.
        assert_eq!(spans(&f), [10, 11, 14, 12, 16]);
        let f = run(
            tracked(
                &symbols,
                0,
                1,
                vec![
                    op(I::I32Const(3)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(5)),
                    op(I::LocalGet(0)),
                    op(I::I32Sub),
                ],
            ),
            |f| {
                assert!(schedule(f, 0, &Arity::new()));
            },
        );
        assert_eq!(spans(&f), [12, 10, 14]);
    }

    fn validate_void_block_fixture(f: &Func) {
        use wasm_encoder::{CodeSection, Function, FunctionSection, Module, TypeSection, ValType};
        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32; f.params.len()], []);
        types.ty().function([ValType::I32], [ValType::I32]);
        let mut functions = FunctionSection::new();
        functions.function(0);
        let mut code =
            Function::new_with_locals_types(std::iter::repeat_n(ValType::I32, f.locals.len()));
        for ins in &f.body {
            if let Ins::Op(op) = ins {
                code.instruction(&op.to_wasm(&f.labels));
            }
        }
        code.instruction(&wasm_encoder::Instruction::End);
        let mut codes = CodeSection::new();
        codes.function(&code);
        let mut module = Module::new();
        module.section(&types).section(&functions).section(&codes);
        let validation = wasmparser::Validator::new().validate_all(&module.finish());
        assert!(validation.is_ok(), "{:?}", validation.err());
    }

    #[test]
    fn void_blocks_rewrite_nested_branches_and_shared_tables() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrIf(1)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrIf(2)),
                op(I::End),
                op(I::End),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert_eq!(spans(&f), [10, 12, 13, 15, 16, 19]);
        assert!(matches!(f.body[2], Ins::Op(I::BrIf(0))));
        assert!(matches!(f.body[4], Ins::Op(I::BrIf(0))));

        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrIf(2)),
                op(I::End),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert!(matches!(
            f.body.as_slice(),
            [Ins::Op(I::LocalGet(0)), Ins::Op(I::BrIf(0))]
        ));

        let mut f = tracked(
            &symbols,
            0,
            0,
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::Br(2)),
                op(I::End),
                op(I::End),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert!(matches!(f.body[2], Ins::Op(I::Br(1))));

        let mut body = Vec::new();
        for keep_middle in [false, true] {
            body.extend([
                op(I::Block(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
            ]);
            if keep_middle {
                body.extend([op(I::LocalGet(0)), op(I::BrIf(0))]);
            }
            body.extend([
                op(I::Loop(BlockType::Empty)),
                op(I::Block(BlockType::Empty)),
                op(I::LocalGet(0)),
                op(I::BrTable(LabelListId(0), 3)),
                op(I::End),
                op(I::End),
                op(I::End),
                op(I::End),
            ]);
        }
        let mut f = tracked(&symbols, 1, 0, body);
        f.labels.push(vec![1, 3]);
        validate_void_block_fixture(&f);
        assert!(blocks(&mut f, &vec![]));
        validate_void_block_fixture(&f);
        assert_eq!(f.label_list(LabelListId(0)), [1, 3]);
        let tables: Vec<_> = f
            .body
            .iter()
            .filter_map(|ins| match ins {
                Ins::Op(I::BrTable(table, default)) => Some((f.label_list(*table), *default)),
                _ => None,
            })
            .collect();
        assert_eq!(
            tables,
            [(vec![1, 2].as_slice(), 2), (vec![1, 3].as_slice(), 3)]
        );
    }

    #[test]
    fn void_blocks_keep_typed_parameters_and_polymorphic_boundaries() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = tracked(
            &symbols,
            0,
            0,
            vec![
                op(I::Block(BlockType::Result(wasm_encoder::ValType::I32))),
                op(I::I32Const(1)),
                op(I::End),
                op(I::Drop),
                op(I::I32Const(1)),
                op(I::Block(BlockType::FunctionType(1))),
                op(I::I32Const(2)),
                op(I::I32Add),
                op(I::End),
                op(I::Drop),
                op(I::Block(BlockType::Empty)),
                op(I::Unreachable),
                op(I::End),
                op(I::Block(BlockType::Empty)),
                op(I::I32Const(3)),
                op(I::Drop),
                op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        let f = run(f, |f| {
            assert!(blocks(f, &vec![]));
        });
        validate_void_block_fixture(&f);
        assert_eq!(
            f.body
                .iter()
                .filter(|ins| matches!(ins, Ins::Op(I::Block(_))))
                .count(),
            3
        );
        assert_eq!(spans(&f), (10..23).chain(24..26).collect::<Vec<_>>());
    }

    #[test]
    fn void_blocks_have_linear_work_at_width_and_depth() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for (width, depth) in [(1000, 1), (1000, 20), (1, 10_000)] {
            let mut body = Vec::new();
            for _ in 0..width {
                body.extend(std::iter::repeat_n(op(I::Block(BlockType::Empty)), depth));
                body.extend([op(I::LocalGet(0)), op(I::Drop)]);
                body.extend(std::iter::repeat_n(op(I::End), depth));
            }
            let n = body.len();
            let mut f = tracked(&symbols, 1, 0, body);
            validate_void_block_fixture(&f);
            let mut visits = 0;
            assert!(blocks_with(&mut f, &vec![], || visits += 1));
            assert_eq!(f.body.len(), 2 * width);
            assert_eq!(visits, 3 * n);
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(2 * width));
            validate_void_block_fixture(&f);
            eprintln!(
                "void blocks width {width}, depth {depth}: {visits} visits for {n} instructions"
            );
        }
    }

    /// Rule 18: a `br 0` that only falls through goes, and a void block it
    /// alone targeted goes with it, as does a void `if`'s empty `else`; a
    /// loop's back edge, a branch that leaves values under its label's,
    /// and a typed block stay.
    #[test]
    fn a_branch_to_the_next_end_goes_when_falling_through_matches_it() {
        use wasm_encoder::ValType;
        let symbols = Symbols::default();
        let op = Ins::Op;
        let cases = [
            (
                // A void block's only branch, then the block.
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::Br(0)),
                    op(I::End),
                ],
                vec![op(I::LocalGet(0)), op(I::Drop)],
            ),
            (
                // A typed block keeps its label; its value is the stack's.
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::I32Const(1)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::Drop),
                ],
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::I32Const(1)),
                    op(I::End),
                    op(I::Drop),
                ],
            ),
            (
                // The branch discards the 1 under its value; falling through cannot.
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::I32Const(1)),
                    op(I::I32Const(2)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::Drop),
                ],
                vec![],
            ),
            (
                // A loop's `br 0` is its back edge.
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::Loop(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::BrIf(1)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::End),
                ],
                vec![],
            ),
            (
                // A then arm's branch to its `if` before the `else`.
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::Br(0)),
                    op(I::Else),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::End),
                ],
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::Else),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::End),
                ],
            ),
            (
                // After `unreachable` the stack is polymorphic: the branch
                // goes, and the block stays around the polymorphic boundary.
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::Unreachable),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::Drop),
                ],
                vec![
                    op(I::Block(BlockType::Result(ValType::I32))),
                    op(I::Unreachable),
                    op(I::End),
                    op(I::Drop),
                ],
            ),
            (
                // A void `if`'s empty `else`, after its then arm's branch.
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::Br(0)),
                    op(I::Else),
                    op(I::End),
                ],
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::Drop),
                    op(I::End),
                ],
            ),
            (
                // A value pushed after `unreachable` is still one too many
                // for a void label to fall through with.
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::Unreachable),
                    op(I::I32Const(1)),
                    op(I::Br(0)),
                    op(I::End),
                ],
                vec![],
            ),
        ];
        for (body, want) in cases {
            let f = tracked(&symbols, 1, 0, body.clone());
            validate_void_block_fixture(&f);
            let f = run(f, |f| {
                blocks(f, &vec![]);
            });
            validate_void_block_fixture(&f);
            let want = if want.is_empty() { body } else { want };
            assert_eq!(format!("{:?}", f.body), format!("{want:?}"));
        }
    }

    /// A body whose labels do not balance, or that branches past its
    /// outermost label, keeps its blocks: Wasm validation reports it as
    /// written.
    #[test]
    fn void_blocks_leave_an_invalid_body_as_written() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for body in [
            vec![
                op(I::Block(BlockType::Empty)),
                op(I::I32Const(1)),
                op(I::Drop),
            ],
            vec![op(I::Block(BlockType::Empty)), op(I::Br(2)), op(I::End)],
        ] {
            let mut f = tracked(&symbols, 0, 0, body.clone());
            assert!(!blocks(&mut f, &vec![]));
            assert_eq!(format!("{:?}", f.body), format!("{body:?}"));
            assert_eq!(spans(&f), (10..13).collect::<Vec<_>>());
        }
    }

    /// A constant address addend folds into a store's offset only while
    /// the offset fits 32 bits; past that the store keeps its `i32.add`.
    #[test]
    fn a_store_offset_folds_only_while_it_fits_32_bits() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(32)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0xFFFF_FFF0, 2))),
                    op(I::LocalGet(0)),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0, 2))),
                ],
            ),
            |f| {
                assert!(schedule(f, 1, &Arity::new()));
            },
        );
        assert!(matches!(f.body[2], Ins::Op(I::I32Add)));
        assert!(matches!(f.body[4], Ins::Op(I::I32Store(m)) if m.offset == 0xFFFF_FFF0));
        assert!(matches!(f.body[7], Ins::Op(I::I32Store(m)) if m.offset == 4));
        assert_eq!(f.body.len(), 8);
        assert_eq!(spans(&f), [10, 11, 12, 13, 14, 15, 18, 16]);
    }

    #[test]
    fn scheduling_visits_are_linear_at_width_and_depth() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for (width, depth) in [(256u32, 1u32), (1024, 1), (1, 1024)] {
            let mut body = Vec::new();
            for w in 0..width {
                body.extend([
                    op(I::LocalGet(0)),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::I32Const(9)),
                    op(I::I32Store(mem(HEAP, 0, 2))),
                    op(I::LocalGet(0)),
                ]);
                for d in 0..depth {
                    let local = 1 + w * depth + d;
                    body.extend([
                        op(I::LocalSet(local)),
                        op(I::I32Const(5)),
                        op(I::LocalGet(local)),
                        op(I::I32Sub),
                    ]);
                }
                body.push(op(I::Drop));
            }
            let before = body.len();
            let mut f = tracked(&symbols, 1, (width * depth) as usize, body);
            let mut visits = 0;
            assert!(schedule_with(&mut f, 1, &Arity::new(), || visits += 1));
            assert_eq!(f.body.len(), (width * (5 + 2 * depth)) as usize);
            assert_eq!(f.origins.as_ref().map(Vec::len), Some(f.body.len()));
            assert!(
                visits <= 9 * before,
                "{visits} scheduling visits for {before} instructions"
            );
            eprintln!(
                "scheduling width {width}, depth {depth}: {visits} visits for {before} instructions"
            );
            let mut settled = 0;
            assert!(!schedule_with(&mut f, 1, &Arity::new(), || settled += 1));
            assert!(settled <= 5 * f.body.len());
            for chunk in f.body.chunks_exact((5 + 2 * depth) as usize) {
                assert!(matches!(chunk[2], Ins::Op(I::I32Store(m)) if m.offset == 4));
                assert!(
                    chunk[3..3 + depth as usize]
                        .iter()
                        .all(|i| matches!(i, Ins::Op(I::I32Const(5))))
                );
                assert!(matches!(chunk[3 + depth as usize], Ins::Op(I::LocalGet(0))));
            }
        }
    }

    #[test]
    fn scheduling_indexes_nested_store_regions_once() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for depth in [1000, 4000] {
            let mut body = Vec::new();
            for _ in 0..depth {
                body.extend([op(I::LocalGet(0)), op(I::I32Const(4)), op(I::I32Add)]);
            }
            for _ in 0..depth {
                body.extend([op(I::I32Const(9)), op(I::I32Store(mem(HEAP, 0, 2)))]);
            }
            let n = body.len();
            let mut f = tracked(&symbols, 1, 0, body);
            let mut visits = 0;
            assert!(schedule_with(&mut f, 1, &Arity::new(), || visits += 1));
            assert_eq!(f.body.len(), 3 * depth);
            assert!(visits <= 9 * n, "{visits} visits for {n} instructions");
            assert!(
                f.body[..depth]
                    .iter()
                    .all(|i| matches!(i, Ins::Op(I::LocalGet(0))))
            );
            assert!(f.body[depth..].chunks_exact(2).all(|pair| matches!(pair, [Ins::Op(I::I32Const(9)), Ins::Op(I::I32Store(m))] if m.offset == 4)));
            eprintln!("scheduling nested stores {depth}: {visits} visits for {n} instructions");
        }
    }

    #[test]
    fn scheduling_keeps_global_reads_after_calls_and_writes() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let global = symbols.intern_global(Global::HeapTop);
        let call = call(
            &symbols,
            Sym::Import(hd_wasm::testing::module::Import::new(
                "fixture",
                "effect",
                &[],
                &[],
            )),
        );
        for effect in [vec![call], vec![op(I::I32Const(2)), Ins::GlobalSet(global)]] {
            let mut body = vec![op(I::I32Const(1))];
            body.extend(effect);
            body.extend([
                op(I::LocalSet(0)),
                Ins::GlobalGet(global),
                op(I::LocalGet(0)),
                op(I::I32Sub),
                op(I::Drop),
            ]);
            let mut f = tracked(&symbols, 0, 1, body);
            let before = spans(&f);
            assert!(!schedule(&mut f, 0, &Arity::new()));
            assert_eq!(spans(&f), before);
        }
    }

    #[test]
    fn scheduling_respects_clobbers_and_multi_result_boundaries() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let multi = call(
            &symbols,
            Sym::Import(hd_wasm::testing::module::Import {
                module: "fixture".to_owned(),
                name: "multi".to_owned(),
                params: vec![],
                results: vec![Val::I32; 2],
            }),
        );
        let mut f = tracked(
            &symbols,
            1,
            2,
            vec![
                op(I::I32Const(1)),
                op(I::LocalTee(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::LocalGet(1)),
                op(I::I32Sub),
                op(I::Drop),
                multi,
                op(I::LocalSet(2)),
                op(I::I32Const(2)),
                op(I::LocalGet(2)),
                op(I::I32Sub),
                op(I::Drop),
                op(I::Drop),
            ],
        );
        assert!(!schedule(&mut f, 1, &Arity::new()));
        assert_eq!(spans(&f), (10..24).collect::<Vec<_>>());
    }

    #[test]
    fn fold_clears_nested_empty_blocks_in_one_walk() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let depth = 4096;
        let mut body = vec![op(I::LocalGet(0)), op(I::I32Const(1)), op(I::I32Mul)];
        body.extend(std::iter::repeat_n(op(I::Block(BlockType::Empty)), depth));
        body.push(op(I::Br(0)));
        body.extend(std::iter::repeat_n(op(I::End), depth));
        // Keep a real branch to the enclosing block, and keep a loop's
        // self-branch: it repeats instead of exiting.
        body.extend([
            op(I::Block(BlockType::Empty)),
            op(I::Block(BlockType::Empty)),
            op(I::Br(1)),
            op(I::End),
            op(I::End),
            op(I::Loop(BlockType::Empty)),
            op(I::Br(0)),
            op(I::End),
        ]);
        let mut f = tracked(&symbols, 1, 0, body);
        assert!(fold(&mut f));
        assert_eq!(f.body.len(), 9);
        assert!(matches!(f.body[0], Ins::Op(I::LocalGet(0))));
        assert!(matches!(f.body[3], Ins::Op(I::Br(1))));
        assert!(matches!(f.body[6], Ins::Op(I::Loop(BlockType::Empty))));
        assert!(matches!(f.body[7], Ins::Op(I::Br(0))));
        assert_eq!(spans(&f)[0], 10);
        assert!(!fold(&mut f));
    }

    #[test]
    fn origins_follow_folds_and_both_control_shapes() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![op(I::LocalGet(0)), op(I::I32Const(0)), op(I::I32Eq)],
        );
        f.origins.as_mut().unwrap()[1] = None;
        let f = run(f, |f| {
            assert!(fold(f));
        });
        assert_eq!(spans(&f), [10, 12]);
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(7)),
                    op(I::Drop),
                    op(I::Else),
                    op(I::Br(1)),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(shapes(f, 1, &Vec::new()));
            },
        );
        assert_eq!(spans(&f), [10, 11, 11, 12, 13]);
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(7)),
                    op(I::LocalSet(1)),
                    op(I::Else),
                    op(I::I32Const(8)),
                    op(I::LocalSet(1)),
                    op(I::End),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(shapes(f, 1, &Vec::new()));
            },
        );
        assert_eq!(spans(&f), [10, 11, 12, 14, 15, 17]);
        let f = run(
            tracked(
                &symbols,
                0,
                0,
                vec![
                    op(I::I32Const(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(10)),
                    op(I::Drop),
                    op(I::Else),
                    op(I::I32Const(20)),
                    op(I::Drop),
                    op(I::End),
                ],
            ),
            |f| {
                assert!(fold_constant_branches(f));
            },
        );
        assert_eq!(spans(&f), [15, 16]);
        // An arm takes its `if`'s place, one label fewer past it, unless
        // it branches to the `if` itself.
        for (target, kept) in [(1, false), (0, true)] {
            let mut f = tracked(
                &symbols,
                0,
                0,
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::I32Const(1)),
                    op(I::If(BlockType::Empty)),
                    op(I::Br(target)),
                    op(I::End),
                    op(I::End),
                ],
            );
            assert!(fold_constant_branches(&mut f));
            let body: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
            let want = if kept {
                ["Block", "Block", "Br(0)", "End", "End"].as_slice()
            } else {
                ["Block", "Br(0)", "End"].as_slice()
            };
            assert_eq!(body.len(), want.len(), "{body:?}");
            assert!(
                body.iter().zip(want).all(|(b, w)| b.contains(w)),
                "{body:?}"
            );
        }
        let f = run(
            tracked(
                &symbols,
                0,
                1,
                vec![op(I::I32Const(0)), op(I::LocalSet(0)), op(I::LocalGet(0))],
            ),
            |f| {
                assert!(fold_constant_branches(f));
            },
        );
        assert_eq!(spans(&f), [12]);
    }

    #[test]
    fn static_counts_meet_scalar_if_values_and_reject_unknown_arms_or_exits() {
        for (dynamic, exit) in [(false, false), (true, false), (false, true)] {
            let symbols = Symbols::default();
            let retain = Hash128(7);
            let count = call(&symbols, Sym::Inst(retain));
            let arity = vec![Some((1, 0)); symbols.len()];
            let mut body = vec![
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::If(BlockType::Result(wasm_encoder::ValType::I32))),
                Ins::Op(I::I32Const(56)),
            ];
            if exit {
                body.extend([Ins::Op(I::I32Const(1)), Ins::Op(I::BrIf(0))]);
            }
            body.extend([
                Ins::Op(I::Else),
                Ins::Op(if dynamic {
                    I::LocalGet(1)
                } else {
                    I::I32Const(72)
                }),
                Ins::Op(I::End),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                count,
            ]);
            let f = run(tracked(&symbols, 2, 1, body), |f| {
                assert_eq!(
                    static_counts(f, retain, &arity, &[false, false]),
                    !dynamic && !exit
                );
            });
            assert_eq!(
                f.body
                    .iter()
                    .filter(|i| matches!(i, Ins::Call(_, _)))
                    .count(),
                usize::from(dynamic || exit)
            );
        }
    }

    #[test]
    fn static_counts_go_only_on_constant_operands() {
        let symbols = Symbols::default();
        let retain = Hash128(7);
        let counts = [
            call(&symbols, Sym::Inst(retain)),
            call(&symbols, Sym::Helper(Helper::RetainKnown)),
            call(&symbols, Sym::Helper(Helper::ReleaseRef)),
        ];
        let mut arity = vec![None; symbols.len()];
        for c in &counts {
            let Ins::Call(id, _) = c else { unreachable!() };
            arity[id.idx()] = Some((1, 0));
        }
        let op = Ins::Op;
        // Local 1 holds only constants, through a tee and a copy; local 2
        // only ever adds to itself; local 0 is a parameter.
        let f = run(
            tracked(
                &symbols,
                1,
                2,
                vec![
                    op(I::I32Const(56)),
                    op(I::LocalTee(1)),
                    counts[0],
                    op(I::I32Const(72)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(1)),
                    counts[1],
                    op(I::LocalGet(2)),
                    op(I::I32Const(8)),
                    op(I::I32Add),
                    op(I::LocalTee(2)),
                    counts[2],
                    op(I::LocalGet(0)),
                    counts[0],
                ],
            ),
            |f| {
                assert!(static_counts(f, retain, &arity, &[false]));
            },
        );
        let left: Vec<String> = f
            .body
            .iter()
            .filter(|i| matches!(i, Ins::Call(_, _)))
            .map(|i| format!("{i:?}"))
            .collect();
        assert_eq!(
            left,
            [format!("{:?}", counts[2]), format!("{:?}", counts[0])]
        );
        assert!(matches!(f.body[2], Ins::Op(I::Drop)));
        assert_eq!(spans(&f)[2], 12);
    }

    #[test]
    fn static_values_require_every_closed_world_call_to_be_constant() {
        let symbols = Symbols::default();
        let callee = symbols.intern(Sym::Inst(Hash128(7)));
        let source = symbols.intern(Sym::Inst(Hash128(8)));
        let branched = symbols.intern(Sym::Inst(Hash128(9)));
        let arity = vec![Some((1, 0)), Some((0, 1)), Some((0, 1))];
        let target = tracked(&symbols, 1, 0, vec![]);
        let constant_caller = tracked(
            &symbols,
            0,
            0,
            vec![Ins::Op(I::I32Const(1)), Ins::Call(callee, None)],
        );
        let facts = static_values(
            &[Some(target.clone()), Some(constant_caller.clone())],
            [],
            &arity,
        );
        assert_eq!(facts[0], [true]);

        let mut constant_source = hd_wasm::testing::module::Module::default().func(
            "constant source",
            vec![],
            vec![Val::I32],
        );
        constant_source.symbols = symbols.clone();
        constant_source.body = vec![Ins::Op(I::I32Const(2))];
        let result_caller = tracked(
            &symbols,
            0,
            0,
            vec![Ins::Call(source, None), Ins::Call(callee, None)],
        );
        let facts = static_values(
            &[
                Some(target.clone()),
                Some(constant_source),
                Some(result_caller),
            ],
            [],
            &arity,
        );
        assert_eq!(facts[0], [true]);

        let mut branched_source = hd_wasm::testing::module::Module::default().func(
            "branched source",
            vec![],
            vec![Val::I32],
        );
        branched_source.symbols = symbols.clone();
        branched_source.body = vec![
            Ins::Op(I::I32Const(0)),
            Ins::Op(I::If(BlockType::Empty)),
            Ins::Op(I::I32Const(2)),
            Ins::Op(I::Return),
            Ins::Op(I::End),
            Ins::Op(I::I32Const(1)),
        ];
        let branched_caller = tracked(
            &symbols,
            0,
            0,
            vec![Ins::Call(branched, None), Ins::Call(callee, None)],
        );
        let facts = static_values(
            &[
                Some(target.clone()),
                None,
                Some(branched_source),
                Some(branched_caller),
            ],
            [],
            &arity,
        );
        assert_eq!(facts[0], [false]);

        let unknown_caller = tracked(
            &symbols,
            1,
            0,
            vec![Ins::Op(I::LocalGet(0)), Ins::Call(callee, None)],
        );
        let facts = static_values(
            &[
                Some(target.clone()),
                Some(constant_caller),
                Some(unknown_caller),
            ],
            [],
            &arity,
        );
        assert_eq!(facts[0], [false]);

        let facts = static_values(&[Some(target)], [callee], &arity);
        assert_eq!(facts[0], [false]);
    }

    // #1912: the proof went in rounds over every function, and a chain of
    // calls that pass a constant on proved one level per round: O(n^2)
    // instructions read for a chain of n functions. Now a function is
    // looked at again only when a fact it reads was proved.
    #[test]
    fn static_values_read_a_call_chain_a_constant_number_of_times() {
        let chain = |n: usize| {
            let symbols = Symbols::default();
            let ids: Vec<SymId> = (0..=n)
                .map(|k| symbols.intern(Sym::Inst(Hash128(100 + k as u128))))
                .collect();
            let mut arity = vec![Some((0, 0))];
            arity.extend((1..=n).map(|_| Some((1, 0))));
            // `f0` passes a constant to `f1`; each `f_i` passes its own
            // parameter on to `f_(i+1)`.
            let mut funcs = vec![Some(tracked(
                &symbols,
                0,
                0,
                vec![Ins::Op(I::I32Const(1)), Ins::Call(ids[1], None)],
            ))];
            for k in 1..=n {
                let body = if k < n {
                    vec![Ins::Op(I::LocalGet(0)), Ins::Call(ids[k + 1], None)]
                } else {
                    vec![]
                };
                funcs.push(Some(tracked(&symbols, 1, 0, body)));
            }
            let (facts, work) =
                hd_wasm::work::count_static_looks(|| static_values(&funcs, [ids[0]], &arity));
            assert!(facts[1..].iter().all(|p| p == &[true]), "{n}: {facts:?}");
            work
        };
        let (small, large) = (chain(20), chain(200));
        assert!(
            small > 0 && large <= 11 * small,
            "instructions read for chains of 20 and 200 calls: {small} and {large}"
        );
    }

    fn releases(f: &Func) -> usize {
        f.body
            .iter()
            .filter(|i| {
                matches!(i, Ins::Call(id, _) if matches!(&*f.symbol(*id),
            Sym::Helper(Helper::ReleaseRef | Helper::ReleaseKnown)))
            })
            .count()
    }

    #[test]
    fn zero_facts_remove_initial_and_moved_slots_but_keep_live_values() {
        let symbols = Symbols::default();
        let f = tracked(
            &symbols,
            1,
            2,
            vec![
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseKnown)),
                Ins::Op(I::LocalGet(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(7)),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                Ins::Op(I::LocalSet(2)),
                Ins::Op(I::LocalGet(2)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::I32Const(0)),
                Ins::Op(I::LocalSet(2)),
            ],
        );
        let f = run(f, |f| {
            zero_releases(f, &mut None, &mut || {});
        });
        assert_eq!(releases(&f), 2);
        assert_eq!(
            f.body
                .iter()
                .filter(|i| matches!(i, Ins::Op(I::LocalSet(2))))
                .count(),
            2
        );
    }

    #[test]
    fn zero_facts_meet_both_if_arms_and_exclude_returned_paths() {
        let symbols = Symbols::default();
        let branch = |live: bool| {
            tracked(
                &symbols,
                2,
                1,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::If(BlockType::Empty)),
                    Ins::Op(if live { I::LocalGet(1) } else { I::I32Const(0) }),
                    Ins::Op(I::LocalSet(2)),
                    Ins::Op(I::Else),
                    Ins::Op(I::I32Const(0)),
                    Ins::Op(I::LocalSet(2)),
                    Ins::Op(I::End),
                    Ins::Op(I::LocalGet(2)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            )
        };
        assert_eq!(
            releases(&run(branch(false), |f| {
                zero_releases(f, &mut None, &mut || {});
            })),
            0
        );
        assert_eq!(
            releases(&run(branch(true), |f| {
                zero_releases(f, &mut None, &mut || {});
            })),
            1
        );
        let mut exits = branch(true);
        exits.body.insert(4, Ins::Op(I::Return));
        exits.origins = Some(vec![Some(17); exits.body.len()]);
        assert_eq!(
            releases(&run(exits, |f| {
                zero_releases(f, &mut None, &mut || {});
            })),
            0
        );
    }

    #[test]
    fn zero_facts_include_loop_backedges_and_branch_table_exits() {
        let symbols = Symbols::default();
        let loop_body = |reset: bool| {
            let mut body = vec![
                Ins::Op(I::Loop(BlockType::Empty)),
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::LocalSet(1)),
            ];
            if reset {
                body.extend([Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(1))]);
            }
            body.extend([
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrIf(0)),
                Ins::Op(I::End),
            ]);
            tracked(&symbols, 1, 1, body)
        };
        assert_eq!(
            releases(&run(loop_body(false), |f| {
                zero_releases(f, &mut None, &mut || {});
            })),
            1
        );
        assert_eq!(
            releases(&run(loop_body(true), |f| {
                zero_releases(f, &mut None, &mut || {});
            })),
            0
        );
        let mut table = tracked(
            &symbols,
            1,
            1,
            vec![
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrTable(LabelListId(0), 1)),
                Ins::Op(I::End),
                Ins::Op(I::I32Const(7)),
                Ins::Op(I::LocalSet(1)),
                Ins::Op(I::End),
                Ins::Op(I::LocalGet(1)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ],
        );
        table.labels.push(vec![0]);
        assert_eq!(
            releases(&run(table, |f| {
                zero_releases(f, &mut None, &mut || {});
            })),
            1
        );
    }

    #[test]
    fn dead_stores_go_unless_a_release_reads_them_on_some_path() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        let zeros = |f: &Func| {
            f.body
                .windows(2)
                .filter(|w| matches!(w, [Ins::Op(I::I32Const(0)), Ins::Op(I::LocalSet(1))]))
                .count()
        };
        // A loop that moves local 1 out and zeroes it: the zero stays when
        // the release after the loop, or the release at the loop head on
        // the backedge, reads it, and goes when the next iteration
        // overwrites it first.
        let body = |head_release: bool, exit_release: bool| {
            let mut b = vec![op(I::Loop(BlockType::Empty))];
            if head_release {
                b.extend([op(I::LocalGet(1)), release]);
            }
            b.extend([
                op(I::LocalGet(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(1)),
                op(I::Drop),
                op(I::I32Const(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::BrIf(0)),
                op(I::End),
            ]);
            if exit_release {
                b.extend([op(I::LocalGet(1)), release]);
            }
            tracked(&symbols, 1, 1, b)
        };
        for (head, exit, kept) in [(false, false, 0), (true, false, 1), (false, true, 1)] {
            let f = run(body(head, exit), |f| {
                live_rules(f, &Arity::new(), None);
            });
            assert_eq!(zeros(&f), kept, "{:?}", f.body);
        }
        // A dead set of a constant drops it, and rule 15 takes the push back.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    release,
                    op(I::I32Const(5)),
                    op(I::LocalSet(1)),
                ],
            ),
            |f| {
                assert!(live_rules(f, &Arity::new(), None));
                assert!(drops(f));
            },
        );
        assert_eq!(f.body.len(), 2);
        assert_eq!(spans(&f), [10, 11]);
    }

    #[test]
    fn single_uses_stay_on_the_stack_by_liveness() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let guard = [
            op(I::If(BlockType::Empty)),
            call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            op(I::Unreachable),
            op(I::End),
        ];
        // Local 2 is written twice, so the count rules leave it; each
        // write has one reader, and the loop head writes it again.
        let mut body = vec![op(I::Loop(BlockType::Empty))];
        body.extend([op(I::LocalGet(0)), op(I::LocalSet(2))]);
        body.extend([op(I::LocalGet(1)), op(I::I32Eqz)]);
        body.extend(guard);
        body.extend([op(I::LocalGet(2)), op(I::Drop)]);
        body.extend([op(I::LocalGet(1)), op(I::I32Const(1)), op(I::I32Add)]);
        body.extend([op(I::LocalSet(2)), op(I::LocalGet(1)), op(I::LocalSet(3))]);
        body.extend([
            op(I::LocalGet(2)),
            op(I::LocalGet(3)),
            op(I::BrIf(0)),
            op(I::Drop),
        ]);
        body.push(op(I::End));
        let f = run(tracked(&symbols, 2, 2, body), |f| {
            while live_rules(f, &Arity::new(), None) {}
        });
        let shown: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
        // The copy of 0 moves past the guard to its read, the copy of 1
        // moves to its read, and the sum stays on the stack under it.
        assert!(
            !shown
                .iter()
                .any(|i| i.contains("Local") && i.contains("(2)")),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|i| i.contains("(3)")), "{shown:?}");
        // A loop test evaluated before the loop and again at its head:
        // each value stays on the stack, written by a tee for the head.
        let f = run(
            tracked(
                &symbols,
                0,
                2,
                vec![
                    op(I::I32Const(0)),
                    op(I::LocalSet(0)),
                    op(I::I32Const(3)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32LtU),
                    op(I::Drop),
                    op(I::Loop(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::LocalGet(1)),
                    op(I::I32LtU),
                    op(I::BrIf(0)),
                    op(I::End),
                ],
            ),
            |f| {
                while live_rules(f, &Arity::new(), None) {}
            },
        );
        let shown: Vec<String> = f.body.iter().map(|i| format!("{i:?}")).collect();
        assert_eq!(
            shown.iter().filter(|i| i.contains("LocalTee")).count(),
            2,
            "{shown:?}"
        );
        assert!(!shown.iter().any(|i| i.contains("LocalSet")), "{shown:?}");
        // A run that consumes the value keeps its local.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::Drop),
                    op(I::LocalGet(1)),
                ],
            ),
            |f| {
                assert!(!live_rules(f, &Arity::new(), None));
            },
        );
        assert_eq!(f.body.len(), 5);
    }

    #[test]
    fn a_branch_to_the_exit_after_its_values_returns_them() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        // The first branch sets both exit values and returns; the second
        // sets one only and keeps its branch.
        let f = run(
            tracked(
                &symbols,
                1,
                2,
                vec![
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::If(BlockType::Empty)),
                    op(I::I32Const(1)),
                    op(I::I32Const(2)),
                    op(I::LocalSet(2)),
                    op(I::LocalSet(1)),
                    op(I::Br(1)),
                    op(I::End),
                    op(I::I32Const(3)),
                    op(I::LocalSet(2)),
                    op(I::Br(0)),
                    op(I::End),
                    op(I::LocalGet(1)),
                    op(I::LocalGet(2)),
                ],
            ),
            |f| {
                assert!(exits(f));
            },
        );
        assert!(matches!(f.body[5], Ins::Op(I::Return)), "{:?}", f.body);
        assert!(matches!(f.body[9], Ins::Op(I::Br(0))), "{:?}", f.body);
        assert_eq!(spans(&f)[5], 17);
    }

    #[test]
    fn local_cleanup_rounds_are_bounded_at_width_1000_and_depth_20() {
        let symbols = Symbols::default();
        let opaque = call(&symbols, Sym::Inst(Hash128(1882)));
        let arity = vec![Some((1, 0)); symbols.len()];
        let op = Ins::Op;
        for width in [1, 1000, 10000] {
            for depth in [1, 20] {
                let mut body = vec![op(I::Block(BlockType::Empty)); depth];
                for _ in 0..width {
                    // The first write becomes dead when pairs forwards the
                    // second one. Early liveness exposes that fact within
                    // this sweep rather than waiting for another fixpoint.
                    body.extend([
                        op(I::I32Const(1)),
                        op(I::LocalSet(0)),
                        op(I::I32Const(42)),
                        op(I::LocalSet(0)),
                        op(I::LocalGet(0)),
                        opaque,
                    ]);
                }
                body.extend(vec![op(I::End); depth]);
                let input = body.len();
                let mut f = tracked(&symbols, 0, 1, body);
                let expected_origins: Vec<_> = f
                    .body
                    .iter()
                    .zip(spans(&f))
                    .filter_map(|(ins, origin)| matches!(ins, Ins::Call(_, _)).then_some(origin))
                    .collect();
                let mut plain = f.clone();
                plain.origins = None;
                locals(&mut plain, &arity);
                let mut rounds = Vec::new();
                locals_with_work(&mut f, &arity, &mut |n| rounds.push(n));
                assert_eq!(rounds.len(), 2, "{width} x {depth}: {rounds:?}");
                assert!(rounds.iter().sum::<usize>() <= 2 * input);
                assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
                assert!(f.locals.is_empty());
                let retained: Vec<_> = f
                    .body
                    .iter()
                    .filter(|ins| !matches!(ins, Ins::Op(I::Block(_) | I::End)))
                    .copied()
                    .collect();
                assert_eq!(retained.len(), 2 * width);
                for pair in retained.chunks_exact(2) {
                    assert!(matches!(pair, [Ins::Op(I::I32Const(42)), Ins::Call(_, _)]));
                }
                let actual_origins: Vec<_> = f
                    .body
                    .iter()
                    .zip(spans(&f))
                    .filter_map(|(ins, origin)| matches!(ins, Ins::Call(_, _)).then_some(origin))
                    .collect();
                assert_eq!(actual_origins, expected_origins);
            }
        }
    }

    #[test]
    fn a_rule_that_found_nothing_runs_again_only_after_an_edit() {
        let mut f = Func::default();
        let mut settled = Settled::new();
        let runs = std::cell::Cell::new([0; 3]);
        let pass = |k: usize, edits: bool| {
            let mut now = runs.get();
            now[k] += 1;
            runs.set(now);
            edits
        };
        assert!(!settled.run(&mut f, Rule::Pairs, |_| pass(0, false)));
        assert!(!settled.run(&mut f, Rule::Pairs, |_| pass(0, false)));
        assert_eq!(runs.get()[0], 1, "nothing changed since it found nothing");
        assert!(settled.run(&mut f, Rule::Forward, |_| pass(1, true)));
        assert!(!settled.run(&mut f, Rule::Pairs, |_| pass(0, false)));
        assert_eq!(runs.get()[0], 2, "another rule edited the function");
        // A rule that repeats itself to its own fixed point has seen its
        // result; a single sweep has not.
        assert!(settled.run(&mut f, Rule::Live, |_| pass(2, true)));
        assert!(!settled.run(&mut f, Rule::Live, |_| pass(2, false)));
        assert_eq!(runs.get()[2], 1);
        assert!(settled.run(&mut f, Rule::Forward, |_| pass(1, true)));
        assert!(!settled.run(&mut f, Rule::Forward, |_| pass(1, false)));
        assert_eq!(runs.get()[1], 3);
    }

    #[test]
    fn liveness_hashes_no_set_node_per_access() -> Result<(), &'static str> {
        let symbols = Symbols::default();
        for width in [100u32, 1000] {
            for depth in [2, 20] {
                for repeats in [1, 10] {
                    let mut body = vec![Ins::Op(I::Block(BlockType::Empty)); depth];
                    for local in 0..width {
                        for _ in 0..repeats {
                            body.extend([
                                Ins::Op(I::LocalGet(local)),
                                Ins::Op(I::Drop),
                                Ins::Op(I::I32Const(1)),
                                Ins::Op(I::LocalTee(local)),
                                Ins::Op(I::Drop),
                                Ins::Op(I::LocalGet(local)),
                                Ins::Op(I::Drop),
                            ]);
                        }
                    }
                    body.extend(vec![Ins::Op(I::End); depth]);
                    let f = tracked(&symbols, 0, width as usize, body);
                    let before = live_set::hash_probes();
                    let dead =
                        Live::dead_accesses(&f, None).ok_or("structured body has liveness")?;
                    let hashes = live_set::hash_probes() - before;
                    assert_eq!(
                        hashes, 0,
                        "{width} locals, depth {depth}, {repeats} repeats"
                    );
                    assert_eq!(dead.len(), f.body.len());
                    // Each tee is read by its following get. Only the last
                    // repeat's trailing get has no later read of that local.
                    for (repeat, accesses) in
                        dead[depth..dead.len() - depth].chunks_exact(7).enumerate()
                    {
                        assert!(!accesses[3]);
                        assert_eq!(accesses[5], repeat % repeats == repeats - 1);
                    }
                    // A positive control proves these bodies exercise the
                    // counter when their arena does hash-cons its nodes.
                    let before = live_set::hash_probes();
                    let live = Live::new(&f, Sets::default()).ok_or("control has liveness")?;
                    assert!(live_set::hash_probes() > before);
                    assert!(live.sets.contains(live.into[0], width - 1));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn liveness_revisits_only_the_blocks_a_backedge_reaches() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        // A long run of forward blocks, then a loop that reads local 1
        // before it writes it, so the read reaches the loop's head again
        // over the backedge.
        for prefix in [16usize, 256, 1024] {
            let mut body = Vec::new();
            for _ in 0..prefix {
                body.extend([
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::BrIf(0)),
                    op(I::End),
                ]);
            }
            body.extend([
                op(I::Loop(BlockType::Empty)),
                op(I::LocalGet(1)),
                op(I::Drop),
                op(I::LocalGet(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::BrIf(0)),
                op(I::End),
            ]);
            let f = tracked(&symbols, 1, 1, body);
            let mut live = Live::blocks(&f, None, Sets::default()).expect("blocks");
            assert!(!live.forward);
            let blocks = live.starts.len();
            let mut visits = 0;
            live.settle_with(&f, || visits += 1);
            // The backedge sends only the loop's blocks and the one
            // before it round again, not every block once more.
            assert!(visits <= blocks + 4, "{visits} visits of {blocks} blocks");
            // Local 1 is live into every block before the loop: its
            // first read is of the zero it starts with.
            assert!(
                live.into[..blocks - 2]
                    .iter()
                    .all(|&row| live.sets.contains(row, 1))
            );
            assert!(live.sets.contains(live.into[0], 0));
        }
    }

    /// Liveness work per instruction stays flat as a body and its locals
    /// grow 10x: a loop whose run writes and reads `n` locals, read once
    /// more after it. Dense rows pay one bit per access and a row per
    /// edge, so the work grows 9.9x; persistent sets paid a path of
    /// `log2 n` nodes per access, 12.6x here and 3x the work.
    #[test]
    fn liveness_work_per_instruction_is_flat_in_body_size() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let work = |n: u32| {
            let mut body = vec![op(I::Loop(BlockType::Empty))];
            for l in 0..n {
                body.extend([op(I::LocalGet(l)), op(I::I32Const(1)), op(I::I32Add)]);
                body.push(op(I::LocalSet(l)));
            }
            body.extend([op(I::LocalGet(0)), op(I::BrIf(0)), op(I::End)]);
            body.extend((0..n).flat_map(|l| [op(I::LocalGet(l)), op(I::Drop)]));
            let f = tracked(&symbols, 0, n as usize, body);
            let (dead, work) = hd_wasm::work::count_liveness(|| Live::dead_accesses(&f, None));
            assert!(
                dead.is_some_and(|dead| !dead[4]),
                "the loop reads its write"
            );
            work
        };
        let (a, b) = (work(200), work(2000));
        assert!(a > 0 && b * 10 <= a * 105, "{a} then {b}");
    }

    /// Rule 13's rounds carry their dead accesses: after each round, the
    /// carried ones are what a new liveness pass finds. The bodies hold
    /// each shape a round can leave: two moves of one local whose spans
    /// cross or nest, with and without a later read of it, a read of the
    /// moved local inside a span, a span across a panic guard's blocks, a
    /// tee's push read again, a moved constant, a dead tee, and a loop.
    #[test]
    fn rule_13_rounds_carry_the_dead_accesses_a_new_pass_finds() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let (get, set, tee) = (
            |l| op(I::LocalGet(l)),
            |l| op(I::LocalSet(l)),
            |l| op(I::LocalTee(l)),
        );
        let add = op(I::I32Add);
        let drop = op(I::Drop);
        let crossing = vec![get(0), set(1), get(0), set(2), get(1), get(2), add, drop];
        let nested = vec![get(0), set(1), get(0), set(2), get(2), get(1), add, drop];
        let read_inside = vec![get(0), set(1), get(0), drop, get(1), drop];
        let guard = vec![
            get(0),
            set(1),
            op(I::I32Const(1)),
            op(I::If(BlockType::Empty)),
            op(I::Unreachable),
            op(I::End),
            get(1),
            drop,
        ];
        let teed = vec![get(0), tee(1), drop, get(1), drop];
        let constant = vec![op(I::I32Const(7)), set(1), get(1), drop];
        let dead_tee = vec![get(0), tee(2), drop];
        let mut cases = Vec::new();
        for body in [
            crossing,
            nested,
            read_inside,
            guard,
            teed,
            constant,
            dead_tee,
        ] {
            let mut later = body.clone();
            later.extend([get(0), drop]);
            let mut looped = vec![op(I::Loop(BlockType::Empty))];
            looped.extend(later.iter().copied());
            looped.extend([get(0), op(I::BrIf(0)), op(I::End)]);
            cases.extend([body, later, looped]);
        }
        for body in cases {
            let mut f = tracked(&symbols, 1, 2, body.clone());
            let mut dead = Live::dead_accesses(&f, None).expect("analysed");
            let mut rounds = 0;
            while let Some(next) = live_once(&mut f, &Arity::new(), &dead) {
                assert_eq!(
                    Some(&next),
                    Live::dead_accesses(&f, None).as_ref(),
                    "{body:?} -> {:?}",
                    f.body
                );
                dead = next;
                rounds += 1;
            }
            assert!(rounds > 0, "{body:?} rewrites");
        }
    }

    /// Rule 11's fixed point settles a loop before the blocks after it.
    /// The loop copies a local its last round wrote, so its exit gets two
    /// facts; taken first in first out, each of the `n` blocks after it
    /// was visited once per fact, and now once.
    #[test]
    fn zero_facts_settle_a_loop_before_the_blocks_after_it() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        for n in [16usize, 256] {
            let mut body = vec![
                op(I::Loop(BlockType::Empty)),
                op(I::LocalGet(1)),
                op(I::LocalSet(2)),
                op(I::LocalGet(0)),
                op(I::LocalSet(1)),
                op(I::LocalGet(0)),
                op(I::BrIf(0)),
                op(I::End),
            ];
            for _ in 0..n {
                body.extend([
                    op(I::Block(BlockType::Empty)),
                    op(I::LocalGet(0)),
                    op(I::BrIf(0)),
                    op(I::End),
                ]);
            }
            body.extend([op(I::I32Const(0)), op(I::LocalSet(2))]);
            let mut f = tracked(&symbols, 1, 2, body);
            let blocks = zero_edges(&f.body, &f.labels)
                .map(|edges| basic_blocks(&edges, f.body.len()).1.len())
                .expect("edges");
            let mut visits = 0;
            zero_releases(&mut f, &mut None, &mut || visits += 1);
            assert!(visits <= blocks + 4, "{visits} visits of {blocks} blocks");
        }
    }

    /// A copy shares its source's local only where no other value of the
    /// copy reaches: a branch around the copy brings the copy's older
    /// value to the join, so the two stay apart. Without the branch they
    /// share and the copy goes.
    #[test]
    fn a_copy_joined_by_a_branch_around_it_keeps_its_own_local() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        for branch in [true, false] {
            let mut body = vec![
                op(I::I32Const(7)),
                op(I::LocalSet(1)),
                op(I::I32Const(1)),
                op(I::LocalSet(2)),
                op(I::Block(BlockType::Empty)),
            ];
            if branch {
                body.extend([op(I::LocalGet(0)), op(I::BrIf(0))]);
            }
            body.extend([
                op(I::LocalGet(1)),
                op(I::LocalSet(2)),
                op(I::End),
                op(I::LocalGet(2)),
                release,
                op(I::LocalGet(1)),
                release,
            ]);
            let f = run(tracked(&symbols, 1, 2, body), coalesce);
            assert_eq!(f.locals.len(), if branch { 2 } else { 1 }, "{:?}", f.body);
        }
    }

    /// Each set of a multi-value copy pairs with its get from the end of
    /// the run of gets, a tee ends the run, and a set whose source an
    /// earlier set of the run wrote copies nothing.
    #[test]
    fn copy_sources_pair_each_set_of_a_run_with_its_get() {
        let op = Ins::Op;
        let body = [
            op(I::LocalGet(1)),
            op(I::LocalGet(2)),
            op(I::LocalSet(1)),
            op(I::LocalSet(2)),
            op(I::LocalGet(3)),
            op(I::LocalTee(4)),
            op(I::I32Const(0)),
            op(I::LocalSet(5)),
            op(I::LocalGet(4)),
            op(I::LocalGet(3)),
            op(I::LocalSet(6)),
            op(I::LocalTee(7)),
        ];
        assert_eq!(
            copy_sources(&body, 8),
            [
                None,
                None,
                Some((2, 1)),
                None,
                None,
                Some((3, 4)),
                None,
                None,
                None,
                None,
                Some((3, 9)),
                Some((4, 8)),
            ]
        );
    }

    #[test]
    fn coalescing_keeps_apart_locals_live_over_a_backedge_or_from_entry() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        let locals = |f: &Func| f.locals.len();
        // Local 1 is read in the loop before local 2 is written there, but
        // the backedge reads 1 again: they never share.
        let looped = vec![
            op(I::I32Const(5)),
            op(I::LocalSet(1)),
            op(I::Loop(BlockType::Empty)),
            op(I::LocalGet(1)),
            release,
            op(I::I32Const(7)),
            op(I::LocalSet(2)),
            op(I::LocalGet(2)),
            release,
            op(I::LocalGet(0)),
            op(I::BrIf(0)),
            op(I::End),
        ];
        assert_eq!(locals(&run(tracked(&symbols, 1, 2, looped), coalesce)), 2);
        // Local 2 is written in one arm only, so the read after the `if`
        // may read its zero: it is live from the entry, and does not take
        // local 1's, whose value is dead by then.
        let one_arm = vec![
            op(I::I32Const(9)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            release,
            op(I::LocalGet(0)),
            op(I::If(BlockType::Empty)),
            op(I::I32Const(3)),
            op(I::LocalSet(2)),
            op(I::End),
            op(I::LocalGet(2)),
            release,
        ];
        assert_eq!(locals(&run(tracked(&symbols, 1, 2, one_arm), coalesce)), 2);
        // With both arms writing it, local 2 is not live at entry and
        // shares a local with 1 or the parameter, both dead by then.
        let both_arms = vec![
            op(I::I32Const(9)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            release,
            op(I::LocalGet(0)),
            op(I::If(BlockType::Empty)),
            op(I::I32Const(3)),
            op(I::LocalSet(2)),
            op(I::Else),
            op(I::I32Const(4)),
            op(I::LocalSet(2)),
            op(I::End),
            op(I::LocalGet(2)),
            release,
        ];
        assert_eq!(
            locals(&run(tracked(&symbols, 1, 2, both_arms), coalesce)),
            1
        );
        // A copy of a running total whose source is written again only
        // after the copy's last read shares the total's local, and the
        // copy goes, in and out of a loop.
        for looping in [false, true] {
            let mut body = vec![op(I::I32Const(1)), op(I::LocalSet(1))];
            if looping {
                body.push(op(I::Loop(BlockType::Empty)));
            }
            body.extend([
                op(I::LocalGet(1)),
                op(I::LocalSet(2)),
                op(I::LocalGet(0)),
                release,
                op(I::LocalGet(2)),
                op(I::I32Const(1)),
                op(I::I32Add),
                op(I::LocalSet(1)),
            ]);
            if looping {
                body.extend([op(I::LocalGet(0)), op(I::BrIf(0)), op(I::End)]);
            }
            body.extend([op(I::LocalGet(1)), release]);
            let f = run(tracked(&symbols, 1, 2, body), coalesce);
            assert_eq!(locals(&f), 1, "{looping}: {:?}", f.body);
            let copies = f.body.windows(2).filter(
                |w| matches!(w, [Ins::Op(I::LocalGet(a)), Ins::Op(I::LocalSet(b))] if a == b),
            );
            assert_eq!(copies.count(), 0, "{:?}", f.body);
        }
        // A copy whose source is written while the copy is still live
        // keeps a local of its own (the parameter, read last, keeps its).
        let clobbered = vec![
            op(I::I32Const(1)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            op(I::LocalSet(2)),
            op(I::I32Const(2)),
            op(I::LocalSet(1)),
            op(I::LocalGet(2)),
            op(I::LocalGet(1)),
            op(I::I32Add),
            release,
            op(I::LocalGet(0)),
            release,
        ];
        assert_eq!(
            locals(&run(tracked(&symbols, 1, 2, clobbered), coalesce)),
            2
        );
    }

    #[test]
    fn coalesced_locals_never_hold_two_live_values() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let release = call(&symbols, Sym::Helper(Helper::ReleaseRef));
        // Locals 1 and 2 live one after the other: one local. Local 3
        // overlaps local 4, and local 5 is read before any write, so it
        // keeps its own zero while 4 is live.
        let body = vec![
            op(I::LocalGet(0)),
            op(I::LocalSet(1)),
            op(I::LocalGet(1)),
            release,
            op(I::LocalGet(0)),
            op(I::I32Eqz),
            op(I::LocalSet(2)),
            op(I::LocalGet(2)),
            release,
            op(I::I32Const(7)),
            op(I::LocalSet(3)),
            op(I::I32Const(8)),
            op(I::LocalSet(4)),
            op(I::LocalGet(3)),
            op(I::LocalGet(4)),
            op(I::I32Add),
            release,
            op(I::LocalGet(5)),
            release,
        ];
        let mut f = run(tracked(&symbols, 1, 5, body), coalesce);
        let named: Vec<u32> = f
            .body
            .iter()
            .filter_map(|i| match i {
                Ins::Op(I::LocalGet(l) | I::LocalSet(l)) => Some(*l),
                _ => None,
            })
            .collect();
        // 1 and 2 take the parameter's local, whose value is dead after
        // its last read; 3 joins them too. 4 and 5 stay apart.
        assert_eq!(named, [0, 0, 0, 0, 0, 1, 0, 1, 2], "{:?}", f.body);
        assert_eq!(f.locals.len(), 2);
        // A copy whose source keeps its value shares the source's local
        // and goes.
        f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    release,
                    op(I::LocalGet(1)),
                    release,
                ],
            ),
            coalesce,
        );
        assert_eq!(f.body.len(), 4);
        assert!(f.locals.is_empty());
        // A two-value copy shares each source's local and goes, though
        // first fit would put its first destination in the parameter's
        // class.
        f = run(
            tracked(
                &symbols,
                1,
                4,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(1)),
                    op(I::LocalGet(0)),
                    op(I::I32Eqz),
                    op(I::LocalSet(2)),
                    op(I::LocalGet(1)),
                    op(I::LocalGet(2)),
                    op(I::LocalSet(4)),
                    op(I::LocalSet(3)),
                    op(I::LocalGet(3)),
                    release,
                    op(I::LocalGet(4)),
                    release,
                ],
            ),
            coalesce,
        );
        assert_eq!(f.body.len(), 10, "{:?}", f.body);
    }

    #[test]
    fn zero_facts_without_locals_distinguish_reachable_and_unreachable_releases() {
        let symbols = Symbols::default();
        let f = run(
            tracked(
                &symbols,
                0,
                0,
                vec![
                    Ins::Op(I::I32Const(0)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                    Ins::Op(I::Return),
                    Ins::Op(I::I32Const(0)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            ),
            |f| {
                assert!(zero_releases(f, &mut None, &mut || {}));
            },
        );
        assert!(matches!(
            f.body.as_slice(),
            [Ins::Op(I::Return), Ins::Op(I::I32Const(0)), Ins::Call(_, _)]
        ));
        assert_eq!(releases(&f), 1);
    }

    #[test]
    fn zero_facts_meet_multiple_words_without_losing_other_high_locals() {
        let symbols = Symbols::default();
        let f = run(
            tracked(
                &symbols,
                1,
                129,
                vec![
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::If(BlockType::Empty)),
                    Ins::Op(I::LocalGet(0)),
                    Ins::Op(I::LocalSet(65)),
                    Ins::Op(I::Else),
                    Ins::Op(I::I32Const(0)),
                    Ins::Op(I::LocalSet(65)),
                    Ins::Op(I::End),
                    Ins::Op(I::LocalGet(65)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                    Ins::Op(I::LocalGet(129)),
                    call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                ],
            ),
            |f| {
                assert!(zero_releases(f, &mut None, &mut || {}));
            },
        );
        assert_eq!(releases(&f), 1);
        assert!(f.body.iter().any(|i| matches!(i, Ins::Op(I::LocalGet(65)))));
        assert!(
            !f.body
                .iter()
                .any(|i| matches!(i, Ins::Op(I::LocalGet(129))))
        );
    }

    #[test]
    fn zero_facts_preserve_source_origins() {
        let symbols = Symbols::default();
        let f = tracked(
            &symbols,
            1,
            0,
            vec![
                Ins::Op(I::I32Const(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
                Ins::Op(I::LocalGet(0)),
                call(&symbols, Sym::Helper(Helper::ReleaseRef)),
            ],
        );
        let f = run(f, |f| {
            zero_releases(f, &mut None, &mut || {});
        });
        assert_eq!(spans(&f), [12, 13]);
    }

    #[test]
    fn an_if_with_nothing_in_it_drops_its_test() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let empty = wasm_encoder::BlockType::Empty;
        // A pure test goes; a call's stays and its result
        // is dropped; an `if` with something in it stays.
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Load(mem(HEAP, 8, 2))),
                    op(I::I32Eqz),
                    op(I::If(empty)),
                    op(I::End),
                    call(&symbols, Sym::Helper(Helper::Alloc)),
                    op(I::If(empty)),
                    op(I::End),
                    op(I::LocalGet(0)),
                    op(I::If(empty)),
                    op(I::Unreachable),
                    op(I::End),
                ],
            ),
            |f| {
                drops(f);
            },
        );
        assert_eq!(
            format!("{:?}", f.body),
            format!(
                "{:?}",
                [
                    call(&symbols, Sym::Helper(Helper::Alloc)),
                    op(I::Drop),
                    op(I::LocalGet(0)),
                    op(I::If(empty)),
                    op(I::Unreachable),
                    op(I::End),
                ]
            )
        );
    }

    #[test]
    fn a_drop_takes_back_what_pushed_its_value() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        // A discarded field read; a tee that is dropped;
        // an addition of a read and a constant; a call's result stays.
        let f = run(
            tracked(
                &symbols,
                1,
                1,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Load(mem(HEAP, 8, 2))),
                    op(I::Drop),
                    op(I::I32Const(3)),
                    op(I::LocalTee(1)),
                    op(I::Drop),
                    op(I::LocalGet(0)),
                    op(I::I32Load(mem(HEAP, 0, 2))),
                    op(I::I32Const(4)),
                    op(I::I32Add),
                    op(I::Drop),
                    call(&symbols, Sym::Helper(Helper::Alloc)),
                    op(I::Drop),
                ],
            ),
            |f| {
                drops(f);
            },
        );
        assert_eq!(
            format!("{:?}", f.body),
            format!(
                "{:?}",
                [
                    op(I::I32Const(3)),
                    op(I::LocalSet(1)),
                    call(&symbols, Sym::Helper(Helper::Alloc)),
                    op(I::Drop),
                ]
            )
        );
        assert_eq!(spans(&f), [13, 15, 21, 22]);
        // A division can trap, so it stays and its result is dropped.
        let f = run(
            tracked(
                &symbols,
                1,
                0,
                vec![
                    op(I::LocalGet(0)),
                    op(I::I32Const(2)),
                    op(I::I32DivS),
                    op(I::Drop),
                ],
            ),
            |f| {
                drops(f);
            },
        );
        assert_eq!(f.body.len(), 4);
    }

    /// Rule 19 where its forms do not apply: a branch that carries a
    /// value, a value that branches to its own `if`, a test with two
    /// empty arms, and a test whose operand has no start in reach.
    #[test]
    fn a_short_circuit_value_stays_where_no_branch_chain_holds_it() {
        let symbols = Symbols::default();
        let op = Ins::Op;
        let i32 = BlockType::Result(wasm_encoder::ValType::I32);
        let value = |x: Vec<Ins>, k: i32| {
            let mut v = vec![op(I::LocalGet(0)), op(I::If(i32))];
            v.extend(x);
            v.extend([op(I::Else), op(I::I32Const(k)), op(I::End)]);
            v
        };
        let two = Ins::Call(symbols.intern(Sym::Inst(Hash128(7))), None);
        let arity = vec![Some((0, 2)); symbols.len()];
        let shaped = |params: usize, results: Vec<Val>, body: Vec<Ins>| {
            let mut f = tracked(&symbols, params, 0, body);
            f.results = results;
            let changed = shapes(&mut f, u32::try_from(params).unwrap(), &arity);
            (changed, f.body)
        };
        // `&&` falls through its `br_if`; the label takes the value under
        // the test, which a new `if` would hide.
        for (label, stays) in [(i32, true), (BlockType::Empty, false)] {
            let mut body = vec![op(I::Block(label)), op(I::I32Const(5))];
            body.extend(value(vec![op(I::LocalGet(1))], 0));
            body.extend([op(I::BrIf(0)), op(I::End)]);
            assert_eq!(shaped(2, vec![], body).0, !stays);
        }
        for (label, stays) in [
            (BlockType::FunctionType(1), true),
            (BlockType::Empty, false),
        ] {
            let mut body = vec![op(I::Loop(label))];
            body.extend(value(vec![op(I::LocalGet(1))], 0));
            body.extend([op(I::BrIf(0)), op(I::End)]);
            assert_eq!(shaped(2, vec![], body).0, !stays);
        }
        for (results, stays) in [(vec![Val::I32], true), (vec![], false)] {
            let mut body = value(vec![op(I::LocalGet(1))], 0);
            body.push(op(I::BrIf(0)));
            assert_eq!(shaped(2, results, body).0, !stays);
        }
        // `X` branches to its own `if` with the value.
        let mut body = vec![op(I::Block(BlockType::Empty))];
        body.extend(value(
            vec![op(I::LocalGet(1)), op(I::LocalGet(1)), op(I::BrIf(0))],
            0,
        ));
        body.extend([op(I::BrIf(0)), op(I::End)]);
        assert!(!shaped(2, vec![], body).0);
        // An `if` with no code on either arm.
        let mut body = value(vec![op(I::LocalGet(1))], 1);
        body.extend([op(I::If(BlockType::Empty)), op(I::End)]);
        assert!(!shaped(2, vec![], body).0);
        // `||` into an `if` needs a block around its operand: none past
        // a block of unknown type, nor past the walk's reach.
        let arms = [
            op(I::If(BlockType::Empty)),
            op(I::LocalGet(1)),
            op(I::Drop),
            op(I::End),
        ];
        let mut body = vec![op(I::Block(BlockType::FunctionType(1))), op(I::End)];
        body.extend(value(vec![op(I::LocalGet(1))], 1));
        body.extend(arms);
        body[2] = op(I::I32Eqz);
        assert!(!shaped(2, vec![], body).0);
        let mut body = vec![op(I::LocalGet(0))];
        body.extend((0..OPERAND_REACH).map(|_| op(I::I32Eqz)));
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        assert!(!shaped(2, vec![], body).0);
        let mut body = vec![op(I::LocalGet(0)), op(I::Block(BlockType::Empty))];
        body.extend((0..OPERAND_REACH).map(|_| op(I::I32Eqz)));
        body.extend([op(I::End), op(I::I32Eqz)]);
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        assert!(!shaped(2, vec![], body).0);
        // A call that pushes a value under the test as well.
        let mut body = vec![two];
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        body.push(op(I::Drop));
        assert!(!shaped(2, vec![], body).0);
        // Within reach, the same chain is one block, around a void block.
        let mut body = vec![
            op(I::LocalGet(0)),
            op(I::Block(BlockType::Empty)),
            op(I::End),
        ];
        body.push(op(I::I32Eqz));
        body.extend(value(vec![op(I::LocalGet(1))], 1).into_iter().skip(1));
        body.extend(arms);
        let (changed, body) = shaped(2, vec![], body);
        assert!(changed);
        assert!(matches!(body[0], Ins::Op(I::Block(BlockType::Empty))));
    }
    #[test]
    fn branch_only_shapes_compact_at_width_1000_depth_20_and_depth_50000() {
        let symbols = Symbols::default();
        let mut baseline = 0;
        for (width, depth) in [(100usize, 20u32), (1000, 20), (1, 50_000)] {
            let mut body = Vec::new();
            for _ in 0..width {
                body.push(Ins::Op(I::Block(BlockType::Empty)));
                for level in 1..=depth {
                    body.extend([
                        Ins::Op(I::LocalGet(0)),
                        Ins::Op(I::If(BlockType::Empty)),
                        Ins::Op(I::Br(level)),
                        Ins::Op(I::Else),
                    ]);
                }
                body.extend(std::iter::repeat_n(Ins::Op(I::End), depth as usize + 1));
            }
            let size = body.len();
            let mut f = tracked(&symbols, 1, 0, body);
            let (changed, work) = hd_wasm::work::count_control(|| batch_branch_shapes(&mut f));
            assert!(changed);
            assert!(
                work <= 4 * size as u64,
                "{width} x {depth}: {work} for {size} instructions"
            );
            if width == 100 {
                baseline = work;
            }
            if width == 1000 {
                assert!(work <= 10 * baseline);
            }
            assert_eq!(f.body.len(), width * (2 + 2 * depth as usize));
            assert!(
                !f.body
                    .iter()
                    .any(|ins| matches!(ins, Ins::Op(I::If(_) | I::Else)))
            );
            validate_void_block_fixture(&f);
            assert!(!batch_branch_shapes(&mut f));
        }
    }
    #[test]
    fn branch_only_compaction_preserves_a_targeted_if_label() {
        let symbols = Symbols::default();
        let mut f = tracked(
            &symbols,
            1,
            0,
            vec![
                Ins::Op(I::Block(BlockType::Empty)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::If(BlockType::Empty)),
                Ins::Op(I::LocalGet(0)),
                Ins::Op(I::BrIf(0)),
                Ins::Op(I::Else),
                Ins::Op(I::Br(1)),
                Ins::Op(I::End),
                Ins::Op(I::End),
            ],
        );
        validate_void_block_fixture(&f);
        assert!(!batch_branch_shapes(&mut f));
        assert!(matches!(f.body[2], Ins::Op(I::If(BlockType::Empty))));
        assert!(matches!(f.body[4], Ins::Op(I::BrIf(0))));
        assert!(matches!(f.body[6], Ins::Op(I::Br(1))));
        validate_void_block_fixture(&f);
    }
    #[test]
    fn branch_only_compaction_remaps_every_branch_table_target() {
        let symbols = Symbols::default();
        let mut f = tracked(&symbols, 1, 0, Vec::new());
        let table = f.intern_labels(vec![1, 1]);
        f.body = vec![
            Ins::Op(I::Block(BlockType::Empty)),
            Ins::Op(I::LocalGet(0)),
            Ins::Op(I::If(BlockType::Empty)),
            Ins::Op(I::Br(1)),
            Ins::Op(I::Else),
            Ins::Op(I::LocalGet(0)),
            Ins::Op(I::BrTable(table, 1)),
            Ins::Op(I::End),
            Ins::Op(I::End),
        ];
        f.origins = None;
        validate_void_block_fixture(&f);
        assert!(batch_branch_shapes(&mut f));
        let mapped = f
            .body
            .iter()
            .find_map(|ins| match ins {
                Ins::Op(I::BrTable(id, default)) => Some((*id, *default)),
                _ => None,
            })
            .expect("the table remains");
        assert_eq!(f.label_list(mapped.0), [0, 0]);
        assert_eq!(mapped.1, 0);
        validate_void_block_fixture(&f);
    }
}
