// Phase 4.3: render diagnostics with the source line and a caret, for manual span review.
// Usage: node --experimental-strip-types audit/scripts/runtime/diagnostics.ts [FILE...]
// With no FILE, samples every 5th distinct `# diagnostic:` code (one fixture per code).
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

function sample(): string[] {
  const byCode = new Map<string, string>();
  for (const path of [...walk(join(root, "spec/conformance")), ...walk(join(root, "test/fixtures"))].sort()) {
    const match = /# diagnostic: ([a-z0-9-]+)/.exec(readFileSync(path, "utf8"));
    if (match && !byCode.has(match[1]!)) byCode.set(match[1]!, path);
  }
  return [...byCode.keys()]
    .sort()
    .filter((_, index) => index % 5 === 0)
    .map((code) => byCode.get(code)!);
}

const files = process.argv.length > 2 ? process.argv.slice(2).map((path) => resolve(root, path)) : sample();
const output: string[] = [
  `# commit bd985d7, ${new Date().toISOString()}, node ${process.version}`,
  `# command: node --experimental-strip-types audit/scripts/runtime/diagnostics.ts ${process.argv.slice(2).join(" ")}`,
];
for (const path of files) {
  const source = readFileSync(path, "utf8");
  const lines = source.split("\n");
  const markers = lines.flatMap((line, index) => {
    const match = /# diagnostic: ([a-z0-9-]+)/.exec(line);
    return match ? [`${match[1]}@${index + 1}`] : [];
  });
  const profile = /^# fixture-runtime-profile: (\S+)/m.exec(source);
  const command = path.includes("/parse/") ? "parse" : "check";
  const result = spawnSync(
    "node",
    [
      "--experimental-strip-types",
      "bin/hd.js",
      command,
      ...(profile ? ["--profile", profile[1]!] : []),
      relative(root, path),
    ],
    { cwd: root, encoding: "utf8" },
  );
  output.push("", `## ${relative(root, path)}  (${command}, exit ${result.status})`);
  output.push(`expected: ${markers.join(", ") || "none"}`);
  const diagnostics = result.stderr.split("\n").filter((line) => /:\d+:\d+: /.test(line));
  output.push(`reported: ${diagnostics.length} diagnostic(s)`);
  for (const diagnostic of diagnostics) {
    const match = /:(\d+):(\d+): (.*)$/.exec(diagnostic)!;
    const line = Number(match[1]);
    const column = Number(match[2]);
    output.push(`  ${line}:${column} ${match[3]}`);
    const text = lines[line - 1] ?? "";
    output.push(`    | ${text}`);
    output.push(`    | ${" ".repeat(Math.max(0, column - 1))}^`);
  }
  if (diagnostics.length === 0) output.push(`  stdout: ${result.stdout.trim()} stderr: ${result.stderr.trim().slice(0, 200)}`);
}
const target = process.argv.length > 2 ? "diagnostics-probes.log" : "diagnostics-sample.log";
writeFileSync(join(root, "audit/evidence/04-runtime", target), output.join("\n") + "\n");
console.log(output.join("\n"));
