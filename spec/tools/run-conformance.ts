// Implementation-neutral conformance runner.
//
// It implements the rules in spec/conformance/README.md (Case Selection,
// Judging a Case, Runtime Execution, Fixture Environments, Command Contract).
// It imports only Node built-ins and reads only files under spec/ plus the
// paths given on the command line. It never imports an implementation, except
// the adapter module that `--adapter` names (spec/tools/README.md, Adapters).
//
// Usage:
//   node --experimental-strip-types spec/tools/run-conformance.ts [options]
//
// Options:
//   --compiler "CMD"   implementation command prefix (default: $HD_TEST_COMMAND,
//                      else `node --experimental-strip-types bin/hd.js`)
//   --adapter MODULE   run each command through MODULE's `createAdapter`
//                      in this process instead of spawning --compiler
//   --manifest PATH    selection manifest: one case path per line. Only the
//                      first tab-separated field of a line is read, blank lines
//                      and `#` lines are ignored, and a first line whose first
//                      field is `path` is a header. Unlisted cases are not run.
//   --jobs N           parallel cases (default: $HD_TEST_JOBS, else min(8, cpus))
//   --phase PHASE      run only cases of phase parse, type, or runtime
//   --tier TIER        run only cases of tier language, std, or cli (README,
//                      Tiers): a case whose specification cites a std/ path is
//                      std, a CLI-tier case is cli, every other case is
//                      language (default: every tier)
//   --cases PATH       case index (default: spec/conformance/cases.tsv)
//   --cli-cases PATH   CLI-tier case index (default: cli-cases.tsv beside the
//                      default case index; none when --cases is given)
//   --root DIR         directory the index paths are relative to
//                      (default: the directory of the case index)
//
// Exit status: 0 when every selected case passes, 1 when any fails, 2 on a
// usage or index error.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { cp, mkdtemp, readdir, readFile, realpath, rm } from "node:fs/promises";
import { availableParallelism, tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

type Phase = "parse" | "runtime" | "type";
type Tier = "cli" | "language" | "std";

interface Options {
  readonly adapter?: string;
  readonly cases: string;
  readonly cliCases?: string;
  readonly command: readonly string[];
  readonly jobs: number;
  readonly manifest?: string;
  readonly phase?: Phase;
  readonly root: string;
  readonly tier?: Tier;
}

interface IndexRow {
  readonly expectation: string;
  readonly path: string;
  readonly phase: Phase;
  readonly tier: Tier;
}

/** One row of the CLI-tier index (README, CLI Cases). */
interface CliRow {
  readonly name: string;
  /** `cli/NAME`, the case's path under the conformance directory. */
  readonly path: string;
  readonly tier: "cli";
}

/** One `run:` line of an `expect.txt`, with the assertions that follow it. */
interface CliStep {
  readonly command: string;
  readonly exit: number;
  readonly files: readonly string[];
  readonly noFiles: readonly string[];
  readonly stderrJson?: readonly unknown[];
  readonly stdout?: string;
  readonly stdoutJson?: readonly unknown[];
}

interface Fixture {
  readonly expectHeader?: string;
  readonly expectedStdout?: string;
  readonly markerLine?: number;
  readonly packageRole?: string;
  readonly testLayout?: string;
  /** `# fixture-package-tree: TREE/PATH` (README, Package Trees). */
  readonly packageTree?: { readonly tree: string; readonly path: string };
  readonly pendingFunction?: string;
  readonly profile?: string;
  readonly scenario?: string;
}

interface CommandResult {
  readonly error?: string;
  readonly signal?: string;
  readonly status: number | null;
  readonly stderr: string;
  readonly stdout: string;
  readonly timedOut: boolean;
}

/**
 * What a fixture's directives select for a command (README, Fixture
 * Environments). The runner hands them to the implementation's adapter, never
 * as command-line options, so an implementation need not give them a spelling.
 */
interface RunnerOptions {
  readonly profile?: string;
  readonly scenario?: string;
  readonly pendingFunction?: string;
  readonly packageRole?: string;
  /** One entry per directory under `packages/`, in ascending name order. */
  readonly dependencies?: readonly { readonly name: string; readonly directory: string }[];
  readonly testLayout?: string;
  readonly packageTree?: { readonly directory: string; readonly path: string };
}

/**
 * How the runner runs `IMPL ARGS...`: a spawned command, or an adapter
 * module's in-process implementation (spec/tools/README.md, Adapters).
 */
interface Implementation {
  run(
    args: readonly string[],
    timeoutMs: number,
    cwd?: string,
    options?: RunnerOptions,
  ): Promise<CommandResult>;
  close(): Promise<void>;
}

interface Located {
  readonly code: string;
  readonly line: number;
  readonly path: string;
  readonly sameFile: boolean;
  readonly text: string;
  readonly warning: boolean;
}

interface Verdict {
  readonly output?: string;
  readonly path: string;
  readonly reason?: string;
  readonly tier?: Tier;
}

const specRoot = resolve(import.meta.dirname, "..");
const defaultCases = resolve(specRoot, "conformance/cases.tsv");
const defaultCliCases = resolve(specRoot, "conformance/cli-cases.tsv");
const cliIndexHeader = "case\trules";
const defaultCommand = "node --experimental-strip-types bin/hd.js";
const timeoutMs = 10_000;
const indexHeader = "path\tphase\texpectation\tspecification";
const packageRoles = new Set(["library", "root-application"]);
const testLayouts = new Set(["test-module", "integration"]);
const headerDirectives = new Set([
  "test",
  "expect",
  "fixture-runtime-profile",
  "fixture-runtime-scenario",
  "fixture-runtime-pending-function",
  "fixture-package-role",
  "fixture-test-layout",
  "fixture-package-tree",
  "expect-empty-stdout",
]);
const runtimeScenarios = new Set([
  "cancellation-cleanup",
  "competing-drivers",
  "reentrant-poll",
  "pending-first-poll",
]);

class UsageError extends Error {}

function splitCommand(value: string): string[] {
  const parts: string[] = [];
  const pattern = /"([^"]*)"|'([^']*)'|([^\s]+)/g;
  for (const match of value.matchAll(pattern)) parts.push(match[1] ?? match[2] ?? match[3]!);
  return parts;
}

