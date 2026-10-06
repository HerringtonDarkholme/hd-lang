import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { runHd } from "./hd-in-process.ts";

// The default profile's `Process` (spec/cli/command-line.md#host-capabilities)
// and its grant (spec/cli/command-line.md#r-cli.cap.scope.process). The only
// program a test starts is this Node executable, on a one-line script.

const NODE = process.execPath;

const PROGRAM = `use std.process.{Process, ProcessError}

fn start!(program: string, args: List[string]) -> string $ Process:
    match $.use(Process).run!(program, args, "from stdin"):
        .Ok(output) => "status \${output.status} out \${output.stdout} err \${output.stderr}"
        .Err(.NotGranted) => "not granted"
        .Err(.NotFound) => "not found"
        .Err(error) => "error \${error}"

pub fn main!() -> void $ Console + Process:
    script := "process.stdout.write(process.env.HD_N3_MARK ?? 'unset'); process.stdin.pipe(process.stderr); process.exitCode = 3"
    println(start!(${JSON.stringify(NODE)}, ["-e", script]))
    println(start!("hd-n3-no-such-program", []))
`;

async function run(
  flags: readonly string[],
): Promise<{ status: number; stdout: string; stderr: string }> {
  const directory = await mkdtemp(join(tmpdir(), "hd-process-host-"));
  try {
    await writeFile(join(directory, "start.hd"), PROGRAM);
    return await runHd(["start.hd", ...flags], {
      cwd: directory,
      variables: { ...process.env, HD_N3_MARK: "inherited" },
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("Process starts a host program with the whole environment, whatever Env grants", async () => {
  const { status, stdout, stderr } = await run(["--cap", "Env=HOME"]);
  assert.equal(status, 0, stderr);
  assert.equal(stdout, "status 3 out inherited err from stdin\nnot found\n");
});

test("a program outside the Process grant is NotGranted, and false refuses the start", async () => {
  const listed = await run(["--cap", `Process=${NODE}`]);
  assert.equal(listed.stdout, "status 3 out inherited err from stdin\nnot granted\n");
  const other = await run(["--cap", "Process=git"]);
  assert.equal(other.stdout, "not granted\nnot granted\n");
  const denied = await run(["--cap", "Process=false"]);
  assert.equal(denied.status, 101);
  assert.equal(denied.stdout, "");
  assert.equal(denied.stderr, "hd: the program needs Process, which --cap Process=false denies\n");
});
