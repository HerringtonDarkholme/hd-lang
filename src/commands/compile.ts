// `hd parse` (`hd debug parse`), `hd check`, `hd debug hir`, and `hd build`:
// the commands that compile FILE without running it.

import { writeFile } from "node:fs/promises";
import { basename, extname, resolve } from "node:path";

import { analyze, compileToWasm, compileToWat } from "../compiler.ts";
import { DiagnosticError } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import type { CommandIo } from "./io.ts";
import {
  loadSource,
  placementOf,
  reportFailure,
  type PackageTree,
  type RuntimeProfileName,
  type SourceArgs,
  type TestLayout,
} from "./source.ts";

/** `hd parse FILE`: parses FILE and its `tests:` block, and prints `FILE: ok`. */
export async function parseCommand(args: SourceArgs, io: CommandIo): Promise<number> {
  const loaded = await loadSource(args, io, { linkTests: false });
  if (typeof loaded === "number") return loaded;
  try {
    const result = parse(loaded.source, loaded.parseOptions);
    if (!result.program) throw new DiagnosticError(result.diagnostics);
    io.out(`${args.file}: ok`);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}

export interface CheckArgs extends SourceArgs {
  /** `--tests`: also check the test cases and test-only code (Testing T42). */
  readonly tests: boolean;
  readonly profile?: RuntimeProfileName;
  readonly testLayout?: TestLayout;
  readonly packageTree?: PackageTree;
}

/** `hd check FILE`: type-checks FILE and prints its warnings, then `FILE: ok`. */
export async function checkCommand(args: CheckArgs, io: CommandIo): Promise<number> {
  const placement = await placementOf(args.file, args.packageTree, args.testLayout);
  const loaded = await loadSource(
    args,
    io,
    { profile: args.profile, testLayout: args.testLayout, linkTests: args.tests },
    placement,
  );
  if (typeof loaded === "number") return loaded;
  try {
    // `hd check` checks test code only with `--tests` (Testing T42).
    const result = analyze(loaded.source, {
      ...loaded.compileOptions,
      skipTestCode: !args.tests,
    });
    if (!result.hir) throw new DiagnosticError(result.diagnostics);
    for (const diagnostic of result.diagnostics) loaded.report(diagnostic);
    io.out(`${args.file}: ok`);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}

export interface HirArgs extends SourceArgs {
  readonly profile?: RuntimeProfileName;
}

/** `hd debug hir FILE`: prints FILE's checked HIR as JSON. */
export async function hirCommand(args: HirArgs, io: CommandIo): Promise<number> {
  const loaded = await loadSource(args, io, { profile: args.profile, linkTests: false });
  if (typeof loaded === "number") return loaded;
  try {
    const result = analyze(loaded.source, loaded.compileOptions);
    if (!result.hir) throw new DiagnosticError(result.diagnostics);
    io.out(JSON.stringify(result.hir, null, 2));
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}

export interface BuildArgs extends SourceArgs {
  /** `--wat`: print the WebAssembly text instead of writing `NAME.wasm`. */
  readonly wat: boolean;
  readonly profile?: RuntimeProfileName;
}

/**
 * `hd build FILE`: writes `NAME.wasm` to the current directory and prints its
 * path, or with `--wat` prints the WAT without assembling it.
 */
export async function buildCommand(args: BuildArgs, io: CommandIo): Promise<number> {
  const placement = await placementOf(args.file, undefined, undefined);
  const loaded = await loadSource(args, io, { profile: args.profile, linkTests: false }, placement);
  if (typeof loaded === "number") return loaded;
  try {
    if (args.wat) {
      io.out(compileToWat(loaded.source, loaded.compileOptions).wat);
      return 0;
    }
    const result = await compileToWasm(loaded.source, loaded.compileOptions);
    const output = resolve(`${basename(loaded.path, extname(loaded.path))}.wasm`);
    await writeFile(output, result.bytes);
    io.out(output);
    return 0;
  } catch (error) {
    return reportFailure(loaded, error);
  }
}
