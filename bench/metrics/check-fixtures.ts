// The fixture guard: checks every hd program the metrics run or generate
// with one hd, so a language change that breaks a fixture fails CI instead
// of the next daily run (crates/hd_cli/tests/all/metrics_fixtures.rs runs
// it against the hd it builds).
//
// - Each program package of lib/fixtures.ts PROGRAMS, with default params,
//   and each generator's output at its smallest size (lib/gen.ts packages
//   with their executables, the test packages, lib/pathological.ts at
//   scale 1, the bench/micro cases as programs and as tests, the dependency
//   packages): `hd check --tests --format json` reports nothing.
// - Each mistake program (fixtures/mistakes/): its diagnostics have the
//   expected code, and no code beyond its `# cascade:` list; its `# fixed:`
//   variant checks clean.
// - Each errors-per-run file: every diagnostic lands on a marked line with
//   the marked code, and every marked line is reported, except that a file
//   with a syntax error need not report later phases (hd type-checks only
//   source that parses).
// It also fails when a fixture file is used by none of these.
//
// Usage: node --experimental-strip-types bench/metrics/check-fixtures.ts
//          [--hd "COMMAND"] [--verbose]
// The default hd is target/release/hd. With --verbose it prints what hd
// reports for each mistake program, as `code@line`.

import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { MICRO_CASES, microPackage } from "./lib/artifact.ts";
import { loadMistakes, type Mistake } from "./lib/corpus.ts";
import { DEPENDENCY_FILES, hasGit, prepareDependent } from "./lib/deps.ts";
import {
  fixture,
  fixturePaths,
  PROGRAMS,
  programFiles,
  writePackage,
  type Files,
  type ProgramName,
} from "./lib/fixtures.ts";
import {
  docTestPackage,
  editSignature,
  executable,
  generatePackage,
  integrationPackage,
  reachEveryModule,
  unitPackage,
} from "./lib/gen.ts";
import { diagnosticsOf, parseHdCommand, runHd, type HdCommand } from "./lib/hd.ts";
import type { MetricContext } from "./lib/metric.ts";
import { CASES, generate, STARTUP } from "./lib/pathological.ts";
import { makeTempDir, removeAllTempDirs } from "./lib/tmp.ts";
import { casePackage } from "./scripts/dev-speed.ts";
import { FILES as ERROR_FILES } from "./scripts/errors-per-run.ts";

const REPO = join(import.meta.dirname, "..", "..");
const TIMEOUT_MS = 60_000;
const POOL = 8;

type Diagnostic = Record<string, unknown>;

const describe = (found: readonly Diagnostic[]): string =>
  found
    .map((d) => `${String(d.code)}@${d.file ? `${String(d.file)}:` : ""}${String(d.line)}`)
    .join(" ") || "none";

/** One check: a name for the report, and the problem it found, if any. */
interface Check {
  readonly name: string;
  readonly run: () => Promise<string | undefined>;
}

/** The files every check uses, so an unused fixture is caught. */
const used = new Set<string>();
const use = (path: string): string => {
  used.add(path);
  return fixture(path);
};

async function checkDir(hd: HdCommand, dir: string, file?: string): Promise<Diagnostic[]> {
  const result = await runHd(
    hd,
    ["check", "--tests", "--format", "json", ...(file ? [file] : [])],
    {
      cwd: dir,
      env: { HD_CACHE: makeTempDir("cache") },
      timeoutMs: TIMEOUT_MS,
    },
  );
  if (result.timedOut) throw new Error(`hd check timed out after ${TIMEOUT_MS / 1000} s`);
  const found = diagnosticsOf(result.stdout + result.stderr);
  if (result.status !== 0 && found.length === 0)
    throw new Error(
      `hd check exited ${String(result.status)}: ${(result.stdout + result.stderr).trim().slice(0, 300)}`,
    );
  return found;
}

const checkFile = async (hd: HdCommand, file: string, text: string) => {
  const dir = makeTempDir("fixture");
  writeFileSync(join(dir, file), text);
  return checkDir(hd, dir, file);
};

