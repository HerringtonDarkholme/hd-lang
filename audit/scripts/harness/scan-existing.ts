// Scans the real selected conformance cases and test/fixtures with `hd check`
// and reports two harness blind spots on existing data:
//   - accept or warn cases whose check output carries unexpected warnings;
//   - reject cases whose check output carries diagnostics beyond the marker.
// Usage: node --experimental-strip-types audit/scripts/harness/scan-existing.ts
import { spawn } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");

interface Row {
  readonly file: string;
  readonly kind: string;
  readonly markers: ReadonlyArray<{ line: number; code: string; kind: string }>;
}

function run(file: string): Promise<{ code: number; output: string }> {
  return new Promise((complete) => {
    const child = spawn("node", ["--experimental-strip-types", "bin/hd.js", "check", file], {
      cwd: root,
    });
    let output = "";
    child.stdout.on("data", (chunk: Buffer) => (output += chunk.toString()));
    child.stderr.on("data", (chunk: Buffer) => (output += chunk.toString()));
    child.on("close", (code) => complete({ code: code ?? 1, output }));
  });
}

async function walk(directory: string): Promise<string[]> {
  const found: string[] = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) found.push(...(await walk(path)));
    else if (entry.name.endsWith(".hd")) found.push(path);
  }
  return found.sort();
}

async function describe(file: string, kind: string): Promise<Row> {
  const lines = (await readFile(file, "utf8")).split("\n");
  const markers = lines.flatMap((text, index) => {
    const match = /# (diagnostic|warning|panic): ([a-z0-9-]+)\s*$/.exec(text);
    return match ? [{ line: index + 1, code: match[2]!, kind: match[1]! }] : [];
  });
  const expectation = lines.find((text) => text.startsWith("# expect"));
  return { file, kind: expectation ? `${kind}:${expectation.slice(2)}` : kind, markers };
}

async function main(): Promise<void> {
  const manifest = (await readFile(resolve(root, "test/portable/cases.tsv"), "utf8"))
    .trimEnd()
    .split("\n")
    .slice(1)
    .map((row) => row.split("\t"));
  const rows: Row[] = [];
  for (const [path, phase] of manifest)
    if (phase !== "parse")
      rows.push(await describe(resolve(root, "spec/conformance", path!), `conformance-${phase}`));
  for (const file of await walk(resolve(root, "test/fixtures")))
    rows.push(await describe(file, "fixture"));
  const results: string[] = [];
  let next = 0;
  async function worker(): Promise<void> {
    while (next < rows.length) {
      const row = rows[next]!;
      next += 1;
      if (row.kind.includes("expect: parse")) continue;
      const { code, output } = await run(row.file);
      const diagnostics = [...output.matchAll(/:(\d+):\d+: (warning: )?([a-z0-9-]+):/g)].map(
        (match) => ({ line: Number(match[1]), warning: Boolean(match[2]), code: match[3]! }),
      );
      const unexpected = diagnostics.filter(
        (diagnostic) =>
          !row.markers.some(
            (marker) =>
              marker.line === diagnostic.line &&
              marker.code === diagnostic.code &&
              (marker.kind === "warning") === diagnostic.warning,
          ),
      );
      if (unexpected.length === 0) continue;
      const relative = row.file.slice(root.length + 1);
      const shown = unexpected
        .map((item) => `${item.line}:${item.warning ? "warning:" : ""}${item.code}`)
        .join(",");
      results.push(`${relative}\t${row.kind}\t${code}\t${row.markers.length}\t${shown}`);
    }
  }
  await Promise.all(Array.from({ length: 6 }, worker));
  console.log("file\tkind\tcheck_exit\tmarkers\tunexpected_diagnostics");
  for (const line of results.sort()) console.log(line);
  console.error(`scanned ${rows.length} files, ${results.length} with unexpected diagnostics`);
}

await main();
