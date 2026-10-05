// Selecting and fetching a package's dependencies
// (spec/cli/command-line.md#dependencies). Selection is minimal version
// selection (spec/lang/10-modules.md#version-selection): it starts at the
// package and the local packages its path requirements name, reads the
// manifest of every version a requirement reaches, and selects the largest
// minimum of each host path and compatibility line. A version the cache
// lacks is fetched with git. The result is the package graph the linker
// joins (src/package.ts), and each selected version's tree hash for `hd.sum`.

import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";

import { readManifest, type DependencyEntry, type Manifest } from "../manifest.ts";
import type { DependencyPackage, PackageDependencies } from "../package.ts";
import { hdFilesUnder } from "../commands/package-mode.ts";
import {
  cacheDirectory,
  cachedEntry,
  removeTree,
  scratchDirectory,
  storeEntry,
  type Variables,
} from "./cache.ts";
import { checkOutCommit, checkOutTag, GitError, listTags, packageDirectory } from "./git.ts";
import {
  compareVersions,
  compatibilityLine,
  invalidKey,
  pseudoCommit,
  parseRequirement,
  parseVersion,
  repositoryUrl,
  sourceName,
  tagName,
  tagVersion,
  type HostPath,
  type Requirement,
  type Version,
} from "./requirement.ts";
import { sumKey, type SumEntries } from "./sum.ts";

/** A dependency problem, at a line of the package's `hd.toml`. */
export interface DependencyProblem {
  readonly line: number;
  /** The stable code; null for a limit of the prototype that no rule names. */
  readonly code: string | null;
  readonly message: string;
}

export interface ResolveOptions {
  /** The command's environment variables: `HD_CACHE`, `HOME`, and what git reads. */
  readonly variables: Variables;
  /**
   * `verify`: every selected version needs its `hd.sum` entry, as in `hd
   * check` (cli.dep.missing-sum). `record`: a selected version without one
   * gets its tree hash, as in `hd add` and `hd fetch`.
   */
  readonly sums: "verify" | "record";
  /** Called before each fetch, with the version it fetches. */
  readonly fetching?: (version: string) => void;
}

/** The selected package graph, and what `hd.sum` holds for it. */
export interface Resolution {
  readonly graph: PackageDependencies;
  /** Each selected version's `hd.sum` entry: its existing one, or its new tree hash. */
  readonly sums: ReadonlyMap<string, string>;
  readonly problems: readonly DependencyProblem[];
}

/** A package of the graph: the root, a local package, or a fetched version. */
interface PackageNode {
  readonly id: string;
  readonly directory: string;
  readonly manifest: Manifest;
  /** A fetched version's host path and version; absent for a local package. */
  readonly fetched?: { readonly host: HostPath; readonly version: Version; readonly hash: string };
}

/** One checked requirement of a package, with the root line it is reported at. */
interface Edge {
  readonly entry: DependencyEntry;
  readonly requirement: Requirement;
  /** The line of the root manifest that reached it. */
  readonly line: number;
}

/** A host-path requirement on its way through selection. */
interface HostEdge {
  readonly host: HostPath;
  readonly version: Version;
  readonly line: number;
}

const lineKey = (host: string, version: Version): string =>
  `${host}\0${compatibilityLine(version)}`;

const posix = (path: string): string => path.split(sep).join("/");

/**
 * The nearest workspace manifest's directory above `directory` that lists
 * it as a member (spec/cli/command-line.md#r-cli.mode.member), or undefined.
 */
export async function workspaceRoot(directory: string): Promise<string | undefined> {
  for (let above = dirname(directory); ; above = dirname(above)) {
    const path = join(above, "hd.toml");
    if (existsSync(path)) {
      const read = readManifest(await readFile(path, "utf8"));
      if ("manifest" in read && read.manifest.workspace) {
        const member = posix(relative(above, directory));
        return read.manifest.members.some((listed) => posix(join(listed)) === member)
          ? above
          : undefined;
      }
    }
    if (dirname(above) === above) return undefined;
  }
}

async function readPackageManifest(directory: string): Promise<Manifest | string> {
  const path = join(directory, "hd.toml");
  if (!existsSync(path)) return `${directory} holds no hd.toml`;
  const read = readManifest(await readFile(path, "utf8"));
  if (!("manifest" in read)) return `${path}:${read.errors[0]!.line}: ${read.errors[0]!.message}`;
  if (read.manifest.name === undefined) return `${path} declares no package`;
  return read.manifest;
}

