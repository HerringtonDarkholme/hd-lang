// Generator cross-check: small EBNF derivations from spec/lang/02-grammar.md fed to
// an implementation's `parse` through the command contract. A rejected
// derivation means the grammar, the layout rules, the implementation's parser,
// or this generator's layout rendering disagree. Every sample needs triage.
//
//   node --experimental-strip-types spec/tools/fuzz/grammar-check.ts \
//     [--compiler "<cmd>"] [--seed S] [--cases N] [--show K] [--jobs J]
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { join } from "node:path";

import {
  classify,
  invoke,
  loadInventory,
  mapParallel,
  outcomeLabel,
  Rng,
  splitCommand,
} from "./common.ts";
import { Generator, loadGrammar, render } from "./generate.ts";

let compiler = splitCommand(
  process.env.HD_FUZZ_COMMAND ?? "node --experimental-strip-types bin/hd.js",
);
let seed = "g1";
let cases = 4000;
let show = 30;
let jobs = Number(process.env.HD_TEST_JOBS ?? Math.min(8, availableParallelism()));
const args = process.argv.slice(2);
for (let index = 0; index < args.length; index += 2) {
  const value = args[index + 1] ?? "";
  if (args[index] === "--compiler") compiler = splitCommand(value);
  else if (args[index] === "--seed") seed = value;
  else if (args[index] === "--cases") cases = Number(value);
  else if (args[index] === "--show") show = Number(value);
  else if (args[index] === "--jobs") jobs = Number(value);
  else throw new Error(`unknown option ${args[index]}`);
}
if (compiler.length === 0) throw new Error("compiler command must not be empty");

const inventory = loadInventory();
const generator = new Generator(loadGrammar());
const work = mkdtempSync(join(tmpdir(), "hd-grammar-check-"));
const rejected = new Map<string, string[]>();
try {
  const indices = Array.from({ length: cases }, (_, index) => index);
  await mapParallel(indices, jobs, async (index) => {
    const rng = new Rng(`${seed}:${index}`);
    const source = render(generator.derive(rng, "source_file", 6 + rng.int(12)));
    const path = join(work, `${index}.hd`);
    writeFileSync(path, source);
    const outcome = classify(
      await invoke(compiler, "parse", path, [], 10_000),
      "parse",
      path,
      inventory,
    );
    if (outcome.kind === "accept") return;
    const label = outcomeLabel(outcome);
    const list = rejected.get(label) ?? [];
    list.push(source);
    rejected.set(label, list);
  });
} finally {
  rmSync(work, { force: true, recursive: true });
}
const total = [...rejected.values()].reduce((sum, sources) => sum + sources.length, 0);
process.stdout.write(`seed=${seed} derivations=${cases} rejected=${total}\n`);
for (const [label, sources] of [...rejected].sort(
  (left, right) => right[1].length - left[1].length,
))
  process.stdout.write(`  ${String(sources.length).padStart(5)}  ${label}\n`);
const smallest = [...new Set([...rejected.values()].flat())].sort(
  (left, right) => left.length - right.length,
);
for (const source of smallest.slice(0, show)) process.stdout.write(`----\n${source.trimEnd()}\n`);
