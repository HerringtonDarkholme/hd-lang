#[cfg(feature = "checked-report")]
mod checked_descriptor_tests {

    use hd_wasm::testing::module::*;

    use std::collections::BTreeMap;

    #[test]
    fn kinds_alias_one_release_function_in_distinct_slots() {
        let glue = SymId(3);
        let mut index = Numbers::new();
        index.insert(glue, 7);
        let mut slots = Vec::new();
        let mut ids = BTreeMap::new();
        assert_eq!(
            descriptor_slot(
                glue,
                DescriptorKey::checked(1),
                0,
                &index,
                &mut slots,
                &mut ids
            )
            .unwrap(),
            0
        );
        assert_eq!(
            descriptor_slot(
                glue,
                DescriptorKey::checked(2),
                0,
                &index,
                &mut slots,
                &mut ids
            )
            .unwrap(),
            1
        );
        assert_eq!(
            descriptor_slot(
                glue,
                DescriptorKey::checked(1),
                0,
                &index,
                &mut slots,
                &mut ids
            )
            .unwrap(),
            0
        );
        assert_eq!(slots, [7, 7]);
    }

    #[test]
    fn extension_descriptors_follow_the_initialized_table_prefix() {
        let glue = SymId(3);
        let mut index = Numbers::new();
        index.insert(glue, 7);
        let mut slots = Vec::new();
        let mut ids = BTreeMap::new();
        assert_eq!(
            descriptor_slot(
                glue,
                DescriptorKey::checked(1),
                41,
                &index,
                &mut slots,
                &mut ids
            )
            .unwrap(),
            41
        );
        assert_eq!(
            descriptor_slot(
                glue,
                DescriptorKey::checked(2),
                41,
                &index,
                &mut slots,
                &mut ids
            )
            .unwrap(),
            42
        );
        assert_eq!(
            descriptor_slot(
                glue,
                DescriptorKey::checked(1),
                41,
                &index,
                &mut slots,
                &mut ids
            )
            .unwrap(),
            41
        );
        assert_eq!(slots, [7, 7]);
    }
}

mod dense_symbol_tests {
    #[test]
    fn emitted_wide_signature_wins_over_logical_helper_metadata() {
        let symbols = hd_wasm::testing::module::Symbols::default();
        let target = symbols.intern(hd_wasm::testing::module::Sym::Helper(
            hd_wasm::testing::rt::Helper::DynTrap {
                params: vec![hd_wasm::testing::module::Val::I32; 64],
                results: vec![hd_wasm::testing::module::Val::I32; 65],
            },
        ));
        assert_eq!(symbols.call_arities(&[])[target.idx()], Some((64, 65)));
        let mut physical = hd_wasm::testing::module::Func::default();
        physical.params = vec![hd_wasm::testing::module::Val::I32; 65];
        let functions = vec![Some(physical)];
        // Wide results add one frame address even when the parameter
        // count was exactly the threshold; do not lower it a second time.
        assert_eq!(
            symbols.call_arities(&functions)[target.idx()],
            Some((65, 0))
        );
    }

    use hd_wasm::testing::module::*;
    use hd_wasm::testing::op::Op as Instruction;

    use hd_base::Hash128;

