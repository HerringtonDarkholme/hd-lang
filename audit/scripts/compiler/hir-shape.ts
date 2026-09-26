// Prints a compact kind-tree of HIR functions from `hd dump-hir` JSON.
// Usage: node --experimental-strip-types hir-shape.ts HIR.json [functionName...]
import { readFileSync } from "node:fs";

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

const SKIP = new Set(["span", "local", "global", "type", "locals", "parameters"]);

function shape(node: Json, depth: number): string {
  if (Array.isArray(node)) return node.map((child) => shape(child, depth)).join("");
  if (node === null || typeof node !== "object") return "";
  const kind = typeof node["kind"] === "string" ? node["kind"] : undefined;
  let text = "";
  if (kind !== undefined) {
    const extras: string[] = [];
    for (const key of ["functionName", "key", "iteratorKind", "value"]) {
      const value = node[key];
      if (typeof value === "string" || typeof value === "number") extras.push(`${key}=${value}`);
    }
    if (typeof node["type"] === "string") extras.push(`: ${node["type"]}`);
    if (node["defaultArguments"]) extras.push(`defaults=${JSON.stringify(node["defaultArguments"])}`);
    text += `${"  ".repeat(depth)}${kind} ${extras.join(" ")}\n`;
  }
  for (const [key, value] of Object.entries(node)) {
    if (SKIP.has(key) || key === "kind") continue;
    text += shape(value, kind === undefined ? depth : depth + 1);
  }
  return text;
}

const [file, ...names] = process.argv.slice(2);
if (file === undefined) throw new Error("usage: hir-shape.ts HIR.json [name...]");
const program = JSON.parse(readFileSync(file, "utf8")) as { [key: string]: Json };
const functions = [...(program["functions"] as Json[]), ...(program["closures"] as Json[])];
for (const declaration of functions) {
  if (declaration === null || typeof declaration !== "object" || Array.isArray(declaration)) continue;
  const name = String(declaration["name"]);
  if (names.length > 0 && !names.includes(name)) continue;
  console.log(`== ${name} synthetic=${String(declaration["synthetic"])} requirements=${JSON.stringify(declaration["requirements"])}`);
  console.log(shape(declaration["body"] ?? null, 1));
}