function parseOptions(args: readonly string[]): Options {
  let command = splitCommand(process.env.HD_TEST_COMMAND ?? defaultCommand);
  let adapter: string | undefined;
  let jobs = Number(process.env.HD_TEST_JOBS ?? Math.min(8, availableParallelism()));
  let cases = defaultCases;
  let cliCases: string | undefined = defaultCliCases;
  let manifest: string | undefined;
  let phase: Phase | undefined;
  let root: string | undefined;
  let tier: Tier | undefined;
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index];
    const value = args[index + 1];
    if (value === undefined) throw new UsageError(`missing value for ${option}`);
    if (option === "--compiler") command = splitCommand(value);
    else if (option === "--adapter") adapter = resolve(value);
    else if (option === "--jobs") jobs = Number(value);
    else if (option === "--manifest") manifest = resolve(value);
    else if (option === "--cases") {
      cases = resolve(value);
      if (cliCases === defaultCliCases) cliCases = undefined;
    } else if (option === "--cli-cases") cliCases = resolve(value);
    else if (option === "--root") root = resolve(value);
    else if (option === "--phase" && /^(parse|type|runtime)$/.test(value)) phase = value as Phase;
    else if (option === "--tier" && /^(language|std|cli)$/.test(value)) tier = value as Tier;
    else throw new UsageError(`invalid option ${option} ${value}`);
  }
  if (command.length === 0) throw new UsageError("compiler command must not be empty");
  if (!Number.isInteger(jobs) || jobs < 1) throw new UsageError("jobs must be a positive integer");
  // A CLI case runs the command in another directory, so a relative path to a
  // file in the command, such as `bin/hd.js`, must name that file absolutely.
  command = command.map((word) => (word.includes("/") && existsSync(word) ? resolve(word) : word));
  return {
    adapter,
    cases,
    cliCases,
    command,
    jobs,
    manifest,
    phase,
    root: root ?? dirname(cases),
    tier,
  };
}

async function readPanicCategories(): Promise<Set<string>> {
  const chapter = await readFile(resolve(specRoot, "lang/06-control-flow.md"), "utf8");
  const sentence = /Stable panic categories are exactly([^.]*)\./.exec(chapter);
  if (!sentence)
    throw new UsageError("spec/lang/06-control-flow.md lists no stable panic categories");
  return new Set([...sentence[1]!.matchAll(/`([a-z0-9-]+)`/g)].map((match) => match[1]!));
}

async function readIndex(path: string): Promise<Map<string, IndexRow>> {
  const lines = (await readFile(path, "utf8")).split("\n").filter((line) => line !== "");
  if (lines.shift() !== indexHeader) throw new UsageError(`${path}: invalid header`);
  const rows = new Map<string, IndexRow>();
  for (const line of lines) {
    const fields = line.split("\t");
    const [casePath, phase, expectation, specification] = fields;
    if (
      fields.length !== 4 ||
      fields.some((field) => field === "") ||
      !/^(parse|type|runtime)$/.test(phase!) ||
      !/^(accept|(reject|warn|panic):[a-z0-9-]+)$/.test(expectation!)
    )
      throw new UsageError(`${path}: invalid row: ${line}`);
    if (rows.has(casePath!)) throw new UsageError(`${path}: duplicate row for ${casePath}`);
    rows.set(casePath!, {
      expectation: expectation!,
      path: casePath!,
      phase: phase as Phase,
      tier: specification!.startsWith("std/") ? "std" : "language",
    });
  }
  return rows;
}

async function readManifest(path: string): Promise<string[]> {
  const entries: string[] = [];
  const lines = (await readFile(path, "utf8")).split("\n");
  for (const [index, raw] of lines.entries()) {
    const line = raw.replace(/\r$/, "");
    if (line.trim() === "" || line.startsWith("#")) continue;
    const first = line.split("\t", 1)[0]!.trim();
    if (index === 0 && first === "path") continue;
    if (!entries.includes(first)) entries.push(first);
  }
  return entries;
}

