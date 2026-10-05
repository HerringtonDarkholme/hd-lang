// Dependencies (spec/cli/command-line.md#dependencies): the implicit fetch
// of `hd check`, `hd build`, `hd run`, and `hd test`, and the dependency
// commands `hd add`, `hd update`, `hd remove`, and `hd fetch`.

import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { join, relative, sep } from "node:path";

import { DiagnosticReporter, Report, type OutputFormat } from "../diagnostic-report.ts";
import { removeDependency, setDependency } from "../dependencies/manifest-edit.ts";
import {
  compareVersions,
  compatibilityLine,
  invalidKey,
  parseHostRequirement,
  parseRequirement,
  repositoryUrl,
  tagVersion,
  type Version,
} from "../dependencies/requirement.ts";
import {
  resolveDependencies,
  resolveWorkspaceDependencies,
  workspaceRoot,
  type Resolution,
} from "../dependencies/resolve.ts";
import { formatSum, readSum, SUM_FILE, type SumEntries } from "../dependencies/sum.ts";
import { GitError, listTags } from "../dependencies/git.ts";
import { readManifest, type Manifest } from "../manifest.ts";
import {
  EXIT_HD_FAILURE,
  variablesOf,
  workingDirectory,
  type CommandEnvironment,
  type CommandIo,
} from "./io.ts";
import {
  MANIFEST_FILE,
  packageMode,
  type LocalPackage,
  type PackageProblem,
} from "./package-mode.ts";
import { reportPackageProblems } from "./source.ts";

/** Where the package's `hd.sum` lives: beside its workspace manifest, or its own (module.sum.file). */
async function sumPath(pkg: LocalPackage): Promise<string> {
  return join((await workspaceRoot(pkg.root)) ?? pkg.root, SUM_FILE);
}

/** `hd.sum`'s entries, or the problem that it is malformed. */
async function readSumFile(pkg: LocalPackage, path: string): Promise<SumEntries | PackageProblem> {
  if (!existsSync(path)) return new Map();
  const read = readSum(await readFile(path, "utf8"));
  if ("entries" in read) return read.entries;
  return {
    path: relative(pkg.root, path).split("\\").join("/"),
    line: read.line,
    column: 1,
    code: null,
    message: read.message,
    severity: "error",
  };
}

/**
 * The resolution's problems, each at a line of the package's `hd.toml`, or
 * of another workspace member's, by its path from the package directory.
 */
function manifestProblems(pkg: LocalPackage, resolution: Resolution): PackageProblem[] {
  return resolution.problems.map(({ directory, line, code, message }) => ({
    path: relative(pkg.root, join(directory, MANIFEST_FILE)).split(sep).join("/"),
    line,
    column: 1,
    code,
    message,
    severity: "error",
  }));
}

/** Each fetch is a line on standard error, as Go's "downloading" lines are. */
function progress(io: CommandIo): (version: string) => void {
  return (version) => io.err(`hd: fetching ${version}`);
}

/**
 * The package with its dependencies selected and fetched, for a command
 * that compiles it (spec/cli/command-line.md#r-cli.dep.implicit-fetch). A
 * dependency problem joins the package's problems, which the command
 * reports before it compiles; it never writes `hd.sum` (cli.dep.no-sum-write).
 */
export async function withDependencies(
  pkg: LocalPackage,
  environment: CommandEnvironment,
  fetching?: (version: string) => void,
): Promise<LocalPackage> {
  const { manifest } = pkg;
  if (!manifest || manifest.dependencies.length === 0) return pkg;
  const path = await sumPath(pkg);
  const sums = await readSumFile(pkg, path);
  if (!(sums instanceof Map))
    return { ...pkg, problems: [...pkg.problems, sums as PackageProblem] };
  const resolution = await resolveDependencies(pkg.root, manifest, sums, {
    variables: variablesOf(environment),
    sums: "verify",
    ...(fetching ? { fetching } : {}),
  });
  return {
    ...pkg,
    problems: [...pkg.problems, ...manifestProblems(pkg, resolution)],
    dependencies: resolution.graph,
  };
}

/** The arguments every dependency command takes. */
export interface DependencyArgs extends CommandEnvironment {
  readonly format: OutputFormat;
}

