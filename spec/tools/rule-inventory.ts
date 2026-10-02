// Rule inventory of a specification chapter, and a diff of two versions of one.
//
//   node --experimental-strip-types spec/tools/rule-inventory.ts CHAPTER
//   node --experimental-strip-types spec/tools/rule-inventory.ts --diff OLD NEW
//
// CHAPTER, OLD, and NEW are Markdown paths, or REV:PATH to read a committed
// version through `git show`, as in main:spec/lang/08-data-and-enums.md.
//
// The inventory lists every diagnostic code, every normative sentence, every
// code example, and every rule ID, with prose statistics. The diff is the
// review aid for a restyle (spec/STYLE.md, "Restyling A Chapter"): a rewrite
// must lose no diagnostic code, no example line, and no rule ID. Examples may
// be moved or split, and error examples added. A retired rule ID is simply
// gone, so the diff reports it as lost, and the change explains it. When OLD
// is REV:PATH, the diff also reports each added rule ID whose marker appears
// in REV's history of the chapters, numbered or in spec/std/: that ID was
// retired and must not be reused. Normative sentences cannot be
// matched mechanically, so the diff pairs each old sentence with its closest
// new one and lists weak pairs for a human to check.
//
// Options:
//   --json        print the data as JSON instead of a Markdown report
//   --out PATH    write the report to PATH instead of standard output
//   --all         with --diff, pair every prose sentence, not only normative ones
//
// Exit status: 0 when nothing is lost, 1 when a diff loses a code, an example
// line, or a rule ID, or reuses a rule ID, and 2 on a usage error. It imports only Node built-ins
// and spec/.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  blocks,
  type Block,
  cells,
  CHAPTER_HISTORY_PATHS,
  errorMarkerCodes,
  isErrorExample,
  knownCodes,
  paragraphs,
  display,
  readable,
  ruleMarkers,
  sentences,
  wordCount,
} from "./spec-prose.ts";

const specRoot = resolve(import.meta.dirname, "..");
const repoRoot = resolve(specRoot, "..");

interface Sentence {
  readonly line: number;
  readonly text: string;
}

interface Example {
  /** 1-based position among the chapter's code fences. */
  readonly index: number;
  readonly line: number;
  readonly info: string;
  readonly code: string;
  readonly error: boolean;
}

export interface Rule {
  readonly id: string;
  readonly line: number;
  readonly text: string;
  readonly codes: string[];
}

interface Stats {
  readonly words: number;
  readonly paragraphs: number;
  readonly averageParagraphWords: number;
  readonly longestParagraphWords: number;
  readonly paragraphsOver90: number;
  readonly sentences: number;
  readonly averageSentenceWords: number;
  readonly sentencesOver25: number;
  readonly sentencesOver35: number;
}

export interface Inventory {
  readonly source: string;
  readonly stats: Stats;
  /** Each code with the number of times the chapter names it. */
  readonly codes: Record<string, number>;
  readonly normative: Sentence[];
  /** Every prose sentence, normative or not. */
  readonly sentences: Sentence[];
  readonly examples: Example[];
  readonly rules: Rule[];
}

