// Runs `hd` command lines in this process: `main` from src/cli.ts writes to
// a buffering sink, so a test sees the exit status and output a spawned `hd`
// gives without starting a process. The in-process conformance adapter's
// workers (hd-adapter-worker.ts) run each command line the same way.

import { inspect } from "node:util";

import { main } from "../src/cli.ts";
import { bufferedIo, type CommandEnvironment } from "../src/commands/index.ts";

/** What one command line did, as a spawned `hd` process reports it. */
export interface HdResult {
  readonly status: number;
  readonly stdout: string;
  readonly stderr: string;
}

/**
 * The conformance runner passes `--scenario pending-first-poll` as a command
 * line option, but that scenario is a harness hook, not an `hd` option
 * (`CommandEnvironment.pendingFirstPoll`): the adapter takes it out of the
 * arguments before they reach the command-line parser.
 */
function interceptPendingFirstPoll(args: readonly string[]): {
  readonly args: string[];
  readonly pendingFirstPoll: boolean;
} {
  const kept: string[] = [];
  let pendingFirstPoll = false;
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === "--scenario" && args[index + 1] === "pending-first-poll") {
      pendingFirstPoll = true;
      index += 1;
    } else kept.push(args[index]!);
  }
  return { args: kept, pendingFirstPoll };
}

/**
 * Runs `hd ARGS...`. `environment` stands in for the process's current
 * directory and `HD_SPEC_DIR`, so a test never changes the process's own.
 */
export async function runHd(
  args: readonly string[],
  environment: CommandEnvironment = {},
): Promise<HdResult> {
  const io = bufferedIo();
  let status: number;
  try {
    const intercepted = interceptPendingFirstPoll(args);
    status = await main(
      intercepted.args,
      io,
      intercepted.pendingFirstPoll ? { ...environment, pendingFirstPoll: true } : environment,
    );
  } catch (error) {
    // An error that escapes `main` ends the `hd` process with status 1 after
    // Node prints it.
    io.err(inspect(error));
    status = 1;
  }
  // A process exit status is one byte.
  return { status: ((status % 256) + 256) % 256, ...io.output() };
}

/** A failed {@link hd} call: the shape of a failed `execFile`. */
export interface HdFailure extends Error {
  readonly code: number;
  readonly stdout: string;
  readonly stderr: string;
}

/**
 * Runs `hd ARGS...` the way a promisified `execFile` runs a process: it
 * resolves with the output on exit status 0, and otherwise rejects with an
 * {@link HdFailure} whose message ends with the standard error.
 */
export async function hd(
  args: readonly string[],
  environment: CommandEnvironment = {},
): Promise<{ readonly stdout: string; readonly stderr: string }> {
  const { status, stdout, stderr } = await runHd(args, environment);
  if (status === 0) return { stdout, stderr };
  const message = `Command failed: hd ${args.join(" ")}\n${stderr}`;
  throw Object.assign(new Error(message), { code: status, stdout, stderr }) as HdFailure;
}
