// 5.4: list allocation instructions per function for a probe's emitted WAT.
// Run: node --experimental-strip-types audit/scripts/arch/req-constructs.ts FILE.hd [OUT.md]
import { readFileSync, writeFileSync } from "node:fs";

import { compilePhased, functionBodies, header } from "./bench-lib.ts";

const [file, out] = process.argv.slice(2);
if (!file) throw new Error("usage: req-constructs.ts FILE.hd [OUT.md]");
const source = readFileSync(file, "utf8");
const { wat } = compilePhased(source);
const lines: string[] = ["| function | allocation instructions (type: count) | helper calls |", "| --- | --- | --- |"];
for (const [name, body] of functionBodies(wat)) {
  if (name.startsWith("$hd.")) continue;
  const counts = new Map<string, number>();
  for (const match of body.matchAll(/\((struct\.new(?:_default)?|array\.new(?:_fixed|_default|_data)?) (\$[^\s)]+)/g)) {
    const key = `${match[1]} ${match[2]}`;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const helpers = new Map<string, number>();
  for (const match of body.matchAll(/\(call (\$hd\.provider_[a-z]+)/g))
    helpers.set(match[1]!, (helpers.get(match[1]!) ?? 0) + 1);
  const signature = /\(export "([^"]+)"\)/.exec(body.split("\n")[0] ?? "")?.[1];
  lines.push(
    `| ${name}${signature ? ` (${signature})` : ""} | ${[...counts].map(([key, count]) => `${key}: ${count}`).join("; ") || "-"} | ${[...helpers].map(([key, count]) => `${key} x${count}`).join("; ") || "-"} |`,
  );
}
const text = header(`node --experimental-strip-types audit/scripts/arch/req-constructs.ts ${file}`) + "\n" + lines.join("\n") + "\n";
if (out) writeFileSync(out, text);
console.log(text);
