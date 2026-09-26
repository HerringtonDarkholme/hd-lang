// Shared helpers for audit step 1.6 (test quality and portability).
import { readFileSync, readdirSync } from "node:fs";
import { relative, resolve } from "node:path";

export const root = resolve(import.meta.dirname, "../../..");
export const fixtureRoot = resolve(root, "test/fixtures");
export const conformanceRoot = resolve(root, "spec/conformance");

export interface Marker {
  readonly kind: "diagnostic" | "warning" | "panic";
  readonly code: string;
  readonly line: number;
}

export interface FixtureInfo {
  readonly body: "test/fixtures" | "spec/conformance";
  readonly path: string; // relative to its body root
  readonly absolute: string;
  readonly source: string;
  readonly lines: readonly string[];
  readonly name: string;
  readonly markers: readonly Marker[];
  readonly expects: readonly string[]; // '# expect:' values
  readonly results: ReadonlyArray<{ entry: string; value: string }>;
  readonly profile?: string;
  readonly scenario?: string;
  readonly pendingFunction?: string;
  readonly packageRole?: string;
  readonly phase?: string; // conformance only, from cases.tsv
  readonly expectation?: string; // conformance only
  readonly section?: string; // conformance only
  readonly selected?: boolean; // conformance only
}

export function walk(directory: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) out.push(...walk(path));
    else if (entry.isFile() && entry.name.endsWith(".hd")) out.push(path);
  }
  return out.sort();
}

const markerPattern = /# (diagnostic|warning|panic): ([a-z0-9-]+)\s*$/;

export function readFixture(absolute: string, body: FixtureInfo["body"]): FixtureInfo {
  const source = readFileSync(absolute, "utf8");
  const lines = source.split("\n");
  const markers: Marker[] = [];
  const expects: string[] = [];
  const results: Array<{ entry: string; value: string }> = [];
  let name = "";
  let profile: string | undefined;
  let scenario: string | undefined;
  let pendingFunction: string | undefined;
  let packageRole: string | undefined;
  for (const [index, line] of lines.entries()) {
    if (line.startsWith("# test: ")) name = line.slice(8).trim();
    if (line.startsWith("# expect: ")) expects.push(line.slice(10).trim());
    if (line.startsWith("# expect-result: ")) {
      const value = line.slice(17).trim();
      const separator = value.indexOf(" = ");
      results.push({ entry: value.slice(0, separator), value: value.slice(separator + 3) });
    }
    const hook = /^# (fixture-[a-z-]+): (.+?)\s*$/.exec(line);
    if (hook?.[1] === "fixture-runtime-profile") profile = hook[2];
    if (hook?.[1] === "fixture-runtime-scenario") scenario = hook[2];
    if (hook?.[1] === "fixture-runtime-pending-function") pendingFunction = hook[2];
    if (hook?.[1] === "fixture-package-role") packageRole = hook[2];
    const marker = markerPattern.exec(line);
    if (marker)
      markers.push({ kind: marker[1] as Marker["kind"], code: marker[2]!, line: index + 1 });
  }
  return {
    absolute,
    body,
    expects,
    lines,
    markers,
    name,
    packageRole,
    path: relative(body === "test/fixtures" ? fixtureRoot : conformanceRoot, absolute),
    pendingFunction,
    profile,
    results,
    scenario,
    source,
  };
}

export function readConformance(): FixtureInfo[] {
  const selected = new Set(
    readFileSync(resolve(root, "test/portable/cases.tsv"), "utf8")
      .trimEnd()
      .split("\n")
      .slice(1)
      .map((row) => row.split("\t")[0]!),
  );
  return readFileSync(resolve(conformanceRoot, "cases.tsv"), "utf8")
    .trimEnd()
    .split("\n")
    .slice(1)
    .map((row) => {
      const [path, phase, expectation, section] = row.split("\t");
      return {
        ...readFixture(resolve(conformanceRoot, path!), "spec/conformance"),
        expectation,
        phase,
        section,
        selected: selected.has(path!),
      };
    });
}

export function readFixtures(): FixtureInfo[] {
  return walk(fixtureRoot).map((path) => readFixture(path, "test/fixtures"));
}

export function specCodes(): Set<string> {
  const readme = readFileSync(resolve(root, "spec/README.md"), "utf8");
  const table = readme.slice(readme.indexOf("| Severity |"), readme.indexOf("Identifier-security"));
  const codes = new Set([...table.matchAll(/`([a-z0-9-]+)`/g)].map((match) => match[1]!));
  const control = readFileSync(resolve(root, "spec/06-control-flow.md"), "utf8");
  const start = control.indexOf("Stable panic categories are exactly");
  const panics = control.slice(start, control.indexOf("\n\n", start));
  for (const match of panics.matchAll(/`([a-z0-9-]+)`/g)) codes.add(match[1]!);
  return codes;
}

export function tsv(rows: ReadonlyArray<readonly unknown[]>): string {
  return rows
    .map((row) => row.map((cell) => String(cell ?? "").replaceAll(/[\t\n]/g, " ")).join("\t"))
    .join("\n");
}

export function header(command: string): string {
  return `# commit bd985d7; command: ${command}; date: ${new Date().toISOString().slice(0, 10)}`;
}
