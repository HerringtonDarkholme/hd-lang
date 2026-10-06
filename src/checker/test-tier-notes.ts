// Notes that name the fix for a `missing-requirement` error a test tier
// causes. A unit test case gets `TestRunner` alone
// (spec/lang/10-modules.md#r-module.testing.unit-row.anywhere), so its note
// names the std fake for each missing host trait and the test root
// (spec/std/testing.md#r-std-testing.unit.hint). A test build runs no entry
// behavior, so a script's top level must be requirement-free there
// (spec/lang/10-modules.md#r-module.init.tests.requirement-free).

import type { Program } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { displayType } from "../types.ts";

/** The std fake of each host capability trait (spec/std/testing.md#unit-test-providers). */
const UNIT_TEST_FAKES: Readonly<
  Record<string, { readonly provider: string; readonly module: string }>
> = {
  Console: { provider: "BufferConsole::new()", module: "std.console" },
  ConsoleInput: { provider: "ScriptedInput::new(lines)", module: "std.console" },
  Args: { provider: "MapArgs::new(program, values)", module: "std.host" },
  Env: { provider: "MapEnv::new(values)", module: "std.host" },
  Clock: { provider: "ManualClock::new(start)", module: "std.time" },
  Random: { provider: "SeededRandom::new(seed)", module: "std.random" },
  FsRead: { provider: "MemoryFs::new()", module: "std.fs" },
  FsWrite: { provider: "MemoryFs::new()", module: "std.fs" },
  Process: { provider: "ScriptedProcess::new(outputs)", module: "std.process" },
  Http: { provider: "ScriptedHttp::new(responses)", module: "std.http" },
};

export interface TestTierNames {
  /** The program's names of the default profile's traits and of `Process`, as std declares them. */
  readonly hostTraits: ReadonlyMap<string, string>;
}

function within(inner: SourceSpan, outer: SourceSpan): boolean {
  return inner.start.offset >= outer.start.offset && inner.end.offset <= outer.end.offset;
}

/** The traits a `missing-requirement` message names, as displayed to the user. */
function missingTraits(message: string): string[] {
  return (
    /requires (.+)$/.exec(message)?.[1]?.split(", ") ??
    /provider '([^']+)' is not available/.exec(message)?.slice(1) ??
    []
  ).map((name) => name.trim());
}

/** The host traits a `missing-requirement` message names, by their std names. */
function missingHostTraits(message: string, names: TestTierNames): string[] {
  return missingTraits(message).flatMap((name) => {
    const standard = names.hostTraits.get(name.trim());
    return standard ? [standard] : [];
  });
}

function unitTestNote(traits: readonly string[]): string {
  const fakes = [...new Set(traits)].map((trait) => {
    const fake = UNIT_TEST_FAKES[trait]!;
    return `$.with(${trait}=${fake.provider}) from ${fake.module}`;
  });
  return `a unit test case gets only TestRunner from the runner: provide a fake with ${fakes.join(", and ")}, or move the test case to tests/ to run it on the real host`;
}

const SCRIPT_NOTE =
  "a test build runs no entry behavior, so a script's top level is module initialization and must be requirement-free: move the script's work into `main`";

function integrationTestNote(traits: readonly string[]): string | undefined {
  if (traits.length === 0) return undefined;
  const bindings = traits.map((trait) => `\`$.with(${trait}=...)\``);
  return traits.length === 1
    ? `the test profile does not bind ${traits[0]}: bind it with ${bindings[0]}`
    : `the test profile does not bind ${traits.join(", ")}: bind them with ${bindings.join(", ")}`;
}

/**
 * `diagnostics` with a note on each `missing-requirement` error that a test
 * tier causes: a unit or integration test body, or a tested script's top level.
 */
export function withTestTierNotes(
  diagnostics: readonly Diagnostic[],
  program: Program,
  options: { readonly integrationTest?: boolean; readonly testBuild?: boolean },
  names: TestTierNames,
): Diagnostic[] {
  const hasMain = program.functions.some((declaration) => declaration.name === "main");
  return diagnostics.map((diagnostic) => {
    if (diagnostic.code !== "missing-requirement") return diagnostic;
    let note: string | undefined;
    if (
      !options.integrationTest &&
      program.tests.some((test) => within(diagnostic.span, test.span))
    ) {
      const traits = missingHostTraits(diagnostic.message, names);
      if (traits.length > 0) note = unitTestNote(traits);
    } else if (
      options.integrationTest &&
      program.tests.some((test) => within(diagnostic.span, test.span))
    ) {
      note = integrationTestNote(missingTraits(diagnostic.message));
    } else if (
      options.testBuild &&
      !hasMain &&
      program.statements.some((statement) => within(diagnostic.span, statement.span))
    )
      note = SCRIPT_NOTE;
    return note ? { ...diagnostic, notes: [...(diagnostic.notes ?? []), note] } : diagnostic;
  });
}

/** The program's display names of the host traits, each mapped to its std name. */
export function testTierNames(
  defaultProfile: readonly string[],
  standardNames: readonly string[],
  process: string,
): TestTierNames {
  const hostTraits = new Map<string, string>();
  defaultProfile.forEach((name, index) => hostTraits.set(displayType(name), standardNames[index]!));
  hostTraits.set(displayType(process), "Process");
  return { hostTraits };
}
