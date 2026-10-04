// The `hd` command line: parses the arguments, calls the one command
// function they name (commands/), and returns its exit status. bin/hd.js
// sets the process exit code from it.

import { pathToFileURL } from "node:url";

import { parseCommandLine, UsageError, type ParsedCommand } from "./cli-args.ts";
import {
  buildCommand,
  checkCommand,
  defCommand,
  docCommand,
  EXIT_HD_FAILURE,
  explainCommand,
  helpCommand,
  hirCommand,
  parseCommand,
  processIo,
  replCommand,
  runCommand,
  testCommand,
  type CommandEnvironment,
  type CommandIo,
  type PackageTree,
  type RuntimeProfileName,
  type RuntimeScenario,
  type TestLayout,
} from "./commands/index.ts";

type Command = ParsedCommand & { kind: "command" };

/** A command's typed flags, read from the parsed command line. */
function flags(parsed: Command) {
  const value = (name: string): string | undefined => {
    const flag = parsed.flags.get(name);
    return typeof flag === "string" ? flag : undefined;
  };
  const count = (name: string): number | undefined => {
    const flag = value(name);
    return flag === undefined ? undefined : Number(flag);
  };
  const tree = value("--package-tree");
  const packageTree: PackageTree | undefined =
    tree === undefined ? undefined : { tree, path: String(value("--package-path")) };
  return {
    format: parsed.format,
    file: parsed.operands[0]!,
    profile: value("--profile") as RuntimeProfileName | undefined,
    testLayout: value("--test-layout") as TestLayout | undefined,
    packageTree,
    tests: parsed.flags.has("--tests"),
    wat: parsed.flags.has("--wat"),
    entry: value("--entry"),
    update: parsed.flags.has("--update"),
    seed: count("--seed"),
    cases: count("--cases"),
    shrink: count("--shrink"),
    scenario: value("--scenario") as RuntimeScenario | undefined,
    pendingFunction: value("--pending-function"),
  };
}

/** The flag combinations that `parseCommandLine` alone cannot reject. */
function checkFlags(parsed: Command): void {
  const where = `hd ${parsed.command.name}`;
  const has = (name: string): boolean => parsed.flags.has(name);
  if (
    parsed.flags.get("--pending-function") &&
    parsed.flags.get("--scenario") !== "cancellation-cleanup"
  )
    throw new UsageError(`${where}: --pending-function needs --scenario cancellation-cleanup`);
  if (has("--package-tree") !== has("--package-path"))
    throw new UsageError(`${where}: --package-tree and --package-path go together`);
  if (has("--package-tree") && has("--test-layout"))
    throw new UsageError(`${where}: --package-tree and --test-layout exclude each other`);
  if (has("--package-tree") && parsed.operands.length === 0)
    throw new UsageError(`${where}: --package-tree needs a FILE`);
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
    if (parsed.kind === "command") checkFlags(parsed);
  } catch (error) {
    if (!(error instanceof UsageError)) throw error;
    io.err(error.message);
    return EXIT_HD_FAILURE;
  }
  if (parsed.kind === "help") return helpCommand({ topic: parsed.topic }, io);
  const [first, second] = parsed.operands;
  const { format } = parsed;
  const options = { ...flags(parsed), ...environment };
  switch (parsed.command.name) {
    case "repl":
      return replCommand({ input: process.stdin, output: process.stdout });
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
      return checkCommand(options, io);
    case "build":
      return buildCommand(options, io);
    case "run":
      return runCommand(options, io);
    case "test":
      return testCommand({ ...options, path: first }, io);
    default:
      throw new Error(`hd ${parsed.command.name} has no command function`);
  }
}

const invokedPath = process.argv[1] && pathToFileURL(process.argv[1]).href;
if (invokedPath === import.meta.url) process.exitCode = await main();