/**
 * Selects and fetches the dependencies of the package in `root`, whose
 * manifest is `manifest`, against the `hd.sum` entries `sums`.
 */
export async function resolveDependencies(
  root: string,
  manifest: Manifest,
  sums: SumEntries,
  options: ResolveOptions,
): Promise<Resolution> {
  const problems: DependencyProblem[] = [];
  const problem = (line: number, code: string | null, message: string): void => {
    problems.push({ line, code, message });
  };
  const rootNode: PackageNode = { id: posix(root), directory: root, manifest };
  // The requirements of each package, checked; the root's dev dependencies
  // are read, and no other package's (module.select.dev-dependencies).
  const edges = new Map<PackageNode, Edge[]>();
  const check = (node: PackageNode, at: (entry: DependencyEntry) => number): Edge[] => {
    const owner = node === rootNode ? "" : ` in ${shownNode(node)}`;
    const checked: Edge[] = [];
    const names = new Map<string, string>();
    const lines = new Map<string, string>();
    for (const entry of node.manifest.dependencies) {
      if (entry.dev && node !== rootNode) continue;
      const line = at(entry);
      const fail = (message: string): void =>
        problem(line, "invalid-requirement", `${message}${owner}`);
      const badKey = invalidKey(entry.key);
      if (badKey !== undefined) {
        fail(badKey);
        continue;
      }
      // Two keys with one source name are invalid (module.dep.key-name.collision).
      const other = names.get(sourceName(entry.key));
      if (other !== undefined) {
        fail(
          `the keys '${other}' and '${entry.key}' are both dep.${sourceName(entry.key)} in source`,
        );
        continue;
      }
      names.set(sourceName(entry.key), entry.key);
      const requirement = parseRequirement(entry.value);
      if (typeof requirement === "string") {
        fail(`${entry.key}: ${requirement}`);
        continue;
      }
      if (requirement.kind === "host") {
        // One key per host path and line (module.dep.one-key-per-line).
        const key = lineKey(requirement.host.path, requirement.version);
        const twin = lines.get(key);
        if (twin !== undefined) {
          fail(
            `'${twin}' and '${entry.key}' both require ${requirement.host.path} on line ${compatibilityLine(requirement.version)}; keep one key`,
          );
          continue;
        }
        lines.set(key, entry.key);
      }
      checked.push({ entry, requirement, line });
    }
    return checked;
  };

  // The root and the local packages its path requirements reach
  // (spec/lang/10-modules.md#r-module.workspace.path-requirement).
  const locals = new Map<string, PackageNode>([[rootNode.id, rootNode]]);
  const pending = [rootNode];
  const rootLine = new Map<PackageNode, number>();
  while (pending.length > 0) {
    const node = pending.shift()!;
    const reportAt = rootLine.get(node);
    const nodeEdges = check(node, (entry) => reportAt ?? entry.line);
    edges.set(node, nodeEdges);
    for (const edge of nodeEdges) {
      if (edge.requirement.kind !== "path") continue;
      const directory = resolve(node.directory, edge.requirement.path);
      const id = posix(directory);
      if (locals.has(id)) continue;
      const [own, other] = [await workspaceRoot(node.directory), await workspaceRoot(directory)];
      if (own === undefined || own !== other) {
        problem(
          edge.line,
          "invalid-requirement",
          `${edge.entry.key}: a path requirement names another member of the package's workspace, and ${edge.requirement.path} is not one; list both directories in the members of a workspace hd.toml`,
        );
        continue;
      }
      const read = await readPackageManifest(directory);
      if (typeof read === "string") {
        problem(edge.line, "invalid-requirement", `${edge.entry.key}: ${read}`);
        continue;
      }
      const local: PackageNode = { id, directory, manifest: read };
      locals.set(id, local);
      rootLine.set(local, edge.line);
      pending.push(local);
    }
  }
  if (problems.length > 0) return { graph: emptyGraph(), sums: new Map(), problems };

  const hostEdges = (node: PackageNode): HostEdge[] =>
    (edges.get(node) ?? []).flatMap(({ requirement, line }) =>
      requirement.kind === "host"
        ? [{ host: requirement.host, version: requirement.version, line }]
        : [],
    );
  const start = [...locals.values()].flatMap(hostEdges);
  // Without an hd.sum entry on a requirement's line, whatever selection
  // picks has none, so `hd check` fails before it fetches (cli.dep.missing-sum).
  if (options.sums === "verify") {
    for (const edge of start)
      if (!hasEntryOnLine(sums, edge)) problem(edge.line, "missing-sum-entry", missingSum(edge));
    if (problems.length > 0) return { graph: emptyGraph(), sums: new Map(), problems };
  }

  // Minimal version selection over every reached version
  // (spec/lang/10-modules.md#r-module.select.reach).
  const fetcher = new Fetcher(options);
  const reached = new Map<string, PackageNode>();
  const selected = new Map<string, HostEdge>();
  const queue = [...start];
  for (let index = 0; index < queue.length; index += 1) {
    const edge = queue[index]!;
    const line = lineKey(edge.host.path, edge.version);
    const best = selected.get(line);
    if (!best || compareVersions(edge.version, best.version) > 0) selected.set(line, edge);
    const key = sumKey(edge.host.path, edge.version.text);
    if (reached.has(key)) continue;
    const node = await fetcher.node(edge, problem);
    if (!node) continue;
    reached.set(key, node);
    const nodeEdges = check(node, () => edge.line).flatMap((checked): Edge[] => {
      if (checked.requirement.kind === "host") return [checked];
      // A fetched version's path requirement is a dependency requirement on
      // its version, by the host path it was fetched from
      // (module.workspace.path-version.fetched-version).
      const requirement = fetchedPathRequirement(node, checked);
      if (typeof requirement === "string") {
        problem(edge.line, "invalid-requirement", requirement);
        return [];
      }
      return [{ ...checked, requirement }];
    });
    edges.set(node, nodeEdges);
    queue.push(...hostEdges(node));
  }
  if (problems.length > 0) return { graph: emptyGraph(), sums: new Map(), problems };

  // Each selected version's entry: kept, recorded, or a mismatch
  // (spec/cli/command-line.md#r-cli.dep.verify, cli.sum.keep).
  const recorded = new Map<string, string>();
  const chosen = new Map<string, PackageNode>();
  for (const [line, edge] of selected) {
    const key = sumKey(edge.host.path, edge.version.text);
    const node = reached.get(key)!;
    chosen.set(line, node);
    const hash = node.fetched!.hash;
    const entry = sums.get(key);
    if (entry === undefined) {
      if (options.sums === "verify") problem(edge.line, "missing-sum-entry", missingSum(edge));
      else recorded.set(key, hash);
    } else if (entry !== hash)
      problem(
        edge.line,
        "sum-mismatch",
        `${key}: the fetched tree's hash ${hash} differs from its hd.sum entry ${entry}; hd never replaces an entry, so find out why the tree changed, as a moved tag, before you delete the entry and run hd fetch`,
      );
    else recorded.set(key, entry);
  }
  for (const node of [...locals.values(), ...chosen.values()])
    if (node !== rootNode && !existsSync(join(node.directory, "src", "lib.hd")))
      problem(
        lineOf(node, rootLine, selected),
        "invalid-requirement",
        `${shownNode(node)} has no library, src/lib.hd, so no package can depend on it`,
      );
  if (problems.length > 0) return { graph: emptyGraph(), sums: new Map(), problems };
  return {
    graph: await linkGraph(rootNode, locals, chosen, edges),
    sums: recorded,
    problems,
  };
}

