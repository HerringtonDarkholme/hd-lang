import { HostContract } from "./wire.mts";

type Dynamic = ReturnType<typeof JSON.parse>;

// Installs invoker-selected package providers, or rejects a module whose
// embedded package requirements have no explicitly registered provider.
export function bindPackageRuntime(options: Dynamic): Dynamic {
  const { env, module, io, imports, grant, platform, isAsciiUrl, writtenHost } = options;
  if (env.packageBindings) {
    const http = Object.freeze({
      coversHost: (host: string, port: number) => grant.coversHost("Http", host, port),
      isAsciiUrl,
      writtenHost,
    });
    const console = Object.freeze({ allows: () => grant.allows("Console") });
    if (typeof env.createPackageRuntime !== "function") {
      throw new HostContract("package host runtime was not supplied");
    }
    const runtime = env.createPackageRuntime({ ...io, http, console }, env);
    try {
      Object.assign(imports, env.packageBindings(runtime));
      return runtime;
    } catch (error) {
      runtime.close();
      platform?.close?.();
      throw error;
    }
  }
  if (!module) {
    return null;
  }
  try {
    const sections = WebAssembly.Module.customSections(module, "hd.capabilities");
    if (sections.length > 1) {
      throw new HostContract("duplicate package host contracts");
    }
    if (sections.length) {
      if (typeof env.readPackageContract !== "function") {
        throw new HostContract("package host contract reader was not supplied");
      }
      const required = env.readPackageContract(new Uint8Array(sections[0]));
      const item = required.find((requiredItem: Dynamic) => !requiredItem.handle) ?? required[0];
      if (item) {
        throw new HostContract(
          `missing package host provider for ${item.identity.package}@${item.identity.version}/${item.identity.trait_path}; register it explicitly`,
        );
      }
    }
    return null;
  } catch (error) {
    platform?.close?.();
    throw error;
  }
}
