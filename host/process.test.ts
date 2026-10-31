import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import net from "node:net";
import type { Socket } from "node:net";
import type { TestContext } from "node:test";
import { Err, Ok } from "./wire.ts";
import { syncClient } from "./node.ts";
import { netHost } from "./node-test-support.ts";

type Dynamic = ReturnType<typeof JSON.parse>;

// Exercise the Windows worker branch on every OS without changing the
// production worker's API or loading a replacement process implementation.
function windowsWorker(testContext: TestContext): Dynamic {
  const root = mkdtempSync(join(tmpdir(), "hd-process-job-"));
  const entry = join(root, "entry.ts");
  writeFileSync(
    entry,
    `Object.defineProperty(process,'platform',{value:'win32'});process.env.HD_PROCESS_TEST_ROOT=${JSON.stringify(root)};await import(${JSON.stringify(new URL("./process.ts", import.meta.url).href)});`,
  );
  const client = syncClient(pathToFileURL(entry), "close");
  const call = (request: Dynamic): Dynamic => {
    const deadline = performance.now() + 10_000;
    return client.call(request, () => deadline - performance.now());
  };
  testContext.after(() => {
    call({ op: "close" });
    client.stop();
    rmSync(root, { recursive: true, force: true });
  });
  return {
    root,
    call,
    start: (more = {}) =>
      call({
        op: "wait_start",
        program: "tool",
        args: ["one two", "'", "λ"],
        cwd: root,
        env: { ONLY: "value" },
        input: "line\nλ",
        timeout: Infinity,
        ...more,
      }).ok,
    helper: (source: string, interpreter = process.execPath) => {
      const path = join(root, "helper");
      writeFileSync(path, `#!${interpreter}\n${source}`);
      chmodSync(path, 0o755);
      return path;
    },
  };
}

test("Windows processLauncher refuses absence and startup failure explicitly", async (testContext) => {
  const { root, call, start } = windowsWorker(testContext);
  const missing = start();
  assert.deepEqual(call({ op: "wait_poll", operation: missing }), {
    error: {
      code: undefined,
      message: "Windows Process requires an explicit processLauncher path",
    },
  });
  call({ op: "wait_finish", operation: missing });
  const invalid = start({ launcher: join(root, "missing-executable") });
  await testContext.waitFor(
    () => {
      const result = call({ op: "wait_poll", operation: invalid });
      assert.equal(result.error?.code, undefined, "launcher failure is Other, not guest NotFound");
      assert.match(result.error?.message ?? "", /cannot start Windows processLauncher/);
    },
    { timeout: 10000 },
  );
  call({ op: "wait_finish", operation: invalid });
});

// These protocol controls use a POSIX shebang fixture; the real Windows
// executable is exercised by the CLI's Node Process wrapper, never skipped.
const protocol = { skip: process.platform === "win32" };
test(
  "Windows processLauncher preserves protocol fields and an open lifetime pipe",
  protocol,
  async (testContext) => {
    const { root, call, start, helper } = windowsWorker(testContext);
    const state = join(root, "protocol-state.json");
    const requestPath = join(root, "protocol-request.json");
    const launcher = helper(
      `state="$HD_PROCESS_TEST_ROOT/protocol-state.json"
request="$HD_PROCESS_TEST_ROOT/protocol-request.json"
mark() { printf '{"stage":"%s","pid":%s}' "$1" "$$" > "$state"; }
mark started
IFS= read -r line
printf '%s' "$line" > "$request"
mark request-read
printf '%s\\n' '{"ok":{"stdout":"complete","stderr":"err","status":23}}'
mark reply-flushed`,
      "/bin/sh",
    );
    const operation = start({ launcher });
    let result: Dynamic;
    await testContext.waitFor(
      () => {
        result = call({ op: "wait_poll", operation });
        const launcherState = existsSync(state) ? readFileSync(state, "utf8") : "not-started";
        assert.ok(result.ok, `poll=${JSON.stringify(result)} launcher=${launcherState}`);
      },
      { timeout: 10000 },
    );
    assert.equal(result.ok.status, 23);
    assert.equal(result.ok.stderr, "err");
    assert.equal(result.ok.stdout, "complete");
    const request = JSON.parse(readFileSync(requestPath, "utf8"));
    assert.deepEqual(
      [request.program, request.args, request.cwd, request.env, request.input],
      ["tool", ["one two", "'", "λ"], root, { ONLY: "value" }, "line\nλ"],
    );
    assert.equal(JSON.parse(readFileSync(state, "utf8")).stage, "reply-flushed");
    call({ op: "wait_finish", operation });
  },
);

test(
  "Windows processLauncher timeout closes its lifetime pipe and reaps before reporting",
  protocol,
  async (testContext) => {
    const { root, call, start, helper } = windowsWorker(testContext);
    const marker = join(root, "cancelled");
    const launcher = helper(
      `process.stdin.resume();process.stdin.on('end',()=>{require('node:fs').writeFileSync(${JSON.stringify(marker)},'closed');process.exit(130);});`,
    );
    const operation = start({ launcher, timeout: 100 });
    await testContext.waitFor(
      () => assert.deepEqual(call({ op: "wait_poll", operation }), { timeout: true }),
      { timeout: 10000 },
    );
    assert.ok(existsSync(marker));
    call({ op: "wait_finish", operation });
  },
);

test(
  "Windows processLauncher rejects malformed results without leaking protocol output",
  protocol,
  async (testContext) => {
    const { call, start, helper } = windowsWorker(testContext);
    const launcher = helper(`process.stdout.write('private malformed protocol');process.exit(0);`);
    const operation = start({ launcher });
    await testContext.waitFor(
      () =>
        assert.deepEqual(call({ op: "wait_poll", operation }), {
          error: { code: undefined, message: "invalid Windows processLauncher result" },
        }),
      { timeout: 10000 },
    );
    call({ op: "wait_finish", operation });
  },
);

