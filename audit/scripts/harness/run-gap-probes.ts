// Runs the phase 1.4 gap probes in audit/probes/gaps through the hd CLI and
// prints a TSV of expected versus observed outcomes.
// Usage: node --experimental-strip-types audit/scripts/harness/run-gap-probes.ts
import { spawn } from "node:child_process";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");

type Expectation =
  | { readonly kind: "accept" }
  | { readonly kind: "pass" }
  | { readonly kind: "fail" }
  | { readonly kind: "reject"; readonly code: string; readonly line: number }
  | { readonly kind: "panic"; readonly code: string };

interface Probe {
  readonly id: string;
  readonly file: string;
  readonly mechanism: string;
  readonly spec: string;
  readonly command: readonly string[];
  readonly expect: Expectation;
  readonly coverage: string;
}

const scenario = ["--scenario", "cancellation-cleanup", "--pending-function", "wait"];

const readonlyCoverage =
  "typing/invalid/readonly-mutation,readonly-mut-field-*,readonly-list-element-replacement,readonly-map-entry-replacement,missing-mutable-edge (selected)";
const deferCoverage =
  "runtime/valid/cancellation-runs-defer,cancellation-unwinds-nested-frames,cancellation-unwinds-suspending-closure (selected; single defer each)";
const suspensionCoverage =
  "runtime/panic/second-suspension-drive (selected); fixtures suspension/06-*seconddrive,08-cancelled-*";
const rowCoverage =
  "typing/valid/row-subtraction-entailment,typing/invalid/row-subtraction-unsound,typing/warnings/requirement-subtract-absent (selected); fixtures requirements/row-subtraction/*";
const nanCoverage = "none: no fixture orders f64 at runtime; float-display covers NaN display only";
const iteratorCoverage =
  "runtime/panic/invalidated-iterator,for-loop-iterator-invalidated,map-iterator-invalidated,removed-map-iterator-invalidated (selected)";

const probes: readonly Probe[] = [
  {
    id: "G01a",
    file: "g01a-readonly-closure-capture.hd",
    mechanism: "readonly write inside closure",
    spec: "04-type-system.md#mutable-paths",
    command: ["check"],
    expect: { kind: "reject", code: "readonly-root", line: 7 },
    coverage: readonlyCoverage,
  },
  {
    id: "G01b",
    file: "g01b-readonly-loop-element.hd",
    mechanism: "readonly list element via for",
    spec: "04-type-system.md#mutable-paths",
    command: ["check"],
    expect: { kind: "reject", code: "readonly-root", line: 7 },
    coverage: readonlyCoverage,
  },
  {
    id: "G01c",
    file: "g01c-control-mut-loop-element.hd",
    mechanism: "control: list[mut User] element write accepted",
    spec: "04-type-system.md#mutable-paths",
    command: ["check"],
    expect: { kind: "accept" },
    coverage: readonlyCoverage,
  },
  {
    id: "G02",
    file: "g02-argument-evaluation-order.hd",
    mechanism: "argument and operand evaluation order",
    spec: "05-expressions.md#evaluation-order; 05-expressions.md#calls",
    command: ["test"],
    expect: { kind: "pass" },
    coverage:
      "runtime/valid/assert (named args, selected); fixtures compiler/13-*,14-*,30-* (named args, fields); callee-first: none found",
  },
  {
    id: "G03a",
    file: "g03a-defer-on-cancellation.hd",
    mechanism: "defer runs LIFO innermost-first on cancellation",
    spec: "11-requirements-and-suspension.md#cancellation",
    command: ["test", ...scenario],
    expect: { kind: "pass" },
    coverage: deferCoverage,
  },
  {
    id: "G03b",
    file: "g03b-control-no-defer.hd",
    mechanism: "control: no defer, scenario must fail",
    spec: "11-requirements-and-suspension.md#cancellation",
    command: ["test", ...scenario],
    expect: { kind: "fail" },
    coverage: deferCoverage,
  },
  {
    id: "G04a",
    file: "g04a-second-drive-via-helper.hd",
    mechanism: "second drive of completed suspension",
    spec: "11-requirements-and-suspension.md#suspendt-protocol",
    command: ["test"],
    expect: { kind: "panic", code: "suspension-invalid-state" },
    coverage: suspensionCoverage,
  },
  {
    id: "G04b",
    file: "g04b-drive-after-cancel.hd",
    mechanism: "drive after cancel",
    spec: "11-requirements-and-suspension.md#suspendt-protocol",
    command: ["test"],
    expect: { kind: "panic", code: "suspension-invalid-state" },
    coverage: suspensionCoverage,
  },
  {
    id: "G04c",
    file: "g04c-control-cancel-after-complete.hd",
    mechanism: "control: cancel after completion is a no-op",
    spec: "11-requirements-and-suspension.md#suspendt-protocol",
    command: ["test"],
    expect: { kind: "pass" },
    coverage: suspensionCoverage,
  },
  {
    id: "G05a",
    file: "g05a-row-subtraction.hd",
    mechanism: "row subtraction removes a requirement",
    spec: "11-requirements-and-suspension.md#requirement-rows",
    command: ["test"],
    expect: { kind: "pass" },
    coverage: rowCoverage,
  },
  {
    id: "G05b",
    file: "g05b-control-no-subtraction.hd",
    mechanism: "control: without subtraction caller needs Logger",
    spec: "11-requirements-and-suspension.md#requirement-rows",
    command: ["check"],
    expect: { kind: "reject", code: "missing-requirement", line: 18 },
    coverage: rowCoverage,
  },
  {
    id: "G05c",
    file: "g05c-subtraction-keeps-other-keys.hd",
    mechanism: "row subtraction keeps other keys",
    spec: "11-requirements-and-suspension.md#requirement-rows",
    command: ["check"],
    expect: { kind: "reject", code: "missing-requirement", line: 19 },
    coverage: rowCoverage,
  },
  {
    id: "G06a",
    file: "g06a-nan-ordering.hd",
    mechanism: "NaN makes all four relational ops false",
    spec: "05-expressions.md#unary-and-binary-operators",
    command: ["test"],
    expect: { kind: "pass" },
    coverage: nanCoverage,
  },
  {
    id: "G06b",
    file: "g06b-control-float-ordering.hd",
    mechanism: "control: 1.0 < 2.0",
    spec: "05-expressions.md#unary-and-binary-operators",
    command: ["test"],
    expect: { kind: "pass" },
    coverage: nanCoverage,
  },
  {
    id: "G06c",
    file: "g06c-nan-tuple-ordering.hd",
    mechanism: "NaN inside tuple ordering",
    spec: "05-expressions.md#unary-and-binary-operators",
    command: ["test"],
    expect: { kind: "pass" },
    coverage: nanCoverage,
  },
  {
    id: "G06d",
    file: "g06d-control-float-equality.hd",
    mechanism: "control: NaN equality",
    spec: "05-expressions.md#unary-and-binary-operators",
    command: ["test"],
    expect: { kind: "pass" },
    coverage: "none for NaN equality at runtime",
  },
  {
    id: "G07a",
    file: "g07a-iterator-invalidated-via-helper.hd",
    mechanism: "iterator version check through alias",
    spec: "06-control-flow.md#for-loops",
    command: ["test"],
    expect: { kind: "panic", code: "iterator-invalidated" },
    coverage: iteratorCoverage,
  },
  {
    id: "G07b",
    file: "g07b-control-replacement-keeps-iterator.hd",
    mechanism: "control: replacement does not invalidate",
    spec: "06-control-flow.md#for-loops",
    command: ["test"],
    expect: { kind: "pass" },
    coverage: "runtime/valid/list-iterator (selected, check content)",
  },
  {
    id: "G07c",
    file: "g07c-map-insert-during-for.hd",
    mechanism: "map insert during for invalidates",
    spec: "06-control-flow.md#for-loops",
    command: ["test"],
    expect: { kind: "panic", code: "iterator-invalidated" },
    coverage: iteratorCoverage,
  },
];

