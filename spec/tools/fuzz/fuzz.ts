// Implementation-neutral hd-lang fuzzer. Entry point.
//
//   node --experimental-strip-types spec/tools/fuzz/fuzz.ts \
//     [--compiler "<cmd>"]... [--seed S] [--cases N] [--jobs J] [--fuzzer NAME]... \
//     [--timeout MS] [--out DIR] [--max-signatures 20] [--minimize] [--adapter wasm] \
//     [--fail-on NAME,...]
//   node --experimental-strip-types spec/tools/fuzz/fuzz.ts --replay FILE [--fuzzer NAME]...
//   node --experimental-strip-types spec/tools/fuzz/fuzz.ts --reference-only --fail-on all
//
// It imports only Node built-ins, its own modules, and `spec/`. See README.md.
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { parseSource } from "../../reference-parser/parser.ts";
import { validateBuild, wasmToolsAvailable } from "./adapters/wasm.ts";
import {
  type Action,
  classify,
  hash,
  type Inventory,
  invoke,
  loadInventory,
  mapParallel,
  normalizeLine,
  type Outcome,
  outcomeLabel,
  Rng,
  specFiles,
  specRoot,
  splitCommand,
} from "./common.ts";
import { Generator, grammarLiterals, loadGrammar, render } from "./generate.ts";
import { minimize } from "./minimize.ts";
import { type Line, mutate, splitLines } from "./mutate.ts";
import { contract, contractActions } from "./oracles/contract.ts";
import { crossImpl } from "./oracles/cross-impl.ts";
import { parseAgreement } from "./oracles/parse-agreement.ts";
import { phaseConsistency } from "./oracles/phase-consistency.ts";
import type { Execution, Observation } from "./oracles/types.ts";

type FuzzerName = "contract" | "cross" | "parse" | "phase" | "wasm";
const allFuzzers: readonly FuzzerName[] = ["parse", "contract", "phase", "cross", "wasm"];

interface Options {
  readonly adapters: ReadonlySet<string>;
  readonly cases: number;
  readonly failOn: ReadonlySet<FuzzerName>;
  readonly compilers: readonly (readonly string[])[];
  readonly fuzzers: readonly FuzzerName[];
  readonly jobs: number;
  readonly maxSignatures: number;
  readonly minTests: number;
  readonly minimize: boolean;
  readonly out: string;
  readonly referenceOnly: boolean;
  readonly replay?: string;
  readonly seed: string;
  readonly timeoutMs: number;
  readonly work: string;
}

function fuzzerList(value: string): FuzzerName[] {
  return value.split(",").flatMap((name) => {
    if (name === "all") return allFuzzers;
    if ((allFuzzers as readonly string[]).includes(name)) return [name as FuzzerName];
    throw new Error(`unknown fuzzer ${name}`);
  });
}

function parseOptions(args: readonly string[]): Options {
  const compilers: string[][] = [];
  const fuzzers: FuzzerName[] = [];
  const adapters = new Set<string>();
  let cases = 5000;
  let jobs = Number(process.env.HD_TEST_JOBS ?? Math.min(8, availableParallelism()));
  let seed = "1";
  let timeoutMs = 10_000;
  let out = join(tmpdir(), `hd-fuzz-out-${process.pid}`);
  let maxSignatures = 20;
  let minTests = 300;
  let doMinimize = false;
  let referenceOnly = false;
  const failOn = new Set<FuzzerName>();
  let replay: string | undefined;
  let work = join(tmpdir(), `hd-fuzz-${process.pid}`);
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index]!;
    const value = args[index + 1];
    const need = (): string => {
      if (value === undefined) throw new Error(`${option} needs a value`);
      index += 1;
      return value;
    };
    if (option === "--compiler") compilers.push(splitCommand(need()));
    else if (option === "--seed") seed = need();
    else if (option === "--cases") cases = Number(need());
    else if (option === "--jobs") jobs = Number(need());
    else if (option === "--timeout") timeoutMs = Number(need());
    else if (option === "--out") out = resolve(need());
    else if (option === "--work") work = resolve(need());
    else if (option === "--max-signatures") maxSignatures = Number(need());
    else if (option === "--min-tests") minTests = Number(need());
    else if (option === "--replay") replay = resolve(need());
    else if (option === "--adapter") adapters.add(need());
    else if (option === "--minimize") doMinimize = true;
    else if (option === "--reference-only") referenceOnly = true;
    else if (option === "--fail-on") for (const name of fuzzerList(need())) failOn.add(name);
    else if (option === "--fuzzer") fuzzers.push(...fuzzerList(need()));
    else throw new Error(`unknown option ${option}`);
  }
  if (referenceOnly && (compilers.length > 0 || adapters.size > 0))
    throw new Error("--reference-only takes no --compiler or --adapter");
  if (compilers.length === 0 && !referenceOnly)
    compilers.push(
      splitCommand(process.env.HD_FUZZ_COMMAND ?? "node --experimental-strip-types bin/hd.js"),
    );
  if (compilers.some((command) => command.length === 0))
    throw new Error("compiler command must not be empty");
  if (!Number.isInteger(cases) || cases < 1) throw new Error("--cases must be a positive integer");
  if (!Number.isInteger(jobs) || jobs < 1) throw new Error("--jobs must be a positive integer");
  let selected = fuzzers.length ? [...new Set(fuzzers)] : [...allFuzzers];
  if (compilers.length < 2) selected = selected.filter((name) => name !== "cross");
  if (!adapters.has("wasm")) selected = selected.filter((name) => name !== "wasm");
  // Without an implementation, `phase` would repeat the `contract` inputs.
  if (referenceOnly) selected = selected.filter((name) => name === "parse" || name === "contract");
  return {
    adapters,
    cases,
    compilers,
    failOn,
    fuzzers: selected,
    jobs,
    maxSignatures,
    minTests,
    minimize: doMinimize,
    out,
    referenceOnly,
    replay,
    seed,
    timeoutMs,
    work,
  };
}

