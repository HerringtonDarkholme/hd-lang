// `spec coverage`: how many rule IDs a conformance fixture cites. A rule is
// covered when a fixture file, cases.tsv, or examples.tsv under
// spec/conformance/ cites its ID. The conformance README does not count.
import { allRules, type Corpus } from "./spec-corpus.ts";
import type { RefIndex } from "./spec-refs.ts";

interface ChapterCoverage {
  readonly chapter: string;
  readonly tier: string;
  readonly rules: number;
  readonly cited: number;
  readonly uncovered: readonly string[];
}

export interface Coverage {
  readonly chapters: ChapterCoverage[];
  readonly rules: number;
  readonly cited: number;
}

/** Whether a citation comes from a fixture, cases.tsv, or examples.tsv. */
function isFixtureCitation(file: string): boolean {
  return file.startsWith("spec/conformance/") && file !== "spec/conformance/README.md";
}

export function coverage(corpus: Corpus, index: RefIndex): Coverage {
  const cited = new Set(index.citations.filter((c) => isFixtureCitation(c.file)).map((c) => c.id));
  const entries = allRules(corpus);
  const chapters = corpus.chapters.map((chapter) => {
    const ids = entries.filter((entry) => entry.chapter === chapter).map((entry) => entry.rule.id);
    return {
      chapter: chapter.name,
      tier: chapter.tier,
      rules: ids.length,
      cited: ids.filter((id) => cited.has(id)).length,
      uncovered: ids.filter((id) => !cited.has(id)),
    };
  });
  return {
    chapters,
    rules: chapters.reduce((sum, c) => sum + c.rules, 0),
    cited: chapters.reduce((sum, c) => sum + c.cited, 0),
  };
}

const share = (cited: number, rules: number): string =>
  rules === 0 ? "-" : `${((100 * cited) / rules).toFixed(1)}%`;

/** The table, or with `uncovered` set the uncovered IDs of the matching chapters. */
export function coverageReport(result: Coverage, uncovered?: string): string {
  if (uncovered !== undefined) {
    const found = result.chapters.filter(
      (c) =>
        c.chapter === uncovered ||
        c.chapter.replace(/^(?:lang|std|cli)\//, "").startsWith(uncovered),
    );
    if (found.length === 0) throw new Error(`no chapter matches ${uncovered}`);
    return found.flatMap((c) => c.uncovered).join("\n") + "\n";
  }
  const rows = [
    ...result.chapters.map((c) => [c.chapter, c.rules, c.cited, share(c.cited, c.rules)] as const),
    ["total", result.rules, result.cited, share(result.cited, result.rules)] as const,
  ];
  const widths = [
    Math.max("Chapter".length, ...rows.map((r) => r[0].length)),
    Math.max("Rules".length, ...rows.map((r) => String(r[1]).length)),
    Math.max("Cited".length, ...rows.map((r) => String(r[2]).length)),
    Math.max("Share".length, ...rows.map((r) => r[3].length)),
  ];
  const line = (a: string, b: string, c: string, d: string): string =>
    `${a.padEnd(widths[0]!)}  ${b.padStart(widths[1]!)}  ${c.padStart(widths[2]!)}  ${d.padStart(widths[3]!)}`;
  const lines = [
    line("Chapter", "Rules", "Cited", "Share"),
    ...rows.map((r) => line(r[0], String(r[1]), String(r[2]), r[3])),
  ];
  return `${lines.join("\n")}\n`;
}
