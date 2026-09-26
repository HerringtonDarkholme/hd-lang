// 5.6 runtime library inventory: list every function, import, global, and type
// in a module, and which ones Binaryen's remove-unused-module-elements drops.
// Run: node --experimental-strip-types audit/scripts/arch/rtlib-inventory.ts FILE.hd [--profile-cap NAME] [OUT.md]
import { readFileSync, writeFileSync } from "node:fs";

import binaryen from "binaryen";

import { WASM_FEATURES } from "../../../src/wasm.ts";
import { compilePhased, header, optimize } from "./bench-lib.ts";

interface Inventory {
  readonly functions: string[];
  readonly imports: string[];
  readonly globals: string[];
  readonly types: number;
  readonly exports: string[];
}

function inventory(module: binaryen.Module): Inventory {
  const functions: string[] = [];
  const imports: string[] = [];
  for (let index = 0; index < module.getNumFunctions(); index += 1) {
    const info = binaryen.getFunctionInfo(module.getFunctionByIndex(index));
    if (info.module) imports.push(`${info.module}.${info.base}`);
    else functions.push(info.name);
  }
  const globals: string[] = [];
  for (let index = 0; index < module.getNumGlobals(); index += 1)
    globals.push(binaryen.getGlobalInfo(module.getGlobalByIndex(index)).name);
  const exports: string[] = [];
  for (let index = 0; index < module.getNumExports(); index += 1)
    exports.push(binaryen.getExportInfo(module.getExportByIndex(index)).name);
  const types = [...module.emitText().matchAll(/^\s*\(type \$/gm)].length;
  return { functions, imports, globals, types, exports };
}

const args = process.argv.slice(2);
const file = args.shift();
if (!file) throw new Error("usage: rtlib-inventory.ts FILE.hd [--cap NAME] [OUT.md]");
let capabilities: string[] | undefined;
if (args[0] === "--cap") {
  args.shift();
  capabilities = [args.shift()!];
}
const out = args.shift();
const compiled = compilePhased(readFileSync(file, "utf8"), { hostCapabilities: capabilities });
const module = binaryen.parseText(compiled.wat);
module.setFeatures(WASM_FEATURES);
const before = inventory(module);
module.runPasses(["remove-unused-module-elements"]);
const after = inventory(module);
const removedBytes = module.emitBinary().length;
module.dispose();
const optimizedBytes = optimize(compiled.wat).length;
const unusedFunctions = before.functions.filter((name) => !after.functions.includes(name));
const unusedImports = before.imports.filter((name) => !after.imports.includes(name));
const unusedGlobals = before.globals.filter((name) => !after.globals.includes(name));
const lines = [
  header(`node --experimental-strip-types audit/scripts/arch/rtlib-inventory.ts ${file}`),
  `Source: \`${file}\``,
  "",
  "| item | emitted | kept after remove-unused-module-elements |",
  "| --- | --- | --- |",
  `| defined functions | ${before.functions.length} | ${after.functions.length} |`,
  `| imports | ${before.imports.length} | ${after.imports.length} |`,
  `| globals | ${before.globals.length} | ${after.globals.length} |`,
  `| type definitions (text) | ${before.types} | ${after.types} |`,
  `| exports | ${before.exports.length} | ${after.exports.length} |`,
  `| wasm bytes | ${compiled.bytes.length} | ${removedBytes} (binaryen -O2: ${optimizedBytes}) |`,
  "",
  `Exports: ${before.exports.join(", ")}`,
  "",
  `Imports: ${before.imports.join(", ")}`,
  "",
  `Unused imports: ${unusedImports.join(", ") || "none"}`,
  "",
  `Unused functions (${unusedFunctions.length}): ${unusedFunctions.join(", ")}`,
  "",
  `Unused globals (${unusedGlobals.length}): ${unusedGlobals.join(", ")}`,
  "",
  `All defined functions: ${before.functions.join(", ")}`,
];
if (out) writeFileSync(out, lines.join("\n") + "\n");
console.log(lines.join("\n"));
