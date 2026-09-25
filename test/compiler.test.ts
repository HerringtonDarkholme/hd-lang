import assert from "node:assert/strict";
import test from "node:test";

import { analyze, compile, instantiate } from "../src/compiler.ts";
import { fixture } from "./fixture.ts";

const PROGRAM = fixture("compiler/00-case-program");

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
  const { instance } = await instantiate(
    fixture("compiler/03-checked-i32-arithmetic-traps-on-overflow"),
  );
  assert.throws(() => (instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
});

test("the minimum i32 literal forms through unary negation", async () => {
  const { instance } = await instantiate(
    fixture("compiler/04-the-minimum-i32-literal-forms-through-unary-negation"),
  );
  assert.equal((instance.exports.main as CallableFunction)(), -2_147_483_648);
  assert.equal(
    analyze(fixture("compiler/04-the-minimum-i32-literal-forms-through-unary-negation-diagnostic"))
      .diagnostics[0]?.code,
    "integer-literal-range",
  );
  const division = await instantiate(
    fixture("compiler/04-the-minimum-i32-literal-forms-through-unary-negation-division"),
  );
  assert.throws(
    () => (division.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
});

test("integer power is right-associative, checked, and rejects negative exponents", async () => {
  const { instance, compilation } = await instantiate(
    fixture("compiler/05-integer-power-is-right-associative-checked-and-rejects-negative-exponent"),
  );
  assert.match(compilation.wat, /call \$hd\.pow_i32/);
  assert.equal((instance.exports.main as CallableFunction)(), 508);

  const negative = await instantiate(
    fixture(
      "compiler/05-integer-power-is-right-associative-checked-and-rejects-negative-exponent-negative",
    ),
  );
  assert.throws(
    () => (negative.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
  const overflow = await instantiate(
    fixture(
      "compiler/05-integer-power-is-right-associative-checked-and-rejects-negative-exponent-overflow",
    ),
  );
  assert.throws(
    () => (overflow.instance.exports.main as CallableFunction)(),
    WebAssembly.RuntimeError,
  );
});

test("floating power uses the host IEEE pow primitive", async () => {
  const { instance, compilation } = await instantiate(
    fixture("compiler/06-floating-power-uses-the-host-ieee-pow-primitive"),
  );
  assert.match(compilation.wat, /import "hd" "pow_f64"/);
  assert.equal((instance.exports.main as CallableFunction)(), 3);
  assert.equal(
    analyze(fixture("compiler/06-floating-power-uses-the-host-ieee-pow-primitive-diagnostic"))
      .diagnostics[0]?.code,
    "mixed-numeric-types",
  );
});

test("checker rejects name, mutability, and type errors", () => {
  assert.equal(
    analyze(fixture("compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic"))
      .diagnostics[0]?.code,
    "unknown-name",
  );
  assert.equal(
    analyze(fixture("compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic-2"))
      .diagnostics[0]?.code,
    "non-reassignable-binding",
  );
  assert.equal(
    analyze(fixture("compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic-3"))
      .diagnostics[0]?.code,
    "type-mismatch",
  );
  assert.equal(
    analyze(fixture("compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic-4"))
      .diagnostics[0]?.code,
    "invalid-binary-operands",
  );
  assert.equal(
    analyze(fixture("compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic-5"))
      .diagnostics[0]?.code,
    "invalid-binary-operands",
  );
  assert.equal(
    analyze(fixture("compiler/07-checker-rejects-name-mutability-and-type-errors-diagnostic-6"))
      .diagnostics[0]?.code,
    "nonfinal-vararg",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/07-checker-rejects-name-mutability-and-type-errors-positional-spread-needs-vararg",
      ),
    ).diagnostics[0]?.code,
    "positional-spread-needs-vararg",
  );
});

test("homogeneous varargs lower through the existing list ABI", async () => {
  const source = fixture("compiler/08-homogeneous-varargs-lower-through-the-existing-list-abi");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 34);
  const variadic = compilation.hir?.functions.find((fn) => fn.name === "count");
  assert.equal(variadic?.parameters[0]?.type, "list[i32]");
});

test("first-class vararg functions retain their calling convention", async () => {
  const source = fixture(
    "compiler/09-first-class-vararg-functions-retain-their-calling-convention",
  );
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 23);
  assert.equal(
    analyze(
      fixture(
        "compiler/09-first-class-vararg-functions-retain-their-calling-convention-type-mismatch",
      ),
    ).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("function parameter defaults evaluate after explicit arguments", async () => {
  const source = fixture(
    "compiler/10-function-parameter-defaults-evaluate-after-explicit-arguments",
  );
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

test("function parameter defaults enforce order, type, and purity", () => {
  assert.equal(
    analyze(
      fixture("compiler/11-function-parameter-defaults-enforce-order-type-and-purity-diagnostic"),
    ).diagnostics[0]?.code,
    "parameter-default-order",
  );
  assert.equal(
    analyze(
      fixture("compiler/11-function-parameter-defaults-enforce-order-type-and-purity-diagnostic-2"),
    ).diagnostics[0]?.code,
    "type-mismatch",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/11-function-parameter-defaults-enforce-order-type-and-purity-impure-parameter-default",
      ),
    ).diagnostics[0]?.code,
    "impure-parameter-default",
  );
});

test("varargs work in suspending and trait method calls", async () => {
  const source = fixture("compiler/12-varargs-work-in-suspending-and-trait-method-calls");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 108);
});

test("named arguments map to parameters without changing source evaluation order", async () => {
  const ordinary = await instantiate(
    fixture(
      "compiler/13-named-arguments-map-to-parameters-without-changing-source-evaluation-ord-2",
    ),
  );
  assert.equal((ordinary.instance.exports.main as CallableFunction)(), 42);

  const source = fixture(
    "compiler/13-named-arguments-map-to-parameters-without-changing-source-evaluation-ord",
  );
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
  const source = fixture("compiler/14-named-enum-payloads-preserve-source-evaluation-order");
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
    analyze(
      fixture(
        "compiler/14-named-enum-payloads-preserve-source-evaluation-order-unknown-named-argument",
      ),
    ).diagnostics[0]?.code,
    "unknown-named-argument",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/14-named-enum-payloads-preserve-source-evaluation-order-duplicate-argument",
      ),
    ).diagnostics[0]?.code,
    "duplicate-argument",
  );
});

