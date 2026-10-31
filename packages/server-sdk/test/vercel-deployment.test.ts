import assert from "node:assert/strict";
import { test, after } from "node:test";
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync, renameSync, mkdirSync } from "node:fs";
import { rm } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { packageVercelNode, validatePolicy } from "../packaging/vercel.ts";
import { readPackageContract } from "../../../host/package-providers.ts";
import { fixture, environment } from "./helpers/deployment-fixture.ts";
import {
  encode,
  contractSection,
  companionHeader,
  withContract,
  withImportModule,
} from "./helpers/deployment-contract.ts";

const compiled = fixture();
after(() => rm(compiled.directory, { recursive: true, force: true }));
const { app, sdk, run, wasm, wasmPath, bindingsPath, manifest, companion } = compiled;
const originalManifest = readFileSync(manifest, "utf8").replaceAll("\r\n", "\n");
const task = join(app, "tasks/deploy-vercel.hd");
const originalTask = readFileSync(task, "utf8").replaceAll("\r\n", "\n");
// Exercise the same public facade as SDK library consumers, with one retained worker.
const oneWorkerTask = originalTask
  .replace("use dep.server_sdk.deployment.", "use dep.server_sdk.")
  .replace("workers: 8", "workers: 1");
assert.notEqual(oneWorkerTask, originalTask);
writeFileSync(task, oneWorkerTask);
const contract = contractSection(wasm),
  interfaces = readPackageContract(contract);
const expectedSDKIdentity = interfaces.find((i) => !i.handle).identity;
const output = join(app, ".vercel/output"),
  selector = join(output, "config.json");
const selected = () => JSON.parse(readFileSync(selector, "utf8"));
const functionDirectory = () =>
  join(output, "functions", selected().routes[0].dest.slice(1) + ".func");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const marker = join(compiled.directory, "UNTRUSTED-VERCEL-COMPANION");
const malicious = (bytes) =>
  companionHeader(bytes) +
  `import {writeFileSync} from 'node:fs';writeFileSync(${JSON.stringify(marker)},'unsafe source JavaScript');\nthrow Error('do not execute');\n`;
const policy = () => ({
  workers: 1,
  startupMs: 3000,
  requestMs: 20000,
  drainMs: 1000,
  stopKillMs: 300,
  restartLimit: 2,
  restartWindowMs: 60000,
  retryAfter: "1",
});
const options = (name) => ({
  rootDirectory: app,
  wasmPath,
  bindingsPath,
  outputDirectory: join(compiled.directory, name),
  name,
  expectedSDKIdentity,
  policy: policy(),
});
function ordinaryTask(ok = true, cwd = app) {
  if (ok) {
    run(["run", "deploy-vercel", "--engine", "node"], cwd);
  } else {
    assert.throws(
      () => run(["run", "deploy-vercel", "--engine", "node"], cwd),
      (error) => {
        assert.match(error.stderr.toString(), /deployment|policy|schema|bindings|companion/i);
        return true;
      },
    );
  }
  assert.equal(existsSync(marker), false);
}
async function changed(operation, unchanged = true) {
  const before = existsSync(selector) ? readFileSync(selector) : null;
  try {
    await operation();
  } finally {
    writeFileSync(wasmPath, wasm);
    writeFileSync(bindingsPath, companion);
    writeFileSync(manifest, originalManifest);
    writeFileSync(task, oneWorkerTask);
  }
  if (before && unchanged) {
    assert.deepEqual(readFileSync(selector), before);
  }
}

