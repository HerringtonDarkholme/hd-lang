import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { copyFile, mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { promisify } from "node:util";
import test from "node:test";

interface CommandResult {
  readonly stdout: string;
  readonly stderr: string;
}

interface SerializedFunction {
  readonly name?: string;
}

interface SerializedHir {
  readonly functions?: readonly SerializedFunction[];
}

const execute = promisify(execFile);
const root = resolve(import.meta.dirname, "..");
const entrypoint = resolve(root, "bin/hd.js");

async function hd(args: readonly string[], cwd = root): Promise<CommandResult> {
  return execute(process.execPath, [entrypoint, ...args], {
    cwd,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
  });
}

test("documented CLI commands work end to end", async () => {
  const core = resolve(root, "examples/core.hd");
  const suspension = resolve(root, "examples/suspension.hd");
  const runtimeFixture = resolve(root, "spec/conformance/runtime/valid/generic-data-embedding.hd");

  const parsed = await hd(["parse", core]);
  assert.match(parsed.stdout, /core\.hd: ok/);

  const checked = await hd(["check", core]);
  assert.match(checked.stdout, /core\.hd: ok/);

  const tested = await hd(["test", runtimeFixture]);
  assert.match(tested.stdout, /generic-data-embedding\.hd: 1 passed/);

  const run = await hd(["run", core]);
  assert.equal(run.stdout.trim(), "7");

  const wat = await hd(["build", "--wat", core]);
  assert.match(wat.stdout, /^\(module/m);
  assert.match(wat.stdout, /\(type \$d0 \(struct/);

  const hir = await hd(["dump-hir", core]);
  const parsedHir = JSON.parse(hir.stdout) as SerializedHir;
  assert.ok(parsedHir.functions?.some((declaration) => declaration.name === "main"));

  const requirements = await hd(["explain-requirements", suspension]);
  assert.match(requirements.stdout, /main: \$ Console/);
  assert.match(requirements.stdout, /add_two: \$ Clock/);
  assert.match(requirements.stdout, /compute: \$\(\)/);

  const trace = await hd(["trace", suspension]);
  assert.match(trace.stdout, /construct main[\s\S]*poll main[\s\S]*42[\s\S]*ready main/);

  const directory = await mkdtemp(join(tmpdir(), "hd-lang-cli-"));
  try {
    const replaySource = join(directory, "suspension.hd");
    await copyFile(suspension, replaySource);

    const recorded = await hd(["record", replaySource]);
    const replayPath = `${replaySource}.replay.json`;
    assert.match(
      recorded.stdout,
      new RegExp(`42[\\s\\S]*${basename(replayPath).replaceAll(".", "\\.")}`),
    );
    const events = JSON.parse(await readFile(replayPath, "utf8")) as unknown[];
    assert.ok(events.length > 0);

    const replayed = await hd(["replay", replaySource]);
    assert.equal(replayed.stdout.trim(), "42");

    const built = await hd(["build", replaySource], directory);
    const wasmPath = built.stdout.trim();
    assert.equal(basename(wasmPath), "suspension.wasm");
    assert.ok((await stat(wasmPath)).size > 8);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
