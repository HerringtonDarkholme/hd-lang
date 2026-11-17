// One program run in the calling thread, as `hd run` does it on V8: the
// package host contract, the startup host, one instance, `hd.init` then
// `hd.poll` to completion. `run.ts` calls it for a lone run (in its main
// thread or a supervised worker), and `stage-host.ts` for each run it is
// sent, so both report the same status and output for the same program.
import { createStartupHost } from "./startup.ts";
import { identityName } from "./core-package.ts";
import { HostContract } from "./run-support.ts";
import type { Sink } from "./core.ts";
import type { EntryCancel } from "./run-support.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;

interface HostProviderConfig {
  module: string;
  grantPath: string;
  path: string;
}

export interface RunConfig extends DynamicRecord {
  cleanupMs?: number | null;
  framesFile?: string;
  grants?: Record<string, boolean | string[]>;
  hostProviders?: HostProviderConfig[];
  interruptSignal?: NodeJS.Signals;
  processLauncher?: string;
  profile?: string;
  program?: string;
  statusFile?: string;
  timeLimitMs?: number | null;
}

export interface RunRequest {
  module: BufferSource | WebAssembly.Module;
  config: RunConfig;
  args: string[];
  sink: Sink;
  deadline?: number | null;
  cancel?: CancelControl | null;
  // The program's standard input: a descriptor, `null` for a closed one,
  // or left out for this process's own (descriptor 0).
  stdin?: number | null;
}

export type CancelControl = Pick<EntryCancel, "boundMs" | "requests" | "wait">;

// Builtins come from `process.getBuiltinModule`, which skips the ESM
// facade an `import` builds (node.ts).
const { writeFileSync } = process.getBuiltinModule("node:fs");
const { pathToFileURL } = process.getBuiltinModule("node:url");

// Runs `module` (bytes or a compiled module) in this thread under
// `config`, writing through `sink`, and returns the status. `cancel`,
// when given, is how a Ctrl-C reaches the run.
export async function runHere({
  module,
  config,
  args,
  sink,
  deadline = null,
  cancel = null,
  stdin,
}: RunRequest): Promise<number> {
  try {
    return await runHereUnchecked({ module, config, args, sink, deadline, cancel, stdin });
  } catch (error) {
    if (!(error instanceof HostContract)) {
      throw error;
    }
    sink.err(`error: ${error.message}\n`);
    return 101;
  }
}

async function packageHost(module: WebAssembly.Module, config: RunConfig): Promise<DynamicRecord> {
  const sections = WebAssembly.Module.customSections(module, "hd.capabilities");
  if (sections.length > 1) {
    throw new HostContract("duplicate package host contracts");
  }
  const required = sections.length
    ? (await import("./package-contract.ts")).readPackageContract(new Uint8Array(sections[0]))
    : [];
  const registrations = new Map();
  const grants = { ...config.grants };
  for (const registration of config.hostProviders ?? []) {
    const item = required.find((entry) => !entry.handle && entry.module === registration.module);
    if (!item || !item.selectors.includes(registration.grantPath)) {
      throw new HostContract("provider mapping names no required package capability");
    }
    if (registrations.has(item.module)) {
      throw new HostContract(`duplicate provider for ${item.identity.trait_path}`);
    }
    registrations.set(item.module, registration);
  }
  for (const item of required.filter((entry) => !entry.handle)) {
    const name = identityName(item.identity);
    if (item.selectors.some((selector) => grants[selector] === false)) {
      throw new HostContract(`package capability denied: ${name}`);
    }
    const registration = registrations.get(item.module);
    if (!registration) {
      throw new HostContract(
        `package capability ${name} is not granted; add its trait path to the app's [capabilities] table`,
      );
    }
    grants[registration.grantPath] = true;
  }
  if (!required.length) {
    return { grants, packageBindings: null, createPackageRuntime: null };
  }
  const [{ createPackageRuntime }, generated] = await Promise.all([
    import("./package-providers.ts"),
    // @ts-expect-error -- generated beside the runner when a package host is present.
    import("./main.host.mjs"),
  ]);
  const providers = new Map();
  for (const [name, registration] of registrations) {
    providers.set(name, (await import(pathToFileURL(registration.path).href)).default);
  }
  return {
    grants,
    createPackageRuntime,
    packageBindings: (runtime: Dynamic) => generated.bind(runtime, providers),
  };
}

async function runHereUnchecked({
  module,
  config,
  args,
  sink,
  deadline = null,
  cancel = null,
  stdin,
}: RunRequest): Promise<number> {
  // The unsupervised path may receive bytes; retain the compiled module
  // so checked reports can read its custom metadata too.
  module = module instanceof WebAssembly.Module ? module : await WebAssembly.compile(module);
  const { grants, packageBindings, createPackageRuntime } = await packageHost(module, config);
  const profile = config.profile ? await import("./profiles.ts") : null;
  const host = await createStartupHost(module, sink, {
    processLauncher: config.processLauncher,
    module,
    args,
    program: config.program ?? "",
    grants,
    providers: profile ? (io: DynamicRecord) => profile.providers(config.profile!, io) : null,
    packageBindings,
    createPackageRuntime,
    deadline,
    ...(stdin === undefined ? {} : { stdin }),
  });
  try {
    const instance = await WebAssembly.instantiate(module, host.imports);
    const { status, frames } = await host.run(instance, "hd.init", "hd.poll", {
      cancel,
    });
    // The frames of a trap, for `hd` to name (`flow.panic.report`).
    if (frames?.length && config.framesFile) {
      writeFileSync(config.framesFile, JSON.stringify(frames));
    }
    return status;
  } finally {
    host.close();
  }
}
