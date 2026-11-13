import { isAbsolute, relative, resolve, sep } from "node:path";
import { deploymentInputs, sha256 } from "./preflight.ts";
import { fixedBindingsModule } from "./companion.ts";
import { publishCloudflare } from "./output.ts";
import { environmentNames, entrySource } from "./cloudflare-entry.ts";
import { readTypeScriptJavaScript } from "./typescript-emission.ts";
const sdk = new URL("../", import.meta.url);
const bad = (message) => new Error("SDK Cloudflare deployment: " + message);
async function sdkJavaScript(name: string): Promise<Buffer> {
  return readTypeScriptJavaScript(new URL(name, sdk), name);
}
export async function packageCloudflare(options) {
  const { name, compatibilityDate, envBindings, signal } = options;
  if (typeof name !== "string" || !/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(name)) {
    throw bad("name must be a lowercase Worker name of at most 63 characters");
  }
  if (
    typeof compatibilityDate !== "string" ||
    !/^\d{4}-\d{2}-\d{2}$/.test(compatibilityDate) ||
    Number.isNaN(Date.parse(compatibilityDate)) ||
    new Date(compatibilityDate).toISOString().slice(0, 10) !== compatibilityDate
  ) {
    throw bad("compatibilityDate must be an ISO calendar date");
  }
  const output = resolve(options.outputDirectory),
    root = resolve(options.rootDirectory);
  const rootFromOutput = relative(output, root);
  if (
    rootFromOutput === "" ||
    (!isAbsolute(rootFromOutput) &&
      rootFromOutput !== ".." &&
      !rootFromOutput.startsWith(".." + sep))
  ) {
    throw bad("output cannot contain the root package");
  }
  const input = await deploymentInputs(options);
  const names = environmentNames(input.grants, envBindings);
  const binding = Buffer.from(fixedBindingsModule(input.contract, "runtime"));
  const files = new Map([
    ["entry.js", Buffer.from(entrySource(names))],
    ["app.wasm", input.wasm],
    ["bindings.js", binding],
    ["grants.json", Buffer.from(JSON.stringify(input.grants, null, 2) + "\n")],
    ["sdk/contract-bindings.js", await sdkJavaScript("packaging/contract-bindings.ts")],
  ]);
  for (const [name, source] of Object.entries(input.runtime.files)) {
    files.set("runtime/" + name, Buffer.from(source));
  }
  for (const name of ["application.ts", "dispatcher.ts", "console.ts", "http.ts", "fetch.ts"]) {
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
  const result = await publishCloudflare({
    outputDirectory: output,
    files,
    signal,
    configuration: {
      name,
      compatibility_date: compatibilityDate,
      rules: [{ type: "CompiledWasm", globs: ["**/*.wasm"], fallthrough: true }],
      secrets: { required: names },
    },
  });
  return Object.freeze({ ...result, ...receipt });
}
