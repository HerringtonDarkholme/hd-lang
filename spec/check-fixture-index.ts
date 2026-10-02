// Checks conformance/cases.tsv and conformance/examples.tsv against the
// fixtures and the specification, as conformance/README.md defines them.
// One process reads every file once; spec/check.sh calls it.
//
// Usage: check-fixture-index.ts SPEC_DIR

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

const specDir = process.argv[2];
if (!specDir) {
  console.error("usage: check-fixture-index.ts SPEC_DIR");
  process.exit(2);
}
const conformance = join(specDir, "conformance");
const problems: string[] = [];
const fail = (message: string): void => void problems.push(message);

const readRows = (path: string): string[][] =>
  readFileSync(path, "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => line.split("\t"));

const fixtureText = new Map<string, string | undefined>();
function fixture(path: string): string | undefined {
  if (!fixtureText.has(path)) {
    const file = join(conformance, path);
    fixtureText.set(path, existsSync(file) ? readFileSync(file, "utf8") : undefined);
  }
  return fixtureText.get(path);
}

const readme = readFileSync(join(specDir, "README.md"), "utf8");
const controlFlow = readFileSync(join(specDir, "lang/06-control-flow.md"), "utf8");
const marker = /# (diagnostic|warning|panic): [a-z0-9-]+[ \t]*$/;

// cases.tsv
const [header, ...cases] = readRows(join(conformance, "cases.tsv"));
if (header?.join("\t") !== "path\tphase\texpectation\tspecification")
  fail("malformed conformance/cases.tsv");
const entries = new Map<string, number>();
for (const row of cases) {
  if (row.length !== 4 || row.some((field) => field === "")) {
    fail("malformed conformance/cases.tsv");
    continue;
  }
  const [path, phase, expectation, section] = row as [string, string, string, string];
  entries.set(path, (entries.get(path) ?? 0) + 1);
  const text = fixture(path);
  if (text === undefined) {
    fail(`missing fixture ${path}`);
    continue;
  }
  const lines = text.split("\n");
  const markers = lines.filter((line) => marker.test(line)).length;
  if (!["parse", "type", "runtime"].includes(phase)) fail(`unknown phase '${phase}' for ${path}`);
  if (expectation === "accept") {
    if (markers !== 0) fail(`accept fixture ${path} declares an error marker`);
  } else if (/^(reject|warn|panic):/.test(expectation)) {
    if (markers !== 1) fail(`fixture ${path} must declare exactly one expectation marker`);
  } else fail(`unknown expectation '${expectation}' for ${path}`);
  const runtimeAccept = phase === "runtime" && expectation === "accept";
  if (lines.some((line) => line.startsWith("# expect-stdout:")) && !runtimeAccept)
    fail(`fixture ${path} uses # expect-stdout: outside a runtime accept case`);
  if (lines.some((line) => line.startsWith("# expect-empty-stdout:"))) {
    if (!runtimeAccept)
      fail(`fixture ${path} uses # expect-empty-stdout: outside a runtime accept case`);
    if (!lines.includes("# expect-empty-stdout: true"))
      fail(`fixture ${path}: # expect-empty-stdout: takes the value true`);
  }
  const specFile = section.split("#")[0]!;
  if (!existsSync(join(specDir, specFile))) fail(`missing specification ${specFile} for ${path}`);
  const [kind, code] = expectation.split(/:(.*)/s) as [string, string | undefined];
  const check = (directive: string, label: string, source: string, category: string): void => {
    if (!text.includes(`# ${directive}: ${code}`))
      fail(`fixture ${path} does not declare ${label} ${code}`);
    if (!source.includes(`\`${code}\``))
      fail(`fixture ${path} uses unknown ${category} category ${code}`);
  };
  if (kind === "reject") check("diagnostic", "diagnostic", readme, "error");
  else if (kind === "warn") check("warning", "warning", readme, "warning");
  else if (kind === "panic") check("panic", "panic", controlFlow, "panic");
}

// Every fixture outside the package sources has exactly one cases.tsv row.
function walk(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory())
      return path === join(conformance, "packages") || path === join(conformance, "trees")
        ? []
        : walk(path);
    return entry.isFile() && entry.name.endsWith(".hd") ? [path] : [];
  });
}
for (const file of walk(conformance).sort()) {
  const path = relative(conformance, file);
  const count = entries.get(path) ?? 0;
  if (count !== 1) fail(`fixture ${path} has ${count} manifest entries`);
}

// examples.tsv
const fragments = new Set([
  "lexical-inventory",
  "type-relation",
  "type-fragment",
  "pattern-fragment",
  "expression-fragment",
  "filesystem-layout",
  "illustrative-pseudocode",
]);
for (const [specification = "", block, classification = "", fixtures = ""] of readRows(
  join(conformance, "examples.tsv"),
).slice(1)) {
  if (!existsSync(join(specDir, specification)))
    fail(`missing example specification ${specification}`);
  if (classification === "accept" || classification === "mixed") {
    for (const path of fixtures.split("|"))
      if (!existsSync(join(conformance, path)))
        fail(`missing example fixture ${path} for ${specification} block ${block}`);
  } else if (fragments.has(classification)) {
    if (fixtures !== "-")
      fail(`${classification} entry must use '-' for ${specification} block ${block}`);
  } else fail(`unknown example classification '${classification}'`);
}

if (problems.length) {
  for (const problem of problems) console.error(`spec check failed: ${problem}`);
  process.exit(1);
}
