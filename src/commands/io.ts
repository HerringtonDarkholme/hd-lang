import { formatWithOptions } from "node:util";

import type { RuntimeScenario } from "./profiles.ts";
import type { PackageTree, RuntimeProfileName, TestLayout } from "./source.ts";

/**
 * Where a command writes. Each call is one line, as `console.log` and
 * `console.error` write it: the line, then a newline.
 */
export interface CommandIo {
  /** Standard output. A non-string value prints as `console.log` prints it. */
  readonly out: (value: unknown) => void;
  /** Standard error. */
  readonly err: (line: string) => void;
}

/** The process's standard output and error, as `hd` writes them. */
export const processIo: CommandIo = {
  out: (value) => console.log(value),
  err: (line) => console.error(line),
};

/**
 * The status of a command line that `hd` rejects, or of a program it
 * rejects (`cli.exit.hd-failure`).
 */
export const EXIT_HD_FAILURE = 101;

/** What a command wrote to a {@link bufferedIo}. */
export interface CommandOutput {
  readonly stdout: string;
  readonly stderr: string;
}

/**
 * A sink that keeps the output as text, as a pipe would receive it: values
 * format without color, and text crosses as UTF-8, so a lone surrogate
 * becomes U+FFFD.
 */
export function bufferedIo(): CommandIo & { output(): CommandOutput } {
  let stdout = "";
  let stderr = "";
  return {
    out: (value) => {
      stdout += `${formatWithOptions({ colors: false }, value).toWellFormed()}\n`;
    },
    err: (line) => {
      stderr += `${line.toWellFormed()}\n`;
    },
    output: () => ({ stdout, stderr }),
  };
}

/**
 * What a command reads from its process apart from its arguments. Each field
 * defaults to the process's own value, so `hd` leaves them unset; a caller
 * that runs several commands in one process sets them per call instead of
 * changing the process.
 */
export interface CommandEnvironment {
  /** The directory relative paths resolve against: the current directory. */
  readonly cwd?: string;
  /** The specification directory: `HD_SPEC_DIR`, else the repository's `spec/`. */
  readonly specDir?: string;
  /**
   * Conformance harness hook, not an `hd` option: `hd test` holds the first
   * poll of every suspending host call pending (spec/conformance/README.md,
   * Runtime Scenarios, `pending-first-poll`). The in-process adapter
   * (test/hd-in-process.ts) sets it from the runner's `pending-first-poll`
   * scenario.
   */
  readonly pendingFirstPoll?: boolean;
  /** Conformance harness hooks, which the in-process adapter sets. */
  readonly runner?: RunnerOptions;
  /**
   * Standard input when it is a terminal, which a command may ask a question
   * on; null when it is not one, as when it is closed or a pipe. Undefined
   * stands for the process's own standard input.
   */
  readonly terminal?: Terminal | null;
}

/** A terminal on standard input and output: `hd new` asks which kind to create. */
export interface Terminal {
  /** Writes `question` and resolves to the line the user answers. */
  readonly ask: (question: string) => Promise<string>;
}

/** The terminal of `environment`, or null when standard input is not one. */
export function terminalOf(environment: CommandEnvironment): Terminal | null {
  if (environment.terminal !== undefined) return environment.terminal;
  if (!process.stdin.isTTY) return null;
  return {
    ask: async (question) => {
      const { createInterface } = await import("node:readline/promises");
      const lines = createInterface({ input: process.stdin, output: process.stdout });
      try {
        return await lines.question(question);
      } finally {
        lines.close();
      }
    },
  };
}

/**
 * What the conformance runner selects for a command, besides its arguments
 * (spec/conformance/README.md, Command Contract). They are not `hd` options:
 * a user's `hd` never sees them, and the adapter hands them to the command
 * functions here.
 */
export interface RunnerOptions {
  /** The runtime profile: the host capabilities a fixture's entry point may require. */
  readonly profile?: RuntimeProfileName;
  /** A runtime scenario that drives the program instead of running its test cases. */
  readonly scenario?: RuntimeScenario;
  /** The suspending function that stays pending, with `cancellation-cleanup`. */
  readonly pendingFunction?: string;
  /** Compile FILE as a test module of this layout. */
  readonly testLayout?: TestLayout;
  /** The other files of FILE's package, and the package path FILE takes. */
  readonly packageTree?: PackageTree;
}

/** The directory `environment`'s relative paths resolve against. */
export function workingDirectory(environment: CommandEnvironment): string {
  return environment.cwd ?? process.cwd();
}
