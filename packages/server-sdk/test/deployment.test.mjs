import assert from "node:assert/strict";
import { test, after } from "node:test";
import { execFileSync } from "node:child_process";
import {
  readFileSync,
  writeFileSync,
  existsSync,
  readdirSync,
  renameSync,
  mkdirSync,
} from "node:fs";
import { rm } from "node:fs/promises";
import { join, dirname } from "node:path";
import { pathToFileURL } from "node:url";
import { readPackageContract } from "../../../host/package-providers.mjs";
import { fixture, environment, repository } from "./helpers/deployment-fixture.mjs";
import {
  encode,
  contractSection,
  withContract,
  withImportModule,
  companionHeader,
} from "./helpers/deployment-contract.mjs";

const compiled = fixture();
after(() => rm(compiled.directory, { recursive: true, force: true }));
const { app, wasmPath, bindingsPath, wasm, companion, manifest, run } = compiled;
const originalManifest = readFileSync(manifest, "utf8").replaceAll("\r\n", "\n"),
  contract = contractSection(wasm);
const interfaces = readPackageContract(contract),
  sdkIdentity = interfaces.find((i) => !i.handle).identity;
const output = join(app, "cloudflare"),
  selector = join(output, "wrangler.json");
const marker = join(compiled.directory, "UNTRUSTED-JAVASCRIPT-EXECUTED");
const malicious = (bytes) =>
  companionHeader(bytes) +
  `import { writeFileSync } from 'node:fs'; writeFileSync(${JSON.stringify(marker)},'untrusted companion body'); throw Error('must not execute');\n`;
const selected = () => JSON.parse(readFileSync(selector, "utf8"));
const base = () => dirname(join(output, selected().main));
function ordinaryTask(ok = true, cwd = app) {
  if (ok) run(["run", "deploy-cloudflare", "--engine", "node"], cwd);
  else
    assert.throws(
      () => run(["run", "deploy-cloudflare", "--engine", "node"], cwd),
      (error) => {
        assert.match(error.stderr.toString(), /deployment|policy|schema|bindings|companion/i);
        return true;
      },
    );
  assert.equal(existsSync(marker), false, "artifact JavaScript must never execute");
}
async function changed(operation, unchanged = true) {
  const before = existsSync(selector) ? readFileSync(selector) : null;
  try {
    await operation();
  } finally {
    writeFileSync(wasmPath, wasm);
    writeFileSync(bindingsPath, companion);
    writeFileSync(manifest, originalManifest);
  }
  if (before && unchanged)
    assert.deepEqual(readFileSync(selector), before, "refusal must preserve the active deployment");
}

test("ordinary build and deploy task publishes complete output from a nested package directory", () => {
  writeFileSync(bindingsPath, malicious(contract));
  try {
    ordinaryTask(true, join(app, "src"));
  } finally {
    writeFileSync(bindingsPath, companion);
  }
  const config = selected();
  assert.equal(config.name, "server-api");
  assert.equal(config.compatibility_date, "2026-10-23");
  assert.match(config.main, /^generations\/[a-f0-9]{64}\/entry\.mjs$/);
  const directory = base(),
    receipt = JSON.parse(readFileSync(join(directory, "deployment.json"), "utf8"));
  for (const name of [
    "entry.mjs",
    "app.wasm",
    "bindings.mjs",
    "grants.json",
    "sdk/application.mjs",
    "sdk/contract-bindings.mjs",
    "runtime/core.mjs",
    "runtime/package-providers.mjs",
  ])
    assert.ok(existsSync(join(directory, name)), name);
  for (const name of [
    "runtimeFingerprint",
    "wasmSha256",
    "bindingsSha256",
    "sourceBindingsSha256",
    "schemaFingerprint",
  ])
    assert.match(receipt[name], /^[0-9a-f]{64}$/);
  assert.ok(
    !readFileSync(join(directory, "bindings.mjs"), "utf8").includes("untrusted companion body"),
  );
  assert.ok(!existsSync(join(app, "build/debug/server_api.host.json")));
  assert.ok(!readdirSync(directory).some((name) => name.endsWith(".hd")));
});

