// Inventory of test/fixtures/**/*.hd and test/*.test.ts (audit step 1.6).
// Writes audit/evidence/01-test-quality/inventory.tsv and selfcontain.tsv.
// Usage: node --experimental-strip-types audit/scripts/test-quality/inventory.ts
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { type FixtureInfo, header, readFixtures, root, specCodes, tsv } from "./lib.ts";

const out = resolve(root, "audit/evidence/01-test-quality");

// Path rules -> primary spec section. First match wins. Assigned by reading
// each fixture (see SUMMARY.md); the rule table is the record of that reading.
const sectionRules: ReadonlyArray<readonly [RegExp, string]> = [
  [/^compiler\/00-/, "06-control-flow.md#conditional-expressions"],
  [/^compiler\/04-.*-diagnostic/, "01-lexical-structure.md#integer-literals"],
  [/^compiler\/0[3-6]-/, "05-expressions.md#unary-and-binary-operators"],
  [/^compiler\/07-.*-diagnostic\.hd$/, "03-names-and-scopes.md#lexical-scopes"],
  [/^compiler\/07-.*-diagnostic-2/, "03-names-and-scopes.md#local-bindings"],
  [/^compiler\/07-.*-diagnostic-3/, "04-type-system.md#assignability-and-coercion"],
  [/^compiler\/07-.*-diagnostic-[45]/, "05-expressions.md#unary-and-binary-operators"],
  [/^compiler\/07-/, "07-functions.md#varargs"],
  [/^compiler\/(08|09|12)-/, "07-functions.md#varargs"],
  [/^compiler\/1[01]-/, "07-functions.md#default-values"],
  [/^compiler\/(13|18)-/, "07-functions.md#positional-and-named-arguments"],
  [/^compiler\/14-/, "08-data-and-enums.md#enum-declarations"],
  [/^compiler\/1[56]-/, "08-data-and-enums.md#matching-enums"],
  [/^compiler\/17-/, "08-data-and-enums.md#shared-enum-constructor-data"],
  [/^compiler\/19-/, "10-modules.md#prelude"],
  [/^compiler\/20-/, "03-names-and-scopes.md#control-flow-binding-scopes"],
  [/^compiler\/21-/, "06-control-flow.md#conditional-expressions"],
  [/^compiler\/22-/, "06-control-flow.md#while-loops"],
  [/^compiler\/23-/, "06-control-flow.md#break-continue-and-loop-else"],
  [/^compiler\/24-/, "06-control-flow.md#for-loops"],
  [/^compiler\/2[67]-/, "04-type-system.md#mutable-paths"],
  [/^compiler\/2[89]-/, "04-type-system.md#composite-values-and-access-permission"],
  [/^compiler\/(25|30|31|32|33|34|35)-/, "08-data-and-enums.md#data-declarations"],
  [/^compiler\/36-/, "08-data-and-enums.md#construction-and-access"],
  [/^compiler\/37-/, "06-control-flow.md#match-expressions"],
  [/^compiler\/(38|40|41|65)-|^compiler\/62-string/, "04-type-system.md#strings"],
  [/^compiler\/(39|42)-|^compiler\/display-/, "01-lexical-structure.md#string-and-character-literals"],
  [/^compiler\/46-/, "06-control-flow.md#runtime-panics"],
  [/^compiler\/4[3-5]-/, "06-control-flow.md#deferred-cleanup"],
  [/^compiler\/(47|49|50)-/, "08-data-and-enums.md#matching-enums"],
  [/^compiler\/(48|51|52|53)-/, "06-control-flow.md#match-expressions"],
  [/^compiler\/(54|58)-/, "04-type-system.md#optional-types"],
  [/^compiler\/55-/, "04-type-system.md#result-types"],
  [/^compiler\/5[67]-/, "10-modules.md#use-forms"],
  [/^compiler\/59-/, "06-control-flow.md#blocks-and-completion"],
  [/^compiler\/60-comprehensions/, "05-expressions.md#comprehensions"],
  [/^compiler\/61-binding/, "03-names-and-scopes.md#binding-expressions"],
  [/^compiler\/6[0-2]-/, "07-functions.md#closures"],
  [/^compiler\/63-generic-iterator/, "06-control-flow.md#for-loops"],
  [/^compiler\/6[34]-(explicitly|named)/, "07-functions.md#recursion"],
  [/^compiler\/64-generic-methods/, "07-functions.md#generic-functions"],
  [/^compiler\/66-/, "09-traits.md#inherent-implementations"],
  [/^compiler\/(67|68|76|78)-/, "09-traits.md#trait-implementations"],
  [/^compiler\/(69|70)-/, "09-traits.md#method-resolution"],
  [/^compiler\/(71|72|73|77)-/, "09-traits.md#trait-declarations"],
  [/^compiler\/(74|75)-/, "09-traits.md#generic-bounds-and-static-dispatch"],
  [/^compiler\/assert-/, "10-modules.md#standard-testing"],
  [/^compiler-types\/callbacks\/trailing/, "07-functions.md#trailing-callback-blocks"],
  [/^compiler-types\/callbacks\/requirement/, "11-requirements-and-suspension.md#requirement-polymorphism"],
  [/^compiler-types\/callbacks\//, "07-functions.md#function-types-and-values"],
  [/^compiler-types\/closures\/(captures|nested)/, "07-functions.md#captures"],
  [/^compiler-types\/closures\//, "11-requirements-and-suspension.md#construction-time-requirement-binding"],
  [/^compiler-types\/collections\/maps\/float-key/, "04-type-system.md#map-key-types"],
  [/^compiler-types\/collections\/trait-lists/, "09-traits.md#dynamic-trait-values"],
  [/^compiler-types\/collections\//, "05-expressions.md#list-and-map-expressions"],
  [/^compiler-types\/data\//, "04-type-system.md#generics"],
  [/^compiler-types\/enums\//, "08-data-and-enums.md#generic-and-recursive-enums"],
  [/^compiler-types\/generics\//, "07-functions.md#generic-functions"],
  [/^compiler-types\/identity\//, "05-expressions.md#unary-and-binary-operators"],
  [/^compiler-types\/requirements\/(context|contexts|provider-scopes)/, "11-requirements-and-suspension.md#provider-scopes"],
  [/^compiler-types\/requirements\/provider-errors/, "11-requirements-and-suspension.md#provider-access"],
  [/^compiler-types\/requirements\/(generic-provider|row-|abi)/, "11-requirements-and-suspension.md#requirement-polymorphism"],
  [/^compiler-types\/requirements\//, "11-requirements-and-suspension.md#requirement-rows"],
  [/^compiler-types\/traits\/(generic-bounds|multiple-bounds)/, "09-traits.md#generic-bounds-and-static-dispatch"],
  [/^compiler-types\/traits\/(defaults|diagnostics|mutable-errors)/, "09-traits.md#trait-implementations"],
  [/^compiler-types\/traits\/embedded/, "09-traits.md#embedding-and-trait-satisfaction"],
  [/^compiler-types\/traits\/inherent/, "09-traits.md#inherent-implementations"],
  [/^compiler-types\/traits\/providers/, "11-requirements-and-suspension.md#provider-scopes"],
  [/^compiler-types\/traits\/suspending/, "11-requirements-and-suspension.md#suspending-functions"],
  [/^compiler-types\/traits\//, "09-traits.md#dynamic-trait-values"],
  [/^compiler-types\/tuples\//, "05-expressions.md#parenthesized-and-tuple-expressions"],
  [/^frontend\/(13|00)-/, "01-lexical-structure.md#whitespace-and-indentation"],
  [/^frontend\/14-/, "01-lexical-structure.md#literals"],
  [/^frontend\/15-/, "01-lexical-structure.md#string-and-character-literals"],
  [/^frontend\/27-/, "01-lexical-structure.md#comments"],
  [/^frontend\/(16|25|26)-/, "02-grammar.md#expressions"],
  [/^frontend\/(17|18)-/, "02-grammar.md#calls-and-arguments"],
  [/^frontend\/(04|05)-/, "02-grammar.md#control-flow-expressions"],
  [/^frontend\/(21|22|23)-/, "02-grammar.md#patterns"],
  [/^frontend\/29-/, "02-grammar.md#test-blocks"],
  [/^frontend\/30-/, "02-grammar.md#requirements-and-provider-contexts"],
  [/^frontend\/31-/, "02-grammar.md#use-declarations"],
  [/^frontend\/(08|09)-/, "02-grammar.md#traits-and-implementations"],
  [/^frontend\/(24)-/, "02-grammar.md#enums"],
  [/^frontend\/(10|12|20)-/, "02-grammar.md#data-types"],
  [/^frontend\/(07)-/, "02-grammar.md#types"],
  [/^frontend\//, "02-grammar.md#declarations"],
  [/^requirements\//, "11-requirements-and-suspension.md#requirement-rows"],
  [/^suspension\/27-/, "10-modules.md#prelude"],
  [/^suspension\/28-/, "11-requirements-and-suspension.md#provider-access"],
  [/^suspension\/24-/, "11-requirements-and-suspension.md#provider-scopes"],
  [/^suspension\/29-/, "11-requirements-and-suspension.md#suspendt-protocol"],
  [/^suspension\/30-/, "10-modules.md#standard-testing"],
  [/^suspension\/31-/, "10-modules.md#module-initialization"],
  [/^suspension\/3[2356]-/, "10-modules.md#wasm-boundary"],
  [/^suspension\/09-/, "06-control-flow.md#deferred-cleanup"],
  [/^suspension\/(07|08|14|15|23)-/, "11-requirements-and-suspension.md#cancellation"],
  [/^suspension\/(06|10|11|12|26)-/, "11-requirements-and-suspension.md#suspendt-protocol"],
  [/^suspension\/05-/, "12-variadic-generics.md#all-motivation"],
  [/^suspension\//, "11-requirements-and-suspension.md#suspending-functions"],
];

// Fixtures whose stated claim is an implementation mechanism exercised only by a
// TypeScript test, and whose portable oracle is trivial or unrelated to the claim.
const implementationFixtures = new Map<string, string>([
  ["suspension/10-", "claim is trace ABI event order; portable oracle is main = 42 only"],
  ["suspension/11-", "claim is host pending across polls; body is `fn main!() -> i32: 42`"],
  ["suspension/12-", "claim is driver misuse; body is `fn main!() -> i32: 42`; TS drives __hd_* exports"],
  ["suspension/26-replay-rejects", "claim is replay rejection on code change; TS-only"],
  ["suspension/26-replay-survives", "claim is replay stability; TS-only"],
  ["suspension/26-suspension-poll", "claim is record/replay configuration identity; TS-only"],
  ["suspension/32-", "claim is record/replay; needs implementation-only profile ready-gate"],
  ["suspension/33-", "needs implementation-only profile ready-counter"],
  ["suspension/35-", "claim is f64 replay bit encoding; needs implementation-only profile ready-float"],
  ["suspension/36-", "claim is UTF-8 replay encoding; needs implementation-only profile ready-text"],
  ["requirements/", "claim is explain-requirements tool output; portable oracle is accept only"],
  ["frontend/00-", "input for lexer token/AST snapshot tests; portable oracle is parse only"],
]);

const implName = /\b(wasm|gc|abi|erased?|erasure|boxing|box(es)?|lower(s|ing)?|dictionar(y|ies)|cfg|frame|carrier|materialize|byte bridge|host ieee|calling convention|trace|replay|layout|storage|typed function references)\b/i;

function sectionOf(path: string): string {
  for (const [pattern, section] of sectionRules) if (pattern.test(path)) return section;
  return "UNMAPPED";
}

function expectationOf(fixture: FixtureInfo): string {
  if (fixture.markers.length)
    return fixture.markers.map((m) => `${m.kind === "diagnostic" ? "reject" : m.kind === "warning" ? "warn" : "panic"}:${m.code}@${m.line}`).join(",");
  if (fixture.results.length) return `result:${fixture.results.map((r) => `${r.entry}=${r.value}`).join(",")}`;
  return `expect:${fixture.expects.join(",")}`;
}

const parseCodes = new Set([
  "expected-token", "inconsistent-dedent", "tab-whitespace", "invalid-integer-literal",
  "invalid-string-interpolation", "reserved-semicolon", "comparison-chaining", "argument-order",
  "nonfinal-positional-spread", "vararg-default", "data-spread-position", "doc-comment-without-target",
]);

function targetDirectory(fixture: FixtureInfo): string {
  const marker = fixture.markers[0];
  if (marker?.kind === "panic") return "runtime/panic";
  if (marker?.kind === "warning") return "typing/warnings";
  if (marker) return parseCodes.has(marker.code) && fixture.path.startsWith("frontend/") ? "parse/invalid" : "typing/invalid";
  if (fixture.results.length || fixture.expects.includes("test")) return "runtime/valid";
  if (fixture.expects.includes("parse")) return "parse/valid";
  return "typing/valid";
}

function tsConsumers(): Map<string, string[]> {
  const consumers = new Map<string, string[]>();
  for (const file of readdirSync(resolve(root, "test")).filter((name) => name.endsWith(".test.ts"))) {
    const text = readFileSync(resolve(root, "test", file), "utf8");
    for (const match of text.matchAll(/fixture(?:Body)?\(\s*"([^"]+)"/g)) {
      const list = consumers.get(`${match[1]}.hd`) ?? [];
      if (!list.includes(file)) list.push(file);
      consumers.set(`${match[1]}.hd`, list);
    }
  }
  return consumers;
}

// Self-containment and determinism flags.
function flags(fixture: FixtureInfo, codes: Set<string>): string[] {
  const result: string[] = [];
  const text = fixture.lines.filter((line) => !line.startsWith("#")).join("\n");
  for (const marker of fixture.markers)
    if (!codes.has(marker.code)) result.push(`nonspec-code:${marker.code}`);
  if (fixture.profile && !["disposed-file", "pending-gate"].includes(fixture.profile))
    result.push(`impl-only-profile:${fixture.profile}`);
  const keys = new Set<string>();
  for (const match of text.matchAll(/\$\s*\(?([A-Z][A-Za-z0-9]*(?:\s*[+-]\s*[A-Z][A-Za-z0-9]*)*)/g))
    for (const key of match[1]!.split(/\s*[+-]\s*/)) keys.add(key.trim());
  for (const match of text.matchAll(/\$\.(?:use|with|context)\(([^)]*)\)/g))
    for (const key of match[1]!.matchAll(/\b([A-Z][A-Za-z0-9]*)(?=\s*[,)=]|$)/g)) keys.add(key[1]!);
  const declared = new Set([...text.matchAll(/^(?:pub\s+)?(?:trait|data|enum)\s+([A-Z][A-Za-z0-9]*)/gm)].map((m) => m[1]!));
  const prelude = new Set(["Console", "Context"]);
  const undeclared = [...keys].filter((key) => !declared.has(key) && !prelude.has(key) && key !== "Self" && key.length > 1);
  if (undeclared.length && !fixture.expects.includes("parse"))
    result.push(`undeclared-requirement-key:${undeclared.sort().join("+")}`);
  for (const { entry, value } of fixture.results) {
    const signature = new RegExp(`^(pub\\s+)?fn ${entry}!?\\(\\)\\s*->\\s*([^:$]+?)\\s*(\\$[^:]*)?:`, "m").exec(text);
    const resultType = signature?.[2]?.trim() ?? "?";
    if (/^f(32|64)$/.test(resultType)) result.push(`float-result-host-format:${entry}=${value}`);
    if (resultType === "char") result.push(`char-result-as-integer:${entry}=${value}`);
    if (resultType === "bool") result.push(`bool-result-as-integer:${entry}=${value}`);
    if (signature?.[3] && signature[3].trim() !== "$()") result.push(`entry-with-requirement-row:${entry}`);
    if (!signature?.[1] && entry === "main" && resultType !== "void") result.push("nonentry-main-result");
    if (entry !== "main") result.push(`named-entry-hook:${entry}`);
  }
  if (/\bfor\s+\w+\s*,\s*\w+\s+in\b|\bfor\s+\w+\s+in\s+\{/.test(text)) result.push("map-iteration(order-specified)");
  if (fixture.markers.some((m) => m.kind === "panic" && m.code === "runtime-error")) result.push("unchecked-panic-code:runtime-error");
  if (/^use std\.task|block_on/m.test(text)) result.push("std.task");
  if (/^use std\.resource/m.test(text)) result.push("std.resource");
  return result;
}

interface TsTest {
  readonly file: string;
  readonly line: number;
  readonly name: string;
  readonly body: string;
}

function tsTests(): TsTest[] {
  const tests: TsTest[] = [];
  for (const file of readdirSync(resolve(root, "test")).filter((name) => name.endsWith(".test.ts")).sort()) {
    const lines = readFileSync(resolve(root, "test", file), "utf8").split("\n");
    const starts: Array<{ line: number; name: string }> = [];
    for (const [index, line] of lines.entries()) {
      const match = /^test\(\s*$|^test\("([^"]+)"/.exec(line);
      if (match) {
        const name = match[1] ?? /"([^"]+)"/.exec(lines[index + 1] ?? "")?.[1] ?? "?";
        starts.push({ line: index + 1, name });
      }
    }
    for (const [index, start] of starts.entries()) {
      const end = starts[index + 1]?.line ?? lines.length + 1;
      tests.push({ body: lines.slice(start.line - 1, end - 1).join("\n"), file, line: start.line, name: start.name });
    }
  }
  return tests;
}

const implChecks: ReadonlyArray<readonly [string, RegExp]> = [
  ["wat", /\.wat\b/],
  ["hir", /\.hir\b|hir!?\)|dump-hir/],
  ["wasm-binary", /WebAssembly\.validate|\.bytes\b/],
  ["lexer/ast", /\blex\(|\bparse\(|\.program\b|snapshot|tokens/],
  ["trace", /trace/],
  ["record/replay", /replay|record/i],
  ["runtime-abi-exports", /__hd_|exports\.\$|hostSuspension|providerConfigurationId|pending:/],
  ["explain-requirements", /explainRequirements|explain-requirements/],
  ["cli", /execFile|bin\/hd\.js/],
  ["toolchain", /binaryen|toolchain|gateModule/i],
];
const languageChecks: ReadonlyArray<readonly [string, RegExp]> = [
  ["result-value", /exports\.[A-Za-z_]+ as CallableFunction\)\([^)]*\),\s*-?[\d"']/],
  ["result-value", /assert\.(equal|deepEqual)\(\s*\(?\s*(await\s+)?\(?[a-z.]*instance\.exports/],
  ["trap", /assert\.throws/],
  ["diagnostic-code", /diagnostics(\[0\])?\??\.?(\[0\])?\??\.code|\.code\)|code:\s*"/],
];

function main(): void {
  const codes = specCodes();
  const consumers = tsConsumers();
  const fixtures = readFixtures();
  const rows: unknown[][] = [
    ["body", "case", "name", "expectation", "class", "promotion_target", "ts_consumers", "notes", "basis"],
  ];
  const self: unknown[][] = [["path", "flags"]];
  for (const fixture of fixtures) {
    const implEntry = [...implementationFixtures].find(([prefix]) => fixture.path.startsWith(prefix));
    const fixtureFlags = flags(fixture, codes);
    const cls = implEntry ? "implementation detail" : "language behavior";
    const notes: string[] = [];
    if (implEntry) notes.push(implEntry[1]);
    if (!implEntry && implName.test(fixture.name)) notes.push("name states an implementation mechanism");
    if (fixture.markers.length > 1) notes.push(`${fixture.markers.length} markers`);
    const target = implEntry ? "none (keep as TS input; drop from portable run)" : `${sectionOf(fixture.path)} -> spec/conformance/${targetDirectory(fixture)}`;
    rows.push([
      "test/fixtures", fixture.path, fixture.name, expectationOf(fixture), cls, target,
      (consumers.get(fixture.path) ?? []).join(","), notes.join("; "),
      implEntry ? "manual" : "manual-read; section via path rule",
    ]);
    self.push([fixture.path, fixtureFlags.join(" ")]);
  }
  for (const test of tsTests()) {
    const impl = implChecks.filter(([, pattern]) => pattern.test(test.body)).map(([label]) => label);
    const lang = [...new Set(languageChecks.filter(([, pattern]) => pattern.test(test.body)).map(([label]) => label))];
    const used = [...test.body.matchAll(/fixture(?:Body)?\(\s*"([^"]+)"/g)].map((m) => m[1]!);
    const cls = impl.length ? "implementation detail" : "language behavior";
    const target = impl.length
      ? lang.length ? "language part already expressed by its fixture(s)" : "none"
      : `redundant with its portable fixture(s); promote those to ${[...new Set(used.map((path) => sectionOf(`${path}.hd`)))].join(",") || "?"}`;
    rows.push([
      "test/*.test.ts", `${test.file}:${test.line}`, test.name,
      `impl=[${impl.join(",")}] lang=[${lang.join(",")}]`, cls, target, used.join(","),
      impl.length && lang.length ? "mixed: implementation and language assertions" : "",
      "heuristic: assertion regex",
    ]);
  }
  writeFileSync(resolve(out, "inventory.tsv"), `${header("node --experimental-strip-types audit/scripts/test-quality/inventory.ts")}\n${tsv(rows)}\n`);
  writeFileSync(resolve(out, "selfcontain.tsv"), `${header("node --experimental-strip-types audit/scripts/test-quality/inventory.ts")}\n${tsv(self)}\n`);
  const unmapped = rows.filter((row) => String(row[5]).startsWith("UNMAPPED"));
  console.log(`fixtures=${fixtures.length} ts=${rows.length - 1 - fixtures.length} unmapped=${unmapped.length}`);
}

main();
