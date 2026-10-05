// Doc tests in `hd test` and `hd check --tests` (spec/lang/10-modules.md#doc-tests,
// spec/cli/command-line.md#test-runs). Each doc test of a module under the
// source root compiles as its own program, which the linker places at a
// fresh path under the test root, so that it sees the package as an
// integration test program does (module.test.doc.view) and its row comes
// from the profile (module.test.doc.row). `loadSource` moves its
// diagnostics to the module's `##` lines (DocTestLoad). src/doc-tests.ts
// finds the blocks.

import type { Expression, Program } from "../ast.ts";
import { analyze } from "../compiler.ts";
import { DiagnosticReporter, type Report } from "../diagnostic-report.ts";
import { DiagnosticError, type Diagnostic } from "../diagnostics.ts";
import { docTests, filePosition, lineOffsets, type DocTest } from "../doc-tests.ts";
import { moduleIdentity, SOURCE_ROOT, TEST_ROOT } from "../package.ts";
import { parse } from "../parser/index.ts";
import { EXIT_HD_FAILURE, type CommandEnvironment } from "./io.ts";
import {
  loadSource,
  reportFailure,
  type LoadedSource,
  type PackagePlacement,
  type RuntimeProfileName,
  type SourceArgs,
} from "./source.ts";

/** A module's test cases counted across its programs: its own, and its doc tests'. */
export interface TestTally {
  passed: number;
  failed: number;
  /** The test cases the run selected, by `--filter` (cli.test.filter). */
  selected: number;
  /** Every test case, selected or not. */
  registered: number;
}

/** The module whose doc tests a command compiles, and where its output goes. */
export interface DocTestModule extends CommandEnvironment {
  readonly format: SourceArgs["format"];
  readonly report: Report;
  /** The module's file as the command names it. */
  readonly file: string;
  /** The module's place in its package. */
  readonly placement: PackagePlacement;
  readonly profile?: RuntimeProfileName;
}

/**
 * The doc tests of the module at `placement`, whose text is `source`. Only
 * a module under the source root has any (spec/lang/10-modules.md#r-module.test.doc.block).
 */
export function moduleDocTests(placement: PackagePlacement | undefined, source: string): DocTest[] {
  if (!placement?.path.startsWith(SOURCE_ROOT)) return [];
  const identity = moduleIdentity(placement.path);
  if (identity === undefined) return [];
  // The root module's doc tests are named `doc pkg.<item>[i]` (cli.test.doc.name.root).
  return docTests(source, identity === "" ? "pkg" : identity);
}

/** A path under the test root that names no file or directory of the package. */
function freshTestPath(files: Readonly<Record<string, string>>): string {
  const paths = Object.keys(files);
  for (let count = 0; ; count += 1) {
    const name = `${TEST_ROOT}doc_test${count === 0 ? "" : `_${count}`}`;
    if (!paths.some((path) => path === `${name}.hd` || path.startsWith(`${name}/`)))
      return `${name}.hd`;
  }
}

/**
 * Loads a doc test's program, linked with the package. With `capture`, its
 * diagnostics are collected there, not reported.
 */
export async function loadDocTest(
  module: DocTestModule,
  test: DocTest,
  moduleSource: string,
  capture?: Diagnostic[],
): Promise<LoadedSource | number> {
  const { placement } = module;
  const files = { ...placement.files, [placement.path]: moduleSource };
  return loadSource(
    { ...module, text: test.program },
    {
      report: module.report,
      profile: module.profile,
      // A doc test sees the package without its test code (module.test.doc.view).
      linkTests: false,
      docTest: {
        test,
        modulePath: placement.path,
        moduleSource,
        ...(capture ? { capture } : {}),
      },
    },
    {
      root: placement.root,
      path: freshTestPath(files),
      files,
      ...(placement.reported ? { reported: placement.reported } : {}),
      ...(placement.programs ? { programs: placement.programs } : {}),
      ...(placement.dependencies ? { dependencies: placement.dependencies } : {}),
    },
  );
}

/**
 * Judges a compile-fail doc test, which never runs: it passes only when
 * compiling it reports each of its `# error: CODE` codes
 * (spec/lang/10-modules.md#r-module.test.doc.compile-fail).
 */
export async function judgeCompileFail(
  module: DocTestModule,
  test: DocTest,
  moduleSource: string,
  tally: TestTally,
): Promise<number> {
  const captured: Diagnostic[] = [];
  const loaded = await loadDocTest(module, test, moduleSource, captured);
  if (typeof loaded !== "number") {
    try {
      captured.push(...analyze(loaded.source, loaded.compileOptions).diagnostics);
    } catch (error) {
      if (!(error instanceof DiagnosticError)) throw error;
      captured.push(...error.diagnostics);
    }
  }
  const codes = [...new Set(captured.map(({ code }) => code))];
  const missing = test.errors.filter((code) => !codes.includes(code));
  if (missing.length === 0) {
    tally.passed += 1;
    module.report.test(test.name, "passed");
    return 0;
  }
  // module.test.doc.compile-fail.fails
  const message = `expected error ${missing.join(", ")}, but compiling it reported ${codes.length === 0 ? "no diagnostic" : codes.join(", ")}`;
  tally.failed += 1;
  module.report.test(test.name, "failed", message);
  if (module.format === "text")
    new DiagnosticReporter(
      module.report,
      module.file,
      moduleSource,
      undefined,
      module.placement.path,
      test,
    ).entryError(`doc test "${test.name}"`, message);
  return 1;
}

/**
 * `hd check --tests` checks the module's doc tests, except compile-fail
 * ones, which only `hd test` judges (spec/cli/command-line.md#r-cli.check.tests.doc).
 */