// Decodes the TEXT of one `# expect-stdout:` line (README, Standard Output).
// Returns undefined for an invalid escape or trailing whitespace.
function decodeStdoutLine(text: string): string | undefined {
  if (/\s$/.test(text)) return undefined;
  let decoded = "";
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index]!;
    if (char !== "\\") {
      decoded += char;
      continue;
    }
    const next = text[index + 1];
    if (next === "\\") decoded += "\\";
    else if (next === "t") decoded += "\t";
    else if (next === "u") {
      const escape = /^\{([0-9A-Fa-f]{1,6})\}/.exec(text.slice(index + 2));
      const scalar = escape ? Number.parseInt(escape[1]!, 16) : Number.NaN;
      if (!escape || scalar > 0x10ffff || (scalar >= 0xd800 && scalar <= 0xdfff)) return undefined;
      decoded += String.fromCodePoint(scalar);
      index += escape[0].length;
    } else return undefined;
    index += 1;
  }
  return decoded;
}

// Reads the fixture's directives. Returns a string when the fixture does not
// agree with its index row; such a case fails without running.
function readFixture(source: string, row: IndexRow, panics: Set<string>): Fixture | string {
  const headers = new Map<string, string>();
  const markers: Array<{ expectation: string; line: number }> = [];
  const stdoutLines: string[] = [];
  for (const [index, line] of source.split("\n").entries()) {
    const stdout = /^# expect-stdout:(?: (.*))?$/.exec(line);
    if (stdout) {
      const decoded = decodeStdoutLine(stdout[1] ?? "");
      if (decoded === undefined) return `invalid '# expect-stdout:' text on line ${index + 1}`;
      stdoutLines.push(decoded);
      continue;
    }
    const header = /^# ([a-z-]+): (.*?)\s*$/.exec(line);
    if (header && headerDirectives.has(header[1]!)) {
      if (headers.has(header[1]!)) return `header directive '${header[1]}' appears twice`;
      headers.set(header[1]!, header[2]!);
    }
    const marker = /# (diagnostic|warning|panic): ([a-z0-9-]+)\s*$/.exec(line);
    if (!marker) continue;
    const kind = marker[1] === "diagnostic" ? "reject" : marker[1] === "warning" ? "warn" : "panic";
    markers.push({ expectation: `${kind}:${marker[2]}`, line: index + 1 });
  }
  if (row.expectation === "accept" && markers.length > 0)
    return `accept case has a line marker (${markers[0]!.expectation} on line ${markers[0]!.line})`;
  if (row.expectation !== "accept") {
    if (markers.length !== 1)
      return `expected exactly one line marker for ${row.expectation}, found ${markers.length}`;
    if (markers[0]!.expectation !== row.expectation)
      return `marker ${markers[0]!.expectation} disagrees with index expectation ${row.expectation}`;
  }
  if (row.expectation.startsWith("panic:") && !panics.has(row.expectation.slice(6)))
    return `'${row.expectation.slice(6)}' is not a stable panic category (spec/lang/06-control-flow.md#runtime-panics)`;
  const expect = headers.get("expect");
  const expectPhase = { accept: "type", parse: "parse", test: "runtime" }[expect ?? ""];
  if (expect !== undefined && (row.expectation !== "accept" || expectPhase !== row.phase))
    return `'# expect: ${expect}' disagrees with index row ${row.phase} ${row.expectation}`;
  const scenario = headers.get("fixture-runtime-scenario");
  if (scenario !== undefined && !runtimeScenarios.has(scenario))
    return `unknown runtime scenario '${scenario}'`;
  if (scenario === "pending-first-poll") {
    if (row.phase !== "runtime" || row.expectation !== "accept")
      return "the pending-first-poll scenario is valid only in a runtime accept case";
    if (headers.has("fixture-runtime-profile"))
      return "the pending-first-poll scenario names no runtime profile";
  }
  const pendingFunction = headers.get("fixture-runtime-pending-function");
  if (pendingFunction !== undefined && scenario !== "cancellation-cleanup")
    return "'# fixture-runtime-pending-function' requires the cancellation-cleanup scenario";
  const packageRole = headers.get("fixture-package-role");
  if (packageRole !== undefined && !packageRoles.has(packageRole))
    return `unknown package role '${packageRole}'`;
  const testLayout = headers.get("fixture-test-layout");
  if (testLayout !== undefined && !testLayouts.has(testLayout))
    return `unknown test layout '${testLayout}'`;
  if (testLayout !== undefined && packageRole !== undefined)
    return "'# fixture-test-layout' names no package role";
  const treeHeader = headers.get("fixture-package-tree");
  const treeMatch =
    treeHeader === undefined
      ? undefined
      : /^([a-z0-9][a-z0-9-]*)\/((?:src|tests)\/[^\s]+\.hd)$/.exec(treeHeader);
  if (treeHeader !== undefined && !treeMatch)
    return `'# fixture-package-tree: ${treeHeader}' is not TREE/PATH with PATH under src/ or tests/`;
  if (treeHeader !== undefined && (packageRole !== undefined || testLayout !== undefined))
    return "'# fixture-package-tree' names no package role and no test layout";
  const profile = headers.get("fixture-runtime-profile");
  const emptyStdout = headers.get("expect-empty-stdout");
  if (emptyStdout !== undefined && emptyStdout !== "true")
    return "'# expect-empty-stdout' must have the value 'true'";
  if (emptyStdout !== undefined && stdoutLines.length > 0)
    return "'# expect-empty-stdout' and '# expect-stdout' exclude each other";
  if (stdoutLines.length > 0 || emptyStdout !== undefined) {
    if (row.phase !== "runtime" || row.expectation !== "accept")
      return "'# expect-stdout' is valid only in a runtime accept case";
    if (profile !== undefined || scenario !== undefined)
      return "'# expect-stdout' requires the console profile and no scenario";
  }
  return {
    expectHeader: expect,
    expectedStdout: stdoutLines.length
      ? stdoutLines.map((text) => `${text}\n`).join("")
      : emptyStdout !== undefined
        ? ""
        : undefined,
    markerLine: markers[0]?.line,
    packageRole,
    testLayout,
    ...(treeMatch ? { packageTree: { tree: treeMatch[1]!, path: treeMatch[2]! } } : {}),
    pendingFunction,
    profile,
    scenario,
  };
}

