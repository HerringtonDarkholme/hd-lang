import { commandHelp, overviewHelp } from "../cli-args.ts";
import { runRepl, type ReplIo } from "../repl-terminal.ts";
import { workingDirectory, type CommandEnvironment, type CommandIo } from "./io.ts";
import { packageMode } from "./package-mode.ts";

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
  const mode = await packageMode(workingDirectory(environment));
  if (mode.kind !== "package") return runRepl(io);
  const pkg = mode.package;
  return runRepl(io, {}, { files: pkg.files, programs: pkg.executables.map(({ path }) => path) });
}
