// The default profile's host providers on Node
// (spec/cli/command-line.md#r-cli.host.default-profile): what `hd FILE`,
// `hd run`, and a task bind for each trait of the profile that the entry
// row names. `Console` stays a built-in of every run (host-functions.ts);
// this file answers `ConsoleInput`, `Args`, `Env`, `FsRead`, and
// `FsWrite`, `Clock` and `Random` through web-host.ts, and `Http` through
// http-host.ts. Each answer is a boundary value, which the
// adapter checks against the method's declared result
// (spec/lang/10-modules.md#host-results).

import {
  appendFileSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  readSync,
  renameSync,
  rmdirSync,
  statSync,
  unlinkSync,
  writeFileSync,
  type Stats,
} from "node:fs";
import { resolve } from "node:path";

import type { HostBoundaryValue, HostSuspensionCall, HostSuspensionOutcome } from "../compiler.ts";
import { wait, WEB_HOST_ANSWERS } from "../web-host.ts";
import {
  coversName,
  coversPath,
  resolvedPath,
  UNLIMITED,
  type CapabilityGrants,
  type Grant,
} from "./capabilities.ts";
import { sendRequest } from "./http-host.ts";
import { coversProgram, PROCESS_NOT_GRANTED, runHostProgram } from "./processes.ts";

/** What the default profile reads from the `hd` command that runs the program. */
export interface DefaultProfileHost {
  /** `Args.program`: the FILE or NAME the command ran (cli.host.default-profile). */
  readonly program: string;
  /** `Args.list`: the program arguments after `--` (cli.args.pass). */
  readonly arguments: readonly string[];
  /** `Env`: the environment of the `hd` process. */
  readonly variables: Readonly<Record<string, string | undefined>>;
  /** The working directory relative paths resolve against (cli.run.cwd.*). */
  readonly workingDirectory: string;
  /** Standard input: the next line, `undefined` at its end, or `null` when it cannot be read. */
  readonly readLine: () => string | undefined | null;
  /** The program's capability grants (cli.cap.*); a trait it lacks has no limit. */
  readonly grants?: CapabilityGrants;
  /** Writes a line of `hd`'s own to standard error, as the `Env` notice. */
  readonly notice?: (line: string) => void;
}

const grantOf = (host: DefaultProfileHost, trait: string): Grant =>
  host.grants?.get(trait) ?? UNLIMITED;

/** The names whose `Env` notice a host has written (cli.cap.env.notice). */
const noticed = new WeakMap<DefaultProfileHost, Set<string>>();

/**
 * Whether the `Env` grant covers `name`. A set variable it does not cover
 * gets one notice line per run (spec/cli/command-line.md#r-cli.cap.env.notice.text).
 */
function envGranted(host: DefaultProfileHost, name: string): boolean {
  if (coversName(grantOf(host, "Env"), name)) return true;
  const written = noticed.get(host) ?? new Set<string>();
  noticed.set(host, written);
  if (host.variables[name] !== undefined && !written.has(name)) {
    written.add(name);
    host.notice?.(`hd: env ${name} is set but not granted; run with --cap Env=${name}`);
  }
  return false;
}

/**
 * The trait whose grant covers each file system method, and the indices of
 * its path arguments (spec/cli/command-line.md#r-cli.cap.scope.path);
 * `rename!` needs both of its paths (cli.cap.scope.rename).
 */
const FS_PATHS: Readonly<Record<string, readonly [string, readonly number[]]>> = {
  "std.fs.FsRead.read_bytes": ["FsRead", [0]],
  "std.fs.FsRead.read_text": ["FsRead", [0]],
  "std.fs.FsRead.list_dir": ["FsRead", [0]],
  "std.fs.FsRead.stat": ["FsRead", [0]],
  "std.fs.FsWrite.write_bytes": ["FsWrite", [0]],
  "std.fs.FsWrite.write_text": ["FsWrite", [0]],
  "std.fs.FsWrite.append_text": ["FsWrite", [0]],
  "std.fs.FsWrite.create_dir_all": ["FsWrite", [0]],
  "std.fs.FsWrite.remove": ["FsWrite", [0]],
  "std.fs.FsWrite.rename": ["FsWrite", [0, 1]],
};

/**
 * `.Err(FsError.NotGranted(path))` for the first path of `call` that its
 * trait's grant does not cover (spec/cli/command-line.md#r-cli.cap.partial.refuse).
 */
