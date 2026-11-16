// Checks conformance/cases.tsv and conformance/examples.tsv against the
// fixtures and the specification, as conformance/README.md defines them.
// One process reads every file once; spec/check.sh calls it.
//
// Usage: check-fixture-index.ts SPEC_DIR

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

import { duplicateFixtures, duplicateTrees } from "./tools/fixture-duplicates.ts";
import { fixtureGuards, type GuardCase } from "./tools/fixture-guards.ts";
import { panicMarkerOnCall } from "./tools/panic-marker-line.ts";

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
const marker = /# (diagnostic|panic): [a-z0-9-]+[ \t]*$/;
// A warning is no case's expectation (#1952): a lint finding is tested by
// the cli/lint-* case of its lint.
const warningMarker = /# warning: [a-z0-9-]+[ \t]*$/;

// cases.tsv
const [header, ...cases] = readRows(join(conformance, "cases.tsv"));
if (header?.join("\t") !== "path\tphase\texpectation\tspecification") {
  fail("malformed conformance/cases.tsv");
}
const entries = new Map<string, number>();
const indexed: GuardCase[] = [];
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
  indexed.push({ path, phase, expectation, text, specification: section });
  const lines = text.split("\n");
  const markers = lines.filter((line) => marker.test(line)).length;
  if (lines.some((line) => warningMarker.test(line))) {
    fail(`fixture ${path} declares a # warning: marker; test a lint in its cli/lint-* case`);
  }
  if (!["parse", "type", "runtime"].includes(phase)) {
    fail(`unknown phase '${phase}' for ${path}`);
  }
  if (expectation === "accept") {
    if (markers !== 0) {
      fail(`accept fixture ${path} declares an error marker`);
    }
  } else if (/^(reject|panic):/.test(expectation)) {
    if (markers !== 1) {
      fail(`fixture ${path} must declare exactly one expectation marker`);
    }
  } else if (expectation.startsWith("warn:")) {
    fail(`fixture ${path} expects a warning; test a lint in its cli/lint-* case`);
  } else {
    fail(`unknown expectation '${expectation}' for ${path}`);
  }
  const runtimeAccept = phase === "runtime" && expectation === "accept";
  if (lines.some((line) => line.startsWith("# expect-stdout:")) && !runtimeAccept) {
    fail(`fixture ${path} uses # expect-stdout: outside a runtime accept case`);
  }
  if (lines.some((line) => line.startsWith("# expect-empty-stdout:"))) {
    if (!runtimeAccept) {
      fail(`fixture ${path} uses # expect-empty-stdout: outside a runtime accept case`);
    }
    if (!lines.includes("# expect-empty-stdout: true")) {
      fail(`fixture ${path}: # expect-empty-stdout: takes the value true`);
    }
  }
  const specFile = section.split("#")[0]!;
  if (!existsSync(join(specDir, specFile))) {
    fail(`missing specification ${specFile} for ${path}`);
  }
  const [kind, code] = expectation.split(/:(.*)/s) as [string, string | undefined];
  const check = (directive: string, label: string, source: string, category: string): void => {
    if (!text.includes(`# ${directive}: ${code}`)) {
      fail(`fixture ${path} does not declare ${label} ${code}`);
    }
    if (!source.includes(`\`${code}\``)) {
      fail(`fixture ${path} uses unknown ${category} category ${code}`);
    }
  };
  if (kind === "reject") {
    check("diagnostic", "diagnostic", readme, "error");
  } else if (kind === "panic") {
    check("panic", "panic", controlFlow, "panic");
    const callee = panicMarkerOnCall(text);
    if (callee !== undefined) {
      fail(
        `fixture ${path} marks a call of its own function ${callee}; the panic marker belongs on the line in ${callee} that raises it`,
      );
    }
  }
}

// Every fixture outside the package sources has exactly one cases.tsv row.
function walk(directory: string): string[] {
  // cost: one listing per conformance directory per run, names only; the
  // check covers every fixture by design.
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory())
    // The inputs of a CLI case (cli/NAME) are indexed by cli-cases.tsv, below.
    {
      return ["lang", "std"].some(
        (tier) =>
          path === join(conformance, tier, "packages") || path === join(conformance, tier, "trees"),
      ) || path === join(conformance, "cli")
        ? []
        : walk(path);
    }
    return entry.isFile() && entry.name.endsWith(".hd") ? [path] : [];
  });
}
for (const file of walk(conformance).sort()) {
  const path = relative(conformance, file);
  const count = entries.get(path) ?? 0;
  if (count !== 1) {
    fail(`fixture ${path} has ${count} manifest entries`);
  }
}