/** What a dependency command does to the manifest's text, or the error it reports. */
type ManifestChange = (
  text: string,
  manifest: Manifest,
  pkg: LocalPackage,
) => Promise<
  | { readonly text: string; readonly done: string[] }
  | { readonly error: string; readonly code?: string }
>;

/**
 * Runs a dependency command: finds the package (cli.dep.package-only),
 * changes its manifest, selects and fetches against the changed manifest,
 * and only then writes `hd.toml` and `hd.sum` (cli.dep.unchanged-on-error).
 */
async function dependencyCommand(
  command: string,
  args: DependencyArgs,
  io: CommandIo,
  change: ManifestChange,
  sumsOnly: "tidy" | "add",
): Promise<number> {
  const report = new Report(args.format, io, { stream: "stdout", summary: true });
  const start = workingDirectory(args);
  const mode = await packageMode(start);
  // At a workspace root, hd fetch fetches for the whole workspace
  // (cli.dep.workspace-fetch); the commands that edit a requirement run in
  // a member (cli.dep.workspace-member-only).
  if (mode.kind === "workspace" && command === "fetch")
    return workspaceFetch(mode.root, args, io, report);
  if (mode.kind !== "package") {
    report.commandError(
      mode.kind === "workspace"
        ? `hd ${command}: ${join(mode.root, MANIFEST_FILE)} is a workspace manifest, and hd ${command} changes one member's requirements; run it inside that member's directory`
        : `hd ${command}: not in a package (no ${MANIFEST_FILE} in ${start} or a directory above it); create one with hd new`,
    );
    return report.finish(EXIT_HD_FAILURE);
  }
  const pkg = mode.package;
  const fail = async (problems: readonly PackageProblem[]): Promise<number> => {
    await reportPackageProblems(report, pkg, problems, args);
    return report.finish(EXIT_HD_FAILURE);
  };
  if (!pkg.manifest) return fail(pkg.problems);
  // A package under a workspace manifest that does not list it is an error
  // (spec/cli/command-line.md#r-cli.mode.member.unlisted).
  if (pkg.unlisted) return fail([pkg.unlisted]);
  const manifestPath = join(pkg.root, MANIFEST_FILE);
  const before = await readFile(manifestPath, "utf8");
  const changed = await change(before, pkg.manifest, pkg);
  if ("error" in changed) {
    report.commandError(`hd ${command}: ${changed.error}`, changed.code ?? null);
    return report.finish(EXIT_HD_FAILURE);
  }
  const read = readManifest(changed.text);
  if (!("manifest" in read))
    return fail(
      read.errors.map(({ line, message }) => ({
        path: MANIFEST_FILE,
        line,
        column: 1,
        code: null,
        message,
        severity: "error",
      })),
    );
  const path = await sumPath(pkg);
  const sums = await readSumFile(pkg, path);
  if (!(sums instanceof Map)) return fail([sums as PackageProblem]);
  const resolution = await resolveDependencies(pkg.root, read.manifest, sums, {
    variables: variablesOf(args),
    sums: "record",
    fetching: progress(io),
  });
  // The problems point into the changed manifest, so they show its lines.
  if (resolution.problems.length > 0)
    return fail(manifestProblems(pkg, resolution).map((problem) => ({ ...problem })));
  // `hd fetch` only adds entries (cli.dep.fetch); the others tidy (cli.dep.tidy).
  if (changed.text !== before) await writeFile(manifestPath, changed.text);
  await writeSum(path, sumsOnly === "add" ? sums : new Map(), resolution.sums);
  if (args.format === "text") for (const line of changed.done) io.out(line);
  return report.finish(0);
}

/**
 * `hd fetch` at a workspace root (spec/cli/command-line.md#r-cli.dep.workspace-fetch):
 * selects for every member at once, fetches what the cache lacks, and adds
 * the missing lines to the workspace's `hd.sum`, as `hd fetch` in a member does.
 */
