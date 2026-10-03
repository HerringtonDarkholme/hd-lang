import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { analyze, compileToWasm, instantiate } from "../src/compiler.ts";
import { conformance } from "./fixture.ts";

test("named functions reify as monomorphic function values", async () => {
  const source = conformance("runtime/valid/function-value-argument");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /func \$fv0 \(type \$sig/);
  assert.match(compilation.wat, /ref\.func \$fv0/);
});

test("erased generic functions box primitive values", async () => {
  const source = conformance("runtime/valid/generic-inference-scalars-and-data");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /param \$l0 anyref/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-f64\)/);
});

test("higher-order erased generics adapt concrete callable ABIs", async () => {
  const source = conformance("runtime/valid/generic-callable-adapter");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /func \$adapt0/);
  assert.match(compilation.wat, /ref\.cast \(ref \$closure/);
});

test("generic requirement rows pack callback providers for Wasm GC", async () => {
  const source = conformance("runtime/valid/row-variable-binds-union");
  const compilation = await compileToWasm(source);
  assert.deepEqual(compilation.hir.functions[0]?.rowParameters, ["R"]);
  assert.equal(compilation.hir.functions[0]?.parameters[0]?.type, "fn()->i32$row:R");
  assert.match(compilation.wat, /type \$hd\.providers \(struct/);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
  assert.match(compilation.wat, /call \$hd\.provider_get/);
});

test("empty generic requirement rows lower to null provider packs", async () => {
  const empty = conformance("runtime/valid/row-inference-empty-row");
  const compilation = await compileToWasm(empty);
  assert.match(compilation.wat, /ref\.null \$hd\.providers/);
});

test("row extension lowers a locally supplied provider for the removed key", async () => {
  const source = conformance("runtime/valid/row-extension-provider-restoration");
  const compilation = await compileToWasm(source);
  assert.deepEqual(compilation.hir.functions[0]?.requirements, ["Backup", "row:R"]);
  assert.match(compilation.wat, /struct\.new \$hd\.providers/);
});

test("generic row forwarding composes symbolic and concrete provider packs", async () => {
  const source = conformance("runtime/valid/row-polymorphic-forwarding");
  const compilation = await compileToWasm(source);
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

test("generic row forwarding unions multiple symbolic provider packs", async () => {
  const source = conformance("runtime/valid/row-polymorphic-union-forwarding");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /func \$hd\.provider_concat/);
});

test("distinct generic provider keys emit valid Wasm", async () => {
  const distinct = conformance("typing/valid/generic-provider-keys-distinct");
  const compilation = await compileToWasm(distinct);
  assert.ok(WebAssembly.validate(compilation.bytes));
});

test("trait implementations lower static and Wasm GC dynamic dispatch", async () => {
  const source = conformance("runtime/valid/static-and-dynamic-trait-dispatch");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /type \$trait0 \(struct/);
  assert.match(compilation.wat, /func \$tadapt0_0/);
  assert.match(compilation.wat, /call_ref \$tsig0_0/);
});

test("mutable trait receivers retain permission in HIR", async () => {
  const source = conformance("runtime/valid/mutable-receivers");
  const compilation = await compileToWasm(source);
  assert.equal(compilation.hir.traits[0]?.methods[0]?.receiverMutable, true);
  assert.equal(compilation.hir.traits[0]?.methods[1]?.receiverMutable, true);
});

test("inherent methods lower as direct functions", async () => {
  const source = conformance("runtime/valid/inherent-methods");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /func \$f\d+/);
});

test("default trait methods lower into static and dynamic dispatch", async () => {
  const source = conformance("runtime/valid/trait-default-method-inherited");
  const compilation = await compileToWasm(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
});

test("suspending trait methods use concrete and dynamic Wasm GC frames", async () => {
  const source = conformance("runtime/valid/suspending-trait-dispatch");
  const compilation = await compileToWasm(source);
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
  const { compilation, instance } = await instantiate(source, {
    trace: (functionIndex, event) => events.push([functionIndex, event]),
    pending: (functionIndex, pollCount) => functionIndex === 0 && pollCount === 1,
  });
  (instance.exports.__hd_start as CallableFunction)();
  (instance.exports.__hd_poll as CallableFunction)();
  (instance.exports.__hd_cancel as CallableFunction)();
  const run = compilation.hir.functions.find((declaration) => declaration.name === "$impl0.run");
  assert.ok(events.some(([functionIndex, event]) => functionIndex === run?.index && event === 7));
});

test("default suspending trait methods lower for each implementation", async () => {
  const source = conformance("runtime/valid/suspending-trait-default-method");
  const compilation = await compileToWasm(source);
  assert.equal(compilation.hir.implementations[0]?.methodFunctions.length, 2);
  assert.match(compilation.wat, /func \$tadapt0_1/);
});

test("trait providers lower through generic row extension", async () => {
  const source = conformance("runtime/valid/trait-value-as-provider");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /field \$hd\.provider-value anyref/);
  // The callback adapter takes the removed key as a typed trait provider
  // beside the provider pack for R.
  assert.match(
    compilation.wat,
    /param \$p0 \(ref null \$trait0\)\) \(param \$p1 \(ref null \$hd\.providers\)\)/,
  );
});