const NORMATIVE =
  /\b(?:must|may|should|shall|cannot|can only|never|only|requires?|required|diagnose[sd]?|invalid|rejected|not allowed|not part of the language)\b|\bis an? (?:[a-z-]+ )?error\b|\bare (?:[a-z-]+ )?errors\b|\bError: `/i;

function load(spec: string): string {
  if (existsSync(spec)) return readFileSync(spec, "utf8");
  const colon = spec.indexOf(":");
  if (colon > 0)
    return execFileSync("git", ["-C", repoRoot, "show", spec], {
      encoding: "utf8",
      maxBuffer: 1 << 26,
    });
  throw new Error(`cannot read ${spec}`);
}

/**
 * For OLD given as REV:PATH, a test of whether a rule ID's marker appears
 * anywhere in REV's history of the chapters, CHAPTER_HISTORY_PATHS: the
 * numbered language chapters, the stdlib chapters, and the CLI chapters, at
 * their current paths and at their paths before task #175.
 */
function historySearch(spec: string): ((id: string) => boolean) | undefined {
  const colon = spec.indexOf(":");
  if (existsSync(spec) || colon <= 0) return undefined;
  const revision = spec.slice(0, colon);
  const pickaxe = (id: string): string[] => [
    "-C",
    repoRoot,
    "log",
    "-1",
    "--format=%h",
    "-S",
    `r[${id}]`,
    revision,
    "--",
    ...CHAPTER_HISTORY_PATHS,
  ];
  return (id) => execFileSync("git", pickaxe(id), { encoding: "utf8" }).trim() !== "";
}

function proseOf(block: Block): string {
  return block.kind === "row" ? cells(block.text).join(" | ") : block.text;
}

function round(value: number): number {
  return Math.round(value * 10) / 10;
}

export function inventory(source: string, markdown: string, known: ReadonlySet<string>): Inventory {
  const parsed = blocks(markdown);
  const prose = parsed.filter((block) => block.kind !== "code" && block.kind !== "heading");
  const codes: Record<string, number> = {};
  const count = (code: string): void => {
    codes[code] = (codes[code] ?? 0) + 1;
  };
  const codesIn = (text: string): string[] =>
    [...text.matchAll(/`([a-z0-9-]+)`/g)]
      .map((match) => match[1]!)
      .filter((code) => known.has(code));
  const normative: Sentence[] = [];
  const all: Sentence[] = [];
  for (const block of prose) {
    for (const code of codesIn(block.text)) count(code);
    for (const sentence of sentences(proseOf(block))) {
      all.push({ line: block.line, text: sentence });
      if (NORMATIVE.test(readable(sentence)) || codesIn(sentence).length > 0)
        normative.push({ line: block.line, text: sentence });
    }
  }
  const examples: Example[] = [];
  for (const block of parsed) {
    if (block.kind !== "code") continue;
    for (const code of errorMarkerCodes(block.text)) count(code);
    examples.push({
      index: examples.length + 1,
      line: block.line,
      info: block.info,
      code: block.text,
      error: isErrorExample(block.text),
    });
  }
  const rules = ruleMarkers(parsed).markers.map((marker) => ({
    id: marker.id,
    line: marker.line,
    text: display(marker.text),
    codes: codesIn(marker.text),
  }));

  const counted = paragraphs(parsed).map((block) => wordCount(block.text));
  const sentenceWords = parsed
    .filter(
      (block) => block.kind === "paragraph" || block.kind === "item" || block.kind === "quote",
    )
    .flatMap((block) => sentences(block.text).map(wordCount));
  const words = prose.reduce((sum, block) => sum + wordCount(proseOf(block)), 0);
  const stats: Stats = {
    words,
    paragraphs: counted.length,
    averageParagraphWords: round(counted.reduce((a, b) => a + b, 0) / Math.max(1, counted.length)),
    longestParagraphWords: Math.max(0, ...counted),
    paragraphsOver90: counted.filter((n) => n > 90).length,
    sentences: sentenceWords.length,
    averageSentenceWords: round(
      sentenceWords.reduce((a, b) => a + b, 0) / Math.max(1, sentenceWords.length),
    ),
    sentencesOver25: sentenceWords.filter((n) => n > 25).length,
    sentencesOver35: sentenceWords.filter((n) => n > 35).length,
  };
  return { source, stats, codes, normative, sentences: all, examples, rules };
}

// ---------------------------------------------------------------- diff

/** unchanged: an identical new example; contained: every line in one new example that adds lines; split: lines spread over several; comment-edited: a line matches only without its trailing comment. */
export type ExampleStatus = "unchanged" | "contained" | "split" | "comment-edited" | "lost";

interface ExampleMatch {
  readonly old: number;
  readonly status: ExampleStatus;
  /** New example indexes holding the old example's lines. */
  readonly into: number[];
  /** Old lines found nowhere in the new version. */
  readonly missing: string[];
}

interface SentenceMatch {
  readonly old: Sentence;
  readonly best: Sentence | undefined;
  readonly score: number;
}

interface Diff {
  readonly old: Inventory;
  readonly new: Inventory;
  readonly lostCodes: string[];
  readonly addedCodes: string[];
  readonly examples: ExampleMatch[];
  /** New examples that no old example maps to. */
  readonly addedExamples: Example[];
  readonly lostRules: string[];
  readonly addedRules: string[];
  /** Added rule IDs that an earlier version of the specification used; undefined when not checked. */
  readonly reusedRules: string[] | undefined;
  readonly sentences: SentenceMatch[];
  /** New normative sentences that are no old sentence's closest match. */
  readonly unmatchedNew: Sentence[];
  /** Whether sentence pairing covered every prose sentence, not only normative ones. */
  readonly allSentences: boolean;
  readonly ok: boolean;
}

const lines = (code: string): string[] =>
  code
    .split("\n")
    .map((line) => line.trimEnd())
    .filter((line) => line.trim() !== "");
const withoutComment = (line: string): string => line.replace(/\s+#(?![{"]).*$/, "").trim();

function matchExample(old: Example, next: readonly Example[]): ExampleMatch {
  const exact = next.find((example) => example.code.trim() === old.code.trim());
  if (exact) return { old: old.index, status: "unchanged", into: [exact.index], missing: [] };
  const into = new Set<number>();
  const missing: string[] = [];
  let edited = false;
  for (const line of lines(old.code)) {
    const hit = next.find((example) => lines(example.code).some((l) => l.trim() === line.trim()));
    if (hit) {
      into.add(hit.index);
      continue;
    }
    const loose = next.find((example) =>
      lines(example.code).some((l) => withoutComment(l) === withoutComment(line)),
    );
    if (loose) {
      into.add(loose.index);
      edited = true;
    } else missing.push(line);
  }
  const status: ExampleStatus =
    missing.length > 0 ? "lost" : edited ? "comment-edited" : into.size > 1 ? "split" : "contained";
  return { old: old.index, status, into: [...into].sort((a, b) => a - b), missing };
}

const STOP = new Set(
  "the and for with that this its are was were has have not but any all each one two into from than then when where which only also does their there them they every such other same".split(
    " ",
  ),
);

function tokens(text: string): Set<string> {
  return new Set(
    text
      .toLowerCase()
      .replaceAll(/[^\p{L}\p{N}_.-]+/gu, " ")
      .split(" ")
      .map((word) => word.replace(/^[.-]+|[.-]+$/g, ""))
      .filter((word) => word.length >= 2 && !STOP.has(word)),
  );
}

/** How alike two sentences are, from 0 to 1, by the words they share. */
export function textSimilarity(a: string, b: string): number {
  return similarity(tokens(a), tokens(b));
}

function similarity(a: Set<string>, b: Set<string>): number {
  if (a.size === 0 || b.size === 0) return 0;
  let shared = 0;
  for (const token of a) if (b.has(token)) shared += 1;
  return (2 * shared) / (a.size + b.size);
}

export function diff(
  old: Inventory,
  next: Inventory,
  allSentences = false,
  usedBefore?: (id: string) => boolean,
): Diff {
  const lostCodes = Object.keys(old.codes)
    .filter((code) => !(code in next.codes))
    .sort();
  const addedCodes = Object.keys(next.codes)
    .filter((code) => !(code in old.codes))
    .sort();
  const examples = old.examples.map((example) => matchExample(example, next.examples));
  const used = new Set(examples.flatMap((match) => match.into));
  const addedExamples = next.examples.filter((example) => !used.has(example.index));
  const nextIds = new Set(next.rules.map((rule) => rule.id));
  const oldIds = new Set(old.rules.map((rule) => rule.id));
  const lostRules = [...oldIds].filter((id) => !nextIds.has(id)).sort();
  const addedRules = [...nextIds].filter((id) => !oldIds.has(id));
  const reusedRules = usedBefore && addedRules.filter(usedBefore).sort();
  const oldSentences = allSentences ? old.sentences : old.normative;
  const nextSentences = allSentences ? next.sentences : next.normative;
  const nextTokens = nextSentences.map((sentence) => tokens(sentence.text));
  const chosen = new Set<number>();
  const matches = oldSentences.map((sentence) => {
    const own = tokens(sentence.text);
    let bestIndex = -1;
    let score = 0;
    nextTokens.forEach((candidate, index) => {
      const value = similarity(own, candidate);
      if (value > score) {
        score = value;
        bestIndex = index;
      }
    });
    if (bestIndex >= 0) chosen.add(bestIndex);
    return { old: sentence, best: nextSentences[bestIndex], score: Math.round(score * 100) / 100 };
  });
  const unmatchedNew = nextSentences.filter((_, index) => !chosen.has(index));
  const ok =
    lostCodes.length === 0 &&
    lostRules.length === 0 &&
    (reusedRules ?? []).length === 0 &&
    examples.every((match) => match.status !== "lost");
  return {
    old,
    new: next,
    lostCodes,
    addedCodes,
    examples,
    addedExamples,
    lostRules,
    addedRules,
    reusedRules,
    sentences: matches,
    unmatchedNew,
    allSentences,
    ok,
  };
}

// ---------------------------------------------------------------- reports

const WEAK = 0.5;

function statsTable(entries: readonly [string, Stats][]): string[] {
  const rows: [string, keyof Stats][] = [
    ["Prose words", "words"],
    ["Paragraphs, list items, and quotes", "paragraphs"],
    ["Average words per paragraph", "averageParagraphWords"],
    ["Longest paragraph (words)", "longestParagraphWords"],
    ["Paragraphs over 90 words", "paragraphsOver90"],
    ["Sentences", "sentences"],
    ["Average words per sentence", "averageSentenceWords"],
    ["Sentences over 25 words", "sentencesOver25"],
    ["Sentences over 35 words", "sentencesOver35"],
  ];
  return [
    `| Measure | ${entries.map(([name]) => name).join(" | ")} |`,
    `| --- |${entries.map(() => " ---: |").join("")}`,
    ...rows.map(([label, key]) => `| ${label} | ${entries.map(([, s]) => s[key]).join(" | ")} |`),
  ];
}

const codeList = (codes: Record<string, number>): string =>
  Object.entries(codes)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([code, n]) => `\`${code}\` (${n})`)
    .join(", ") || "none";

