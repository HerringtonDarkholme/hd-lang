import { spawn } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { availableParallelism } from "node:os";
import { resolve } from "node:path";

interface CommandResult {
  readonly code: number;
  readonly stderr: string;
  readonly stdout: string;
}

interface ConformanceCase {
  readonly expectation: string;
  readonly line?: number;
  readonly path: string;
  readonly pendingFunction?: string;
  readonly phase: "parse" | "runtime" | "type";
  readonly profile?: string;
  readonly scenario?: string;
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

interface Options {
  readonly command: readonly string[];
  readonly jobs: number;
  readonly phase?: ConformanceCase["phase"];
  readonly suite: "all" | "conformance" | "fixtures";
}

const root = resolve(import.meta.dirname, "../../..");
const portableManifest = resolve(root, "audit/probes/harness/portable-cases.tsv");
const specManifest = resolve(root, "audit/probes/harness/spec-cases.tsv");
const conformanceRoot = resolve(root, "audit/probes/harness/conformance");
const fixtureRoot = resolve(root, "audit/probes/harness/fixtures");

function splitCommand(value: string): string[] {
  const parts: string[] = [];
  const pattern = /"([^"]*)"|'([^']*)'|([^\s]+)/g;
  for (const match of value.matchAll(pattern)) parts.push(match[1] ?? match[2] ?? match[3]!);
  return parts;
}

function parseOptions(args: readonly string[]): Options {
  let command = splitCommand(
    process.env.HD_TEST_COMMAND ?? "node --experimental-strip-types bin/hd.js",
  );
  let jobs = Number(process.env.HD_TEST_JOBS ?? Math.min(8, availableParallelism()));
  let phase: Options["phase"];
  let suite: Options["suite"] = "all";
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index];
    const value = args[index + 1];
    if (option === "--compiler" && value) command = splitCommand(value);
    else if (option === "--jobs" && value) jobs = Number(value);
    else if (option === "--phase" && /^(parse|type|runtime)$/.test(value ?? ""))
      phase = value as Options["phase"];
    else if (option === "--suite" && /^(all|conformance|fixtures)$/.test(value ?? ""))
      suite = value as Options["suite"];
    else throw new Error(`invalid option ${option ?? ""}`);
    index += 1;
  }
  if (command.length === 0) throw new Error("compiler command must not be empty");
  if (!Number.isInteger(jobs) || jobs < 1) throw new Error("jobs must be a positive integer");
  if (phase && suite === "fixtures") throw new Error("--phase cannot use --suite fixtures");
  return { command, jobs, phase, suite };
}

