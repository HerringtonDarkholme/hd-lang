mod row;

mod checked_fixed_allocation_tests {
    use hd_base::Hash128;
    use hd_wasm::testing::op::Op as Instruction;
    use hd_wasm::testing::rt::*;

    use hd_wasm::testing::module::{Ins, Module, Sym};

    #[test]
    fn fixed_requests_use_the_checked_allocator_without_class_bits() -> Result<(), String> {
        let keys: Vec<_> = hd_wasm::runtime_items(true, false)
            .iter()
            .enumerate()
            .map(|(i, _)| u128::try_from(i).map(|n| Some(Hash128(n + 1))))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let runtime = hd_wasm::Runtime::from_keys(&keys).ok_or("missing checked runtime key")?;
        let mut m = Module::new(&runtime);
        for size in [0, 16, 2048, u32::MAX] {
            let f = helper(&Helper::NewFixed { size }, &mut m);
            assert_eq!(f.name, "rt:new");
            assert!(f.body.iter().any(|ins| matches!(ins,
                Ins::Call(id, _) if *f.symbol(*id) == Sym::Inst(runtime.alloc))));
            assert!(
                !f.body.iter().any(|ins| matches!(ins,
                Ins::Call(id, _) if matches!(*f.symbol(*id), Sym::Helper(Helper::AllocClass { .. }))))
            );
            assert!(
                !f.body
                    .iter()
                    .any(|ins| matches!(ins, Ins::Op(Instruction::I32Or)))
            );
        }
        let mut unchecked = Module::default();
        let fast = helper(&Helper::NewFixed { size: 16 }, &mut unchecked);
        assert!(fast.name.starts_with("rt:new/class-"));
        assert!(fast.body.iter().any(|ins| matches!(ins,
            Ins::Call(id, _) if matches!(*fast.symbol(*id), Sym::Helper(Helper::AllocClass { .. })))));
        Ok(())
    }
}
