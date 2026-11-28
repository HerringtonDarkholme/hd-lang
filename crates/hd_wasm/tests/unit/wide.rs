mod tests {
    use hd_wasm::testing::wide::*;

    use hd_wasm::testing::op::Op as I;

    use hd_wasm::testing::module::Module;
    use hd_wasm::testing::module::{Global, Ins, Sym, Val};
    use wasm_encoder::BlockType;

    #[test]
    fn wide_expansions_preserve_origins_without_changing_instructions() {
        let m = Module::default();
        let mut f = m.func("storage", vec![], vec![Val::I32]);
        f.locals = vec![Val::I32; WIDTH + 1];
        f.stack_values = true;
        f.origin_root = Some(99);
        f.body = vec![
            Ins::Op(I::I32Const(7)),
            Ins::Op(I::LocalSet(0)),
            Ins::Op(I::LocalGet(0)),
        ];
        f.origins = Some(vec![Some(10), Some(11), Some(12)]);
        let mut plain = f.clone();
        plain.origins = None;
        storage(&mut plain).unwrap();
        storage(&mut f).unwrap();
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
        let expected = [
            vec![Some(99); 18],
            vec![Some(10)],
            vec![Some(11); 4],
            vec![Some(12); 2],
            vec![Some(99); 4],
        ]
        .concat();
        assert_eq!(f.origins.as_ref().unwrap(), &expected);
        assert_eq!(f.body.len(), expected.len());

        let mut f = m.func("callee", vec![Val::I32; WIDTH + 1], vec![]);
        f.origin_root = Some(99);
        f.body = vec![
            Ins::Op(I::LocalGet(u32::try_from(WIDTH).unwrap())),
            Ins::Op(I::Drop),
        ];
        f.origins = Some(vec![Some(10), Some(11)]);
        callee(&mut f).unwrap();
        let spans = f.origins.as_ref().unwrap();
        assert_eq!(spans.len(), f.body.len());
        assert!(spans[..3 * (WIDTH + 1)].iter().all(|s| *s == Some(99)));
        assert_eq!(&spans[3 * (WIDTH + 1)..], [Some(10), Some(11)]);

        let target = Sym::Inst(hd_base::Hash128(7));
        let mut f = m.func("caller", vec![], vec![]);
        let target = f.intern(target);
        f.body = vec![Ins::Call(target, None)];
        f.origins = Some(vec![Some(17)]);
        let mut sig = vec![None; target.idx() + 1];
        sig[target.idx()] = Some((vec![Val::I32; WIDTH + 1], vec![]));
        calls(&mut f, &sig).unwrap();
        assert_eq!(f.origins.as_ref().unwrap().len(), f.body.len());
        assert!(f.origins.unwrap().iter().all(|s| *s == Some(17)));
    }

    use hd_base::Hash128;
    use std::io::Write;

    #[test]
    fn shadow_storage_zeroes_each_entry_and_restores_every_exit() {
        let mut m = Module::default();
        let mut branched = m.func("branched", vec![Val::I32], vec![Val::I32]);
        branched.stack_values = true;
        let value = branched.local(Val::I32);
        // A local is zero at each entry, including after a previous entry
        // left nonzero bytes in this frame.
        branched.get(value);
        branched.get(0);
        branched.op(I::I32Eqz);
        branched.op(I::BrIf(0));
        branched.i32(7);
        branched.op(I::LocalTee(value));
        branched.op(I::Block(BlockType::Result(wasm_encoder::ValType::I32)));
        branched.get(value);
        branched.get(0);
        branched.i32(1);
        branched.op(I::I32Sub);
        branched.br_table(vec![0], 1);
        branched.op(I::End);
        branched.op(I::I32Add);
        branched.op(I::Return);
        m.define(Sym::Inst(Hash128(6401)), branched);
        m.export("branched", Sym::Inst(Hash128(6401)));
        let mut top = m.func("top", vec![], vec![Val::I32]);
        top.global_get(Global::Stack);
        m.define(Sym::Inst(Hash128(6402)), top);
        m.export("top", Sym::Inst(Hash128(6402)));
        let mut numeric = m.func(
            "numeric",
            vec![],
            vec![Val::I64, Val::F64, Val::F32, Val::I32],
        );
        numeric.stack_values = true;
        let locals = [Val::I64, Val::F64, Val::F32, Val::I32].map(|v| numeric.local(v));
        for (l, op) in locals.into_iter().zip([
            I::I64Const(123),
            I::F64Const(1.5.into()),
            I::F32Const(2.25.into()),
            I::I32Const(7),
        ]) {
            numeric.op(op);
            numeric.set(l);
        }
        for l in locals {
            numeric.get(l);
        }
        m.define(Sym::Inst(Hash128(6403)), numeric);
        m.export("numeric", Sym::Inst(Hash128(6403)));
        let bytes = m.finish().expect("link shadow-storage fixture");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)
            .expect("validate shadow storage");
        let mut child = hd_testkit::node::command()
            .args([
                "--input-type=module",
                "-e",
                r"
                import assert from 'node:assert/strict';
                const chunks=[]; for await (const c of process.stdin) chunks.push(c);
                const {instance:{exports:e}}=await WebAssembly.instantiate(Buffer.concat(chunks), {'hd:rt': {panic(){throw Error('unexpected frame overflow')}, stderr(){}}});
                const top=e.top();
                assert.deepEqual(e.numeric(),[123n,1.5,2.25,7]);
                assert.equal(e.top(),top);
                for(const [input,want] of [[1,14],[2,7],[0,0],[1,14],[0,0]]){
                    assert.equal(e.branched(input),want);
                    assert.equal(e.top(),top);
                }
            ",
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("start node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&bytes)
            .expect("write wasm");
        assert!(child.wait().expect("run node").success());
    }
}
