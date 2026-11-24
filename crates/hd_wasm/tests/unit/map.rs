mod seed_tests {
    use hd_base::Hash128;
    use hd_wasm::testing::map::*;
    use hd_wasm::testing::op::Op as I;

    use hd_wasm::testing::module::{HEADER, HEAP, Module, Sym, Val, mem};
    use hd_wasm::testing::rt::Helper;

    #[test]
    fn map_headers_keep_seeds_across_copies_growth_and_replay()
    -> Result<(), Box<dyn std::error::Error>> {
        // Reuse the assembler harness's bounded bump allocator and
        // non-freeing count stubs; this oracle inspects real map helpers'
        // header/seed behavior, without testing the allocator again.
        let mut m = Module::default();
        let entry = Entry {
            size: 0,
            leaves: vec![],
            keys: 0,
            key_counted: hd_wasm::testing::rt::Counted::None,
            val_counted: hd_wasm::testing::rt::Counted::None,
            cyclic: false,
        };
        let mut make = m.func("test:map", vec![Val::I32], vec![Val::I32]);
        make.get(0);
        make.helper(Helper::MapSeed);
        make.helper(Helper::MapTable {
            entry: entry.clone(),
        });
        let constructor = Sym::Inst(Hash128(1));
        m.define(constructor.clone(), make);
        m.export("make", constructor);
        let mut get = m.func("test:seed", vec![Val::I32], vec![Val::I64]);
        get.get(0);
        get.op(I::I64Load(mem(HEAP, SEED, 3)));
        let key = Sym::Inst(Hash128(2));
        m.define(key.clone(), get);
        m.export("seed", key);
        let mut copy = m.func("test:copy", vec![Val::I32], vec![Val::I32]);
        copy.get(0);
        copy.u32(HEADER);
        copy.op(I::I32Sub);
        copy.i32(2);
        copy.op(I::I32Store(mem(HEAP, 0, 2)));
        copy.get(0);
        copy.helper(Helper::MapUnique {
            entry: entry.clone(),
        });
        let key = Sym::Inst(Hash128(3));
        m.define(key.clone(), copy);
        m.export("copy", key);
        m.export("grow", Sym::Helper(Helper::MapGrow { entry }));
        m.export("next", Sym::Helper(Helper::MapSeed));
        m.export("reset", Sym::Helper(Helper::EntryReset));
        let bytes = m.finish().map_err(|e| format!("{e:?}"))?;
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&bytes)?;
        let script = format!(
            r"
import assert from 'node:assert/strict';
const module = new WebAssembly.Module(new Uint8Array({bytes:?}));
function run(seed, maintain = true) {{
  let calls = 0;
  const imports = {{}};
  for (const item of WebAssembly.Module.imports(module)) {{
    (imports[item.module] ??= {{}})[item.name] = item.name === 'hash_seed'
      ? () => {{ calls++; return seed; }}
      : () => {{ throw Error('unexpected host import ' + item.name); }};
  }}
  const map = new WebAssembly.Instance(module, imports).exports;
  const first = map.make(0), second = map.make(0);
  const a = map.seed(first), b = map.seed(second);
  assert.notEqual(a, b);
  if (maintain) {{
    const copy = map.copy(first);
    assert.notEqual(copy, first);
    assert.equal(map.seed(copy), a);
    const grown = map.grow(copy);
    assert.equal(map.seed(grown), a);
    // Existing tables and the per-instance sequence survive an entry reset.
    map.reset();
  }}
  const c = map.next();
  const values = [a, b, c];
  for (let i = 0; i < 61; i++) values.push(map.next());
  assert.equal(new Set(values).size, values.length);
  assert.equal(calls, 1, 'zero is a valid run seed, not an initialization sentinel');
  return values;
}}
assert.deepEqual(run(0n), run(0n));
assert.deepEqual(run(0n), run(0n, false), 'copy/grow/reset must consume no seed');
assert.deepEqual(run(19n), run(19n));
assert.notDeepEqual(run(0n), run(19n));
console.log('ok');
"
        );
        // cost: one Node process runs a fixed Wasm module with two fixed run seeds;
        // its replay and lifecycle checks use a constant number of seed requests.
        let output = hd_testkit::node::command()
            .args(["--input-type=module", "-e", &script])
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout)?.trim(), "ok");
        Ok(())
    }
}
