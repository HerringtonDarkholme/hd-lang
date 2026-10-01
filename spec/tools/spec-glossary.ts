// `spec glossary`: the defined terms of the specification, as data and as a
// Markdown page that the website renders.
//
// The hand-written glossaries in spec/README.md and spec/std/README.md are the
// primary source. The chapters add the terms they define: a bold term in a
// prose sentence or a rule, and a heading whose section opens by defining it,
// as "The prelude is ...". Every definition is spec text, quoted as written;
// the tool writes none. A chapter term that no hand-written glossary lists is
// reported as missing.
import { posix } from "node:path";

import { type Chapter, type Corpus, sectionAt } from "./spec-corpus.ts";
import { cells, sentences, STD_DIRECTORY } from "./spec-prose.ts";

type TermSource = "glossary" | "chapter-bold" | "chapter-heading";

interface GlossaryTerm {
  readonly term: string;
  /** The definition as Markdown, quoted from the spec; links are relative to spec/. */
  readonly definition: string;
  /** The page under spec/ that defines the term, as `08-data-and-enums.md` or `std/iter.md`. */
  readonly chapter: string;
  /** The defining rule's or section's fragment, without `#`, as `r-data.kind.data`. */
  readonly anchor: string;
  /** The defining rule ID, when the anchor names a rule. */
  readonly rule: string | undefined;
  /** How the defining place is named: a rule ID in backticks, or a section title. */
  readonly label: string;
  readonly source: TermSource;
  /** Where the entry was read, as `spec/README.md:178`. */
  readonly at: string;
}

interface Glossary {
  /** Whether spec/README.md or spec/std/README.md has a hand-written glossary. */
  readonly handWritten: boolean;
  /** Every term, sorted: the hand-written entries, then the chapter terms they lack. */
  readonly terms: GlossaryTerm[];
  /** Terms the chapters define that no hand-written glossary lists. */
  readonly missing: GlossaryTerm[];
}

/** GitHub's heading anchor algorithm, as website/src/markdown.ts applies it. */
function githubSlug(text: string): string {
  return text
    .replaceAll(/<[^>]+>/g, "")
    .trim()
    .toLowerCase()
    .replaceAll(/[`*_~]/g, "")
    .replaceAll(/[^\p{L}\p{N}_\- ]/gu, "")
    .replaceAll(" ", "-");
}

/** Lookup keys for a term: lowercase, without code marks, with and without a plural `s`. */
export function termKeys(term: string): string[] {
  const key = term.toLowerCase().replaceAll("`", "").replaceAll(/\s+/g, " ").trim();
  const keys = [key];
  if (key.endsWith("es") && key.length > 4) keys.push(key.slice(0, -2));
  if (key.endsWith("s") && key.length > 3) keys.push(key.slice(0, -1));
  return keys;
}

/** Rewrites the relative links of Markdown written in `directory` (under spec/) to be relative to spec/. */
export function rebaseLinks(markdown: string, directory: string): string {
  if (directory === "" || directory === ".") return markdown;
  return markdown.replaceAll(/\]\(([^)\s]+)\)/g, (whole, href: string) => {
    if (/^(?:[a-z][a-z0-9+.-]*:|\/|#)/i.test(href)) return whole;
    return `](${posix.normalize(posix.join(directory, href))})`;
  });
}

const GLOSSARY_ROW = /^\|\s*\*\*(.+?)\*\*\s*\|\s*(.*?)\s*\|\s*$/;
const SEE = /\s*See \[([^\]]*)\]\(([^)\s]+)\)\.\s*$/;
const LINK = /\[([^\]]*)\]\(([^)\s]+)\)/;