async function invoke(
  command: readonly string[],
  action: string,
  path: string,
  options: readonly string[] = [],
): Promise<CommandResult> {
  return new Promise((complete, reject) => {
    const child = spawn(command[0]!, [...command.slice(1), action, ...options, path], {
      cwd: root,
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

async function readConformanceCases(): Promise<ConformanceCase[]> {
  const lines = (await readFile(portableManifest, "utf8")).trimEnd().split("\n");
  if (lines.shift() !== "path\tphase") throw new Error(`${portableManifest} has an invalid header`);
  return Promise.all(
    lines.map(async (row) => {
      const [path, phase, ...extra] = row.split("\t");
      if (!path || !/^(parse|type|runtime)$/.test(phase ?? "") || extra.length)
        throw new Error(`${portableManifest} has an invalid row: ${row}`);
      const sourceLines = (await readFile(resolve(conformanceRoot, path), "utf8")).split("\n");
      const markers: Array<{ expectation: string; line: number }> = [];
      let pendingFunction: string | undefined;
      let profile: string | undefined;
      let scenario: string | undefined;
      for (const [index, sourceLine] of sourceLines.entries()) {
        const pendingMarker =
          /^# fixture-runtime-pending-function: ([A-Za-z_][A-Za-z0-9_]*)\s*$/.exec(sourceLine);
        if (pendingMarker) pendingFunction = pendingMarker[1];
        const profileMarker = /^# fixture-runtime-profile: ([a-z0-9-]+)\s*$/.exec(sourceLine);
        if (profileMarker) profile = profileMarker[1];
        const scenarioMarker = /^# fixture-runtime-scenario: ([a-z0-9-]+)\s*$/.exec(sourceLine);
        if (scenarioMarker) scenario = scenarioMarker[1];
        const marker = /# (diagnostic|warning|panic): ([a-z0-9-]+)\s*$/.exec(sourceLine);
        if (!marker) continue;
        const prefix =
          marker[1] === "diagnostic" ? "reject" : marker[1] === "warning" ? "warn" : "panic";
        markers.push({ expectation: `${prefix}:${marker[2]}`, line: index + 1 });
      }
      if (markers.length > 1)
        throw new Error(`${path} has more than one conformance expectation marker`);
      return {
        expectation: markers[0]?.expectation ?? "accept",
        line: markers[0]?.line,
        path,
        pendingFunction,
        phase: phase as ConformanceCase["phase"],
        profile,
        scenario,
      };
    }),
  );
}

async function runConformanceCase(
  command: readonly string[],
  testCase: ConformanceCase,
): Promise<string | undefined> {
  const path = resolve(conformanceRoot, testCase.path);
  if (testCase.phase === "runtime") {
    const profileOptions = testCase.profile ? ["--profile", testCase.profile] : [];
    const checked = await invoke(command, "check", path, profileOptions);
    if (checked.code !== 0)
      return failure(testCase.path, "runtime fixture did not type-check", checked);
    const runtimeOptions = [
      ...(testCase.scenario ? ["--scenario", testCase.scenario] : []),
      ...(testCase.pendingFunction ? ["--pending-function", testCase.pendingFunction] : []),
      ...profileOptions,
    ];
    const result = await invoke(command, "test", path, runtimeOptions);
    if (testCase.expectation === "accept")
      return result.code === 0 ? undefined : failure(testCase.path, "expected success", result);
    if (testCase.expectation.startsWith("panic:")) {
      const code = testCase.expectation.slice("panic:".length);
      if (result.code === 0) return failure(testCase.path, `expected panic ${code}`, result);
      return containsCode(result, code)
        ? undefined
        : failure(testCase.path, `missing runtime panic ${code}`, result);
    }
    return `${testCase.path}: invalid runtime expectation ${testCase.expectation}`;
  }
  const result = await invoke(command, testCase.phase === "parse" ? "parse" : "check", path);
  if (testCase.expectation === "accept")
    return result.code === 0 ? undefined : failure(testCase.path, "expected success", result);
  const [kind, code] = testCase.expectation.split(":", 2);
  if (kind === "reject") {
    if (result.code === 0) return failure(testCase.path, `expected rejection ${code}`, result);
    const found = testCase.line
      ? containsLocatedCode(result, path, testCase.line, code!)
      : containsCode(result, code!);
    return found
      ? undefined
      : failure(testCase.path, `missing ${code} at line ${testCase.line}`, result);
  }
  if (kind === "warn") {
    if (result.code !== 0) return failure(testCase.path, "expected a warning", result);
    const found = testCase.line
      ? containsLocatedCode(result, path, testCase.line, code!)
      : containsCode(result, code!);
    return found
      ? undefined
      : failure(testCase.path, `missing ${code} at line ${testCase.line}`, result);
  }
  return `${testCase.path}: invalid expectation ${testCase.expectation}`;
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
  command: readonly string[],
  testCase: FixtureCase,
): Promise<string | undefined> {
  const kinds = new Set(testCase.directives.map(({ kind }) => kind));
  if (kinds.size !== 1) return `${testCase.name}: cannot mix expectation directive kinds`;
  const kind = testCase.directives[0]!.kind;
  const profileOptions = testCase.profile ? ["--profile", testCase.profile] : [];
  if (kind === "expect-result") {
    for (const directive of testCase.directives) {
      const separator = directive.value.indexOf(" = ");
      if (separator < 1) return `${testCase.name}: expected '# expect-result: ENTRY = VALUE'`;
      const entry = directive.value.slice(0, separator);
      const expected = directive.value.slice(separator + 3);
      const result = await invoke(command, "run", testCase.path, [
        "--entry",
        entry,
        ...profileOptions,
      ]);
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
    const action =
      directive.value === "parse" ? "parse" : directive.value === "test" ? "test" : "check";
    if (!["accept", "parse", "test"].includes(directive.value))
      return `${testCase.name}: expected '# expect: accept', '# expect: parse', or '# expect: test'`;
    const result = await invoke(command, action, testCase.path, profileOptions);
    return result.code === 0 ? undefined : failure(testCase.name, "expected acceptance", result);
  }
  const checked = await invoke(command, "check", testCase.path, profileOptions);
  let result = checked;
  if (kind === "diagnostic" && checked.code === 0)
    return failure(testCase.name, "expected rejection", checked);
  if (kind === "warning" && checked.code !== 0)
    return failure(testCase.name, "expected warning, but compilation failed", checked);
  if (kind === "panic") {
    if (checked.code !== 0) return failure(testCase.name, "panic fixture did not compile", checked);
    result = await invoke(command, "run", testCase.path, profileOptions);
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
  const failures: string[] = [];
  let count = 0;
  if (options.suite !== "fixtures") {
    const cases = (await readConformanceCases()).filter(
      ({ phase }) => !options.phase || phase === options.phase,
    );
    const spec = await readFile(specManifest, "utf8");
    for (const testCase of cases)
      if (!spec.includes(`${testCase.path}\t${testCase.phase}\t${testCase.expectation}\t`))
        failures.push(`${testCase.path}: selection does not match spec/conformance/cases.tsv`);
    const problems = await mapParallel(cases, options.jobs, (testCase) =>
      runConformanceCase(options.command, testCase),
    );
    failures.push(...problems.filter((problem): problem is string => Boolean(problem)));
    count += cases.length;
  }
  if (!options.phase && options.suite !== "conformance") {
    const cases = await Promise.all((await fixturePaths(fixtureRoot)).map(readFixtureCase));
    const problems = await mapParallel(cases, options.jobs, (testCase) =>
      runFixtureCase(options.command, testCase),
    );
    failures.push(...problems.filter((problem): problem is string => Boolean(problem)));
    count += cases.length;
  }
  if (failures.length) {
    console.error(failures.join("\n\n"));
    console.error(`portable tests: ${failures.length} of ${count} failed`);
    return 1;
  }
  console.log(`portable tests: ${count} passed`);
  return 0;
}

process.exitCode = await main();
