import type { HirFunction } from "./hir.ts";
import { RuntimePanicError } from "./runtime-panic.ts";
import {
  type CaseResult,
  PropertyDiscard,
  propertyRun,
  type PropertyRun,
  runProperty,
} from "./property-tests.ts";

// Runs the entry point and the test cases that `hd run` or `hd test`
// selected (spec/10-modules.md#test-outcomes). An `it_each` table is one test
// function that runs once per row: the runner selects the row through the
// exported `__hd_each_index` global and stops at the row count the function
// reports through `__hd_each_count` (spec/std/testing.md#table-test-rows).

export type RunOutcome =
  | { readonly kind: "passed"; readonly count: number; readonly result?: unknown }
  | { readonly kind: "exit"; readonly code: number }
  | { readonly kind: "failed"; readonly subject: string; readonly outcome?: string };

type Exports = WebAssembly.Exports;

function exportName(declaration: HirFunction): string {
  return /^\$test\.\d+$/.test(declaration.name)
    ? `__hd_test_${declaration.name.slice(6)}`
    : declaration.name;
}

function call(exports: Exports, declaration: HirFunction, row: number | undefined): unknown {
  if (declaration.parameters.length > 0)
    throw new Error(`${declaration.name} must not declare ordinary parameters`);
  const entry = exports[exportName(declaration)];
  if (typeof entry !== "function") throw new Error(`${declaration.name} has no runnable export`);
  if (row !== undefined) (exports.__hd_each_index as WebAssembly.Global).value = row;
  return entry(...declaration.requirements.map((requirement) => ({ requirement })));
}

/** The row count a table reported, or -1 when it stopped before reporting one. */
function rowCount(exports: Exports): number {
  return (exports.__hd_each_count as WebAssembly.Global).value as number;
}

function caseName(declaration: HirFunction, row: number | undefined): string {
  const name = declaration.testOptions?.name ?? declaration.name;
  return row === undefined ? name : `${name}[${row}]`;
}

// An entry point or test case with a non-void result exports the code that
// `report()` gives for it, or -1 for an `.Err`
// (spec/10-modules.md#r-module.entry.exit-report), whether it suspends or
// not. A test case fails on any nonzero code
// (spec/10-modules.md#r-module.testing.fail).
function judge(declaration: HirFunction, result: unknown, subject: string): RunOutcome | undefined {
  if (typeof result !== "number") return undefined;
  if (declaration.entry) {
    if (result === -1) return { kind: "failed", subject: "main" };
    if (result !== 0) return { kind: "exit", code: result };
    return undefined;
  }
  if (declaration.testOptions && result === -1) return { kind: "failed", subject };
  if (declaration.testOptions && result !== 0)
    return { kind: "failed", subject, outcome: `reported exit code ${result}` };
  return undefined;
}

/** The timeout a test function reported, in milliseconds, or undefined. */
function timeoutMillis(exports: Exports): number | undefined {
  const global = exports.__hd_timeout_ms as WebAssembly.Global | undefined;
  const value = global === undefined ? -1 : Number(global.value);
  return value >= 0 ? value : undefined;
}

// A test case fails when it runs longer than its `timeout`
// (spec/std/testing.md#r-std-testing.option.timeout-any-duration). The
// prototype runs a body synchronously, so it checks the elapsed time after
// the body returns; it cannot stop a body that never returns.
function overran(exports: Exports, started: number, subject: string): RunOutcome | undefined {
  const limit = timeoutMillis(exports);
  if (limit === undefined || performance.now() - started <= limit) return undefined;
  return { kind: "failed", subject: `${subject} exceeding its ${limit}ms timeout` };
}

/** Runs one test case (or table row) in `exports`; undefined when it passes. */
function runCase(
  exports: Exports,
  declaration: HirFunction,
  row: number | undefined,
): { readonly outcome?: RunOutcome; readonly noRows?: boolean } {
  const expected = declaration.testOptions?.expectPanic;
  const subject = `test "${caseName(declaration, row)}"`;
  const started = performance.now();
  let result: unknown;
  try {
    result = call(exports, declaration, row);
  } catch (error) {
    if (!(error instanceof RuntimePanicError)) throw error;
    // A table with no rows reports count 0, then indexes row 0.
    if (row === 0 && rowCount(exports) === 0) return { noRows: true };
    if (error.code !== expected) throw error;
    const late = overran(exports, started, subject);
    return late ? { outcome: late } : {};
  }
  const late = overran(exports, started, subject);
  if (late) return { outcome: late };
  if (expected !== undefined)
    return { outcome: { kind: "failed", subject: `${subject} expecting panic ${expected}` } };
  const outcome = judge(declaration, result, subject);
  return outcome ? { outcome } : {};
}

// Each test case, and each `it_each` row, runs in its own fresh program
// instance (spec/10-modules.md#r-module.testing.instance); the entry point
// runs in `shared`.
export async function runSelected(
  selected: readonly HirFunction[],
  shared: Exports,
  fresh: () => Promise<Exports>,
  // Called as each test case, or `it_each` row, starts (for `snapshot_file`).
  begin?: (name: string, row: number | undefined) => void,
  // The draws of property test cases (src/property-tests.ts).
  properties: PropertyRun = propertyRun(),
): Promise<RunOutcome> {
  let count = 0;
  let last: unknown;
  for (const declaration of selected) {
    if (!declaration.testOptions) {
      const result = call(shared, declaration, undefined);
      const outcome = judge(declaration, result, "main");
      if (outcome) return outcome;
      // An entry point returns its exit code, which is not printed.
      last = declaration.entry && typeof result === "number" ? undefined : result;
      count += 1;
      continue;
    }
    if (declaration.testOptions.property) {
      const name = declaration.testOptions.name;
      const once = async (): Promise<CaseResult> => {
        const exports = await fresh();
        begin?.(name, undefined);
        try {
          const { outcome } = runCase(exports, declaration, undefined);
          if (outcome?.kind !== "failed") return "pass";
          return { failure: outcome.outcome ?? outcome.subject };
        } catch (error) {
          if (error instanceof PropertyDiscard) return "discard";
          if (error instanceof RuntimePanicError) return { failure: `panicked with ${error.code}` };
          throw error;
        }
      };
      const failed = await runProperty(properties, name, once);
      if (failed) return { kind: "failed", ...failed };
      count += 1;
      continue;
    }
    const table = declaration.testOptions?.table === true;
    let rows = 1;
    for (let row = 0; row < rows; row += 1) {
      const exports = await fresh();
      begin?.(declaration.testOptions.name ?? declaration.name, table ? row : undefined);
      const { outcome, noRows } = runCase(exports, declaration, table ? row : undefined);
      if (outcome) return outcome;
      if (noRows) break;
      count += 1;
      if (!table) break;
      rows = rowCount(exports);
    }
  }
  return { kind: "passed", count, result: last };
}
