// Selecting and fetching a package's dependencies
// (spec/cli/command-line.md#dependencies). Selection is minimal version
// selection (spec/lang/10-modules.md#version-selection): it starts at the
// package, or at every member of its workspace
// (spec/lang/10-modules.md#r-module.workspace.selection), reads the manifest
// of every version a requirement reaches, and selects the largest minimum of
// each host path and compatibility line. A version the cache lacks is
// fetched with git. Each manifest read is checked against its `hd.sum`
// manifest line, and each selected tree against its tree line
// (spec/cli/command-line.md#hdsum). The result is the package graph the
// linker joins (src/package.ts), and the `hd.sum` lines of the selection.

import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve, sep } from "node:path";

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
  parseRequirement,
  pseudoCommit,
  repositoryUrl,
  sourceName,
  tagName,
  tagVersion,
  type HostPath,
  type Requirement,
  type Version,
} from "./requirement.ts";
import { manifestHash, manifestKey, sumKey, type SumEntries } from "./sum.ts";

/** A line of a local package's `hd.toml`: the root's, or another member's. */
export interface Place {
  /** The directory of the package whose `hd.toml` holds the line. */
  readonly directory: string;
  readonly line: number;
}

/** A dependency problem, at a line of a local package's `hd.toml`. */
export interface DependencyProblem extends Place {
  /** The stable code; null for a limit of the prototype that no rule names. */
  readonly code: string | null;
  readonly message: string;
}

export interface ResolveOptions {
  /** The command's environment variables: `HD_CACHE`, `HOME`, and what git reads. */
  readonly variables: Variables;
  /**
   * `verify`: every version selection reads needs its `hd.sum` manifest
   * line, and every selected version its tree line, as in `hd check`
   * (cli.dep.missing-sum). `record`: a missing line gets its hash, as in
   * `hd add` and `hd fetch`.
   */
  readonly sums: "verify" | "record";
  /** Called before each fetch, with the version it fetches. */
  readonly fetching?: (version: string) => void;
}

/** The selected package graph, and what `hd.sum` holds for it. */
export interface Resolution {
  /** The graph of the package the command works on. */
  readonly graph: PackageDependencies;
  /** The `hd.sum` lines of the whole selection: existing ones, or new hashes. */
  readonly sums: ReadonlyMap<string, string>;
  readonly problems: readonly DependencyProblem[];
}

/** A package of the graph: a local package (a member), or a fetched version. */
interface PackageNode {
  readonly id: string;
  readonly directory: string;
  readonly manifest: Manifest;
  /** A fetched version's host path and version; absent for a local package. */
  readonly fetched?: { readonly host: HostPath; readonly version: Version; readonly hash: string };
}

/** One checked requirement of a package, with the local line it is reported at. */
interface Edge {
  readonly entry: DependencyEntry;
  readonly requirement: Requirement;
  /** The line of a local manifest that reached it. */
  readonly at: Place;
}

/** A host-path requirement on its way through selection. */
interface HostEdge {
  readonly host: HostPath;
  readonly version: Version;
  readonly at: Place;
}

type Problem = (at: Place, code: string | null, message: string) => void;

const lineKey = (host: string, version: Version): string =>
  `${host}\0${compatibilityLine(version)}`;

const posix = (path: string): string => path.split(sep).join("/");

/** A workspace manifest's directory and the member directories it lists. */
interface Workspace {
  readonly root: string;
  readonly members: readonly string[];
}

async function enclosingWorkspace(directory: string): Promise<Workspace | undefined> {
  for (let above = dirname(directory); ; above = dirname(above)) {
    const path = join(above, "hd.toml");
    if (existsSync(path)) {
      const read = readManifest(await readFile(path, "utf8"));
      if ("manifest" in read && read.manifest.workspace) {
        const members = read.manifest.members.map((member) => resolve(above, member));
        return members.some((member) => posix(member) === posix(directory))
          ? { root: above, members }
          : undefined;
      }
    }
    if (dirname(above) === above) return undefined;
  }
}

/**
 * The nearest workspace manifest's directory above `directory` that lists
 * it as a member (spec/cli/command-line.md#r-cli.mode.member), or undefined.
 */