/** Spawns `COMMAND ARGS...` for each run. */
function spawnImplementation(command: readonly string[]): Implementation {
  return {
    run(args, limitMs, cwd, options) {
      return new Promise((complete) => {
        // A spawned command has no adapter to take runner options, and the
        // contract has no command-line spelling for them.
        if (options && Object.keys(options).length > 0)
          return complete({
            error: "the case selects runner options, which only an --adapter can receive",
            status: null,
            stderr: "",
            stdout: "",
            timedOut: false,
          });
        // A language case sets no cwd: its working directory is not part of
        // the contract. A CLI case runs in its own directory.
        const child = spawn(command[0]!, [...command.slice(1), ...args], {
          cwd,
          stdio: ["ignore", "pipe", "pipe"],
        });
        let stdout = "";
        let stderr = "";
        let timedOut = false;
        let error: string | undefined;
        const timer = setTimeout(() => {
          timedOut = true;
          child.kill("SIGKILL");
        }, limitMs);
        child.stdout.setEncoding("utf8").on("data", (chunk: string) => (stdout += chunk));
        child.stderr.setEncoding("utf8").on("data", (chunk: string) => (stderr += chunk));
        child.on("error", (cause) => (error = cause.message));
        child.on("close", (status, signal) => {
          clearTimeout(timer);
          complete({ error, signal: signal ?? undefined, status, stderr, stdout, timedOut });
        });
      });
    },
    async close() {},
  };
}

/**
 * Loads an adapter module: it exports `createAdapter({ jobs })`, whose
 * `run(args, timeoutMs)` resolves to `{ status, stdout, stderr, timedOut }`
 * and whose `close()` releases it.
 */
async function adapterImplementation(module: string, jobs: number): Promise<Implementation> {
  const { createAdapter } = (await import(pathToFileURL(module).href)) as {
    createAdapter?: (options: { jobs: number }) => Implementation;
  };
  if (typeof createAdapter !== "function")
    throw new UsageError(`${module} exports no createAdapter function`);
  return createAdapter({ jobs });
}

/**
 * Runs `IMPL ACTION [OPTION]... FILE`, or `IMPL FILE` when `action` is
 * undefined. `runner` goes to the adapter, not to the command line; `flags`
 * are the command's own options.
 */
function invoke(
  implementation: Implementation,
  action: string | undefined,
  flags: readonly string[],
  file: string,
  runner: RunnerOptions = {},
): Promise<CommandResult> {
  return implementation.run(
    [...(action === undefined ? [] : [action]), ...flags, file],
    timeoutMs,
    undefined,
    runner,
  );
}

const realpathCache = new Map<string, Promise<string | undefined>>();

function realPathOf(path: string): Promise<string | undefined> {
  const absolute = resolve(path);
  let cached = realpathCache.get(absolute);
  if (!cached) {
    cached = realpath(absolute).catch(() => undefined);
    realpathCache.set(absolute, cached);
  }
  return cached;
}

function outputLines(result: CommandResult): string[] {
  return `${result.stdout}\n${result.stderr}`
    .split("\n")
    .map((line) => line.replace(/\r$/, ""))
    .filter((line) => line !== "");
}

