import assert from "node:assert/strict";
import test from "node:test";

import {
  analyze,
  type Compilation,
  compileToWasm,
  compileToWat,
  instantiate,
} from "../src/compiler.ts";
import { PRELUDE_NAMES, PRELUDE_ORIGINS } from "../src/checker/prelude-names.ts";
import { emitHostFunctionImports } from "../src/emitter/intrinsics.ts";
import type { HirProgram } from "../src/hir.ts";
import { RUNTIME_WAT } from "../src/emitter/runtime/index.ts";
import { type RuntimePanicName, runtimePanicCode } from "../src/runtime-panic.ts";
import { conformance } from "./fixture.ts";

const PROGRAM = conformance("runtime/valid/conditional-call-program");

test("checker creates typed HIR with resolved locals and calls", () => {
  const result = analyze(PROGRAM);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.hir?.functions[0]?.result, "i32");
  assert.equal(result.hir?.functions[1]?.locals[0]?.name, "base");
});

test("compiler emits genuine Wasm GC and executes the entry point", async () => {
  const compilation = await compileToWasm(PROGRAM);
  // The test body needs a GC suspension frame; unrelated runtime types may be absent.
  assert.match(compilation.wat, /\(type \$s\d+ \(struct/);
  assert.match(compilation.wat, /\(struct\.new \$s\d+/);
  assert.ok(WebAssembly.validate(compilation.bytes));
  const { instance } = await instantiate(PROGRAM);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("runtime.wat panic tags match runtime-panic.ts", () => {
  const tags = [
    ...RUNTIME_WAT.matchAll(/\(global \$hd\.panic-([a-z-]+) i32 \(i32\.const (\d+)\)\)/g),
  ];
  assert.ok(tags.length > 0);
  for (const [, name, code] of tags)
    assert.equal(Number(code), runtimePanicCode(name as RuntimePanicName), name);
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

  // A negated exponent is signed, so it is rejected before it can run.
  assert.equal(analyze("fn main() -> i32: 2 ** -1\n").diagnostics[0]?.code, "type-mismatch");
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
    analyze(conformance("typing/invalid/bool-ordering")).diagnostics[0]?.code,
    "missing-partial-ord",
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
  assert.equal(variadic?.parameters[0]?.type, "List[i32]");
});

test("a vararg function value does not fit a fixed-arity function type", () => {
  assert.equal(
    analyze(conformance("typing/invalid/vararg-function-value-arity")).diagnostics[0]?.code,
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
  assert.ok(
    compilation.hir?.functions.some(
      (declaration) =>
        declaration.name === "$enum-shared.Status.Unknown" && declaration.parameters.length === 0,
    ),
  );
});

test("shared enum data is a per-variant constant computed once", async () => {
  const source = [
    "let calls: i32 = 0",
    "",
    "fn bump() -> i32:",
    "    calls = calls + 1",
    "    calls",
    "",
    "enum Box[T](count: i32, seen: i32 = bump(), items: List[i32] = [1]):",
    "    Full(value: T) -> Box(1)",
    "    Empty -> Box(0)",
    "",
    "fn main() -> i32:",
    '    let first: Box[string] = Box.Full("a")',
    '    let second: Box[string] = Box.Full("b")',
    "    let empty: Box[string] = Box.Empty",
    "    let again: Box[string] = Box.Empty",
    "    canonical := if empty is again: 100 else: 0",
    "    shared := if first.items is second.items: 10 else: 0",
    "    canonical + shared + first.seen + second.seen + calls",
    "",
  ].join("\n");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  // `bump` runs once per variant: Full sees 1, and Empty's run makes calls 2.
  assert.equal((instance.exports.main as CallableFunction)(), 100 + 10 + 1 + 1 + 2);
});

test("shared enum data is read-only and cannot use the payload", () => {
  assert.deepEqual(
    analyze(conformance("typing/invalid/enum-shared-field-assignment")).diagnostics.map(
      (diagnostic) => diagnostic.code,
    ),
    ["invalid-assignment-target"],
  );
  assert.deepEqual(
    analyze(conformance("typing/invalid/enum-shared-constructor-payload")).diagnostics.map(
      (diagnostic) => diagnostic.code,
    ),
    ["unknown-name"],
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
    conformance("typing/invalid/prelude-shadow-renamed-use"),
  ];
  for (const source of cases) {
    assert.equal(analyze(source).diagnostics[0]?.code, "prelude-name-shadow", source);
  }
});

test("a use of the prelude's own declaration is allowed", () => {
  assert.deepEqual([...PRELUDE_ORIGINS.keys()].sort(), [...PRELUDE_NAMES].sort());
  assert.deepEqual(analyze(conformance("typing/valid/reimport-prelude-name")).diagnostics, []);
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
  assert.match(compilation.wat, /call \$hd\.vector_push/);
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

test("strings use GC byte arrays and len counts bytes", async () => {
  const source = conformance("runtime/valid/string-length-counts-bytes");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.bytes 8/);
  assert.equal((instance.exports.main as CallableFunction)(), 8);
});

/** A call of the `std.text` string-kernel function `string_<name>` (lib/std/text.hd). */
function stringKernelCall(compilation: Compilation, name: string): RegExp {
  const kernel = compilation.hir.functions.find(
    (declaration) => declaration.name === `__std_text_string_${name}`,
  );
  assert.ok(kernel, `std.text.string_${name} is declared`);
  return new RegExp(`call \\$f${kernel.index}\\b`);
}

test("string interpolation displays built-ins from left to right", async () => {
  const source = conformance("runtime/valid/string-interpolation-built-ins");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  // The primitives' `Display` is std hd code (lib/std/format.hd), a `char`'s
  // UTF-8 encoding too; the segments join through std's `string_concat`.
  assert.doesNotMatch(compilation.wat, /\$hd\.\w+_to_string/);
  assert.doesNotMatch(compilation.wat, /host:string_from_scalar/);
  assert.match(compilation.wat, stringKernelCall(compilation, "concat"));
});

test("strings compare by UTF-8 value order", async () => {
  const source = conformance("runtime/valid/string-ordering");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, stringKernelCall(compilation, "compare"));
  // `starts_with` is std hd code over the byte primitives (lib/std/text.hd).
  assert.match(compilation.wat, /call \$hd\.string_get/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("strings concatenate and unnamed enum fields use underscore selectors", async () => {
  const source = conformance("runtime/valid/string-concatenation-and-numeric-selectors");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, stringKernelCall(compilation, "concat"));
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

test("panic detail import follows the checker-set flag", () => {
  const panicking = compileToWat('fn main() -> void:\n    panic("boom")\n');
  assert.match(panicking.wat, /panic_with_message/);
  const program: HirProgram = {
    data: [],
    enums: [],
    traits: [],
    implementations: [],
    globals: [],
    functions: [],
    closures: [],
    hostCapabilities: [],
    hasPanicDetail: false,
  };
  assert.ok(!emitHostFunctionImports(program).imports.includes("panic_with_message"));
  assert.ok(
    emitHostFunctionImports({ ...program, hasPanicDetail: true }).imports.includes(
      "panic_with_message",
    ),
  );
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
  assert.match(compilation.wat, stringKernelCall(compilation, "equal"));
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

test("optionals inject plain values, match both cases, and propagate .None", async () => {
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
  assert.ok(compilation.hir.enums.some((declaration) => declaration.name === "ResourceError"));
  assert.match(compilation.wat, /type \$e\d+ \(struct/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("Result patterns recursively match imported enum payloads", async () => {
  const source = conformance("runtime/valid/result-pattern-nested-enum");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("cannot-infer-type names the uninferred parameter and the annotation that solves it", () => {
  const message = (source: string): string | undefined =>
    analyze(source).diagnostics.find(({ code }) => code === "cannot-infer-type")?.message;
  assert.equal(
    message("fn main() -> void:\n    a := Result.Ok(123)\n"),
    "cannot infer `E` in `Result[i32, E]`; annotate the binding: `let a: Result[i32, E] = ...`",
  );
  assert.equal(
    message('fn main() -> void:\n    failed := Result.Err("x")\n'),
    "cannot infer `T` in `Result[T, string]`; annotate the binding: `let failed: Result[T, string] = ...`",
  );
  assert.equal(
    message(
      "enum Maybe[T]:\n    Some(value: T)\n    None\n\nfn main() -> void:\n    m := Maybe.None\n",
    ),
    "cannot infer `T` in `Maybe[T]`; annotate the binding: `let m: Maybe[T] = ...`",
  );
  assert.equal(
    message("fn main() -> void:\n    items := [Result.Ok(1)]\n"),
    "cannot infer `E` in `Result[i32, E]`; annotate a binding for it: `let value: Result[i32, E] = ...`",
  );
  assert.equal(
    message('fn make[T]() -> T:\n    panic("no")\n\nfn main() -> void:\n    made := make()\n'),
    "cannot infer `T` in the call to `make`; annotate the binding, or write the type arguments: `make::[...]`",
  );
});

test("optional and Result context errors have stable diagnostics", () => {
  assert.equal(
    analyze(conformance("typing/invalid/none-without-expected-type")).diagnostics[0]?.code,
    "missing-contextual-enum-type",
  );
  assert.equal(
    analyze(conformance("typing/invalid/result-constructor-without-context")).diagnostics[0]?.code,
    "cannot-infer-type",
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

test("closures mutate their captures without a mut fn marker", async () => {
  const source = [
    "fn main() -> i32:",
    "    let count: i32 = 0",
    "    let items: mut List[i32] = []",
    "    step := fn() -> i32:",
    "        count = count + 1",
    "        items.push(count)",
    "        count",
    "    _ := step()",
    "    step() * 10 + i32(items.len())",
    "",
  ].join("\n");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 22);
  assert.deepEqual(
    analyze(conformance("parse/invalid/mut-closure-literal")).diagnostics.map(
      (diagnostic) => diagnostic.code,
    ),
    ["syntax-error"],
  );
});

test("a direct is on a function value is unsupported-function-identity", () => {
  for (const name of ["closure-identity", "named-function-identity"])
    assert.deepEqual(
      analyze(conformance(`typing/invalid/${name}`)).diagnostics.map(
        (diagnostic) => diagnostic.code,
      ),
      ["unsupported-function-identity"],
    );
  assert.deepEqual(analyze(conformance("typing/valid/generic-function-identity")).diagnostics, []);
});

test("function types convert by declared variance and share one closure layout", async () => {
  const source = [
    "data User:",
    "    name: string",
    "",
    'fn fresh() -> mut User: User { name: "Ada" }',
    "fn name_length(user: User) -> i32: i32(user.name.len())",
    "",
    "fn widen(callback: fn(User) -> i32) -> fn(mut User) -> i32:",
    "    callback",
    "",
    "fn makers(list: List[fn() -> mut User]) -> List[fn() -> User]:",
    "    list",
    "",
    "fn main() -> i32:",
    '    let user: mut User = User { name: "Grace" }',
    "    maker := makers([fresh])[0]",
    "    reader := widen(name_length)",
    "    reader(user) * 10 + i32(maker().name.len())",
    "",
  ].join("\n");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 53);
  assert.deepEqual(
    analyze(conformance("typing/invalid/function-result-representation-change")).diagnostics.map(
      (diagnostic) => diagnostic.code,
    ),
    ["variance-representation-change"],
  );
});

test("function types are implementation targets owned by the standard library", async () => {
  const source = [
    "trait Describe:",
    "    fn describe(self) -> i32",
    "",
    "impl Describe for fn(i32) -> i32:",
    "    fn describe(self) -> i32: 7",
    "",
    "fn inc(value: i32) -> i32: value + 1",
    "",
    "fn main() -> i32:",
    "    callback := inc",
    "    callback.describe()",
    "",
  ].join("\n");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 7);
  assert.deepEqual(
    analyze(conformance("typing/invalid/function-type-orphan-impl")).diagnostics.map(
      (diagnostic) => diagnostic.code,
    ),
    ["orphan-impl"],
  );
  assert.deepEqual(
    analyze(
      ["trait Marker", "impl Marker for fn(i32) -> i32", "impl Marker for fn(i32) -> i32", ""].join(
        "\n",
      ),
    ).diagnostics.map((diagnostic) => diagnostic.code),
    ["overlapping-impl"],
  );
});

test("spelled std.function constructors are the function type sugar", async () => {
  const source = [
    "use std.function.Fn",
    "",
    "fn count(label: string, values...: List[i32]) -> usize: values.len()",
    "fn inc(value: i32) -> i32: value + 1",
    "",
    "fn keep(callback: Fn[(i32,), i32, $()]) -> fn(i32) -> i32:",
    "    callback",
    "",
    "fn main() -> i32:",
    "    kept := keep(inc)",
    '    kept(1) * 10 + i32(count("n", 1, 2, 3))',
    "",
  ].join("\n");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 23);
  for (const [name, code] of [
    ["typing/invalid/function-type-non-tuple-inputs", "generic-kind-mismatch"],
    ["parse/invalid/vararg-ellipsis-after-type", "syntax-error"],
  ])
    assert.deepEqual(
      analyze(conformance(name)).diagnostics.map((diagnostic) => diagnostic.code),
      [code],
    );
});

test("generic function values passed as arguments infer their type arguments", async () => {
  const source = [
    "enum Box[T]:",
    "    Full(value: T)",
    "    Empty",
    "",
    "fn identity[T](value: T) -> T: value",
    "fn apply[A, B](value: A, f: fn(A) -> B) -> B: f(value)",
    "fn unbox(value: Box[i32]) -> i32:",
    "    match value:",
    "        Box.Full(inner) => inner",
    "        Box.Empty => 0",
    "",
    "fn main() -> i32:",
    "    apply(4, identity) * 10 + unbox(apply(2, Box.Full))",
    "",
  ].join("\n");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  for (const name of [
    "generic-function-value-argument-unsolved",
    "generic-variant-constructor-argument-unsolved",
  ])
    assert.deepEqual(
      analyze(conformance(`typing/invalid/${name}`)).diagnostics.map(
        (diagnostic) => diagnostic.code,
      ),
      ["cannot-infer-type"],
    );
});

test("a function result that is a function type with a row keeps that row", async () => {
  const program = `trait Db:
    fn name(self) -> string

data MemoryDb: pass

impl Db for MemoryDb:
    fn name(self) -> string: "db"

fn orders() -> string $ Db:
    $.use(Db).name()

fn pick(flag: bool) -> (fn() -> string $ Db):
    orders

fn run() -> string $ Db:
    handler := pick(true)
    handler()

fn main() -> i32:
    $.with(Db=MemoryDb {}):
        if run() == "db": 1 else: 0
`;
  const { instance } = await instantiate(program);
  assert.equal((instance.exports.main as CallableFunction)(), 1);
});

test("a trait default method instantiates the trait's type parameters", async () => {
  const program = `trait Source[T]:
    fn next(mut self) -> T?
    fn second(mut self) -> T?:
        let first: T? = self.next()
        self.next()

data Counter:
    n: i32

impl Source[i32] for Counter:
    fn next(mut self) -> i32?:
        self.n = self.n + 1
        .Some(self.n)

fn main() -> i32:
    let counter: mut Counter = Counter { n: 0 }
    match counter.second():
        .Some(value) => value
        .None => 0
`;
  const { instance } = await instantiate(program);
  assert.equal((instance.exports.main as CallableFunction)(), 2);
});
