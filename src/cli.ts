// The `hd` command line: parses the arguments, calls the one command
// function they name (commands/), and returns its exit status. bin/hd.js
// sets the process exit code from it.

import { pathToFileURL } from "node:url";

import { parseCommandLine, UsageError, type ParsedCommand } from "./cli-args.ts";
import {
  addCommand,
  buildCommand,
  checkCommand,
  defCommand,
  docCommand,
  EXIT_HD_FAILURE,
  explainCommand,
  fetchCommand,
  fileCommand,
  helpCommand,
  hirCommand,
  newCommand,
  parseCommand,
  processIo,
  removeCommand,
  replCommand,
  runCommand,
  testCommand,
  updateCommand,
  type CommandEnvironment,
  type CommandIo,
  type RunnerOptions,
  standardInput,
  terminalOf,
} from "./commands/index.ts";

type Command = ParsedCommand & { kind: "command" };

/**
 * A command's typed arguments: its flags, read from the parsed command line,
 * and the conformance runner's options, which only an adapter supplies.
 */
function flags(parsed: Command, runner: RunnerOptions = {}) {
  const value = (name: string): string | undefined => {
    const flag = parsed.flags.get(name);
    return typeof flag === "string" ? flag : undefined;
  };
  const count = (name: string): number | undefined => {
    const flag = value(name);
    return flag === undefined ? undefined : Number(flag);
  };
  return {
    format: parsed.format,
    file: parsed.operands[0]!,
    profile: runner.profile,
    testLayout: runner.testLayout,
    packageTree: runner.packageTree,
    tests: parsed.flags.has("--tests"),
    all: parsed.flags.has("--all"),
    wat: parsed.flags.has("--wat"),
    release: parsed.flags.has("--release"),
    entry: runner.entry,
    update: parsed.flags.has("--update"),
    filter: value("--filter"),
    denySkipped: parsed.flags.has("--deny-skipped"),
    seed: count("--seed"),
    cases: count("--cases"),
    shrink: count("--shrink"),
    scenario: runner.scenario,
    pendingFunction: runner.pendingFunction,
  };
}

/**
 * Runs one `hd` command line, writing to `io`, and returns its exit status.
 * `environment` replaces the process's current directory and `HD_SPEC_DIR`
 * for this call only.
 */
export async function main(
  args = process.argv.slice(2),
  io: CommandIo = processIo,
  environment: CommandEnvironment = {},
): Promise<number> {
  let parsed: ParsedCommand;
  try {
    parsed = parseCommandLine(args);
  } catch (error) {
    if (!(error instanceof UsageError)) throw error;
    io.err(error.message);
    return EXIT_HD_FAILURE;
  }
  if (parsed.kind === "help") return helpCommand({ topic: parsed.topic }, io);
  if (parsed.kind === "default") {
    // `hd` opens the REPL on a terminal (cli.repl.open.terminal), and
    // otherwise runs all of standard input as a single-file program, in
    // every mode (cli.stdin.program).
    if (terminalOf(environment))
      return replCommand({ input: process.stdin, output: process.stdout }, environment);
    const text = await standardInput(environment);
    return fileCommand({ ...environment, file: "<stdin>", text, format: parsed.format }, io);
  }
  const [first, second] = parsed.operands;
  const { format } = parsed;
  const options = { ...flags(parsed, environment.runner), ...environment };
  switch (parsed.command.name) {
    case "repl":
      return replCommand({ input: process.stdin, output: process.stdout }, environment);
    case "new": {
      const app = parsed.flags.has("--app");
      const lib = parsed.flags.has("--lib");
      if (app && lib) {
        io.err("hd new: pass one of --app and --lib, not both");
        return EXIT_HD_FAILURE;
      }
      return newCommand(
        {
          ...environment,
          ...(app ? { kind: "app" as const } : lib ? { kind: "lib" as const } : {}),
          vcs: parsed.flags.get("--vcs") !== "none",
          ...(first === undefined ? {} : { path: first }),
        },
        io,
      );
    }
    case "explain":
      return explainCommand({ code: first!, format, ...environment }, io);
    case "doc":
      return docCommand({ name: first!, target: second ?? ".", format, ...environment }, io);
    case "def":
      return defCommand({ name: first!, target: second ?? ".", format, ...environment }, io);
    case "parse":
    case "debug parse":
      return parseCommand(options, io);
    case "debug hir":
      return hirCommand(options, io);
    case "check":
      return checkCommand({ ...options, file: first }, io);
    case "add":
      return addCommand({ ...environment, format, name: first!, requirement: second! }, io);
    case "update":
      return updateCommand({ ...environment, format, ...(first ? { name: first } : {}) }, io);
    case "remove":
      return removeCommand({ ...environment, format, name: first! }, io);
    case "fetch":
      return fetchCommand({ ...environment, format }, io);
    case "build":
      return buildCommand({ ...options, file: first }, io);
    case "run":
      return runCommand({ ...options, name: first, programArguments: parsed.programArguments }, io);
    case "file":
      return fileCommand({ ...options, programArguments: parsed.programArguments }, io);
    case "test":
      return testCommand({ ...options, path: first }, io);
    default:
      throw new Error(`hd ${parsed.command.name} has no command function`);
  }
}

const invokedPath = process.argv[1] && pathToFileURL(process.argv[1]).href;
if (invokedPath === import.meta.url) process.exitCode = await main();
