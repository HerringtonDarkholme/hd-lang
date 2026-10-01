// `spec counts`: numbered rules per chapter, per rule-ID prefix or topic, and
// per tier, with a best-effort Design Cost Order kind (AGENTS.md).
import type { Rule } from "./rule-inventory.ts";
import { type Chapter, type Corpus, sectionAt, type Tier } from "./spec-corpus.ts";

/** The four kinds of change in AGENTS.md "Design Cost Order", most costly first. */
export const KINDS = ["syntax", "semantic", "intrinsic", "core-library"] as const;
export type Kind = (typeof KINDS)[number];

const SYNTAX_PREFIXES = new Set(["lex", "grammar"]);
const SYNTAX_HEADING = /\b(?:syntax|grammar|spellings?|tokens?|layout|lexical)\b/i;
const INTRINSIC_TEXT = /\b(?:intrinsics?|lang items?|prelude|known by name)\b/i;
const INTRINSIC_SEGMENT = /^(?:intrinsics?|lang-items?|prelude)$/;

/**
 * A heuristic guess at the Design Cost Order kind a rule belongs to:
 * - core-library: a stdlib-tier rule (spec/std/);
 * - syntax: prefix `lex` or `grammar`, a rule that names `syntax-error`, or a
 *   section heading about syntax, grammar, spellings, tokens, or layout;
 * - intrinsic: an ID segment, a heading, or the rule text names an
 *   intrinsic, a lang item, or the prelude;
 * - semantic: every other language-tier rule.
 */
export function ruleKind(chapter: Chapter, rule: Rule): Kind {
  if (chapter.tier === "std") return "core-library";
  const prefix = rule.id.split(".", 1)[0]!;
  const trail = (sectionAt(chapter, rule.line)?.trail ?? []).join(" ");
  if (
    SYNTAX_PREFIXES.has(prefix) ||
    rule.codes.includes("syntax-error") ||
    SYNTAX_HEADING.test(trail)
  )
    return "syntax";
  if (
    rule.id.split(".").some((segment) => INTRINSIC_SEGMENT.test(segment)) ||
    INTRINSIC_TEXT.test(trail) ||
    INTRINSIC_TEXT.test(rule.text)
  )
    return "intrinsic";
  return "semantic";
}

export interface ChapterCount {
  readonly chapter: string;
  readonly tier: Tier;
  readonly prefix: string;
  readonly rules: number;
  /** Heuristic; see ruleKind. */
  readonly kinds: Record<Kind, number>;
}

export interface Counts {
  readonly chapters: ChapterCount[];
  /** Rules per first ID segment. */
  readonly prefixes: Record<string, number>;
  /** Rules per first two ID segments. */
  readonly topics: Record<string, number>;
  readonly tiers: Record<Tier, number>;
  /** Heuristic; see ruleKind. */
  readonly kinds: Record<Kind, number>;
  readonly total: number;
}

const emptyKinds = (): Record<Kind, number> => ({
  syntax: 0,
  semantic: 0,
  intrinsic: 0,
  "core-library": 0,
});

export function counts(corpus: Corpus): Counts {
  const prefixes: Record<string, number> = {};
  const topics: Record<string, number> = {};
  const tiers: Record<Tier, number> = { language: 0, std: 0 };
  const kinds = emptyKinds();
  const chapters = corpus.chapters.map((chapter) => {
    const own = emptyKinds();
    for (const rule of chapter.inventory.rules) {
      const segments = rule.id.split(".");
      prefixes[segments[0]!] = (prefixes[segments[0]!] ?? 0) + 1;
      const topic = segments.slice(0, 2).join(".");
      topics[topic] = (topics[topic] ?? 0) + 1;
      const kind = ruleKind(chapter, rule);
      own[kind] += 1;
      kinds[kind] += 1;
    }
    tiers[chapter.tier] += chapter.inventory.rules.length;
    return {
      chapter: chapter.name,
      tier: chapter.tier,
      prefix: chapter.prefix ?? "-",
      rules: chapter.inventory.rules.length,
      kinds: own,
    };
  });
  return { chapters, prefixes, topics, tiers, kinds, total: tiers.language + tiers.std };
}

export type CountsView = "chapter" | "prefix" | "topic" | "kind";

function table(
  header: readonly string[],
  rows: readonly (readonly (string | number)[])[],
): string[] {
  const widths = header.map((title, index) =>
    Math.max(title.length, ...rows.map((row) => String(row[index]).length)),
  );
  const line = (row: readonly (string | number)[]): string =>
    row
      .map((cell, index) =>
        typeof cell === "number"
          ? String(cell).padStart(widths[index]!)
          : cell.padEnd(widths[index]!),
      )
      .join("  ")
      .trimEnd();
  return [line(header), widths.map((width) => "-".repeat(width)).join("  "), ...rows.map(line)];
}

export function countsReport(result: Counts, view: CountsView = "chapter"): string {
  const out: string[] = [];
  if (view === "prefix" || view === "topic") {
    const source = view === "prefix" ? result.prefixes : result.topics;
    const rows = Object.entries(source).sort(([a], [b]) => a.localeCompare(b));
    out.push(...table([view === "prefix" ? "Prefix" : "Topic", "Rules"], rows));
  } else if (view === "kind") {
    out.push(
      "Kind is a heuristic guess (spec/tools/spec-counts.ts, ruleKind), not a classification.",
      "",
      ...table(
        ["Chapter", "Rules", ...KINDS.map((kind) => `${kind} (heuristic)`)],
        result.chapters.map((row) => [row.chapter, row.rules, ...KINDS.map((k) => row.kinds[k])]),
      ),
      "",
      ...table(
        ["Kind (heuristic)", "Rules"],
        KINDS.map((kind) => [kind, result.kinds[kind]]),
      ),
    );
  } else
    out.push(
      ...table(
        ["Chapter", "Tier", "Prefix", "Rules"],
        result.chapters.map((row) => [row.chapter, row.tier, row.prefix, row.rules]),
      ),
    );
  out.push(
    "",
    `Language tier: ${result.tiers.language}`,
    `Stdlib tier: ${result.tiers.std}`,
    `Total: ${result.total}`,
  );
  return `${out.join("\n")}\n`;
}