function fsRefusal(
  key: string,
  call: HostSuspensionCall,
  host: DefaultProfileHost,
): HostBoundaryValue | undefined {
  const checked = FS_PATHS[key];
  if (!checked) return undefined;
  const grant = grantOf(host, checked[0]);
  if (grant.kind === "all") return undefined;
  for (const index of checked[1]) {
    const path = String(call.arguments[index]);
    if (!coversPath(grant, resolvedPath(host.workingDirectory, path)))
      return { tag: "err", value: { tag: "NotGranted", path } } as HostBoundaryValue;
  }
  return undefined;
}

/**
 * The traits of the default profile that this file answers, by module and
 * name; `Console` is a built-in of every run. A REPL session's entry row
 * names them (spec/cli/command-line.md#r-cli.repl.host.default-profile).
 */
export const DEFAULT_PROFILE_TRAITS: readonly { readonly module: string; readonly name: string }[] =
  [
    { module: "std.console", name: "ConsoleInput" },
    { module: "std.host", name: "Args" },
    { module: "std.host", name: "Env" },
    { module: "std.time", name: "Clock" },
    { module: "std.random", name: "Random" },
    { module: "std.fs", name: "FsRead" },
    { module: "std.fs", name: "FsWrite" },
    { module: "std.http", name: "Http" },
    { module: "std.process", name: "Process" },
  ];

type Answer = (call: HostSuspensionCall, host: DefaultProfileHost) => HostBoundaryValue | void;

const ready = (value?: HostBoundaryValue): HostSuspensionOutcome =>
  value === undefined ? { pending: false } : { pending: false, value };

const ok = (value?: HostBoundaryValue): HostBoundaryValue =>
  (value === undefined ? { tag: "ok" } : { tag: "ok", value }) as HostBoundaryValue;

const optional = (value: HostBoundaryValue | undefined): HostBoundaryValue =>
  value === undefined ? { tag: "none" } : { tag: "some", value };

/**
 * The `FsError` of a failed file system call, by its Node error code
 * (spec/std/fs.md#r-std-fs.error.other).
 */
function fsError(error: unknown, path: string): HostBoundaryValue {
  const code = (error as NodeJS.ErrnoException).code;
  const variant =
    code === "ENOENT"
      ? "NotFound"
      : code === "EACCES" || code === "EPERM"
        ? "PermissionDenied"
        : code === "EEXIST"
          ? "AlreadyExists"
          : code === "ENOTDIR"
            ? "NotADirectory"
            : code === "EISDIR"
              ? "IsADirectory"
              : undefined;
  return (
    variant ? { tag: variant, path } : { tag: "Other", message: (error as Error).message }
  ) as HostBoundaryValue;
}

/** Runs a file system call, and answers its `Result[T, FsError]`. */
function fsResult(path: string, run: () => HostBoundaryValue | void): HostBoundaryValue {
  try {
    return ok(run() ?? undefined);
  } catch (error) {
    return { tag: "err", value: fsError(error, path) } as HostBoundaryValue;
  }
}

function entry(path: string, stats: Stats): HostBoundaryValue {
  const kind = stats.isSymbolicLink() ? "Symlink" : stats.isDirectory() ? "Directory" : "File";
  return {
    path,
    kind: { tag: kind },
    size: kind === "File" ? BigInt(stats.size) : 0n,
  } as HostBoundaryValue;
}

function bytesOf(value: unknown): Uint8Array {
  return Uint8Array.from(value as readonly number[]);
}