// No two cases test one program, and no two package trees of a tier hold the
// same files (A1, A2 and A14 of docs/audit/conformance-fixture-review-2026-11-14.md).
// A fixture names a tree of its own tier, so trees compare within a tier.
for (const problem of duplicateFixtures(indexed)) {
  fail(problem);
}
// The fixture rules of conformance/README.md "Writing a Case" that a script
// can check (A10, A10c, A11, A16 and the literal part of A17 of the review).
for (const problem of fixtureGuards(indexed)) {
  fail(problem);
}
function treeFiles(root: string, directory = root): [string, Buffer][] {
  // cost: one listing per tree directory and one read per tree file per run.
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      return treeFiles(root, path);
    }
    return entry.isFile() ? [[relative(root, path), readFileSync(path)] as [string, Buffer]] : [];
  });
}
for (const tier of ["lang", "std"]) {
  const trees = join(conformance, tier, "trees");
  // cost: one listing of TIER/trees per run, then treeFiles per tree.
  const listed = readdirSync(trees, { withFileTypes: true }).filter((entry) => entry.isDirectory());
  for (const problem of duplicateTrees(
    listed.map((entry) => ({
      name: `${tier}/trees/${entry.name}`,
      files: treeFiles(join(trees, entry.name)),
    })),
  )) {
    fail(problem);
  }
}

// cli-cases.tsv: every directory under cli/ is one case with an expect.txt and
// one row, and each row cites rules that spec/cli/command-line.md defines.
const cliRules = new Set(
  [
    ...readFileSync(join(specDir, "cli/command-line.md"), "utf8").matchAll(/\br\[(cli\.[^\]]+)\]/g),
  ].map((match) => match[1]!),
);
const [cliHeader, ...cliRows] = readRows(join(conformance, "cli-cases.tsv"));
if (cliHeader?.join("\t") !== "case\trules") {
  fail("malformed conformance/cli-cases.tsv");
}
const cliNamed = new Set<string>();
for (const row of cliRows) {
  const [name = "", rules = ""] = row;
  if (row.length !== 2 || !/^[a-z0-9][a-z0-9-]*$/.test(name) || rules === "") {
    fail(`malformed conformance/cli-cases.tsv row ${row.join("\t")}`);
    continue;
  }
  if (cliNamed.has(name)) {
    fail(`cli case ${name} has two rows`);
  }
  cliNamed.add(name);
  if (!existsSync(join(conformance, "cli", name, "expect.txt"))) {
    fail(`cli case ${name} has no expect.txt`);
  }
  for (const rule of rules.split(",")) {
    if (!cliRules.has(rule)) {
      fail(`cli case ${name} cites ${rule}, which cli/command-line.md lacks`);
    }
  }
}
// cost: one listing of conformance/cli per run.
for (const entry of readdirSync(join(conformance, "cli"), { withFileTypes: true })) {
  if (entry.isDirectory() && !cliNamed.has(entry.name)) {
    fail(`cli case directory ${entry.name} has no cli-cases.tsv row`);
  }
}

// examples.tsv (STYLE.md, Examples). A mixed or error-fragment entry names its fixtures; a
// complete, known-failure, or fragment entry names them or '-'.
const examined = new Set(["complete", "known-failure", "fragment", "mixed", "error-fragment"]);
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
  if (!existsSync(join(specDir, specification))) {
    fail(`missing example specification ${specification}`);
  }
  if (examined.has(classification)) {
    if (fixtures === "-" && (classification === "mixed" || classification === "error-fragment")) {
      fail(`${classification} entry must name its fixtures for ${specification} block ${block}`);
    }
    for (const path of fixtures === "-" ? [] : fixtures.split("|")) {
      if (!existsSync(join(conformance, path))) {
        fail(`missing example fixture ${path} for ${specification} block ${block}`);
      }
    }
  } else if (fragments.has(classification)) {
    if (fixtures !== "-") {
      fail(`${classification} entry must use '-' for ${specification} block ${block}`);
    }
  } else {
    fail(`unknown example classification '${classification}'`);
  }
}

if (problems.length) {
  for (const problem of problems) {
    console.error(`spec check failed: ${problem}`);
  }
  process.exit(1);
}