const quote = (text: string): string => display(text).replaceAll("|", "\\|");

function inventoryReport(inv: Inventory): string {
  const out = [`# Rule inventory: ${inv.source}`, "", ...statsTable([["", inv.stats]]), ""];
  out.push(`## Diagnostic codes (${Object.keys(inv.codes).length})`, "", codeList(inv.codes), "");
  out.push(`## Rule IDs (${inv.rules.length})`, "");
  for (const rule of inv.rules)
    out.push(
      `- \`${rule.id}\` (line ${rule.line})${rule.codes.length ? ` → ${rule.codes.map((c) => `\`${c}\``).join(", ")}` : ""}`,
    );
  out.push("", `## Examples (${inv.examples.length})`, "");
  for (const example of inv.examples)
    out.push(
      `- ${example.index}. line ${example.line}, \`${example.info || "plain"}\`${example.error ? ", error example" : ""}: \`${lines(example.code)[0]?.trim() ?? ""}\``,
    );
  out.push("", `## Normative sentences (${inv.normative.length})`, "");
  for (const sentence of inv.normative) out.push(`- ${sentence.line}: ${quote(sentence.text)}`);
  return `${out.join("\n")}\n`;
}

function diffReport(result: Diff): string {
  const { old, new: next } = result;
  const out = [
    `# Rule inventory diff`,
    "",
    `Old: ${old.source}  `,
    `New: ${next.source}`,
    "",
    `Result: ${result.ok ? "**nothing lost**" : "**LOST CONTENT**"}`,
    "",
    "## Prose statistics",
    "",
    ...statsTable([
      ["Old", old.stats],
      ["New", next.stats],
    ]),
    "",
    "## Diagnostic codes",
    "",
    `Old: ${Object.keys(old.codes).length} codes. New: ${Object.keys(next.codes).length} codes.`,
    "",
    `- Lost: ${result.lostCodes.map((c) => `\`${c}\``).join(", ") || "none"}`,
    `- Added: ${result.addedCodes.map((c) => `\`${c}\``).join(", ") || "none"}`,
    `- Old counts: ${codeList(old.codes)}`,
    `- New counts: ${codeList(next.codes)}`,
    "",
    "## Examples",
    "",
    `Old: ${old.examples.length}. New: ${next.examples.length}` +
      ` (${next.examples.filter((e) => e.error).length} error examples).`,
    "",
    "| Old | Status | New | Missing lines |",
    "| ---: | --- | --- | --- |",
    ...result.examples.map(
      (m) =>
        `| ${m.old} | ${m.status} | ${m.into.join(", ") || "-"} | ${m.missing.map((l) => `\`${l.trim().replaceAll("|", "\\|")}\``).join("<br>") || "-"} |`,
    ),
    "",
    `Added examples: ${
      result.addedExamples
        .map((e) => `${e.index}${e.error ? " (error example)" : " (NOT an error example)"}`)
        .join(", ") || "none"
    }`,
    "",
    "## Rule IDs",
    "",
    `Old: ${old.rules.length}. New: ${next.rules.length}.`,
    "",
    `- Lost (retired; explain each in the commit message): ${result.lostRules.map((id) => `\`${id}\``).join(", ") || "none"}`,
    `- Added: ${result.addedRules.length}`,
    `- Reused (added, but an earlier version used it): ${
      result.reusedRules === undefined
        ? "not checked; give OLD as REV:PATH"
        : result.reusedRules.map((id) => `\`${id}\``).join(", ") || "none"
    }`,
    "",
    result.allSentences ? "## Sentences" : "## Normative sentences",
    "",
    `Old: ${result.sentences.length}. New: ${result.allSentences ? next.sentences.length : next.normative.length}.` +
      ` Pairs scoring under ${WEAK} need a careful look; a low score often means a sentence was split.`,
    "",
    "| Score | Old (line) | Closest new (line) |",
    "| ---: | --- | --- |",
    ...result.sentences.map(
      (m) =>
        `| ${m.score < WEAK ? `**${m.score}**` : m.score} | ${m.old.line}: ${quote(m.old.text)} | ${m.best ? `${m.best.line}: ${quote(m.best.text)}` : "-"} |`,
    ),
    "",
    `New sentences that are no old sentence's closest match (${result.unmatchedNew.length}):`,
    "",
    ...result.unmatchedNew.map((s) => `- ${s.line}: ${quote(s.text)}`),
  ];
  return `${out.join("\n")}\n`;
}