const ANSWERS: Readonly<Record<string, Answer>> = {
  "std.host.Args.program": (_call, host) => host.program,
  "std.host.Args.list": (_call, host) => [...host.arguments],
  // A variable outside the `Env` grant reads as unset (cli.cap.env.get, cli.cap.env.names).
  "std.host.Env.get": (call, host) => {
    const name = String(call.arguments[0]);
    return optional(envGranted(host, name) ? host.variables[name] : undefined);
  },
  "std.host.Env.names": (_call, host) =>
    Object.entries(host.variables)
      .filter(([name, value]) => value !== undefined && coversName(grantOf(host, "Env"), name))
      .map(([name]) => name),
  // The end of input is `.Ok(.None)`, and a read that fails is
  // `.Err(ConsoleError.Closed)` (cli.host.default-profile.input-closed).
  "std.console.ConsoleInput.read_line": (_call, host) => {
    const line = host.readLine();
    return line === null
      ? ({ tag: "err", value: { tag: "Closed" } } as HostBoundaryValue)
      : ok(optional(line));
  },
  // `Clock` and `Random` need only web-platform APIs; the playground shares them.
  ...WEB_HOST_ANSWERS,
  "std.fs.FsRead.read_bytes": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () => [...readFileSync(resolve(host.workingDirectory, path))]);
  },
  "std.fs.FsRead.read_text": (call, host) => {
    const path = String(call.arguments[0]);
    let bytes: Uint8Array;
    try {
      bytes = readFileSync(resolve(host.workingDirectory, path));
    } catch (error) {
      return { tag: "err", value: fsError(error, path) } as HostBoundaryValue;
    }
    try {
      return ok(new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes));
    } catch {
      return { tag: "err", value: { tag: "InvalidUtf8", path } } as HostBoundaryValue;
    }
  },
  // Each entry's path is the directory's path, `/`, and its name, in the
  // byte order of the names, as a `MemoryFs` lists them.
  "std.fs.FsRead.list_dir": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () => {
      const directory = resolve(host.workingDirectory, path);
      return readdirSync(directory)
        .sort((left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right)))
        .map((name) => entry(`${path}/${name}`, lstatSync(resolve(directory, name))));
    });
  },
  "std.fs.FsRead.stat": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () => {
      const stats = lstatSync(resolve(host.workingDirectory, path), { throwIfNoEntry: false });
      return optional(stats ? entry(path, stats) : undefined);
    });
  },
  "std.fs.FsWrite.write_bytes": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () =>
      writeFileSync(resolve(host.workingDirectory, path), bytesOf(call.arguments[1])),
    );
  },
  "std.fs.FsWrite.write_text": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () =>
      writeFileSync(resolve(host.workingDirectory, path), String(call.arguments[1])),
    );
  },
  "std.fs.FsWrite.append_text": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () =>
      appendFileSync(resolve(host.workingDirectory, path), String(call.arguments[1])),
    );
  },
  "std.fs.FsWrite.create_dir_all": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () => {
      mkdirSync(resolve(host.workingDirectory, path), { recursive: true });
    });
  },
  // A file or an empty directory (std-fs.write.remove).
  "std.fs.FsWrite.remove": (call, host) => {
    const path = String(call.arguments[0]);
    return fsResult(path, () => {
      const target = resolve(host.workingDirectory, path);
      if (statSync(target).isDirectory()) rmdirSync(target);
      else unlinkSync(target);
    });
  },
  "std.fs.FsWrite.rename": (call, host) => {
    const from = String(call.arguments[0]);
    return fsResult(from, () =>
      renameSync(
        resolve(host.workingDirectory, from),
        resolve(host.workingDirectory, String(call.arguments[1])),
      ),
    );
  },
  "std.http.Http.send": (call, host) => sendRequest(call.arguments[0]!, grantOf(host, "Http")),
  // A program outside the grant never starts (cli.cap.scope.process).
  "std.process.Process.run": (call, host) => {
    const request = call.arguments as unknown as readonly [string, readonly string[], string];
    return coversProgram(grantOf(host, "Process"), request[0])
      ? runHostProgram(request, host.workingDirectory, host.variables)
      : PROCESS_NOT_GRANTED;
  },
};

/**
 * The answer of the default profile to `call`, or undefined when `call` is
 * on no trait of the profile.
 */
export function defaultProfileAnswer(
  call: HostSuspensionCall,
  host: DefaultProfileHost,
): HostSuspensionOutcome | undefined {
  const key = `${call.standardName}.${call.methodName}`;
  const answer = call.standardName ? ANSWERS[key] : undefined;
  if (!answer) return undefined;
  return ready(fsRefusal(key, call, host) ?? answer(call, host) ?? undefined);
}

/**
 * Lines of standard input, read as the program asks for them. `text` stands
 * for all of standard input when a caller supplied it; otherwise the lines
 * come from the process's file descriptor 0. Bytes that are not valid UTF-8
 * become U+FFFD.
 */
export function inputLines(text?: string): () => string | undefined | null {
  const decoder = new TextDecoder("utf-8");
  let pending = text ?? "";
  let ended = text !== undefined;
  let closed = false;
  const chunk = Buffer.alloc(64 * 1024);
  const fill = (): void => {
    for (;;) {
      try {
        const count = readSync(0, chunk, 0, chunk.length, null);
        if (count === 0) {
          pending += decoder.decode();
          ended = true;
        } else pending += decoder.decode(chunk.subarray(0, count), { stream: true });
        return;
      } catch (error) {
        // A non-blocking descriptor has no data yet; wait and read again.
        if ((error as NodeJS.ErrnoException).code === "EAGAIN") {
          wait(5);
          continue;
        }
        closed = true;
        return;
      }
    }
  };
  return () => {
    for (;;) {
      const newline = pending.indexOf("\n");
      if (newline !== -1) {
        const line = pending.slice(0, newline);
        pending = pending.slice(newline + 1);
        return line.endsWith("\r") ? line.slice(0, -1) : line;
      }
      if (ended) {
        if (pending === "") return undefined;
        const line = pending;
        pending = "";
        return line;
      }
      if (closed) return null;
      fill();
    }
  };
}