interface Input {
  readonly actions: readonly Action[];
  readonly origin: string;
  readonly source: string;
}

class Corpus {
  readonly seeds: ReadonlyArray<{ path: string; source: string }>;
  readonly accepted: ReadonlyArray<{ path: string; source: string }>;
  readonly donors: readonly Line[];
  readonly vocabulary: readonly string[];
  readonly generator: Generator;
  private readonly seed: string;

  constructor(seed: string) {
    this.seed = seed;
    const conformance = resolve(specRoot, "conformance");
    this.seeds = specFiles(conformance).map((path) => ({
      path: path.slice(conformance.length + 1),
      source: readFileSync(path, "utf8"),
    }));
    this.accepted = this.seeds.filter(({ source }) => parseSource(source).length === 0);
    this.donors = this.seeds.flatMap(({ source }) =>
      splitLines(source).filter((line) => line.tokens.length > 0),
    );
    const grammar = loadGrammar();
    this.vocabulary = grammarLiterals(grammar);
    this.generator = new Generator(grammar);
  }

  parseInput(index: number): Input {
    const rng = new Rng(`${this.seed}:parse:${index}`);
    if (rng.chance(0.5)) {
      const source = render(this.generator.derive(rng, "source_file", 20 + rng.int(140)));
      return { actions: ["parse"], origin: "generate", source };
    }
    const seed = rng.pick(this.seeds);
    const mutant = mutate(seed.source, rng, this.vocabulary, this.donors);
    return {
      actions: ["parse"],
      origin: `mutate:${seed.path}:${mutant.operators.join("+")}`,
      source: mutant.source,
    };
  }

  contractInput(index: number): Input {
    const rng = new Rng(`${this.seed}:contract:${index}`);
    const seed = rng.pick(this.accepted);
    for (let attempt = 0; attempt < 8; attempt += 1) {
      const mutant = mutate(seed.source, rng, this.vocabulary, this.donors);
      if (parseSource(mutant.source).length === 0)
        return {
          actions: contractActions,
          origin: `mutate:${seed.path}:${mutant.operators.join("+")}`,
          source: mutant.source,
        };
    }
    return { actions: contractActions, origin: `seed:${seed.path}`, source: seed.source };
  }

  input(fuzzer: FuzzerName, index: number): Input {
    if (fuzzer === "parse") return this.parseInput(index);
    if (fuzzer === "cross")
      return index % 2 === 0 ? this.parseInput(index / 2) : this.contractInput((index - 1) / 2);
    return this.contractInput(index);
  }
}

class Executor {
  private readonly outcomes = new Map<string, Promise<Outcome>>();
  private readonly files = new Map<string, string>();
  calls = 0;
  callMs = 0;
  private readonly options: Options;
  readonly inventory: Inventory;

  constructor(options: Options, inventory: Inventory) {
    this.options = options;
    this.inventory = inventory;
  }