test(
  "emitted data-only bindings and public core run the retained application through stop",
  { timeout: 10000 },
  async () => {
    const directory = base(),
      bindings = await import(pathToFileURL(join(directory, "bindings.mjs")).href);
    const { createCoreHost } = await import(
      pathToFileURL(join(directory, "runtime/core.mjs")).href
    );
    const { createApplication } = await import(
      pathToFileURL(join(directory, "sdk/application.mjs")).href
    );
    const out = [],
      err = [],
      counts = [];
    let stopped;
    const completion = new Promise((resolve) => {
      stopped = resolve;
    });
    const application = createApplication({
      module: new WebAssembly.Module(readFileSync(join(directory, "app.wasm"))),
      bindings,
      createCoreHost,
      grants: JSON.parse(readFileSync(join(directory, "grants.json"), "utf8")),
      sink: { out: (text) => out.push(text), err: (text) => err.push(text) },
      onStopped: stopped,
    });
    try {
      for (const message of ["one", "two", "three"]) {
        const result = await application.dispatch({
          method: { tag: "Post", fields: {} },
          url: "https://installed.test/slow",
          headers: [],
          redirects: 10,
          body: new TextEncoder().encode(JSON.stringify({ message })),
        });
        assert.equal(result.response.status, 200);
        const reply = JSON.parse(new TextDecoder().decode(result.response.body));
        assert.equal(reply.message, message);
        counts.push(reply.count);
      }
      application.stop();
      await completion;
      assert.deepEqual(counts, [1, 2, 3]);
      assert.deepEqual(err, []);
      assert.equal(out.join(""), "total 3\n");
      await assert.rejects(application.dispatch({}), /admission is closed/);
    } finally {
      application.fail(new Error("test ended"));
    }
  },
);

test("production denial wins over development allowance at deploy time", () =>
  changed(() => {
    writeFileSync(
      manifest,
      originalManifest
        .replace(
          '"dep.server_sdk.server.HttpServer" = true',
          '"dep.server_sdk.server.HttpServer" = false',
        )
        .replace(
          "[dev-capabilities]\n",
          '[dev-capabilities]\n"dep.server_sdk.server.HttpServer" = true\n',
        ),
    );
    ordinaryTask(false);
  }));

test("development denial does not replace explicit production allowance", () =>
  changed(() => {
    writeFileSync(
      manifest,
      originalManifest.replace(
        "[dev-capabilities]\n",
        '[dev-capabilities]\n"dep.server_sdk.server.HttpServer" = false\n',
      ),
    );
    // Invoke the ordinary task: its deployment capability is still development consent.
    ordinaryTask();
    const grants = JSON.parse(readFileSync(join(base(), "grants.json"), "utf8"));
    assert.equal(grants["dep.server_sdk.server.HttpServer"], true);
    assert.equal(Object.hasOwn(grants, "dep.server_sdk.deployment.Deployment"), false);
  }, false));

for (const kind of [
  "foreign SDK",
  "false alias",
  "source method shape",
  "returned-handle method shape",
])
  test(`paired artifact metadata cannot bypass ${kind}`, () =>
    changed(() => {
      const declarations = structuredClone(interfaces),
        source = declarations.find((i) => !i.handle);
      if (kind === "foreign SDK") source.identity.package = "foreign:" + source.identity.package;
      if (kind === "false alias") {
        source.selectors.push("dep.sdk_alias.server.HttpServer");
        writeFileSync(
          manifest,
          originalManifest.replace(
            "[capabilities]\n",
            '[capabilities]\n"dep.sdk_alias.server.HttpServer" = false\n',
          ),
        );
      }
      if (kind === "source method shape") source.methods[0].member = "forged";
      if (kind === "returned-handle method shape") {
        const method = declarations.find((i) => i.handle && i.methods.length).methods[0];
        method.wait = method.wait === "may" ? "never" : "may";
      }
      const bytes = encode(declarations);
      writeFileSync(wasmPath, withContract(wasm, bytes));
      writeFileSync(bindingsPath, malicious(bytes));
      ordinaryTask(false);
    }));

test("stale companion metadata refuses deployment without evaluating its body", () =>
  changed(() => {
    const bytes = contract.slice();
    bytes[bytes.length - 1] ^= 1;
    writeFileSync(bindingsPath, malicious(bytes));
    ordinaryTask(false);
  }));

test("unknown physical package import is rejected despite valid paired contract data", () =>
  changed(() => {
    const bytes = Buffer.from(wasm),
      at = bytes.indexOf(Buffer.from("accept.finish"));
    assert.ok(at >= 0);
    Buffer.from("xccept.finish").copy(bytes, at);
    new WebAssembly.Module(bytes);
    writeFileSync(wasmPath, bytes);
    ordinaryTask(false);
  }));

