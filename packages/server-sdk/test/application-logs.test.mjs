import assert from "node:assert/strict";
import { AsyncLocalStorage } from "node:async_hooks";
import { test, before, after } from "node:test";
import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createCoreHost } from "../../../host/core.mts";
import { createPackageRuntime } from "../../../host/package-providers.mts";
import { createApplication } from "../adapters/application.mjs";
import { createSupervisor } from "../adapters/supervisor.mjs";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const hd = process.env.HD_SDK_TEST_HD ?? resolve(root, "target/debug/hd");
let directory, module, bindings, bindingsURL, source;
before(async () => {
  directory = mkdtempSync(join(root, ".cache-muse-sdk-logs-"));
  const app = join(directory, "app");
  cpSync(new URL("./fixtures/application-logs/", import.meta.url), app, { recursive: true });
  const manifest = join(app, "hd.toml");
  writeFileSync(
    manifest,
    readFileSync(manifest, "utf8").replace(
      'path = "../../.."',
      "path = " + JSON.stringify(join(root, "packages/server-sdk")),
    ),
  );
  execFileSync(hd, ["build"], {
    cwd: app,
    env: { ...process.env, HD_CACHE: join(directory, "cache") },
    timeout: 30000,
    stdio: "pipe",
  });
  const build = join(app, "build/debug");
  module = new WebAssembly.Module(readFileSync(join(build, "application_logs.wasm")));
  bindingsURL = pathToFileURL(
    join(
      build,
      readdirSync(build).find((f) => f.endsWith(".host.mjs")),
    ),
  ).href;
  bindings = await import(bindingsURL);
  source = bindings.interfaces.find((item) => !item.handle);
});
after(() => {
  if (directory) {
    rmSync(directory, { recursive: true, force: true });
  }
});
const request = (path) => ({
  method: { tag: "Get", fields: {} },
  url: "https://logs.test/" + path,
  headers: [["x-private", "not SDK metadata"]],
  body: new TextEncoder().encode("not SDK metadata"),
  redirects: 10,
});

test("actual concurrent HD handler and cleanup logs retain their lease and coroutine", async () => {
  const owner = new AsyncLocalStorage(),
    records = [],
    ports = [];
  const app = createApplication({
    module,
    createPackageRuntime,
    bindings: {
      ...bindings,
      bind(runtime, providers) {
        const server = providers.get(source.module),
          console = server.console;
        server.console = (id, context) => {
          const value = console(id, context);
          ports.push({ id, value });
          return value;
        };
        return bindings.bind(runtime, providers);
      },
    },
    createCoreHost,
    grants: { [source.selectors[0]]: true, Console: true, Clock: true },
    log: (record) => records.push({ owner: owner.getStore(), record }),
  });
  try {
    owner.run("startup", () => app.start());
    const slow = owner.run("A", () => app.dispatch(request("slow"))),
      fast = owner.run("B", () => app.dispatch(request("fast")));
    const results = await Promise.all([slow, fast]);
    assert.ok(results.every((result) => result.response.status === 200));
    assert.equal(records[0].record.lease, null);
    assert.equal(records[0].record.text, "startup\n");
    for (const [index, label, context] of [
      [0, "slow", "A"],
      [1, "fast", "B"],
    ]) {
      const id = ports[index].id;
      const own = records.filter(({ record }) => record.lease?.sequence === String(id.sequence));
      assert.deepEqual(
        own.map(({ owner, record }) => [owner, record.stream, record.text]),
        [
          [context, "out", `start https://logs.test/${label}\n`],
          [context, "err", `end https://logs.test/${label}\n`],
          [context, "err", `cleanup https://logs.test/${label}\n`],
        ],
      );
      assert.ok(own.every(({ record }) => record.lease.generation === String(id.generation)));
      assert.throws(() => ports[index].value.write_sync("late"), /expired request lease/);
    }
    assert.equal(records.length, 7);
    assert.equal(app.active(), 0);
  } finally {
    app.stop();
  }
});

test("actual HD Console denial returns NotGranted and produces no application records", async () => {
  const records = [];
  const app = createApplication({
    module,
    createPackageRuntime,
    bindings,
    createCoreHost,
    grants: { [source.selectors[0]]: true, Console: false, Clock: true },
    log: (record) => records.push(record),
  });
  try {
    app.start();
    const result = await app.dispatch(request("denied"));
    assert.equal(result.response.status, 200);
    assert.equal(new TextDecoder().decode(result.response.body), "stdout/stderr");
    assert.deepEqual(records, []);
  } finally {
    app.stop();
  }
});

test(
  "supervised HD output has worker/request identity and private diagnostics stay off responses",
  {
    timeout: 10000,
  },
  async () => {
    const events = [],
      logs = [],
      waiting = [];
    const supervisor = createSupervisor({
      program: {
        module,
        bindingsURL,
        runtimeURL: new URL("../../../host/core.mts", import.meta.url).href,
        packageRuntimeURL: new URL("../../../host/package-providers.mts", import.meta.url).href,
        applicationURL: new URL("../adapters/application.mjs", import.meta.url).href,
        grants: { [source.selectors[0]]: true, Console: true, Clock: true },
      },
      policy: {
        workers: 2,
        startupMs: 1500,
        requestMs: 3000,
        drainMs: 250,
        stopKillMs: 300,
        restartLimit: 1,
        restartWindowMs: 10000,
        retryAfter: "1",
      },
      event(event) {
        events.push(event);
        for (const watch of waiting.slice()) {
          if (watch.predicate(event)) {
            waiting.splice(waiting.indexOf(watch), 1);
            watch.resolve(event);
          }
        }
      },
      log(record) {
        logs.push(record);
        if (record.lease === null) {
          throw new Error("private observer failure");
        }
      },
    });
    const wait = (predicate) =>
      events.some(predicate)
        ? Promise.resolve(events.find(predicate))
        : new Promise((resolve) => waiting.push({ predicate, resolve }));
    try {
      await Promise.all([0, 1].map((slot) => wait((e) => e.kind === "ready" && e.slot === slot)));
      const results = await Promise.all([
        supervisor.dispatch(request("slow")),
        supervisor.dispatch(request("fast")),
      ]);
      assert.ok(results.every((result) => result.response.status === 200));
      assert.equal(
        logs.filter((record) => record.lease === null && record.text === "startup\n").length,
        2,
      );
      const associated = logs.filter((record) => record.lease !== null);
      assert.equal(associated.length, 6);
      for (const record of associated) {
        assert.ok([1, 2].includes(record.request));
        assert.equal(record.worker.generation, 1);
        assert.equal(record.worker.slot, record.request - 1);
        assert.ok(
          Object.isFrozen(record) &&
            Object.isFrozen(record.worker) &&
            Object.isFrozen(record.lease),
        );
        assert.equal(record.text.includes("not SDK metadata"), false);
      }
      const failed = await supervisor.dispatch(request("panic"));
      assert.equal(failed.response.status, 500);
      assert.equal(failed.response.body.length, 0);
      await wait((e) => e.kind === "ready" && e.slot === 0 && e.generation === 2);
      assert.equal(JSON.stringify(events).includes("private guest diagnostic"), false);
      assert.ok(logs.some((record) => record.text.includes("private guest diagnostic")));
      assert.equal((await supervisor.dispatch(request("fast"))).response.status, 200);
    } finally {
      await supervisor.close();
    }
  },
);