  file(source: string): string {
    const key = hash(source);
    let path = this.files.get(key);
    if (!path) {
      const directory = join(this.options.work, key);
      mkdirSync(directory, { recursive: true });
      path = join(directory, "case.hd");
      writeFileSync(path, source);
      this.files.set(key, path);
    }
    return path;
  }

  outcome(compiler: number, action: Action, source: string): Promise<Outcome> {
    const key = `${compiler}\u0000${action}\u0000${hash(source)}`;
    let found = this.outcomes.get(key);
    if (!found) {
      const path = this.file(source);
      found = invoke(
        this.options.compilers[compiler]!,
        action,
        path,
        [],
        this.options.timeoutMs,
      ).then((result) => {
        this.calls += 1;
        this.callMs += result.ms;
        return classify(result, action, path, this.inventory);
      });
      this.outcomes.set(key, found);
    }
    return found;
  }

  async execute(source: string, actions: readonly Action[]): Promise<Execution[]> {
    const executions: Execution[] = [];
    for (let compiler = 0; compiler < this.options.compilers.length; compiler += 1) {
      const execution: Execution = {};
      for (const action of actions)
        execution[action] = await this.outcome(compiler, action, source);
      executions.push(execution);
    }
    return executions;
  }
}

interface Evaluation {
  readonly executions: readonly Execution[];
  readonly observations: readonly Observation[];
  readonly reference: readonly string[];
}

function referenceCodes(source: string): string[] | Observation {
  try {
    return parseSource(source).map(({ code }) => code);
  } catch (error) {
    return {
      detail: String(error),
      signature: `parse-agreement|reference-crash|${normalizeLine(String(error), "")}`,
    };
  }
}

async function evaluate(
  fuzzer: FuzzerName,
  input: Input,
  executor: Executor,
  options: Options,
): Promise<Evaluation> {
  const reference = referenceCodes(input.source);
  if (!Array.isArray(reference))
    return { executions: [], observations: [reference], reference: [] };
  if (options.referenceOnly) {
    // No implementation: the only oracle is that the reference parser emits
    // codes the spec inventory knows about.
    const { diagnostics, referenceParser } = executor.inventory;
    const observations = [...new Set(reference)]
      .filter((code) => !diagnostics.has(code) && !referenceParser.has(code))
      .map((code) => ({ detail: code, signature: `reference|unlisted-code|${code}` }));
    return { executions: [], observations, reference };
  }
  if (fuzzer === "wasm") {
    const observations: Observation[] = [];
    const executions = await executor.execute(input.source, ["check"]);
    for (const [index, execution] of executions.entries()) {
      if (execution.check?.kind !== "accept") continue;
      const path = executor.file(input.source);
      const verdict = await validateBuild(options.compilers[index]!, path, options.timeoutMs);
      if (verdict.status !== "valid") {
        const tag = options.compilers.length > 1 ? `c${index}:` : "";
        observations.push({
          detail: verdict.detail,
          signature: `wasm|${tag}${verdict.status}|${normalizeLine(verdict.detail, path)}`,
        });
      }
    }
    return { executions, observations, reference };
  }
  const executions = await executor.execute(input.source, input.actions);
  const oracleInput = { executions, reference };
  const observations =
    fuzzer === "parse"
      ? parseAgreement(oracleInput)
      : fuzzer === "contract"
        ? contract(oracleInput)
        : fuzzer === "phase"
          ? phaseConsistency(oracleInput)
          : crossImpl(oracleInput);
  return { executions, observations, reference };
}

interface SignatureRecord {
  count: number;
  readonly details: Set<string>;
  readonly firstIndex: number;
  readonly origin: string;
  readonly source: string;
  minimized?: string;
}

interface FuzzerReport {
  readonly fuzzer: FuzzerName;
  cases: number;
  stoppedEarly: boolean;
  readonly signatures: Map<string, SignatureRecord>;
  readonly labels: Map<string, number>;
  readonly unknownCodes: Map<string, number>;
  readonly referenceOnlyCodes: Map<string, number>;
  referenceAccepted: number;
  generated: number;
  generatedAccepted: number;
  seconds: number;
}

function bump(map: Map<string, number>, key: string): void {
  map.set(key, (map.get(key) ?? 0) + 1);
}