// In a package tree case, a diagnostic in any tree file also counts as the
// fixture's own (README, Package Trees).
async function locatedDiagnostics(
  result: CommandResult,
  file: string,
  tree?: string,
): Promise<Located[]> {
  const target = await realPathOf(file);
  const treeRoot = tree === undefined ? undefined : await realPathOf(tree);
  const located: Located[] = [];
  for (const text of outputLines(result)) {
    const match = /^(.+?):(\d+):(\d+): (?:(warning): )?([a-z0-9-]+):(?:\s|$)/.exec(text);
    if (!match) continue;
    const reported = await realPathOf(match[1]!);
    located.push({
      code: match[5]!,
      line: Number(match[2]),
      path: match[1]!,
      sameFile:
        (target !== undefined && reported === target) ||
        (treeRoot !== undefined && reported !== undefined && reported.startsWith(treeRoot + sep)),
      text,
      warning: match[4] === "warning",
    });
  }
  return located;
}

function panicReports(result: CommandResult, panics: Set<string>): string[] {
  const reports: string[] = [];
  for (const text of outputLines(result)) {
    const match = /^(?:.+?:\d+:\d+: )?([a-z0-9-]+):(?:\s|$)/.exec(text);
    if (match && panics.has(match[1]!)) reports.push(match[1]!);
  }
  return reports;
}

// Applies the Command Contract's exit rules. Returns a failure reason or
// undefined when the result is a well-formed 0 or 1.
async function contractViolation(
  result: CommandResult,
  file: string,
  panics: Set<string>,
  tree?: string,
): Promise<string | undefined> {
  if (result.error) return `could not start the implementation: ${result.error}`;
  if (result.timedOut) return `ran longer than ${timeoutMs / 1000} s`;
  if (result.signal) return `terminated by signal ${result.signal}`;
  if (result.status !== 0 && result.status !== 1) return `exit status ${result.status}`;
  if (result.status === 1) {
    const located = (await locatedDiagnostics(result, file, tree)).some((entry) => entry.sameFile);
    if (!located && panicReports(result, panics).length === 0)
      return "exit 1 without a located diagnostic or panic report";
  }
  return undefined;
}

function describe(entries: readonly Located[]): string {
  return entries
    .map((entry) => `${entry.warning ? "warning " : ""}${entry.code} at line ${entry.line}`)
    .join(", ");
}

async function judgeRejectOrWarn(
  result: CommandResult,
  file: string,
  row: IndexRow,
  line: number,
  tree?: string,
): Promise<string | undefined> {
  const [kind, code] = row.expectation.split(":") as [string, string];
  const located = await locatedDiagnostics(result, file, tree);
  // A package tree case judges the code, not the line (README, Package Trees).
  const onLine = (entry: Located): boolean => tree !== undefined || entry.line === line;
  if (kind === "warn") {
    if (result.status !== 0) return `expected exit 0 with warning ${code}, got exit 1`;
    const found = located.some(
      (entry) => entry.warning && entry.sameFile && entry.code === code && onLine(entry),
    );
    return found ? undefined : `no located warning ${code} on line ${line}`;
  }
  if (result.status !== 1) return `expected rejection ${code}, got exit 0`;
  const errors = located.filter((entry) => !entry.warning);
  const marked = (entry: Located): boolean =>
    entry.sameFile && entry.code === code && onLine(entry);
  if (!errors.some(marked)) {
    const near = located.filter((entry) => entry.code === code);
    return `no located error ${code} on line ${line}${near.length ? ` (found ${describe(near)})` : ""}`;
  }
  const others = errors.filter((entry) => !marked(entry));
  if (others.length) return `other located errors reported: ${describe(others)}`;
  return undefined;
}

function snippet(results: readonly CommandResult[]): string {
  const lines = results.flatMap(outputLines);
  const shown = lines.slice(0, 12);
  if (lines.length > shown.length) shown.push(`... (${lines.length - shown.length} more lines)`);
  return shown.join("\n");
}

// The package-role options (README, Package Roles): the role, then one
// dependency per directory under packages/, in ascending name order.
async function packageOptions(
  options: Options,
  role: string | undefined,
): Promise<Pick<RunnerOptions, "dependencies" | "packageRole">> {
  if (role === undefined) return {};
  const directory = resolve(options.root, "packages");
  const entries = await readdir(directory, { withFileTypes: true }).catch(() => []);
  const names = entries
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
  return {
    packageRole: role,
    dependencies: names.map((name) => ({ name, directory: resolve(directory, name) })),
  };
}

