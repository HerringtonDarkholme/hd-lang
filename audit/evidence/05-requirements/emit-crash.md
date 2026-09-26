<!-- commit bd985d7; command: hd check / hd build / hd test on the two files below; date: 2026-09-25T20:48:52Z -->

## test/fixtures/frontend/30-parser-lowers-multi-provider-use-to-an-ordered-tuple.hd

```text
$ hd check test/fixtures/frontend/30-parser-lowers-multi-provider-use-to-an-ordered-tuple.hd
test/fixtures/frontend/30-parser-lowers-multi-provider-use-to-an-ordered-tuple.hd: ok
exit=0
$ hd test test/fixtures/frontend/30-parser-lowers-multi-provider-use-to-an-ordered-tuple.hd
[wasm-validator error in function f0] array.new_fixed value must have proper type, on 
(array.new_fixed $hd.list 2
 (local.get $0)
 (local.get $1)
)
[wasm-validator error in function f0] array.new_fixed value must have proper type, on 
(array.new_fixed $hd.list 2
 (local.get $0)
 (local.get $1)
)
file:///Users/hd/code/test/hd-lang/src/wasm.ts:29
      throw new WasmValidationError("Binaryen rejected generated Wasm");
            ^

```

## spec/conformance/typing/valid/resource-disposed-result.hd

```text
$ hd check spec/conformance/typing/valid/resource-disposed-result.hd
spec/conformance/typing/valid/resource-disposed-result.hd: ok
exit=0
$ hd test spec/conformance/typing/valid/resource-disposed-result.hd
file:///Users/hd/code/test/hd-lang/src/wasm.ts:23
    throw new WasmValidationError(`Binaryen could not parse generated WAT: ${String(error)}`);
          ^

WasmValidationError: Binaryen could not parse generated WAT: [object Object]

Node.js v24.19.0
```
