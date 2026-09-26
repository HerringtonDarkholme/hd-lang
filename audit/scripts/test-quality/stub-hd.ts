// Stub hd implementation for harness-portability checks (audit step 1.6).
// `parse FILE` uses only the specification reference parser. Every other
// command reports a located `unsupported-feature` diagnostic and exits 1.
// Usage: node --experimental-strip-types audit/scripts/test-quality/stub-hd.ts <command> [--opt value]... FILE
//
// STUB_MODE=relative-hd is a second experiment: every command is delegated to
// bin/hd.js unchanged, except that the file path in its output is printed
// relative to the working directory. It isolates the runner's dependence on
// the exact PATH spelling in located diagnostics.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { relative, resolve } from "node:path";

import { parseSource } from "../../../spec/reference-parser/parser.ts";

function main(args: readonly string[]): number {
  const [command, ...rest] = args;
  const operands: string[] = [];
  for (let index = 0; index < rest.length; index += 1) {
    const value = rest[index]!;
    if (value.startsWith("--")) {
      // Options with a value (--entry, --profile, --scenario, --pending-function).
      if (value !== "--wat") index += 1;
      continue;
    }
    operands.push(value);
  }
  const file = operands[0];
  if (!command || !file || operands.length !== 1) {
    console.error("usage: stub-hd <command> [--option value]... FILE");
    return 2;
  }
  if (process.env.STUB_MODE === "relative-hd") {
    const hd = resolve(import.meta.dirname, "../../../bin/hd.js");
    const result = spawnSync(process.execPath, ["--experimental-strip-types", hd, ...args], { encoding: "utf8" });
    const shorten = (text: string): string => text.replaceAll(file, relative(process.cwd(), file));
    process.stdout.write(shorten(result.stdout));
    process.stderr.write(shorten(result.stderr));
    return result.status ?? 1;
  }
  if (command === "parse") {
    const diagnostics = parseSource(readFileSync(file, "utf8"));
    if (diagnostics.length === 0) {
      console.log(`${file}: ok`);
      return 0;
    }
    for (const diagnostic of diagnostics)
      console.error(`${file}:${diagnostic.line}:1: ${diagnostic.code}: reference parser rejection`);
    return 1;
  }
  console.error(`${file}:1:1: unsupported-feature: stub implements only 'parse', not '${command}'`);
  return 1;
}

process.exitCode = main(process.argv.slice(2));
