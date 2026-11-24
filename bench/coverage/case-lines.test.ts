import assert from "node:assert/strict";
import { test } from "node:test";
import { caseLineIndex } from "./case-lines.ts";

test("per-case counters stay separate and zero-hit source lines remain in the dictionary", () => {
  const file = "crates/hd_check/src/value.rs";
  const source = "fn value() {\n    first();\n    second();\n}\n";
  const data = (first: number, second: number) =>
    `SF:/repo/${file}\nDA:2,${first}\nDA:3,${second}\nend_of_record\n`;
  let reads = 0;
  const index = caseLineIndex(data(1, 0), () => {
    reads++;
    return source;
  });
  assert.deepEqual(index.lines, [`${file}:2`, `${file}:3`]);
  assert.deepEqual(index.covered(data(1, 0)), [0]);
  assert.deepEqual(index.covered(data(0, 4)), [1]);
  assert.equal(reads, 1);
});

test("ordinary core lines exclude invariant, inline tests, out-of-line tests and non-core sources", () => {
  const sources = new Map([
    [
      "crates/hd_check/src/lib.rs",
      'fn check() {\n    work();\n    panic!("invariant");\n}\n#[cfg(test)]\nmod checks;\n',
    ],
    ["crates/hd_check/src/checks.rs", "fn test_helper() {\n    helper();\n}\n"],
  ]);
  const lcov = [...sources.keys(), "crates/hd_driver/src/lib.rs"]
    .map((file) => `SF:/repo/${file}\nDA:2,1\nDA:3,1\nend_of_record\n`)
    .join("");
  const index = caseLineIndex(lcov, (file) => {
    const source = sources.get(file);
    assert(source);
    return source;
  });
  assert.deepEqual(index.lines, ["crates/hd_check/src/lib.rs:2"]);
  assert.deepEqual(index.covered(lcov), [0]);
});
