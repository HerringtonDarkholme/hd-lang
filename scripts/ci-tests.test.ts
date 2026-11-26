import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./ci-tests.sh", import.meta.url));

function run(body: string, limit = "10") {
  const dir = mkdtempSync(join(tmpdir(), "hd-ci-tests-"));
  try {
    for (const path of ["target", "crates/hd_command", "crates/hd_run_wasmtime"]) {
      mkdirSync(join(dir, path), { recursive: true });
    }
    writeFileSync(join(dir, "target/probe child"), `#!/usr/bin/env bash\n${body}\n`, {
      mode: 0o755,
    });
    writeFileSync(join(dir, "target/next"), '#!/usr/bin/env bash\nprintf "next binary\\n"\n', {
      mode: 0o755,
    });
    writeFileSync(
      join(dir, "target/ci-tests.tsv"),
      "crates/hd_command\tall\ttarget/probe child\ncrates/hd_run_wasmtime\tall\ttarget/next\n",
    );
    const result = spawnSync("bash", [script, "^crates/", "an argument with spaces", "--exact"], {
      cwd: dir,
      env: { ...process.env, CI_TEST_LIMIT: limit },
      encoding: "utf8",
      timeout: 20_000,
    });
    assert.ifError(result.error);
    assert.equal(result.signal, null, result.stdout + result.stderr);
    const resources = join(dir, "target/ci-resources/hd_command-all");
    if (process.platform === "linux" && existsSync("/usr/bin/time")) {
      assert.match(readFileSync(join(resources, "time.txt"), "utf8"), /Maximum resident set size/);
      for (const phase of ["before", "after"]) {
        assert.match(readFileSync(join(resources, `memory-${phase}.txt`), "utf8"), /cgroup:/);
      }
    }
    return { status: result.status, output: result.stdout + result.stderr };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test(
  "CI runner preserves arguments and continues after a successful binary",
  { skip: process.platform === "win32" },
  () => {
    const { status, output } = run('printf "argument=<%s>\\n" "$@"');
    assert.equal(status, 0, output);
    assert.match(output, /argument=<an argument with spaces>\nargument=<--exact>/);
    assert.match(output, /next binary/);
  },
);

test(
  "CI runner reports an early SIGKILL and still runs the next binary",
  { skip: process.platform === "win32" },
  () => {
    // Only this disposable child is killed; no process-group signal is sent.
    const { status, output } = run('kill -KILL "$$"');
    assert.equal(status, 1, output);
    assert.match(output, /crates\/hd_command all was killed by signal 9/);
    assert.doesNotMatch(output, /ran over/);
    assert.match(output, /next binary/);
  },
);

test(
  "CI runner distinguishes its deadline from an early kill and continues",
  { skip: process.platform === "win32" },
  () => {
    const { status, output } = run("sleep 5", "1");
    assert.equal(status, 1, output);
    assert.match(output, /crates\/hd_command all ran over 1 s and was stopped/);
    assert.doesNotMatch(output, /was killed by signal/);
    assert.match(output, /next binary/);
  },
);

test(
  "CI runner records an external signal before early kill-after escalation",
  { skip: process.platform === "win32" },
  () => {
    // This disposable child's direct parent is timeout, in its own group.
    // USR1 is ignored by the child, so timeout's ten-second escalation fires
    // before the thirty-second command deadline, reproducing #2371's shape.
    const { status, output } = run('trap "" USR1; kill -USR1 "$PPID"; sleep 15', "30");
    assert.equal(status, 1, output);
    assert.match(output, /sending signal (?:SIG)?USR1 to command/);
    assert.match(output, /crates\/hd_command all was killed by signal 9/);
    assert.doesNotMatch(output, /ran over/);
    assert.match(output, /next binary/);
  },
);
