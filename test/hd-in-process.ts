// Runs `hd` command lines in this process: `main` from src/cli.ts writes to
// a buffering sink, so a test sees the exit status and output a spawned `hd`
// gives without starting a process. The in-process conformance adapter's
// workers (hd-adapter-worker.ts) run each command line the same way.

import { inspect } from "node:util";

import { main } from "../src/cli.ts";
import {
  bufferedIo,
  EXIT_HD_FAILURE,
  RUNTIME_PROFILE_NAMES,
  RUNTIME_SCENARIO_NAMES,
  type CommandEnvironment,
  type RunnerOptions,
} from "../src/commands/index.ts";

/** What one command line did, as a spawned `hd` process reports it. */
export interface HdResult {
  readonly status: number;
  readonly stdout: string;
  readonly stderr: string;
}

/**
 * What the conformance runner selects for a command, besides its arguments
 * (spec/tools/README.md, Adapters). They are not `hd` options: this adapter
 * hands them to the command functions (`CommandEnvironment.runner`), so no
 * `hd` command line, help text, or usage error mentions them.
 */
export interface AdapterRunnerOptions {
  readonly profile?: string;
  readonly scenario?: string;
  readonly pendingFunction?: string;
  readonly packageRole?: string;
  readonly dependencies?: readonly { readonly name: string; readonly directory: string }[];
  readonly testLayout?: string;
  readonly packageTree?: { readonly directory: string; readonly path: string };
}

/** A runner option this compiler does not support: the command fails like a rejected command line. */
class UnsupportedRunnerOption extends Error {}

function oneOf<T extends string>(what: string, names: readonly T[], value: string): T {
  if (!(names as readonly string[]).includes(value))
    throw new UnsupportedRunnerOption(`${what} must be one of ${names.join(", ")}, not '${value}'`);
  return value as T;
}

/** The command-function options for the runner's `options`, and whether the first poll stays pending. */
function runnerEnvironment(options: AdapterRunnerOptions): CommandEnvironment {
  if (options.packageRole !== undefined || options.dependencies?.length)
    throw new UnsupportedRunnerOption(
      "the prototype does not support package roles or dependencies yet",
    );
  const { scenario } = options;
  const pendingFirstPoll = scenario === "pending-first-poll";
  const runner: RunnerOptions = {
    ...(options.profile === undefined
      ? {}
      : { profile: oneOf("profile", RUNTIME_PROFILE_NAMES, options.profile) }),
    ...(scenario === undefined || pendingFirstPoll
      ? {}
      : { scenario: oneOf("scenario", RUNTIME_SCENARIO_NAMES, scenario) }),
    ...(options.pendingFunction === undefined ? {} : { pendingFunction: options.pendingFunction }),
    ...(options.testLayout === undefined
      ? {}
      : {
          testLayout: oneOf(
            "test layout",
            ["test-module", "integration"] as const,
            options.testLayout,
          ),
        }),
    ...(options.packageTree === undefined
      ? {}
      : { packageTree: { tree: options.packageTree.directory, path: options.packageTree.path } }),
  };
  return { runner, ...(pendingFirstPoll ? { pendingFirstPoll } : {}) };
}

/**
 * Runs `hd ARGS...`. `environment` stands in for the process's current
 * directory and `HD_SPEC_DIR`, so a test never changes the process's own.
 * `runner` carries the conformance runner's options.
 */
export async function runHd(
  args: readonly string[],
  environment: CommandEnvironment = {},
  runner: AdapterRunnerOptions = {},
): Promise<HdResult> {
  const io = bufferedIo();
  let status: number;
  try {
    // Standard input is closed, as the conformance runner closes it
    // (spec/conformance/README.md#running-a-case), unless the caller gives a terminal.
    status = await main([...args], io, {
      terminal: null,
      ...environment,
      ...runnerEnvironment(runner),
    });
  } catch (error) {
    if (error instanceof UnsupportedRunnerOption) {
      io.err(`hd: ${error.message}`);
      return { status: EXIT_HD_FAILURE, ...io.output() };
    }
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
  runner: AdapterRunnerOptions = {},
): Promise<{ readonly stdout: string; readonly stderr: string }> {
  const { status, stdout, stderr } = await runHd(args, environment, runner);
  if (status === 0) return { stdout, stderr };
  const message = `Command failed: hd ${args.join(" ")}\n${stderr}`;
  throw Object.assign(new Error(message), { code: status, stdout, stderr }) as HdFailure;
}