async function workspaceFetch(
  root: string,
  args: DependencyArgs,
  io: CommandIo,
  report: Report,
): Promise<number> {
  const read = readManifest(await readFile(join(root, MANIFEST_FILE), "utf8"));
  if (!("manifest" in read)) {
    report.commandError(
      `hd fetch: ${join(root, MANIFEST_FILE)}:${read.errors[0]!.line}: ${read.errors[0]!.message}`,
    );
    return report.finish(EXIT_HD_FAILURE);
  }
  const path = join(root, SUM_FILE);
  let sums: SumEntries = new Map();
  if (existsSync(path)) {
    const parsed = readSum(await readFile(path, "utf8"));
    if (!("entries" in parsed)) {
      report.commandError(`hd fetch: ${path}:${parsed.line}: ${parsed.message}`);
      return report.finish(EXIT_HD_FAILURE);
    }
    sums = parsed.entries;
  }
  const resolution = await resolveWorkspaceDependencies(root, read.manifest.members, sums, {
    variables: variablesOf(args),
    sums: "record",
    fetching: progress(io),
  });
  if (resolution.problems.length > 0) {
    // Each problem is at a line of a member's hd.toml, shown from the root.
    const cwd = workingDirectory(args);
    for (const { directory, line, code, message } of resolution.problems) {
      const file = relative(root, join(directory, MANIFEST_FILE)).split(sep).join("/");
      const reporter = new DiagnosticReporter(
        report,
        relative(cwd, join(directory, MANIFEST_FILE)) || file,
        "",
        undefined,
        file,
      );
      if (code === null) reporter.uncoded(message, line, 1);
      else {
        const position = { offset: 0, line, column: 1 };
        reporter.diagnostic({ code, message, span: { start: position, end: position } });
      }
    }
    return report.finish(EXIT_HD_FAILURE);
  }
  await writeSum(path, sums, resolution.sums);
  return report.finish(0);
}

/**
 * Writes `hd.sum` with `kept` and then `recorded`, unless that changes
 * nothing; an empty selection writes no new file.
 */
async function writeSum(
  path: string,
  kept: SumEntries,
  recorded: ReadonlyMap<string, string>,
): Promise<void> {
  const entries = new Map(kept);
  for (const [key, hash] of recorded) entries.set(key, hash);
  const sumText = formatSum(entries);
  const sumBefore = existsSync(path) ? await readFile(path, "utf8") : undefined;
  if (sumText !== (sumBefore ?? "") && (sumBefore !== undefined || entries.size > 0))
    await writeFile(path, sumText);
}

export interface AddArgs extends DependencyArgs {
  readonly name: string;
  readonly requirement: string;
  /** `--dev`: the requirement goes to `[dev-dependencies]` (cli.dep.add.dev). */
  readonly dev?: boolean;
}

/**
 * `hd add [--dev] NAME PATH@VERSION` (spec/cli/command-line.md#r-cli.dep.add):
 * sets NAME's requirement in `[dependencies]`, or in `[dev-dependencies]`
 * with `--dev`, after checking both (cli.dep.add.check), then fetches and
 * records hashes.
 */
export function addCommand(args: AddArgs, io: CommandIo): Promise<number> {
  const table = args.dev ? "dev-dependencies" : "dependencies";
  const other = args.dev ? "dependencies" : "dev-dependencies";
  return dependencyCommand(
    "add",
    args,
    io,
    async (text, manifest) => {
      const badKey = invalidKey(args.name);
      if (badKey !== undefined) return { error: badKey, code: "invalid-requirement" };
      const requirement = parseHostRequirement(args.requirement);
      if (typeof requirement === "string")
        return { error: requirement, code: "invalid-requirement" };
      // The key lives in one table. An add to the other table is rejected
      // until the user removes it explicitly (cli.dep.add.other-table).
      // An earlier version of the same host path lowers the requirement,
      // and hd add says so
      // (spec/cli/command-line.md#r-cli.dep.add.lower).
      const wanted = !!args.dev;
      const entry = manifest.dependencies.find(
        ({ key, dev }) => key === args.name && dev === wanted,
      );
      if (
        entry === undefined &&
        manifest.dependencies.some(({ key, dev }) => key === args.name && dev !== wanted)
      )
        return {
          error: `${args.name} is in [${other}]; run \`hd remove ${args.name}\` first`,
        };
      const current = entry && parseRequirement(entry.value);
      const lowered =
        typeof current === "object" &&
        current.kind === "host" &&
        current.host.path === requirement.host.path &&
        compareVersions(requirement.version, current.version) < 0
          ? current.version
          : undefined;
      return {
        text: setDependency(text, args.name, args.requirement, table),
        done: [
          lowered
            ? `lowered ${args.name} ${lowered.text} -> ${requirement.version.text}`
            : `${args.name} = "${args.requirement}"`,
        ],
      };
    },
    "tidy",
  );
}

