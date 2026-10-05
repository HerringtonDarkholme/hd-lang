import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { availableParallelism, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { createAdapter, type Adapter } from "./hd-adapter.ts";

interface CommandResult {
  readonly code: number;
  readonly stderr: string;
  readonly stdout: string;
}

interface Directive {
  readonly kind: "diagnostic" | "expect" | "expect-result" | "panic" | "warning";
  readonly line?: number;
  readonly value: string;
}

interface FixtureCase {
  readonly directives: readonly Directive[];
  readonly name: string;
  readonly path: string;
  readonly profile?: string;
}

type Phase = "parse" | "runtime" | "type";

interface Options {
  // Run only the conformance cases whose fixture differs from this git revision.
  readonly changed?: string;
  readonly command: readonly string[];
  readonly commandText: string;
  // Run this repository's compiler in-process (hd-adapter.ts) instead of
  // spawning `command`: the default, unless --compiler or HD_TEST_COMMAND
  // names an implementation.
  readonly inProcess: boolean;
  readonly jobs: number;
  // Run only these conformance cases, by case path (`typing/invalid/foo.hd`,
  // `cli/NAME`). A timed-out case reruns through the real harness this way.
  readonly only: readonly string[];
  readonly phase?: Phase;
  readonly suite: "all" | "conformance" | "fixtures";
  readonly tier?: "cli" | "language" | "std";
}

const root = resolve(import.meta.dirname, "..");
const portableManifest = resolve(root, "test/portable/cases.tsv");
const conformanceRunner = resolve(root, "spec/tools/run-conformance.ts");
const adapterModule = resolve(root, "test/hd-adapter.ts");
// The conformance runner's limit (spec/conformance/README.md#command-contract).
const timeoutMs = 10_000;
const fixtureRoot = resolve(root, "test/fixtures");

// Fixture expectations compare plain text; a FORCE_COLOR inherited from the
// caller's shell or CI would make Node color printed values.
const { FORCE_COLOR: _forceColor, ...childEnv } = process.env;

function splitCommand(value: string): string[] {
  const parts: string[] = [];
  const pattern = /"([^"]*)"|'([^']*)'|([^\s]+)/g;
  for (const match of value.matchAll(pattern)) parts.push(match[1] ?? match[2] ?? match[3]!);
  return parts;
}

function parseOptions(args: readonly string[]): Options {
  let commandText = process.env.HD_TEST_COMMAND ?? "node --experimental-strip-types bin/hd.js";
  let inProcess = process.env.HD_TEST_COMMAND === undefined;
  let jobs = Number(process.env.HD_TEST_JOBS ?? availableParallelism());
  let phase: Options["phase"];
  let suite: Options["suite"] = "all";
  let tier: Options["tier"];
  let changed: string | undefined;
  const only: string[] = [];
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index];
    const value = args[index + 1];
    if (option === "--changed") {
      // An optional revision follows; the default is origin/main.
      if (value && !value.startsWith("--")) changed = value;
      else {
        changed = "origin/main";
        index -= 1;
      }
    } else if (option === "--only" && value && !value.startsWith("--")) only.push(value);
    else if (option === "--compiler" && value) {
      commandText = value;
      inProcess = false;
    } else if (option === "--jobs" && value) jobs = Number(value);
    else if (option === "--phase" && /^(parse|type|runtime)$/.test(value ?? ""))
      phase = value as Phase;
    else if (option === "--suite" && /^(all|conformance|fixtures)$/.test(value ?? ""))
      suite = value as Options["suite"];
    else if (option === "--tier" && /^(language|std|cli)$/.test(value ?? ""))
      tier = value as Options["tier"];
    else throw new Error(`invalid option ${option ?? ""}`);
    index += 1;
  }
  const command = splitCommand(commandText);
  if (command.length === 0) throw new Error("compiler command must not be empty");
  if (!Number.isInteger(jobs) || jobs < 1) throw new Error("jobs must be a positive integer");
  if (phase && suite === "fixtures") throw new Error("--phase cannot use --suite fixtures");
  if (tier && suite === "fixtures") throw new Error("--tier cannot use --suite fixtures");
  if (only.length > 0 && changed !== undefined)
    throw new Error("--only cannot be combined with --changed");
  if (only.length > 0 && suite === "fixtures")
    throw new Error("--only selects conformance cases and cannot use --suite fixtures");
  return { changed, command, commandText, inProcess, jobs, only, phase, suite, tier };
}

