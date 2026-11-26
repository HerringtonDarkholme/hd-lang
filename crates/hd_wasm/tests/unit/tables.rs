mod tests {

    use hd_base::Hash128;
    use hd_wasm::testing::module::Val;
    use hd_wasm::testing::module::{Module, Sym, WasmVal as _};
    use hd_wasm::testing::op::Op as I;
    use std::io::Write as _;
    use std::process::Stdio;
    use wasm_encoder::BlockType;

    #[test]
    fn dense_tables_preserve_scalar_bits_and_the_default_for_any_index() {
        let mut module = Module::default();
        module.optimized = true;
        let kinds = [Val::I32, Val::I64, Val::F32, Val::F64];
        for (id, ty) in kinds.into_iter().enumerate() {
            let name = ["i32", "i64", "f32", "f64"][id];
            let mut f = module.func(name, vec![Val::I32], vec![ty]);
            let constant = |case| match ty {
                Val::I32 => I::I32Const([-7, 44, 9][case]),
                Val::I64 => I::I64Const([i64::MIN, i64::MAX, -3][case]),
                Val::F32 => I::F32Const(wasm_encoder::Ieee32::new(
                    [(-0.0f32).to_bits(), 1.5f32.to_bits(), 0x7fc0_1234][case],
                )),
                Val::F64 => I::F64Const(wasm_encoder::Ieee64::new(
                    [(-0.0f64).to_bits(), 1.5f64.to_bits(), 0x7ff8_0000_0000_1234][case],
                )),
            };
            f.get(0);
            f.op(I::I32Eqz);
            f.op(I::If(BlockType::Result(ty.wasm())));
            f.op(constant(0));
            f.op(I::Else);
            f.get(0);
            f.i32(1);
            f.op(I::I32Eq);
            f.op(I::If(BlockType::Result(ty.wasm())));
            f.op(constant(1));
            f.op(I::Else);
            f.op(constant(2));
            f.op(I::End);
            f.op(I::End);
            let key = Sym::Inst(Hash128(u128::try_from(id + 1).expect("id")));
            module.define(key.clone(), f);
            module.export(name, key);
        }
        let wasm = module.finish().expect("table module");
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&wasm)
            .expect("valid table loads");
        let mut loads = 0;
        for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.expect("payload") {
                for op in body.get_operators_reader().expect("operators") {
                    match op.expect("operator") {
                        wasmparser::Operator::If { .. } => panic!("constant choice kept a branch"),
                        wasmparser::Operator::I32Load { .. }
                        | wasmparser::Operator::I64Load { .. }
                        | wasmparser::Operator::F32Load { .. }
                        | wasmparser::Operator::F64Load { .. } => loads += 1,
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(loads, 4);
        let script = format!(
            r"
import assert from 'node:assert/strict';
const e = (await WebAssembly.instantiate(new Uint8Array({wasm:?}), {{}})).instance.exports;
for (const [input, index] of [[0,0],[1,1],[2,2],[3,2],[0xffffffff,2],[-1,2]]) {{
  assert.equal(e.i32(input), [-7,44,9][index]);
  assert.equal(e.i64(input), [-(1n<<63n),(1n<<63n)-1n,-3n][index]);
  for (const name of ['f32','f64']) {{
    if (index === 0) assert.ok(Object.is(e[name](input), -0));
    else if (index === 1) assert.equal(e[name](input), 1.5);
    else assert.ok(Number.isNaN(e[name](input)));
  }}
}}
"
        );
        let mut child = hd_testkit::node::command()
            .arg("--input-type=module")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("script");
        let result = child.wait_with_output().expect("table program");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
