// `spec rewrite`: the before/after check a batch runs after a spec pass.
//
// It compares the specification at a base git revision with the working tree
// (or with a second revision) and reports rule counts, retired and added rule
// IDs, IDs kept with changed text, lost diagnostic codes, lost examples, and
// retired IDs that the repository still cites. Example matching is
// rule-inventory.ts's diff(); citations come from spec-refs.ts.
import { diff, type ExampleStatus, type Inventory, textSimilarity } from "./rule-inventory.ts";
import { allRules, type Corpus, type Tier } from "./spec-corpus.ts";
import { counts } from "./spec-counts.ts";
import { buildIndex, deadCitations } from "./spec-refs.ts";

/** What `--fail-on` may name. */
export const FAIL_KINDS = ["lost-codes", "lost-examples", "reused-ids"] as const;
export type FailKind = (typeof FAIL_KINDS)[number];

interface CountRow {
  readonly before: number;
  readonly after: number;
  readonly delta: number;
}

interface ChapterCountRow extends CountRow {
  readonly chapter: string;
  readonly tier: Tier;
}

interface PlacedId {
  readonly id: string;
  /** The chapter under spec/ that holds (or held) the rule. */
  readonly chapter: string;
}

interface ChangedRule {
  readonly id: string;
  readonly chapter: string;
  readonly before: string;
  readonly after: string;
  /** Word overlap of the two texts, 0 to 1; a low score suggests a new meaning. */
  readonly similarity: number;
}

interface LostExample {
  readonly chapter: string;
  /** The example's position among the chapter's code fences at the base. */
  readonly index: number;
  readonly line: number;
  /** Base lines found in no example after the rewrite. */
  readonly missing: string[];
}

type RewriteExampleStatus = ExampleStatus | "moved";

interface CitedRetired {
  readonly id: string;
  readonly file: string;
  readonly line: number;
  /** `error` fails spec/check.sh; `warning` is in records or src/; `history` is allowed. */
  readonly severity: "error" | "warning" | "history";
}

interface Rewrite {
  readonly base: string;
  readonly head: string;
  readonly chapters: ChapterCountRow[];
  readonly tiers: Record<Tier, CountRow>;
  readonly total: CountRow;
  readonly retired: PlacedId[];
  readonly added: PlacedId[];
  /** IDs that changed chapters; neither retired nor added. */
  readonly moved: { id: string; from: string; to: string }[];
  /** IDs kept with changed rule text; STYLE.md gives a rule whose meaning changes a new ID. */
  readonly changed: ChangedRule[];
  /** Added IDs that the chapters used before the base: a retired ID reused. */
  readonly revived: string[];
  /** Codes that the chapters name at the base and no longer name. */
  readonly lostCodes: string[];
  readonly addedCodes: string[];
  /** Codes and panic categories that left the README Diagnostics table or Control Flow. */
  readonly lostTableCodes: string[];
  readonly addedTableCodes: string[];
  readonly examples: {
    readonly before: number;
    readonly after: number;
    readonly statuses: Record<RewriteExampleStatus, number>;
    readonly lost: LostExample[];
  };
  /** Citations of retired IDs, in the working tree. */
  readonly citedRetired: CitedRetired[];
}

const row = (before: number, after: number): CountRow => ({
  before,
  after,
  delta: after - before,
});

/** An inventory without its sentences, so diff() pairs only examples, codes, and IDs. */
const lean = (inventory: Inventory): Inventory => ({
  ...inventory,
  normative: [],
  sentences: [],
});

const normalize = (text: string): string => text.replaceAll(/\s+/g, " ").trim();

function namedCodes(corpus: Corpus): Set<string> {
  return new Set(corpus.chapters.flatMap((chapter) => Object.keys(chapter.inventory.codes)));
}

const minus = <T>(a: Iterable<T>, b: ReadonlySet<T>): T[] => [...a].filter((x) => !b.has(x));

interface RewriteOptions {
  readonly base: Corpus;
  readonly head: Corpus;
  readonly baseLabel: string;
  readonly headLabel: string;
  /** The repository whose files are scanned for citations; none when undefined. */
  readonly repoRoot?: string;
  /**
   * Rule IDs in the chapters' history up to the base, as spec-refs.ts
   * historicalIds gives them; undefined skips the reuse check.
   */
  readonly earlierIds?: ReadonlySet<string>;
}

/** Chapter order in a report: spec/lang/, then spec/std/, then spec/cli/. */
function tierOrder(chapter: string): number {
  return chapter.startsWith("std/") ? 1 : chapter.startsWith("cli/") ? 2 : 0;
}

