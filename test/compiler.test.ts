import assert from "node:assert/strict";
import test from "node:test";

import { analyze, compile, instantiate } from "../src/compiler.ts";
import { conformance } from "./fixture.ts";

const PROGRAM = conformance("runtime/valid/conditional-call-program");

test("checker creates typed HIR with resolved locals and calls", () => {
  const result = analyze(PROGRAM);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.hir?.functions[0]?.result, "i32");
  assert.equal(result.hir?.functions[1]?.locals[0]?.name, "base");
});

test("compiler emits genuine Wasm GC and executes the entry point", async () => {
  const compilation = compile(PROGRAM);
  assert.ok(compilation.wat.includes("type $hd.runtime (struct"));
  assert.ok(WebAssembly.validate(compilation.bytes));
  const { instance } = await instantiate(PROGRAM);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("checked i32 arithmetic traps on overflow", async () => {
  const { instance } = await instantiate(conformance("runtime/panic/i32-add-overflow-in-function"));
  assert.throws(() => (instance.exports.main as CallableFunction)());
});

test("the minimum i32 literal forms through unary negation", async () => {
  const { instance } = await instantiate(conformance("runtime/valid/i32-minimum-literal"));
  assert.equal((instance.exports.main as CallableFunction)(), -2_147_483_648);
  assert.equal(
    analyze(conformance("typing/invalid/unary-plus-i32-literal-range")).diagnostics[0]?.code,
    "integer-literal-range",
  );
  const division = await instantiate(conformance("runtime/panic/i32-min-divided-by-minus-one"));
  assert.throws(() => (division.instance.exports.main as CallableFunction)());
});

test("integer power is right-associative, checked, and rejects negative exponents", async () => {
  const { instance, compilation } = await instantiate(
    conformance("runtime/valid/integer-power-associativity"),
  );
  assert.match(compilation.wat, /call \$hd\.pow_i32/);
  assert.equal((instance.exports.main as CallableFunction)(), 508);

  // Current MVP behavior; spec L2 makes a signed exponent a type error (known failure).
  const negative = await instantiate("fn main() -> i32: 2 ** -1\n");
  assert.throws(() => (negative.instance.exports.main as CallableFunction)());
  const overflow = await instantiate(conformance("runtime/panic/integer-power-overflow"));
  assert.throws(() => (overflow.instance.exports.main as CallableFunction)());
});

test("floating power uses the host IEEE pow primitive", async () => {
  const { instance, compilation } = await instantiate(conformance("runtime/valid/floating-power"));
  assert.match(compilation.wat, /import "hd" "pow_f64"/);
  assert.equal((instance.exports.main as CallableFunction)(), 3);
  assert.equal(
    analyze(conformance("typing/invalid/power-mixed-numeric-types")).diagnostics[0]?.code,
    "mixed-numeric-types",
  );
});

test("checker rejects name, mutability, and type errors", () => {
  assert.equal(
    analyze(conformance("typing/invalid/unknown-value-name")).diagnostics[0]?.code,
    "unknown-name",
  );
  assert.equal(
    analyze(conformance("typing/invalid/reassign-short-binding-in-function")).diagnostics[0]?.code,
    "non-reassignable-binding",
  );
  assert.equal(
    analyze(conformance("typing/invalid/function-result-type-mismatch")).diagnostics[0]?.code,
    "type-mismatch",
  );
  assert.equal(
    analyze(conformance("typing/invalid/float-remainder")).diagnostics[0]?.code,
    "invalid-binary-operands",
  );
  assert.equal(
    analyze(conformance("typing/invalid/bool-ordering")).diagnostics[0]?.code,
    "invalid-binary-operands",
  );
  assert.equal(
    analyze(conformance("typing/invalid/nonfinal-vararg-then-parameter")).diagnostics[0]?.code,
    "nonfinal-vararg",
  );
  assert.equal(
    analyze(conformance("typing/invalid/positional-spread-without-vararg")).diagnostics[0]?.code,
    "positional-spread-needs-vararg",
  );
});

test("homogeneous varargs lower through the existing list ABI", async () => {
  const source = conformance("runtime/valid/homogeneous-varargs");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 34);
  const variadic = compilation.hir?.functions.find((fn) => fn.name === "count");
  assert.equal(variadic?.parameters[0]?.type, "list[i32]");
});

test("first-class vararg functions retain their calling convention", async () => {
  const source = conformance("runtime/valid/vararg-function-values");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 23);
  assert.equal(
    analyze(conformance("typing/invalid/list-function-to-vararg-function-type")).diagnostics[0]
      ?.code,
    "type-mismatch",
  );
});