function emptyGraph(): PackageDependencies {
  return { dependencies: {}, devDependencies: {}, packages: {} };
}

function shownNode(node: PackageNode): string {
  return node.fetched
    ? sumKey(node.fetched.host.path, node.fetched.version.text)
    : (node.manifest.name ?? node.directory);
}

function lineOf(
  node: PackageNode,
  rootLine: ReadonlyMap<PackageNode, number>,
  selected: ReadonlyMap<string, HostEdge>,
): number {
  if (!node.fetched) return rootLine.get(node) ?? 1;
  return selected.get(lineKey(node.fetched.host.path, node.fetched.version))?.line ?? 1;
}

function hasEntryOnLine(sums: SumEntries, edge: HostEdge): boolean {
  for (const key of sums.keys()) {
    const at = key.lastIndexOf("@");
    if (key.slice(0, at) !== edge.host.path) continue;
    const version = parseVersion(key.slice(at + 1));
    if (
      version &&
      compatibilityLine(version) === compatibilityLine(edge.version) &&
      compareVersions(version, edge.version) >= 0
    )
      return true;
  }
  return false;
}

function missingSum(edge: HostEdge): string {
  return `${sumKey(edge.host.path, edge.version.text)} has no hd.sum entry, so its tree cannot be checked; run hd fetch to fetch it and record its hash, or hd add to change the requirement`;
}

