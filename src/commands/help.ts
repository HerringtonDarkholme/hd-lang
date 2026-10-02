import { commandHelp, overviewHelp } from "../cli-args.ts";
import { runRepl, type ReplIo } from "../repl-terminal.ts";
import type { CommandIo } from "./io.ts";

export interface HelpArgs {
  /** A command, such as `test` or `debug hir`; absent for the command list. */
  readonly topic?: string;
}

/** `hd help [COMMAND]`: the command list, or one command's usage and flags. */
export function helpCommand(args: HelpArgs, io: CommandIo): number {
  io.out(args.topic ? commandHelp(args.topic) : overviewHelp());
  return 0;
}

/** `hd repl`: an interactive session over `io`'s streams, until end of input or `:quit`. */
export function replCommand(io: ReplIo): Promise<number> {
  return runRepl(io);
}
