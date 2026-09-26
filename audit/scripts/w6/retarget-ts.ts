// Rewrites fixture("old/path") and fixtureBody(...) calls in test/*.test.ts to conformance("new/path")
// for every file W6 moved or deleted, using audit/evidence/w6/moves.tsv.
// Usage: node --experimental-strip-types audit/scripts/w6/retarget-ts.ts
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");
const moves = new Map<string, string>();
for (const line of readFileSync(resolve(root, "audit/evidence/w6/moves.tsv"), "utf8")
  .split("\n")
  .slice(1)) {
  const [source = "", action = "", destination = ""] = line.split("\t");
  if (!source || action === "held back") continue;
  moves.set(
    source.replace(/^test\/fixtures\//, "").replace(/\.hd$/, ""),
    destination.replace(/^spec\/conformance\//, "").replace(/\.hd$/, ""),
  );
}
for (const file of readdirSync(resolve(root, "test")).filter((name) => name.endsWith(".test.ts"))) {
  const path = resolve(root, "test", file);
  const before = readFileSync(path, "utf8");
  let used = false;
  let after = before.replace(
    /\bfixture(Body)?\((\s*)"([^"]+)"/g,
    (match, body: string | undefined, space: string, old: string) => {
      const target = moves.get(old);
      if (!target) return match;
      used = true;
      return `conformance${body ?? ""}(${space}"${target}"`;
    },
  );
  if (!used) continue;
  after = after.replace(/import \{ ([^}]*) \} from "\.\/fixture\.ts";/, (match, list: string) => {
    const names = new Set(list.split(",").map((name) => name.trim()));
    for (const name of ["fixture", "fixtureBody", "conformance", "conformanceBody"])
      if (new RegExp(`\\b${name}\\(`).test(after)) names.add(name);
      else names.delete(name);
    return `import { ${[...names].filter(Boolean).sort().join(", ")} } from "./fixture.ts";`;
  });
  writeFileSync(path, after);
  console.log(`retargeted ${file}`);
}