test("function parameter defaults evaluate after explicit arguments", async () => {
  const source = conformance("runtime/valid/parameter-defaults-after-explicit-arguments");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 1438);
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$parameter-default.combine.second",
    ),
  );
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$parameter-default.choose.fallback",
    ),
  );
});

test("function parameter defaults enforce order, type, and requirement-freedom", () => {
  assert.equal(
    analyze(conformance("typing/invalid/parameter-default-order")).diagnostics[0]?.code,
    "default-order",
  );
  assert.equal(
    analyze(conformance("typing/invalid/parameter-default-type-mismatch")).diagnostics[0]?.code,
    "type-mismatch",
  );
  assert.equal(
    analyze(conformance("typing/invalid/suspending-parameter-default")).diagnostics[0]?.code,
    "suspension-forbidden-context",
  );
});

test("varargs work in suspending and trait method calls", async () => {
  const source = conformance("runtime/valid/varargs-in-trait-and-suspending-methods");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 108);
});

test("named arguments map to parameters without changing source evaluation order", async () => {
  const ordinary = await instantiate(conformance("runtime/valid/named-arguments-reordered"));
  assert.equal((ordinary.instance.exports.main as CallableFunction)(), 42);

  const source = conformance("runtime/valid/named-arguments-source-evaluation-order");
  const polls: number[] = [];
  const suspended = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 3) polls.push(functionIndex);
    },
  });
  assert.equal((suspended.instance.exports.main as CallableFunction)(), 21);
  assert.deepEqual(polls, [0, 1]);
});

test("named enum payloads preserve source evaluation order", async () => {
  const source = conformance("runtime/valid/named-enum-payload-evaluation-order");
  const polls: number[] = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 2) polls.push(functionIndex);
    },
  });
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 21);
  assert.deepEqual(polls, [0, 1]);

  assert.equal(
    analyze(conformance("typing/invalid/variant-unknown-payload-field")).diagnostics[0]?.code,
    "unknown-data-field",
  );
  assert.equal(
    analyze(conformance("typing/invalid/variant-duplicate-argument")).diagnostics[0]?.code,
    "duplicate-argument",
  );
});

test("named enum payload patterns resolve bindings by field name", async () => {
  const source = conformance("runtime/valid/named-enum-payload-patterns");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/variant-pattern-unknown-field")).diagnostics[0]?.code,
    "unknown-data-field",
  );
});

test("literal enum payload patterns constrain variants in Wasm and suspension CFG", async () => {
  const source = conformance("runtime/valid/literal-payload-patterns");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 138);
  assert.equal(
    analyze(conformance("typing/invalid/literal-payload-pattern-nonexhaustive")).diagnostics[0]
      ?.code,
    "nonexhaustive-match",
  );
});

test("shared enum data uses per-variant factories and pure ordered defaults", async () => {
  const source = conformance("runtime/valid/shared-enum-data-defaults");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 84);
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$enum-variant.Status.Unknown",
    ),
  );
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) => declaration.name === "$enum-default.Status.doubled",
    ),
  );
});