/** How the fixture runner runs `IMPL ARGS...`: in-process, or a spawned command. */
type Implementation = Adapter | readonly string[];

async function invoke(
  implementation: Implementation,
  action: string,
  path: string,
  options: readonly string[] = [],
  runner: { readonly profile?: string; readonly entry?: string } = {},
): Promise<CommandResult> {
  // With no action, `IMPL FILE OPTION...` runs FILE as a single file
  // (spec/cli/command-line.md#r-cli.file.run).
  const args = action === "" ? [path, ...options] : [action, ...options, path];
  if (!Array.isArray(implementation)) {
    // Runner options go to the adapter, not to the command line.
    const result = await (implementation as Adapter).run(args, timeoutMs, undefined, runner);
    if (result.timedOut)
      return { code: 1, stderr: `ran longer than ${timeoutMs / 1000} s`, stdout: "" };
    return { code: result.status ?? 1, stderr: result.stderr, stdout: result.stdout };
  }
  const command = implementation as readonly string[];
  if (runner.profile !== undefined || runner.entry !== undefined)
    return {
      code: 1,
      stderr:
        "the fixture selects a runner option (runtime profile or entry), which only the in-process adapter can receive",
      stdout: "",
    };
  return new Promise((complete, reject) => {
    const child = spawn(command[0]!, [...command.slice(1), ...args], {
      cwd: root,
      env: childEnv,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => (stdout += chunk));
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => (stderr += chunk));
    child.on("error", reject);
    child.on("close", (code) => complete({ code: code ?? 1, stderr, stdout }));
  });
}

function failure(name: string, message: string, result: CommandResult): string {
  const output = `${result.stdout}${result.stderr}`.trim();
  return `${name}: ${message}${output ? `\n${output}` : ""}`;
}

function containsCode(result: CommandResult, code: string): boolean {
  return `${result.stdout}${result.stderr}`.includes(`${code}:`);
}

