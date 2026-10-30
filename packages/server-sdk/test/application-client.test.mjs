import assert from "node:assert/strict";
import http from "node:http";
import { once } from "node:events";
import { cpSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { after, before, test } from "node:test";
import { createCoreHost } from "../../../host/core.mjs";
import { createPackageRuntime } from "../../../host/package-providers.mjs";
import { createApplication } from "../adapters/application.mjs";
import { readProductionGrants } from "../packaging/grants.mjs";

// This module and its binding companion come from the real HD build. The
// handler row is Http; main requires no ambient Clock for SDK deadlines.
const root = fileURLToPath(new URL("../../../", import.meta.url));
const hd = process.env.HD_SDK_TEST_HD ?? resolve(root, "target/debug/hd");
let directory, module, bindings, source;
before(async () => {
  directory = mkdtempSync(join(root, ".cache-muse-sdk-client-"));
  const fixture = fileURLToPath(new URL("./fixtures/application-client/", import.meta.url));
  const app = join(directory, "app");
  cpSync(fixture, app, { recursive: true });
  const manifest = join(app, "hd.toml");
  writeFileSync(
    manifest,
    readFileSync(manifest, "utf8").replace(
      'path="../../.."',
      `path=${JSON.stringify(join(root, "packages/server-sdk"))}`,
    ),
  );
  execFileSync(hd, ["build"], {
    cwd: app,
    env: { ...process.env, HD_CACHE: join(directory, "cache") },
    timeout: 30000,
    stdio: "pipe",
  });
  const build = join(app, "build/debug");
  module = new WebAssembly.Module(readFileSync(join(build, "application_client_proposal.wasm")));
  const binding = readdirSync(build).find((file) => file.endsWith(".host.mjs"));
  assert.ok(binding, "the SDK emits public signature-generated bindings");
  bindings = await import(pathToFileURL(join(build, binding)).href);
  source = bindings.interfaces.find((item) => !item.handle);
});
after(() => {
  if (directory) {
    rmSync(directory, { recursive: true, force: true });
  }
});
const tick = () => new Promise((resolve) => setImmediate(resolve));
const incoming = (url) => ({
  method: { tag: "Post", fields: {} },
  url: "https://application.test/proxy",
  headers: [],
  body: new TextEncoder().encode(url),
  redirects: 10,
});

async function setup(Http, handler, grants) {
  const upstream = http.createServer(handler);
  upstream.listen(0, "127.0.0.1");
  await once(upstream, "listening");
  const url = `http://127.0.0.1:${upstream.address().port}`;
  const clients = [],
    errors = [];
  let ambientHttp = 0,
    ambientClock = 0;
  const app = createApplication({
    module,
    createPackageRuntime,
    bindings: {
      ...bindings,
      bind(runtime, providers) {
        const server = providers.get(source.module);
        const client = server.client;
        // Retain the returned native object only to test its public expiry
        // behavior after the actual compiled guest releases the lease.
        server.client = (...args) => {
          const value = client(...args);
          clients.push(value);
          return value;
        };
        return bindings.bind(runtime, providers);
      },
    },
    createCoreHost(sink, env, layer) {
      const providers = layer.providers;
      const host = createCoreHost(sink, env, {
        ...layer,
        providers(...args) {
          const result = providers(...args);
          assert.throws(
            () => result.traits["hd:Http"].send(),
            /server HTTP sends require a request lease/,
          );
          assert.throws(
            () => result.traits["hd:Clock"].sleep(),
            /server sleeps require a request lease/,
          );
          for (const [key, method, count] of [
            ["hd:Http", "send", () => ambientHttp++],
            ["hd:Clock", "sleep", () => ambientClock++],
          ]) {
            const guarded = result.traits[key][method];
            result.traits[key][method] = (...values) => {
              count();
              return guarded(...values);
            };
          }
          return result;
        },
      });
      return new Proxy(host, {
        get(target, key) {
          assert.notEqual(key, "grant", "clients consume only the public context policy");
          return Reflect.get(target, key);
        },
      });
    },
    grants: grants ?? { [source.selectors[0]]: true, Http },
    sink: { out() {}, err: (text) => errors.push(text) },
  });
  return {
    app,
    url,
    clients,
    errors,
    check() {
      assert.equal(ambientHttp, 0, "outgoing requests use the lease client");
      assert.equal(ambientClock, 0, "SDK timeout and cleanup use the source clock");
      assert.deepEqual(errors, []);
    },
    async close() {
      app.fail(new Error("test finished"));
      upstream.closeAllConnections();
      await new Promise((resolve) => upstream.close(resolve));
    },
  };
}

test(
  "Application sends real outgoing binary HTTP and preserves independent cookie fields",
  { timeout: 10000 },
  async () => {
    const bytes = new Uint8Array([0, 255, 128, 65]);
    const state = await setup(true, (_request, response) => {
      response.writeHead(503, { "set-cookie": ["one=1; Path=/", "two=2; Path=/"] });
      response.end(bytes);
    });
    try {
      const result = await state.app.dispatch(incoming(state.url + "/binary"));
      assert.equal(result.response.status, 503);
      assert.deepEqual(result.response.body, bytes);
      assert.deepEqual(
        result.response.headers.filter(([key]) => key === "set-cookie").map(([, value]) => value),
        ["one=1; Path=/", "two=2; Path=/"],
      );
      assert.equal(result.response.url, state.url + "/binary");
      assert.equal(result.cancelled, null);
      assert.equal(result.abandoned, false);
      state.check();
      state.app.stop();
    } finally {
      await state.close();
    }
  },
);

test(
  "Application consults its resolved Http grant before native fetch effects",
  { timeout: 10000 },
  async () => {
    for (const grant of [false, ["allowed.test:443"]]) {
      let hits = 0,
        fetches = 0;
      const fetch = globalThis.fetch;
      globalThis.fetch = (...args) => {
        fetches++;
        return fetch(...args);
      };
      const state = await setup(grant, (_request, response) => {
        hits++;
        response.end("unexpected");
      });
      try {
        const result = await state.app.dispatch(incoming(state.url + "/denied"));
        assert.equal(result.response.status, 502);
        assert.match(
          new TextDecoder().decode(result.response.body),
          /http access to 127\.0\.0\.1 is not granted/,
        );
        assert.equal(fetches, 0);
        assert.equal(hits, 0);
        state.check();
        state.app.stop();
      } finally {
        globalThis.fetch = fetch;
        await state.close();
      }
    }
  },
);

test(
  "parallel Application leases use distinct clients and every retained client expires",
  { timeout: 10000 },
  async () => {
    let hits = 0;
    const state = await setup(true, (request, response) => {
      hits++;
      setTimeout(() => response.end(request.url), 25);
    });
    try {
      const jobs = [0, 1, 2, 3].map((n) => state.app.dispatch(incoming(state.url + "/" + n)));
      assert.equal(state.app.active(), 4);
      const results = await Promise.all(jobs);
      assert.deepEqual(
        results.map((value) => new TextDecoder().decode(value.response.body)),
        ["/0", "/1", "/2", "/3"],
      );
      assert.equal(new Set(state.clients).size, 4);
      assert.equal(state.app.active(), 0);
      for (const client of state.clients) {
        assert.throws(
          () => client.send(incoming(state.url + "/late"), {}),
          /expired request lease/,
        );
      }
      await tick();
      assert.equal(hits, 4, "expired handles start no additional native request");
      state.check();
      state.app.stop();
    } finally {
      await state.close();
    }
  },
);

test(
  "Application disconnect aborts a live upstream body while sibling leases survive",
  { timeout: 10000 },
  async () => {
    let started, closed;
    const upstreamStarted = new Promise((resolve) => {
      started = resolve;
    });
    const upstreamClosed = new Promise((resolve) => {
      closed = resolve;
    });
    const state = await setup(true, (request, response) => {
      if (request.url === "/wait") {
        response.writeHead(200);
        response.write("partial");
        started();
        response.once("close", closed);
      } else {
        response.end("sibling");
      }
    });
    try {
      const controller = new AbortController();
      const cancelled = state.app.dispatch(incoming(state.url + "/wait"), controller.signal);
      await upstreamStarted;
      const sibling = state.app.dispatch(incoming(state.url + "/fast"));
      controller.abort();
      const result = await cancelled;
      assert.equal(result.cancelled, "Disconnected");
      assert.equal(result.abandoned, false);
      assert.equal(result.response, null);
      await upstreamClosed;
      assert.equal(new TextDecoder().decode((await sibling).response.body), "sibling");
      assert.equal(
        new TextDecoder().decode(
          (await state.app.dispatch(incoming(state.url + "/later"))).response.body,
        ),
        "sibling",
      );
      for (const client of state.clients) {
        assert.throws(
          () => client.send(incoming(state.url + "/late"), {}),
          /expired request lease/,
        );
      }
      assert.equal(state.app.active(), 0);
      state.check();
      state.app.stop();
    } finally {
      await state.close();
    }
  },
);

test(
  "Application preserves owned cancellation reasons while cancelling native I/O",
  { timeout: 10000 },
  async () => {
    for (const [reason, expected] of [
      ["Deadline", "Deadline"],
      ["Stopping", "Stopping"],
      ["private platform reason", "Disconnected"],
    ]) {
      let started;
      const active = new Promise((resolve) => {
        started = resolve;
      });
      const state = await setup(true, (_request, response) => {
        response.writeHead(200);
        response.write("partial");
        started();
      });
      try {
        const controller = new AbortController();
        const job = state.app.dispatch(incoming(state.url + "/wait"), controller.signal);
        await active;
        controller.abort(reason);
        const result = await job;
        assert.equal(result.cancelled, expected);
        assert.equal(result.response, null);
        assert.equal(result.abandoned, false);
        assert.equal(state.app.active(), 0);
        state.check();
        state.app.stop();
      } finally {
        await state.close();
      }
    }
  },
);

test(
  "deployed applications use current root grants instead of development permissions",
  { timeout: 10000 },
  async () => {
    const rootDirectory = join(directory, "app");
    const manifest = join(rootDirectory, "hd.toml");
    const original = readFileSync(manifest, "utf8");
    const productionManifest = original.replaceAll("\r\n", "\n");
    const initialGrants = await readProductionGrants(rootDirectory);
    const selector = source.selectors.find((path) => initialGrants[path] === true);
    assert.ok(selector, "the root explicitly grants one declared source path");
    try {
      for (const [production, development, status] of [
        [false, true, 502],
        [true, false, 200],
        [["127.0.0.1"], false, 200],
        [["allowed.test"], true, 502],
      ]) {
        writeFileSync(
          manifest,
          productionManifest.replace("\nHttp=true\n", `\nHttp=${JSON.stringify(production)}\n`) +
            `\n[dev-capabilities]\nHttp=${development}\n`,
        );
        const grants = await readProductionGrants(rootDirectory);
        assert.equal(grants[selector], true);
        let hits = 0;
        const state = await setup(
          undefined,
          (_request, response) => {
            hits++;
            response.end("production allowed");
          },
          grants,
        );
        try {
          const result = await state.app.dispatch(incoming(state.url));
          assert.equal(result.response.status, status);
          assert.equal(hits, status === 200 ? 1 : 0);
          state.check();
          state.app.stop();
        } finally {
          await state.close();
        }
      }
      writeFileSync(
        manifest,
        productionManifest.replace(
          `${JSON.stringify(selector)}=true`,
          `${JSON.stringify(selector)}=false`,
        ) + `\n[dev-capabilities]\n${JSON.stringify(selector)}=true\n`,
      );
      const grants = await readProductionGrants(rootDirectory);
      assert.equal(grants[selector], false);
      assert.throws(
        () => createApplication({ module, bindings, createCoreHost, createPackageRuntime, grants }),
        /grant/,
      );
    } finally {
      writeFileSync(manifest, original);
    }
  },
);