function tally(
  report: FuzzerReport,
  evaluation: Evaluation,
  input: Input,
  inventory: Inventory,
): void {
  if (evaluation.reference.length === 0) report.referenceAccepted += 1;
  if (input.origin === "generate") {
    report.generated += 1;
    if (evaluation.reference.length === 0) report.generatedAccepted += 1;
  }
  for (const [index, execution] of evaluation.executions.entries())
    for (const [action, outcome] of Object.entries(execution)) {
      bump(report.labels, `c${index}\t${action}\t${outcomeLabel(outcome)}`);
      if (outcome.unknownCode) bump(report.unknownCodes, `${action}\t${outcome.unknownCode}`);
      for (const code of outcome.kind === "reject" ? outcome.detail.split(",") : [])
        if (inventory.referenceParser.has(code) && !inventory.diagnostics.has(code))
          bump(report.referenceOnlyCodes, `${action}\t${code}`);
    }
}

async function runFuzzer(
  fuzzer: FuzzerName,
  corpus: Corpus,
  executor: Executor,
  options: Options,
  inventory: Inventory,
): Promise<FuzzerReport> {
  const started = performance.now();
  const report: FuzzerReport = {
    cases: 0,
    fuzzer,
    generated: 0,
    generatedAccepted: 0,
    labels: new Map(),
    referenceAccepted: 0,
    referenceOnlyCodes: new Map(),
    seconds: 0,
    signatures: new Map(),
    stoppedEarly: false,
    unknownCodes: new Map(),
  };
  const indices = Array.from({ length: options.cases }, (_, index) => index);
  await mapParallel(indices, options.jobs, async (index) => {
    if (report.signatures.size >= options.maxSignatures) {
      report.stoppedEarly = true;
      return;
    }
    const input = corpus.input(fuzzer, index);
    const evaluation = await evaluate(fuzzer, input, executor, options);
    report.cases += 1;
    tally(report, evaluation, input, inventory);
    for (const observation of evaluation.observations) {
      const record = report.signatures.get(observation.signature);
      if (record) {
        record.count += 1;
        if (record.details.size < 6) record.details.add(observation.detail);
      } else if (report.signatures.size < options.maxSignatures)
        report.signatures.set(observation.signature, {
          count: 1,
          details: new Set([observation.detail]),
          firstIndex: index,
          origin: input.origin,
          source: input.source,
        });
    }
    if (report.cases % 250 === 0)
      process.stderr.write(
        `[${fuzzer}] ${report.cases}/${options.cases} cases, ${report.signatures.size} signatures, ${((performance.now() - started) / 1000).toFixed(0)} s\n`,
      );
  });
  report.seconds = (performance.now() - started) / 1000;
  return report;
}

async function minimizeReport(
  report: FuzzerReport,
  corpus: Corpus,
  executor: Executor,
  options: Options,
): Promise<void> {
  const entries = [...report.signatures];
  await mapParallel(
    entries,
    Math.max(1, Math.floor(options.jobs / 2)),
    async ([signature, record]) => {
      const template = corpus.input(report.fuzzer, record.firstIndex);
      // Contract-stream inputs are, by construction, accepted by the reference
      // parser; minimization keeps that precondition.
      const requireAccepted = report.fuzzer !== "parse" && template.actions.length > 1;
      const keeps = async (source: string): Promise<boolean> => {
        const reference = referenceCodes(source);
        if (requireAccepted && (!Array.isArray(reference) || reference.length > 0)) return false;
        const evaluation = await evaluate(
          report.fuzzer,
          { ...template, source },
          executor,
          options,
        );
        return evaluation.observations.some((item) => item.signature === signature);
      };
      record.minimized = await minimize(record.source, keeps, options.minTests);
    },
  );
}