function containsLocatedCode(
  result: CommandResult,
  path: string,
  line: number,
  code: string,
): boolean {
  const escapedPath = path.replaceAll(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedCode = code.replaceAll(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`${escapedPath}:${line}:\\d+: (?:warning: )?${escapedCode}:`).test(
    `${result.stdout}${result.stderr}`,
  );
}

async function mapParallel<T, U>(
  values: readonly T[],
  jobs: number,
  operation: (value: T) => Promise<U>,
): Promise<U[]> {
  const results: U[] = [];
  let next = 0;
  async function worker(): Promise<void> {
    while (next < values.length) {
      const index = next;
      next += 1;
      results[index] = await operation(values[index]!);
    }
  }
  await Promise.all(Array.from({ length: Math.min(jobs, values.length) }, worker));
  return results;
}

// A verdict reason exactly equal to this failed only by the time limit: the
// conformance runner reports `ran longer than ${timeoutMs / 1000} s` for a
// timed-out command, with nothing else.
const timeoutReason = `ran longer than ${timeoutMs / 1000} s`;

interface CaseVerdict {
  readonly path: string;
  readonly reason: string;
}

/** The `FAIL  PATH: REASON` lines of a conformance run's stdout. */
function failVerdicts(output: string): CaseVerdict[] {
  const verdicts: CaseVerdict[] = [];
  for (const line of output.split("\n")) {
    if (!line.startsWith("FAIL  ")) continue;
    const rest = line.slice("FAIL  ".length);
    const separator = rest.indexOf(": ");
    if (separator < 0) continue;
    verdicts.push({ path: rest.slice(0, separator), reason: rest.slice(separator + 2) });
  }
  return verdicts;
}

/** The case paths a conformance run's stdout reports as passed. */
function passPaths(output: string): Set<string> {
  const passed = new Set<string>();
  for (const line of output.split("\n"))
    if (line.startsWith("pass  ")) passed.add(line.slice("pass  ".length));
  return passed;
}

// Whether a verdict reason is a timeout and nothing else: the bare message,
// or a step label before it (`check:`, `run:`, `step N (CMD):`). No other
// failure reason ends with the timeout sentence.
function isTimeoutReason(reason: string): boolean {
  return reason === timeoutReason || reason.endsWith(`: ${timeoutReason}`);
}

/** The case paths that failed only by the time limit. */
function timeoutPaths(output: string): string[] {
  return failVerdicts(output)
    .filter(({ reason }) => isTimeoutReason(reason))
    .map(({ path }) => path);
}

/**
 * The run's own failed count from its summary line, or undefined when the
 * run never summarized (a runner exception prints no summary).
 */
export function summaryFailedCount(output: string): number | undefined {
  const match = /^conformance: \d+ passed, (\d+) failed, \d+ selected/m.exec(output);
  return match ? Number(match[1]) : undefined;
}

/**
 * The timeout paths a failed first pass may retry, or undefined when it
 * must fail instead. Every failure the run reports must be a timeout: a
 * crash (no summary line) or any other failure never retries, so a runner
 * exception beside timeouts still fails instead of passing on the retry.
 */
export function retryTimeouts(output: string): string[] | undefined {
  const timeouts = timeoutPaths(output);
  if (timeouts.length === 0) return undefined;
  const failed = summaryFailedCount(output);
  if (failed === undefined || failed !== timeouts.length) return undefined;
  return timeouts;
}

/** A selection manifest naming exactly `paths`, judged by the case index. */
async function selectionManifest(paths: readonly string[]): Promise<string> {
  const path = join(await mkdtemp(join(tmpdir(), "hd-selected-")), "cases.tsv");
  await writeFile(path, `path\n${paths.join("\n")}\n`);
  return path;
}

interface ConformanceResult {
  readonly code: number;
  /** The runner's stdout, re-emitted to this process's stdout as it arrives. */
  readonly output: string;
}

// Runs one conformance pass through the implementation-neutral runner in
// spec/tools, which judges cases by spec/conformance/README.md.
async function runConformanceOnce(
  options: Options,
  manifest: string | undefined,
  jobs: number,
): Promise<ConformanceResult> {
  const args = [
    "--experimental-strip-types",
    conformanceRunner,
    ...(manifest ? ["--manifest", manifest] : []),
    ...(options.inProcess ? ["--adapter", adapterModule] : ["--compiler", options.commandText]),
    "--jobs",
    String(jobs),
    ...(options.phase ? ["--phase", options.phase] : []),
    ...(options.tier ? ["--tier", options.tier] : []),
  ];
  return new Promise((complete, reject) => {
    const child = spawn(process.execPath, args, {
      cwd: root,
      stdio: ["ignore", "pipe", "inherit"],
    });
    let output = "";
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => {
      output += chunk;
      process.stdout.write(chunk);
    });
    child.on("error", reject);
    child.on("close", (code) => complete({ code: code ?? 1, output }));
  });
}

