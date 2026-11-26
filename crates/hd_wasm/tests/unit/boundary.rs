mod discarded_tests {
    use hd_base::{DefId, Hash128};
    use hd_wasm::testing::boundary::*;
    use hd_wasm::testing::op::Op as I;

    use hd_tir::ir::{Body, Callee, NONE, PlaceRoot, Ref, Tag, local_flags};

    use hd_types::{Ty, TyList};

    use hd_wasm::testing::module::{HEAP, Ins, Module, Sym, Val, mem};
    use hd_wasm::testing::rt::Helper;

    use hd_base::{NodeIdx, Symbol};
    use hd_tir::ir::{BodyKind, Place};

    #[test]
    fn opaque_signature_nodes_use_declared_width_without_expansion() {
        let ty = LTy::Record(vec![
            LTy::Void,
            LTy::Never("unreturned".into()),
            LTy::Codec(Box::new(LCodec {
                vals: vec![Val::I64, Val::I32, Val::F64],
                size: 24,
                leaves: vec![],
                codec: Hash128(17),
            })),
            // No definition is installed for this recursive codec. Width
            // reads its declared ABI and must never resolve the definition.
            LTy::Named(Box::new(LNamed {
                id: 91,
                vals: vec![Val::I32, Val::I64],
            })),
            LTy::Str,
        ]);
        let mut visits = 0;
        assert_eq!(ty.value_count_with(&mut || visits += 1), 7);
        assert_eq!(ty.value_count(), ty.vals().len());
        assert_eq!(visits, 6);
    }

    #[test]
    fn signature_width_visits_wide_deep_records_once() {
        for (width, depth) in [(100, 20), (1_000, 20), (20, 1_000)] {
            let mut ty = LTy::Record((0..width).map(|_| LTy::Str).collect());
            for _ in 0..depth {
                ty = LTy::Record(vec![ty]);
            }
            let mut visits = 0;
            assert_eq!(ty.value_count_with(&mut || visits += 1), width * 2);
            assert_eq!(visits, width + depth + 1);
            // Drain the test's owned tree iteratively as well.
            let mut work = vec![ty];
            while let Some(ty) = work.pop() {
                if let LTy::Record(fields) = ty {
                    work.extend(fields);
                }
            }
        }
    }

    fn inst(b: &mut Body, tag: Tag, a: u32, w: u32) -> u32 {
        let i = u32::try_from(b.len()).expect("instruction");
        b.tags.push(tag);
        b.data.push([a, w]);
        b.ty.push(Ty::VOID);
        b.syn.push(NodeIdx::NONE);
        i
    }

    fn place(b: &mut Body, text: &str) -> u32 {
        let p: Place = text.parse().expect("place");
        match (p.words(), p.root) {
            (Some(words), _) => b.push_record(&words),
            (None, PlaceRoot::Local(l)) => l.raw() | hd_tir::ir::place::BARE_LOCAL,
            _ => unreachable!("place words"),
        }
    }

    fn body() -> Body {
        let mut b = Body::new(DefId::from_raw(0), BodyKind::Fn);
        b.local_ty.push(Ty::VOID);
        b.local_name.push(Symbol::from_raw(0));
        b.local_syn.push(NodeIdx::NONE);
        b.local_flags.push(local_flags::ASSIGNED);
        inst(&mut b, Tag::Hole, NONE, NONE);
        b
    }

    fn finish(b: &mut Body, tail: u32) {
        let list = b.push_record(&(0..u32::try_from(b.len()).expect("body")).collect::<Vec<_>>());
        let root = inst(b, Tag::Block, list, tail);
        b.sub_root.push(root);
    }

    #[test]
    fn tag_tests_copies_and_drops_do_not_observe_payload_bytes() {
        let mut b = body();
        let destination = place(&mut b, "[$0]");
        inst(&mut b, Tag::Store, destination, 0);
        let source = place(&mut b, "[$0 as .0 .0]");
        let read = inst(&mut b, Tag::Read, source, 0);
        let branches = b.push_record(&[0, NONE, NONE]);
        inst(&mut b, Tag::SwitchTag, read, branches);
        inst(&mut b, Tag::Drop, destination, NONE);
        finish(&mut b, NONE);
        assert!(tags_only(&b, 0));
    }

    #[test]
    fn references_captures_returns_and_calls_keep_owned_decoding() {
        for use_ in [Tag::Ref, Tag::Return, Tag::Call] {
            let mut b = body();
            let p = place(&mut b, "[%0]");
            match use_ {
                Tag::Ref => {
                    inst(&mut b, Tag::Ref, p, 0);
                }
                Tag::Return => {
                    inst(&mut b, Tag::Return, 0, NONE);
                }
                Tag::Call => {
                    let args = b.push_record(&[0, 0, 0, 0]);
                    inst(&mut b, Tag::Call, NONE, args);
                }
                _ => unreachable!(),
            }
            finish(&mut b, NONE);
            assert!(!tags_only(&b, 0), "{use_:?}");
        }
        let mut b = body();
        let p = place(&mut b, "[$0]");
        inst(&mut b, Tag::Store, p, 0);
        b.local_flags[0] |= local_flags::CAPTURED;
        finish(&mut b, NONE);
        assert!(!tags_only(&b, 0));
        let mut b = body();
        finish(&mut b, 0);
        assert!(!tags_only(&b, 0));
    }

    #[test]
    fn a_payload_used_as_an_element_key_is_an_observation() {
        let mut b = body();
        let other = inst(&mut b, Tag::Hole, NONE, NONE);
        let destination = place(&mut b, &format!("[%{other} [%0]]"));
        inst(&mut b, Tag::Store, destination, other);
        finish(&mut b, NONE);
        assert!(!tags_only(&b, 0));
    }

    #[test]
    fn dynamic_function_values_are_not_callee_records() {
        for value in [Ref::konst(0).0, 0, 1, NONE] {
            let mut b = body();
            let call = inst(&mut b, Tag::CallValue, value, NONE);
            assert_eq!(dynamic_method(&b, call), None);
        }
        let mut b = body();
        for row in [Ref::CONST_BIT, NONE, 0] {
            let call = inst(&mut b, Tag::Call, row, NONE);
            assert_eq!(dynamic_method(&b, call), None);
        }
        let method = DefId::from_raw(19);
        let callee = Callee::TraitMethod {
            trait_: DefId::from_raw(7),
            method,
            self_ty: Ty::VOID,
            targs: TyList::EMPTY,
            choice: (hd_tir::ir::ChoiceKind::TraitValue, 0),
        };
        let row = b.push_record(&callee.words());
        for tag in [Tag::Call, Tag::Await] {
            let call = inst(&mut b, tag, row, NONE);
            assert_eq!(dynamic_method(&b, call), Some(method));
        }
    }

    fn result_type() -> LTy {
        let string = hd_wasm::testing::layout::Mem::string;
        let error_layout =
            hd_wasm::testing::layout::flat_of(&[vec![], vec![string()]]).expect("fits");
        let outer = hd_wasm::testing::layout::flat_of(&[vec![], vec![error_layout.mem.clone()]])
            .expect("fits");
        let error = LTy::Enum(Box::new(LEnum {
            layout: Box::new(error_layout),
            variants: vec![vec![], vec![LTy::Str]],
        }));
        LTy::Enum(Box::new(LEnum {
            layout: Box::new(outer),
            variants: vec![vec![], vec![error]],
        }))
    }

    /// Walk the emitted helper graph separately from unrelated host frame edges.
    #[test]
    fn discarded_error_payload_decoder_has_no_owned_string_allocation_edge() {
        fn reaches_allocation(ty: LTy) -> bool {
            let mut module = Module::default();
            let mut pending = vec![Helper::Dec(ty)];
            #[allow(
                clippy::mutable_key_type,
                reason = "Walk keys identify immutable recipes; mutexes protect append-only nodes and memoized results only."
            )]
            let mut seen = std::collections::BTreeSet::new();
            while let Some(helper) = pending.pop() {
                if !seen.insert(helper.clone()) {
                    continue;
                }
                if matches!(helper, Helper::Str | Helper::New | Helper::NewFixed { .. }) {
                    return true;
                }
                let body = hd_wasm::testing::rt::helper(&helper, &mut module);
                for instruction in &body.body {
                    if let Ins::Call(symbol, _) = instruction
                        && let Sym::Helper(callee) = body.symbols.get(*symbol)
                    {
                        pending.push(callee.clone());
                    }
                }
            }
            false
        }
        let owned = result_type();
        assert!(reaches_allocation(owned.clone()));
        let mut discarded = owned;
        discard_strings(&mut discarded);
        assert!(!reaches_allocation(discarded));
    }

    #[test]
    fn discarded_and_owned_decoder_keys_are_distinct_and_validate_the_same_wire() {
        let owned = result_type();
        let mut discarded = owned.clone();
        discard_strings(&mut discarded);
        assert_ne!(owned, discarded);
        assert_eq!(owned.vals(), discarded.vals());
        let mut m = Module::default();
        m.export("owned", Sym::Helper(Helper::Dec(owned)));
        m.export("discarded", Sym::Helper(Helper::Dec(discarded)));
        let mut byte = m.func("byte", vec![Val::I32], vec![Val::I32]);
        byte.get(0);
        byte.op(I::I32Load8U(mem(HEAP, 0, 0)));
        m.define(Sym::Inst(Hash128(77)), byte);
        m.export("byte", Sym::Inst(Hash128(77)));
        let wasm = m.finish().expect("decoder module");
        let script = format!(
            r"
const bytes = new Uint8Array({wasm:?});
const module = new WebAssembly.Module(bytes);
let panics = 0;
const imports = {{}};
for (const i of WebAssembly.Module.imports(module)) {{
  imports[i.module] ??= {{}};
  imports[i.module][i.name] = () => {{ if (i.name === 'panic') panics++; return 0; }};
}}
const e = new WebAssembly.Instance(module, imports).exports;
// The result is `string??`'s values (the outer tag, then the inner
// optional's string, whose non-null address says `.Some`) and the bytes read.
const buffer = new Uint8Array(e['hd.x'].buffer);
const text = 'failure λ 🦀';
const utf8 = new TextEncoder().encode(text);
const reply = new Uint8Array([1, 1, utf8.length, ...utf8]);
buffer.set(reply);
const full = e.owned(0);
if (full[0] !== 1 || full[1] === 0 || full[2] !== utf8.length || full[3] !== reply.length) throw Error('owned result shape');
buffer.fill(90);
const kept = new Uint8Array(Array.from({{length:full[2]}}, (_, i) => e.byte(full[1] + i)));
if (new TextDecoder().decode(kept) !== text) throw Error('owned error lost after another host buffer write');
buffer.set(reply);
const skipped = e.discarded(0);
if (skipped[0] !== 1 || skipped[1] === 0 || skipped[2] !== 0 || skipped[3] !== reply.length) throw Error('discarded result shape');
// Invalid outer/inner tags and a string length exceeding the exchange memory.
for (const bad of [[2], [1,2], [1,1,128,128,8]]) {{
  for (const decode of [e.owned, e.discarded]) {{
    buffer.fill(0); buffer.set(bad);
    const before = panics;
    let failed = false;
    try {{ decode(0); }} catch {{ failed = true; }}
    if (!failed || panics !== before + 1) throw Error('malformed reply was accepted');
  }}
}}
console.log('owned and discarded boundary decoders passed');
"
        );
        let output = hd_testkit::node::command()
            .args(["--input-type=module", "-e", &script])
            .output()
            .expect("Node available");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "owned and discarded boundary decoders passed"
        );
    }
}

