// W6 converter: promotes test/fixtures files into spec/conformance/.
//
// Reads audit/scripts/w6/plan.tsv (source, action, dest, remap, note) and, for
// each row:
//   del   git rm the test/fixtures copy (an identical conformance copy exists)
//   hold  leave the file in test/fixtures
//   mv    git mv, keep the body, normalize headers, apply marker remaps
//   res   as mv, and turn each `# expect-result: ENTRY = VALUE` into a named
//         test block that checks ENTRY() with assert_equal (D1, D2)
//   man   as res or mv; the file is then finished by hand (see the note column)
//
// Header normalization: `# expect:` and `# expect-result:` are dropped,
// `# fixture-*` directives are kept, and `# test:` is kept (renamed later when
// names collide, F-211). Values rendered by the old JavaScript host are
// rewritten as hd literals: an f64 result `3` becomes `3.0`, and a char
// result given as a code point becomes a `'\u{...}'` literal.
//
// Writes audit/evidence/w6/moves.tsv (source, action, destination, note) and
// audit/evidence/w6/new-cases.tsv (cases.tsv rows for the promoted files).
//
// Usage: node --experimental-strip-types audit/scripts/w6/convert.ts [--dry-run]
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");
const dryRun = process.argv.includes("--dry-run");
const prefixes: Record<string, string> = {
  pi: "parse/invalid",
  pv: "parse/valid",
  rp: "runtime/panic",
  rv: "runtime/valid",
  ti: "typing/invalid",
  tv: "typing/valid",
  tw: "typing/warnings",
};

interface Row {
  readonly action: string;
  readonly dest: string;
  readonly note: string;
  readonly remap: string;
  readonly source: string;
}

function readPlan(): Row[] {
  const text = readFileSync(resolve(import.meta.dirname, "plan.tsv"), "utf8");
  return text
    .split("\n")
    .slice(1)
    .filter(Boolean)
    .map((line) => {
      const [source = "", action = "", dest = "", remap = "", note = ""] = line.split("\t");
      return { action, dest, note, remap, source };
    });
}

function inventorySections(): Map<string, string> {
  const text = readFileSync(resolve(root, "audit/evidence/01-test-quality/inventory.tsv"), "utf8");
  const sections = new Map<string, string>();
  for (const line of text.split("\n")) {
    const fields = line.split("\t");
    if (fields[0] !== "test/fixtures") continue;
    sections.set(fields[1]!, (fields[5] ?? "").replace(/ -> .*/, ""));
  }
  return sections;
}

function destination(dest: string): string {
  const [prefix, name] = dest.split("/");
  const directory = prefixes[prefix!];
  if (!directory || !name) throw new Error(`bad destination ${dest}`);
  return `${directory}/${name}.hd`;
}

function returnType(body: readonly string[], entry: string): { bang: boolean; type: string } {
  const pattern = new RegExp(`^(?:pub )?fn ${entry}(!?)\\(\\)\\s*->\\s*([^:$]+?)\\s*(?:\\$.*)?:`);
  for (const line of body) {
    const match = pattern.exec(line);
    if (match) return { bang: match[1] === "!", type: match[2]!.trim() };
  }
  throw new Error(`no zero-parameter declaration for ${entry}`);
}

function literal(value: string, type: string): string {
  if (type === "f64" || type === "f32") return /^-?\d+$/.test(value) ? `${value}.0` : value;
  if (type === "char") return `'\\u{${Number(value).toString(16).toUpperCase()}}'`;
  if (type === "bool") return value === "1" ? "true" : value === "0" ? "false" : value;
  return value;
}

function applyRemaps(line: string, remap: string): string {
  if (remap === "-" || !remap) return line;
  for (const pair of remap.split(",")) {
    const [from, to] = pair.split("=");
    line = line.replace(
      new RegExp(`# (diagnostic|warning|panic): ${from}(\\s*)$`),
      `# $1: ${to}$2`,
    );
  }
  return line;
}

function ensureAssertEqualUse(body: string[]): string[] {
  const index = body.findIndex((line) => line.startsWith("use std.testing."));
  if (index < 0) return ["use std.testing.assert_equal", "", ...body];
  const line = body[index]!;
  if (/\bassert_equal\b/.test(line)) return body;
  const names = line.replace("use std.testing.", "").replace(/[{}]/g, "").split(",");
  const merged = [...names.map((name) => name.trim()).filter(Boolean), "assert_equal"];
  const copy = [...body];
  copy[index] = `use std.testing.{${merged.join(", ")}}`;
  return copy;
}

// Names that describe an implementation mechanism, or that several fixtures
// share (F-211), are replaced by a sentence made from the destination name.
const mechanism =
  /\b(wasm|gc|abi|lower|lowers|lowering|erased?|erase|boxing|box frame|cfg|dictionar(y|ies)|frames?|carrier|bridge|globals?)\b/i;

function originalName(source: string): string | undefined {
  const line = source.split("\n").find((text) => text.startsWith("# test: "));
  return line?.slice("# test: ".length).trim();
}