test("named arguments work through static and dynamic trait dispatch", async () => {
  const source = conformance("runtime/valid/named-arguments-trait-dispatch");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("prelude names cannot be shadowed by declarations or bindings", () => {
  const cases = [
    conformance("typing/invalid/prelude-shadow"),
    conformance("typing/invalid/prelude-shadow-println-function"),
    conformance("typing/invalid/prelude-shadow-result-generic"),
    conformance("typing/invalid/prelude-shadow-console-parameter"),
    conformance("typing/invalid/prelude-shadow-hash-local"),
  ];
  for (const source of cases) {
    assert.equal(analyze(source).diagnostics[0]?.code, "prelude-name-shadow", source);
  }
});

test("branch scopes do not leak and may shadow each other", () => {
  const source = conformance("runtime/valid/branch-scopes-shadow");
  assert.deepEqual(analyze(source).diagnostics, []);
  assert.equal(
    analyze(conformance("typing/invalid/branch-binding-does-not-leak")).diagnostics[0]?.code,
    "unknown-name",
  );
});

test("else if chains preserve value typing and selection order", async () => {
  const source = conformance("runtime/valid/else-if-chain");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("while, break, and continue lower to structured Wasm control flow", async () => {
  const source = conformance("runtime/valid/while-break-and-continue");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(loop \$loop/);
  assert.equal((instance.exports.main as CallableFunction)(), 7);
  assert.equal(
    analyze(conformance("typing/invalid/break-outside-loop")).diagnostics[0]?.code,
    "break-outside-loop",
  );
});

test("while else produces values on break or normal exhaustion", async () => {
  const broken = conformance("runtime/valid/while-else-break-value");
  const brokenResult = await instantiate(broken);
  assert.equal((brokenResult.instance.exports.main as CallableFunction)(), 42);

  const exhausted = conformance("runtime/valid/while-else-exhaustion-value");
  const exhaustedResult = await instantiate(exhausted);
  assert.equal((exhaustedResult.instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/break-value-in-void-loop")).diagnostics[0]?.code,
    "break-value-context",
  );
  assert.equal(
    analyze(conformance("typing/invalid/plain-break-in-value-loop")).diagnostics[0]?.code,
    "break-value-context",
  );
});

test("for loops iterate lists and maps with continue, destructuring, and else values", async () => {
  const source = conformance("runtime/valid/for-loops-lists-and-maps");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/for-over-non-iterable")).diagnostics[0]?.code,
    "unsatisfied-trait-bound",
  );
  assert.equal(
    analyze(conformance("typing/invalid/for-binding-arity")).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("data declarations lower to Wasm GC structs", async () => {
  const source = conformance("runtime/valid/data-fields-named-in-any-order");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$d0 \(struct/);
  assert.match(compilation.wat, /\(struct\.new \$d0/);
  assert.match(compilation.wat, /\(struct\.get \$d0 \$d0f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable data paths weaken one way and share Wasm GC identity", async () => {
  const source = conformance("runtime/valid/mutable-data-paths-share-identity");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /struct\.set \$d0/);
  assert.match(compilation.wat, /field \$d1f0 \(mut/);
});

test("mutable list and map roots support indexed replacement", async () => {
  const source = conformance("runtime/valid/indexed-replacement-list-and-map");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /array\.set \$hd\.list/);
  assert.match(compilation.wat, /call \$hd\.map_insert/);
});

test("mutable lists append through growable Wasm GC storage", async () => {
  const source = conformance("runtime/valid/list-append-grows");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /type \$hd\.vector \(struct/);
  assert.match(compilation.wat, /call \$hd\.vector_append/);
  assert.ok(
    analyze(conformance("typing/invalid/readonly-list-append")).diagnostics.some(
      (diagnostic) => diagnostic.code === "mutable-receiver-required",
    ),
  );
});

test("mutable maps grow from empty storage and remove entries in insertion order", async () => {
  const source = conformance("runtime/valid/map-grow-and-remove");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /call \$hd\.map_remove/);
  assert.match(compilation.wat, /struct\.set \$hd\.map \$hd\.map-keys/);

  assert.equal(
    analyze(conformance("typing/invalid/readonly-map-remove")).diagnostics[0]?.code,
    "mutable-receiver-required",
  );
});

test("data field initializers preserve source evaluation order", async () => {
  const source = conformance("runtime/valid/data-field-evaluation-order");
  const polls: number[] = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 2) polls.push(functionIndex);
    },
  });
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 21);
  assert.deepEqual(polls, [0, 1]);
});

test("data field defaults run per construction after explicit fields", async () => {
  const source = conformance("runtime/valid/data-field-defaults");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 12);

  const main = compilation.hir?.functions.find((declaration) => declaration.name === "main");
  const firstBinding = main?.body[0];
  assert.equal(firstBinding?.kind, "binding");
  if (firstBinding?.kind === "binding") {
    assert.equal(firstBinding.value.kind, "data");
    if (firstBinding.value.kind === "data")
      assert.deepEqual(firstBinding.value.fieldIndices, [0, 1, 2]);
  }
});

test("data field defaults are type checked and requirement-free", () => {
  assert.equal(
    analyze(conformance("typing/valid/generic-optional-field-default")).diagnostics.length,
    0,
  );
  assert.equal(
    analyze(conformance("typing/invalid/suspending-data-field-default")).diagnostics[0]?.code,
    "suspension-forbidden-context",
  );
  assert.equal(
    analyze(conformance("typing/invalid/data-field-default-type-mismatch")).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("data copy-update evaluates its source before replacements", async () => {
  const source = conformance("runtime/valid/copy-update-evaluation-order");
  const polls: number[] = [];
  const { instance, compilation } = await instantiate(source, {
    trace: (functionIndex, event) => {
      if (event === 1 && functionIndex !== 2) polls.push(functionIndex);
    },
  });
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.deepEqual(polls, [0, 1]);
});

test("data copy-update supplies omitted fields without running defaults", async () => {
  const source = conformance("runtime/valid/copy-update-skips-defaults");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/copy-update-source-type-mismatch")).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("fieldless data lowers to an empty Wasm GC struct", async () => {
  const source = conformance("runtime/valid/fieldless-data-argument");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$d0 \(struct\s*\)\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("data initialization checks field names, presence, and types", () => {
  assert.equal(
    analyze(conformance("typing/invalid/data-literal-unknown-field")).diagnostics[0]?.code,
    "unknown-data-field",
  );
  assert.equal(
    analyze(conformance("typing/invalid/data-literal-missing-field")).diagnostics[0]?.code,
    "missing-required-field",
  );
  assert.equal(
    analyze(conformance("typing/invalid/data-literal-field-type-mismatch")).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("data patterns destructure nested fields and test literals", async () => {
  const source = conformance("runtime/valid/data-patterns");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /struct\.get \$d0 \$d0f0 \(struct\.get \$d1 \$d1f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(
    analyze(conformance("typing/invalid/data-pattern-repeated-field")).diagnostics[0]?.code,
    "duplicate-data-pattern-field",
  );
  assert.equal(
    analyze(conformance("typing/invalid/data-pattern-unknown-field")).diagnostics[0]?.code,
    "unknown-data-field",
  );
  assert.equal(
    analyze(conformance("typing/invalid/data-pattern-nonexhaustive")).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
});

test("strings use GC byte arrays and len counts Unicode scalars", async () => {
  const source = conformance("runtime/valid/string-length-counts-scalars");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.bytes 8/);
  assert.equal((instance.exports.main as CallableFunction)(), 3);
});

test("string interpolation displays built-ins from left to right", async () => {
  const source = conformance("runtime/valid/string-interpolation-built-ins");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 29);
  assert.match(compilation.wat, /call \$hd\.i32_to_string/);
  assert.match(compilation.wat, /call \$hd\.char_to_string/);
});

test("strings compare by UTF-8 value order", async () => {
  const source = conformance("runtime/valid/string-ordering");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /call \$hd\.string_compare/);
  assert.match(compilation.wat, /call \$hd\.string_starts_with/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("strings concatenate and unnamed enum fields use numeric selectors", async () => {
  const source = conformance("runtime/valid/string-concatenation-and-numeric-selectors");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /call \$hd\.string_concat/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("character literals carry Unicode scalar values and compare in scalar order", async () => {
  const source = conformance("runtime/valid/character-literals");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 0x1f600);
});

test("defer runs after a return value is evaluated", async () => {
  const source = conformance("runtime/valid/defer-after-return-value");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /local\.set \$tmp0/);
  assert.equal((instance.exports.main as CallableFunction)(), 1);
});

test("defer rejects escaping control flow", () => {
  const source = conformance("typing/invalid/defer-return");
  assert.equal(analyze(source).diagnostics[0]?.code, "defer-control-flow");
});

test("defer is LIFO and runs on loop continue and break", async () => {
  const source = conformance("runtime/valid/defer-lifo-and-loop-exits");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 1221);
});

test("explicit panic lowers to unreachable and skips pending defer", async () => {
  const source = conformance("runtime/panic/explicit-panic-skips-defer");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /unreachable/);
  assert.throws(() => (instance.exports.main as CallableFunction)());
});

test("enums use tagged GC structs and match binds payloads", async () => {
  const source = conformance("runtime/valid/enum-payload-match");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$e0 \(struct/);
  assert.match(compilation.wat, /\(struct\.new \$e0 \(i32\.const 0\)/);
  assert.match(compilation.wat, /\(struct\.get \$e0 \$e0tag/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("match checking enforces coverage, payload arity, and arm reachability", () => {
  assert.equal(
    analyze(conformance("typing/invalid/enum-match-missing-variant")).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(conformance("typing/invalid/variant-pattern-missing-payload")).diagnostics[0]?.code,
    "pattern-arity",
  );
  assert.equal(
    analyze(conformance("typing/invalid/match-arm-after-catch-all")).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
  assert.equal(
    analyze(conformance("typing/invalid/bare-variant-pattern-in-match")).diagnostics[0]?.code,
    "bare-variant-pattern",
  );
});

test("contextual enum variant patterns use the subject type", async () => {
  const source = conformance("runtime/valid/contextual-variant-patterns");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const swapped = analyze(conformance("typing/warnings/variant-binding-names-swapped"));
  assert.ok(swapped.hir);
  assert.ok(
    swapped.diagnostics.some(
      (diagnostic) =>
        diagnostic.code === "variant-binding-name-mismatch" && diagnostic.severity === "warning",
    ),
  );
});

test("contextual enum variant expressions use their expected type", async () => {
  const source = conformance("runtime/valid/contextual-variant-expressions");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/contextual-variant-binding-without-type")).diagnostics[0]
      ?.code,
    "missing-contextual-enum-type",
  );
});

test("boolean matches are exhaustive and lower to scalar tests", async () => {
  const source = conformance("runtime/valid/bool-match");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /i32\.eq[\s\S]*local\.get \$tmp/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/bool-match-missing-false")).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(conformance("typing/invalid/duplicate-bool-match-arm")).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
});

test("numeric, character, and string literal patterns require a catch-all", async () => {
  const source = conformance("runtime/valid/literal-patterns");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /call \$hd\.string_compare/);
  assert.match(compilation.wat, /f64\.eq/);
  assert.equal((instance.exports.main as CallableFunction)(), 32);
  assert.equal(
    analyze(conformance("typing/invalid/integer-literal-match-without-catch-all")).diagnostics[0]
      ?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(conformance("typing/invalid/duplicate-literal-match-arm")).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
});

test("match guards see pattern bindings and do not contribute coverage", async () => {
  const source = conformance("runtime/valid/match-guards");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/guarded-catch-all-not-exhaustive")).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(conformance("typing/valid/match-guard-reads-binding")).diagnostics.length,
    0,
  );
  assert.equal(
    analyze(conformance("typing/invalid/match-guard-type-mismatch")).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("optionals inject plain values, match both cases, and propagate nil", async () => {
  const source = conformance("runtime/valid/optional-propagation");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$hd\.variant/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.equal((instance.exports.main as CallableFunction)(), 41);
});

test("Result constructors, matching, and error propagation use the erased carrier", async () => {
  const source = conformance("runtime/valid/result-propagation");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 35);
});

test("imported ResourceError is a generic Wasm GC enum", async () => {
  const source = conformance("runtime/valid/resource-error-operation-payload");
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.enums.at(-1)?.name, "ResourceError");
  assert.match(compilation.wat, /type \$e\d+ \(struct/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("Result patterns recursively match imported enum payloads", async () => {
  const source = conformance("runtime/valid/result-pattern-nested-enum");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("optional and Result context errors have stable diagnostics", () => {
  assert.equal(
    analyze(conformance("typing/invalid/nil-without-expected-type")).diagnostics[0]?.code,
    "nil-needs-optional-type",
  );
  assert.equal(
    analyze(conformance("typing/invalid/result-constructor-without-context")).diagnostics[0]?.code,
    "unresolved-generic-placeholder",
  );
  assert.equal(
    analyze(conformance("typing/invalid/propagation-operand-not-optional")).diagnostics[0]?.code,
    "invalid-result-propagation",
  );
  const nested = analyze(conformance("typing/warnings/unused-nested-optional-binding"));
  assert.ok(nested.hir);
  assert.equal(nested.diagnostics[0]?.code, "unused-local-binding");
});

test("optional and Result values are must-use unless explicitly discarded", () => {
  assert.equal(
    analyze(conformance("typing/invalid/discarded-optional-result")).diagnostics[0]?.code,
    "discarded-must-use-value",
  );
  assert.deepEqual(
    analyze(conformance("typing/valid/optional-result-explicitly-discarded")).diagnostics,
    [],
  );
});

test("typed noncapturing closures lower to Wasm typed function references", async () => {
  const source = conformance("runtime/valid/closure-as-function-argument");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$sig0 \(func/);
  assert.match(compilation.wat, /call_ref \$sig0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/closure-argument-type-mismatch")).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("expected function types infer inline closure parameters and results", async () => {
  const source = conformance("runtime/valid/closure-parameter-inference");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/closure-parameter-without-type")).diagnostics[0]?.code,
    "closure-parameter-needs-annotation",
  );
});

test("nonrecursive closures infer result types from fallthrough and returns", async () => {
  const source = conformance("runtime/valid/closure-result-inference");
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.equal(analysis.hir?.functions[0]?.locals[0]?.type, "fn(i32)->i32");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(conformance("typing/invalid/closure-returns-without-common-type")).diagnostics[0]?.code,
    "no-common-type",
  );
});

test("explicitly typed local closures recurse through their current environment", async () => {
  const source = conformance("runtime/valid/recursive-local-closure");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /ref\.func \$c0\) \(local\.get \$env\)/);
  assert.equal(compilation.hir.closures[0]?.captures.length, 0);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("named local functions capture enclosing values and recurse", async () => {
  const source = conformance("runtime/valid/named-local-functions");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});
