// Checks the chapters' examples against conformance/examples.tsv
// (STYLE.md, Examples): every `hd` and `text` fence of a numbered, stdlib,
// or CLI chapter has one row, and each `complete` example passes `check` as
// printed, through the conformance runner and so the command contract. A
// `known-failure` example must still fail, so a fixed one is reclassified.
//
// Usage: check-examples.ts SPEC_DIR EXAMPLES_TSV [COMPILER]
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export interface Example {
  readonly info: "hd" | "text";
  readonly body: string;
  readonly line: number;
}

/** The chapter's `hd` and `text` fences, in order: an example's block number is its position here plus one. */
export function exampleBlocks(text: string): Example[] {
  return [...text.matchAll(/^```(hd|text)\n(.*?)^```/gms)].map((match) => ({
    info: match[1] as "hd" | "text",
    body: match[2]!,
    line: text.slice(0, match.index).split("\n").length + 1,
  }));
}

/** The chapters whose examples are inventoried: numbered, stdlib, and CLI. */
function chapters(specDir: string): string[] {
  const names: string[] = [];
  for (const [directory, pattern] of [
    ["lang", /^\d\d-.*\.md$/],
    ["std", /\.md$/],
    ["cli", /\.md$/],
  ] as const) {
    // cost: three fixed spec directories, listed once per run.
    for (const name of readdirSync(join(specDir, directory)).sort()) {
      if (pattern.test(name) && name !== "README.md") {
        names.push(`${directory}/${name}`);
      }
    }
  }
  return names;
}

/** examples.tsv as chapter -> block -> classification. */
export function readInventory(path: string): Map<string, Map<number, string>> {
  const inventory = new Map<string, Map<number, string>>();
  for (const row of readFileSync(path, "utf8").split(/\r?\n/).slice(1).filter(Boolean)) {
    const [specification = "", block = "", classification = ""] = row.split("\t");
    const blocks = inventory.get(specification) ?? new Map<number, string>();
    blocks.set(Number(block), classification);
    inventory.set(specification, blocks);
  }
  return inventory;
}

export interface Checked {
  readonly where: string;
  readonly classification: string;
}

/**
 * An error example as a `reject-all` case for the conformance runner: each
 * `# error: code` line marker becomes `# diagnostic: code`, and a bare
 * `# error` becomes `# diagnostic: any`. Undefined when the example marks no
 * rejected line, which leaves it to the fixtures (a warning or panic example).
 */