/**
 * The dependency requirement a fetched version's path requirement stands
 * for: its version, at the host path of the other directory in the same
 * repository; or the message of why it has none.
 */
function fetchedPathRequirement(node: PackageNode, edge: Edge): Requirement | string {
  const { host } = node.fetched!;
  const requirement = edge.requirement as Extract<Requirement, { kind: "path" }>;
  const shown = shownNode(node);
  // A released version holds no bare path requirement
  // (spec/lang/10-modules.md#r-module.version.no-bare-path-release).
  if (requirement.version === undefined)
    return `${shown} requires ${edge.entry.key} by a path with no version, so this version cannot be fetched as a dependency`;
  const directory = posix(
    join(host.subdirectory === "" ? "." : host.subdirectory, requirement.path),
  );
  if (directory === ".." || directory.startsWith("../"))
    return `${shown} requires ${edge.entry.key} at ${requirement.path}, outside its repository`;
  const subdirectory = directory === "." ? "" : directory;
  return {
    kind: "host",
    host: {
      path: subdirectory === "" ? host.repository : `${host.repository}/${subdirectory}`,
      repository: host.repository,
      subdirectory,
    },
    version: requirement.version,
  };
}

/** Finds each reached version in the cache, or fetches it with git. */
class Fetcher {
  private readonly tags = new Map<string, Promise<Map<string, string>>>();
  private readonly options: ResolveOptions;
  private cacheRoot: string | undefined;

  constructor(options: ResolveOptions) {
    this.options = options;
  }

  private cache(): string {
    this.cacheRoot ??= cacheDirectory(this.options.variables);
    return this.cacheRoot;
  }

  async node(
    edge: HostEdge,
    problem: (line: number, code: string | null, message: string) => void,
  ): Promise<PackageNode | undefined> {
    const { host, version, line } = edge;
    const key = sumKey(host.path, version.text);
    const cache = this.cache();
    const entry =
      (await cachedEntry(cache, host.path, version.text)) ??
      (await this.fetch(edge, cache, problem));
    if (!entry) return undefined;
    const manifest = await readPackageManifest(entry.directory);
    if (typeof manifest === "string") {
      problem(line, "invalid-requirement", `${key} is no package: ${manifest}`);
      return undefined;
    }
    return {
      id: key,
      directory: entry.directory,
      manifest,
      fetched: { host, version, hash: entry.hash },
    };
  }

  private async fetch(
    edge: HostEdge,
    cache: string,
    problem: (line: number, code: string | null, message: string) => void,
  ): Promise<{ directory: string; hash: string } | undefined> {
    const { host, version, line } = edge;
    const key = sumKey(host.path, version.text);
    const url = repositoryUrl(host);
    const pseudo = pseudoCommit(version);
    const tag = tagName(host, version);
    let scratch: string | undefined;
    try {
      if (pseudo) {
        // A pseudo-version names one commit by its hash and time; nothing
        // falls back to another commit (module.version.pseudo-missing).
        this.options.fetching?.(key);
        scratch = await scratchDirectory(cache);
        const into = join(scratch, "checkout");
        const found = await checkOutCommit(url, pseudo.hash, into, this.options.variables);
        if (found === "missing" || found.time !== pseudo.time) {
          problem(
            line,
            "unknown-version",
            found === "missing"
              ? `${host.path} has no commit ${pseudo.hash} on a branch or tag, so the pseudo-version ${version.text} does not exist`
              : `the commit ${pseudo.hash} of ${host.path} has the time ${found.time}, not ${pseudo.time}, so the pseudo-version ${version.text} names no commit; did you mean ${version.text.replace(pseudo.time, found.time)}?`,
          );
          return undefined;
        }
        return await this.store(edge, cache, packageDirectory(into, host.subdirectory), problem);
      }
      let tags = this.tags.get(url);
      if (!tags) {
        tags = listTags(url, this.options.variables);
        this.tags.set(url, tags);
      }
      const known = await tags;
      // A missing tag is an error; nothing falls back to another version
      // (spec/lang/10-modules.md#r-module.version.no-fallback).
      if (!known.has(tag)) {
        const near = [...known.keys()]
          .map((name) => tagVersion(host, name))
          .filter((found): found is Version => found !== undefined)
          .filter((found) => compatibilityLine(found) === compatibilityLine(version))
          .sort(compareVersions)
          .map(({ text }) => text);
        problem(
          line,
          "unknown-version",
          `${host.path} has no tag ${tag}, so version ${version.text} does not exist; ${near.length > 0 ? `its versions on line ${compatibilityLine(version)} are ${near.join(", ")}` : `it has no tag on line ${compatibilityLine(version)}`}`,
        );
        return undefined;
      }
      this.options.fetching?.(key);
      scratch = await scratchDirectory(cache);
      const checkout = await checkOutTag(
        url,
        tag,
        join(scratch, "checkout"),
        this.options.variables,
      );
      return await this.store(edge, cache, packageDirectory(checkout, host.subdirectory), problem);
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      problem(
        line,
        "fetch-failed",
        `cannot fetch ${key} from ${url}: ${error.message}; hd fetches with git and git's own credentials, so check that 'git ls-remote ${url}' works`,
      );
      return undefined;
    } finally {
      if (scratch !== undefined) await removeTree(scratch);
    }
  }

