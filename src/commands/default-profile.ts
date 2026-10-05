// The default profile's host providers on Node
// (spec/cli/command-line.md#r-cli.host.default-profile): what `hd FILE`,
// `hd run`, and a task bind for each trait of the profile that the entry
// row names. `Console` stays a built-in of every run (host-functions.ts);
// this file answers `ConsoleInput`, `Args`, `Env`, `Clock`, `Random`,
// `FsRead`, and `FsWrite`. Each answer is a boundary value, which the
// adapter checks against the method's declared result
// (spec/lang/10-modules.md#host-results).

import { randomBytes } from "node:crypto";
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
import { RuntimePanicError } from "../runtime-panic.ts";

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

/** Waits `milliseconds` in real time, blocking the thread, as a CLI program may. */
function wait(milliseconds: number): void {
  if (milliseconds <= 0) return;
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, milliseconds);
}

const ANSWERS: Readonly<Record<string, Answer>> = {
  "std.host.Args.program": (_call, host) => host.program,
  "std.host.Args.list": (_call, host) => [...host.arguments],
  "std.host.Env.get": (call, host) => optional(host.variables[String(call.arguments[0])]),
  "std.host.Env.names": (_call, host) =>
    Object.entries(host.variables)
      .filter(([, value]) => value !== undefined)
      .map(([name]) => name),
  // The end of input is `.Ok(.None)`, and a read that fails is
  // `.Err(ConsoleError.Closed)` (cli.host.default-profile.input-closed).
  "std.console.ConsoleInput.read_line": (_call, host) => {
    const line = host.readLine();
    return line === null
      ? ({ tag: "err", value: { tag: "Closed" } } as HostBoundaryValue)
      : ok(optional(line));
  },
  // The wall clock, and a monotonic clock whose origin is the start of the
  // `hd` process (spec/std/time.md#r-std-time.instant.decl).
  "std.time.Clock.now": () => ({ millis: BigInt(Date.now()) }) as HostBoundaryValue,
  "std.time.Clock.monotonic": () =>
    ({ millis: BigInt(Math.floor(performance.now())) }) as HostBoundaryValue,
  "std.time.Clock.sleep": (call) => {
    const milliseconds = (call.arguments[0] as { readonly millis: bigint }).millis;
    // A negative duration panics on every provider (std-time.clock.sleep.negative).
    if (milliseconds < 0n)
      throw new RuntimePanicError(
        "explicit-panic",
        `sleep! with a negative duration of ${milliseconds} milliseconds`,
      );
    wait(Number(milliseconds));
  },
  // The operating system's random source.
  "std.random.Random.next_u64": () => randomBytes(8).readBigUInt64BE(0),
  "std.random.Random.fill": (call) => [...randomBytes(Number(call.arguments[0]))],
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
};

/**
 * The answer of the default profile to `call`, or undefined when `call` is
 * on no trait of the profile.
 */
export function defaultProfileAnswer(
  call: HostSuspensionCall,
  host: DefaultProfileHost,
): HostSuspensionOutcome | undefined {
  const answer = call.standardName ? ANSWERS[`${call.standardName}.${call.methodName}`] : undefined;
  return answer ? ready(answer(call, host) ?? undefined) : undefined;
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