test("named enum payload patterns resolve bindings by field name", async () => {
  const source = fixture("compiler/15-named-enum-payload-patterns-resolve-bindings-by-field-name");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture(
        "compiler/15-named-enum-payload-patterns-resolve-bindings-by-field-name-unknown-variant-pattern-field",
      ),
    ).diagnostics[0]?.code,
    "unknown-variant-pattern-field",
  );
});

test("literal enum payload patterns constrain variants in Wasm and suspension CFG", async () => {
  const source = fixture(
    "compiler/16-literal-enum-payload-patterns-constrain-variants-in-wasm-and-suspension-",
  );
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 138);
  assert.equal(
    analyze(
      fixture(
        "compiler/16-literal-enum-payload-patterns-constrain-variants-in-wasm-and-suspension--nonexhaustive-match",
      ),
    ).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
});

test("shared enum data uses per-variant factories and pure ordered defaults", async () => {
  const source = fixture(
    "compiler/17-shared-enum-data-uses-per-variant-factories-and-pure-ordered-defaults",
  );
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
  const source = fixture(
    "compiler/18-named-arguments-work-through-static-and-dynamic-trait-dispatch",
  );
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("prelude names cannot be shadowed by declarations or bindings", () => {
  const cases = [
    fixture("compiler/19-shadow-display"),
    fixture("compiler/19-shadow-println"),
    fixture("compiler/19-shadow-result"),
    fixture("compiler/19-shadow-console"),
    fixture("compiler/19-shadow-hash"),
  ];
  for (const source of cases) {
    assert.equal(analyze(source).diagnostics[0]?.code, "prelude-name-shadow", source);
  }
});

test("branch scopes do not leak and may shadow each other", () => {
  const source = fixture("compiler/20-branch-scopes-do-not-leak-and-may-shadow-each-other");
  assert.deepEqual(analyze(source).diagnostics, []);
  assert.equal(
    analyze(fixture("compiler/20-branch-scopes-do-not-leak-and-may-shadow-each-other-diagnostic"))
      .diagnostics[0]?.code,
    "unknown-name",
  );
});

test("else if chains preserve value typing and selection order", async () => {
  const source = fixture("compiler/21-else-if-chains-preserve-value-typing-and-selection-order");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("while, break, and continue lower to structured Wasm control flow", async () => {
  const source = fixture(
    "compiler/22-while-break-and-continue-lower-to-structured-wasm-control-flow",
  );
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(loop \$loop/);
  assert.equal((instance.exports.main as CallableFunction)(), 7);
  assert.equal(
    analyze(
      fixture(
        "compiler/22-while-break-and-continue-lower-to-structured-wasm-control-flow-diagnostic",
      ),
    ).diagnostics[0]?.code,
    "break-outside-loop",
  );
});

test("while else produces values on break or normal exhaustion", async () => {
  const broken = fixture(
    "compiler/23-while-else-produces-values-on-break-or-normal-exhaustion-broken",
  );
  const brokenResult = await instantiate(broken);
  assert.equal((brokenResult.instance.exports.main as CallableFunction)(), 42);

  const exhausted = fixture(
    "compiler/23-while-else-produces-values-on-break-or-normal-exhaustion-exhausted",
  );
  const exhaustedResult = await instantiate(exhausted);
  assert.equal((exhaustedResult.instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture("compiler/23-while-else-produces-values-on-break-or-normal-exhaustion-diagnostic"),
    ).diagnostics[0]?.code,
    "break-value-context",
  );
  assert.equal(
    analyze(
      fixture("compiler/23-while-else-produces-values-on-break-or-normal-exhaustion-diagnostic-2"),
    ).diagnostics[0]?.code,
    "break-value-context",
  );
});

test("for loops iterate lists and maps with continue, destructuring, and else values", async () => {
  const source = fixture(
    "compiler/24-for-loops-iterate-lists-and-maps-with-continue-destructuring-and-else-va",
  );
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture(
        "compiler/24-for-loops-iterate-lists-and-maps-with-continue-destructuring-and-else-va-diagnostic",
      ),
    ).diagnostics[0]?.code,
    "not-iterable",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/24-for-loops-iterate-lists-and-maps-with-continue-destructuring-and-else-va-diagnostic-2",
      ),
    ).diagnostics[0]?.code,
    "for-binding-arity",
  );
});

