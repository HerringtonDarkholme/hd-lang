mod tests {
    use hd_wasm::testing::module::{Func, Module, Sym, Val};
    use hd_wasm::testing::op::Op as I;
    use hd_wasm::testing::rt::Helper;
    use hd_wasm::testing::rt::row::*;

    use hd_wasm::testing::module::Ins;

    fn recipe(width: usize, depth: usize, duplicate_suppliers: bool) -> RowAdapter {
        RowAdapter {
            params: vec![Val::I32, Val::I32],
            results: vec![Val::I32],
            entries: (0..width)
                .map(|key| RowEntry {
                    required: i64::try_from(key).unwrap(),
                    supplier: if duplicate_suppliers {
                        0
                    } else {
                        i64::try_from(key).unwrap()
                    },
                    projection: vec![4; depth],
                })
                .collect(),
            open: true,
            code: None,
        }
    }

    #[test]
    fn dynamic_and_constant_code_helpers_validate() {
        let mut module = Module::default();
        let code = hd_base::Hash128(0x1939);
        let mut original = module.func("original", vec![Val::I32, Val::I32], vec![Val::I32]);
        original.i32(0);
        module.define(Sym::Inst(code), original);
        for (index, known) in [false, true].into_iter().enumerate() {
            let mut row = recipe(33, 20, false);
            row.code = known.then_some(code);
            let mut caller = module.func("call_adapter", row.params.clone(), row.results.clone());
            // A real dynamic original contributes a reachable table slot.
            caller.table_slot(Sym::Inst(code));
            caller.op(I::Drop);
            for parameter in 0..row.params.len() {
                caller.get(u32::try_from(parameter).expect("parameters"));
            }
            caller.helper(Helper::RowAdapter(row));
            let sym = Sym::Inst(hd_base::Hash128(0x1986 + index as u128));
            module.define(sym.clone(), caller);
            module.export(format!("adapter{index}"), sym);
        }
        let bytes = module.finish().expect("row helpers link");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("row helpers validate");
    }

    #[test]
    fn gather_code_is_linear_and_scratch_locals_are_constant() {
        let module = Module::default();
        let small = build(&module, &recipe(100, 20, false));
        let large = build(&module, &recipe(1000, 20, false));
        assert_eq!(small.locals.len(), large.locals.len());
        let count = |function: &Func, op: I| {
            function.body.iter()
            .filter(|ins| matches!(ins, Ins::Op(actual) if std::mem::discriminant(actual) == std::mem::discriminant(&op))).count()
        };
        assert_eq!(count(&large, I::I64Eq), 1000);
        assert_eq!(count(&large, I::I64LtS), 1000);
        assert!(large.body.len() < small.body.len() * 11);
        let shared = build(&module, &recipe(1000, 20, true));
        assert_eq!(count(&shared, I::I64Eq), 1);
        assert_eq!(shared.locals.len(), large.locals.len());
        for function in [&small, &large, &shared] {
            let helpers: Vec<_> = function
                .body
                .iter()
                .filter_map(|ins| {
                    if let Ins::Call(id, _) = ins
                        && let Sym::Helper(helper) = function.symbol(*id)
                    {
                        return Some(helper.clone());
                    }
                    None
                })
                .collect();
            assert_eq!(
                helpers
                    .iter()
                    .filter(|h| matches!(h, Helper::ListBuf { .. }))
                    .count(),
                1
            );
            assert!(!helpers.contains(&Helper::CtxFind));
        }
    }
}
