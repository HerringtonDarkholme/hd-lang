// dead-code: how program size grows with source size, and whether unused
// std code stays out of the program.
//
// - Bytes per 1,000 lines: the generated small (1k-line) and 10k-line
//   packages, each with an executable that calls into every module
//   (`shapeK`, `totalK`, `labelK`). The value is (size at 10k - size at 1k)
//   / (thousands of lines between them).
// - Unused std: a program that uses no std module (recursive fib, checked
//   with `panic`), and the same program with unused `use` declarations of
//   five std modules (fixtures/dead-code/). Both must be the same size, and the first program's
//   import and export names must name no std module (the file names of
//   spec/std/). Names are read with WebAssembly.Module.imports and
//   .exports, so that half needs a .wasm file; the size half does not.
// Targets (Pillar 3): ≤ 10 KB per 1,000 lines (this harness's proposal;
// the architecture names a budget without a number); no unused std.
// n/a: never; a program that fails to build fails its line.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { buildFixture, buildProgram, howNote, type Program } from "../lib/artifact.ts";
import { PACKAGE } from "../lib/fixtures.ts";
import { generatePackage, reachEveryModule, SIZES, type GeneratedPackage } from "../lib/gen.ts";
import type { MetricContext, TargetResult } from "../lib/metric.ts";
import { errorText, failed, formatValue, judge, judgeBool, type Metric } from "../lib/metric.ts";

const NAME = "dead-code";
const LIMIT_PER_1000 = 10 * 1024;
const TIMEOUT_MS = 300_000;

/**
 * Std module names that mark std code when a Wasm name holds one as a
 * word: the file names of spec/std/, less the words a runtime uses for its
 * own helpers, such as `host_string_new` or `text`. `std` itself counts.
 */
export function stdModules(repoRoot: string): string[] {
  const common = new Set([
    "cli",
    "cmp",
    "error",
    "format",
    "hash",
    "host",
    "iter",
    "num",
    "ops",
    "option",
    "path",
    "result",
    "text",
  ]);
  return [
    "std",
    ...readdirSync(join(repoRoot, "spec", "std"))
      .filter((file) => file.endsWith(".md") && file !== "README.md")
      .map((file) => file.slice(0, -3))
      .filter((module) => !common.has(module)),
  ];
}

/** The words of a Wasm name: split at non-letters and at lower-to-upper case changes. */
const words = (name: string): string[] =>
  name
    .replaceAll(/([a-z])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z]+/)
    .filter(Boolean);

/** The names that hold a std module name as a word. */
export function stdNamesIn(names: readonly string[], modules: readonly string[]): string[] {
  return names.filter((name) => words(name).some((word) => modules.includes(word)));
}

function wasmNames(path: string): string[] {
  const module = new WebAssembly.Module(readFileSync(path));
  return [
    ...WebAssembly.Module.imports(module).map((entry) => `${entry.module}.${entry.name}`),
    ...WebAssembly.Module.exports(module).map((entry) => entry.name),
  ];
}

async function bytesPerThousand(context: MetricContext): Promise<TargetResult> {
  const label = "Wasm bytes per 1,000 lines";
  const target = `≤ ${formatValue(LIMIT_PER_1000, "bytes")}`;
  try {
    const built: { pkg: GeneratedPackage; program: Program }[] = [];
    for (const size of ["small", "10k"] as const) {
      const pkg = generatePackage({ seed: "dead-code", lines: SIZES[size], name: PACKAGE });
      const files = new Map(pkg.files);
      files.set("src/main.hd", reachEveryModule(pkg));
      built.push({
        pkg,
        program: await buildProgram(context.hd, files, `dead-${size}`, TIMEOUT_MS),
      });
    }
    const [small, large] = built as [(typeof built)[0], (typeof built)[0]];
    if (small.program.bytes === undefined || large.program.bytes === undefined) {
      throw new Error(`the build wrote no file to measure (${howNote(large.program)})`);
    }
    const perThousand =
      ((large.program.bytes - small.program.bytes) / (large.pkg.lines - small.pkg.lines)) * 1000;
    return judge(
      NAME,
      label,
      perThousand,
      LIMIT_PER_1000,
      "bytes",
      "at-most",
      `${small.program.bytes} B at ${small.pkg.lines} lines, ${large.program.bytes} B at ${large.pkg.lines} lines; ${howNote(large.program)}`,
    );
  } catch (error) {
    return failed(NAME, label, target, errorText(error));
  }
}

async function unusedStd(context: MetricContext): Promise<TargetResult> {
  const label = "no unused std in the program";
  const target = "none";
  try {
    const bare = await buildFixture(context.hd, "dead-code/std-free");
    const withUses = await buildFixture(context.hd, "dead-code/unused-uses");
    if (bare.bytes === undefined || withUses.bytes === undefined) {
      throw new Error(`the build wrote no file to measure (${howNote(bare)})`);
    }
    const problems: string[] = [];
    if (withUses.bytes !== bare.bytes) {
      problems.push(
        `unused uses add ${withUses.bytes - bare.bytes} B (${bare.bytes} B without them)`,
      );
    }
    let checked = "sizes only (no .wasm file)";
    if (bare.how === "wasm" && bare.artifact) {
      const named = stdNamesIn(wasmNames(bare.artifact), stdModules(context.repoRoot));
      if (named.length) {
        problems.push(`std names in the module: ${named.slice(0, 5).join(", ")}`);
      }
      checked = "sizes, and import and export names";
    }
    return judgeBool(
      NAME,
      label,
      problems.length === 0,
      target,
      problems.length === 0 ? "none" : `${problems.length} found`,
      `${problems.length ? `${problems.join("; ")}; ` : `${bare.bytes} B either way; `}checked ${checked}`,
    );
  } catch (error) {
    return failed(NAME, label, target, errorText(error));
  }
}

export const deadCode: Metric = {
  name: NAME,
  pillar: 3,
  summary: "Wasm bytes per 1,000 generated lines; no unused std in a std-free program",
  async run(context) {
    context.log(`${NAME}: builds of the 1k and 10k packages, and a std-free program`);
    return [await bytesPerThousand(context), await unusedStd(context)];
  },
};
