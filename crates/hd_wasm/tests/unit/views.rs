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
    use hd_tir::own::Reason;
    use hd_wasm::testing::module::{HEAP, mem};
    use hd_wasm::testing::rt::{Helper, helper};

    fn attributed_release_module(
        tracked: bool,
        wrapper_site: bool,
        named: bool,
    ) -> hd_wasm::MappedModule {
        let mut module = Module::default();
        module.track_origins = tracked;
        let key = Hash128(2372);
        let shim = Sym::Shim(key, false, 1, true);
        let mut drop = module.func("release Buffer", vec![Val::I32], vec![]);
        // A release shim clears a payload slot; retain that observable effect.
        drop.get(0);
        drop.u32(0);
        drop.op(hd_wasm::testing::op::Op::I32Store(mem(HEAP, 0, 2)));
        module.define(shim, drop);
        let wrapper = Helper::ReleaseSpilled {
            size: 0,
            leaves: vec![],
            shim: key,
        };
        let mut body = helper(&wrapper, &mut module);
        // Named runtime ownership is tested on an adapter, like boundary cleanup.
        // Module relinking regenerates typed helpers from their constructors.
        if named {
            let call = body.body.iter().position(|ins| matches!(ins, hd_wasm::testing::module::Ins::Call(id, _) if matches!(body.symbol(*id), Sym::Shim(..)))).expect("release shim call");
            body.note_runtime_count(
                call,
                CountAction::Release,
                "owned payload",
                Reason::Overwritten,
            );
        }
        let symbol = if wrapper_site {
            Sym::Helper(wrapper)
        } else {
            Sym::Inst(Hash128(2374))
        };
        module.define(symbol.clone(), body);
        let mut caller = module.func("app/drop_buffer", vec![], vec![]);
        caller.origins = Some(vec![]);
        caller.origin_at = Some(0);
        caller.origin_root = Some(0);
        caller.origin_item = Some("app/drop_buffer".into());
        let start = caller.body.len();
        caller.call(symbol);
        if wrapper_site {
            caller.note_count(
                start,
                1,
                CountAction::Release,
                "Buffer",
                Some(Reason::LastUse),
            );
        }
        let caller_key = Sym::Inst(Hash128(2373));
        module.define(caller_key.clone(), caller);
        module.export("drop_buffer", caller_key);
        let mapped = module
            .finish_with_origins()
            .expect("encode actual runtime helper");
        wasmparser::Validator::new()
            .validate_all(&mapped.wasm)
            .expect("valid Wasm");
        mapped
    }

    fn executable(wasm: &[u8]) -> Vec<u8> {
        let mut module = wasm_encoder::Module::new();
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            if let Some((id, range)) = payload.expect("module section").as_section()
                && id != 0
            {
                module.section(&wasm_encoder::RawSection {
                    id,
                    data: &wasm[usize::try_from(range.start).unwrap()
                        ..usize::try_from(range.end).unwrap()],
                });
            }
        }
        module.finish()
    }

    #[test]
    fn attribution_preserves_real_non_wrapper_releases_and_named_internal_ownership() {
        for (wrapper_site, named) in [(false, false), (false, true)] {
            let mapped = attributed_release_module(true, wrapper_site, named);
            let body = mapped
                .functions
                .iter()
                .find(|f| f.name == "rt:release_spilled")
                .expect("helper");
            assert_eq!(
                body.counts.len(),
                1,
                "the real inner release remains visible: wrapper={wrapper_site} named={named}"
            );
            assert_eq!(
                body.counts[0].reason,
                if named {
                    Reason::Overwritten
                } else {
                    Reason::Conservative("source call")
                }
            );
        }
    }

    #[test]
    fn attribution_preserves_executable_wasm() {
        for (wrapper_site, named) in [(false, false), (false, true), (true, false)] {
            assert_eq!(
                executable(&attributed_release_module(false, wrapper_site, named).wasm),
                executable(&attributed_release_module(true, wrapper_site, named).wasm)
            );
        }
    }

    fn count_wrapper(h: Helper, tracked: bool) -> hd_wasm::MappedModule {
        let mut module = Module::default();
        module.track_origins = tracked;
        let runtime = module.runtime;
        // Give the assembler harness's count/free endpoints observable effects.
        // This tests encoder attribution, not a replacement runtime's semantics.
        for (key, result) in [
            (runtime.release, true),
            (runtime.release_known, true),
            (runtime.acyclic.release, true),
            (runtime.acyclic.release_known, true),
            (runtime.free, false),
            (runtime.acyclic.free, false),
        ] {
            let mut function = module.func(
                "test count endpoint",
                vec![Val::I32],
                if result { vec![Val::I32] } else { vec![] },
            );
            function.get(0);
            function.u32(0);
            function.op(hd_wasm::testing::op::Op::I32Store(mem(HEAP, 0, 2)));
            if result {
                function.u32(0);
            }
            module.define(Sym::Inst(key), function);
        }
        let shim = Sym::Shim(Hash128(2372), false, 1, true);
        let mut function = module.func("test release shim", vec![Val::I32], vec![]);
        function.get(0);
        function.u32(0);
        function.op(hd_wasm::testing::op::Op::I32Store(mem(HEAP, 0, 2)));
        module.define(shim, function);
        let (params, results) = hd_wasm::testing::rt::arity(&h);
        assert_eq!(results, 0);
        let mut caller = module.func("app/release", vec![Val::I32; params], vec![]);
        caller.origins = Some(vec![]);
        caller.origin_at = Some(0);
        caller.origin_root = Some(0);
        caller.origin_item = Some("app/release".into());
        for i in 0..params {
            caller.get(u32::try_from(i).unwrap());
        }
        let start = caller.body.len();
        caller.call(Sym::Helper(h));
        caller.note_count(
            start,
            1,
            CountAction::Release,
            "owned value",
            Some(Reason::LastUse),
        );
        let key = Sym::Inst(Hash128(2373));
        module.define(key.clone(), caller);
        module.export("release", key);
        let mapped = module
            .finish_with_origins()
            .expect("encode release wrapper");
        wasmparser::Validator::new()
            .validate_all(&mapped.wasm)
            .expect("valid wrapper module");
        mapped
    }

    #[test]
    fn every_release_wrapper_keeps_the_callers_site_without_an_internal_duplicate() {
        use hd_wasm::testing::layout::Leaf;
        use hd_wasm::testing::rt::Counted;
        let leaf = Leaf {
            offset: 0,
            val: Val::I32,
            bytes: 4,
            signed: false,
        };
        let helpers = [
            Helper::ReleaseRef,
            Helper::ReleaseKnown,
            Helper::ReleaseFresh,
            Helper::CycleRelease,
            Helper::DropParts {
                leaves: vec![leaf],
                parts: vec![(
                    0,
                    Counted::Address {
                        index: 0,
                        known: true,
                    },
                )],
            },
            Helper::ReleaseSpilled {
                size: 4,
                leaves: vec![leaf],
                shim: Hash128(2372),
            },
        ];
        for h in helpers {
            let wrapper_name = helper(&h, &mut Module::default()).name;
            let plain = count_wrapper(h.clone(), false);
            let mapped = count_wrapper(h.clone(), true);
            assert_eq!(executable(&plain.wasm), executable(&mapped.wasm), "{h:?}");
            let caller = mapped
                .functions
                .iter()
                .find(|f| f.name == "app/release")
                .expect("caller");
            assert_eq!(caller.counts.len(), 1, "{h:?}");
            assert_eq!(caller.counts[0].reason, Reason::LastUse, "{h:?}");
            let duplicates: Vec<_> = mapped
                .functions
                .iter()
                .filter(|f| f.name.split('#').next() == Some(wrapper_name.as_str()))
                .flat_map(|f| &f.counts)
                .filter(|c| {
                    c.action == CountAction::Release && matches!(c.reason, Reason::Conservative(_))
                })
                .collect();
            assert!(duplicates.is_empty(), "{h:?}: {duplicates:?}");
        }
    }
}
