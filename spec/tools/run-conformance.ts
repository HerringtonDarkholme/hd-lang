// Implementation-neutral conformance runner.
//
// It implements the rules in spec/conformance/README.md (Case Selection,
// Judging a Case, Runtime Execution, Fixture Environments, Command Contract).
// It imports only Node built-ins and reads only files under spec/ plus the
// paths given on the command line. It never imports an implementation.
//
// Usage:
//   node --experimental-strip-types spec/tools/run-conformance.ts [options]
//
// Options:
//   --compiler "CMD"   implementation command prefix (default: $HD_TEST_COMMAND,
//                      else `node --experimental-strip-types bin/hd.js`)
//   --manifest PATH    selection manifest: one case path per line. Only the
//                      first tab-separated field of a line is read, blank lines
//                      and `#` lines are ignored, and a first line whose first
//                      field is `path` is a header. Unlisted cases are not run.
//   --jobs N           parallel cases (default: $HD_TEST_JOBS, else min(8, cpus))
//   --phase PHASE      run only cases of phase parse, type, or runtime
//   --cases PATH       case index (default: spec/conformance/cases.tsv)
//   --root DIR         directory the index paths are relative to
//                      (default: the directory of the case index)
//
// Exit status: 0 when every selected case passes, 1 when any fails, 2 on a
// usage or index error.
import { spawn } from "node:child_process";
import { readdir, readFile, realpath } from "node:fs/promises";
import { availableParallelism } from "node:os";
import { dirname, resolve } from "node:path";

type Phase = "parse" | "runtime" | "type";

interface Options {
  readonly cases: string;
  readonly command: readonly string[];
  readonly jobs: number;
  readonly manifest?: string;
  readonly phase?: Phase;
  readonly root: string;
}

interface IndexRow {
  readonly expectation: string;
  readonly path: string;
  readonly phase: Phase;
}

interface Fixture {
  readonly expectHeader?: string;
  readonly expectedStdout?: string;
  readonly markerLine?: number;
  readonly packageRole?: string;
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
}

const specRoot = resolve(import.meta.dirname, "..");
const defaultCases = resolve(specRoot, "conformance/cases.tsv");
const defaultCommand = "node --experimental-strip-types bin/hd.js";
const timeoutMs = 10_000;
const indexHeader = "path\tphase\texpectation\tspecification";
const packageRoles = new Set(["library", "root-application"]);
const headerDirectives = new Set([
  "test",
  "expect",
  "fixture-runtime-profile",
  "fixture-runtime-scenario",
  "fixture-runtime-pending-function",
  "fixture-package-role",
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
  let jobs = Number(process.env.HD_TEST_JOBS ?? Math.min(8, availableParallelism()));
  let cases = defaultCases;
  let manifest: string | undefined;
  let phase: Phase | undefined;
  let root: string | undefined;
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index];
    const value = args[index + 1];
    if (value === undefined) throw new UsageError(`missing value for ${option}`);
    if (option === "--compiler") command = splitCommand(value);
    else if (option === "--jobs") jobs = Number(value);
    else if (option === "--manifest") manifest = resolve(value);
    else if (option === "--cases") cases = resolve(value);
    else if (option === "--root") root = resolve(value);
    else if (option === "--phase" && /^(parse|type|runtime)$/.test(value)) phase = value as Phase;
    else throw new UsageError(`invalid option ${option} ${value}`);
  }
  if (command.length === 0) throw new UsageError("compiler command must not be empty");
  if (!Number.isInteger(jobs) || jobs < 1) throw new UsageError("jobs must be a positive integer");
  return { cases, command, jobs, manifest, phase, root: root ?? dirname(cases) };
}

async function readPanicCategories(): Promise<Set<string>> {
  const chapter = await readFile(resolve(specRoot, "06-control-flow.md"), "utf8");
  const sentence = /Stable panic categories are exactly([^.]*)\./.exec(chapter);
  if (!sentence) throw new UsageError("spec/06-control-flow.md lists no stable panic categories");
  return new Set([...sentence[1]!.matchAll(/`([a-z0-9-]+)`/g)].map((match) => match[1]!));
}

