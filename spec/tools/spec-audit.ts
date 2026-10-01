// `spec audit`: spec/STYLE.md checks over every chapter, as warnings.
//
// The targets are STYLE.md's "Writing Rules" (25-word sentences, 90-word
// paragraphs), its section template (an example near the rules), unique rule
// text, the rule ID syntax and prefix table, and the README Diagnostics table.
// spec/check-spec-style.ts enforces the hard failures; this audit reports the
// softer targets for a cleanup pass.
import {
  CHAPTER_PREFIXES,
  display,
  knownCodes,
  paragraphs,
  readable,
  RULE_ID,
  ruleMarkers,
  sentences,
  wordCount,
} from "./spec-prose.ts";
import { allRules, type Chapter, type Corpus } from "./spec-corpus.ts";

const SENTENCE_TARGET = 25;
const PARAGRAPH_TARGET = 90;
/** A sentence shorter than this is too generic to call a duplicate. */
const DUPLICATE_MIN_WORDS = 6;

const AUDIT_KINDS = [
  "long-sentence",
  "long-paragraph",
  "no-example",
  "duplicate-rule",
  "bad-rule-id",
  "unknown-code",
  "unused-code",
] as const;
type AuditKind = (typeof AUDIT_KINDS)[number];

interface AuditWarning {
  readonly kind: AuditKind;
  /** The path under spec/. */
  readonly file: string;
  readonly line: number;
  readonly message: string;
}

const clip = (text: string, length = 70): string =>
  text.length > length ? `${text.slice(0, length - 3)}...` : text;

function proseWarnings(chapter: Chapter): AuditWarning[] {
  const out: AuditWarning[] = [];
  const at = (kind: AuditKind, line: number, message: string): void => {
    out.push({ kind, file: chapter.name, line, message });
  };
  for (const block of chapter.blocks) {
    if (block.kind !== "paragraph" && block.kind !== "item" && block.kind !== "quote") continue;
    for (const sentence of sentences(block.text)) {
      const words = wordCount(sentence);
      if (words > SENTENCE_TARGET)
        at("long-sentence", block.line, `sentence has ${words} words: ${clip(readable(sentence))}`);
    }
  }
  for (const block of paragraphs(chapter.blocks)) {
    const words = wordCount(block.text);
    if (words > PARAGRAPH_TARGET) at("long-paragraph", block.line, `paragraph has ${words} words`);
  }
  for (const section of chapter.sections) {
    const inside = (line: number): boolean => line > section.line && line <= section.end;
    const rules = chapter.inventory.rules.filter((rule) => inside(rule.line));
    if (rules.length === 0) continue;
    if (chapter.blocks.some((block) => block.kind === "code" && inside(block.line))) continue;
    at(
      "no-example",
      section.line,
      `section "${section.title}" has ${rules.length} rule${rules.length === 1 ? "" : "s"} and no example`,
    );
  }
  return out;
}

function idWarnings(corpus: Corpus): AuditWarning[] {
  const out: AuditWarning[] = [];
  const seen = new Map<string, string>();
  for (const chapter of corpus.chapters) {
    for (const problem of ruleMarkers(chapter.blocks).problems) {
      const [line, ...rest] = problem.split(": ");
      out.push({
        kind: "bad-rule-id",
        file: chapter.name,
        line: Number(line),
        message: rest.join(": "),
      });
    }
    for (const rule of chapter.inventory.rules) {
      const where = `${chapter.name}:${rule.line}`;
      const warn = (message: string): void => {
        out.push({ kind: "bad-rule-id", file: chapter.name, line: rule.line, message });
      };
      if (!RULE_ID.test(rule.id)) warn(`rule ID ${rule.id} breaks the RULE_ID pattern`);
      if (chapter.prefix === undefined)
        warn(`rule ID ${rule.id} is in a chapter with no prefix in CHAPTER_PREFIXES`);
      else if (rule.id.split(".", 1)[0] !== chapter.prefix)
        warn(`rule ID ${rule.id} lacks this chapter's prefix ${chapter.prefix}`);
      const first = seen.get(rule.id);
      if (first) warn(`rule ID ${rule.id} duplicates ${first}`);
      else seen.set(rule.id, where);
    }
  }
  const prefixes = Object.values(CHAPTER_PREFIXES);
  for (const prefix of new Set(prefixes))
    if (prefixes.filter((other) => other === prefix).length > 1)
      out.push({
        kind: "bad-rule-id",
        file: "tools/spec-prose.ts",
        line: 0,
        message: `prefix ${prefix} names two chapters in CHAPTER_PREFIXES`,
      });
  return out;
}

