import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import net from "node:net";
import { Dec, Enc, Err, Ok, str } from "./wire.mjs";
import { METHODS } from "./gen/abi.mjs";
import { createHost, syncClient } from "./node.mjs";

// Exercise the Windows worker branch on every OS without changing the
// production worker's API or loading a replacement process implementation.
function windowsWorker(t) {
  const root = mkdtempSync(join(tmpdir(), "hd-process-job-"));
  const entry = join(root, "entry.mjs");
  writeFileSync(
    entry,
    `Object.defineProperty(process,'platform',{value:'win32'});await import(${JSON.stringify(new URL("./process.mjs", import.meta.url).href)});`,
  );
  const client = syncClient(pathToFileURL(entry), "close");
  const call = (request) => {
    const deadline = performance.now() + 10_000;
    return client.call(request, () => deadline - performance.now());
  };
  t.after(() => {
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
    helper: (source) => {
      const path = join(root, "helper");
      writeFileSync(path, `#!${process.execPath}\n${source}`);
      chmodSync(path, 0o755);
      return path;
    },
  };
}

test("Windows processLauncher refuses absence and startup failure explicitly", async (t) => {
  const { root, call, start } = windowsWorker(t);
  const missing = start();
  assert.deepEqual(call({ op: "wait_poll", operation: missing }), {
    error: {
      code: undefined,
      message: "Windows Process requires an explicit processLauncher path",
    },
  });
  call({ op: "wait_finish", operation: missing });
  const invalid = start({ launcher: join(root, "missing-executable") });
  await t.waitFor(
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
  async (t) => {
    const { root, call, start, helper } = windowsWorker(t);
    const launcher =
      helper(`const rl=require('node:readline').createInterface({input:process.stdin});
    rl.once('line',line=>{const request=JSON.parse(line);setTimeout(()=>{
      process.stdout.write(JSON.stringify({ok:{stdout:JSON.stringify(request),stderr:'err',status:23}}));process.exit(0);
    },30);});rl.on('close',()=>process.exit(99));`);
    const operation = start({ launcher });
    let result;
    await t.waitFor(
      () => {
        result = call({ op: "wait_poll", operation });
        assert.ok(result.ok);
      },
      { timeout: 10000 },
    );
    assert.equal(result.ok.status, 23);
    assert.equal(result.ok.stderr, "err");
    const request = JSON.parse(result.ok.stdout);
    assert.deepEqual(
      [request.program, request.args, request.cwd, request.env, request.input],
      ["tool", ["one two", "'", "λ"], root, { ONLY: "value" }, "line\nλ"],
    );
    call({ op: "wait_finish", operation });
  },
);

test(
  "Windows processLauncher timeout closes its lifetime pipe and reaps before reporting",
  protocol,
  async (t) => {
    const { root, call, start, helper } = windowsWorker(t);
    const marker = join(root, "cancelled");
    const launcher = helper(
      `process.stdin.resume();process.stdin.on('end',()=>{require('node:fs').writeFileSync(${JSON.stringify(marker)},'closed');process.exit(130);});`,
    );
    const operation = start({ launcher, timeout: 100 });
    await t.waitFor(
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
  async (t) => {
    const { call, start, helper } = windowsWorker(t);
    const launcher = helper(`process.stdout.write('private malformed protocol');process.exit(0);`);
    const operation = start({ launcher });
    await t.waitFor(
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
  async (t) => {
    const { root, call, start, helper } = windowsWorker(t);
    const ready = join(root, "grandchild"),
      release = join(root, "release");
    const script = `const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(ready)},String(process.pid));const timer=setInterval(()=>{if(fs.existsSync(${JSON.stringify(release)})){clearInterval(timer);}},10);setTimeout(()=>process.exit(3),15000).unref();`;
    const launcher = helper(
      `const child=require('node:child_process').spawn(process.execPath,['-e',${JSON.stringify(script)}],{stdio:['ignore','inherit','inherit']});child.unref();process.stdout.write(JSON.stringify({ok:{stdout:'complete',stderr:'',status:0}})+'\\n');`,
    );
    t.after(() => {
      if (existsSync(ready)) {
        try {
          process.kill(Number(readFileSync(ready, "utf8")), "SIGKILL");
        } catch {}
      }
    });
    const operation = start({ launcher });
    await t.waitFor(() => assert.ok(existsSync(ready)), { timeout: 5000 });
    await t.waitFor(
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

function netHost(t, trait = "hd:Net", env = {}, make = createHost) {
  const host = make({ out() {}, err() {} }, env);
  host.core.st.deadline = performance.now() + 5000;
  t.after(() => host.close());
  const memory = new WebAssembly.Memory({ initial: trait === "hd:Process" ? 32 : 1 });
  host.prime({ exports: { "hd.x": memory } });
  const imports = host.imports[trait];
  const decode = (name, len) =>
    METHODS[trait][name].result.dec(new Dec(new Uint8Array(memory.buffer, 0, len)));
  const call = (name, ...args) => {
    const method = METHODS[trait][name];
    const encoded = new Enc(),
      raw = [];
    if (method.params.length === 1 && method.params[0] === str) {
      const bytes = new TextEncoder().encode(args[0]);
      new Uint8Array(memory.buffer).set(bytes);
      raw.push(bytes.length);
    } else {
      method.params.forEach((param, i) => {
        if (typeof param === "string") raw.push(args[i]);
        else param.enc(encoded, args[i]);
      });
      if (raw.length !== method.params.length) {
        new Uint8Array(memory.buffer).set(encoded.done());
        raw.push(encoded.len);
      }
    }
    if (method.wait === "never") return { result: decode(name, imports[name](...raw)) };
    const [status, value] = imports[`${name}.start`](...raw);
    return status === 0 ? { result: decode(name, value) } : { operation: value };
  };
  const take = (name, started) => {
    if (started.result) return started.result;
    if (!host.core.st.ops.get(started.operation).completed) host.imports["hd:rt"].block();
    return decode(name, imports[`${name}.finish`](started.operation));
  };
  return { host, call, take };
}

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "hd-job-descendant-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

function processHost(t) {
  const api = netHost(t, "hd:Process", { processLauncher: process.env.HD_PROCESS_LAUNCHER });
  return {
    ...api,
    run: (script, options = {}) =>
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
  test(`Process Job ${phase} preserves cancellation and normal completion`, async (t) => {
    const root = join(fixture(t), "descendant");
    let peer;
    const server = net.createServer((socket) => {
      peer = socket;
      socket.on("error", () => {});
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    t.after(() => {
      peer?.destroy();
      server.close();
    });
    const { host, run, take } = processHost(t);
    host.core.st.deadline = performance.now() + 20_000;
    const child = run("", {
      args: [
        fileURLToPath(new URL("./fixtures/process-descendant.mjs", import.meta.url)),
        phase,
        root,
        String(server.address().port),
      ],
    });
    await t.waitFor(
      () => {
        assert.ok(peer);
        assert.ok(existsSync(root + ".awake"));
        assert.ok(existsSync(root + ".parent"));
      },
      { timeout: 10_000 },
    );
    const pid = Number(readFileSync(root + ".awake", "utf8"));
    t.after(() => {
      try {
        process.kill(pid, "SIGKILL");
      } catch {}
    });
    const parentPid = Number(readFileSync(root + ".parent", "utf8"));
    if (phase !== "running") {
      await t.waitFor(() => assert.throws(() => process.kill(parentPid, 0)), { timeout: 5000 });
    }
    if (phase === "detached") {
      assert.deepEqual(
        take("run", child),
        Ok({ stdout: "", stderr: "", status: 0, program: process.execPath }),
      );
      // A normally completed call does not kill detached descendants.
      process.kill(pid, 0);
      peer.write("release");
      await t.waitFor(() => assert.ok(existsSync(root + ".released")), { timeout: 5000 });
    } else {
      assert.equal(host.core.st.ops.get(child.operation).completed, false);
      const closed = new Promise((resolve) => peer.once("close", resolve));
      host.imports["hd:rt"].abort(child.operation);
      await closed;
      assert.equal(existsSync(root + ".released"), false);
      await t.waitFor(() => assert.throws(() => process.kill(pid, 0)), { timeout: 5000 });
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
  (t) => {
    const api = netHost(t, "hd:Process");
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

test("Process cancellation during helper startup leaves the worker usable", (t) => {
  const { host, run, take } = processHost(t);
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