// Runs the selected conformance cases. Cases that fail only by the time
// limit rerun once, serially; a case that times out again still fails, and a
// real failure never reruns. Selection manifests live in `hd-selected-*`
// temporary directories, removed once the run no longer needs them.
async function runConformance(options: Options): Promise<boolean> {
  const selected: string[] = [];
  const select = async (paths: readonly string[]): Promise<string> => {
    const manifest = await selectionManifest(paths);
    selected.push(manifest);
    return manifest;
  };
  try {
    const manifest =
      options.only.length > 0
        ? await select(options.only)
        : options.changed
          ? await changedManifest(options.changed)
          : undefined;
    if (options.changed && !manifest) {
      console.log(`conformance: no selected fixture differs from ${options.changed}`);
      return true;
    }
    const first = await runConformanceOnce(options, manifest ?? portableManifest, options.jobs);
    if (first.code === 0) return true;
    const retryPaths = retryTimeouts(first.output);
    if (retryPaths === undefined) return false;
    const retry = await runConformanceOnce(options, await select(retryPaths), 1);
    const passed = passPaths(retry.output);
    console.log(
      `${retryPaths.filter((path) => passed.has(path)).length} passed after a serial retry`,
    );
    return retry.code === 0;
  } finally {
    await Promise.all(
      selected.map((manifest) => rm(dirname(manifest), { recursive: true, force: true })),
    );
  }
}

// The rows of the portable manifest whose fixture, under spec/conformance,
// differs from `base` in the working tree (committed, staged, or not).
async function changedManifest(base: string): Promise<string | undefined> {
  const prefix = "spec/conformance/";
  const changed = new Set(
    execFileSync("git", ["diff", "--name-only", base, "--", prefix], {
      cwd: root,
      encoding: "utf8",
    })
      .split("\n")
      .concat(
        execFileSync("git", ["ls-files", "--others", "--exclude-standard", "--", prefix], {
          cwd: root,
          encoding: "utf8",
        }).split("\n"),
      )
      .filter((path) => path.endsWith(".hd") || path.startsWith(`${prefix}cli/`))
      .map((path) => path.slice(prefix.length))
      // A file of a CLI case selects the case, `cli/NAME`.
      .map((path) => (path.startsWith("cli/") ? path.split("/").slice(0, 2).join("/") : path)),
  );
  const [header, ...rows] = (await readFile(portableManifest, "utf8")).trimEnd().split("\n");
  const selected = rows.filter((row) => changed.has(row.split("\t")[0]!));
  if (selected.length === 0) return undefined;
  const path = join(await mkdtemp(join(tmpdir(), "hd-changed-")), "cases.tsv");
  await writeFile(path, `${[header, ...selected].join("\n")}\n`);
  return path;
}

async function fixturePaths(directory: string): Promise<string[]> {
  const paths: string[] = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) paths.push(...(await fixturePaths(path)));
    else if (entry.isFile() && entry.name.endsWith(".hd")) paths.push(path);
  }
  return paths.sort();
}

async function readFixtureCase(path: string): Promise<FixtureCase> {
  const lines = (await readFile(path, "utf8")).split("\n");
  let name: string | undefined;
  let profile: string | undefined;
  const directives: Directive[] = [];
  for (const [index, line] of lines.entries()) {
    if (line.startsWith("# test: ")) name = line.slice("# test: ".length).trim();
    if (line.startsWith("# fixture-runtime-profile: "))
      profile = line.slice("# fixture-runtime-profile: ".length).trim();
    for (const kind of ["expect", "expect-result"] as const) {
      const prefix = `# ${kind}: `;
      if (line.startsWith(prefix))
        directives.push({ kind, value: line.slice(prefix.length).trim() });
    }
    const marker = /# (diagnostic|warning|panic): ([a-z0-9-]+)\s*$/.exec(line);
    if (marker)
      directives.push({
        kind: marker[1] as Directive["kind"],
        line: index + 1,
        value: marker[2]!,
      });
  }
  if (!name) throw new Error(`${path}: missing '# test:' directive`);
  if (directives.length === 0) throw new Error(`${path}: missing expectation directive`);
  return { directives, name, path, profile };
}