async function runCase(
  options: Options,
  implementation: Implementation,
  row: IndexRow,
  panics: Set<string>,
): Promise<Verdict> {
  const file = resolve(options.root, row.path);
  let source: string;
  try {
    source = await readFile(file, "utf8");
  } catch {
    return { path: row.path, reason: `cannot read fixture ${file}` };
  }
  const fixture = readFixture(source, row, panics);
  if (typeof fixture === "string") return { path: row.path, reason: fixture };
  const tree = fixture.packageTree && resolve(options.root, "trees", fixture.packageTree.tree);
  if (tree && (await realPathOf(tree)) === undefined)
    return { path: row.path, reason: `package tree ${tree} does not exist` };
  if (tree && (await realPathOf(resolve(tree, fixture.packageTree!.path))) !== undefined)
    return { path: row.path, reason: `package tree already holds ${fixture.packageTree!.path}` };
  const runner: RunnerOptions = {
    ...(fixture.profile ? { profile: fixture.profile } : {}),
    ...(await packageOptions(options, fixture.packageRole)),
    ...(fixture.testLayout ? { testLayout: fixture.testLayout } : {}),
    ...(tree ? { packageTree: { directory: tree, path: fixture.packageTree!.path } } : {}),
  };
  const fail = (reason: string, results: readonly CommandResult[]): Verdict => ({
    output: snippet(results),
    path: row.path,
    reason,
  });

  if (row.phase !== "runtime") {
    const action = row.phase === "parse" ? "parse" : "check";
    const result = await invoke(
      implementation,
      action,
      action === "check" ? ["--tests"] : [],
      file,
      runner,
    );
    const violation = await contractViolation(result, file, panics, tree);
    if (violation) return fail(`${action}: ${violation}`, [result]);
    if (row.expectation === "accept")
      return result.status === 0
        ? { path: row.path }
        : fail(`${action}: expected exit 0, got exit 1`, [result]);
    if (row.expectation.startsWith("panic:"))
      return { path: row.path, reason: `${row.phase}-phase case cannot expect a panic` };
    const problem = await judgeRejectOrWarn(result, file, row, fixture.markerLine!, tree);
    return problem ? fail(`${action}: ${problem}`, [result]) : { path: row.path };
  }

  if (row.expectation.startsWith("reject:") || row.expectation.startsWith("warn:"))
    return { path: row.path, reason: `runtime case cannot expect ${row.expectation}` };
  const checked = await invoke(implementation, "check", ["--tests"], file, runner);
  const checkViolation = await contractViolation(checked, file, panics, tree);
  if (checkViolation) return fail(`check: ${checkViolation}`, [checked]);
  if (checked.status !== 0) return fail("check: runtime case did not type-check", [checked]);
  // The pending-first-poll scenario must give the ordinary run's result, so
  // the ordinary run comes first (README, Runtime Scenarios).
  if (fixture.scenario === "pending-first-poll") {
    const ordinary = await invoke(implementation, "test", [], file, runner);
    const ordinaryViolation = await contractViolation(ordinary, file, panics, tree);
    if (ordinaryViolation) return fail(`test: ${ordinaryViolation}`, [checked, ordinary]);
    if (ordinary.status !== 0) return fail("test: expected exit 0, got exit 1", [ordinary]);
  }
  const testRunner: RunnerOptions = {
    ...runner,
    ...(fixture.scenario ? { scenario: fixture.scenario } : {}),
    ...(fixture.pendingFunction ? { pendingFunction: fixture.pendingFunction } : {}),
  };
  const tested = await invoke(implementation, "test", [], file, testRunner);
  const testViolation = await contractViolation(tested, file, panics, tree);
  if (testViolation) return fail(`test: ${testViolation}`, [checked, tested]);
  if (row.expectation === "accept") {
    if (tested.status !== 0) return fail("test: expected exit 0, got exit 1", [tested]);
    if (fixture.expectedStdout === undefined) return { path: row.path };
    // `IMPL FILE`: run the fixture as a single file (Command Contract).
    const ran = await invoke(implementation, undefined, [], file);
    const runViolation = await contractViolation(ran, file, panics);
    if (runViolation) return fail(`run: ${runViolation}`, [ran]);
    if (ran.status !== 0) return fail("run: expected exit 0, got exit 1", [ran]);
    if (ran.stdout !== fixture.expectedStdout)
      return fail(
        `run: stdout ${JSON.stringify(ran.stdout)} differs from expected ${JSON.stringify(fixture.expectedStdout)}`,
        [ran],
      );
    return { path: row.path };
  }
  const code = row.expectation.slice("panic:".length);
  if (tested.status !== 1) return fail(`test: expected panic ${code}, got exit 0`, [tested]);
  const reports = panicReports(tested, panics);
  if (reports.length === 0) return fail(`test: no panic report (expected ${code})`, [tested]);
  const wrong = [...new Set(reports.filter((reported) => reported !== code))];
  if (wrong.length)
    return fail(`test: reported panic category ${wrong.join(", ")}, expected ${code}`, [tested]);
  return { path: row.path };
}

async function readCliIndex(path: string): Promise<Map<string, CliRow>> {
  const lines = (await readFile(path, "utf8")).split("\n").filter((line) => line !== "");
  if (lines.shift() !== cliIndexHeader) throw new UsageError(`${path}: invalid header`);
  const rows = new Map<string, CliRow>();
  for (const line of lines) {
    const fields = line.split("\t");
    const [name, rules] = fields;
    if (fields.length !== 2 || !/^[a-z0-9][a-z0-9-]*$/.test(name!) || rules === "")
      throw new UsageError(`${path}: invalid row: ${line}`);
    if (rows.has(`cli/${name}`)) throw new UsageError(`${path}: duplicate row for ${name}`);
    rows.set(`cli/${name}`, { name: name!, path: `cli/${name}`, tier: "cli" });
  }
  return rows;
}

