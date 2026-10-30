import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, cpSync, rmSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createCoreHost } from "../../../host/core.mjs";
import { createPackageRuntime } from "../../../host/package-providers.mjs";
import { createApplication } from "../adapters/application.mjs";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const hd = process.env.HD_SDK_TEST_HD ?? resolve(root, "target/debug/hd");
let directory, module, bindings;

before(() => {
  directory = mkdtempSync(join(root, ".cache-muse-sdk-dispatch-"));
  cpSync(join(root, "examples/server-api"), join(directory, "app"), { recursive: true });
  const main = join(directory, "app/src/main.hd");
  writeFileSync(
    main,
    readFileSync(main, "utf8").replace(
      "    input := match request.json::[Input]():",
      '    if request.url.path.ends_with("/trap"): panic("private handler failure")\n    input := match request.json::[Input]():',
    ),
  );
  assert.ok(
    readFileSync(main, "utf8").includes('panic("private handler failure")'),
    "the fixture really injects a guest panic",
  );
  const manifest = join(directory, "app/hd.toml");
  writeFileSync(
    manifest,
    readFileSync(manifest, "utf8").replace(
      "../../packages/server-sdk",
      join(root, "packages/server-sdk"),
    ),
  );
  execFileSync(hd, ["build"], {
    cwd: join(directory, "app"),
    env: { ...process.env, HD_CACHE: join(directory, "cache") },
    timeout: 30000,
    stdio: "pipe",
  });
  const build = join(directory, "app/build/debug");
  module = new WebAssembly.Module(readFileSync(join(build, "server_api.wasm")));
  const binding = readdirSync(build).find((file) => file.endsWith(".host.mjs"));
  assert.ok(binding, "the SDK package capability emits signature-generated bindings");
  return import(pathToFileURL(join(build, binding)).href).then((value) => {
    bindings = value;
  });
});
after(() => {
  if (directory) {
    rmSync(directory, { recursive: true, force: true });
  }
});

function application() {
  const errors = [],
    output = [];
  const source = bindings.interfaces.find((item) => !item.handle);
  const app = createApplication({
    module,
    bindings,
    createCoreHost,
    createPackageRuntime,
    grants: { [source.selectors[0]]: true, Console: true, Clock: true },
    sink: {
      out(text) {
        output.push(text);
      },
      err(text) {
        errors.push(text);
      },
    },
  });
  return { app, errors, output };
}
const request = (path, message) => ({
  method: { tag: "Post", fields: {} },
  url: "https://server.test" + path,
  headers: [],
  body: new TextEncoder().encode(JSON.stringify({ message })),
  redirects: 10,
});
const body = (result) => JSON.parse(new TextDecoder().decode(result.response.body));

test(
  "the public driver serves concurrent jobs and publishes after guest cleanup",
  { timeout: 10000 },
  async () => {
    const { app, errors, output } = application();
    try {
      for (let n = 1; n <= 3; n++) {
        assert.equal(body(await app.dispatch(request("/", "hello"))).count, n);
      }
      const results = await Promise.all(
        ["a", "b", "c"].map((text) => app.dispatch(request("/slow", text))),
      );
      assert.deepEqual(
        results
          .map(body)
          .map((value) => value.count)
          .sort(),
        [4, 5, 6],
      );
      assert.ok(results.every((value) => !value.abandoned && !value.cancelled));
      assert.equal(app.active(), 0);
      assert.deepEqual(errors, []);
      app.stop();
      assert.equal(output.join(""), "total 6\n");
      await assert.rejects(app.dispatch(request("/", "late")), /admission is closed/);
    } finally {
      app.fail(new Error("test ended"));
    }
  },
);

test(
  "disconnect cancels one handler while its retained instance keeps serving",
  { timeout: 10000 },
  async () => {
    const { app, errors } = application();
    try {
      const controller = new AbortController();
      const slow = app.dispatch(request("/slow", "cancel"), controller.signal);
      controller.abort();
      const other = app.dispatch(request("/", "other"));
      const cancelled = await slow;
      assert.equal(cancelled.cancelled, "Disconnected");
      assert.equal(cancelled.abandoned, false);
      assert.equal(cancelled.response, null);
      assert.equal(body(await other).message, "other");
      assert.equal(body(await app.dispatch(request("/", "still alive"))).message, "still alive");
      assert.equal(app.active(), 0);
      assert.deepEqual(errors, []);
      app.stop();
    } finally {
      app.fail(new Error("test ended"));
    }
  },
);

test(
  "poisoning an instance aborts every active lease and forbids reuse",
  { timeout: 10000 },
  async () => {
    const { app } = application();
    const a = app.dispatch(request("/slow", "a"));
    const b = app.dispatch(request("/slow", "b"));
    const failure = new Error("private trap");
    app.fail(failure);
    const results = await Promise.allSettled([a, b]);
    assert.ok(results.every((value) => value.status === "rejected" && value.reason === failure));
    assert.equal(app.active(), 0);
    await assert.rejects(app.dispatch(request("/", "late")), /instance failed/);
  },
);

test(
  "a real guest panic poisons other active requests before reuse",
  { timeout: 10000 },
  async () => {
    const { app, errors } = application();
    const a = app.dispatch(request("/slow", "a"));
    assert.equal(app.active(), 1, "the other request is actually admitted before the trap");
    const b = app.dispatch(request("/trap", "b"));
    const results = await Promise.allSettled([a, b]);
    assert.ok(results.every((value) => value.status === "rejected"));
    assert.match(errors.join(""), /private handler failure/);
    assert.equal(app.active(), 0);
    await assert.rejects(app.dispatch(request("/", "late")), /instance failed/);
  },
);
