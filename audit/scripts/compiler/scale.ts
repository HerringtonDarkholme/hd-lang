// Generates large programs and times each stage in-process (parse, check,
// emit WAT, Binaryen assemble), cold and after a one-line edit.
// Usage: node --experimental-strip-types scale.ts OUT_DIR N1,N2,...
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

interface Stages {
  parse: (source: string) => { program?: unknown; diagnostics: readonly unknown[] };
  check: (program: unknown) => { program?: unknown; diagnostics: readonly unknown[] };
  emitWat: (program: unknown) => string;
  assembleWat: (wat: string) => { bytes: Uint8Array };
}

const stages: Stages = {
  ...((await import(resolve("src/parser/index.ts"))) as Pick<Stages, "parse">),
  ...((await import(resolve("src/checker/index.ts"))) as Pick<Stages, "check">),
  ...((await import(resolve("src/emitter/index.ts"))) as Pick<Stages, "emitWat">),
  ...((await import(resolve("src/wasm.ts"))) as Pick<Stages, "assembleWat">),
};

function unit(index: number, bump: number): string {
  return [
    `data P${index}:`,
    "    x: i32",
    "    y: i32 = 2",
    "",
    `fn g${index}(a: i32, b: i32 = 1, rest: i32...) -> i32:`,
    `    p := P${index} { x: a + ${bump} }`,
    `    q := P${index} { ...p, x: b }`,
    "    let total: i32 = 0",
    "    for v in rest:",
    "        total = total + v",
    "    doubled := [for v in rest if v > 1 => v * 2]",
    '    _ := "v=${q.x}"',
    "    if total > 10:",
    "        total + q.y + doubled.len()",
    "    else:",
    `        ${index === 0 ? "0" : `g${index - 1}(total, rest=[1, 2])`}`,
    "",
  ].join("\n");
}

function program(count: number, editAt: number): string {
  const units = Array.from({ length: count }, (_, index) => unit(index, index === editAt ? 7 : 3));
  return `${units.join("\n")}\nfn main() -> i32:\n    g${count - 1}(1, 2, 3, 4)\n`;
}

function time<T>(action: () => T): [T, number] {
  const start = performance.now();
  const value = action();
  return [value, performance.now() - start];
}

function measure(source: string): number[] {
  const [parsed, parseMs] = time(() => stages.parse(source));
  if (!parsed.program) throw new Error(JSON.stringify(parsed.diagnostics.slice(0, 3)));
  const [checked, checkMs] = time(() => stages.check(parsed.program));
  if (!checked.program) throw new Error(JSON.stringify(checked.diagnostics.slice(0, 3)));
  const [wat, emitMs] = time(() => stages.emitWat(checked.program));
  const [, assembleMs] = time(() => stages.assembleWat(wat));
  return [parseMs, checkMs, emitMs, assembleMs];
}

const [outDir, countsText] = process.argv.slice(2);
if (outDir === undefined || countsText === undefined) throw new Error("usage: OUT_DIR N1,N2");
mkdirSync(outDir, { recursive: true });
measure(program(20, -1));
console.log("| units | source lines | run | parse ms | check ms | emit ms | assemble ms | total ms |");
console.log("|---:|---:|---|---:|---:|---:|---:|---:|");
for (const count of countsText.split(",").map(Number)) {
  const cold = program(count, -1);
  const edited = program(count, Math.floor(count / 2));
  writeFileSync(join(outDir, `scale-${count}.hd`), cold);
  for (const [label, source] of [["cold", cold], ["one-line edit", edited]] as const) {
    const ms = measure(source);
    const total = ms.reduce((sum, value) => sum + value, 0);
    console.log(
      `| ${count} | ${source.split("\n").length} | ${label} | ${ms.map((value) => value.toFixed(0)).join(" | ")} | ${total.toFixed(0)} |`,
    );
  }
}