const clean = (found: readonly Diagnostic[]): string | undefined =>
  found.length === 0 ? undefined : `expected a clean check, got ${describe(found)}`;

/** A package check that must report nothing. */
const cleanPackage = (hd: HdCommand, name: string, files: () => Files): Check => ({
  name,
  run: async () => clean(await checkDir(hd, writePackage("fixture", files()))),
});

/** A single-file check that must report nothing. */
const cleanFile = (hd: HdCommand, name: string, text: () => string): Check => ({
  name,
  run: async () => clean(await checkFile(hd, "case.hd", text())),
});

function programChecks(hd: HdCommand): Check[] {
  return (Object.keys(PROGRAMS) as ProgramName[]).map((name) => {
    for (const path of Object.values(PROGRAMS[name].files)) use(path);
    return cleanPackage(hd, `program ${name}`, () => programFiles(name));
  });
}

function generatorChecks(hd: HdCommand, context: MetricContext): Check[] {
  for (const path of [
    "package/module.hd.tmpl",
    "package/helpers.hd.tmpl",
    "package/lib.hd",
    "package/smoke.hd",
    "package/main.hd",
    "package/reach-all.hd.tmpl",
    "unit-test-perf/module.hd.tmpl",
    "unit-test-perf/module_test.hd.tmpl",
    "integration-test-perf/lib.hd.tmpl",
    "integration-test-perf/export.hd.tmpl",
    "dev-speed/case-test.hd.tmpl",
    "deps/dep.hd",
    "deps/dependent.hd",
    STARTUP,
    ...CASES.map((entry) => `pathological/${entry.name}.hd.tmpl`),
  ])
    use(path);
  const small = () => generatePackage({ seed: "fixtures", lines: 400 });
  const withMain = (main: (pkg: ReturnType<typeof small>) => string) => () => {
    const pkg = small();
    return new Map([...pkg.files, ["src/main.hd", main(pkg)]]);
  };
  return [
    cleanPackage(hd, "generated package, with its executable", withMain(executable)),
    cleanPackage(hd, "generated package, reaching every module", withMain(reachEveryModule)),
    cleanPackage(hd, "generated package, signatures edited", () => {
      const pkg = small();
      return new Map(
        [...pkg.files].map(([path, text]) => [
          path,
          path.match(/^src\/m\d+\.hd$/) ? editSignature(text) : text,
        ]),
      );
    }),
    cleanPackage(hd, "unit-test package", () => unitPackage(20)),
    cleanPackage(hd, "integration-test package", () => integrationPackage(2)),
    cleanPackage(hd, "doc-test package", () => docTestPackage(2)),
    cleanPackage(hd, "dependency package", () => DEPENDENCY_FILES),
    {
      name: "dependent package, after hd add",
      run: async () => {
        if (!hasGit()) return undefined;
        const prepared = await prepareDependent(hd);
        if ("problem" in prepared) return prepared.problem;
        if ("na" in prepared) return `no dependency to check: ${prepared.na}`;
        const result = await runHd(hd, ["check", "--tests", "--format", "json"], {
          cwd: prepared.dir,
          env: { ...prepared.remote.env, HD_CACHE: makeTempDir("cache") },
          timeoutMs: TIMEOUT_MS,
        });
        return clean(diagnosticsOf(result.stdout + result.stderr));
      },
    },
    cleanFile(hd, "pathological start-up program", () => fixture(STARTUP)),
    ...CASES.map((entry) =>
      cleanFile(hd, `pathological ${entry.name}, scale 1`, () => generate(entry, 1)),
    ),
    ...MICRO_CASES.flatMap((name) => [
      cleanPackage(hd, `micro case ${name}`, () => microPackage(context, name)),
      cleanPackage(hd, `micro case ${name} as a test`, () => casePackage(context, name)),
    ]),
  ];
}

/** Why a mistake program's report does not show its mistake, or undefined. */
export function mistakeProblem(mistake: Mistake, found: readonly Diagnostic[]): string | undefined {
  const codes = found.map((d) => String(d.code));
  const allowed = new Set([mistake.code, ...mistake.cascade]);
  if (!codes.includes(mistake.code)) return `expected ${mistake.code}, got ${describe(found)}`;
  const other = found.filter((d) => !allowed.has(String(d.code)));
  if (other.length > 0)
    return `expected ${[...allowed].join(", ")} only, also got ${describe(other)}`;
  return undefined;
}

