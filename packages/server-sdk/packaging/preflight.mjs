import { readFile, mkdir, writeFile, mkdtemp, rm } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { readProductionGrants } from "./grants.mjs";
import { readPortableRuntime } from "./portable-runtime.mjs";
import { checkCompanionPairing } from "./companion.mjs";
import { contractBindings } from "./contract-bindings.mjs";
import { readServerSchema } from "./schema-asset.mjs";
import { validateServerContract } from "./server-schema.mjs";

export const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const bad = (message) => new Error("SDK deployment: " + message);
const portable = new Set([
  "hd:rt",
  "hd:prim",
  "hd:Console",
  "hd:Clock",
  "hd:Random",
  "hd:Env",
  "hd:Http",
]);

// Only the installed SDK's checked source archive is loaded as JavaScript.
// Artifact JavaScript is never imported. The private temporary graph is removed
// after loading; the parser's imports and closures remain ordinary live ESM.
async function contractReader(snapshot) {
  const directory = await mkdtemp(join(tmpdir(), "hd-sdk-contract-"));
  try {
    await mkdir(join(directory, "gen"));
    for (const [name, source] of Object.entries(snapshot.files))
      await writeFile(join(directory, name), source, { encoding: "utf8", flag: "wx" });
    const [providers, core] = await Promise.all([
      import(pathToFileURL(join(directory, "package-providers.mjs")).href),
      import(pathToFileURL(join(directory, "core.mjs")).href),
    ]);
    return {
      readPackageContract: providers.readPackageContract,
      createCoreHost: core.createCoreHost,
    };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}
export async function deploymentInputs({
  rootDirectory,
  wasmPath,
  bindingsPath,
  expectedSDKIdentity,
  signal,
}) {
  signal?.throwIfAborted();
  const grants = await readProductionGrants(rootDirectory);
  const [wasmInput, sidecarInput, runtime, schema] = await Promise.all([
    readFile(wasmPath),
    readFile(bindingsPath),
    readPortableRuntime(),
    readServerSchema(),
  ]);
  const wasm = Uint8Array.from(wasmInput),
    sidecar = Uint8Array.from(sidecarInput);
  let module;
  try {
    module = new WebAssembly.Module(wasm);
  } catch {
    throw bad("input is not a valid Wasm module");
  }
  const exports = new Map(WebAssembly.Module.exports(module).map(({ name, kind }) => [name, kind]));
  if (
    !["hd.init", "hd.poll", "hd.wake"].every((name) => exports.get(name) === "function") ||
    exports.get("hd.x") !== "memory"
  )
    throw bad("module lacks the SDK's synchronous driver exports");
  const sections = WebAssembly.Module.customSections(module, "hd.capabilities");
  if (
    sections.length !== 1 ||
    sections[0].byteLength < 4 ||
    new DataView(sections[0]).getUint32(0, true) !== runtime.packageHostABI ||
    schema.packageHostABI !== runtime.packageHostABI
  )
    throw bad("module and SDK package-host ABI do not match");
  const contract = new Uint8Array(sections[0]);
  const { readPackageContract, createCoreHost } = await contractReader(runtime);
  const interfaces = readPackageContract(contract);
  const source = validateServerContract(interfaces, expectedSDKIdentity, schema.schema);
  if (
    !source.selectors.some((name) => grants[name] === true) ||
    source.selectors.some((name) => grants[name] === false)
  )
    throw bad("root production policy refuses the SDK HttpServer");
  checkCompanionPairing(sidecar, contract);
  const modules = new Set(interfaces.map((i) => i.module));
  for (const item of WebAssembly.Module.imports(module)) {
    if (!portable.has(item.module) && !modules.has(item.module))
      throw bad("unsupported deployed capability module " + item.module);
    if (["hd:Clock", "hd:Random"].includes(item.module) && grants[item.module.slice(3)] === false)
      throw bad("required primitive capability is explicitly denied: " + item.module.slice(3));
  }
  const bindings = contractBindings(contract, readPackageContract);
  const implementation = Object.fromEntries(
    source.methods.map((m) => [
      m.member,
      () => {
        throw bad("preflight must not invoke an application provider");
      },
    ]),
  );
  const host = createCoreHost(
    { out() {}, err() {} },
    {
      module,
      grants,
      packageBindings: (runtime) =>
        bindings.bind(runtime, new Map([[source.module, implementation]])),
    },
    {
      providers: () => ({
        traits: {
          "hd:Env": {
            get() {
              return null;
            },
            names() {
              return [];
            },
          },
          "hd:Http": {
            send() {
              throw bad("preflight must not perform outgoing HTTP");
            },
          },
        },
      }),
    },
  );
  try {
    for (const item of WebAssembly.Module.imports(module)) {
      const namespace = host.imports[item.module];
      if (
        item.kind !== "function" ||
        !namespace ||
        !Object.hasOwn(namespace, item.name) ||
        typeof namespace[item.name] !== "function"
      )
        throw bad("unsupported deployed import member " + item.module + "." + item.name);
    }
  } finally {
    host.close();
  }
  signal?.throwIfAborted();
  return {
    grants,
    wasm,
    contract,
    runtime,
    schemaFingerprint: schema.fingerprint,
    sourceBindingsSha256: sha256(sidecar),
    wasmSha256: sha256(wasm),
  };
}
