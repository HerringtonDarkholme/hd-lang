// Style lint for the specification (see spec/STYLE.md).
//
//   node --experimental-strip-types spec/check-spec-style.ts SPEC_DIR [--all]
//
// Warnings, which never fail the check: a paragraph, list item, or quote over
// 90 words, and a sentence over 35 words, in a chapter: a numbered language
// chapter, or a stdlib chapter in spec/std/. A chapter that carries rule IDs
// has been restyled, so its warnings are listed one per line; other chapters
// get a one-line count unless --all is given.
//
// Failures: a malformed or misplaced rule ID marker, a duplicate rule ID, an
// ID without its chapter's prefix, a marker outside a chapter, and an
// error-example marker naming an unknown code. Retired IDs are not listed
// anywhere, so the rule inventory diff (spec/tools/rule-inventory.ts) catches
// a reused one by searching the history.
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  blocks,
  CHAPTER_PREFIXES,
  errorMarkerCodes,
  CLI_DIRECTORY,
  LANG_DIRECTORY,
  STD_DIRECTORY,
  knownCodes,
  paragraphs,
  readable,
  ruleMarkers,
  sentences,
  wordCount,
} from "./tools/spec-prose.ts";

export const PARAGRAPH_WORDS = 90;
export const SENTENCE_WORDS = 35;

/**
 * Checks the scanner on known input before trusting it with the chapters, so
 * a regression that stops markers from being found cannot pass silently.
 */
function selfCheck(): string[] {
  const sample = [
    "1. r[data.good.one] A rule. Error: `x`.",
    "2. r[Data.Bad] A malformed ID.",
    "A paragraph that mentions r[data.stray] mid-sentence, and `r[data.quoted]` in code.",
    "",
    "| Rule | Text |",
    "| --- | --- |",
    "| r[data.good.cell] Cell | text |",
    "",
    "```text",
    "1. r[data.in.fence] not a marker",
    "```",
  ].join("\n");
  const { markers, problems } = ruleMarkers(blocks(sample));
  const found = markers.map((marker) => marker.id).join(",");
  const failures: string[] = [];
  if (found !== "data.good.one,data.good.cell") failures.push(`self-check found markers ${found}`);
  const expected = ["malformed rule ID r[Data.Bad]", "r[data.stray] must open"];
  if (
    problems.length !== expected.length ||
    !expected.every((text, index) => problems[index]!.includes(text))
  )
    failures.push(`self-check problems: ${problems.join("; ")}`);
  const split = sentences("Use `a. B` here. Then stop.");
  if (split.length !== 2) failures.push(`self-check split ${JSON.stringify(split)}`);
  return failures;
}

async function main(args: readonly string[]): Promise<number> {
  const positional = args.filter((arg) => !arg.startsWith("--"));
  if (positional.length !== 1) {
    console.error("usage: check-spec-style.ts SPEC_DIR [--all]");
    return 2;
  }
  const all = args.includes("--all");
  const specDirectory = resolve(positional[0]!);
  const read = (name: string): Promise<string> => readFile(resolve(specDirectory, name), "utf8");
  const codes = knownCodes(
    await read("README.md"),
    await read(`${LANG_DIRECTORY}/06-control-flow.md`),
  );
  const markdown = async (directory: string): Promise<string[]> =>
    (await readdir(resolve(specDirectory, directory)).catch(() => []))
      .filter((name) => name.endsWith(".md"))
      .sort()
      .map((name) => (directory === "." ? name : `${directory}/${name}`));
  const names = [
    ...(await markdown(".")),
    ...(await markdown(LANG_DIRECTORY)),
    ...(await markdown(STD_DIRECTORY)),
    ...(await markdown(CLI_DIRECTORY)),
  ];
  const failures: string[] = selfCheck();
  const seen = new Map<string, string>();
  const summaries: string[] = [];
  const details: string[] = [];

  for (const name of names) {
    const text = await read(name);
    const parsed = blocks(text);
    const prefix = CHAPTER_PREFIXES[name];
    const { markers, problems } = ruleMarkers(parsed);
    for (const problem of problems) failures.push(`spec/${name}:${problem}`);
    for (const marker of markers) {
      const where = `spec/${name}:${marker.line}`;
      if (prefix === undefined) {
        failures.push(`${where}: rule ID ${marker.id} outside a chapter`);
        continue;
      }
      if (marker.id.split(".", 1)[0] !== prefix)
        failures.push(
          `${where}: rule ID ${marker.id} must start with this chapter's prefix ${prefix}.`,
        );
      const previous = seen.get(marker.id);
      if (previous) failures.push(`${where}: duplicate rule ID ${marker.id}, first at ${previous}`);
      else seen.set(marker.id, where);
    }
    if (prefix === undefined) continue;

    for (const block of parsed)
      if (block.kind === "code")
        for (const code of errorMarkerCodes(block.text))
          if (!codes.has(code))
            failures.push(`spec/${name}:${block.line}: error example names unknown code ${code}`);

    const warnings: string[] = [];
    for (const block of paragraphs(parsed)) {
      const words = wordCount(block.text);
      if (words > PARAGRAPH_WORDS)
        warnings.push(
          `spec/${name}:${block.line}: warning: paragraph has ${words} words (limit ${PARAGRAPH_WORDS})`,
        );
    }
    for (const block of parsed) {
      if (block.kind !== "paragraph" && block.kind !== "item" && block.kind !== "quote") continue;
      for (const sentence of sentences(block.text)) {
        const words = wordCount(sentence);
        if (words > SENTENCE_WORDS)
          warnings.push(
            `spec/${name}:${block.line}: warning: sentence has ${words} words (limit ${SENTENCE_WORDS}): ${readable(sentence).slice(0, 60)}...`,
          );
      }
    }
    if (warnings.length === 0) continue;
    if (all || markers.length > 0) details.push(...warnings);
    else summaries.push(`${name} ${warnings.length}`);
  }

  if (details.length > 0) console.log(details.join("\n"));
  if (summaries.length > 0)
    console.log(
      `style warnings in chapters not yet restyled (--all lists them): ${summaries.join(", ")}`,
    );
  if (failures.length > 0) {
    console.error(`specification style failures:\n${failures.map((f) => `- ${f}`).join("\n")}`);
    return 1;
  }
  console.log(`specification style passed (${seen.size} rule IDs)`);
  return 0;
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url))
  process.exitCode = await main(process.argv.slice(2));
