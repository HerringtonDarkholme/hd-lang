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
  readonly compiler?: number;
  readonly compilerUncovered?: readonly string[];
}

export interface Coverage {
  readonly chapters: ChapterCoverage[];
  readonly rules: number;
  readonly cited: number;
  readonly compiler?: number;
}

/** Whether a citation comes from a fixture, cases.tsv, or examples.tsv. */
function isFixtureCitation(file: string): boolean {
  return file.startsWith("spec/conformance/") && file !== "spec/conformance/README.md";
}

/** The checked-in paths which the new compiler's Q18 gate passes. */
export function compilerPassList(report: string): Set<string> {
  const match = /<!-- pass-list-start -->\s*```text\n([\s\S]*?)\n```\s*<!-- pass-list-end -->/.exec(
    report,
  );
  if (!match) throw new Error("compiler/CONFORMANCE.md has no pass list");
  return new Set(match[1]!.split(/\r?\n/).filter((line) => line !== ""));
}

function passedCitation(file: string, text: string, passed: ReadonlySet<string>): boolean {
  const prefix = "spec/conformance/";
  if (!file.startsWith(prefix)) return false;
  if (file === `${prefix}cases.tsv`) return passed.has(text.split("\t")[0]!);
  if (file === `${prefix}cli-cases.tsv`) return passed.has(`cli/${text.split("\t")[0]!}`);
  if (file.endsWith(".tsv") || file.endsWith(".md")) return false;
  return passed.has(file.slice(prefix.length));
}

export function coverage(
  corpus: Corpus,
  index: RefIndex,
  compilerPasses?: ReadonlySet<string>,
): Coverage {
  const cited = new Set(index.citations.filter((c) => isFixtureCitation(c.file)).map((c) => c.id));
  const compilerCited =
    compilerPasses === undefined
      ? undefined
      : new Set(
          index.citations
            .filter((citation) => passedCitation(citation.file, citation.text, compilerPasses))
            .map((citation) => citation.id),
        );
  const entries = allRules(corpus);
  const chapters = corpus.chapters.map((chapter) => {
    const ids = entries.filter((entry) => entry.chapter === chapter).map((entry) => entry.rule.id);
    return {
      chapter: chapter.name,
      tier: chapter.tier,
      rules: ids.length,
      cited: ids.filter((id) => cited.has(id)).length,
      uncovered: ids.filter((id) => !cited.has(id)),
      ...(compilerCited === undefined
        ? {}
        : {
            compiler: ids.filter((id) => compilerCited.has(id)).length,
            compilerUncovered: ids.filter((id) => !compilerCited.has(id)),
          }),
    };
  });
  return {
    chapters,
    rules: chapters.reduce((sum, c) => sum + c.rules, 0),
    cited: chapters.reduce((sum, c) => sum + c.cited, 0),
    ...(compilerCited === undefined
      ? {}
      : { compiler: chapters.reduce((sum, chapter) => sum + chapter.compiler!, 0) }),
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
    return found.flatMap((c) => c.compilerUncovered ?? c.uncovered).join("\n") + "\n";
  }
  if (result.compiler !== undefined) {
    const rows = [
      ...result.chapters.map(
        (chapter) =>
          [
            chapter.chapter,
            chapter.rules,
            chapter.cited,
            share(chapter.cited, chapter.rules),
            chapter.compiler!,
            share(chapter.compiler!, chapter.rules),
          ] as const,
      ),
      [
        "total",
        result.rules,
        result.cited,
        share(result.cited, result.rules),
        result.compiler,
        share(result.compiler, result.rules),
      ] as const,
    ];
    const headers = ["Chapter", "Rules", "Cited", "Share", "Compiler", "Compiler share"];
    const widths = headers.map((header, column) =>
      Math.max(header.length, ...rows.map((row) => String(row[column]).length)),
    );
    const line = (row: readonly (string | number)[]): string =>
      row
        .map((value, column) =>
          column === 0
            ? String(value).padEnd(widths[column]!)
            : String(value).padStart(widths[column]!),
        )
        .join("  ");
    return `${[line(headers), ...rows.map(line)].join("\n")}\n`;
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