async function readIndex(path: string): Promise<Map<string, IndexRow>> {
  const lines = (await readFile(path, "utf8")).split("\n").filter((line) => line !== "");
  if (lines.shift() !== indexHeader) throw new UsageError(`${path}: invalid header`);
  const rows = new Map<string, IndexRow>();
  for (const line of lines) {
    const fields = line.split("\t");
    const [casePath, phase, expectation] = fields;
    if (
      fields.length !== 4 ||
      fields.some((field) => field === "") ||
      !/^(parse|type|runtime)$/.test(phase!) ||
      !/^(accept|(reject|warn|panic):[a-z0-9-]+)$/.test(expectation!)
    )
      throw new UsageError(`${path}: invalid row: ${line}`);
    if (rows.has(casePath!)) throw new UsageError(`${path}: duplicate row for ${casePath}`);
    rows.set(casePath!, { expectation: expectation!, path: casePath!, phase: phase as Phase });
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
    return `'${row.expectation.slice(6)}' is not a stable panic category (spec/06-control-flow.md#runtime-panics)`;
  const expect = headers.get("expect");
  const expectPhase = { accept: "type", parse: "parse", test: "runtime" }[expect ?? ""];
  if (expect !== undefined && (row.expectation !== "accept" || expectPhase !== row.phase))
    return `'# expect: ${expect}' disagrees with index row ${row.phase} ${row.expectation}`;
  const scenario = headers.get("fixture-runtime-scenario");
  const pendingFunction = headers.get("fixture-runtime-pending-function");
  if (pendingFunction !== undefined && scenario !== "cancellation-cleanup")
    return "'# fixture-runtime-pending-function' requires the cancellation-cleanup scenario";
  const packageRole = headers.get("fixture-package-role");
  if (packageRole !== undefined && !packageRoles.has(packageRole))
    return `unknown package role '${packageRole}'`;
  const profile = headers.get("fixture-runtime-profile");
  if (stdoutLines.length > 0) {
    if (row.phase !== "runtime" || row.expectation !== "accept")
      return "'# expect-stdout' is valid only in a runtime accept case";
    if (profile !== undefined || scenario !== undefined)
      return "'# expect-stdout' requires the console profile and no scenario";
  }
  return {
    expectHeader: expect,
    expectedStdout: stdoutLines.length
      ? stdoutLines.map((text) => `${text}\n`).join("")
      : undefined,
    markerLine: markers[0]?.line,
    packageRole,
    pendingFunction,
    profile,
    scenario,
  };
}

function invoke(
  command: readonly string[],
  action: string,
  options: readonly string[],
  file: string,
): Promise<CommandResult> {
  return new Promise((complete) => {
    // No cwd: the working directory is not part of the contract.
    const child = spawn(command[0]!, [...command.slice(1), action, ...options, file], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    let error: string | undefined;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => (stdout += chunk));
    child.stderr.setEncoding("utf8").on("data", (chunk: string) => (stderr += chunk));
    child.on("error", (cause) => (error = cause.message));
    child.on("close", (status, signal) => {
      clearTimeout(timer);
      complete({ error, signal: signal ?? undefined, status, stderr, stdout, timedOut });
    });
  });
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

async function locatedDiagnostics(result: CommandResult, file: string): Promise<Located[]> {
  const target = await realPathOf(file);
  const located: Located[] = [];
  for (const text of outputLines(result)) {
    const match = /^(.+?):(\d+):(\d+): (?:(warning): )?([a-z0-9-]+):(?:\s|$)/.exec(text);
    if (!match) continue;
    const reported = await realPathOf(match[1]!);
    located.push({
      code: match[5]!,
      line: Number(match[2]),
      path: match[1]!,
      sameFile: target !== undefined && reported === target,
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
): Promise<string | undefined> {
  if (result.error) return `could not start the implementation: ${result.error}`;
  if (result.timedOut) return `ran longer than ${timeoutMs / 1000} s`;
  if (result.signal) return `terminated by signal ${result.signal}`;
  if (result.status !== 0 && result.status !== 1) return `exit status ${result.status}`;
  if (result.status === 1) {
    const located = (await locatedDiagnostics(result, file)).some((entry) => entry.sameFile);
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
): Promise<string | undefined> {
  const [kind, code] = row.expectation.split(":") as [string, string];
  const located = await locatedDiagnostics(result, file);
  if (kind === "warn") {
    if (result.status !== 0) return `expected exit 0 with warning ${code}, got exit 1`;
    const found = located.some(
      (entry) => entry.warning && entry.sameFile && entry.code === code && entry.line === line,
    );
    return found ? undefined : `no located warning ${code} on line ${line}`;
  }
  if (result.status !== 1) return `expected rejection ${code}, got exit 0`;
  const errors = located.filter((entry) => !entry.warning);
  const marked = (entry: Located): boolean =>
    entry.sameFile && entry.code === code && entry.line === line;
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
// --dependency NAME=DIR per directory under packages/, in ascending name order.
async function packageOptions(options: Options, role: string | undefined): Promise<string[]> {
  if (role === undefined) return [];
  const directory = resolve(options.root, "packages");
  const entries = await readdir(directory, { withFileTypes: true }).catch(() => []);
  const names = entries
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
  return [
    "--package-role",
    role,
    ...names.flatMap((name) => ["--dependency", `${name}=${resolve(directory, name)}`]),
  ];
}

async function runCase(options: Options, row: IndexRow, panics: Set<string>): Promise<Verdict> {
  const file = resolve(options.root, row.path);
  let source: string;
  try {
    source = await readFile(file, "utf8");
  } catch {
    return { path: row.path, reason: `cannot read fixture ${file}` };
  }
  const fixture = readFixture(source, row, panics);
  if (typeof fixture === "string") return { path: row.path, reason: fixture };
  const profile = [
    ...(fixture.profile ? ["--profile", fixture.profile] : []),
    ...(await packageOptions(options, fixture.packageRole)),
  ];
  const fail = (reason: string, results: readonly CommandResult[]): Verdict => ({
    output: snippet(results),
    path: row.path,
    reason,
  });

  if (row.phase !== "runtime") {
    const action = row.phase === "parse" ? "parse" : "check";
    const result = await invoke(options.command, action, action === "check" ? profile : [], file);
    const violation = await contractViolation(result, file, panics);
    if (violation) return fail(`${action}: ${violation}`, [result]);
    if (row.expectation === "accept")
      return result.status === 0
        ? { path: row.path }
        : fail(`${action}: expected exit 0, got exit 1`, [result]);
    if (row.expectation.startsWith("panic:"))
      return { path: row.path, reason: `${row.phase}-phase case cannot expect a panic` };
    const problem = await judgeRejectOrWarn(result, file, row, fixture.markerLine!);
    return problem ? fail(`${action}: ${problem}`, [result]) : { path: row.path };
  }

  if (row.expectation.startsWith("reject:") || row.expectation.startsWith("warn:"))
    return { path: row.path, reason: `runtime case cannot expect ${row.expectation}` };
  const checked = await invoke(options.command, "check", profile, file);
  const checkViolation = await contractViolation(checked, file, panics);
  if (checkViolation) return fail(`check: ${checkViolation}`, [checked]);
  if (checked.status !== 0) return fail("check: runtime case did not type-check", [checked]);
  const testOptions = [
    ...profile,
    ...(fixture.scenario ? ["--scenario", fixture.scenario] : []),
    ...(fixture.pendingFunction ? ["--pending-function", fixture.pendingFunction] : []),
  ];
  const tested = await invoke(options.command, "test", testOptions, file);
  const testViolation = await contractViolation(tested, file, panics);
  if (testViolation) return fail(`test: ${testViolation}`, [checked, tested]);
  if (row.expectation === "accept") {
    if (tested.status !== 0) return fail("test: expected exit 0, got exit 1", [tested]);
    if (fixture.expectedStdout === undefined) return { path: row.path };
    const ran = await invoke(options.command, "run", [], file);
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
  let panics: Set<string>;
  let selected: string[];
  try {
    options = parseOptions(process.argv.slice(2));
    index = await readIndex(options.cases);
    panics = await readPanicCategories();
    selected = options.manifest ? await readManifest(options.manifest) : [...index.keys()];
  } catch (error) {
    console.error(`run-conformance: ${(error as Error).message}`);
    return 2;
  }
  const verdicts: Verdict[] = [];
  const rows: IndexRow[] = [];
  for (const path of selected) {
    const row = index.get(path);
    if (!row) verdicts.push({ path, reason: "selected case is not in the case index" });
    else if (!options.phase || row.phase === options.phase) rows.push(row);
  }
  verdicts.push(...(await mapParallel(rows, options.jobs, (row) => runCase(options, row, panics))));
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
  console.log(
    `conformance: ${verdicts.length - failed} passed, ${failed} failed, ${verdicts.length} selected`,
  );
  return failed ? 1 : 0;
}

process.exitCode = await main();