test("ordinary Vercel facade task prepares complete Build Output v3 from nested CWD", () => {
  writeFileSync(bindingsPath, malicious(contract));
  try {
    ordinaryTask(true, join(app, "src"));
  } finally {
    writeFileSync(bindingsPath, companion);
  }
  const config = selected();
  assert.equal(config.version, 3);
  assert.deepEqual(
    config.routes.map((r) => r.src),
    ["/(.*)"],
  );
  assert.match(config.routes[0].dest, /^\/server-api-[a-f0-9]{48}$/);
  const directory = functionDirectory();
  const metadata = JSON.parse(readFileSync(join(directory, ".vc-config.json"), "utf8"));
  assert.deepEqual(metadata, {
    runtime: "nodejs24.x",
    handler: "index.js",
    launcherType: "Nodejs",
    shouldAddHelpers: false,
  });
  for (const file of [
    "index.js",
    "app.wasm",
    "bindings.js",
    "grants.json",
    "deployment.json",
    "runtime/core.js",
    "runtime/deployed-core.js",
    "sdk/worker.js",
    "sdk/vercel-node.js",
  ]) {
    assert.ok(existsSync(join(directory, file)), file);
  }
  const receipt = JSON.parse(readFileSync(join(directory, "deployment.json"), "utf8"));
  assert.equal(receipt.wasmSha256, hash(wasm));
  assert.equal(receipt.sourceBindingsSha256, hash(Buffer.from(malicious(contract))));
  assert.equal(receipt.bindingsSha256, hash(readFileSync(join(directory, "bindings.js"))));
  assert.ok(
    !readFileSync(join(directory, "bindings.js"), "utf8").includes("unsafe source JavaScript"),
  );
});

test("SDK library tests reach every declared host through the public facade", () => {
  assert.match(run(["test", "--engine", "node"], sdk).toString(), /test result: ok\./);
});