function main(args: readonly string[]): number {
  const json = args.includes("--json");
  const allSentences = args.includes("--all");
  const outIndex = args.indexOf("--out");
  const out = outIndex >= 0 ? args[outIndex + 1] : undefined;
  const rest = args.filter(
    (arg, index) =>
      arg !== "--json" &&
      arg !== "--diff" &&
      arg !== "--all" &&
      (outIndex < 0 || (index !== outIndex && index !== outIndex + 1)),
  );
  const diffing = args.includes("--diff");
  if (
    (diffing && rest.length !== 2) ||
    (!diffing && rest.length !== 1) ||
    (outIndex >= 0 && !out)
  ) {
    console.error(
      "usage: rule-inventory.ts [--json] [--out PATH] CHAPTER\n" +
        "       rule-inventory.ts --diff [--all] [--json] [--out PATH] OLD NEW",
    );
    return 2;
  }
  const known = knownCodes(
    readFileSync(resolve(specRoot, "README.md"), "utf8"),
    readFileSync(resolve(specRoot, "lang/06-control-flow.md"), "utf8"),
  );
  let text: string;
  let ok = true;
  if (diffing) {
    const [oldSpec, newSpec] = rest as [string, string];
    const result = diff(
      inventory(oldSpec, load(oldSpec), known),
      inventory(newSpec, load(newSpec), known),
      allSentences,
      historySearch(oldSpec),
    );
    ok = result.ok;
    text = json ? `${JSON.stringify(result, null, 2)}\n` : diffReport(result);
  } else {
    const spec = rest[0]!;
    const result = inventory(spec, load(spec), known);
    text = json ? `${JSON.stringify(result, null, 2)}\n` : inventoryReport(result);
  }
  if (out) writeFileSync(out, text);
  else process.stdout.write(text);
  return ok ? 0 : 1;
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) process.exitCode = main(process.argv.slice(2));
