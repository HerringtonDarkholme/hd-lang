import { isAbsolute, relative, resolve, sep } from "node:path";
import { deploymentInputs, sha256 } from "./preflight.ts";
import { fixedBindingsModule } from "./companion.ts";
import { publishVercelNode } from "./output.ts";
import { environmentNames } from "./cloudflare-entry.ts";
import { entrySource, deployedCoreSource } from "./vercel-entry.ts";
import { readTypeScriptJavaScript } from "./typescript-emission.ts";
const sdk = new URL("../", import.meta.url);
const bad = (message) => new Error("SDK Vercel deployment: " + message);
async function sdkJavaScript(name: string): Promise<Buffer> {
  return readTypeScriptJavaScript(new URL(name, sdk), name);
}
export function validatePolicy(policy) {
  for (const name of [
    "workers",
    "startupMs",
    "requestMs",
    "drainMs",
    "stopKillMs",
    "restartLimit",
    "restartWindowMs",
  ]) {
    if (!Number.isSafeInteger(policy?.[name]) || policy[name] < (name === "restartLimit" ? 0 : 1)) {
      throw bad("explicit lifecycle policy " + name + " required");
    }
  }
  for (const name of ["startupMs", "requestMs", "drainMs", "stopKillMs"]) {
    if (policy[name] > 2147483647) {
      throw bad(name + " must not exceed 2147483647 milliseconds");
    }
  }
  if (
    policy.workers > 8 ||
    policy.requestMs !== 20000 ||
    policy.stopKillMs < 200 ||
    typeof policy.retryAfter !== "string" ||
    !/^[\x20-\x7e]+$/.test(policy.retryAfter)
  ) {
    throw bad("policy violates fixed server bounds");
  }
}
export async function packageVercelNode(options) {
  const { name, signal } = options;
  const policy = Object.freeze({ ...options.policy }),
    envBindings = options.envBindings === undefined ? undefined : [...options.envBindings];
  if (typeof name !== "string" || !/^[a-z0-9][a-z0-9-]{0,62}$/.test(name)) {
    throw bad("simple function name required");
  }
  validatePolicy(policy);
  const output = resolve(options.outputDirectory),
    root = resolve(options.rootDirectory),
    containment = relative(output, root);
  if (
    containment === "" ||
    (!isAbsolute(containment) && containment !== ".." && !containment.startsWith(".." + sep))
  ) {
    throw bad("output cannot contain the root package");
  }
  const input = await deploymentInputs(options);
  const names = environmentNames(input.grants, envBindings);
  const binding = Buffer.from(fixedBindingsModule(input.contract, "runtime"));
  const files = new Map([
    ["index.js", Buffer.from(entrySource(names, policy))],
    ["app.wasm", input.wasm],
    ["bindings.js", binding],
    ["grants.json", Buffer.from(JSON.stringify(input.grants, null, 2) + "\n")],
    ["package.json", Buffer.from(JSON.stringify({ private: true, type: "module" }) + "\n")],
    [
      ".vc-config.json",
      Buffer.from(
        JSON.stringify(
          {
            runtime: "nodejs24.x",
            handler: "index.js",
            launcherType: "Nodejs",
            shouldAddHelpers: false,
          },
          null,
          2,
        ) + "\n",
      ),
    ],
    ["sdk/contract-bindings.js", await sdkJavaScript("packaging/contract-bindings.ts")],
    ["runtime/deployed-core.js", Buffer.from(deployedCoreSource)],
  ]);
  for (const [name, source] of Object.entries(input.runtime.files)) {
    files.set("runtime/" + name, Buffer.from(source));
  }
  for (const name of [
    "application.ts",
    "dispatcher.ts",
    "console.ts",
    "http.ts",
    "fetch.ts",
    "worker.ts",
    "supervisor.ts",
    "vercel-node.ts",
  ]) {
    files.set("sdk/" + name.replace(/\.ts$/, ".js"), await sdkJavaScript("adapters/" + name));
  }
  const receipt = {
    runtimeFingerprint: input.runtime.fingerprint,
    wasmSha256: input.wasmSha256,
    bindingsSha256: sha256(binding),
    sourceBindingsSha256: input.sourceBindingsSha256,
    schemaFingerprint: input.schemaFingerprint,
  };
  files.set("deployment.json", Buffer.from(JSON.stringify(receipt, null, 2) + "\n"));
  const result = await publishVercelNode({
    outputDirectory: output,
    files,
    configuration: { name },
    signal,
  });
  return Object.freeze({ ...result, ...receipt });
}