export async function checkDocTests(module: DocTestModule, moduleSource: string): Promise<number> {
  let status = 0;
  for (const test of moduleDocTests(module.placement, moduleSource)) {
    if (test.errors.length > 0) continue;
    const loaded = await loadDocTest(module, test, moduleSource);
    if (typeof loaded === "number") {
      status = Math.max(status, loaded);
      continue;
    }
    try {
      const result = analyze(loaded.source, loaded.compileOptions);
      if (!result.hir) throw new DiagnosticError(result.diagnostics);
      for (const diagnostic of result.diagnostics) loaded.report(diagnostic);
    } catch (error) {
      status = Math.max(status, reportFailure(loaded, error));
    }
  }
  return status;
}

/** The line where a module's `tests:` block starts, for the order of its doc tests. */
export function testsBlockLine(source: string): number {
  const match = /^tests:/m.exec(source);
  return match ? source.slice(0, match.index).split("\n").length : Number.POSITIVE_INFINITY;
}

// An update run rewrites a failing doc test `snapshot`'s expected text in
// place, inside its block's `##` lines (spec/cli/command-line.md#r-cli.test.doc.update).
// A mismatched `snapshot` fails as `assert_equal` of two strings does, with
// the reason "the snapshot matches" and both texts as hd literals
// (lib/std/format.hd), so the failure gives the new literal.

const SNAPSHOT_FAILURE = "the snapshot matches: actual ";

/** The string literal at `start` of `text`, through its closing quote. */
function literalAt(text: string, start: number): { text: string; end: number } | undefined {
  if (text[start] !== '"') return undefined;
  for (let index = start + 1; index < text.length; index += 1) {
    if (text[index] === "\\") index += 1;
    else if (text[index] === '"') return { text: text.slice(start, index + 1), end: index + 1 };
  }
  return undefined;
}

const ESCAPES: Readonly<Record<string, string>> = { n: "\n", r: "\r", t: "\t", "0": "\0" };

/** The text of a string literal as `debug` writes one. */
function decodeLiteral(literal: string): string {
  return literal
    .slice(1, -1)
    .replace(/\\(?:u\{([0-9a-fA-F]+)\}|(.))/gs, (_, hex: string | undefined, other: string) =>
      hex === undefined
        ? (ESCAPES[other] ?? other)
        : String.fromCodePoint(Number.parseInt(hex, 16)),
    );
}

type Call = Extract<Expression, { kind: "call" }>;

/** The calls of `std.testing.snapshot` in a doc test's program, in source order. */
function snapshotCalls(program: Program): Call[] {
  const names = new Set<string>();
  const namespaces = new Set<string>();
  for (const use of program.uses)
    for (const { name, alias } of use.names) {
      if (use.module === "std.testing" && name === "snapshot") names.add(alias ?? name);
      if (use.module === "std" && name === "testing") namespaces.add(alias ?? name);
    }
  const calls: Call[] = [];
  const visit = (value: unknown): void => {
    if (!value || typeof value !== "object") return;
    if (Array.isArray(value)) return value.forEach(visit);
    const node = value as Partial<Call>;
    const callee = node.callee;
    if (
      node.kind === "call" &&
      callee &&
      ((callee.kind === "name" && names.has(callee.name)) ||
        (callee.kind === "member" &&
          callee.name === "snapshot" &&
          callee.receiver.kind === "name" &&
          namespaces.has(callee.receiver.name)))
    )
      calls.push(node as Call);
    for (const [key, child] of Object.entries(node)) if (key !== "span") visit(child);
  };
  visit(program);
  return calls.sort((left, right) => left.span.start.offset - right.span.start.offset);
}

/**
 * The module's text with a doc test's failing `snapshot` rewritten to expect
 * the actual text, or undefined when `message` is no snapshot mismatch or
 * no call of the block expects the text that failed.
 */
export function snapshotRewrite(
  test: DocTest,
  message: string,
  moduleSource: string,
): string | undefined {
  const at = message.indexOf(SNAPSHOT_FAILURE);
  if (at < 0) return undefined;
  const actual = literalAt(message, at + SNAPSHOT_FAILURE.length);
  if (!actual || !message.startsWith(", expected ", actual.end)) return undefined;
  const expected = literalAt(message, actual.end + ", expected ".length);
  if (!expected) return undefined;
  const wanted = decodeLiteral(expected.text);
  const program = parse(test.program, { testModule: true, integrationTest: true }).program;
  if (!program) return undefined;
  const offsets = lineOffsets(moduleSource);
  const offset = (position: Call["span"]["start"]): number =>
    filePosition(test, position, offsets).offset;
  for (const call of snapshotCalls(program)) {
    const names = call.argumentNames ?? [];
    const named = names.indexOf("expect");
    const index = named >= 0 ? named : call.arguments.length > 1 && names[1] === undefined ? 1 : -1;
    const argument = call.arguments[index];
    if (argument?.kind === "string" && argument.value === wanted) {
      const start = offset(argument.span.start);
      const end = offset(argument.span.end);
      return moduleSource.slice(0, start) + actual.text + moduleSource.slice(end);
    }
    // A call without `expect` expects "" and gets the argument.
    if (argument === undefined && wanted === "") {
      const close = offset(call.span.end) - 1;
      if (moduleSource[close] !== ")") continue;
      return `${moduleSource.slice(0, close)}, expect=${actual.text}${moduleSource.slice(close)}`;
    }
  }
  return undefined;
}

/** The status of a module's runs together: an `hd` failure outranks a failed test case. */
export function combinedStatus(left: number, right: number): number {
  return left === EXIT_HD_FAILURE || right === EXIT_HD_FAILURE
    ? EXIT_HD_FAILURE
    : Math.max(left, right);
}
