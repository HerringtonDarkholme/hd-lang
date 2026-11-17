// disk: what one worktree costs on disk: its build artifacts. The `hd`
// toolchain is installed once per machine, not per worktree, so its size is
// a second line with no target (owner, 2026-11-13, #1770).
//
// Artifacts: a fresh copy of the small package, with the executable
// src/main.hd, gets `hd check`, `hd test` and `hd build` with an empty
// HD_CACHE; the artifacts are the files those runs add inside the package.
// HD_CACHE is shared by every package of the user (cli.cache.shared), so its
// size is in the note but not counted.
// Toolchain: when a script that the hd command's program runs (an argument
// that is an existing file) lies in a Node package with a node_modules
// folder, node_modules is the toolchain, and the line says so. A native
// program is never run from node_modules, even when it sits in a checkout
// that has one. Otherwise the toolchain is the files the command names: its program,
// found on PATH, and any argument that is an existing file.
// Sizes add up file sizes; a hard-linked file counts once, and symbolic
// links are not followed.
// Target (Pillar 2): ≤ 10 MB of artifacts. No n/a case.

import { existsSync, lstatSync, realpathSync } from "node:fs";
import { delimiter, dirname, isAbsolute, join } from "node:path";

import { freshCache, runHd, runProblem } from "../lib/hd.ts";
import { failed, formatValue, judge, type Metric, type TargetResult } from "../lib/metric.ts";
import { listTree, treeBytes } from "../lib/tmp.ts";
import { addExecutable, materialize } from "../lib/gen.ts";

const NAME = "disk";
const LIMIT = 10 * 1024 * 1024;
const TIMEOUT_MS = 120_000;

export type Toolchain =
  | { readonly kind: "node_modules"; readonly path: string }
  | { readonly kind: "files"; readonly paths: readonly string[] };

/** The program on PATH, or undefined. */
function onPath(program: string): string | undefined {
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    const candidate = join(dir, program);
    if (dir && existsSync(candidate) && lstatSync(candidate).isFile()) {
      return candidate;
    }
  }
  return undefined;
}

/** The node_modules of the nearest Node package above `file`, if it has one. */
function nodeModulesAbove(file: string): string | undefined {
  for (let dir = dirname(file); ; dir = dirname(dir)) {
    if (existsSync(join(dir, "package.json"))) {
      return existsSync(join(dir, "node_modules")) ? join(dir, "node_modules") : undefined;
    }
    if (dirname(dir) === dir) {
      return undefined;
    }
  }
}

/** What the hd command runs from: a node_modules folder, or the files it names. */
export function toolchainOf(argv: readonly string[]): Toolchain {
  const files = argv
    .filter((word) => isAbsolute(word) && existsSync(word) && lstatSync(word).isFile())
    .map((word) => realpathSync(word));
  const program = isAbsolute(argv[0]!) ? argv[0] : onPath(argv[0]!);
  const native = program && existsSync(program) ? realpathSync(program) : undefined;
  for (const file of files) {
    if (file === native) {
      continue;
    }
    const modules = nodeModulesAbove(file);
    if (modules) {
      return { kind: "node_modules", path: modules };
    }
  }
  const paths = new Set(files);
  if (program && existsSync(program)) {
    paths.add(realpathSync(program));
  }
  return { kind: "files", paths: [...paths] };
}

/** What one worktree adds on disk, apart from the toolchain shared per machine. */
export interface DiskSizes {
  readonly artifacts: number;
  readonly artifactFiles: number;
  readonly toolchain: Toolchain;
  readonly toolchainBytes: number;
  readonly cacheBytes: number;
}

const ARTIFACTS = "artifacts per worktree";

/**
 * The two lines: the artifacts, judged against the per-worktree target, and
 * the toolchain's size, recorded with no target.
 */
export function diskLines(sizes: DiskSizes): TargetResult[] {
  const { toolchain } = sizes;
  const where =
    toolchain.kind === "node_modules"
      ? `node_modules is the toolchain (${toolchain.path})`
      : `toolchain files: ${toolchain.paths.join(", ") || "none found"}`;
  const note = [
    `${sizes.artifactFiles} files`,
    `HD_CACHE ${formatValue(sizes.cacheBytes, "MB")}, shared per user, not counted`,
  ].join("; ");
  return [
    judge(NAME, ARTIFACTS, sizes.artifacts, LIMIT, "MB", "at-most", note),
    {
      metric: NAME,
      name:
        toolchain.kind === "node_modules"
          ? "toolchain (node_modules), once per machine"
          : "toolchain, once per machine",
      value: formatValue(sizes.toolchainBytes, "MB"),
      target: "recorded, no target",
      status: "pass",
      note: where,
    },
  ];
}

export const disk: Metric = {
  name: NAME,
  pillar: 2,
  summary: "build artifacts of the small package per worktree; the toolchain's size apart",
  async run(context) {
    const { dir } = materialize("small", NAME);
    addExecutable(dir);
    const source = new Set(listTree(dir));
    const env = freshCache();
    const toolchain = toolchainOf(context.hd.argv);
    for (const command of ["check", "test", "build"]) {
      const result = await runHd(context.hd, [command], { cwd: dir, env, timeoutMs: TIMEOUT_MS });
      const problem = runProblem(result, TIMEOUT_MS, `hd ${command}`);
      if (problem) {
        return [failed(NAME, ARTIFACTS, `≤ ${formatValue(LIMIT, "MB")}`, problem)];
      }
    }
    const added = listTree(dir).filter((path) => !source.has(path));
    const artifacts = added.reduce((sum, path) => sum + lstatSync(join(dir, path)).size, 0);
    const toolchainBytes =
      toolchain.kind === "node_modules"
        ? treeBytes(toolchain.path).bytes
        : toolchain.paths.reduce((sum, path) => sum + treeBytes(path).bytes, 0);
    const cacheBytes = treeBytes(env.HD_CACHE!).bytes;
    return diskLines({
      artifacts,
      artifactFiles: added.length,
      toolchain,
      toolchainBytes,
      cacheBytes,
    });
  },
};
