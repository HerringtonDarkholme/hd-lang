import { resolve } from "node:path";
import { packageVercelNode } from "../packaging/vercel.ts";
import { packageCloudflare } from "../packaging/cloudflare.ts";

function paths(options, context) {
  const identity = context.interfaceIdentity;
  if (
    typeof identity?.package !== "string" ||
    !identity.package ||
    typeof identity.version !== "string" ||
    identity.trait_path !== "deployment.Deployment"
  ) {
    throw new Error("SDK deployment requires its validated host interface identity");
  }
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
  vercel(options, context) {
    const supervisorOptions = options.supervisor;
    return result(() =>
      packageVercelNode({
        ...paths(options, context),
        envBindings: [...options.env_bindings],
        signal: context.signal,
        policy: {
          workers: supervisorOptions.workers,
          startupMs: supervisorOptions.startup_ms,
          requestMs: 20000,
          drainMs: supervisorOptions.drain_ms,
          stopKillMs: supervisorOptions.stop_kill_ms,
          restartLimit: supervisorOptions.restart_limit,
          restartWindowMs: supervisorOptions.restart_window_ms,
          retryAfter: supervisorOptions.retry_after,
        },
      }),
    );
  },
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