    fn encoded(reverse_interning: bool) -> Vec<u8> {
        let mut m = Module::default();
        let a = Sym::Import(Import::new("test", "a", &[], &[Val::I32]));
        let b = Sym::Import(Import::new("test", "b", &[], &[Val::I32]));
        if reverse_interning {
            let _ = m.symbols.intern(b.clone());
            let _ = m.symbols.intern(a.clone());
            let _ = m.symbols.intern_global(Global::HashSeed);
        } else {
            let _ = m.symbols.intern(a.clone());
            let _ = m.symbols.intern(b.clone());
            let _ = m.symbols.intern_global(Global::Forbid);
        }
        let leaf = Sym::Inst(Hash128(1));
        let root = Sym::Inst(Hash128(2));
        let mut f = m.func("leaf", vec![], vec![Val::I32]);
        f.u32(7);
        m.define(leaf.clone(), f);
        let mut f = m.func("root", vec![], vec![Val::I32]);
        f.global_get(Global::Forbid);
        f.op(Instruction::Drop);
        f.global_get(Global::HashSeed);
        f.op(Instruction::Drop);
        f.call(b);
        f.op(Instruction::Drop);
        f.table_slot(leaf);
        f.call_indirect(vec![], vec![Val::I32]);
        f.op(Instruction::Drop);
        f.op(Instruction::Block(wasm_encoder::BlockType::Empty));
        f.u32(0);
        f.br_table(vec![0, 0], 0);
        f.op(Instruction::End);
        f.call(a);
        m.define(root.clone(), f);
        m.export("root", root);
        m.finish().expect("module encodes")
    }

    #[test]
    fn arena_insertion_order_does_not_change_wasm_numbering() {
        let a = encoded(false);
        let b = encoded(true);
        assert_eq!(a, b);
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&a)
            .expect("valid Wasm");
    }

    #[test]
    fn reachability_bits_cross_word_boundaries() {
        let mut reached = Reached::default();
        for id in [0, 63, 64, 130] {
            assert!(reached.insert(SymId(id)));
            assert!(!reached.insert(SymId(id)));
        }
        assert!(!reached.contains(SymId(62)));
        assert_eq!(
            reached.iter().map(|id| id.0).collect::<Vec<_>>(),
            [0, 63, 64, 130]
        );
    }

    #[test]
    fn cancellation_fact_is_specialized_and_markers_are_not_linked() {
        for explicit in [false, true] {
            let mut m = Module::default();
            let root = Sym::Inst(Hash128(42));
            let mut f = m.func("probe", vec![], vec![Val::I32]);
            if explicit {
                f.helper(hd_wasm::testing::rt::Helper::CancellationRoot);
            }
            f.helper(hd_wasm::testing::rt::Helper::CanCancel);
            m.define(root.clone(), f);
            m.export("probe", root);
            let wasm = m.finish().expect("module encodes");
            wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
                .validate_all(&wasm)
                .expect("valid Wasm");
            let bodies: Vec<_> = wasmparser::Parser::new(0)
                .parse_all(&wasm)
                .filter_map(|p| match p.expect("payload") {
                    wasmparser::Payload::CodeSectionEntry(body) => Some(body),
                    _ => None,
                })
                .collect();
            assert_eq!(bodies.len(), 1, "link-time helpers disappear");
            let ops: Vec<_> = bodies[0]
                .get_operators_reader()
                .expect("operators")
                .into_iter()
                .collect::<Result<_, _>>()
                .expect("valid operators");
            assert!(
                matches!(ops.as_slice(), [wasmparser::Operator::I32Const { value }, wasmparser::Operator::End] if *value == i32::from(explicit))
            );
        }
    }

    #[test]
    fn final_reachability_removes_a_call_folded_out_of_the_body() {
        let mut m = Module::default();
        let dead = Sym::Inst(Hash128(1));
        let root = Sym::Inst(Hash128(2));
        let f = m.func("dead_after_fold", vec![], vec![]);
        m.define(dead.clone(), f);
        let mut f = m.func("root", vec![], vec![]);
        f.i32(0);
        f.op(Instruction::If(wasm_encoder::BlockType::Empty));
        f.call(dead);
        f.op(Instruction::End);
        m.define(root.clone(), f);
        m.export("root", root);
        let wasm = m.finish().expect("module encodes");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&wasm)
            .expect("valid Wasm");
        let functions = wasmparser::Parser::new(0)
            .parse_all(&wasm)
            .filter(|p| matches!(p, Ok(wasmparser::Payload::CodeSectionEntry(_))))
            .count();
        assert_eq!(functions, 1, "only the exported root is still linked");
    }
}
