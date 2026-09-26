// Phase 4.2: run every `# panic:` fixture and compare the reported category.
// Usage: node --experimental-strip-types audit/scripts/runtime/panics.ts
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";

const root = resolve(dirname(new URL(import.meta.url).pathname), "../../..");

function walk(directory: string): string[] {
  return readdirSync(directory).flatMap((entry) => {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) return walk(path);
    return path.endsWith(".hd") ? [path] : [];
  });
}

const files = [
  ...walk(join(root, "spec/conformance")),
  ...walk(join(root, "test/fixtures")),
  ...walk(join(root, "audit/probes/runtime/panics")),
].filter((path) => /# panic: /.test(readFileSync(path, "utf8")));

const rows = ["fixture\tcommand\texpected\tmarker_line\texit\treported\tmatch\tlocation_in_output"];
for (const path of files.sort()) {
  const source = readFileSync(path, "utf8");
  const lines = source.split("\n");
  const markerIndex = lines.findIndex((line) => /# panic: /.test(line));
  const expected = /# panic: ([a-z0-9-]+)/.exec(lines[markerIndex]!)![1]!;
  const option = (name: string, flag: string): string[] => {
    const match = new RegExp(`^# ${name}: (\\S+)`, "m").exec(source);
    return match ? [flag, match[1]!] : [];
  };
  const isConformance = path.includes("spec/conformance");
  const command =
    isConformance || (path.includes("audit/probes") && /^test "/m.test(source)) ? "test" : "run";
  const args = [
    "--experimental-strip-types",
    "bin/hd.js",
    command,
    ...option("fixture-runtime-profile", "--profile"),
    ...option("fixture-runtime-scenario", "--scenario"),
    ...option("fixture-runtime-pending-function", "--pending-function"),
    relative(root, path),
  ];
  const result = spawnSync("node", args, { cwd: root, encoding: "utf8" });
  const stderr = result.stderr.trim();
  const reportedMatch = /^([a-z0-9-]+): runtime panic/m.exec(stderr);
  const reported = reportedMatch
    ? reportedMatch[1]!
    : `NONSTRUCTURED(${(stderr.split("\n").find((line) => /Error/.test(line)) ?? stderr.split("\n")[0] ?? "").slice(0, 80)})`;
  const hasLocation = /:\d+:\d+/.test(stderr) ? "yes" : "no";
  rows.push(
    [
      relative(root, path),
      command,
      expected,
      String(markerIndex + 1),
      String(result.status),
      reported,
      reported === expected ? "yes" : expected === "runtime-error" ? "any" : "NO",
      hasLocation,
    ].join("\t"),
  );
}
const header = `# commit bd985d7, ${new Date().toISOString()}, node ${process.version}\n# command: node --experimental-strip-types audit/scripts/runtime/panics.ts\n`;
writeFileSync(join(root, "audit/evidence/04-runtime/panics.tsv"), header + rows.join("\n") + "\n");
console.log(rows.join("\n"));
