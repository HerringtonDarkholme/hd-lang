import type { ReadStream } from "node:tty";

import { commandHelp, overviewHelp } from "../cli-args.ts";
import type { ReplHost } from "../repl.ts";
import { runRepl, type ReplIo } from "../repl-terminal.ts";
import {
  defaultProfileAnswer,
  DEFAULT_PROFILE_TRAITS,
  inputLines,
  type DefaultProfileHost,
} from "./default-profile.ts";
import { variablesOf, workingDirectory, type CommandEnvironment, type CommandIo } from "./io.ts";
import { MANIFEST_FILE, packageMode } from "./package-mode.ts";
import { withDependencies } from "./dependencies.ts";

export interface HelpArgs {
  /** A command, such as `test` or `debug hir`; absent for the command list. */
  readonly topic?: string;
}

/** `hd help [COMMAND]`: the command list, or one command's usage and flags. */
export function helpCommand(args: HelpArgs, io: CommandIo): number {
  io.out(args.topic ? commandHelp(args.topic) : overviewHelp());
  return 0;
}

/**
 * `hd` on a terminal, and `hd repl`: an interactive session over `io`'s
 * streams, until end of input or `:quit`. In package mode the session acts
 * as code inside `src/lib.hd` (spec/cli/command-line.md#r-cli.repl.package.lib);
 * in a workspace or outside any package it may use only `std`
 * (spec/cli/command-line.md#r-cli.repl.outside, spec/cli/command-line.md#r-cli.workspace.repl).
 */
export async function replCommand(
  io: ReplIo,
  environment: CommandEnvironment = {},
): Promise<number> {
  const host = replHost(io, environment);
  const mode = await packageMode(workingDirectory(environment));
  if (mode.kind !== "package") return runRepl(io, {}, undefined, host);
  // The session may use the package's dependencies, which are selected and
  // fetched as for hd check (cli.repl.package.dependencies, cli.dep.implicit-fetch).
  const pkg = await withDependencies(mode.package, environment, (version) =>
    io.output.write(`hd: fetching ${version}\n`),
  );
  for (const problem of pkg.problems)
    if (problem.severity === "error" && problem.path.endsWith(MANIFEST_FILE))
      io.output.write(
        `${problem.path}:${problem.line}: ${problem.code ?? "error"}: ${problem.message}\n`,
      );
  return runRepl(
    io,
    {},
    {
      files: pkg.files,
      name: pkg.name,
      programs: pkg.executables.map(({ path }) => path),
      ...(pkg.dependencies ? { dependencies: pkg.dependencies } : {}),
    },
    host,
  );
}

/**
 * The default profile, as a session binds it
 * (spec/cli/command-line.md#r-cli.repl.host.default-profile): relative paths
 * from the command's working directory (cli.repl.host.cwd), and `Args` with
 * an empty program name and no arguments (cli.repl.host.args).
 */
function replHost(io: ReplIo, environment: CommandEnvironment): ReplHost {
  let lines: (() => string | undefined | null) | undefined;
  // `read_line!` reads a line typed at the terminal, which the session's
  // own reader holds in raw mode. On any other input, the session's inputs
  // are standard input, so a program sees its end.
  const readLine = (): string | undefined | null => {
    if (io.input !== process.stdin) return undefined;
    const terminal = process.stdin as ReadStream;
    const raw = terminal.isTTY && terminal.isRaw;
    if (raw) terminal.setRawMode(false);
    try {
      return (lines ??= inputLines())();
    } finally {
      if (raw) terminal.setRawMode(true);
    }
  };
  const host: DefaultProfileHost = {
    program: "",
    arguments: [],
    variables: variablesOf(environment),
    workingDirectory: workingDirectory(environment),
    readLine,
  };
  return { traits: DEFAULT_PROFILE_TRAITS, invoke: (call) => defaultProfileAnswer(call, host) };
}
