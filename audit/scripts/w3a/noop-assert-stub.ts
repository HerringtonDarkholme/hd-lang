// W3a non-vacuity stub: an implementation under test whose `assert_equal`
// never fails.
//
// It follows the command contract in spec/conformance/README.md:
//   noop-assert-stub.ts ACTION [OPTION VALUE]... FILE
//
// `parse` and `check` run the real compiler on FILE unchanged. For `test`, it
// writes a temporary copy of FILE in which every `assert_equal(` call is
// renamed to `noop_assert_equal(`, a helper appended to the copy that takes
// the same arguments and does nothing. The helper's `T` is unbounded, so the
// MVP gap F-500 (primitives do not satisfy a `T: PartialEq` bound) does not
// make the stub fail for an unrelated reason. Line numbers are preserved. The
// real compiler then runs on the copy, and the copy's path is rewritten to
// FILE in the output, so located reports still name the original fixture.
//
// Usage with the neutral runner:
//   node --experimental-strip-types spec/tools/run-conformance.ts \
//     --compiler "node --experimental-strip-types audit/scripts/w3a/noop-assert-stub.ts" \
//     --manifest audit/evidence/w3a/manifest.txt
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";

const repoRoot = resolve(import.meta.dirname, "../../..");
const compiler = ["--experimental-strip-types", join(repoRoot, "bin/hd.js")];

const helper = `

fn noop_assert_equal[T](actual: T, expected: T, reason: string) -> void:
    return
`;

function run(args: readonly string[], rewrite?: { from: string; to: string }): number {
  const result = spawnSync(process.execPath, [...compiler, ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  const fix = (text: string): string =>
    rewrite ? text.split(rewrite.from).join(rewrite.to) : text;
  process.stdout.write(fix(result.stdout ?? ""));
  process.stderr.write(fix(result.stderr ?? ""));
  return result.status ?? 1;
}

function main(): number {
  const args = process.argv.slice(2);
  const file = args.at(-1);
  if (args[0] !== "test" || file === undefined) return run(args);
  const source = readFileSync(file, "utf8");
  const stubbed = source.replace(/\bassert_equal\(/g, "noop_assert_equal(") + helper;
  const dir = mkdtempSync(join(tmpdir(), "w3a-noop-"));
  const copy = join(dir, basename(file));
  try {
    writeFileSync(copy, stubbed);
    return run([...args.slice(0, -1), copy], { from: copy, to: file });
  } finally {
    rmSync(dir, { force: true, recursive: true });
  }
}

process.exitCode = main();