  /** Stores a checked-out package directory as the version's cache entry. */
  private async store(
    edge: HostEdge,
    cache: string,
    directory: string,
    problem: (line: number, code: string | null, message: string) => void,
  ): Promise<{ directory: string; hash: string } | undefined> {
    const { host, version, line } = edge;
    if (!existsSync(join(directory, "hd.toml"))) {
      const where = pseudoCommit(version)
        ? `the commit ${pseudoCommit(version)!.hash}`
        : `the tag ${tagName(host, version)}`;
      problem(
        line,
        "invalid-requirement",
        `${sumKey(host.path, version.text)} is no package: ${where} has no ${host.subdirectory === "" ? "" : `${host.subdirectory}/`}hd.toml`,
      );
      return undefined;
    }
    return storeEntry(cache, host.path, version.text, directory);
  }
}

/** The library files of a package: under `src/`, without its programs and test modules. */
async function libraryFiles(node: PackageNode): Promise<Record<string, string>> {
  const files = await hdFilesUnder(join(node.directory, "src"));
  const programs = new Set(["main.hd"]);
  for (const { module } of node.manifest.executables) {
    const base = module.split(".").join("/");
    programs.add(`${base}.hd`).add(`${base}/mod.hd`);
  }
  return Object.fromEntries(
    Object.entries(files).filter(([path]) => !programs.has(path) && !path.endsWith("_test.hd")),
  );
}

/** The linker's view of the selected graph (src/package.ts). */
async function linkGraph(
  rootNode: PackageNode,
  locals: ReadonlyMap<string, PackageNode>,
  chosen: ReadonlyMap<string, PackageNode>,
  edges: ReadonlyMap<PackageNode, readonly Edge[]>,
): Promise<PackageDependencies> {
  const target = (from: PackageNode, edge: Edge): PackageNode | undefined => {
    const { requirement } = edge;
    if (requirement.kind === "host")
      return chosen.get(lineKey(requirement.host.path, requirement.version));
    return locals.get(posix(resolve(from.directory, requirement.path)));
  };
  const shown = new Map<PackageNode, string>();
  for (const edge of edges.get(rootNode) ?? []) {
    const node = target(rootNode, edge);
    if (node) shown.set(node, `dep.${sourceName(edge.entry.key)}`);
  }
  const packages: Record<string, DependencyPackage> = {};
  for (const node of [...locals.values(), ...chosen.values()]) {
    if (node === rootNode) continue;
    const dependencies: Record<string, string> = {};
    for (const edge of edges.get(node) ?? []) {
      const found = target(node, edge);
      if (found) dependencies[sourceName(edge.entry.key)] = found.id;
    }
    packages[node.id] = {
      id: node.id,
      shown: shown.get(node) ?? shownNode(node),
      sourceRoot: posix(join(node.directory, "src")),
      files: await libraryFiles(node),
      dependencies,
    };
  }
  const dependencies: Record<string, string> = {};
  const devDependencies: Record<string, string> = {};
  for (const edge of edges.get(rootNode) ?? []) {
    const node = target(rootNode, edge);
    if (node)
      (edge.entry.dev ? devDependencies : dependencies)[sourceName(edge.entry.key)] = node.id;
  }
  return { dependencies, devDependencies, packages };
}
