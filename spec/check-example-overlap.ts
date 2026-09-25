import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const stop = new Set(
  "fn let mut pub data enum trait impl type use as for in if else match while return break continue pass true false nil self Self where reified annotate shape and or not is string bool void i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 list map Result Ok Err Any".split(
    " ",
  ),
);
const identifierPattern = /[^\W\d]\w*/gu;

function identifiers(text: string): Set<string> {
  return new Set(
    [...text.matchAll(identifierPattern)]
      .map(([value]) => value)
      .filter((value) => value.length >= 3 && !stop.has(value)),
  );
}

function salientIdentifiers(text: string): Set<string> {
  const source = text.replaceAll(/"(?:\\.|[^"\\])*"/g, '""').replaceAll(/#.*$/gm, "");
  const declared = new Set(
    [
      ...source.matchAll(/\b(?:fn|data|enum|trait|impl|type)\s+(?:\[[^\]]+\]\s*)?([^\W\d]\w*)/gu),
    ].map((match) => match[1]!),
  );
  const members = new Set(
    [...source.matchAll(/\.([^\W\d]\w*)\s*(?:\[|\()/gu)].map((match) => match[1]!),
  );
  const associated = new Set(
    [...source.matchAll(/::([^\W\d]\w*)\s*\(/gu)].map((match) => match[1]!),
  );
  const capitals = new Set(
    [...source.matchAll(/\b[A-Z][A-Za-z0-9_]*\b/g)]
      .map(([value]) => value)
      .filter((value) => !stop.has(value)),
  );
  const all = identifiers(source);
  const result = new Set(
    [...declared, ...members, ...associated, ...capitals].filter((value) => all.has(value)),
  );
  return result.size > 0 ? result : all;
}

function textBlocks(text: string): string[] {
  return [...text.matchAll(/^```text\s*\n(.*?)^```/gms)].map((match) => match[1]!);
}

async function main(args: readonly string[]): Promise<number> {
  if (args.length !== 2) {
    console.error("usage: check-example-overlap.ts SPEC_DIR EXAMPLES_TSV");
    return 2;
  }
  const specificationDirectory = resolve(args[0]!);
  const examplesPath = resolve(args[1]!);
  const failures: string[] = [];
  const blockCache = new Map<string, string[]>();
  const fixtureCache = new Map<string, string>();
  const rows = (await readFile(examplesPath, "utf8")).split(/\r?\n/).slice(1).filter(Boolean);
  for (const row of rows) {
    const [specification = "", blockText = "", classification = "", fixtureText = ""] =
      row.split("\t");
    if (!new Set(["accept", "mixed"]).has(classification)) continue;
    let blocks = blockCache.get(specification);
    if (!blocks) {
      blocks = textBlocks(await readFile(resolve(specificationDirectory, specification), "utf8"));
      blockCache.set(specification, blocks);
    }
    const blockIds = salientIdentifiers(blocks[Number(blockText) - 1]!);
    if (blockIds.size === 0) continue;
    const fixtureIds = new Set<string>();
    for (const fixture of fixtureText.split("|")) {
      const path = resolve(specificationDirectory, "conformance", fixture);
      let source = fixtureCache.get(path);
      if (source === undefined) {
        source = await readFile(path, "utf8");
        fixtureCache.set(path, source);
      }
      for (const value of identifiers(source)) fixtureIds.add(value);
    }
    const overlap = [...blockIds].filter((value) => fixtureIds.has(value));
    const coverage = overlap.length / blockIds.size;
    if (coverage < 0.6) {
      const missing = [...blockIds]
        .filter((value) => !fixtureIds.has(value))
        .sort()
        .join(", ");
      failures.push(
        `${specification} block ${blockText} covers ${overlap.length}/${blockIds.size} salient identifiers (${Math.round(coverage * 100)}%) with ${fixtureText}; missing ${missing}`,
      );
    }
  }
  if (failures.length > 0) {
    console.error(
      `example inventory overlap failures:\n${failures.map((failure) => `- ${failure}`).join("\n")}`,
    );
    return 1;
  }
  console.log("example inventory overlap passed");
  return 0;
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url))
  process.exitCode = await main(process.argv.slice(2));