function mistakeChecks(hd: HdCommand, verbose: boolean): Check[] {
  return loadMistakes().flatMap((mistake) => {
    use(`mistakes/${mistake.name}.hd`);
    const file = `${mistake.name}.hd`;
    const checks: Check[] = [
      {
        name: `mistake ${mistake.name}`,
        run: async () => {
          const found = await checkFile(hd, file, mistake.text);
          if (verbose) console.log(`${mistake.name}: ${describe(found)}`);
          return mistakeProblem(mistake, found);
        },
      },
    ];
    if (mistake.fixedText !== undefined) {
      const fixed = mistake.fixedText;
      checks.push({
        name: `mistake ${mistake.name}, fixed`,
        run: async () => clean(await checkFile(hd, file, fixed)),
      });
    }
    return checks;
  });
}

/** Why an errors-per-run file's report differs from its markers, or undefined. */
export function markerProblem(text: string, found: readonly Diagnostic[]): string | undefined {
  const marks = new Map<number, string>();
  text.split("\n").forEach((line, index) => {
    const match = /# (?:diagnostic|warning): ([a-z0-9-]+)\s*$/.exec(line);
    if (match) marks.set(index + 1, match[1]!);
  });
  const stray = found.filter((d) => marks.get(Number(d.line)) !== d.code);
  if (stray.length > 0) return `unmarked or wrong diagnostics: ${describe(stray)}`;
  const parses = ![...marks.values()].includes("syntax-error");
  const missing = [...marks].filter(
    ([line, code]) =>
      (parses || code === "syntax-error") && !found.some((d) => Number(d.line) === line),
  );
  if (missing.length > 0)
    return `marked lines not reported: ${missing.map(([line, code]) => `${code}@${line}`).join(" ")}`;
  return undefined;
}

function markerChecks(hd: HdCommand): Check[] {
  return ERROR_FILES.map((label) => {
    const path = `errors-per-run/${label}.hd`;
    const text = use(path);
    return {
      name: `errors-per-run ${label}`,
      run: async () => markerProblem(text, await checkFile(hd, `${label}.hd`, text)),
    };
  });
}

/** Every check of the fixtures, and the fixture files none of them uses. */
export function fixtureChecks(
  hd: HdCommand,
  verbose = false,
): { readonly checks: Check[]; readonly unused: string[] } {
  const context = { repoRoot: REPO } as MetricContext;
  const checks = [
    ...programChecks(hd),
    ...generatorChecks(hd, context),
    ...mistakeChecks(hd, verbose),
    ...markerChecks(hd),
  ];
  return { checks, unused: fixturePaths().filter((path) => !used.has(path)) };
}

async function main(): Promise<number> {
  const args = process.argv.slice(2);
  const at = args.indexOf("--hd");
  const hd = parseHdCommand(at >= 0 ? args[at + 1]! : "target/release/hd", REPO);
  const started = performance.now();
  const { checks, unused } = fixtureChecks(hd, args.includes("--verbose"));
  const problems: string[] = unused.map((path) => `fixtures/${path}: no check uses it`);
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < checks.length) {
      const check = checks[next++]!;
      try {
        const problem = await check.run();
        if (problem) problems.push(`${check.name}: ${problem}`);
      } catch (error) {
        problems.push(`${check.name}: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
  };
  await Promise.all(Array.from({ length: POOL }, worker));
  const seconds = ((performance.now() - started) / 1000).toFixed(1);
  for (const problem of problems.sort()) console.log(`FAIL ${problem}`);
  console.log(
    `${checks.length} fixture checks with ${hd.display} in ${seconds} s: ${problems.length} failed`,
  );
  return problems.length === 0 ? 0 : 1;
}

if (import.meta.main) {
  try {
    process.exitCode = await main();
  } finally {
    removeAllTempDirs();
  }
}