test("current production false rejects Vercel preparation despite development true", () =>
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

test("production true remains effective despite development false", () =>
  changed(() => {
    writeFileSync(
      manifest,
      originalManifest.replace(
        "[dev-capabilities]\n",
        '[dev-capabilities]\n"dep.server_sdk.server.HttpServer" = false\n',
      ),
    );
    ordinaryTask();
    const grants = JSON.parse(readFileSync(join(functionDirectory(), "grants.json"), "utf8"));
    assert.equal(grants["dep.server_sdk.server.HttpServer"], true);
    assert.equal(Object.hasOwn(grants, "dep.server_sdk.deployment.Deployment"), false);
  }, false));

for (const kind of ["false alias", "foreign nominal SDK", "stale companion"]) {
  test(`Vercel task rejects ${kind} without changing the selected function`, () =>
    changed(() => {
      const declarations = structuredClone(interfaces),
        source = declarations.find((i) => !i.handle);
      if (kind === "false alias") {
        source.selectors.push("dep.alias.server.HttpServer");
        writeFileSync(
          manifest,
          originalManifest.replace(
            "[capabilities]\n",
            '[capabilities]\n"dep.alias.server.HttpServer" = false\n',
          ),
        );
      }
      if (kind === "foreign nominal SDK") {
        source.identity.package = "foreign:" + source.identity.package;
      }
      const bytes = encode(declarations);
      if (kind !== "stale companion") {
        writeFileSync(wasmPath, withContract(wasm, bytes));
      } else {
        bytes[bytes.length - 1] ^= 1;
      }
      writeFileSync(bindingsPath, malicious(bytes));
      ordinaryTask(false);
    }));
}

test("physical import mismatches and primitive denials reject before output", () =>
  changed(() => {
    for (const kind of ["physical", "unsupported", "Clock", "Random"]) {
      let bytes = Buffer.from(wasm);
      if (kind === "physical") {
        const at = bytes.indexOf(Buffer.from("accept.finish"));
        assert.ok(at >= 0);
        Buffer.from("xccept.finish").copy(bytes, at);
      } else {
        bytes = withImportModule(
          wasm,
          "hd:Clock",
          kind === "unsupported" ? "hd:Store" : "hd:" + kind,
        );
      }
      writeFileSync(wasmPath, bytes);
      let changedManifest = originalManifest;
      if (kind === "Clock") {
        changedManifest = originalManifest.replace("Clock = true", "Clock = false");
      } else if (kind === "Random") {
        changedManifest = originalManifest.replace("Clock = true", "Clock = true\nRandom = false");
      }
      writeFileSync(manifest, changedManifest);
      ordinaryTask(false);
    }
  }));

test("captured policy, binding names and artifact bytes resist caller and on-disk mutation", async () =>
  changed(async () => {
    const input = options("captured-policy"),
      originalPolicy = { ...input.policy };
    input.envBindings = [];
    let calls = 0;
    input.signal = {
      throwIfAborted() {
        if (++calls === 2) {
          input.policy.workers = 99;
          input.policy.requestMs = 1;
          input.envBindings.push("UNGRANTED_NAME");
          writeFileSync(wasmPath, "no longer valid Wasm");
          writeFileSync(bindingsPath, malicious(contract));
        }
      },
    };
    const result = await packageVercelNode(input);
    const directory = join(
      result.directory,
      "functions",
      `${input.name}-${result.generation.slice(0, 48)}.func`,
    );
    assert.deepEqual(readFileSync(join(directory, "app.wasm")), wasm);
    assert.equal(result.wasmSha256, hash(wasm));
    assert.equal(result.sourceBindingsSha256, hash(Buffer.from(companion)));
    const entry = readFileSync(join(directory, "index.js"), "utf8");
    assert.ok(entry.includes("const policy=" + JSON.stringify(originalPolicy)));
    assert.ok(entry.includes("const names=[]"));
    assert.ok(!entry.includes("UNGRANTED_NAME"));
    assert.equal(existsSync(marker), false);
  }));

test("invalid lifecycle policy fails before creating output", async () => {
  for (const patch of [
    { workers: 9 },
    { startupMs: 0 },
    { requestMs: 20001 },
    { stopKillMs: 199 },
    { restartLimit: -1 },
    { retryAfter: 1 },
    { retryAfter: "" },
    { retryAfter: "1\r\nx-secret: private" },
    { retryAfter: "雪" },
  ]) {
    const input = options("invalid-policy");
    Object.assign(input.policy, patch);
    await assert.rejects(packageVercelNode(input), /SDK Vercel deployment/);
    assert.equal(existsSync(input.outputDirectory), false);
  }
});

test(
  "relocated installed SDK rebuilds through its task and emitted Node graph serves retained state",
  { timeout: 20000 },
  () => {
    const moved = join(compiled.directory, "relocated");
    renameSync(compiled.installation, moved);
    const relocatedApp = join(moved, "app");
    run(["build", "--engine", "node"], relocatedApp);
    run(["run", "deploy-vercel", "--engine", "node"], join(relocatedApp, "src"));
    const relocatedOutput = join(relocatedApp, ".vercel/output");
    const config = JSON.parse(readFileSync(join(relocatedOutput, "config.json"), "utf8"));
    const directory = join(relocatedOutput, "functions", config.routes[0].dest.slice(1) + ".func");
    const standalone = join(compiled.directory, "standalone.func");
    renameSync(directory, standalone);
    for (const [path, name] of [
      ["app/src", "saved-app-source"],
      ["app/tasks", "saved-task-source"],
      ["sdk/src", "saved-sdk-source"],
      ["sdk/providers", "saved-providers"],
    ]) {
      renameSync(join(moved, path), join(compiled.directory, name));
    }
    renameSync(compiled.cache, join(compiled.directory, "saved-cache"));
    const script = fileURLToPath(new URL("./helpers/vercel-runtime-control.ts", import.meta.url));
    const cwd = join(compiled.directory, "unrelated/nested");
    mkdirSync(cwd, { recursive: true });
    const stdout = execFileSync(process.execPath, [script, standalone], {
      cwd,
      env: environment(join(compiled.directory, "empty-cache")),
      encoding: "utf8",
      timeout: 15000,
    });
    assert.match(
      stdout,
      /VERCEL-RETAINED-CONTROL:.*"counts":\[1,2,3\].*"healthy":4.*"stalled":\[431,413\].*"cleanupBackstops":0/,
    );
  },
);

test("Vercel scheduled lifecycle budgets reject MAX+1 before creating output", async () => {
  for (const name of ["startupMs", "requestMs", "drainMs", "stopKillMs"]) {
    const input = options("timer-overflow-" + name.toLowerCase());
    input.policy[name] = 2147483648;
    await assert.rejects(
      packageVercelNode(input),
      new RegExp(name + " must not exceed 2147483647 milliseconds"),
    );
    assert.equal(existsSync(input.outputDirectory), false);
  }
});

test("Vercel scheduled lifecycle budgets accept MAX while request budget stays fixed", () => {
  assert.doesNotThrow(() =>
    validatePolicy({
      ...policy(),
      startupMs: 2147483647,
      drainMs: 2147483647,
      stopKillMs: 2147483647,
      restartWindowMs: 2147483648,
    }),
  );
  // The public supervisor accepts MAX requestMs; this deployed profile keeps20s.
  assert.throws(
    () => validatePolicy({ ...policy(), requestMs: 2147483647 }),
    /fixed server bounds/,
  );
});
