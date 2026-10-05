import { strict as assert } from "node:assert";
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { resolve } from "node:path";
import { promisify } from "node:util";

import {
  failVerdicts,
  parseOptions,
  passPaths,
  runConformanceOnce,
  selectionManifest,
  timeoutPaths,
} from "./run-portable.ts";

const execFileAsync = promisify(execFile);
const root = resolve(import.meta.dirname, "..");

test("--only collects case paths", () => {
  const options = parseOptions(["--only", "typing/invalid/foo.hd", "--only", "cli/bar"]);
  assert.deepEqual(options.only, ["typing/invalid/foo.hd", "cli/bar"]);
});

test("--only defaults to no selection", () => {
  assert.deepEqual(parseOptions([]).only, []);
});

test("--only cannot combine with --changed", () => {
  assert.throws(() => parseOptions(["--only", "a.hd", "--changed"]), /--only cannot be combined/);
});

test("--only cannot use the fixtures suite", () => {
  assert.throws(() => parseOptions(["--only", "a.hd", "--suite", "fixtures"]), /--suite fixtures/);
});

test("selectionManifest names exactly the selected paths", async () => {
  const path = await selectionManifest(["typing/invalid/foo.hd", "cli/bar"]);
  assert.equal(await readFile(path, "utf8"), "path\ntyping/invalid/foo.hd\ncli/bar\n");
});

const sample = [
  "pass  typing/valid/foo.hd",
  "FAIL  typing/invalid/slow.hd: check: ran longer than 10 s",
  "FAIL  typing/invalid/broken.hd: check: expected exit 0, got exit 1",
  "      | src/main.hd:3:5: type-mismatch: x",
  "FAIL  cli/slow.hd: step 1 (hd check): ran longer than 10 s",
  "FAIL  typing/invalid/odd.hd: ran longer than 10 s and then recovered",
  "conformance: 1 passed, 4 failed, 5 selected (language: 1 of 5; stdlib: 0 of 0; cli: 0 of 0)",
].join("\n");

test("failVerdicts splits verdict lines into path and reason", () => {
  assert.deepEqual(failVerdicts(sample), [
    { path: "typing/invalid/slow.hd", reason: "check: ran longer than 10 s" },
    { path: "typing/invalid/broken.hd", reason: "check: expected exit 0, got exit 1" },
    { path: "cli/slow.hd", reason: "step 1 (hd check): ran longer than 10 s" },
    { path: "typing/invalid/odd.hd", reason: "ran longer than 10 s and then recovered" },
  ]);
});

test("a bare timeout and each step label count as a timeout", () => {
  assert.deepEqual(timeoutPaths(sample), ["typing/invalid/slow.hd", "cli/slow.hd"]);
  assert.deepEqual(timeoutPaths("FAIL  a.hd: run: ran longer than 10 s\n"), ["a.hd"]);
  assert.deepEqual(timeoutPaths("FAIL  a.hd: ran longer than 10 s\n"), ["a.hd"]);
});

test("passPaths collects passes", () => {
  assert.deepEqual([...passPaths(sample)], ["typing/valid/foo.hd"]);
});

test("--only runs one case through the real harness", async () => {
  const selected = "parse/valid/functions-and-closures.hd";
  const { stdout } = await execFileAsync(
    process.execPath,
    [
      "--experimental-strip-types",
      resolve(root, "test/run-portable.ts"),
      "--suite",
      "conformance",
      "--only",
      selected,
    ],
    { cwd: root },
  );
  assert.match(stdout, new RegExp(`pass  ${selected}\n`));
  assert.match(stdout, /1 selected/);
});

test("the serial retry path runs one case with one job", async () => {
  const options = parseOptions(["--suite", "conformance"]);
  const manifest = await selectionManifest(["parse/valid/expressions.hd"]);
  const retry = await runConformanceOnce(options, manifest, 1);
  assert.equal(retry.code, 0);
  assert.ok(passPaths(retry.output).has("parse/valid/expressions.hd"));
});
