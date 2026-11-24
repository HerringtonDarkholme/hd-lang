mod tests {
    use hd_base::Hash128;
    use hd_wasm::testing::module::{Module, Sym, Val};
    use hd_wasm::views::CountAction;

    /// The runtime module is intentionally std-only in hd source. Exercise a
    /// direct free at the real backend encoder, including acyclic relinking.
    #[test]
    fn a_final_free_call_keeps_its_source_and_block_label() {
        let mut module = Module::default();
        let runtime = module.runtime;
        module.track_origins = true;
        let free = module.func("free", vec![Val::I32], vec![]);
        module.define(Sym::Inst(runtime.free), free);
        let mut caller = module.func("app/free_block", vec![Val::I32], vec![]);
        caller.origins = Some(Vec::new());
        caller.origin_at = Some(0);
        caller.origin_root = Some(0);
        caller.origin_item = Some("app/free_block".into());
        caller.get(0);
        caller.call(Sym::Inst(runtime.free));
        let key = Sym::Inst(Hash128(100));
        module.define(key.clone(), caller);
        module.export("free_block", key);
        let mapped = module.finish_with_origins().expect("encode");
        wasmparser::Validator::new()
            .validate_all(&mapped.wasm)
            .expect("valid module");
        let function = mapped
            .functions
            .iter()
            .find(|f| f.name == "app/free_block")
            .expect("caller");
        assert_eq!(function.counts.len(), 1);
        let count = &function.counts[0];
        assert_eq!(count.action, CountAction::Free);
        assert_eq!(count.subject, "block");
        assert_eq!(count.origin, Some(0));
        assert!(
            function
                .instructions
                .iter()
                .any(|i| i.offset == count.offset)
        );
    }
}