function writeReport(report: FuzzerReport, options: Options, executor: Executor): void {
  const directory = join(options.out, report.fuzzer);
  mkdirSync(join(directory, "examples"), { recursive: true });
  const rows = ["id\tcount\tfirst_case\tsignature\torigin\tdetails"];
  const json: unknown[] = [];
  let id = 0;
  for (const [signature, record] of report.signatures) {
    id += 1;
    const name = `${String(id).padStart(2, "0")}-${hash(signature)}`;
    writeFileSync(join(directory, "examples", `${name}.hd`), record.source);
    if (record.minimized !== undefined)
      writeFileSync(join(directory, "examples", `${name}.min.hd`), record.minimized);
    const details = [...record.details].map((item) => item.replaceAll(/\s+/g, " ")).join(" ; ");
    rows.push(
      `${name}\t${record.count}\t${record.firstIndex}\t${signature}\t${record.origin}\t${details}`,
    );
    json.push({ ...record, details: [...record.details], name, signature });
  }
  writeFileSync(join(directory, "signatures.tsv"), `${rows.join("\n")}\n`);
  const sorted = (map: Map<string, number>): string =>
    [...map]
      .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
      .map(([key, count]) => `${count}\t${key}`)
      .join("\n");
  writeFileSync(
    join(directory, "outcomes.tsv"),
    `count\tcompiler\taction\toutcome\n${sorted(report.labels)}\n`,
  );
  writeFileSync(
    join(directory, "uninventoried-codes.tsv"),
    `count\taction\tcode\n${sorted(report.unknownCodes)}\n`,
  );
  writeFileSync(
    join(directory, "reference-only-codes.tsv"),
    `count\taction\tcode\n${sorted(report.referenceOnlyCodes)}\n`,
  );
  const meta = {
    cases: report.cases,
    compilers: options.compilers.map((command) => command.join(" ")),
    fuzzer: report.fuzzer,
    generated: report.generated,
    generatedReferenceAccepted: report.generatedAccepted,
    maxSignatures: options.maxSignatures,
    referenceAccepted: report.referenceAccepted,
    requestedCases: options.cases,
    seconds: Math.round(report.seconds),
    seed: options.seed,
    signatures: report.signatures.size,
    stoppedEarly: report.stoppedEarly,
    timeoutMs: options.timeoutMs,
    totalCalls: executor.calls,
  };
  writeFileSync(
    join(directory, "run.json"),
    `${JSON.stringify({ meta, signatures: json }, null, 2)}\n`,
  );
  process.stdout.write(
    `${report.fuzzer}: ${report.cases} cases, ${report.signatures.size} signatures${report.stoppedEarly ? " (stopped early)" : ""}, ${report.seconds.toFixed(0)} s\n`,
  );
  for (const [signature, record] of report.signatures)
    process.stdout.write(`  ${String(record.count).padStart(5)}  ${signature}\n`);
}

async function replay(options: Options, executor: Executor): Promise<number> {
  const source = readFileSync(options.replay!, "utf8");
  let found = 0;
  for (const fuzzer of options.fuzzers) {
    const actions: readonly Action[] = fuzzer === "parse" ? ["parse"] : contractActions;
    const evaluation = await evaluate(
      fuzzer,
      { actions, origin: "replay", source },
      executor,
      options,
    );
    process.stdout.write(`${fuzzer}: reference=${evaluation.reference.join(",") || "accept"}\n`);
    for (const [index, execution] of evaluation.executions.entries())
      for (const [action, outcome] of Object.entries(execution))
        process.stdout.write(
          `  c${index} ${action}: ${outcomeLabel(outcome)}${outcome.unknownCode ? ` (uninventoried ${outcome.unknownCode})` : ""}${outcome.kind === "violation" ? ` [${outcome.detail}]` : ""}\n`,
        );
    for (const observation of evaluation.observations) {
      found += 1;
      process.stdout.write(`  SIGNATURE ${observation.signature}\n`);
    }
  }
  return found > 0 ? 1 : 0;
}

async function main(): Promise<number> {
  const options = parseOptions(process.argv.slice(2));
  const inventory = loadInventory();
  const executor = new Executor(options, inventory);
  mkdirSync(options.work, { recursive: true });
  if (options.replay) return replay(options, executor);
  const corpus = new Corpus(options.seed);
  mkdirSync(options.out, { recursive: true });
  process.stdout.write(
    `seed=${options.seed} cases=${options.cases} jobs=${options.jobs} fuzzers=${options.fuzzers.join(",")} compilers=${options.compilers.length} seeds=${corpus.seeds.length} (${corpus.accepted.length} reference-accepted) wasm-tools=${options.adapters.has("wasm") ? wasmToolsAvailable().join("+") || "none" : "off"}\n`,
  );
  let failed = 0;
  for (const fuzzer of options.fuzzers) {
    const report = await runFuzzer(fuzzer, corpus, executor, options, inventory);
    writeReport(report, options, executor);
    if (options.minimize) {
      await minimizeReport(report, corpus, executor, options);
      writeReport(report, options, executor);
    }
    if (options.failOn.has(fuzzer)) failed += report.signatures.size;
  }
  process.stdout.write(`report: ${options.out}\n`);
  if (failed > 0)
    process.stderr.write(
      `fuzz: ${failed} signature(s) in a --fail-on fuzzer; see ${options.out}\n`,
    );
  return failed > 0 ? 1 : 0;
}

const defaultWork = !process.argv.includes("--work");
try {
  process.exitCode = await main();
} finally {
  // The default scratch directory is private to this process; reports go to --out.
  if (defaultWork)
    rmSync(join(tmpdir(), `hd-fuzz-${process.pid}`), { force: true, recursive: true });
}