test("erased generic trait bounds lower dictionary dispatch", async () => {
  const source = conformance("runtime/valid/generic-bound-dispatch");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /param \$bound0 \(ref null \$trait0\)/);
  assert.match(compilation.wat, /struct\.new \$trait0 \(ref\.null any\)/);
  assert.match(compilation.wat, /struct\.get \$trait0 \$trait0value/);
});

test("multiple trait bounds lower independent dictionaries", async () => {
  const source = conformance("runtime/valid/multiple-bounds-dispatch");
  const compilation = await compileToWasm(source);
  assert.match(
    compilation.wat,
    /param \$bound0 \(ref null \$trait0\).*param \$bound1 \(ref null \$trait1\)/s,
  );
  assert.match(compilation.wat, /local\.get \$bound0/);
  assert.match(compilation.wat, /local\.get \$bound1/);
});

test("generic data uses one erased GC layout with precise instantiated member types", async () => {
  const source = conformance("runtime/valid/generic-data-fields");
  const compilation = await compileToWasm(source);
  assert.equal(
    compilation.hir.functions.find(({ name }) => name === "main")?.locals[0]?.type,
    "Box[i32]",
  );
  assert.match(compilation.wat, /field \$d0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref null \$d0\)/);
});

test("explicit generic data construction records its instantiated HIR type", async () => {
  const source = readFileSync(
    resolve(import.meta.dirname, "../spec/conformance/runtime/valid/generic-data-embedding.hd"),
    "utf8",
  );
  const compilation = await compileToWasm(source);
  const body = compilation.hir.functions.find((declaration) => declaration.name === "$test.0");
  assert.equal(body?.locals[0]?.type, "Shipment[i32]");
});

test("generic enums erase payloads and recover instantiated match bindings", async () => {
  const source = conformance("runtime/valid/generic-enum-payloads");
  const compilation = await compileToWasm(source);
  assert.equal(
    compilation.hir.functions.find(({ name }) => name === "main")?.locals[0]?.type,
    "Maybe[i32]",
  );
  assert.match(compilation.wat, /field \$e0f0 \(mut anyref\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.box-i32/);
  assert.match(compilation.wat, /ref\.cast \(ref \$hd\.box-i32\)/);
});

test("generic lists lower to growable GC vectors with erased element storage", async () => {
  const source = conformance("runtime/valid/generic-list-element");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /type \$hd\.vector \(struct/);
  assert.match(compilation.wat, /type \$hd\.list \(array \(mut anyref\)\)/);
  assert.match(compilation.wat, /struct\.new \$hd\.vector/);
  assert.match(compilation.wat, /call \$hd\.vector_get/);
});

test("generic maps lower to GC storage with lookup and replacement", async () => {
  const source = conformance("runtime/valid/map-lookup-and-duplicate-keys");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /type \$hd\.map \(struct/);
  assert.match(compilation.wat, /call \$hd\.map_insert/);
  assert.match(compilation.wat, /call \$hd\.map_get/);
});

test("capturing closures store outer locals in GC environments", async () => {
  const source = conformance("runtime/valid/closure-captures-local");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /struct\.new \$closure0/);
});