test(
  "Windows processLauncher result does not wait for inherited protocol pipes",
  protocol,
  async (testContext) => {
    const { root, call, start, helper } = windowsWorker(testContext);
    const ready = join(root, "grandchild"),
      release = join(root, "release");
    const script = `const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(ready)},String(process.pid));const timer=setInterval(()=>{if(fs.existsSync(${JSON.stringify(release)})){clearInterval(timer);}},10);setTimeout(()=>process.exit(3),15000).unref();`;
    const launcher = helper(
      `const child=require('node:child_process').spawn(process.execPath,['-e',${JSON.stringify(script)}],{stdio:['ignore','inherit','inherit']});child.unref();process.stdout.write(JSON.stringify({ok:{stdout:'complete',stderr:'',status:0}})+'\\n');`,
    );
    testContext.after(() => {
      if (existsSync(ready)) {
        try {
          process.kill(Number(readFileSync(ready, "utf8")), "SIGKILL");
        } catch {}
      }
    });
    const operation = start({ launcher });
    await testContext.waitFor(() => assert.ok(existsSync(ready)), { timeout: 5000 });
    await testContext.waitFor(
      () =>
        assert.deepEqual(call({ op: "wait_poll", operation }), {
          ok: { stdout: "complete", stderr: "", status: 0 },
        }),
      { timeout: 5000 },
    );
    process.kill(Number(readFileSync(ready, "utf8")), 0);
    call({ op: "wait_finish", operation });
    writeFileSync(release, "");
  },
);

function fixture(testContext: TestContext): string {
  const root = mkdtempSync(join(tmpdir(), "hd-job-descendant-"));
  testContext.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

function processHost(testContext: TestContext): Dynamic {
  const api = netHost(testContext, "hd:Process", {
    processLauncher: process.env.HD_PROCESS_LAUNCHER,
  });
  return {
    ...api,
    run: (script: string, options: Dynamic = {}) =>
      api.call("run", {
        program: process.execPath,
        args: ["-e", script],
        env: [],
        current_dir: null,
        stdin: "",
        ...options,
      }),
  };
}

// The maintained fixture is also driven by the CLI wrapper on Windows,
// with the exact CARGO_BIN_EXE_hd launcher supplied explicitly.
for (const phase of ["running", "leader-exited", "detached"]) {
  test(`Process Job ${phase} preserves cancellation and normal completion`, async (testContext) => {
    const root = join(fixture(testContext), "descendant");
    let peer: Socket | undefined;
    const server = net.createServer((socket) => {
      peer = socket;
      socket.on("error", () => {});
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    testContext.after(() => {
      peer?.destroy();
      server.close();
    });
    const { host, run, take } = processHost(testContext);
    host.core.st.deadline = performance.now() + 20_000;
    const child = run("", {
      args: [
        fileURLToPath(new URL("./fixtures/process-descendant.ts", import.meta.url)),
        phase,
        root,
        String((server.address() as net.AddressInfo).port),
      ],
    });
    await testContext.waitFor(
      () => {
        assert.ok(peer);
        assert.ok(existsSync(root + ".awake"));
        assert.ok(existsSync(root + ".parent"));
      },
      { timeout: 10_000 },
    );
    const pid = Number(readFileSync(root + ".awake", "utf8"));
    testContext.after(() => {
      try {
        process.kill(pid, "SIGKILL");
      } catch {}
    });
    const parentPid = Number(readFileSync(root + ".parent", "utf8"));
    if (phase !== "running") {
      await testContext.waitFor(() => assert.throws(() => process.kill(parentPid, 0)), {
        timeout: 5000,
      });
    }
    if (phase === "detached") {
      assert.deepEqual(
        take("run", child),
        Ok({ stdout: "", stderr: "", status: 0, program: process.execPath }),
      );
      // A normally completed call does not kill detached descendants.
      process.kill(pid, 0);
      peer!.write("release");
      await testContext.waitFor(() => assert.ok(existsSync(root + ".released")), { timeout: 5000 });
    } else {
      assert.equal(host.core.st.ops.get(child.operation).completed, false);
      const closed = new Promise<void>((resolve) => peer!.once("close", resolve));
      host.imports["hd:rt"].abort(child.operation);
      await closed;
      assert.equal(existsSync(root + ".released"), false);
      await testContext.waitFor(() => assert.throws(() => process.kill(pid, 0)), { timeout: 5000 });
      // Cancellation does not poison a sibling/successor command.
      assert.equal(take("run", run("process.stdout.write('next')")).value.stdout, "next");
    }
  });
}

test(
  "Process standalone Windows host refuses a missing launcher as Other",
  {
    skip: process.platform !== "win32",
  },
  (testContext) => {
    const api = netHost(testContext, "hd:Process");
    const child = api.call("run", {
      program: process.execPath,
      args: ["-e", "process.exit(99)"],
      env: [],
      current_dir: null,
      stdin: "",
    });
    assert.deepEqual(
      api.take("run", child),
      Err({ tag: "Other", message: "Windows Process requires an explicit processLauncher path" }),
    );
  },
);

test("Process cancellation during helper startup leaves the worker usable", (testContext) => {
  const { host, run, take } = processHost(testContext);
  host.core.st.deadline = performance.now() + 20_000;
  for (let i = 0; i < 8; i++) {
    const child = run("setInterval(()=>{},1000)");
    assert.equal(typeof child.operation, "number");
    host.imports["hd:rt"].abort(child.operation);
  }
  assert.equal(
    take("run", run("process.stdout.write('after startup aborts')")).value.stdout,
    "after startup aborts",
  );
});
