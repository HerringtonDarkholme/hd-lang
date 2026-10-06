// The `hd` commands, one function each. A command takes typed arguments and
// a `CommandIo` to write to, and returns its exit status; it never touches
// the process's streams or exit code. `cli.ts` turns a command line into one
// call, and the in-process conformance adapter (test/hd-adapter.ts) calls
// the same functions without starting a process.

export {
  bufferedIo,
  EXIT_HD_FAILURE,
  processIo,
  standardInput,
  terminalOf,
  type CommandEnvironment,
  type CommandIo,
  type CommandOutput,
  type RunnerOptions,
  type Terminal,
} from "./io.ts";
export {
  buildCommand,
  checkCommand,
  hirCommand,
  parseCommand,
  type BuildArgs,
  type CheckArgs,
  type HirArgs,
} from "./compile.ts";
export {
  fileCommand,
  moduleCommand,
  runCommand,
  testCommand,
  type FileArgs,
  type ModuleArgs,
  type RunArgs,
  type TestArgs,
} from "./execute.ts";
export {
  addCommand,
  fetchCommand,
  removeCommand,
  updateCommand,
  type AddArgs,
  type DependencyArgs,
  type RemoveArgs,
  type UpdateArgs,
} from "./dependencies.ts";
export { cleanCommand, type CleanArgs } from "./clean.ts";
export { helpCommand, replCommand, type HelpArgs } from "./help.ts";
export { newCommand, type NewArgs, type PackageKind } from "./new.ts";
export {
  defCommand,
  docCommand,
  explainCommand,
  type ExplainArgs,
  type LookupArgs,
} from "./queries.ts";
export { RUNTIME_PROFILE_NAMES, RUNTIME_SCENARIO_NAMES, type RuntimeScenario } from "./profiles.ts";
export type { PackageTree, RuntimeProfileName, SourceArgs, TestLayout } from "./source.ts";
