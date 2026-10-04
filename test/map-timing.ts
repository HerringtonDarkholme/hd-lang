// Timing probe for Job U (temporary): inserts and looks up 10,000 int keys.
import { instantiate } from "../src/compiler.ts";

const source = `pub fn main() -> void $ Console:
    let table: mut Map[i32, i32] = {}
    let index = 0
    while index < 10000:
        table[index] = index * 2 + 1
        index = index + 1
    let total = 0
    index = 0
    while index < 10000:
        total = total + table[index]
        index = index + 1
    println(total)
`;

const lines: string[] = [];
const { instance, compilation } = await instantiate(source, {
  console: (text) => lines.push(text),
});
const entry = compilation.hir.functions.find((fn) => fn.entry)!;
const run = instance.exports[entry.name] as (...args: unknown[]) => unknown;
const providers = entry.requirements.map((requirement) => ({ requirement }));
const samples: number[] = [];
for (let round = 0; round < 7; round++) {
  lines.length = 0;
  const start = performance.now();
  run(...providers);
  samples.push(performance.now() - start);
  if (lines.at(-1) !== "100000000") throw new Error(`wrong checksum: ${lines.join("|")}`);
}
samples.sort((a, b) => a - b);
console.log(`samples(ms): ${samples.map((s) => s.toFixed(1)).join(", ")}`);
console.log(`median(ms): ${samples[3]!.toFixed(1)}`);
