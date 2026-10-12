import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { diagnosticCodePhases } from "../../spec/tools/diagnostic-codes.ts";
import {
  compileRequest,
  type CompilerExports,
  encodeRequest,
  GOAL,
} from "../playground/src/compiler-wire.ts";

const REPO_DIR = resolve(import.meta.dirname, "../..");
/** The compiler wasm the playground serves; a build output (`pnpm run build:web-compiler`). */
const COMPILER_WASM = join(REPO_DIR, "website/playground/assets/hd_web.wasm");
const FILE = "src/main.hd";

let compiler: CompilerExports | undefined;
let syntaxCodes: ReadonlySet<string> | undefined;
const cache = new Map<string, boolean>();

function load(): { exports: CompilerExports; syntax: ReadonlySet<string> } {
  if (compiler === undefined) {
    if (!existsSync(COMPILER_WASM))
      execFileSync(process.execPath, [join(REPO_DIR, "scripts/build-web-compiler.js")], {
        stdio: "inherit",
      });
    const module = new WebAssembly.Module(readFileSync(COMPILER_WASM));
    compiler = new WebAssembly.Instance(module, {}).exports as unknown as CompilerExports;
    const phases = diagnosticCodePhases(
      readFileSync(join(REPO_DIR, "spec/README.md"), "utf8"),
      readFileSync(join(REPO_DIR, "spec/conformance/cases.tsv"), "utf8"),
    );
    syntaxCodes = new Set(
      [...phases].filter(([, phase]) => phase === "parse").map(([code]) => code),
    );
  }
  return { exports: compiler, syntax: syntaxCodes! };
}

/**
 * Whether the Rust compiler accepts `code` as a complete source file as far as
 * syntax goes: its check reports no lexical or grammar diagnostic. Other
 * errors, such as names the block does not define, do not count. The site
 * asks the compiler wasm in this process; a CLI call per block would be slow.
 */
export function parsesAsHd(code: string): boolean {
  let parses = cache.get(code);
  if (parses === undefined) {
    const { exports, syntax } = load();
    const { json } = compileRequest(exports, encodeRequest({ [FILE]: code }, FILE, GOAL.check));
    parses = !(json.diagnostics ?? []).some(({ code: name }) => name !== null && syntax.has(name));
    cache.set(code, parses);
  }
  return parses;
}
