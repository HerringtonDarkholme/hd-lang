import assert from "node:assert/strict";
import http from "node:http";
import { once } from "node:events";
import { readFileSync, cpSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test, before, after } from "node:test";
import { createSupervisor } from "../adapters/supervisor.mjs";
import { createFetchHandler } from "../adapters/fetch.mjs";
import { nativeListener } from "./helpers/native-listener.mjs";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const hd = process.env.HD_SDK_TEST_HD ?? join(root, "target/debug/hd");
const builds = new Map();
let directory;
before(() => {
  directory = mkdtempSync(join(root, ".cache-muse-supervised-"));
  for (const [program, fixture] of [
    ["lifecycle", "supervised-application"],
    ["main-cpu", "supervised-main-cpu"],
  ]) {
    const app = join(directory, program);
    cpSync(new URL("./fixtures/" + fixture + "/", import.meta.url), app, { recursive: true });
    const manifest = join(app, "hd.toml");
    writeFileSync(
      manifest,
      readFileSync(manifest, "utf8").replace(
        'path = "../../.."',
        "path = " + JSON.stringify(join(root, "packages/server-sdk")),
      ),
    );
    const options = {
      cwd: app,
      env: { ...process.env, HD_CACHE: join(directory, "cache") },
      stdio: "pipe",
      timeout: 30000,
    };
    execFileSync(hd, ["check", "--format", "json"], options);
    execFileSync(hd, ["build", "--format", "json"], options);
    builds.set(program, pathToFileURL(join(app, "build/debug/")).href);
  }
});
after(() => {
  if (directory) rmSync(directory, { recursive: true, force: true });
});
const tick = () => new Promise((resolve) => setImmediate(resolve));
function bounded(promise, ms = 4000) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("bounded control did not complete")), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}
async function upstream(serve) {
  const server = http.createServer(serve);
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    async close() {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
async function setup({
  workers = 1,
  Http = true,
  deadline = 1000,
  program = "lifecycle",
  ready = true,
  startupMs = 1500,
  cleanupClock = null,
} = {}) {
  const name = program.replaceAll("-", "_");
  const build = new URL(builds.get(program));
  const bindingsURL = new URL(name + ".host.mjs", build).href;
  const bindings = await import(bindingsURL),
    source = bindings.interfaces.find((item) => !item.handle);
  const module = new WebAssembly.Module(readFileSync(new URL(name + ".wasm", build)));
  const events = [],
    waiting = [],
    logs = [];
  const supervisor = createSupervisor({
    program: Object.freeze({
      module,
      bindingsURL,
      runtimeURL: new URL("../../../host/core.mjs", import.meta.url).href,
      applicationURL: new URL(
        cleanupClock
          ? "./fixtures/logical-cleanup-application.mjs" + (cleanupClock.hold ? "?hold" : "")
          : "../adapters/application.mjs",
        import.meta.url,
      ).href,
      ...(cleanupClock ? { testCleanupClock: cleanupClock.observation.buffer } : {}),
      grants: Object.freeze({ [source.selectors[0]]: true, Http, Clock: true }),
    }),
    policy: Object.freeze({
      workers,
      startupMs,
      requestMs: deadline,
      drainMs: 250,
      stopKillMs: 300,
      restartLimit: 1,
      restartWindowMs: 10000,
      retryAfter: "1",
    }),
    event(event) {
      events.push(event);
      // Matching watchers remove themselves; iterate a stable snapshot.
      for (const watch of waiting.slice())
        if (watch.predicate(event)) {
          waiting.splice(waiting.indexOf(watch), 1);
          watch.resolve(event);
        }
    },
  });
  const wait = (predicate) =>
    bounded(
      events.some(predicate)
        ? Promise.resolve(events.find(predicate))
        : new Promise((resolve) => waiting.push({ predicate, resolve })),
    );
  // Only the fixed 20-second mapper deadline is accelerated. Its cleanup
  // stays at the fixed200ms host backstop; the SDK keeps100ms.
  const fetcher = createFetchHandler(supervisor, {
    sink: { err: (text) => logs.push(text) },
    timers: { set: (fn, ms) => setTimeout(fn, ms === 20000 ? deadline : ms), clear: clearTimeout },
  });
  let listener;
  try {
    if (ready)
      await Promise.all(
        Array.from({ length: workers }, (_, slot) =>
          wait((e) => e.kind === "ready" && e.slot === slot),
        ),
      );
    listener = await nativeListener(fetcher);
    return {
      supervisor,
      fetcher,
      events,
      logs,
      wait,
      listener,
      request: (path, options = {}) =>
        fetch(listener.url + path, { signal: AbortSignal.timeout(5000), ...options }),
      async close() {
        await supervisor.close();
        await listener.close();
      },
    };
  } catch (error) {
    await supervisor.close();
    if (listener) await listener.close();
    throw error;
  }
}

// Real source/main/host/bindings compose here. All responses traverse native
// Node HTTP + Fetch mapping + the supervised worker + ordinary HD serve.
test(
  "actual HD retains state and native response acknowledgement follows held cleanup",
  { timeout: 10000 },
  async () => {
    let cleanupEntered, releaseCleanup;
    const entered = new Promise((resolve) => {
      cleanupEntered = resolve;
    });
    const gate = new Promise((resolve) => {
      releaseCleanup = resolve;
    });
    const up = await upstream(async (_req, res) => {
      cleanupEntered();
      await gate;
      res.end("cleanup acknowledged");
    });
    const f = await setup({ deadline: 3000 });
    try {
      assert.equal(await (await f.request("/count")).text(), "1");
      assert.equal(await (await f.request("/count")).text(), "2");
      let delivered = false;
      const response = f
        .request("/ack", { method: "POST", body: up.url + "/cleanup" })
        .then((value) => {
          delivered = true;
          return value;
        });
      await bounded(entered);
      await tick();
      await tick();
      assert.equal(delivered, false);
      assert.equal(
        f.listener.acknowledgements.some((e) => e.path === "/ack"),
        false,
      );
      releaseCleanup();
      assert.equal(await (await response).text(), "after cleanup");
      assert.equal(await (await f.request("/count")).text(), "4");
      f.fetcher.stop();
      await f.supervisor.stop();
      assert.equal(f.supervisor.state(), "Stopped");
    } finally {
      releaseCleanup();
      await f.close();
      await up.close();
    }
  },
);

test(
  "real false/scoped Http policy denies before effects and on redirect, preserving bytes/cookies",
  { timeout: 10000 },
  async () => {
    let effects = 0;
    const up = await upstream((req, res) => {
      effects++;
      if (req.url === "/redirect") {
        res.writeHead(302, {
          Location: "http://localhost:" + up.url.split(":").at(-1) + "/denied",
        });
        res.end();
      } else {
        res.writeHead(503, { "Set-Cookie": ["a=1", "b=2"] });
        res.end(Buffer.from([0, 255, 1]));
      }
    });
    try {
      const denied = await setup({ Http: false });
      try {
        assert.equal(
          (await denied.request("/proxy", { method: "POST", body: up.url + "/never" })).status,
          502,
        );
        assert.equal(effects, 0);
      } finally {
        await denied.close();
      }
      const scoped = await setup({ Http: ["127.0.0.1"] });
      try {
        const response = await scoped.request("/proxy", {
          method: "POST",
          body: up.url + "/binary",
        });
        assert.equal(response.status, 503);
        assert.deepEqual(response.headers.getSetCookie(), ["a=1", "b=2"]);
        assert.deepEqual([...new Uint8Array(await response.arrayBuffer())], [0, 255, 1]);
        assert.equal(
          (await scoped.request("/proxy", { method: "POST", body: up.url + "/redirect" })).status,
          502,
        );
        assert.equal(effects, 2);
      } finally {
        await scoped.close();
      }
    } finally {
      await up.close();
    }
  },
);

test(
  "actual CPU-bound HD main never opens readiness and exhausts explicit test restart bound",
  { timeout: 10000 },
  async () => {
    const f = await setup({ program: "main-cpu", ready: false, startupMs: 160 });
    try {
      assert.equal((await f.request("/not-ready")).status, 503);
      await f.wait((e) => e.kind === "restart-exhausted");
      assert.equal(f.supervisor.ready(), false);
      assert.equal(f.events.filter((e) => e.kind === "ready").length, 0);
      assert.equal(f.events.filter((e) => e.kind === "starting").length, 2);
    } finally {
      await f.close();
    }
  },
);

test(
  "actual CPU-bound HD handler is killed while sibling and replacement keep serving",
  { timeout: 10000 },
  async () => {
    const f = await setup({ workers: 2, deadline: 250 });
    try {
      const loop = f.request("/cpu");
      await f.wait((e) => e.kind === "admitted" && e.id === 1);
      assert.equal(await (await f.request("/count")).text(), "1");
      assert.equal((await loop).status, 504);
      await f.wait((e) => e.kind === "ready" && e.slot === 0 && e.generation === 2);
      assert.equal(await (await f.request("/count")).text(), "1");
      assert.equal(
        f.logs.some((text) => text.includes("http.cleanup-timeout")),
        false,
      );
    } finally {
      await f.close();
    }
  },
);

test(
  "actual CPU-bound HD cleanup after confirmed entry is isolated on native disconnect",
  { timeout: 10000 },
  async () => {
    let markEntered;
    const entered = new Promise((resolve) => {
      markEntered = resolve;
    });
    const up = await upstream((_req, res) => {
      markEntered();
      res.end("entered");
    });
    const f = await setup({ workers: 2, deadline: 3000 });
    try {
      const controller = new AbortController();
      const closing = f.request("/cleanup-cpu", {
        method: "POST",
        body: up.url + "/entered",
        signal: controller.signal,
      });
      closing.catch(() => {});
      await bounded(entered);
      controller.abort();
      assert.equal(await (await f.request("/count")).text(), "1");
      await f.wait((e) => e.kind === "ready" && e.slot === 0 && e.generation === 2);
      assert.equal(
        f.events.filter((e) => e.kind === "cancel" && e.reason === "Disconnected").length,
        1,
      );
      assert.equal(await (await f.request("/count")).text(), "1");
      assert.equal(
        f.logs.some((text) => text.includes("http.cleanup-timeout")),
        false,
      );
    } finally {
      await f.close();
      await up.close();
    }
  },
);

test(
  "native oversized body is rejected before HD dispatch and first stop drains admitted work",
  { timeout: 10000 },
  async () => {
    const f = await setup({ deadline: 3000 });
    try {
      const tooBig = await f.request("/count", {
        method: "POST",
        body: new Uint8Array(1024 * 1024 + 1),
      });
      assert.equal(tooBig.status, 413);
      assert.equal(f.events.filter((e) => e.kind === "admitted").length, 0);
      const slow = f.request("/slow");
      await f.wait((e) => e.kind === "admitted" && e.id === 1);
      f.fetcher.stop();
      assert.equal((await f.request("/closed")).status, 503);
      assert.equal((await slow).status, 200);
      await f.supervisor.stop();
      assert.equal(f.supervisor.state(), "Stopped");
      assert.equal(
        f.events.some((e) => e.kind === "cancel"),
        false,
      );
    } finally {
      await f.close();
    }
  },
);

test(
  "actual pending block_on fails explicitly without same-thread spinning or upstream effect",
  { timeout: 10000 },
  async () => {
    let effects = 0;
    const up = await upstream((_req, res) => {
      effects++;
      res.end("unexpected");
    });
    const f = await setup({ deadline: 3000 });
    try {
      const response = await f.request("/block-pending", {
        method: "POST",
        body: up.url + "/never",
      });
      assert.equal(response.status, 500);
      assert.equal(await response.text(), "");
      assert.equal(effects, 0);
      assert.equal(
        f.events.some((e) => e.kind === "terminated" && e.cause === "host-contract"),
        true,
      );
      await f.wait((e) => e.kind === "ready" && e.generation === 2);
      assert.equal(await (await f.request("/count")).text(), "1");
    } finally {
      await f.close();
      await up.close();
    }
  },
);

test(
  "actual HD panic poisons only its owning instance and never leaks into native replies",
  { timeout: 10000 },
  async () => {
    let markEntered, release;
    const entered = new Promise((resolve) => {
      markEntered = resolve;
    });
    const gate = new Promise((resolve) => {
      release = resolve;
    });
    const up = await upstream(async (_req, res) => {
      markEntered();
      await gate;
      res.end("healthy sibling");
    });
    const f = await setup({ workers: 2, deadline: 3000 });
    try {
      const sibling = f.request("/proxy", { method: "POST", body: up.url + "/held" });
      await bounded(entered);
      const failed = await f.request("/trap");
      assert.equal(failed.status, 500);
      assert.equal(await failed.text(), "");
      release();
      assert.equal(await (await sibling).text(), "healthy sibling");
      await f.wait((e) => e.kind === "ready" && e.slot === 1 && e.generation === 2);
      assert.equal(f.events.filter((e) => e.kind === "terminated").length, 1);
      assert.equal(f.events.find((e) => e.kind === "terminated").slot, 1);
      assert.equal(JSON.stringify(f.events).includes("private HD panic"), false);
      assert.equal(await (await f.request("/count")).text(), "2");
    } finally {
      release();
      await f.close();
      await up.close();
    }
  },
);

test(
  "a still-open chunked upload over one MiB receives413 before HD dispatch",
  { timeout: 10000 },
  async () => {
    const f = await setup({ deadline: 3000 });
    let controller;
    try {
      const body = new ReadableStream({
        start(value) {
          controller = value;
          value.enqueue(new Uint8Array(1024 * 1024));
          value.enqueue(new Uint8Array(1));
        },
      });
      const response = await f.request("/count", { method: "POST", body, duplex: "half" });
      assert.equal(response.status, 413);
      await response.arrayBuffer();
      assert.equal(f.events.filter((e) => e.kind === "admitted").length, 0);
    } finally {
      try {
        controller?.close();
      } catch {}
      await f.close();
    }
  },
);

test(
  "ten actual HD cleanups use a 100ms logical Clock and retain their worker",
  { timeout: 20000 },
  async (t) => {
    // Like DeadlineClock in tests/serve.hd, advance the SDK cleanup in logical
    // time. The supervisor and mapper retain their real 200ms watchdogs. A
    // loaded worker need not receive a real 100ms timer callback within 200ms
    // of a cancellation sent by another thread.
    const trials = [];
    for (let trial = 0; trial < 10; trial++) {
      let markEntered;
      const entered = new Promise((resolve) => {
        markEntered = resolve;
      });
      const up = await upstream((_req, res) => {
        markEntered();
        res.end("entered");
      });
      const observation = new Int32Array(new SharedArrayBuffer(12));
      const f = await setup({ deadline: 3000, cleanupClock: { observation } });
      try {
        const controller = new AbortController();
        const request = f.request("/cleanup-wait", {
          method: "POST",
          body: up.url + "/entered",
          signal: controller.signal,
        });
        request.catch(() => {});
        await bounded(entered);
        const start = performance.now();
        controller.abort();
        const terminal = await f.wait((e) => e.kind === "released" || e.kind === "terminated");
        const elapsedAfterDisconnectMs = performance.now() - start;
        const healthyResponse = await f.request("/count");
        const healthy = healthyResponse.status;
        const healthyCount = await healthyResponse.text();
        const backstops = f.events.filter(
          (e) => e.kind === "terminated" && e.cause === "cleanup-deadline",
        ).length;
        trials.push({
          trial,
          elapsedAfterDisconnectMs,
          terminal,
          healthy,
          healthyCount,
          backstops,
          events: f.events.slice(),
          logs: f.logs.slice(),
        });

        const detail = JSON.stringify(trials.at(-1));
        assert.deepEqual(Array.from(observation), [1, 100, 1], detail);
        assert.equal(terminal.kind, "released", detail);
        assert.equal(
          terminal.cleanupConfirmed,
          false,
          "the waiting cleanup is abandoned at its bound",
        );
        assert.equal(terminal.generation, 1);
        assert.equal(healthy, 200);
        assert.equal(healthyCount, "2", "the cancelled handler and healthy request share state");
        assert.equal(backstops, 0);
        assert.equal(
          f.logs.some((x) => x.includes("http.cleanup-timeout")),
          false,
        );
      } finally {
        await f.close();
        await up.close();
      }
    }
    assert.equal(trials.length, 10);
    t.diagnostic(
      JSON.stringify({
        platform: "node-worker",
        clock: "logical-cleanup",
        trials: trials.length,
        backstops: trials.reduce((n, value) => n + value.backstops, 0),
        acknowledgementMs: trials.map((value) => value.elapsedAfterDisconnectMs),
      }),
    );
  },
);

test("a withheld actual HD cleanup triggers at exactly 200ms", { timeout: 10000 }, async (t) => {
  let markEntered;
  const entered = new Promise((resolve) => {
    markEntered = resolve;
  });
  const up = await upstream((_req, res) => {
    markEntered();
    res.end("entered");
  });
  const observation = new Int32Array(new SharedArrayBuffer(12));
  const f = await setup({ deadline: 3000, cleanupClock: { observation, hold: true } });
  const plainRequest = (path, body = "") => ({
    method: { tag: "Post", fields: {} },
    url: "https://in.test" + path,
    query: [],
    headers: [],
    redirects: 10,
    body: new TextEncoder().encode(body),
  });
  const realSetTimeout = setTimeout,
    realClearTimeout = clearTimeout;
  let observationTimer;
  try {
    const controller = new AbortController();
    // Keep both supervisor timers in the same controlled clock. The worker
    // and this native observation watchdog keep their own real clocks.
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const request = f.supervisor.dispatch(
      plainRequest("/cleanup-wait", up.url + "/entered"),
      controller.signal,
    );
    try {
      await Promise.race([
        entered,
        new Promise((_, reject) => {
          observationTimer = realSetTimeout(
            () => reject(new Error("handler did not call upstream")),
            4000,
          );
        }),
      ]);
    } finally {
      realClearTimeout(observationTimer);
    }
    controller.abort();
    try {
      await Promise.race([
        Atomics.waitAsync(observation, 0, 0).value,
        new Promise((_, reject) => {
          observationTimer = realSetTimeout(
            () => reject(new Error("cleanup Clock was not called")),
            4000,
          );
        }),
      ]);
    } finally {
      realClearTimeout(observationTimer);
    }
    assert.deepEqual(Array.from(observation), [1, 100, 0]);
    t.mock.timers.tick(199);
    assert.equal(
      f.events.some((e) => e.kind === "released" || e.kind === "terminated"),
      false,
    );
    t.mock.timers.tick(1);
    const terminal = f.events.find((e) => e.kind === "terminated");
    assert.equal(terminal?.cause, "cleanup-deadline");
    assert.equal(terminal.generation, 1);
    const result = await request;
    assert.equal(result.cancelled, "Disconnected");
    assert.equal(result.abandoned, true);
    t.mock.timers.reset();
    await f.wait((e) => e.kind === "ready" && e.generation === 2);
    const healthy = await f.supervisor.dispatch(plainRequest("/count"));
    assert.equal(healthy.response.status, 200);
    assert.equal(new TextDecoder().decode(healthy.response.body), "1");
    t.diagnostic(
      JSON.stringify({
        terminal,
        observations: Array.from(observation),
        events: f.events,
        logs: f.logs,
      }),
    );
  } finally {
    realClearTimeout(observationTimer);
    t.mock.timers.reset();
    await f.close();
    await up.close();
  }
});