/** The entries of the `## Glossary` table in a README at `directory` under spec/ ("" for spec/). */
function handWrittenGlossary(readme: string, directory: string): GlossaryTerm[] {
  const lines = readme.split("\n");
  const start = lines.findIndex((line) => /^## Glossary\s*$/.test(line));
  if (start < 0) return [];
  const file = posix.join("spec", directory, "README.md");
  const out: GlossaryTerm[] = [];
  for (
    let index = start + 1;
    index < lines.length && !lines[index]!.startsWith("## ");
    index += 1
  ) {
    const row = GLOSSARY_ROW.exec(lines[index]!);
    if (!row) continue;
    const see = SEE.exec(row[2]!);
    const link = see ?? LINK.exec(row[2]!);
    if (!link) continue;
    const target = posix.normalize(posix.join(directory, link[2]!));
    const hash = target.indexOf("#");
    const anchor = hash < 0 ? "" : target.slice(hash + 1);
    // A pointer to the other glossary, such as "A stdlib term, in the ... glossary".
    if (anchor === "glossary") continue;
    out.push({
      term: row[1]!,
      definition: rebaseLinks(see ? row[2]!.slice(0, see.index).trim() : row[2]!, directory),
      chapter: hash < 0 ? target : target.slice(0, hash),
      anchor,
      rule: anchor.startsWith("r-") ? anchor.slice(2) : undefined,
      label: link[1]!,
      source: "glossary",
      at: `${file}:${index + 1}`,
    });
  }
  return out;
}

/** Bold text that is not a defined term: normative vocabulary and callout labels. */
const NOT_A_TERM =
  /^(?:must(?: not)?|should(?: not)?|may|shall|is an error|is invalid|is rejected|not|never|only|all|any|one|none|no|every|exactly|both|either|neither|why|note|example)$/i;

function boldTerms(text: string): string[] {
  const found: string[] = [];
  for (const match of text
    .replaceAll(/`[^`]*`/g, (span) => span.replaceAll("*", " "))
    .matchAll(/\*\*([^*]+?)\*\*/g)) {
    const term = match[1]!.trim();
    if (/[.:!?]$/.test(term) || NOT_A_TERM.test(term) || !/\p{L}/u.test(term)) continue;
    if (term.split(/\s+/).length > 6) continue;
    found.push(term);
  }
  return found;
}

/** Each section's anchor, as the website and GitHub give it, keyed by its heading line. */
function sectionAnchors(chapter: Chapter): Map<number, string> {
  const seen = new Map<string, number>();
  const out = new Map<number, string>();
  for (const section of chapter.sections) {
    const base = githubSlug(section.title);
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    out.set(section.line, count === 0 ? base : `${base}-${count}`);
  }
  return out;
}

const LEADING_MARKER = /^r\[([^\]]+)\]\s*/;
const escapeCells = (text: string): string => text.replaceAll(/(?<!\\)\|/g, "\\|");

/** The terms a chapter defines: bold terms in prose and rules, and defining headings. */
function chapterTerms(chapter: Chapter): GlossaryTerm[] {
  const anchors = sectionAnchors(chapter);
  const directory = posix.dirname(chapter.name);
  const out: GlossaryTerm[] = [];
  const place = (line: number, rule: string | undefined) => {
    const section = sectionAt(chapter, line);
    return rule
      ? { anchor: `r-${rule}`, rule, label: `\`${rule}\`` }
      : {
          anchor: section ? anchors.get(section.line)! : "",
          rule: undefined,
          label: section?.title ?? chapter.name,
        };
  };
  for (const block of chapter.blocks) {
    if (block.kind !== "paragraph" && block.kind !== "item" && block.kind !== "row") continue;
    const parts = block.kind === "row" ? cells(block.text) : [block.text];
    const rule = parts.map((part) => LEADING_MARKER.exec(part)?.[1]).find((id) => id);
    for (const term of boldTerms(block.text)) {
      // In a table row, the cells after the term's cell define it.
      const after = parts.slice(parts.findIndex((part) => part.includes(`**${term}**`)) + 1);
      const definition =
        block.kind === "row"
          ? (after.length > 0 ? after : parts)
              .map((part) => part.replace(LEADING_MARKER, ""))
              .filter((part) => part !== "")
              .join(" | ")
          : (sentences(block.text).find((sentence) => sentence.includes(`**${term}**`)) ??
            block.text.replace(LEADING_MARKER, ""));
      out.push({
        term,
        definition: rebaseLinks(definition, directory),
        chapter: chapter.name,
        ...place(block.line, rule),
        source: "chapter-bold",
        at: `spec/${chapter.name}:${block.line}`,
      });
    }
  }
  // A heading defines its term when its section opens with "A term is a ...",
  // "The term is the ...", or "The terms are ...".
  for (const section of chapter.sections) {
    if (section.level < 2) continue;
    const first = chapter.blocks.find(
      (block) =>
        block.line > section.line &&
        block.line <= section.end &&
        (block.kind === "paragraph" || block.kind === "item"),
    );
    if (!first) continue;
    const sentence = sentences(first.text)[0] ?? "";
    const title = section.title.replaceAll("`", "").toLowerCase();
    const opens = termKeys(title).some((key) =>
      new RegExp(
        String.raw`^(?:an?|the)\s+(?:\*\*)?${key.replaceAll(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?:\*\*)?\s+(?:is (?:an?|the|one|any)\b|are\b)`,
        "i",
      ).test(sentence.replace(LEADING_MARKER, "")),
    );
    if (!opens) continue;
    const rule = LEADING_MARKER.exec(first.text)?.[1];
    out.push({
      term: section.title.replaceAll("`", "").toLowerCase(),
      definition: rebaseLinks(sentence.replace(LEADING_MARKER, ""), directory),
      chapter: chapter.name,
      ...place(rule ? first.line : section.line, rule),
      source: "chapter-heading",
      at: `spec/${chapter.name}:${section.line}`,
    });
  }
  return out;
}

