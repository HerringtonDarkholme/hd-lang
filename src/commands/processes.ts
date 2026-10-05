// The `Process` provider that `hd test` binds for an integration test
// (spec/cli/command-line.md#r-cli.test.process): its programs are the
// package's executables, each started by its name, as `hd run NAME` would.

import { spawnSync } from "node:child_process";
import { constants } from "node:os";
import { resolve } from "node:path";

import type { HostBoundaryValue, HostSuspensionCall, HostSuspensionOutcome } from "../compiler.ts";
import { displayType } from "../types.ts";
import type { LocalPackage } from "./package-mode.ts";

/** The `hd` executable that runs a package's executables. */
const HD = resolve(import.meta.dirname, "..", "..", "bin", "hd.js");

/** Answers `Process.run!` calls for the executables of `pkg`, or for none without one. */
export type ProcessProvider = (call: HostSuspensionCall) => HostSuspensionOutcome;

/** Whether `call` is a call on `std.process.Process`, under whatever name the program gives it. */
export function isProcessCall(call: HostSuspensionCall): boolean {
  return displayType(call.providerKey) === "Process";
}

/**
 * A provider whose programs are `pkg`'s executables. A name that names no
 * executable is `.Err(.NotFound)` (spec/cli/command-line.md#r-cli.test.process.missing).
 * Each executable runs in the package directory
 * (spec/cli/command-line.md#r-cli.test.process.cwd), in the test profile,
 * a checked build (spec/cli/command-line.md#r-cli.profile.test), and its
 * output is decoded as UTF-8 with U+FFFD for each invalid sequence
 * (spec/cli/command-line.md#r-cli.test.process.decode). A non-zero exit is
 * `.Ok` (spec/lang/10-modules.md#processes).
 */
export function executableProcesses(
  pkg: Pick<LocalPackage, "executables" | "root"> | undefined,
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
    const executable = pkg?.executables.find((candidate) => candidate.name === name);
    if (!pkg || !executable) return ready("err", { tag: "NotFound" });
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