test("data declarations lower to Wasm GC structs", async () => {
  const source = fixture("compiler/25-data-declarations-lower-to-wasm-gc-structs");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$d0 \(struct/);
  assert.match(compilation.wat, /\(struct\.new \$d0/);
  assert.match(compilation.wat, /\(struct\.get \$d0 \$d0f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("mutable data paths weaken one way and share Wasm GC identity", async () => {
  const source = fixture(
    "compiler/26-mutable-data-paths-weaken-one-way-and-share-wasm-gc-identity",
  );
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /struct\.set \$d0/);
  assert.match(compilation.wat, /field \$d1f0 \(mut/);

  assert.equal(
    analyze('data User:\n    name: string\nfn bad(user: User) -> void: user.name = "x"\n')
      .diagnostics[0]?.code,
    "readonly-root",
  );
  assert.equal(
    analyze(
      'data Child:\n    name: string\ndata Parent:\n    child: Child\nfn bad(parent: mut Parent) -> void: parent.child.name = "x"\n',
    ).diagnostics[0]?.code,
    "readonly-edge",
  );
});

test("mutable list and map roots support indexed replacement", async () => {
  const source = fixture("compiler/27-mutable-list-and-map-roots-support-indexed-replacement");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /array\.set \$hd\.list/);
  assert.match(compilation.wat, /call \$hd\.map_insert/);
});

test("mutable lists append through growable Wasm GC storage", async () => {
  const source = fixture("compiler/28-mutable-lists-append-through-growable-wasm-gc-storage");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /type \$hd\.vector \(struct/);
  assert.match(compilation.wat, /call \$hd\.vector_append/);
  assert.ok(
    analyze(
      fixture(
        "compiler/28-mutable-lists-append-through-growable-wasm-gc-storage-mutable-receiver-required",
      ),
    ).diagnostics.some((diagnostic) => diagnostic.code === "mutable-receiver-required"),
  );
});

test("mutable maps grow from empty storage and remove entries in insertion order", async () => {
  const source = fixture(
    "compiler/29-mutable-maps-grow-from-empty-storage-and-remove-entries-in-insertion-ord",
  );
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.match(compilation.wat, /call \$hd\.map_remove/);
  assert.match(compilation.wat, /struct\.set \$hd\.map \$hd\.map-keys/);

  assert.equal(
    analyze(
      fixture(
        "compiler/29-mutable-maps-grow-from-empty-storage-and-remove-entries-in-insertion-ord-mutable-receiver-required",
      ),
    ).diagnostics[0]?.code,
    "mutable-receiver-required",
  );
});

test("data field initializers preserve source evaluation order", async () => {
  const source = fixture("compiler/30-data-field-initializers-preserve-source-evaluation-order");
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
  const source = fixture(
    "compiler/31-data-field-defaults-run-per-construction-after-explicit-fields",
  );
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

test("data field defaults are type checked and purity checked", () => {
  assert.equal(
    analyze(
      fixture(
        "compiler/32-data-field-defaults-are-type-checked-and-purity-checked-impure-data-default",
      ),
    ).diagnostics.length,
    0,
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/32-data-field-defaults-are-type-checked-and-purity-checked-impure-data-default-2",
      ),
    ).diagnostics[0]?.code,
    "impure-data-default",
  );
  assert.equal(
    analyze(
      fixture("compiler/32-data-field-defaults-are-type-checked-and-purity-checked-type-mismatch"),
    ).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("data copy-update evaluates its source before replacements", async () => {
  const source = fixture("compiler/33-data-copy-update-evaluates-its-source-before-replacements");
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
  const source = fixture(
    "compiler/34-data-copy-update-supplies-omitted-fields-without-running-defaults",
  );
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture(
        "compiler/34-data-copy-update-supplies-omitted-fields-without-running-defaults-type-mismatch",
      ),
    ).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("fieldless data lowers to an empty Wasm GC struct", async () => {
  const source = fixture("compiler/35-fieldless-data-lowers-to-an-empty-wasm-gc-struct");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$d0 \(struct\s*\)\)/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("data initialization checks field names, presence, and types", () => {
  assert.equal(
    analyze(fixture("compiler/36-data-initialization-unknown-field")).diagnostics[0]?.code,
    "unknown-data-field",
  );
  assert.equal(
    analyze(fixture("compiler/36-data-initialization-missing-field")).diagnostics[0]?.code,
    "missing-required-field",
  );
  assert.equal(
    analyze(fixture("compiler/36-data-initialization-wrong-field-type")).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("data patterns destructure nested fields and test literals", async () => {
  const source = fixture("compiler/37-data-patterns-destructure-nested-fields-and-test-literals");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /struct\.get \$d0 \$d0f0 \(struct\.get \$d1 \$d1f0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  assert.equal(
    analyze(fixture("compiler/37-data-pattern-duplicate-field")).diagnostics[0]?.code,
    "duplicate-data-pattern-field",
  );
  assert.equal(
    analyze(fixture("compiler/37-data-pattern-unknown-field")).diagnostics[0]?.code,
    "unknown-data-field",
  );
  assert.equal(
    analyze(fixture("compiler/37-data-pattern-nonexhaustive")).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
});

test("strings use GC byte arrays and len counts Unicode scalars", async () => {
  const source = fixture("compiler/38-strings-use-gc-byte-arrays-and-len-counts-unicode-scalars");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.bytes 8/);
  assert.equal((instance.exports.main as CallableFunction)(), 3);
});

test("string interpolation displays built-ins from left to right", async () => {
  const source = fixture("compiler/39-string-interpolation-displays-built-ins-left-to-right");
  const { instance, compilation } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 29);
  assert.match(compilation.wat, /call \$hd\.i32_to_string/);
  assert.match(compilation.wat, /call \$hd\.char_to_string/);
  assert.equal(analyze('fn main() -> string: "${1.5}"\n').diagnostics[0]?.code, "missing-display");
});

test("strings compare by UTF-8 value order", async () => {
  const source = fixture("compiler/40-strings-compare-by-utf-8-value-order");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /call \$hd\.string_compare/);
  assert.match(compilation.wat, /call \$hd\.string_starts_with/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("strings concatenate and unnamed enum fields use numeric selectors", async () => {
  const source = fixture(
    "compiler/41-strings-concatenate-and-unnamed-enum-fields-use-numeric-selectors",
  );
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /call \$hd\.string_concat/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("character literals carry Unicode scalar values and compare in scalar order", async () => {
  const source = fixture(
    "compiler/42-character-literals-carry-unicode-scalar-values-and-compare-in-scalar-ord",
  );
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 0x1f600);
});

test("defer runs after a return value is evaluated", async () => {
  const source = fixture("compiler/43-defer-runs-after-a-return-value-is-evaluated");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /local\.set \$tmp0/);
  assert.equal((instance.exports.main as CallableFunction)(), 1);
});

test("defer rejects escaping control flow", () => {
  const source = fixture("compiler/44-defer-rejects-escaping-control-flow");
  assert.equal(analyze(source).diagnostics[0]?.code, "defer-control-flow");
});

test("defer is LIFO and runs on loop continue and break", async () => {
  const source = fixture("compiler/45-defer-is-lifo-and-runs-on-loop-continue-and-break");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 1221);
});

test("explicit panic lowers to unreachable and skips pending defer", async () => {
  const source = fixture(
    "compiler/46-explicit-panic-lowers-to-unreachable-and-skips-pending-defer",
  );
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /unreachable/);
  assert.throws(() => (instance.exports.main as CallableFunction)(), WebAssembly.RuntimeError);
});

test("enums use tagged GC structs and match binds payloads", async () => {
  const source = fixture("compiler/47-enums-use-tagged-gc-structs-and-match-binds-payloads");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$e0 \(struct/);
  assert.match(compilation.wat, /\(struct\.new \$e0 \(i32\.const 0\)/);
  assert.match(compilation.wat, /\(struct\.get \$e0 \$e0tag/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("match checking enforces coverage, payload arity, and arm reachability", () => {
  assert.equal(
    analyze(fixture("compiler/48-match-nonexhaustive")).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(fixture("compiler/48-match-pattern-arity")).diagnostics[0]?.code,
    "pattern-arity",
  );
  assert.equal(
    analyze(fixture("compiler/48-match-unreachable-arm")).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
  assert.equal(
    analyze(fixture("compiler/48-match-bare-variant")).diagnostics[0]?.code,
    "bare-variant-pattern",
  );
});

test("contextual enum variant patterns use the subject type", async () => {
  const source = fixture("compiler/49-contextual-enum-variant-patterns-use-the-subject-type");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);

  const swapped = analyze(
    fixture(
      "compiler/49-contextual-enum-variant-patterns-use-the-subject-type-variant-binding-name-mismatch",
    ),
  );
  assert.ok(swapped.hir);
  assert.ok(
    swapped.diagnostics.some(
      (diagnostic) =>
        diagnostic.code === "variant-binding-name-mismatch" && diagnostic.severity === "warning",
    ),
  );
});

test("contextual enum variant expressions use their expected type", async () => {
  const source = fixture("compiler/50-contextual-enum-variant-expressions-use-their-expected-type");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture("compiler/50-contextual-enum-variant-expressions-use-their-expected-type-diagnostic"),
    ).diagnostics[0]?.code,
    "missing-contextual-enum-type",
  );
});

test("boolean matches are exhaustive and lower to scalar tests", async () => {
  const source = fixture("compiler/51-boolean-matches-are-exhaustive-and-lower-to-scalar-tests");
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /i32\.eq[\s\S]*local\.get \$tmp/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture("compiler/51-boolean-matches-are-exhaustive-and-lower-to-scalar-tests-diagnostic"),
    ).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/51-boolean-matches-are-exhaustive-and-lower-to-scalar-tests-unreachable-match-arm",
      ),
    ).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
});

test("numeric, character, and string literal patterns require a catch-all", async () => {
  const source = fixture(
    "compiler/52-numeric-character-and-string-literal-patterns-require-a-catch-all",
  );
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /call \$hd\.string_compare/);
  assert.match(compilation.wat, /f64\.eq/);
  assert.equal((instance.exports.main as CallableFunction)(), 32);
  assert.equal(
    analyze(
      fixture(
        "compiler/52-numeric-character-and-string-literal-patterns-require-a-catch-all-diagnostic",
      ),
    ).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/52-numeric-character-and-string-literal-patterns-require-a-catch-all-unreachable-match-arm",
      ),
    ).diagnostics[0]?.code,
    "unreachable-match-arm",
  );
});

test("match guards see pattern bindings and do not contribute coverage", async () => {
  const source = fixture(
    "compiler/53-match-guards-see-pattern-bindings-and-do-not-contribute-coverage",
  );
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture(
        "compiler/53-match-guards-see-pattern-bindings-and-do-not-contribute-coverage-diagnostic",
      ),
    ).diagnostics[0]?.code,
    "nonexhaustive-match",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/53-match-guards-see-pattern-bindings-and-do-not-contribute-coverage-type-mismatch",
      ),
    ).diagnostics.length,
    0,
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/53-match-guards-see-pattern-bindings-and-do-not-contribute-coverage-type-mismatch-2",
      ),
    ).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("optionals inject plain values, match both cases, and propagate nil", async () => {
  const source = fixture(
    "compiler/54-optionals-inject-plain-values-match-both-cases-and-propagate-nil",
  );
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /type \$hd\.variant/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.equal((instance.exports.main as CallableFunction)(), 41);
});

test("Result constructors, matching, and error propagation use the erased carrier", async () => {
  const source = fixture(
    "compiler/55-result-constructors-matching-and-error-propagation-use-the-erased-carrie",
  );
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 35);
});

test("imported ResourceError is a generic Wasm GC enum", async () => {
  const source = fixture("compiler/56-imported-resourceerror-is-a-generic-wasm-gc-enum");
  const { instance, compilation } = await instantiate(source);
  assert.equal(compilation.hir.enums.at(-1)?.name, "ResourceError");
  assert.match(compilation.wat, /type \$e\d+ \(struct/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("Result patterns recursively match imported enum payloads", async () => {
  const source = fixture("compiler/57-result-patterns-recursively-match-imported-enum-payloads");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("optional and Result context errors have stable diagnostics", () => {
  assert.equal(
    analyze(
      fixture("compiler/58-optional-and-result-context-errors-have-stable-diagnostics-diagnostic"),
    ).diagnostics[0]?.code,
    "nil-needs-optional-type",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/58-optional-and-result-context-errors-have-stable-diagnostics-diagnostic-2",
      ),
    ).diagnostics[0]?.code,
    "result-constructor-needs-context",
  );
  assert.equal(
    analyze(
      fixture(
        "compiler/58-optional-and-result-context-errors-have-stable-diagnostics-diagnostic-3",
      ),
    ).diagnostics[0]?.code,
    "invalid-result-propagation",
  );
  const nested = analyze(
    fixture("compiler/58-optional-and-result-context-errors-have-stable-diagnostics-diagnostic-4"),
  );
  assert.ok(nested.hir);
  assert.equal(nested.diagnostics[0]?.code, "unused-local-binding");
});

test("optional and Result values are must-use unless explicitly discarded", () => {
  assert.equal(
    analyze(fixture("compiler/59-optional-result-discarded")).diagnostics[0]?.code,
    "discarded-must-use-value",
  );
  assert.deepEqual(
    analyze(fixture("compiler/59-optional-result-explicitly-discarded")).diagnostics,
    [],
  );
});

test("typed noncapturing closures lower to Wasm typed function references", async () => {
  const source = fixture(
    "compiler/60-typed-noncapturing-closures-lower-to-wasm-typed-function-references",
  );
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /\(type \$sig0 \(func/);
  assert.match(compilation.wat, /call_ref \$sig0/);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture(
        "compiler/60-typed-noncapturing-closures-lower-to-wasm-typed-function-references-diagnostic",
      ),
    ).diagnostics[0]?.code,
    "type-mismatch",
  );
});

test("expected function types infer inline closure parameters and results", async () => {
  const source = fixture(
    "compiler/61-expected-function-types-infer-inline-closure-parameters-and-results",
  );
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture(
        "compiler/61-expected-function-types-infer-inline-closure-parameters-and-results-diagnostic",
      ),
    ).diagnostics[0]?.code,
    "closure-parameter-needs-annotation",
  );
});

test("nonrecursive closures infer result types from fallthrough and returns", async () => {
  const source = fixture(
    "compiler/62-nonrecursive-closures-infer-result-types-from-fallthrough-and-returns",
  );
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.equal(analysis.hir?.functions[0]?.locals[0]?.type, "fn(i32)->i32");
  const { instance } = await instantiate(source);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
  assert.equal(
    analyze(
      fixture(
        "compiler/62-nonrecursive-closures-infer-result-types-from-fallthrough-and-returns-closure-result-type",
      ),
    ).diagnostics[0]?.code,
    "closure-result-type",
  );
});

test("explicitly typed local closures recurse through their current environment", async () => {
  const source = fixture(
    "compiler/63-explicitly-typed-local-closures-recurse-through-their-current-environmen",
  );
  const { instance, compilation } = await instantiate(source);
  assert.match(compilation.wat, /ref\.func \$c0\) \(local\.get \$env\)/);
  assert.equal(compilation.hir.closures[0]?.captures.length, 0);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});

test("named local functions capture enclosing values and recurse", async () => {
  const source = fixture("compiler/64-named-local-functions-capture-enclosing-values-and-recurse");
  const { instance, compilation } = await instantiate(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.equal((instance.exports.main as CallableFunction)(), 42);
});
