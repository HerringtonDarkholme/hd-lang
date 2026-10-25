import { resolve } from "node:path";
import { packageCloudflare } from "../packaging/cloudflare.mjs";

function paths(options, context) {
  const identity = context.interfaceIdentity;
  if (
    typeof identity?.package !== "string" ||
    !identity.package ||
    typeof identity.version !== "string" ||
    identity.trait_path !== "deployment.Deployment"
  )
    throw new Error("SDK deployment requires its validated host interface identity");
  const rootDirectory = process.cwd();
  return {
    rootDirectory,
    wasmPath: resolve(rootDirectory, options.artifacts.wasm),
    bindingsPath: resolve(rootDirectory, options.artifacts.bindings),
    outputDirectory: resolve(rootDirectory, options.artifacts.output),
    name: options.name,
    expectedSDKIdentity: Object.freeze({
      package: identity.package,
      version: identity.version,
      trait_path: "server.HttpServer",
    }),
  };
}
const result = async (operation) => {
  try {
    const receipt = await operation();
    return {
      ok: true,
      value: {
        directory: receipt.directory,
        runtime_fingerprint: receipt.runtimeFingerprint,
        wasm_sha256: receipt.wasmSha256,
        bindings_sha256: receipt.bindingsSha256,
      },
    };
  } catch (error) {
    return { ok: false, error: { tag: "Failed", fields: { message: error.message } } };
  }
};

export default {
  cloudflare(options, context) {
    return result(() =>
      packageCloudflare({
        ...paths(options, context),
        compatibilityDate: options.compatibility_date,
        envBindings: [...options.env_bindings],
        signal: context.signal,
      }),
    );
  },
};