mod package_interface_tests {
    use hd_wasm::testing::boundary::*;

    use hd_wasm::testing::module::Module;

    use hd_host_abi::schema::{Identity, Interface};

    fn interface(i: usize, handle: bool) -> Interface {
        let identity = Identity {
            package: "interfaces".into(),
            version: "1.0.0".into(),
            trait_path: format!("Trait{i}"),
        };
        Interface {
            module: package_module(&identity, handle),
            identity,
            selectors: vec![],
            handle,
            methods: vec![],
        }
    }

    #[test]
    fn package_interface_recording_is_linear_and_preserves_order() {
        for width in [1_000, 10_000] {
            let mut m = Module::default();
            // Provider and returned-handle roles of the same trait must
            // remain distinct. Repeat records in reverse order as well.
            for i in (0..width).rev() {
                for handle in [false, true] {
                    record_package_interface(&mut m, interface(i, handle));
                }
            }
            for i in 0..width {
                for handle in [true, false] {
                    record_package_interface(&mut m, interface(i, handle));
                }
            }
            assert_eq!(m.package_interface_probes, 4 * width);
            assert_eq!(m.package_interface_at.len(), 2 * width);
            assert_eq!(m.package_contract.interfaces.len(), 2 * width);
            for (at, got) in m.package_contract.interfaces.iter().enumerate() {
                assert_eq!(*got, interface(width - at / 2 - 1, at % 2 == 1));
                assert_eq!(m.package_interface_at[&got.module], at);
            }
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "two package host interfaces for one module")]
    fn package_interface_duplicate_must_have_the_same_schema() {
        let mut m = Module::default();
        let mut value = interface(0, false);
        record_package_interface(&mut m, value.clone());
        value.selectors.push("different".into());
        record_package_interface(&mut m, value);
    }
}