for (const kind of ["unsupported builtin", "denied Clock", "denied Random"])
  test(`deployment preflight refuses ${kind} before publishing`, () =>
    changed(() => {
      const target =
        kind === "unsupported builtin"
          ? "hd:Store"
          : kind === "denied Random"
            ? "hd:Random"
            : "hd:Clock";
      writeFileSync(wasmPath, withImportModule(wasm, "hd:Clock", target));
      if (kind === "denied Clock")
        writeFileSync(manifest, originalManifest.replace("Clock = true", "Clock = false"));
      if (kind === "denied Random")
        writeFileSync(
          manifest,
          originalManifest.replace("Clock = true", "Clock = true\nRandom = false"),
        );
      ordinaryTask(false);
    }));

test(
  "installed packaging survives relocation with no source, compiler cache or checkout graph",
  { timeout: 10000 },
  async () => {
    const moved = join(compiled.directory, "relocated");
    renameSync(compiled.installation, moved);
    // Source trees are kept outside the installed distribution used by this child.
    for (const [path, name] of [
      ["app/src", "app-source"],
      ["app/tasks", "app-tasks"],
      ["sdk/src", "sdk-source"],
      ["sdk/providers", "sdk-providers"],
    ])
      renameSync(join(moved, path), join(compiled.directory, name));
    renameSync(compiled.cache, join(compiled.directory, "old-compiler-cache"));
    const cwd = join(compiled.directory, "unrelated/nested");
    mkdirSync(cwd, { recursive: true });
    writeFileSync(join(compiled.directory, "unrelated/hd.toml"), "[capabilities]\nClock=false\n");
    const script = join(compiled.directory, "isolated-package.mjs");
    const url = (path) => JSON.stringify(pathToFileURL(join(moved, path)).href);
    writeFileSync(
      script,
      `
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import fs from "node:fs";
import promises from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
const forbidden = ${JSON.stringify([join(repository, "host"), join(repository, "lib"), join(repository, "crates"), compiled.cache, join(compiled.directory, "old-compiler-cache")])};
const lookups = [];
function guard(path) {
  const text = resolve(path instanceof URL ? fileURLToPath(path) : String(path));
  if (text.endsWith(".hd") || forbidden.some((base) => text === base || text.startsWith(base + sep))) {
    lookups.push(text);
    throw Error("deployment attempted a compiler/source lookup");
  }
}
for (const [api, names] of [[fs, ["readFileSync", "statSync", "lstatSync", "accessSync", "existsSync", "readdirSync", "openSync"]], [promises, ["readFile", "stat", "lstat", "access", "readdir", "open"]]])
  for (const name of names) {
    const original = api[name];
    api[name] = function(path, ...args) { guard(path); return original.call(api, path, ...args); };
  }
syncBuiltinESMExports();
const { packageCloudflare } = await import(${url("sdk/packaging/cloudflare.mjs")});
const options = ${JSON.stringify({ rootDirectory: join(moved, "app"), wasmPath: join(moved, "app/build/debug/server_api.wasm"), bindingsPath: join(moved, "app/build/debug/server_api.host.mjs"), outputDirectory: join(moved, "app/relocated-cloudflare"), name: "relocated", compatibilityDate: "2026-10-23", expectedSDKIdentity: sdkIdentity })};
await assert.rejects(packageCloudflare({...options,expectedSDKIdentity:null}), /trusted installed SDK identity/);
for (const patch of [{name:"Uppercase"},{name:""},{name:"x".repeat(64)},{compatibilityDate:"2026-02-30"},{outputDirectory:options.rootDirectory}])
  await assert.rejects(packageCloudflare({...options,...patch}));
const receipt = await packageCloudflare(options);
const config = JSON.parse(await readFile(options.outputDirectory + "/wrangler.json", "utf8"));
assert.equal(config.name,"relocated");
assert.ok((await readFile(options.outputDirectory + "/" + config.main,"utf8")).includes("createApplication"));
assert.equal((await packageCloudflare(options)).generation,receipt.generation);
assert.deepEqual(lookups,[]);
console.log("relocated installed SDK passed");
`,
    );
    assert.equal(
      execFileSync(process.execPath, [script], {
        cwd,
        env: environment(join(compiled.directory, "empty-cache")),
        encoding: "utf8",
        timeout: 10000,
      }).trim(),
      "relocated installed SDK passed",
    );
  },
);
