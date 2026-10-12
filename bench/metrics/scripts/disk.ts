// disk: what one worktree costs on disk, build artifacts plus toolchain.
//
// Artifacts: a fresh copy of the small package, with the executable
// src/main.hd, gets `hd check`, `hd test` and `hd build` with an empty
// HD_CACHE; the artifacts are the files those runs add inside the package.
// HD_CACHE is shared by every package of the user (cli.cache.shared), so its
// size is in the note but not counted.
// Toolchain: when a file the hd command names lies in a Node package with a
// node_modules folder, node_modules is the toolchain, and the line says so.
// Otherwise the toolchain is the files the command names: its program,
// found on PATH, and any argument that is an existing file.
// Sizes add up file sizes; a hard-linked file counts once, and symbolic
// links are not followed.
// Target (Pillar 2): ≤ 10 MB. No n/a case.

import { existsSync, lstatSync, realpathSync } from "node:fs";
import { delimiter, dirname, isAbsolute, join } from "node:path";

import { addExecutable, freshCache, materialize, runProblem } from "../lib/fixture.ts";
import { runHd } from "../lib/hd.ts";
import { failed, formatValue, judge, type Metric } from "../lib/metric.ts";
import { listTree, treeBytes } from "../lib/tmp.ts";

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
    if (dir && existsSync(candidate) && lstatSync(candidate).isFile()) return candidate;
  }
  return undefined;
}

/** The node_modules of the nearest Node package above `file`, if it has one. */
function nodeModulesAbove(file: string): string | undefined {
  for (let dir = dirname(file); ; dir = dirname(dir)) {
    if (existsSync(join(dir, "package.json")))
      return existsSync(join(dir, "node_modules")) ? join(dir, "node_modules") : undefined;
    if (dirname(dir) === dir) return undefined;
  }
}

/** What the hd command runs from: a node_modules folder, or the files it names. */
export function toolchainOf(argv: readonly string[]): Toolchain {
  const files = argv
    .filter((word) => isAbsolute(word) && existsSync(word) && lstatSync(word).isFile())
    .map((word) => realpathSync(word));
  for (const file of files) {
    const modules = nodeModulesAbove(file);
    if (modules) return { kind: "node_modules", path: modules };
  }
  const program = isAbsolute(argv[0]!) ? argv[0] : onPath(argv[0]!);
  const paths = new Set(files);
  if (program && existsSync(program)) paths.add(realpathSync(program));
  return { kind: "files", paths: [...paths] };
}

export const disk: Metric = {
  name: NAME,
  pillar: 2,
  summary: "build artifacts of the small package plus the toolchain, per worktree",
  async run(context) {
    const { dir } = materialize("small", NAME);
    addExecutable(dir);
    const source = new Set(listTree(dir));
    const env = freshCache();
    const toolchain = toolchainOf(context.hd.argv);
    const label =
      toolchain.kind === "node_modules"
        ? "artifacts plus toolchain (node_modules) per worktree"
        : "artifacts plus toolchain per worktree";
    for (const command of ["check", "test", "build"]) {
      const result = await runHd(context.hd, [command], { cwd: dir, env, timeoutMs: TIMEOUT_MS });
      const problem = runProblem(result, TIMEOUT_MS, `hd ${command}`);
      if (problem) return [failed(NAME, label, "≤ 10.0 MB", problem)];
    }
    const added = listTree(dir).filter((path) => !source.has(path));
    const artifacts = added.reduce((sum, path) => sum + lstatSync(join(dir, path)).size, 0);
    const tools =
      toolchain.kind === "node_modules"
        ? treeBytes(toolchain.path).bytes
        : toolchain.paths.reduce((sum, path) => sum + treeBytes(path).bytes, 0);
    const cache = treeBytes(env.HD_CACHE!).bytes;
    const where =
      toolchain.kind === "node_modules"
        ? `node_modules is the toolchain (${toolchain.path})`
        : `toolchain files: ${toolchain.paths.join(", ") || "none found"}`;
    const note = [
      `artifacts ${formatValue(artifacts, "bytes")} in ${added.length} files`,
      `toolchain ${formatValue(tools, "MB")}; ${where}`,
      `HD_CACHE ${formatValue(cache, "MB")}, shared per user, not counted`,
    ].join("; ");
    return [judge(NAME, label, artifacts + tools, LIMIT, "MB", "at-most", note)];
  },
};