function run(args: readonly string[]): Promise<{ code: number; output: string }> {
  return new Promise((complete) => {
    const child = spawn("node", ["--experimental-strip-types", "bin/hd.js", ...args], {
      cwd: root,
    });
    let output = "";
    child.stdout.on("data", (chunk: Buffer) => (output += chunk.toString()));
    child.stderr.on("data", (chunk: Buffer) => (output += chunk.toString()));
    child.on("close", (code) => complete({ code: code ?? 1, output }));
  });
}

function summarize(output: string): string {
  const crash = /^(\w*Error): (.*)$/m.exec(output);
  if (crash && output.includes("    at ")) return `CRASH ${crash[1]}: ${crash[2]!.slice(0, 80)}`;
  const lines = output
    .trim()
    .split("\n")
    .filter((line) => line.trim() !== "");
  return lines.slice(-2).join(" | ").slice(0, 160);
}

function verdict(expect: Expectation, file: string, code: number, output: string): boolean {
  const crashed = output.includes("    at ");
  if (expect.kind === "accept" || expect.kind === "pass") return code === 0;
  if (expect.kind === "fail") return code !== 0;
  if (expect.kind === "panic")
    return code !== 0 && !crashed && output.includes(`${expect.code}: runtime panic`);
  return code !== 0 && output.includes(`${file}:${expect.line}:`) && output.includes(expect.code);
}

async function main(): Promise<void> {
  console.log("id\tfile\tmechanism\tspec\tcommand\texpected\texit\tobserved\tverdict\tcoverage");
  for (const probe of probes) {
    const file = `audit/probes/gaps/${probe.file}`;
    const { code, output } = await run([...probe.command, file]);
    const expected =
      probe.expect.kind === "reject"
        ? `reject ${probe.expect.code}@${probe.expect.line}`
        : probe.expect.kind === "panic"
          ? `panic ${probe.expect.code}`
          : probe.expect.kind;
    const ok = verdict(probe.expect, file, code, output);
    console.log(
      [
        probe.id,
        probe.file,
        probe.mechanism,
        probe.spec,
        `hd ${probe.command.join(" ")}`,
        expected,
        String(code),
        summarize(output),
        ok ? "PASS" : "FAIL",
        probe.coverage,
      ].join("\t"),
    );
  }
}

await main();
