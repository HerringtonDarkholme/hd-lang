import assert from "node:assert/strict";
import test from "node:test";

import { analyze } from "../compiler.ts";

function unusedNames(source: string): string[] {
  const result = analyze(source);
  assert.ok(result.hir);
  return result.diagnostics
    .filter((diagnostic) => diagnostic.code === "unused-local-binding")
    .map((diagnostic) => diagnostic.message.match(/'([^']+)'/)?.[1] ?? diagnostic.message);
}

test("unused-local warnings name exactly the unread bindings", () => {
  const names = unusedNames(
    [
      "fn run(flag: bool) -> i32:",
      "    let total: i32 = 0",
      "    let used1 = 1",
      "    let unused1 = 2",
      "    let used2 = used1 + 3",
      "    let unused2 = 4",
      "    let f = fn(x: i32) -> i32: x + used2",
      "    let unused3 = 5",
      "    if flag:",
      "        total = total + f(used1)",
      "    else:",
      "        total = total - used2",
      "    let (pair1, _) = (used1, unused1)",
      "    total = total + pair1",
      "    total",
      "",
    ].join("\n"),
  );
  // unused1 is read: building the (used1, unused1) tuple reads it even though
  // the slot destructures to _. Only the never-read bindings warn.
  assert.deepEqual(names, ["unused2", "unused3"]);
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
