// Fails if any file under audit/fuzz imports something other than a Node
// built-in, a sibling module inside audit/fuzz, or a module under spec/.
//   node --experimental-strip-types audit/fuzz/check-imports.ts
import { readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";

import { repoRoot, specFiles } from "./common.ts";

const fuzzRoot = import.meta.dirname;
const specRoot = resolve(repoRoot, "spec");
const failures: string[] = [];
const pattern =
  /(?:^|\s)(?:import|export)\s[^"'`;]*?from\s*["'`]([^"'`]+)["'`]|import\s*\(\s*["'`]([^"'`]+)["'`]|^\s*import\s*["'`]([^"'`]+)["'`]/gm;

for (const file of specFiles(fuzzRoot, ".ts")) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(pattern)) {
    const specifier = match[1] ?? match[2] ?? match[3]!;
    const where = relative(repoRoot, file);
    if (specifier.startsWith("node:")) continue;
    if (!specifier.startsWith(".")) {
      failures.push(`${where}: package import ${specifier}`);
      continue;
    }
    const target = resolve(dirname(file), specifier);
    const inside = (root: string): boolean => !relative(root, target).startsWith("..");
    if (!inside(fuzzRoot) && !inside(specRoot))
      failures.push(`${where}: imports ${relative(repoRoot, target)}`);
  }
  // Belt and braces: no string that points into the compiler source tree.
  for (const line of text.split("\n"))
    if (/["'`](?:\.\.\/)+src\/|["'`]\/?src\//.test(line) && !line.includes("check-imports: allow"))
      failures.push(`${relative(repoRoot, file)}: references src/: ${line.trim()}`);
}

if (failures.length) {
  process.stderr.write(`${failures.join("\n")}\n`);
  process.exitCode = 1;
} else process.stdout.write("check-imports: ok (node built-ins, audit/fuzz, spec/ only)\n");
