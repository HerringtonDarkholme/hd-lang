// Read-only compiler probes using unchanged, existing hd fixtures.
// Run from any directory with Node's --experimental-strip-types flag.
import { readFileSync } from "node:fs";
import { analyze, compileToWat } from "../../src/compiler.ts";
import { parse } from "../../src/parser/index.ts";
import { DEFAULTED_SPANS } from "../../src/checker/literal-join.ts";
import { derivedFieldDiagnostic, derivedFieldSpan } from "../../src/checker/derive-intrinsics.ts";
import { sourceSpanKey } from "../../src/diagnostics.ts";
import { emissionReachability } from "../../src/emitter/reachability.ts";
import { buildSuspensionPlan, needsSuspensionCfg } from "../../src/emitter/suspension.ts";

const fixtureSource = (fixture) =>
  readFileSync(new URL(`../../spec/conformance/${fixture}`, import.meta.url), "utf8");

const literalFixture = "typing/valid/literal-first-use-fallback.hd";
const source = fixtureSource(literalFixture);
const retained = [DEFAULTED_SPANS.size];
const diagnostics = [];
for (let index = 0; index < 6; index++) {
  const result = analyze(source);
  retained.push(DEFAULTED_SPANS.size);
  diagnostics.push(result.diagnostics.map((diagnostic) => diagnostic.code));
}
console.log(
  JSON.stringify({
    probe: "retained-literal-spans",
    fixture: literalFixture,
    retained,
    diagnostics,
  }),
);

// This isolates key reuse in the diagnostic helper. It is deliberately not
// presented as an end-to-end wrong-program diagnostic reproduction.
const parsed = parse(fixtureSource("typing/valid/derived-equality.hd"));
if (!parsed.program) throw new Error("existing derivation fixture did not parse");
const field = parsed.program.data[0].fields[0];
const sameCoordinates = { start: { ...field.span.start }, end: { ...field.span.end } };
const before = derivedFieldDiagnostic(
  "unsatisfied-trait-bound",
  "unrelated bound failure",
  sameCoordinates,
);
derivedFieldSpan(field, "Eq", "AuditOrigin");
const after = derivedFieldDiagnostic(
  "unsatisfied-trait-bound",
  "unrelated bound failure",
  sameCoordinates,
);
console.log(
  JSON.stringify({
    probe: "origin-key-reuse",
    key: sourceSpanKey(sameCoordinates),
    before,
    after,
    evidence: "helper-level only",
  }),
);

for (const fixture of [
  "runtime/valid/defer-order.hd",
  "runtime/valid/closure-mutable-capture-runtime.hd",
  "typing/valid/requirements-and-suspension.hd",
]) {
  const result = compileToWat(fixtureSource(fixture));
  const reachable = emissionReachability(result.hir).program;
  const userFunctions = [...result.hir.functions, ...result.hir.closures].filter(
    (declaration) => !declaration.standard,
  );
  const kinds = {};
  const visit = (value) => {
    if (!value || typeof value !== "object") return;
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    if (typeof value.kind === "string") kinds[value.kind] = (kinds[value.kind] ?? 0) + 1;
    for (const [key, child] of Object.entries(value)) if (key !== "span") visit(child);
  };
  userFunctions.forEach((declaration) => visit(declaration.body));
  const suspensionPlans = userFunctions
    .filter((declaration) => declaration.suspending && needsSuspensionCfg(declaration))
    .map((declaration) => {
      const plan = buildSuspensionPlan(declaration);
      return {
        name: declaration.name,
        blocks: plan.blocks.length,
        sites: plan.sites.length,
        temporaries: plan.temporaries.length,
      };
    });
  console.log(
    JSON.stringify({
      probe: "pipeline-trace",
      fixture,
      functions: result.hir.functions.length,
      closures: result.hir.closures.length,
      globals: result.hir.globals.length,
      reachableFunctions: reachable.functions.length,
      reachableClosures: reachable.closures.length,
      storageAndCleanupNodes: Object.fromEntries(
        ["cell-new", "cell-get", "cell-set", "defer"].map((kind) => [kind, kinds[kind] ?? 0]),
      ),
      suspensionPlans,
      watCharacters: result.wat.length,
      diagnostics: result.diagnostics.map((diagnostic) => diagnostic.code),
    }),
  );
}