export async function workspaceRoot(directory: string): Promise<string | undefined> {
  return (await enclosingWorkspace(directory))?.root;
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
 * manifest is `manifest`, against the `hd.sum` entries `sums`. In a
 * workspace, selection starts at every member, and `manifest` stands for the
 * root's own `hd.toml`, which a dependency command may not have written yet.
 */
export async function resolveDependencies(
  root: string,
  manifest: Manifest,
  sums: SumEntries,
  options: ResolveOptions,
): Promise<Resolution> {
  const problems: DependencyProblem[] = [];
  const problem: Problem = (at, code, message) => {
    problems.push({ ...at, code, message });
  };
  const failed = (): Resolution => ({ graph: emptyGraph(), sums: new Map(), problems });

  // The members: the root, and the other members of its workspace.
  const rootNode: PackageNode = { id: posix(root), directory: root, manifest };
  const members = new Map<string, PackageNode>([[rootNode.id, rootNode]]);
  const workspace = await enclosingWorkspace(root);
  for (const directory of workspace?.members ?? []) {
    if (members.has(posix(directory))) continue;
    const read = await readPackageManifest(directory);
    if (typeof read === "string") {
      problem({ directory: workspace!.root, line: 1 }, "invalid-requirement", read);
      continue;
    }
    members.set(posix(directory), { id: posix(directory), directory, manifest: read });
  }
  const isMember = (node: PackageNode): boolean => members.get(node.id) === node;

  // The requirements of each package, checked. The members' dev
  // dependencies are read, and no other package's (module.select.dev-dependencies).
  const edges = new Map<PackageNode, Edge[]>();
  const check = (node: PackageNode, at: (entry: DependencyEntry) => Place): Edge[] => {
    const owner = isMember(node) ? "" : ` in ${shownNode(node)}`;
    const checked: Edge[] = [];
    const names = new Map<string, string>();
    const lines = new Map<string, string>();
    for (const entry of node.manifest.dependencies) {
      if (entry.dev && !isMember(node)) continue;
      const place = at(entry);
      const fail = (message: string): void =>
        problem(place, "invalid-requirement", `${message}${owner}`);
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
      checked.push({ entry, requirement, at: place });
    }
    return checked;
  };

  // A member's path requirement names another member of its workspace
  // (spec/lang/10-modules.md#r-module.workspace.path-requirement).
  for (const node of members.values()) {
    const nodeEdges = check(node, (entry) => ({ directory: node.directory, line: entry.line }));
    edges.set(node, nodeEdges);
    for (const edge of nodeEdges) {
      if (edge.requirement.kind !== "path") continue;
      if (!members.has(posix(resolve(node.directory, edge.requirement.path))))
        problem(
          edge.at,
          "invalid-requirement",
          `${edge.entry.key}: a path requirement names another member of the package's workspace, and ${edge.requirement.path} is not one; list both directories in the members of a workspace hd.toml`,
        );
    }
  }
  if (problems.length > 0) return failed();

  const hostEdges = (node: PackageNode): HostEdge[] =>
    (edges.get(node) ?? []).flatMap(({ requirement, at }) =>
      requirement.kind === "host"
        ? [{ host: requirement.host, version: requirement.version, at }]
        : [],
    );
  const start = [...members.values()].flatMap(hostEdges);
  // Selection reads the manifest of every version a member requires, so
  // without its manifest line `hd check` fails before it fetches
  // (cli.dep.missing-sum, cli.sum.manifest-line).
  if (options.sums === "verify") {
    for (const edge of start)
      if (!sums.has(manifestKey(edge.host.path, edge.version.text)))
        problem(edge.at, "missing-sum-entry", missingManifest(edge));
    if (problems.length > 0) return failed();
  }

  // Minimal version selection over every reached version
  // (spec/lang/10-modules.md#r-module.select.reach,
  // spec/lang/10-modules.md#r-module.select.largest).
  const fetcher = new Fetcher(options);
  const recorded = new Map<string, string>();
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
    // Each manifest selection reads is checked before it is read
    // (spec/cli/command-line.md#r-cli.dep.verify.manifest).
    const lineOfManifest = manifestKey(edge.host.path, edge.version.text);
    const expected = sums.get(lineOfManifest);
    if (expected === undefined && options.sums === "verify") {
      problem(edge.at, "missing-sum-entry", missingManifest(edge));
      continue;
    }
    const node = await fetcher.node(edge, problem);
    if (!node) continue;
    reached.set(key, node);
    const hash = await manifestHash(node.directory);
    if (expected !== undefined && expected !== hash) {
      problem(edge.at, "sum-mismatch", mismatch(lineOfManifest, "manifest", hash, expected));
      continue;
    }
    recorded.set(lineOfManifest, hash);
    const nodeEdges = check(node, () => edge.at).flatMap((checked): Edge[] => {
      if (checked.requirement.kind === "host") return [checked];
      // A fetched version's path requirement is a dependency requirement on
      // its version, by the host path it was fetched from
      // (module.workspace.path-version.fetched-version).
      const requirement = fetchedPathRequirement(node, checked);
      if (typeof requirement === "string") {
        problem(edge.at, "invalid-requirement", requirement);
        return [];
      }
      return [{ ...checked, requirement }];
    });
    edges.set(node, nodeEdges);
    queue.push(...hostEdges(node));
  }
  if (problems.length > 0) return failed();

  // Each selected version's tree line: kept, recorded, or a mismatch
  // (spec/cli/command-line.md#r-cli.dep.verify, cli.sum.keep).
  const chosen = new Map<string, PackageNode>();
  for (const [line, edge] of selected) {
    const key = sumKey(edge.host.path, edge.version.text);
    const node = reached.get(key)!;
    chosen.set(line, node);
    const hash = node.fetched!.hash;
    const entry = sums.get(key);
    if (entry === undefined) {
      if (options.sums === "verify") problem(edge.at, "missing-sum-entry", missingSum(edge));
      else recorded.set(key, hash);
    } else if (entry !== hash) problem(edge.at, "sum-mismatch", mismatch(key, "tree", hash, entry));
    else recorded.set(key, entry);
  }
  if (problems.length > 0) return failed();

  // A package that something depends on needs a library
  // (spec/lang/10-modules.md#r-module.path.no-lib-dependency).
  const target = (from: PackageNode, edge: Edge): PackageNode | undefined => {
    const { requirement } = edge;
    if (requirement.kind === "host")
      return chosen.get(lineKey(requirement.host.path, requirement.version));
    return members.get(posix(resolve(from.directory, requirement.path)));
  };
  const checkedLibrary = new Set<PackageNode>();
  for (const node of [...members.values(), ...chosen.values()])
    for (const edge of edges.get(node) ?? []) {
      const found = target(node, edge);
      if (!found || checkedLibrary.has(found)) continue;
      checkedLibrary.add(found);
      if (!existsSync(join(found.directory, "src", "lib.hd")))
        problem(
          edge.at,
          "invalid-requirement",
          `${shownNode(found)} has no library, src/lib.hd, so no package can depend on it`,
        );
    }
  if (problems.length > 0) return failed();
  return {
    graph: await linkGraph(rootNode, edges, target),
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

function missingSum(edge: HostEdge): string {
  return `${sumKey(edge.host.path, edge.version.text)} has no hd.sum entry, so its tree cannot be checked; run hd fetch to fetch it and record its hash, or hd add to change the requirement`;
}

function missingManifest(edge: HostEdge): string {
  return `${sumKey(edge.host.path, edge.version.text)} has no hd.sum line for its manifest, ${manifestKey(edge.host.path, edge.version.text)}, so selection cannot read it; run hd fetch to fetch it and record its hash, or hd add to change the requirement`;
}

function mismatch(key: string, what: string, hash: string, entry: string): string {
  return `${key}: the fetched ${what}'s hash ${hash} differs from its hd.sum entry ${entry}; hd never replaces an entry, so find out why the ${what} changed, as a moved tag, before you delete the entry and run hd fetch`;
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

  async node(edge: HostEdge, problem: Problem): Promise<PackageNode | undefined> {
    const { host, version, at } = edge;
    const key = sumKey(host.path, version.text);
    const cache = this.cache();
    const entry =
      (await cachedEntry(cache, host.path, version.text)) ??
      (await this.fetch(edge, cache, problem));
    if (!entry) return undefined;
    const manifest = await readPackageManifest(entry.directory);
    if (typeof manifest === "string") {
      problem(at, "invalid-requirement", `${key} is no package: ${manifest}`);
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
    problem: Problem,
  ): Promise<{ directory: string; hash: string } | undefined> {
    const { host, version, at } = edge;
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
            at,
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
          at,
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
        at,
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
    problem: Problem,
  ): Promise<{ directory: string; hash: string } | undefined> {
    const { host, version, at } = edge;
    if (!existsSync(join(directory, "hd.toml"))) {
      const pseudo = pseudoCommit(version);
      const where = pseudo ? `the commit ${pseudo.hash}` : `the tag ${tagName(host, version)}`;
      problem(
        at,
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

/**
 * The linker's view of the root's part of the selected graph
 * (src/package.ts): the packages its requirements reach, the root's dev
 * dependencies included and no other package's.
 */
async function linkGraph(
  rootNode: PackageNode,
  edges: ReadonlyMap<PackageNode, readonly Edge[]>,
  target: (from: PackageNode, edge: Edge) => PackageNode | undefined,
): Promise<PackageDependencies> {
  const linked = (node: PackageNode): readonly Edge[] =>
    (edges.get(node) ?? []).filter((edge) => node === rootNode || !edge.entry.dev);
  const shown = new Map<PackageNode, string>();
  for (const edge of linked(rootNode)) {
    const node = target(rootNode, edge);
    if (node) shown.set(node, `dep.${sourceName(edge.entry.key)}`);
  }
  const packages: Record<string, DependencyPackage> = {};
  const pending = linked(rootNode).flatMap((edge) => target(rootNode, edge) ?? []);
  const seen = new Set<PackageNode>([rootNode]);
  while (pending.length > 0) {
    const node = pending.shift()!;
    if (seen.has(node)) continue;
    seen.add(node);
    const dependencies: Record<string, string> = {};
    for (const edge of linked(node)) {
      const found = target(node, edge);
      if (!found) continue;
      dependencies[sourceName(edge.entry.key)] = found.id;
      pending.push(found);
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
  for (const edge of linked(rootNode)) {
    const node = target(rootNode, edge);
    if (node)
      (edge.entry.dev ? devDependencies : dependencies)[sourceName(edge.entry.key)] = node.id;
  }
  return { dependencies, devDependencies, packages };
}
