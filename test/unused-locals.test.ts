import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../src/compiler.ts";

function unusedNames(source: string): string[] {
  const result = analyze(source);
  assert.ok(result.hir);
  return result.diagnostics
    .filter((diagnostic) => diagnostic.code === "unused-local-binding")
    .map((diagnostic) => diagnostic.message.match(/'([^']+)'/)?.[1] ?? diagnostic.message);
}

test("annotated lets warn exactly like unannotated ones", () => {
  const lines = ["fn run() -> i32:", "    let total: i32 = 0"];
  for (let index = 0; index < 200; index += 1) {
    lines.push(`    let used${index}: i32 = ${index}`);
    lines.push(`    let unread${index}: i32 = ${index}`);
    lines.push(`    total = total + used${index}`);
  }
  lines.push("    total", "");
  const names = unusedNames(`${lines.join("\n")}\n`);
  assert.equal(names.length, 200);
  assert.ok(names.every((name) => name.startsWith("unread")));
});

test("unused-local detection covers a many-local body", () => {
  const lines = ["fn run() -> i32:", "    let total: i32 = 0"];
  for (let index = 0; index < 500; index += 1) {
    lines.push(`    let used${index} = ${index}`);
    lines.push(`    let unread${index} = ${index}`);
    lines.push(`    total = total + used${index}`);
  }
  lines.push("    total", "");
  const names = unusedNames(`${lines.join("\n")}\n`);
  assert.equal(names.length, 500);
  assert.ok(names.every((name) => name.startsWith("unread")));
});