test("reference identity lowers to Wasm GC identity", async () => {
  const source = conformance("runtime/valid/reference-identity");
  const compilation = await compileToWasm(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /ref\.eq/);
  assert.match(compilation.wat, /global \$e0v0/);
});

test("heterogeneous tuples lower to Wasm GC storage", async () => {
  const source = conformance("runtime/valid/heterogeneous-tuples");
  const compilation = await compileToWasm(source);
  assert.deepEqual(compilation.diagnostics, []);
  assert.match(compilation.wat, /array\.new_fixed \$hd\.list/);
});

test("nested closures propagate grandparent captures through GC environments", async () => {
  const source = conformance("runtime/valid/nested-closure-captures");
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /\(type \$env0 \(struct/);
  assert.match(compilation.wat, /\(type \$env1 \(struct/);
  assert.match(compilation.wat, /struct\.get \$env0 \$env0f0/);
});

test("closures store captured provider values in GC environments", async () => {
  const source = `trait Clock
trait Backup < Clock:
    fn label(self) -> i32

fn make_reader() -> (fn() -> i32) $ Backup:
    $.with(Clock=$.use(Backup)):
        clock := $.use(Clock)
        fn() -> i32:
            _ := clock
            42

fn main() -> i32 $ Backup:
    reader := make_reader()
    reader()
`;
  const compilation = await compileToWasm(source);
  assert.match(
    compilation.wat,
    /\(type \$env0 \(struct\n\s*\(field \$env0f0 \(ref null \$trait0\)\)/,
  );
});

test("requirement-bearing closures lower provider parameters", async () => {
  const source = `trait Clock

fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock:
    reader := fn() -> i32 $ Clock:
        _ := $.use(Clock)
        42
    invoke(reader)
`;
  const compilation = await compileToWasm(source);
  assert.match(
    compilation.wat,
    /type \$sig0 \(func \(param anyref\) \(param \(ref null \$trait0\)\)/,
  );
  assert.match(compilation.wat, /call_ref \$sig0/);
});

test("requirement-bearing function values lower provider parameters", async () => {
  const source = `trait Clock

fn read() -> i32 $ Clock:
    _ := $.use(Clock)
    42

fn invoke(callback: fn() -> i32 $ Clock) -> i32 $ Clock:
    callback()

fn main() -> i32 $ Clock: invoke(read)
`;
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /func \$fv0[^]*param \$provider0 \(ref null \$trait0\)/);
});

test("closure HIR records inferred unsatisfied requirements", () => {
  const source = conformance("runtime/valid/closure-inferred-requirement-row");
  const analysis = analyze(source);
  assert.deepEqual(analysis.diagnostics, []);
  assert.equal(analysis.hir?.functions[0]?.locals[0]?.type, "fn()->i32$Clock");
});

test("concrete requirement rows lower hidden typed providers", async () => {
  const source = `trait Clock

fn read() -> i32 $ Clock: 40
fn middle() -> i32 $ Clock: read() + 1
fn main() -> i32 $ Clock: middle() + 1
`;
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /param \$provider0 \(ref null \$trait0\)/);
  assert.match(compilation.wat, /call \$f0 \(local\.get \$provider0\)/);
});

test("provider scopes lower hidden provider locals", async () => {
  const source = `trait Clock
trait Backup < Clock:
    fn label(self) -> i32

fn main() -> i32 $ Clock + Backup:
    _ := $.use(Clock)
    $.with(Clock=$.use(Backup)):
        _ := $.use(Clock)
        42
`;
  const compilation = await compileToWasm(source);
  assert.match(compilation.wat, /\(local \$l0 \(ref null \$trait0\)\)/);
  assert.match(compilation.wat, /\(local\.set \$l0 \(block \(result \(ref null \$trait0\)\)/);
  assert.match(compilation.wat, /\(local\.set \$tmp0 \(local\.get \$provider0\)\)/);
});

test("concrete requirement rows normalize + lists as sets", () => {
  const result = analyze(conformance("typing/valid/requirement-row-duplicate-keys"));
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(result.hir?.functions[0]?.requirements, ["Clock", "Logger"]);

  const joined = analyze(conformance("typing/valid/requirement-row-plus-list"));
  assert.deepEqual(joined.diagnostics, []);
  assert.deepEqual(joined.hir?.functions[0]?.requirements, ["Clock", "Logger"]);

  const empty = analyze(conformance("typing/valid/explicit-empty-row"));
  assert.deepEqual(empty.diagnostics, []);
});

test("provider contexts lower to GC structs and lexical call arguments", async () => {
  const source = conformance("runtime/valid/context-values-install-providers");
  const compilation = await compileToWasm(source);
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

test("owning a trait argument's outer constructor permits a foreign trait impl (TQ-2)", () => {
  const owned = analyze(
    "data Word:\n    text: string\n\nimpl Iterable[Word] for string:\n    fn iter(self) -> mut Iterator[Word]:\n        Iterator::from_fn(fn() -> Word?: .None)\n",
  );
  assert.deepEqual(
    owned.diagnostics.map((diagnostic) => diagnostic.code),
    [],
  );
  const nested = analyze(
    "data Word:\n    text: string\n\nimpl Iterable[List[Word]] for string:\n    fn iter(self) -> mut Iterator[List[Word]]:\n        Iterator::from_fn(fn() -> List[Word]?: .None)\n",
  );
  assert.deepEqual(
    nested.diagnostics.map((diagnostic) => diagnostic.code),
    ["orphan-impl"],
  );
});

test("built-in comparison dictionaries carry their supertrait dictionaries (EQ-1)", async () => {
  const source = [
    "fn rank(value: Ordering) -> i32:",
    "    match value:",
    "        .Less => 1",
    "        .Equal => 2",
    "        .Greater => 3",
    "",
    "fn order[T < Ord](left: T, right: T) -> i32: rank(left.cmp(right))",
    "",
    "fn less[T < PartialOrd](left: T, right: T) -> bool: left < right",
    "",
    "fn main() -> i32:",
    '    order(1, 2) * 100 + order(["b"], ["a"]) * 10 + (if less(1.5, 2.5): 1 else: 0)',
    "",
  ].join("\n");
  const compilation = await compileToWasm(source);
  const trait = (name: string) => compilation.hir.traits.find((item) => item.name === name)!;
  assert.deepEqual(
    trait("Ord").supertraits.map((parent) => parent.traitName),
    ["PartialOrd"],
  );
  assert.deepEqual(
    trait("PartialOrd").supertraits.map((parent) => parent.traitName),
    ["Eq"],
  );
  const printed: string[] = [];
  const { instance } = await instantiate(source, { console: (text) => printed.push(text) });
  assert.equal((instance.exports.main as CallableFunction)(), 131);
  assert.deepEqual(
    analyze(
      "fn smallest[T < Ord](value: T) -> T: value\n\nfn main() -> f64: smallest(1.5)\n",
    ).diagnostics.map((diagnostic) => diagnostic.code),
    ["unsatisfied-trait-bound"],
  );
});

test("a type parameter calls an associated function through its bound (TQ-9)", async () => {
  const compilation = await compileToWasm(conformance("runtime/valid/associated-function-calls"));
  const make = compilation.hir.functions.find((item) => item.name === "make");
  const call = make?.body[0];
  assert.equal(call?.kind, "expression");
  if (call?.kind === "expression") {
    assert.equal(call.expression.kind, "trait-call");
    if (call.expression.kind === "trait-call")
      assert.equal(call.expression.receiver.kind, "trait-bound-dictionary");
  }
});

test("a generic inherent implementation lowers to a generic function (TQ-19)", async () => {
  const source = [
    "data Box[T]:",
    "    value: T",
    "",
    "impl[T] Box[T]:",
    "    fn get(self) -> T: self.value",
    "",
    "fn through[T](box: Box[T]) -> T: box.get()",
    "",
    "fn main() -> i32: through(Box { value: 40 }) + Box { value: 3 }.get()",
    "",
  ].join("\n");
  const compilation = await compileToWasm(source);
  const get = compilation.hir.functions.find((item) => item.name.endsWith(".get"));
  assert.deepEqual(get?.genericParameters, ["T"]);
  const printed: string[] = [];
  const { instance } = await instantiate(source, { console: (text) => printed.push(text) });
  assert.equal((instance.exports.main as CallableFunction)(), 43);
});

test("a marker implementation's bounds are proven (TQ-20)", () => {
  const source = [
    "trait Marker",
    "",
    "data Box[T]:",
    "    value: T",
    "",
    "impl Marker for i32",
    "",
    "impl[T < Marker] Marker for Box[T]",
    "",
    "fn need[T < Marker](value: T) -> void:",
    "    pass",
    "",
    "fn good(value: Box[i32]) -> void: need(value)",
    "",
    "fn bad(value: Box[string]) -> void: need(value)",
    "",
  ].join("\n");
  assert.deepEqual(
    analyze(source).diagnostics.map((diagnostic) => [diagnostic.code, diagnostic.span.start.line]),
    [["unsatisfied-trait-bound", 15]],
  );
});

test("i64 literals, explicit widening, checked arithmetic, and narrowing (F-253)", async () => {
  const source = [
    "fn wide(small: i32) -> i64:",
    "    let big: i64 = 3000000000",
    "    big * 2 + i64(small)",
    "",
    "fn main() -> bool: wide(1) == 6000000001",
    "",
  ].join("\n");
  const printed: string[] = [];
  const { instance } = await instantiate(source, { console: (text) => printed.push(text) });
  assert.equal((instance.exports.main as CallableFunction)(), 1);
  assert.deepEqual(
    analyze("fn narrow(value: i64) -> i32: value\n").diagnostics.map((item) => item.code),
    ["implicit-narrowing"],
  );
  // No implicit widening (04-type-system.md#r-types.num.no-implicit): the
  // fix-it writes the conversion around the narrower value.
  const widened = "fn widen(value: i32) -> i64: value\n";
  const [returned] = analyze(widened).diagnostics;
  assert.equal(returned?.code, "type-mismatch");
  assert.deepEqual(
    returned?.fix?.edits.map((edit) => [edit.span.start.offset, edit.replacement]),
    [
      [widened.indexOf("value\n"), "i64("],
      [widened.indexOf("value\n") + "value".length, ")"],
    ],
  );
  const mixed = "fn add(small: i16, large: i64) -> i64: small + large\n";
  const [operand] = analyze(mixed).diagnostics;
  assert.equal(operand?.code, "type-mismatch");
  assert.equal(operand?.fix?.edits[0]?.span.start.offset, mixed.indexOf("small +"));
  assert.equal(operand?.fix?.edits[0]?.replacement, "i64(");
  assert.deepEqual(
    analyze("fn huge() -> i64: 9223372036854775808\n").diagnostics.map((item) => item.code),
    ["integer-literal-range"],
  );
});

test("println drives write_line! on a program-defined Console (MHP-1)", async () => {
  const source = [
    "data Buffer:",
    "    lines: mut List[string]",
    "",
    "impl Console for Buffer:",
    "    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]:",
    "        self.lines.append(text)",
    "        .Ok(())",
    "",
    'let recorded: string = ""',
    "",
    "pub fn main() -> void $ Console:",
    "    let buffer: mut Buffer = Buffer { lines: [] }",
    "    $.with(Console=buffer):",
    '        println("one")',
    "        println(2)",
    '    recorded = "${buffer.lines[0]} ${buffer.lines[1]}"',
    "",
    "pub fn recorded_lines() -> i32: recorded.len()",
    "",
  ].join("\n");
  assert.deepEqual(analyze(source).diagnostics, []);
  const printed: string[] = [];
  const { instance } = await instantiate(source, { console: (text) => printed.push(text) });
  (instance.exports.main as CallableFunction)({});
  assert.equal((instance.exports.recorded_lines as CallableFunction)(), "one 2".length);
  assert.deepEqual(printed, []);
});

test("println drives a write_line! pending on a host operation until it finishes (MHP-1)", async () => {
  const source = [
    "pub trait Gate:",
    "    fn wait!(self) -> void",
    "",
    "data GatedConsole:",
    "    gate: Gate",
    "",
    "impl Console for GatedConsole:",
    "    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]:",
    "        self.gate.wait!()",
    "        .Ok(())",
    "",
    "let written: i32 = 0",
    "",
    "pub fn main() -> void $ Console + Gate:",
    "    let gated: mut GatedConsole = GatedConsole { gate: $.use(Gate) }",
    "    $.with(Console=gated):",
    '        println("held")',
    "    written = written + 1",
    "",
    "pub fn written_lines() -> i32: written",
    "",
  ].join("\n");
  assert.deepEqual(analyze(source, { hostCapabilities: ["Gate"] }).diagnostics, []);
  // The gate stays pending for two polls; `block_on` keeps driving the write.
  let polls = 0;
  const { instance } = await instantiate(source, {
    hostCapabilities: ["Gate"],
    hostSuspensionPending: () => {
      polls += 1;
      return polls <= 2;
    },
  });
  (instance.exports.main as CallableFunction)({}, { name: "gate" });
  assert.equal(polls, 3);
  assert.equal((instance.exports.written_lines as CallableFunction)(), 1);
});

test("Debug is checked, and derived builders render debug text (T33, T53)", async () => {
  const source =
    "@derive(Debug)\ndata P:\n    x: i32\n\npub fn main() -> void $ Console: println(debug(P { x: 1 }))\n";
  assert.deepEqual(analyze(source).diagnostics, []);
  const printed: string[] = [];
  const { instance } = await instantiate(source, { console: (text) => printed.push(text) });
  (instance.exports.main as CallableFunction)({});
  assert.deepEqual(printed, ["P { x: 1 }"]);
  assert.deepEqual(
    analyze("data Q:\n    x: i32\n\nfn show(q: Q) -> string: debug(q)\n").diagnostics.map(
      (item) => item.code,
    ),
    ["unsatisfied-trait-bound"],
  );
});

test("@derive(Debug) picks Rust's builder per data type and variant (T54, Open Issues item 8)", async () => {
  const source = [
    "@derive(Debug)",
    "data Unit: pass",
    "",
    "@derive(Debug)",
    "enum Shape:",
    "    Empty",
    "    Circle(i32)",
    "    Rect(width: i32, height: i32)",
    "    Mixed(i32, label: string)",
    "",
    "pub fn main() -> void $ Console:",
    "    println(debug(Unit {}))",
    "    println(debug(Shape.Empty))",
    "    println(debug(Shape.Circle(3)))",
    "    println(debug(Shape.Rect(width=4, height=5)))",
    '    println(debug(Shape.Mixed(6, label="m")))',
    "",
  ].join("\n");
  const printed: string[] = [];
  const { instance } = await instantiate(source, { console: (text) => printed.push(text) });
  (instance.exports.main as CallableFunction)({});
  assert.deepEqual(printed, [
    "Unit",
    "Empty",
    "Circle(3)",
    "Rect { width: 4, height: 5 }",
    'Mixed { _0: 6, label: "m" }',
  ]);
});

test("u8 checked arithmetic and ExitCode entry results (T8)", async () => {
  const add = "fn add(a: u8, b: u8) -> u8: a + b\n\nfn main() -> u8: add(200, 55)\n";
  const { instance } = await instantiate(add);
  assert.equal((instance.exports.main as CallableFunction)(), 255);
  const overflow = await instantiate(add.replace("55", "56"));
  assert.throws(() => (overflow.instance.exports.main as CallableFunction)());
  const exit = [
    "use std.process.ExitCode",
    "",
    "pub fn main() -> Result[ExitCode, string]:",
    "    .Ok(ExitCode(7))",
    "",
  ].join("\n");
  const entry = await instantiate(exit);
  assert.equal((entry.instance.exports.main as CallableFunction)(), 7);
  const failed = await instantiate(exit.replace(".Ok(ExitCode(7))", '.Err("no")'));
  assert.equal((failed.instance.exports.main as CallableFunction)(), -1);
});