export interface RemoveArgs extends DependencyArgs {
  readonly name: string;
}

/** `hd remove NAME` (spec/cli/command-line.md#r-cli.dep.remove). */
export function removeCommand(args: RemoveArgs, io: CommandIo): Promise<number> {
  return dependencyCommand(
    "remove",
    args,
    io,
    async (text, manifest) => {
      const removed = removeDependency(text, args.name);
      if (removed !== undefined) return { text: removed, done: [`removed ${args.name}`] };
      return manifest.dependencies.some(({ key }) => key === args.name)
        ? {
            error: `${args.name} is a [dependencies.${args.name}] table, which hd remove cannot edit; delete it from hd.toml by hand`,
          }
        : {
            error: `the package has no dependency named '${args.name}' in [dependencies] or [dev-dependencies]`,
          };
    },
    "tidy",
  );
}

/** `hd fetch` (spec/cli/command-line.md#r-cli.dep.fetch). */
export function fetchCommand(args: DependencyArgs, io: CommandIo): Promise<number> {
  return dependencyCommand("fetch", args, io, async (text) => ({ text, done: [] }), "add");
}

export interface UpdateArgs extends DependencyArgs {
  /** NAME; absent to move every requirement. */
  readonly name?: string;
}

/**
 * `hd update [NAME]` (spec/cli/command-line.md#r-cli.dep.update): moves each
 * requirement to the newest release tag on its compatibility line, which it
 * reads from the repository (cli.dep.update.network).
 */
export function updateCommand(args: UpdateArgs, io: CommandIo): Promise<number> {
  return dependencyCommand(
    "update",
    args,
    io,
    async (text, manifest) => {
      const entries = manifest.dependencies.filter(
        ({ key }) => args.name === undefined || key === args.name,
      );
      if (args.name !== undefined && entries.length === 0)
        return { error: `the package has no dependency named '${args.name}'` };
      let next = text;
      const done: string[] = [];
      for (const entry of entries) {
        // A path requirement and an invalid one have no tag to move to; the
        // selection reports an invalid one.
        const requirement = parseRequirement(entry.value);
        if (typeof requirement === "string" || requirement.kind !== "host") continue;
        let tags: Map<string, string>;
        try {
          tags = await listTags(repositoryUrl(requirement.host), variablesOf(args));
        } catch (error) {
          if (!(error instanceof GitError)) throw error;
          return {
            error: `cannot list the tags of ${requirement.host.path} at ${repositoryUrl(requirement.host)}: ${error.message}`,
            code: "fetch-failed",
          };
        }
        const newest = newestRelease(requirement.host, tags.keys(), requirement.version);
        if (!newest) continue;
        const value = `${requirement.host.path}@${newest.text}`;
        next = setDependency(
          next,
          entry.key,
          value,
          entry.dev ? "dev-dependencies" : "dependencies",
        );
        done.push(`${entry.key}: ${requirement.version.text} -> ${newest.text}`);
      }
      return { text: next, done };
    },
    "tidy",
  );
}

/**
 * The newest release on `current`'s compatibility line, when it is newer
 * than `current` (spec/cli/command-line.md#r-cli.dep.update.release).
 */
function newestRelease(
  host: Parameters<typeof tagVersion>[0],
  tags: Iterable<string>,
  current: Version,
): Version | undefined {
  let newest: Version | undefined;
  for (const tag of tags) {
    const version = tagVersion(host, tag);
    if (!version || version.pre.length > 0) continue;
    if (compatibilityLine(version) !== compatibilityLine(current)) continue;
    if (!newest || compareVersions(version, newest) > 0) newest = version;
  }
  return newest && compareVersions(newest, current) > 0 ? newest : undefined;
}