const byTerm = (a: GlossaryTerm, b: GlossaryTerm): number =>
  a.term.toLowerCase().localeCompare(b.term.toLowerCase()) || a.chapter.localeCompare(b.chapter);

export function glossary(corpus: Corpus): Glossary {
  const listed = [
    ...handWrittenGlossary(corpus.readme, ""),
    ...handWrittenGlossary(corpus.stdReadme, STD_DIRECTORY),
  ];
  const known = new Set(listed.flatMap((entry) => termKeys(entry.term)));
  const missing: GlossaryTerm[] = [];
  for (const chapter of corpus.chapters)
    for (const entry of chapterTerms(chapter)) {
      const keys = termKeys(entry.term);
      if (keys.some((key) => known.has(key))) continue;
      for (const key of keys) known.add(key);
      missing.push(entry);
    }
  return {
    handWritten: listed.length > 0,
    terms: [...listed, ...missing].sort(byTerm),
    missing: missing.sort(byTerm),
  };
}

/** The glossary as a Markdown page under spec/, one table per initial letter. */
export function glossaryMarkdown(result: Glossary): string {
  const out = [
    "# Glossary",
    "",
    "This page is generated by `pnpm run spec glossary --markdown`. It",
    "collects the [language glossary](README.md#glossary), the",
    "[Standard Library glossary](std/README.md#glossary), and the terms the",
    "chapters define in bold that those glossaries do not list yet. Each term",
    "links to the rule, or the section, that defines it, and each definition",
    "is quoted from the specification.",
  ];
  let letter = "";
  for (const entry of result.terms) {
    const initial =
      entry.term
        .replace(/^[^\p{L}]+/u, "")
        .charAt(0)
        .toUpperCase() || "#";
    if (initial !== letter) {
      letter = initial;
      out.push("", `## ${letter}`, "", "| Term | Definition | Defined in |", "| --- | --- | --- |");
    }
    const href = entry.anchor ? `${entry.chapter}#${entry.anchor}` : entry.chapter;
    const note = entry.source === "glossary" ? "" : " (not in a hand-written glossary)";
    out.push(
      `| [**${escapeCells(entry.term)}**](${href}) | ${escapeCells(entry.definition)} |[${escapeCells(entry.label)}](${href})${note} |`,
    );
  }
  return `${out.join("\n")}\n`;
}

export function glossaryReport(result: Glossary): string {
  const count = (source: TermSource): number =>
    result.terms.filter((entry) => entry.source === source).length;
  const out = [
    `Glossary terms: ${result.terms.length} (${count("glossary")} hand-written, ${count("chapter-bold")} bold in a chapter, ${count("chapter-heading")} from a heading)`,
  ];
  if (!result.handWritten) out.push("spec/README.md has no hand-written glossary.");
  out.push(
    `Defined in a chapter but missing from the hand-written glossaries: ${result.missing.length}`,
  );
  for (const entry of result.missing)
    out.push(`  ${entry.at}: ${entry.term} (${entry.rule ?? `#${entry.anchor}`})`);
  return `${out.join("\n")}\n`;
}