// Parses an `expect.txt` (README, CLI Cases). Returns a string when the file
// is invalid; such a case fails without running.
function parseCliExpect(text: string): CliStep[] | string {
  interface Draft {
    command: string;
    exit?: number;
    files: string[];
    noFiles: string[];
    stderrJson?: unknown[];
    stdoutJson?: unknown[];
    stdoutLines?: string[];
  }
  const drafts: Draft[] = [];
  const jsonLines = (value: string): unknown[] | undefined => {
    try {
      const parsed: unknown = JSON.parse(value);
      return Array.isArray(parsed) ? parsed : undefined;
    } catch {
      return undefined;
    }
  };
  for (const [index, raw] of text.split("\n").entries()) {
    const line = raw.replace(/\r$/, "");
    if (line.trim() === "" || line.startsWith("#")) continue;
    const where = `expect.txt line ${index + 1}`;
    const match = /^([a-z-]+):(?: (.*))?$/.exec(line);
    if (!match) return `${where}: expected 'KIND: VALUE'`;
    const [, kind, value = ""] = match;
    if (kind === "run") {
      if (!/^hd( \S+)*$/.test(value))
        return `${where}: 'run:' takes 'hd' and words with no quoting`;
      drafts.push({ command: value, files: [], noFiles: [] });
      continue;
    }
    const step = drafts.at(-1);
    if (!step) return `${where}: '${kind}:' comes before the first 'run:'`;
    if (kind === "exit") {
      if (step.exit !== undefined) return `${where}: a step has one 'exit:'`;
      if (!/^\d{1,3}$/.test(value) || Number(value) > 255)
        return `${where}: 'exit:' takes a status from 0 to 255`;
      step.exit = Number(value);
    } else if (kind === "stdout") {
      if (step.stdoutJson) return `${where}: 'stdout:' and 'stdout-json:' exclude each other`;
      const decoded = decodeStdoutLine(value);
      if (decoded === undefined) return `${where}: invalid 'stdout:' text`;
      (step.stdoutLines ??= []).push(decoded);
    } else if (kind === "stdout-json" || kind === "stderr-json") {
      const parsed = jsonLines(value);
      if (!parsed) return `${where}: '${kind}:' takes a JSON array`;
      if (kind === "stdout-json") {
        if (step.stdoutJson || step.stdoutLines)
          return `${where}: 'stdout-json:' appears once and excludes 'stdout:'`;
        step.stdoutJson = parsed;
      } else {
        if (step.stderrJson) return `${where}: a step has one 'stderr-json:'`;
        step.stderrJson = parsed;
      }
    } else if (kind === "file" || kind === "no-file") {
      if (value === "" || value.startsWith("/") || value.split("/").includes(".."))
        return `${where}: '${kind}:' takes a relative path with no '..'`;
      (kind === "file" ? step.files : step.noFiles).push(value);
    } else return `${where}: unknown line kind '${kind}'`;
  }
  if (drafts.length === 0) return "expect.txt has no 'run:' line";
  return drafts.map((draft) => ({
    command: draft.command,
    exit: draft.exit ?? 0,
    files: draft.files,
    noFiles: draft.noFiles,
    stderrJson: draft.stderrJson,
    stdout: draft.stdoutLines?.map((line) => `${line}\n`).join(""),
    stdoutJson: draft.stdoutJson,
  }));
}

// Splits JSON-lines output into objects. Returns a string for any other text.
function parseJsonLines(text: string): unknown[] | string {
  const lines = text.split("\n");
  if (lines.at(-1) === "") lines.pop();
  const objects: unknown[] = [];
  for (const [index, line] of lines.entries()) {
    let value: unknown;
    try {
      value = JSON.parse(line);
    } catch {
      return `line ${index + 1} is not JSON`;
    }
    if (typeof value !== "object" || value === null || Array.isArray(value))
      return `line ${index + 1} is not a JSON object`;
    objects.push(value);
  }
  return objects;
}

// The subset match of `stdout-json:` (README, CLI Cases). Returns a reason
// when `actual` does not match `expected`.
function jsonMismatch(expected: unknown, actual: unknown, where: string): string | undefined {
  if (expected === null) return undefined;
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual) || actual.length !== expected.length)
      return `${where}: expected an array of ${expected.length}, got ${JSON.stringify(actual)}`;
    for (const [index, item] of expected.entries()) {
      const mismatch = jsonMismatch(item, actual[index], `${where}[${index}]`);
      if (mismatch) return mismatch;
    }
    return undefined;
  }
  if (typeof expected === "object") {
    if (typeof actual !== "object" || actual === null || Array.isArray(actual))
      return `${where}: expected an object, got ${JSON.stringify(actual)}`;
    for (const [key, item] of Object.entries(expected)) {
      if (!(key in actual)) return `${where}: missing key '${key}'`;
      const mismatch = jsonMismatch(
        item,
        (actual as Record<string, unknown>)[key],
        `${where}.${key}`,
      );
      if (mismatch) return mismatch;
    }
    return undefined;
  }
  return expected === actual
    ? undefined
    : `${where}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`;
}