function chooseNames(rows: readonly Row[]): Map<string, string> {
  const counts = new Map<string, number>();
  const promoted = rows.filter(({ action }) => ["mv", "res", "man"].includes(action));
  for (const row of promoted) {
    const name = originalName(readFileSync(resolve(root, "test/fixtures", row.source), "utf8"));
    if (name) counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  const names = new Map<string, string>();
  for (const row of promoted) {
    const name = originalName(readFileSync(resolve(root, "test/fixtures", row.source), "utf8"));
    const fallback = row.dest.split("/")[1]!.replaceAll("-", " ");
    names.set(row.source, !name || counts.get(name)! > 1 || mechanism.test(name) ? fallback : name);
  }
  return names;
}

function convert(source: string, row: Row, chosen: string): string {
  const lines = source.replace(/\n+$/, "").split("\n");
  let headerEnd = 0;
  while (headerEnd < lines.length && lines[headerEnd]!.startsWith("# ")) headerEnd += 1;
  const header = lines.slice(0, headerEnd);
  let body = lines.slice(headerEnd);
  while (body.length && body[0] === "") body = body.slice(1);
  const name = `# test: ${chosen}`;
  const kept = header.filter((line) => line.startsWith("# fixture-"));
  const results = header
    .filter((line) => line.startsWith("# expect-result: "))
    .map((line) => {
      const match = /^# expect-result: (\S+) = (.+)$/.exec(line);
      if (!match) throw new Error(`bad expect-result in ${row.source}: ${line}`);
      return { entry: match[1]!, value: match[2]!.trim() };
    });
  body = body.map((line) => applyRemaps(line, row.remap));
  if (row.action === "res" && results.length === 0)
    throw new Error(`${row.source}: res without expect-result`);
  if (results.length) {
    const claim = chosen;
    body = ensureAssertEqualUse(body);
    for (const { entry, value } of results) {
      const { bang, type } = returnType(body, entry);
      const expected = literal(value, type);
      body.push(
        "",
        `test "${entry} returns ${expected}":`,
        `    assert_equal(${entry}${bang ? "!" : ""}(), ${expected}, reason="${claim.replaceAll('"', "'")}")`,
      );
    }
  }
  return [name, ...kept, "", ...body].join("\n") + "\n";
}

function phaseAndExpectation(path: string, text: string): { expectation: string; phase: string } {
  const marker = /# (diagnostic|warning|panic): ([a-z0-9-]+)\s*$/m.exec(text);
  const phase = path.startsWith("parse/")
    ? "parse"
    : path.startsWith("runtime/")
      ? "runtime"
      : "type";
  if (!marker) return { expectation: "accept", phase };
  const kind = marker[1] === "diagnostic" ? "reject" : marker[1] === "warning" ? "warn" : "panic";
  return { expectation: `${kind}:${marker[2]}`, phase };
}

function git(...args: string[]): void {
  if (dryRun) return;
  execFileSync("git", args, { cwd: root, stdio: "inherit" });
}

const sections = inventorySections();
const plan = readPlan();
const names = chooseNames(plan);
const moves: string[] = ["source\taction\tdestination\tnote"];
const cases: string[] = [];
for (const row of plan) {
  const from = resolve(root, "test/fixtures", row.source);
  if (row.action === "hold") {
    moves.push(`test/fixtures/${row.source}\theld back\ttest/fixtures/${row.source}\t${row.note}`);
    continue;
  }
  if (row.action === "del") {
    git("rm", "-q", `test/fixtures/${row.source}`);
    moves.push(`test/fixtures/${row.source}\tdeleted\tspec/conformance/${row.dest}\t${row.note}`);
    continue;
  }
  const path = destination(row.dest);
  const text = convert(readFileSync(from, "utf8"), row, names.get(row.source)!);
  const to = resolve(root, "spec/conformance", path);
  if (!dryRun) {
    mkdirSync(dirname(to), { recursive: true });
    git("mv", `test/fixtures/${row.source}`, `spec/conformance/${path}`);
    writeFileSync(to, text);
  } else console.log(`--- ${path}\n${text}`);
  const { expectation, phase } = phaseAndExpectation(path, text);
  const section = sections.get(row.source) ?? "";
  cases.push(`${path}\t${phase}\t${expectation}\t${section}`);
  const action =
    row.action === "res" ? "converted" : row.action === "man" ? "converted by hand" : "moved";
  moves.push(`test/fixtures/${row.source}\t${action}\tspec/conformance/${path}\t${row.note}`);
}
const out = resolve(root, "audit/evidence/w6");
mkdirSync(out, { recursive: true });
if (!dryRun) {
  writeFileSync(resolve(out, "moves.tsv"), moves.join("\n") + "\n");
  writeFileSync(resolve(out, "new-cases.tsv"), cases.join("\n") + "\n");
}
console.log(`${cases.length} promoted, ${moves.length - 1 - cases.length} held back or deleted`);
