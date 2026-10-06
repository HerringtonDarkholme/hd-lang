// The `Process` providers. `hd test` binds one for an integration test
// (spec/cli/command-line.md#r-cli.test.process): its programs are the
// package's executables and tasks (cli.test.process.tasks), each started by
// its name, as `hd run NAME` would. The default profile's starts a host
// program (runHostProgram). Both check the `Process` grant first.

import { spawnSync } from "node:child_process";
import { constants } from "node:os";
import { resolve } from "node:path";

import type { HostBoundaryValue, HostSuspensionCall, HostSuspensionOutcome } from "../compiler.ts";
import { displayType } from "../types.ts";
import type { Grant } from "./capabilities.ts";
import type { LocalPackage } from "./package-mode.ts";

/**
 * Whether the `Process` grant covers `program`: an entry equals it, as the
 * program wrote it, before the host looks it up
 * (spec/cli/command-line.md#r-cli.cap.scope.process).
 */
export function coversProgram(grant: Grant, program: string): boolean {
  if (grant.kind === "all") return true;
  if (grant.kind === "deny") return false;
  return grant.entries.includes(program);
}

/** `.Err(ProcessError.NotGranted)` (spec/lang/10-modules.md#r-module.process.not-granted). */
export const PROCESS_NOT_GRANTED = {
  tag: "err",
  value: { tag: "NotGranted" },
} as HostBoundaryValue;

/**
 * The default profile's `Process.run!` (spec/cli/command-line.md#host-capabilities):
 * the operating system finds `program` by its name, and it runs in the
 * program's working directory with the whole environment of `hd`
 * (cli.host.process.env). Its output is decoded as UTF-8 with U+FFFD.
 */
export function runHostProgram(
  [program, args, stdin]: readonly [string, readonly string[], string],
  directory: string,
  variables: Readonly<Record<string, string | undefined>>,
): HostBoundaryValue {
  const ran = spawnSync(program, [...args], {
    cwd: directory,
    env: variables,
    input: stdin,
    maxBuffer: 1 << 30,
  });
  if (ran.error) {
    const code = (ran.error as NodeJS.ErrnoException).code;
    const error =
      code === "ENOENT"
        ? { tag: "NotFound" }
        : code === "EACCES" || code === "EPERM"
          ? { tag: "PermissionDenied" }
          : { tag: "Other", message: ran.error.message };
    return { tag: "err", value: error } as HostBoundaryValue;
  }
  const text = (bytes: Buffer): string => new TextDecoder("utf-8").decode(bytes);
  return {
    tag: "ok",
    value: {
      stdout: text(ran.stdout),
      stderr: text(ran.stderr),
      status: ran.status ?? 128 + (ran.signal ? constants.signals[ran.signal] : 0),
    },
  } as HostBoundaryValue;
}

/** The `hd` executable that runs a package's executables. */
const HD = resolve(import.meta.dirname, "..", "..", "bin", "hd.js");

/** Answers `Process.run!` calls for the executables of `pkg`, or for none without one. */
export type ProcessProvider = (call: HostSuspensionCall) => HostSuspensionOutcome;

/** Whether `call` is a call on `std.process.Process`, under whatever name the program gives it. */
export function isProcessCall(call: HostSuspensionCall): boolean {
  return displayType(call.providerKey) === "Process";
}

/**
 * A provider whose programs are `pkg`'s executables and tasks; a name names
 * at most one (spec/cli/command-line.md#r-cli.task.name-clash). A name that
 * names neither is `.Err(.NotFound)`
 * (spec/cli/command-line.md#r-cli.test.process.unknown).
 * Each executable runs in the package directory
 * (spec/cli/command-line.md#r-cli.test.process.cwd), in the test profile,
 * a checked build (spec/cli/command-line.md#r-cli.profile.test), and its
 * output is decoded as UTF-8 with U+FFFD for each invalid sequence
 * (spec/cli/command-line.md#r-cli.test.process.decode). A non-zero exit is
 * `.Ok` (spec/lang/10-modules.md#processes).
 */
export function executableProcesses(
  pkg: Pick<LocalPackage, "executables" | "tasks" | "root"> | undefined,
  variables: Readonly<Record<string, string | undefined>> = process.env,
): ProcessProvider {
  // A `Result[ProcessOutput, ProcessError]` as the host boundary takes it.
  const ready = (tag: "ok" | "err", value: HostBoundaryValue): HostSuspensionOutcome => ({
    pending: false,
    value: { tag, value } as HostBoundaryValue,
  });
  return (call) => {
    if (call.methodName !== "run") throw new Error(`Process has no method ${call.methodName}`);
    const [name, args, stdin] = call.arguments as readonly [string, readonly string[], string];
    const program = [...(pkg?.executables ?? []), ...(pkg?.tasks ?? [])].find(
      (candidate) => candidate.name === name,
    );
    if (!pkg || !program) return ready("err", { tag: "NotFound" });
    // `hd run` gets the command's own environment, so it reads the same
    // cache and git configuration (spec/cli/command-line.md#cache).
    const ran = spawnSync(process.execPath, ["--no-warnings", HD, "run", name, "--", ...args], {
      cwd: pkg.root,
      env: variables,
      input: stdin,
      maxBuffer: 1 << 30,
    });
    if (ran.error) return ready("err", { tag: "Other", message: ran.error.message });
    const text = (bytes: Buffer): string => new TextDecoder("utf-8").decode(bytes);
    return ready("ok", {
      stdout: text(ran.stdout),
      stderr: text(ran.stderr),
      // A program that a signal ends has no status; it reports 128 plus the
      // signal's number, as a shell does.
      status: ran.status ?? 128 + (ran.signal ? constants.signals[ran.signal] : 0),
    });
  };
}
