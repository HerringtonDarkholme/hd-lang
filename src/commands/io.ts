import { formatWithOptions } from "node:util";

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
}

/** The directory `environment`'s relative paths resolve against. */
export function workingDirectory(environment: CommandEnvironment): string {
  return environment.cwd ?? process.cwd();
}