async function runFixtureCase(
  command: Implementation,
  testCase: FixtureCase,
): Promise<string | undefined> {
  const kinds = new Set(testCase.directives.map(({ kind }) => kind));
  if (kinds.size !== 1) return `${testCase.name}: cannot mix expectation directive kinds`;
  const kind = testCase.directives[0]!.kind;
  if (kind === "expect-result") {
    for (const directive of testCase.directives) {
      const separator = directive.value.indexOf(" = ");
      if (separator < 1) return `${testCase.name}: expected '# expect-result: ENTRY = VALUE'`;
      const entry = directive.value.slice(0, separator);
      const expected = directive.value.slice(separator + 3);
      const result = await invoke(command, "", testCase.path, [], {
        profile: testCase.profile,
        entry,
      });
      if (result.code !== 0) return failure(testCase.name, `${entry} failed`, result);
      if (result.stdout.trim() !== expected)
        return failure(
          testCase.name,
          `${entry} returned ${JSON.stringify(result.stdout.trim())}, expected ${JSON.stringify(expected)}`,
          result,
        );
    }
    return undefined;
  }
  if (kind === "expect") {
    const directive = testCase.directives[0]!;
    // `hd test FILE` is an error for a file with no `tests:` block, so such a
    // fixture runs its entry point instead.
    const hasTests = /^tests:/m.test(await readFile(testCase.path, "utf8"));
    const action =
      directive.value === "parse"
        ? "parse"
        : directive.value === "test"
          ? hasTests
            ? "test"
            : ""
          : "check";
    if (!["accept", "parse", "test"].includes(directive.value))
      return `${testCase.name}: expected '# expect: accept', '# expect: parse', or '# expect: test'`;
    const result = await invoke(
      command,
      action,
      testCase.path,
      action === "check" ? ["--tests"] : [],
      { profile: testCase.profile },
    );
    return result.code === 0 ? undefined : failure(testCase.name, "expected acceptance", result);
  }
  const checked = await invoke(command, "check", testCase.path, ["--tests"], {
    profile: testCase.profile,
  });
  let result = checked;
  if (kind === "diagnostic" && checked.code === 0)
    return failure(testCase.name, "expected rejection", checked);
  if (kind === "warning" && checked.code !== 0)
    return failure(testCase.name, "expected warning, but compilation failed", checked);
  if (kind === "panic") {
    if (checked.code !== 0) return failure(testCase.name, "panic fixture did not compile", checked);
    result = await invoke(command, "", testCase.path, [], { profile: testCase.profile });
    if (result.code === 0) return failure(testCase.name, "expected a runtime panic", result);
  }
  const missing = testCase.directives.filter((directive) => {
    if (kind === "panic")
      return directive.value !== "runtime-error" && !containsCode(result, directive.value);
    return !containsLocatedCode(result, testCase.path, directive.line!, directive.value);
  });
  return missing.length
    ? `${testCase.name}: missing ${missing.map(({ line, value }) => `${value} at line ${line}`).join(", ")}`
    : undefined;
}

async function main(): Promise<number> {
  const options = parseOptions(process.argv.slice(2));
  let passed = true;
  if (options.suite !== "fixtures") passed = await runConformance(options);
  // test/fixtures cases have no phase or tier; --phase and --tier select conformance cases only.
  // --only names conformance cases, so the fixtures run nothing to select.
  if (
    !options.phase &&
    !options.tier &&
    !options.changed &&
    options.only.length === 0 &&
    options.suite !== "conformance"
  ) {
    const cases = await Promise.all((await fixturePaths(fixtureRoot)).map(readFixtureCase));
    const implementation = options.inProcess
      ? createAdapter({ jobs: options.jobs })
      : options.command;
    const problems = await mapParallel(cases, options.jobs, (testCase) =>
      runFixtureCase(implementation, testCase),
    );
    if (options.inProcess) await (implementation as Adapter).close();
    const failures = problems.filter((problem): problem is string => Boolean(problem));
    if (failures.length) {
      console.error(failures.join("\n\n"));
      console.error(`fixture tests: ${failures.length} of ${cases.length} failed`);
      passed = false;
    } else console.log(`fixture tests: ${cases.length} passed`);
  }
  return passed ? 0 : 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) process.exitCode = await main();

export {
  failVerdicts,
  parseOptions,
  passPaths,
  runConformanceOnce,
  selectionManifest,
  timeoutPaths,
};
export type { CaseVerdict, ConformanceResult, Options };