export function rewrite(options: RewriteOptions): Rewrite {
  const { base, head } = options;
  const before = counts(base);
  const after = counts(head);
  const names = [
    ...new Set([...before.chapters, ...after.chapters].map((entry) => entry.chapter)),
  ].sort((a, b) => tierOrder(a) - tierOrder(b) || a.localeCompare(b));
  const chapters = names.map((chapter) => {
    const old = before.chapters.find((entry) => entry.chapter === chapter);
    const next = after.chapters.find((entry) => entry.chapter === chapter);
    return {
      chapter,
      tier: (old ?? next)!.tier,
      ...row(old?.rules ?? 0, next?.rules ?? 0),
    };
  });

  const oldRules = new Map(allRules(base).map(({ chapter, rule }) => [rule.id, { chapter, rule }]));
  const newRules = new Map(allRules(head).map(({ chapter, rule }) => [rule.id, { chapter, rule }]));
  const retired = [...oldRules]
    .filter(([id]) => !newRules.has(id))
    .map(([id, { chapter }]) => ({ id, chapter: chapter.name }));
  const added = [...newRules]
    .filter(([id]) => !oldRules.has(id))
    .map(([id, { chapter }]) => ({ id, chapter: chapter.name }));
  const moved: Rewrite["moved"] = [];
  const changed: ChangedRule[] = [];
  for (const [id, old] of oldRules) {
    const next = newRules.get(id);
    if (!next) continue;
    if (next.chapter.name !== old.chapter.name)
      moved.push({ id, from: old.chapter.name, to: next.chapter.name });
    const was = normalize(old.rule.text);
    const now = normalize(next.rule.text);
    if (was !== now)
      changed.push({
        id,
        chapter: next.chapter.name,
        before: was,
        after: now,
        similarity: Math.round(textSimilarity(was, now) * 100) / 100,
      });
  }
  changed.sort((a, b) => a.similarity - b.similarity || a.id.localeCompare(b.id));
  const revived = options.earlierIds
    ? added.map((entry) => entry.id).filter((id) => options.earlierIds!.has(id))
    : [];

  const namedBefore = namedCodes(base);
  const namedAfter = namedCodes(head);

  // Examples: each base chapter against the same chapter, then a lost one
  // against every chapter after the rewrite, which finds an example that moved.
  const combined: Inventory = lean({
    ...(head.chapters[0]?.inventory ?? base.chapters[0]!.inventory),
    examples: head.chapters
      .flatMap((chapter) => chapter.inventory.examples)
      .map((example, index) => ({ ...example, index: index + 1 })),
    rules: [],
    codes: {},
  });
  const statuses: Record<RewriteExampleStatus, number> = {
    unchanged: 0,
    contained: 0,
    split: 0,
    "comment-edited": 0,
    moved: 0,
    lost: 0,
  };
  const lost: LostExample[] = [];
  for (const chapter of base.chapters) {
    const same = head.chapters.find((entry) => entry.name === chapter.name);
    const own = same
      ? diff(lean(chapter.inventory), lean(same.inventory)).examples
      : chapter.inventory.examples.map((example) => ({
          old: example.index,
          status: "lost" as ExampleStatus,
          into: [],
          missing: [],
        }));
    const anywhere = own.some((match) => match.status === "lost")
      ? diff(lean(chapter.inventory), combined).examples
      : [];
    for (const match of own) {
      if (match.status !== "lost") {
        statuses[match.status] += 1;
        continue;
      }
      const elsewhere = anywhere.find((entry) => entry.old === match.old)!;
      if (elsewhere.status !== "lost") {
        statuses.moved += 1;
        continue;
      }
      statuses.lost += 1;
      const example = chapter.inventory.examples[match.old - 1]!;
      lost.push({
        chapter: chapter.name,
        index: match.old,
        line: example.line,
        missing: elsewhere.missing,
      });
    }
  }

  const retiredIds = new Set(retired.map((entry) => entry.id));
  const citedRetired: CitedRetired[] =
    options.repoRoot === undefined || retiredIds.size === 0
      ? []
      : deadCitations(buildIndex(head, options.repoRoot))
          .filter((citation) => retiredIds.has(citation.id))
          .map((citation) => ({
            id: citation.id,
            file: citation.file,
            line: citation.line,
            severity: citation.failing ? "error" : citation.history ? "history" : "warning",
          }));

  return {
    base: options.baseLabel,
    head: options.headLabel,
    chapters,
    tiers: {
      language: row(before.tiers.language, after.tiers.language),
      std: row(before.tiers.std, after.tiers.std),
      cli: row(before.tiers.cli, after.tiers.cli),
    },
    total: row(before.total, after.total),
    retired,
    added,
    moved,
    changed,
    revived,
    lostCodes: minus(namedBefore, namedAfter).sort(),
    addedCodes: minus(namedAfter, namedBefore).sort(),
    lostTableCodes: minus(base.known, head.known).sort(),
    addedTableCodes: minus(head.known, base.known).sort(),
    examples: {
      before: base.chapters.reduce((sum, c) => sum + c.inventory.examples.length, 0),
      after: head.chapters.reduce((sum, c) => sum + c.inventory.examples.length, 0),
      statuses,
      lost,
    },
    citedRetired,
  };
}

