// 5.6: collect every host import emitted across the fixture corpus.
// Run: node --experimental-strip-types audit/scripts/arch/rtlib-imports.ts
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";

import { compilePhased, header } from "./bench-lib.ts";

const root = resolve(import.meta.dirname, "../../..");
const profiles: Record<string, string[]> = {
  "pending-gate": ["Gate"],
  "ready-counter": ["Counter"],
  "ready-float": ["FloatCell"],
  "ready-gate": ["Gate"],
  "ready-text": ["TextBridge"],
};

function walk(directory: string): string[] {
  return readdirSync(directory).flatMap((name) => {
    const path = resolve(directory, name);
    return statSync(path).isDirectory() ? walk(path) : name.endsWith(".hd") ? [path] : [];
  });
}

const seen = new Map<string, { signature: string; count: number; example: string }>();
let modules = 0;
for (const path of [...walk(resolve(root, "spec/conformance")), ...walk(resolve(root, "test/fixtures"))]) {
  const source = readFileSync(path, "utf8");
  const profile = /^# fixture-runtime-profile: ([a-z0-9-]+)/m.exec(source)?.[1];
  let wat: string;
  try {
    wat = compilePhased(source, { hostCapabilities: profile ? profiles[profile] : undefined }).wat;
  } catch {
    continue;
  }
  modules += 1;
  for (const match of wat.matchAll(/\(import "([^"]+)" "([^"]+)" \(func \$[^\s]+ ?(.*)\)\)$/gm)) {
    const name = `${match[1]}.${match[2]!.replace(/^host_\d+_\d+_/, "host_T_M_")}`;
    const entry = seen.get(name) ?? { signature: match[3]!, count: 0, example: relative(root, path) };
    entry.count += 1;
    seen.set(name, entry);
  }
}
const lines = [
  header("node --experimental-strip-types audit/scripts/arch/rtlib-imports.ts"),
  `Modules compiled: ${modules}. Host-provider imports are normalized as host_T_M_* (T = trait index, M = method index).`,
  "",
  "| import | signature | modules importing it | example |",
  "| --- | --- | --- | --- |",
  ...[...seen]
    .sort((a, b) => b[1].count - a[1].count)
    .map(([name, entry]) => `| ${name} | \`${entry.signature}\` | ${entry.count} | ${entry.example} |`),
];
writeFileSync(resolve(root, "audit/evidence/05-requirements/rtlib-imports.md"), lines.join("\n") + "\n");
console.log(lines.join("\n"));