async function runCliCase(
  options: Options,
  implementation: Implementation,
  row: CliRow,
): Promise<Verdict> {
  const directory = resolve(options.root, row.path);
  let text: string;
  try {
    text = await readFile(join(directory, "expect.txt"), "utf8");
  } catch {
    return { path: row.path, reason: `cannot read ${join(directory, "expect.txt")}` };
  }
  const steps = parseCliExpect(text);
  if (typeof steps === "string") return { path: row.path, reason: steps };
  // The case runs in a copy named after the case, without expect.txt, so a
  // command may write files and a package takes its name from the directory.
  const work = await mkdtemp(join(tmpdir(), "hd-cli-"));
  const cwd = join(work, row.name);
  try {
    await cp(directory, cwd, {
      recursive: true,
      filter: (source) => source !== join(directory, "expect.txt"),
    });
    for (const [index, step] of steps.entries()) {
      const label = `step ${index + 1} (${step.command})`;
      const result = await implementation.run(step.command.split(" ").slice(1), timeoutMs, cwd);
      const fail = (reason: string): Verdict => ({
        output: snippet([result]),
        path: row.path,
        reason: `${label}: ${reason}`,
      });
      if (result.error) return fail(`could not start the implementation: ${result.error}`);
      if (result.timedOut) return fail(`ran longer than ${timeoutMs / 1000} s`);
      if (result.signal) return fail(`terminated by signal ${result.signal}`);
      if (result.status !== step.exit)
        return fail(`expected exit ${step.exit}, got exit ${result.status}`);
      if (step.stdout !== undefined && result.stdout !== step.stdout)
        return fail(
          `stdout ${JSON.stringify(result.stdout)} differs from expected ${JSON.stringify(step.stdout)}`,
        );
      for (const [stream, expected, actual] of [
        ["stdout-json", step.stdoutJson, result.stdout],
        ["stderr-json", step.stderrJson, result.stderr],
      ] as const) {
        if (expected === undefined) continue;
        const parsed = parseJsonLines(actual);
        if (typeof parsed === "string") return fail(`${stream}: ${parsed}`);
        const mismatch = jsonMismatch(expected, parsed, stream);
        if (mismatch) return fail(mismatch);
      }
      for (const file of step.files)
        if (!existsSync(join(cwd, file))) return fail(`expected file ${file} does not exist`);
      for (const file of step.noFiles)
        if (existsSync(join(cwd, file))) return fail(`file ${file} exists, and should not`);
    }
  } finally {
    await rm(work, { recursive: true, force: true });
  }
  return { path: row.path };
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

async function main(): Promise<number> {
  let options: Options;
  let index: Map<string, IndexRow>;
  let cliIndex: Map<string, CliRow>;
  let panics: Set<string>;
  let selected: string[];
  let implementation: Implementation;
  try {
    options = parseOptions(process.argv.slice(2));
    index = await readIndex(options.cases);
    cliIndex = options.cliCases ? await readCliIndex(options.cliCases) : new Map();
    panics = await readPanicCategories();
    selected = options.manifest
      ? await readManifest(options.manifest)
      : [...index.keys(), ...cliIndex.keys()];
    implementation = options.adapter
      ? await adapterImplementation(options.adapter, options.jobs)
      : spawnImplementation(options.command);
  } catch (error) {
    console.error(`run-conformance: ${(error as Error).message}`);
    return 2;
  }
  const verdicts: Verdict[] = [];
  const rows: Array<IndexRow | CliRow> = [];
  for (const path of selected) {
    const row = index.get(path) ?? cliIndex.get(path);
    if (!row) verdicts.push({ path, reason: "selected case is not in the case index" });
    else if (
      // A CLI case has no phase, so `--phase` leaves it out.
      (!options.phase || ("phase" in row && row.phase === options.phase)) &&
      (!options.tier || row.tier === options.tier)
    )
      rows.push(row);
  }
  verdicts.push(
    ...(await mapParallel(rows, options.jobs, async (row) => ({
      ...("phase" in row
        ? await runCase(options, implementation, row, panics)
        : await runCliCase(options, implementation, row)),
      tier: row.tier,
    }))),
  );
  await implementation.close();
  let failed = 0;
  for (const verdict of verdicts) {
    if (!verdict.reason) {
      console.log(`pass  ${verdict.path}`);
      continue;
    }
    failed += 1;
    console.log(`FAIL  ${verdict.path}: ${verdict.reason}`);
    if (verdict.output) console.log(verdict.output.replace(/^/gm, "      | "));
  }
  // Per-tier passes, as spec/conformance/README.md#case-selection states:
  // "language: X of Y; stdlib: X of Y; cli: X of Y".
  const tierCount = (tier: Tier): string => {
    const ran = verdicts.filter((verdict) => verdict.tier === tier);
    return `${ran.filter((verdict) => !verdict.reason).length} of ${ran.length}`;
  };
  console.log(
    `conformance: ${verdicts.length - failed} passed, ${failed} failed, ${verdicts.length} selected` +
      ` (language: ${tierCount("language")}; stdlib: ${tierCount("std")}; cli: ${tierCount("cli")})`,
  );
  return failed ? 1 : 0;
}

process.exitCode = await main();
