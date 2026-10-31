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

/**
 * The indexed cases the Rust compiler passes: every case of `cases.tsv`
 * and every `cli/NAME` of `cli-cases.tsv`, except the paths the root
 * `KNOWN_FAILURES.tsv` lists (header `path issue engine reason`) for any
 * engine.
 */
export function compilerPasses(
  cases: string,
  cliCases: string,
  knownFailures: string,
): Set<string> {
  const rows = (text: string) =>
    text
      .split(/\r?\n/)
      .slice(1)
      .filter((line) => line !== "")
      .map((line) => line.split("\t"));
  const header = knownFailures.split(/\r?\n/, 1)[0];
  if (header !== "path\tissue\tengine\treason") {
    throw new Error("KNOWN_FAILURES.tsv must start with the header path, issue, engine, reason");
  }
  const known = new Set(rows(knownFailures).map((columns) => columns[0]!));
  const indexed = [
    ...rows(cases).map((columns) => columns[0]!),
    ...rows(cliCases).map((columns) => `cli/${columns[0]!}`),
  ];
  return new Set(indexed.filter((path) => !known.has(path)));
}

function passedCitation(file: string, text: string, passed: ReadonlySet<string>): boolean {
  const prefix = "spec/conformance/";
  if (!file.startsWith(prefix)) {
    return false;
  }
  if (file === `${prefix}cases.tsv`) {
    return passed.has(text.split("\t")[0]!);
  }
  if (file === `${prefix}cli-cases.tsv`) {
    return passed.has(`cli/${text.split("\t")[0]!}`);
  }
  if (file.endsWith(".tsv") || file.endsWith(".md")) {
    return false;
  }
  return passed.has(file.slice(prefix.length));
}

export function coverage(
  corpus: Corpus,
  index: RefIndex,
  compilerPasses?: ReadonlySet<string>,
): Coverage {
  const cited = new Set(
    index.citations.filter((item) => isFixtureCitation(item.file)).map((item) => item.id),
  );
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
    rules: chapters.reduce((sum, value) => sum + value.rules, 0),
    cited: chapters.reduce((sum, value) => sum + value.cited, 0),
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
      (item) =>
        item.chapter === uncovered ||
        item.chapter.replace(/^(?:lang|std|cli)\//, "").startsWith(uncovered),
    );
    if (found.length === 0) {
      throw new Error(`no chapter matches ${uncovered}`);
    }
    return found.flatMap((item) => item.compilerUncovered ?? item.uncovered).join("\n") + "\n";
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
    ...result.chapters.map(
      (item) => [item.chapter, item.rules, item.cited, share(item.cited, item.rules)] as const,
    ),
    ["total", result.rules, result.cited, share(result.cited, result.rules)] as const,
  ];
  const widths = [
    Math.max("Chapter".length, ...rows.map((item) => item[0].length)),
    Math.max("Rules".length, ...rows.map((item) => String(item[1]).length)),
    Math.max("Cited".length, ...rows.map((item) => String(item[2]).length)),
    Math.max("Share".length, ...rows.map((item) => item[3].length)),
  ];
  const line = (chapter: string, rules: string, cited: string, shareText: string): string =>
    `${chapter.padEnd(widths[0]!)}  ${rules.padStart(widths[1]!)}  ${cited.padStart(widths[2]!)}  ${shareText.padStart(widths[3]!)}`;
  const lines = [
    line("Chapter", "Rules", "Cited", "Share"),
    ...rows.map((item) => line(item[0], String(item[1]), String(item[2]), item[3])),
  ];
  return `${lines.join("\n")}\n`;
}
