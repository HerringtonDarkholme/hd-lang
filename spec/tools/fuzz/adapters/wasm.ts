// Optional Wasm backend adapter. The core fuzzer never needs it.
//
// For an input the implementation's `check` accepts, it runs the
// implementation's `build FILE` command, locates the emitted `.wasm` (a path
// printed on stdout, or FILE with a `.wasm` extension), and validates it with
// external tools: Binaryen `wasm-opt` (default `node_modules/.bin/wasm-opt`,
// override with HD_FUZZ_WASM_OPT) and, if on PATH, `wasm-tools validate`.
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync } from "node:fs";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";

import { type CommandResult, invoke, messageLabel, repoRoot } from "../common.ts";

export interface WasmVerdict {
  /** "valid", "build-failed", "no-module", or "invalid:<tool>". */
  readonly status: string;
  readonly detail: string;
}

function which(tool: string): string | undefined {
  const found = spawnSync("sh", ["-c", `command -v ${tool}`], { encoding: "utf8" });
  return found.status === 0 ? found.stdout.trim() : undefined;
}

const wasmOpt = process.env.HD_FUZZ_WASM_OPT ?? resolve(repoRoot, "node_modules/.bin/wasm-opt");
const wasmTools = which("wasm-tools");

export function wasmToolsAvailable(): string[] {
  return [existsSync(wasmOpt) ? "wasm-opt" : "", wasmTools ? "wasm-tools" : ""].filter(Boolean);
}

function emittedModule(result: CommandResult, path: string): string | undefined {
  for (const line of result.stdout.split("\n")) {
    const candidate = line.trim();
    if (candidate.endsWith(".wasm")) {
      const absolute = resolve(dirname(path), candidate);
      if (existsSync(absolute)) return absolute;
    }
  }
  const sibling = path.replace(/\.hd$/, ".wasm");
  return existsSync(sibling) ? sibling : undefined;
}

function firstLine(text: string): string {
  return (text.split("\n").find((line) => line.trim() && !line.startsWith("warning:")) ?? "")
    .trim()
    .slice(0, 120);
}

/**
 * `build` may write its module into the working directory, so it runs inside
 * the case's own scratch directory. Command words that name files relative to
 * the repository root (such as `bin/hd.js`) are made absolute first.
 */
function absoluteCommand(command: readonly string[]): string[] {
  return command.map((word) =>
    !word.startsWith("-") && !isAbsolute(word) && existsSync(resolve(repoRoot, word))
      ? resolve(repoRoot, word)
      : word,
  );
}

export async function validateBuild(
  command: readonly string[],
  path: string,
  timeoutMs: number,
): Promise<WasmVerdict> {
  // Build a private copy in a fresh directory: identical inputs from different
  // cases share a case file, and concurrent builds must not share an output.
  const directory = mkdtempSync(join(dirname(path), "build-"));
  const copy = join(directory, basename(path));
  copyFileSync(path, copy);
  path = copy;
  const built = await invoke(absoluteCommand(command), "build", path, [], timeoutMs, directory);
  if (built.timedOut || built.signal || built.code !== 0)
    return {
      detail: messageLabel(`${built.stdout}\n${built.stderr}`, path),
      status: built.timedOut ? "build-timeout" : "build-failed",
    };
  const module = emittedModule(built, path);
  if (!module) return { detail: firstLine(built.stdout), status: "no-module" };
  if (existsSync(wasmOpt)) {
    const checked = spawnSync(wasmOpt, ["--all-features", module], {
      encoding: "utf8",
      timeout: timeoutMs,
    });
    if (checked.status !== 0)
      return { detail: firstLine(checked.stderr || checked.stdout), status: "invalid:wasm-opt" };
  }
  if (wasmTools) {
    const checked = spawnSync(wasmTools, ["validate", "--features", "all", module], {
      encoding: "utf8",
      timeout: timeoutMs,
    });
    if (checked.status !== 0)
      return { detail: firstLine(checked.stderr), status: "invalid:wasm-tools" };
  }
  return { detail: "", status: "valid" };
}