export function errorCase(body: string): string | undefined {
  let marked = 0;
  const lines = body
    .split("\n")
    .map((line) => {
      const marker = /# error(?:: ([a-z0-9-]+))?[ \t]*$/.exec(line);
      if (!marker) {
        return line;
      }
      marked += 1;
      return `${line.slice(0, marker.index)}# diagnostic: ${marker[1] ?? "any"}`;
    })
    .map((line) =>
      // A warning or panic marker is for the fixtures, not this case.
      line.replace(/# (warning|panic): /, "# $1 shown: "),
    );
  return marked > 0 ? lines.join("\n") : undefined;
}

/**
 * Writes each complete and known-failure example to `directory` with a
 * `cases.tsv` that runs it as a type-phase accept case. Returns the written
 * examples by case path, and the inventory count failures.
 */
function writeCases(
  specDir: string,
  inventory: Map<string, Map<number, string>>,
  directory: string,
): { checked: Map<string, Checked>; failures: string[] } {
  const manifest = ["path\tphase\texpectation\tspecification"];
  const checked = new Map<string, Checked>();
  const failures: string[] = [];
  for (const chapter of chapters(specDir)) {
    const examples = exampleBlocks(readFileSync(join(specDir, chapter), "utf8"));
    const indexed = inventory.get(chapter) ?? new Map<number, string>();
    if (indexed.size !== examples.length) {
      failures.push(
        `${chapter} has ${examples.length} examples but ${indexed.size} inventory entries`,
      );
    }
    for (const [index, example] of examples.entries()) {
      const classification = indexed.get(index + 1) ?? "";
      const where = `spec/${chapter}:${example.line} (block ${index + 1}, ${classification})`;
      const path = `${chapter.replace(/\.md$/, "").replace("/", "__")}-${index + 1}.hd`;
      if (classification === "mixed") {
        const errors = errorCase(example.body);
        if (errors === undefined) {
          continue;
        }
        writeFileSync(join(directory, path), errors);
        manifest.push(`${path}\ttype\treject-all\t-`);
        checked.set(path, { where, classification: "error" });
        continue;
      }
      if (classification === "known-failure") {
        // An error example that marks lines is a known failure when the
        // compiler does not report exactly those lines.
        const errors = errorCase(example.body);
        if (errors !== undefined) {
          writeFileSync(join(directory, path), errors);
          manifest.push(`${path}\ttype\treject-all\t-`);
          checked.set(path, { where, classification: "known-error-failure" });
          continue;
        }
      }
      if (classification !== "complete" && classification !== "known-failure") {
        continue;
      }
      writeFileSync(join(directory, path), example.body);
      manifest.push(`${path}\ttype\taccept\t-`);
      checked.set(path, { where, classification });
    }
  }
  writeFileSync(join(directory, "cases.tsv"), manifest.join("\n") + "\n");
  return { checked, failures };
}

/**
 * The runner's verdicts by case path: undefined for a pass, else the reason
 * and output. It prints `pass  PATH`, or `FAIL  PATH: reason` followed by
 * its output lines indented with `      | `.
 */
function readVerdicts(stdout: string): Map<string, string | undefined> {
  const verdicts = new Map<string, string | undefined>();
  let failing: string | undefined;
  for (const line of stdout.split("\n")) {
    const verdict = /^(pass|FAIL) {2}([^\s:]+)(?::\s*(.*))?$/.exec(line);
    if (verdict) {
      failing = verdict[1] === "FAIL" ? verdict[2]! : undefined;
      verdicts.set(verdict[2]!, failing && `${verdict[3] ?? ""}\n`);
    } else if (failing !== undefined && line.startsWith("      | ")) {
      verdicts.set(failing, `${verdicts.get(failing)}  ${line.slice(8)}\n`);
    }
  }
  return verdicts;
}

function judge(path: string, entry: Checked, verdicts: Map<string, string | undefined>) {
  if (!verdicts.has(path)) {
    return `${entry.where}: the runner reported no verdict`;
  }
  const failure = verdicts.get(path);
  if (entry.classification === "error" && failure !== undefined) {
    return `${entry.where} does not reject exactly its marked lines: ${failure.trimEnd()}`;
  }
  if (entry.classification === "known-error-failure" && failure === undefined) {
    return `${entry.where} now rejects exactly its marked lines; classify it mixed`;
  }
  if (entry.classification === "complete" && failure !== undefined) {
    return `${entry.where} does not check as printed: ${failure.trimEnd()}`;
  }
  if (entry.classification === "known-failure" && failure === undefined) {
    return `${entry.where} now checks as printed; classify it complete`;
  }
  return undefined;
}

/** Runs the written cases through the conformance runner and judges each. Returns an exit status. */
export function runAndJudge({
  directory,
  checked,
  failures,
  compiler: compilerArgument,
  label,
}: {
  directory: string;
  checked: Map<string, Checked>;
  failures: string[];
  compiler: string | undefined;
  label: string;
}): number {
  const runner = fileURLToPath(new URL("./tools/run-conformance.ts", import.meta.url));
  const compiler = compilerArgument ? ["--compiler", compilerArgument] : [];
  const result = spawnSync(
    process.execPath,
    // Two jobs: parallel `hd check` processes queue on hd's disk cache, so
    // eight ran about seven times slower each and slow examples hit the
    // runner's 10 s limit under load, for no gain in total time.
    [
      "--experimental-strip-types",
      runner,
      ...compiler,
      "--jobs",
      "2",
      "--cases",
      join(directory, "cases.tsv"),
    ],
    { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  if (result.status !== 0 && result.status !== 1) {
    console.error(result.stdout + result.stderr);
    return 2;
  }
  const verdicts = readVerdicts(result.stdout);
  for (const [path, entry] of checked) {
    const failure = judge(path, entry, verdicts);
    if (failure) {
      failures.push(failure);
    }
  }
  if (failures.length > 0) {
    console.error(`example check failures:\n${failures.map((text) => `- ${text}`).join("\n")}`);
    return 1;
  }
  const complete = [...checked.values()].filter((entry) => entry.classification === "complete");
  console.log(`${label} passed: ${complete.length} complete examples check as printed`);
  return 0;
}

function main(args: readonly string[]): number {
  if (args.length < 2 || args.length > 3) {
    console.error("usage: check-examples.ts SPEC_DIR EXAMPLES_TSV [COMPILER]");
    return 2;
  }
  const directory = mkdtempSync(join(tmpdir(), "hd-spec-examples-"));
  try {
    const inventory = readInventory(resolve(args[1]!));
    const { checked, failures } = writeCases(resolve(args[0]!), inventory, directory);
    return runAndJudge({
      directory,
      checked,
      failures,
      compiler: args[2],
      label: "example check",
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