/** A sentence reduced to its words, lowercase, with inline code kept as written. */
export function normalizeSentence(sentence: string): string {
  return display(sentence)
    .toLowerCase()
    .replaceAll(/[^\p{L}\p{N}`_.-]+/gu, " ")
    .replaceAll(/(?<![\p{L}\p{N}])[.-]+|[.-]+(?![\p{L}\p{N}])/gu, " ")
    .replaceAll(/\s+/g, " ")
    .trim();
}

function duplicateWarnings(corpus: Corpus): AuditWarning[] {
  const groups = new Map<string, { file: string; line: number; id: string }[]>();
  for (const { chapter, rule } of allRules(corpus))
    for (const sentence of sentences(rule.text)) {
      if (/^(?:Error|Warning|Panic):/.test(sentence.trim())) continue;
      if (wordCount(sentence) < DUPLICATE_MIN_WORDS) continue;
      const key = normalizeSentence(sentence);
      const group = groups.get(key) ?? [];
      if (!group.some((entry) => entry.id === rule.id))
        group.push({ file: chapter.name, line: rule.line, id: rule.id });
      groups.set(key, group);
    }
  const out: AuditWarning[] = [];
  for (const [key, group] of groups) {
    if (group.length < 2) continue;
    const [first, ...rest] = group;
    for (const entry of rest)
      out.push({
        kind: "duplicate-rule",
        file: entry.file,
        line: entry.line,
        message: `${entry.id} repeats a sentence of ${first!.id} (${first!.file}:${first!.line}): ${clip(key)}`,
      });
  }
  return out;
}

const NAMED_CODES = /\b(Error|Warning|Panic):((?:\s*(?:,|and|or)?\s*`[a-z0-9-]+`)+)/g;

function codeWarnings(corpus: Corpus): AuditWarning[] {
  const tableCodes = knownCodes(corpus.readme, "");
  const panics = knownCodes("", corpus.controlFlow);
  const out: AuditWarning[] = [];
  // A rule's text, or for a rule table row, the whole row with its Error column.
  const ruleText = corpus.chapters
    .flatMap((chapter) => chapter.blocks)
    .filter((block) => block.kind !== "code" && /(?:^|\| )r\[/.test(block.text))
    .map((block) => block.text)
    .join("\n");
  for (const chapter of corpus.chapters)
    for (const block of chapter.blocks) {
      if (block.kind === "code" || block.kind === "heading") continue;
      for (const match of block.text.matchAll(NAMED_CODES))
        for (const code of match[2]!.matchAll(/`([a-z0-9-]+)`/g)) {
          const known = match[1] === "Panic" ? panics : tableCodes;
          if (!known.has(code[1]!))
            out.push({
              kind: "unknown-code",
              file: chapter.name,
              line: block.line,
              message: `${match[1]}: \`${code[1]}\` is not in the ${match[1] === "Panic" ? "panic category list" : "README Diagnostics table"}`,
            });
        }
    }
  const prose = corpus.chapters.map((chapter) => chapter.text).join("\n");
  for (const code of [...tableCodes].sort())
    if (!ruleText.includes(`\`${code}\``))
      out.push({
        kind: "unused-code",
        file: "README.md",
        line: corpus.readme.split("\n").findIndex((line) => line.includes(`\`${code}\``)) + 1,
        message: `\`${code}\` is in the Diagnostics table, but no rule names it${prose.includes(`\`${code}\``) ? " (chapter prose does)" : ""}`,
      });
  return out;
}

export function audit(corpus: Corpus): AuditWarning[] {
  return [
    ...corpus.chapters.flatMap(proseWarnings),
    ...idWarnings(corpus),
    ...duplicateWarnings(corpus),
    ...codeWarnings(corpus),
  ];
}

export function auditTotals(warnings: readonly AuditWarning[]): Record<AuditKind, number> {
  const totals = Object.fromEntries(AUDIT_KINDS.map((kind) => [kind, 0])) as Record<
    AuditKind,
    number
  >;
  for (const warning of warnings) totals[warning.kind] += 1;
  return totals;
}

const SHORT: Record<AuditKind, string> = {
  "long-sentence": "sent>25",
  "long-paragraph": "para>90",
  "no-example": "no-ex",
  "duplicate-rule": "dup",
  "bad-rule-id": "bad-id",
  "unknown-code": "unk-code",
  "unused-code": "unused",
};

export function auditReport(warnings: readonly AuditWarning[], listAll: boolean): string {
  const out: string[] = [];
  if (listAll)
    for (const warning of warnings)
      out.push(
        `spec/${warning.file}:${warning.line}: warning: ${warning.kind}: ${warning.message}`,
      );
  const files = [...new Set(warnings.map((warning) => warning.file))].sort();
  const header = ["File", ...AUDIT_KINDS.map((kind) => SHORT[kind]), "total"];
  const rows = files.map((file) => {
    const totals = auditTotals(warnings.filter((warning) => warning.file === file));
    return [
      file,
      ...AUDIT_KINDS.map((kind) => totals[kind]),
      Object.values(totals).reduce((a, b) => a + b, 0),
    ];
  });
  const totals = auditTotals(warnings);
  rows.push(["total", ...AUDIT_KINDS.map((kind) => totals[kind]), warnings.length]);
  const widths = header.map((title, index) =>
    Math.max(title.length, ...rows.map((row) => String(row[index]).length)),
  );
  const line = (row: readonly (string | number)[]): string =>
    row
      .map((cell, index) =>
        index === 0 ? String(cell).padEnd(widths[0]!) : String(cell).padStart(widths[index]!),
      )
      .join("  ");
  if (out.length > 0) out.push("");
  out.push(
    `Audit summary (spec/STYLE.md targets: sentences <= ${SENTENCE_TARGET} words, paragraphs <= ${PARAGRAPH_TARGET} words)`,
    "",
    line(header),
    ...rows.map(line),
    "",
    `Kinds: ${AUDIT_KINDS.map((kind) => `${SHORT[kind]} = ${kind}`).join(", ")}.`,
  );
  return `${out.join("\n")}\n`;
}
