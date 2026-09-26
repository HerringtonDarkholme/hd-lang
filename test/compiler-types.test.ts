import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { analyze, compile, instantiate } from "../src/compiler.ts";
import { conformance, fixture } from "./fixture.ts";

test("named functions reify as monomorphic function values", () => {
  const source = conformance("runtime/valid/function-value-argument");
  const compilation = compile(source);
  assert.match(compilation.wat, /func \$fv0 \(type \$sig/);
  assert.match(compilation.wat, /ref\.func \$fv0/);
});

test("erased generic functions box primitive values", () => {
  const source = conformance("runtime/valid/generic-inference-scalars-and-data");
  const compilation = compile(source);
  assert.match(compilation.wat, /param \$l0 anyref/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-f64\)/);
});

test("higher-order erased generics adapt concrete callable ABIs", () => {
  const source = conformance("runtime/valid/generic-callable-adapter");
  const compilation = compile(source);
  assert.match(compilation.wat, /func \$adapt0/);
  assert.match(compilation.wat, /ref\.cast \(ref \$closure/);
});

test("generic requirement rows pack callback providers for Wasm GC", () => {
  const source = conformance("runtime/valid/row-variable-binds-union");
  const compilation = compile(source);
  assert.deepEqual(compilation.hir.functions[0]?.rowParameters, ["r"]);
  assert.equal(compilation.hir.functions[0]?.parameters[0]?.type, "fn()->i32$row:r");
  assert.match(compilation.wat, /type \$hd\.providers \(struct/);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
  assert.match(compilation.wat, /call \$hd\.provider_get/);
});

test("empty generic requirement rows lower to null provider packs", () => {
  const empty = conformance("runtime/valid/row-inference-empty-row");
  const compilation = compile(empty);
  assert.match(compilation.wat, /ref\.null \$hd\.providers/);
});

test("generic row subtraction lowers a locally supplied provider", () => {
  const source = conformance("runtime/valid/row-subtraction-provider-restoration");
  const compilation = compile(source);
  assert.deepEqual(compilation.hir.functions[0]?.requirements, ["Backup", "row:r\\Logger"]);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
});

test("generic row forwarding composes symbolic and concrete provider packs", () => {
  const source = conformance("runtime/valid/row-polymorphic-forwarding");
  const compilation = compile(source);
  const forwarded = compilation.hir.functions[1]?.body[0];
  assert.equal(forwarded?.kind, "expression");
  assert.equal(forwarded?.kind === "expression" && forwarded.expression.kind, "call");
  const pack =
    forwarded?.kind === "expression" && forwarded.expression.kind === "call"
      ? forwarded.expression.providers[0]
      : undefined;
  assert.equal(pack?.kind, "provider-pack");
  assert.deepEqual(pack?.kind === "provider-pack" && pack.keys, ["Clock"]);
  assert.equal(pack?.kind === "provider-pack" && pack.bases.length, 1);
  assert.match(compilation.wat, /call \$hd\.provider_concat/);
});

test("generic row forwarding unions multiple symbolic provider packs", () => {
  const source = conformance("runtime/valid/row-polymorphic-union-forwarding");
  const compilation = compile(source);
  assert.match(compilation.wat, /func \$hd\.provider_concat/);
});

test("distinct generic provider keys emit valid Wasm", () => {
  const distinct = conformance("typing/valid/generic-provider-keys-distinct");
  const compilation = compile(distinct);
  assert.ok(WebAssembly.validate(compilation.bytes));
});

test("trait implementations lower static and Wasm GC dynamic dispatch", () => {
  const source = conformance("runtime/valid/static-and-dynamic-trait-dispatch");
  const compilation = compile(source);
  assert.match(compilation.wat, /type \$trait0 \(struct/);
  assert.match(compilation.wat, /func \$tadapt0_0/);
  assert.match(compilation.wat, /call_ref \$tsig0_0/);
});

test("mutable trait receivers retain permission in HIR", () => {
  const source = conformance("runtime/valid/mutable-receivers");
  const compilation = compile(source);
  assert.equal(compilation.hir.traits[0]?.methods[0]?.receiverMutable, true);
  assert.equal(compilation.hir.traits[0]?.methods[1]?.receiverMutable, true);
});

test("inherent methods lower as direct functions", () => {
  const source = conformance("runtime/valid/inherent-methods");
  const compilation = compile(source);
  assert.match(compilation.wat, /func \$f\d+/);
});

test("default trait methods lower into static and dynamic dispatch", () => {
  const source = conformance("runtime/valid/trait-default-method-inherited");
  const compilation = compile(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
});

test("suspending trait methods use concrete and dynamic Wasm GC frames", () => {
  const source = conformance("runtime/valid/suspending-trait-dispatch");
  const compilation = compile(source);
  assert.match(compilation.wat, /type \$ts0_0 \(struct/);
  assert.match(compilation.wat, /ref\.func \$tspolladapt0_0/);
  assert.match(compilation.wat, /call_ref \$tspollsig0_0/);
});

test("cancelling a dynamic suspending trait method reaches child cleanup", async () => {
  const source = `trait Job:
    fn run!(self) -> void

data Worker:
    value: i32

fn wait!() -> void: pass

impl Job for Worker:
    fn run!(self) -> void:
        defer:
            _ := self.value
        wait!()

fn main!() -> void:
    let job: Job = Worker { value: 42 }
    job.run!()
`;
  const events: Array<[number, number]> = [];
  const { instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  (instance.exports.__hd_start as CallableFunction)();
  (instance.exports.__hd_poll as CallableFunction)();
  (instance.exports.__hd_cancel as CallableFunction)();
  assert.ok(events.some(([functionIndex, event]) => functionIndex === 2 && event === 7));
});

test("default suspending trait methods lower for each implementation", () => {
  const source = conformance("runtime/valid/suspending-trait-default-method");
  const compilation = compile(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
});

test("trait providers lower through generic row subtraction", () => {
  const source = conformance("runtime/valid/trait-value-as-provider");
  const compilation = compile(source);
  assert.match(compilation.wat, /field \$hd\.provider-value anyref/);
  assert.match(compilation.wat, /ref\.cast \(ref null \$trait0\)/);
});

test("erased generic trait bounds lower dictionary dispatch", () => {
  const source = conformance("runtime/valid/generic-bound-dispatch");
  const compilation = compile(source);
  assert.match(compilation.wat, /param \$bound0 \(ref null \$trait0\)/);
  assert.match(compilation.wat, /struct\.new \$trait0 \(ref\.null any\)/);
  assert.match(compilation.wat, /struct\.get \$trait0 \$trait0value/);
});

test("multiple trait bounds lower independent dictionaries", () => {
  const source = conformance("runtime/valid/multiple-bounds-dispatch");
  const compilation = compile(source);
  assert.match(
    compilation.wat,
    /param \$bound0 \(ref null \$trait0\).*param \$bound1 \(ref null \$trait1\)/s,
  );
  assert.match(compilation.wat, /local\.get \$bound0/);
  assert.match(compilation.wat, /local\.get \$bound1/);
});

test("generic data uses one erased GC layout with precise instantiated member types", () => {
  const source = conformance("runtime/valid/generic-data-fields");
  const compilation = compile(source);
  assert.equal(
    compilation.hir.functions.find(({ name }) => name === "main")?.locals[0]?.type,
    "Box[i32]",
  );
  assert.match(compilation.wat, /field \$d0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref null \$d0\)/);
});

test("explicit generic data construction records its instantiated HIR type", () => {
  const source = readFileSync(
    resolve(import.meta.dirname, "../spec/conformance/runtime/valid/generic-data-embedding.hd"),
    "utf8",
  );
  const compilation = compile(source);
  assert.equal(compilation.hir.functions.at(-1)?.locals[0]?.type, "Shipment[i32]");
});

test("generic enums erase payloads and recover instantiated match bindings", () => {
  const source = conformance("runtime/valid/generic-enum-payloads");
  const compilation = compile(source);
  assert.equal(
    compilation.hir.functions.find(({ name }) => name === "main")?.locals[0]?.type,
    "Maybe[i32]",
  );
  assert.match(compilation.wat, /field \$e0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-i32\)/);
});

test("generic lists lower to growable GC vectors with erased element storage", () => {
  const source = conformance("runtime/valid/generic-list-element");
  const compilation = compile(source);
  assert.match(compilation.wat, /type \$hd\.vector \(struct/);
  assert.match(compilation.wat, /type \$hd\.list \(array \(mut anyref\)\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.vector/);
  assert.match(compilation.wat, /call \$hd\.vector_get/);
});

test("generic maps lower to GC storage with lookup and replacement", () => {
  const source = conformance("runtime/valid/map-lookup-and-duplicate-keys");
  const compilation = compile(source);
  assert.match(compilation.wat, /type \$hd\.map \(struct/);
  assert.match(compilation.wat, /call \$hd\.map_insert/);
  assert.match(compilation.wat, /call \$hd\.map_get/);
});

test("capturing closures store outer locals in GC environments", () => {
  const source = conformance("runtime/valid/closure-captures-local");
  const compilation = compile(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /struct\.new \$closure0/);
});

test("reference identity lowers to Wasm GC identity", () => {
  const source = conformance("runtime/valid/reference-identity");
  const compilation = compile(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /ref\.eq/);
  assert.match(compilation.wat, /global \$e0v0/);
});

test("heterogeneous tuples lower to Wasm GC storage", () => {
  const source = conformance("runtime/valid/heterogeneous-tuples");
  const compilation = compile(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.list/);
});

test("nested closures propagate grandparent captures through GC environments", () => {
  const source = conformance("runtime/valid/nested-closure-captures");
  const compilation = compile(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /\(type \$env1 \(struct/);
  assert.match(compilation.wat, /struct\.get \$env0 \$env0f0/);
});

test("closures store escaped lexical provider overrides in GC environments", () => {
  const source = `fn make_reader() -> (fn() -> i32) $ Backup:
    $.with(Clock=$.use(Backup)):
        fn() -> i32:
            _ := $.use(Clock)
            42

fn main() -> i32 $ Backup:
    reader := make_reader()
    reader()
`;
  const compilation = compile(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct\n\s*\(field \$env0f0 externref\)/);
});

test("requirement-bearing closures lower provider parameters", () => {
  const source = `fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock:
    reader := fn() -> i32 $ Clock:
        _ := $.use(Clock)
        42
    invoke(reader)
`;
  const compilation = compile(source);
  assert.match(compilation.wat, /type \$sig0 \(func \(param anyref\) \(param externref\)/);
  assert.match(compilation.wat, /call_ref \$sig0/);
});

test("requirement-bearing function values lower provider parameters", () => {
  const source = `fn read() -> i32 $ Clock:
    _ := $.use(Clock)
    42

fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock: invoke(read)
`;
  const compilation = compile(source);
  assert.match(compilation.wat, /func \$fv0[^]*param \$provider0 externref/);
});

test("closure HIR records inferred unsatisfied requirements", () => {
  const source = conformance("runtime/valid/closure-inferred-requirement-row");
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.equal(analysis.hir?.functions[0]?.locals[0]?.type, "fn()->i32$Clock");
});

test("concrete requirement rows lower hidden externref providers", () => {
  const source = `fn read() -> i32 $ Clock: 40
fn middle() -> i32 $ Clock: read() + 1
fn main() -> i32 $ Clock: middle() + 1
`;
  const compilation = compile(source);
  assert.match(compilation.wat, /param \$provider0 externref/);
  assert.match(compilation.wat, /call \$f0 \(local\.get \$provider0\)/);
});

test("provider scopes lower hidden provider locals", () => {
  const source = `fn main() -> i32 $ Clock + Backup:
    _ := $.use(Clock)
    $.with(Clock=$.use(Backup)):
        _ := $.use(Clock)
        42
`;
  const compilation = compile(source);
  assert.match(compilation.wat, /local\.set \$l0 \(local\.get \$provider0\)/);
});

test("concrete requirement rows normalize union and subtraction as sets", () => {
  const result = analyze(conformance("typing/valid/requirement-row-ungrouped-subtraction"));
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(result.hir?.functions[0]?.requirements, ["Clock"]);

  const grouped = analyze(conformance("typing/valid/requirement-row-grouped-subtraction"));
  assert.deepEqual(grouped.diagnostics, []);
  assert.deepEqual(grouped.hir?.functions[0]?.requirements, ["Clock"]);

  const empty = analyze(fixture("compiler-types/requirements/normalization/explicit-empty-row"));
  assert.deepEqual(empty.diagnostics, []);
});

test("provider contexts lower to GC structs and lexical call arguments", () => {
  const source = conformance("runtime/valid/context-values-install-providers");
  const compilation = compile(source);
  assert.match(compilation.wat, /\(type \$context0 \(struct/);
  assert.match(compilation.wat, /struct\.new \$context0/);
  assert.match(compilation.wat, /struct\.get \$context0 \$context0f0/);
  assert.match(compilation.wat, /call \$f1 \(local\.get \$l/);
});

test("context creation accepts spreads and normalizes exact replacement keys", () => {
  const source = conformance("typing/valid/context-spread-normalization");
  const result = analyze(source);
  assert.deepEqual(result.diagnostics, []);
  const final = result.hir?.functions[0]?.body.at(-1);
  assert.equal(final?.kind, "expression");
  if (final?.kind === "expression") assert.equal(final.expression.type, "context:Backup+Clock");
});