/** The `--fail-on` kinds that the rewrite trips. */
export function failures(result: Rewrite, kinds: readonly FailKind[]): FailKind[] {
  return kinds.filter((kind) => {
    if (kind === "lost-codes") return result.lostCodes.length + result.lostTableCodes.length > 0;
    if (kind === "lost-examples") return result.examples.lost.length > 0;
    return result.changed.length + result.revived.length > 0;
  });
}

const number = (value: number): string => value.toLocaleString("en-US");
const signed = (value: number): string => (value > 0 ? `+${number(value)}` : number(value));
const change = (entry: CountRow): string =>
  `${number(entry.before)} -> ${number(entry.after)} (${signed(entry.delta)})`;
const ids = (list: readonly string[]): string =>
  list.length === 0 ? "none" : list.map((id) => `\`${id}\``).join(", ");

/** The one-line summary a batch report quotes. */
export function rewriteSummary(result: Rewrite): string {
  const cited = result.citedRetired.filter((c) => c.severity !== "history").length;
  return [
    `rules: language ${change(result.tiers.language)}, stdlib ${change(result.tiers.std)}, cli ${change(result.tiers.cli)}`,
    `IDs: ${result.retired.length} retired, ${result.added.length} added, ${result.moved.length} moved, ${result.changed.length} changed text, ${result.revived.length} reused`,
    `codes: ${result.lostCodes.length + result.lostTableCodes.length} lost, ${result.addedCodes.length} added`,
    `examples: ${result.examples.lost.length} lost`,
    `retired IDs still cited: ${cited}`,
  ].join("; ");
}

export function rewriteReport(result: Rewrite): string {
  const out: string[] = [`Spec rewrite: ${result.base} -> ${result.head}`, ""];
  const width = Math.max(7, ...result.chapters.map((entry) => entry.chapter.length));
  const cell = (value: string | number, size: number): string => String(value).padStart(size);
  out.push(
    `${"Chapter".padEnd(width)}  Tier      Before  After  Delta`,
    `${"-".repeat(width)}  --------  ------  -----  -----`,
    ...result.chapters.map(
      (entry) =>
        `${entry.chapter.padEnd(width)}  ${entry.tier.padEnd(8)}  ${cell(number(entry.before), 6)}  ${cell(number(entry.after), 5)}  ${cell(signed(entry.delta), 5)}`,
    ),
    "",
    `Language tier: ${change(result.tiers.language)}`,
    `Stdlib tier: ${change(result.tiers.std)}`,
    `CLI tier: ${change(result.tiers.cli)}`,
    `Total: ${change(result.total)}`,
    "",
    `Retired IDs (${result.retired.length}): ${ids(result.retired.map((entry) => entry.id))}`,
    `Added IDs (${result.added.length}): ${ids(result.added.map((entry) => entry.id))}`,
  );
  if (result.moved.length > 0)
    out.push(
      `Moved IDs (${result.moved.length}): ${result.moved.map((m) => `\`${m.id}\` (${m.from} -> ${m.to})`).join(", ")}`,
    );
  out.push(
    `Reused IDs, retired before the base (${result.revived.length}): ${ids(result.revived)}`,
    `IDs kept with changed text (${result.changed.length}); STYLE.md gives a rule whose meaning changes a new ID:`,
  );
  for (const entry of result.changed)
    out.push(
      `  ${entry.id} (${entry.chapter}, similarity ${entry.similarity})`,
      `    before: ${entry.before}`,
      `    after:  ${entry.after}`,
    );
  out.push(
    "",
    `Diagnostic codes named by rules: lost ${ids(result.lostCodes)}; added ${ids(result.addedCodes)}`,
    `Diagnostics table and panic categories: lost ${ids(result.lostTableCodes)}; added ${ids(result.addedTableCodes)}`,
    "",
    `Examples: ${number(result.examples.before)} -> ${number(result.examples.after)}; ${Object.entries(
      result.examples.statuses,
    )
      .map(([status, n]) => `${status} ${n}`)
      .join(", ")}`,
  );
  for (const entry of result.examples.lost)
    out.push(
      `  lost: ${entry.chapter} example ${entry.index} (line ${entry.line})`,
      ...entry.missing.map((line) => `    ${line.trim()}`),
    );
  const live = result.citedRetired.filter((c) => c.severity !== "history");
  out.push(
    "",
    `Retired IDs still cited in the working tree: ${live.length}` +
      ` (${result.citedRetired.length - live.length} more allowed as history)`,
    ...live.map((c) => `  ${c.file}:${c.line}: ${c.severity}: ${c.id}`),
    "",
    `Summary: ${rewriteSummary(result)}`,
  );
  return `${out.join("\n")}\n`;
}
