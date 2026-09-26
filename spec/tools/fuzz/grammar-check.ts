// Generator cross-check: small EBNF derivations from spec/02-grammar.md fed to
// the spec reference parser. No implementation is involved. A `syntax-error`
// on a derivation means the grammar, the layout rules, the reference parser,
// or this generator's layout rendering disagree. Every sample needs triage.
//
//   node --experimental-strip-types spec/tools/fuzz/grammar-check.ts [--seed S] [--cases N] [--show K]
import { parseSource } from "../../reference-parser/parser.ts";
import { Rng } from "./common.ts";
import { Generator, loadGrammar, render } from "./generate.ts";

let seed = "g1";
let cases = 4000;
let show = 30;
const args = process.argv.slice(2);
for (let index = 0; index < args.length; index += 2) {
  const value = args[index + 1] ?? "";
  if (args[index] === "--seed") seed = value;
  else if (args[index] === "--cases") cases = Number(value);
  else if (args[index] === "--show") show = Number(value);
  else throw new Error(`unknown option ${args[index]}`);
}

const generator = new Generator(loadGrammar());
const rejected = new Map<string, string[]>();
let syntaxErrors = 0;
for (let index = 0; index < cases; index += 1) {
  const rng = new Rng(`${seed}:${index}`);
  const source = render(generator.derive(rng, "source_file", 6 + rng.int(12)));
  const codes = parseSource(source).map(({ code }) => code);
  if (codes.includes("syntax-error")) syntaxErrors += 1;
  for (const code of new Set(codes)) {
    const list = rejected.get(code) ?? [];
    list.push(source);
    rejected.set(code, list);
  }
}
process.stdout.write(`seed=${seed} derivations=${cases} syntax-error=${syntaxErrors}\n`);
for (const [code, sources] of [...rejected].sort((left, right) => right[1].length - left[1].length))
  process.stdout.write(`  ${String(sources.length).padStart(5)}  ${code}\n`);
const smallest = [...new Set(rejected.get("syntax-error") ?? [])].sort(
  (left, right) => left.length - right.length,
);
for (const source of smallest.slice(0, show)) process.stdout.write(`----\n${source.trimEnd()}\n`);
