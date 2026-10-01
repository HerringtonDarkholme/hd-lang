// `spec refs`: where the repository cites a rule, and which citations are dead.
//
// A citation is a `#r-<id>` anchor (or a bare `r-<id>`), or a bare rule ID
// such as `data.embed.width`. A bare ID counts only when it is a rule ID now,
// or was one in the history of the chapters, so a field access in a comment
// is not mistaken for a citation. Sources are read as text: Markdown prose
// outside code fences, hd and TypeScript comments, and TSV rows. Nothing
// under src/ is imported.
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { basename, relative, resolve } from "node:path";

import { allRules, type Corpus } from "./spec-corpus.ts";
import { CHAPTER_PREFIXES } from "./spec-prose.ts";

/** Where a citation lives; the first four are gated by spec/check.sh. */
type Area = "spec" | "fixtures" | "guide" | "lib-std" | "records" | "src";
const GATED_AREAS: ReadonlySet<Area> = new Set(["spec", "fixtures", "guide", "lib-std"]);
const AREAS: readonly Area[] = ["spec", "fixtures", "guide", "lib-std", "records", "src"];

export interface Citation {
  /** Repository-relative path. */
  readonly file: string;
  readonly line: number;
  readonly id: string;
  /** `anchor` for `#r-<id>` or `r-<id>`, `id` for a bare rule ID. */
  readonly form: "anchor" | "id";
  /** The Markdown file an anchor names, as written; empty when none. */
  readonly target: string;
  readonly area: Area;
  /** Whether the line records history, such as "(since retired)". */
  readonly history: boolean;
  readonly text: string;
}

const ID = String.raw`[a-z][a-z0-9]*(?:-[a-z0-9]+)*(?:\.[a-z][a-z0-9]*(?:-[a-z0-9]+)*)+`;
/** `path.md#r-id`, `#r-id`, or a bare `r-id`. */
const ANCHOR = new RegExp(String.raw`(?:([\w./-]*\.md)?#|(?<![\w#-]))r-(${ID})`, "g");
/** A bare ID: not part of a path, an anchor, a member chain, or a call, and not a `.*` wildcard. */
const BARE = new RegExp(String.raw`(?<![\w#./@$-])(${ID})(?![\w(-]|\.[\w*])`, "g");

const SEGMENT = String.raw`[a-z][a-z0-9]*(?:-[a-z0-9]+)*`;
const FULL_ID = new RegExp(`^${ID}$`);
const WILDCARD = new RegExp(`^${SEGMENT}(?:\\.${SEGMENT})*\\.\\*$`);
const RELATIVE_ID = new RegExp(`^(?:\\.${SEGMENT})+$`);

/** Words that mark a line as a record of history rather than a live citation. */
const HISTORY =
  /\b(?:retired|retires|retiring|since (?:moved|removed|replaced|renamed|revised)|withdrawn|formerly|former|previously|was removed|were removed|replaced by|renamed|reverses|reversed|becomes|became)\b/i;

/** Whether a repository-relative path is an archived record, which is history throughout. */
export function isArchived(file: string): boolean {
  return file.startsWith("future-work/archive/");
}

/** The area of a repository-relative path, or undefined when refs does not scan it. */
export function areaOf(file: string): Area | undefined {
  if (file.startsWith("spec/conformance/"))
    return /\.(?:hd|tsv|md)$/.test(file) ? "fixtures" : undefined;
  if (file.startsWith("spec/tools/") || file.startsWith("spec/reference-parser/"))
    return file.endsWith(".md") ? "spec" : undefined;
  if (file.startsWith("spec/")) return file.endsWith(".md") ? "spec" : undefined;
  if (file.startsWith("guide/")) return file.endsWith(".md") ? "guide" : undefined;
  if (file.startsWith("lib/std/")) return file.endsWith(".hd") ? "lib-std" : undefined;
  if (file.startsWith("future-work/")) return file.endsWith(".md") ? "records" : undefined;
  if (file === "test/portable/KNOWN_FAILURES.tsv") return "records";
  if (file.startsWith("src/")) return /\.(?:ts|md)$/.test(file) ? "src" : undefined;
  return undefined;
}

/** The scannable text of each line: prose, comments, or table cells; "" for code. */
function citableLines(file: string, text: string): string[] {
  const lines = text.split(/\r?\n/);
  if (file.endsWith(".md")) {
    let fence: string | undefined;
    return lines.map((line) => {
      const open = /^\s*(`{3,}|~{3,})/.exec(line);
      if (fence === undefined) {
        if (open) fence = open[1]!;
        return open ? "" : line;
      }
      if (open && open[1]![0] === fence[0] && open[1]!.length >= fence.length) fence = undefined;
      return "";
    });
  }
  if (file.endsWith(".hd"))
    return lines.map((line) => line.slice(Math.max(0, line.indexOf("#"))).replace(/^[^#].*$/, ""));
  if (file.endsWith(".ts")) {
    let block = false;
    return lines.map((line) => {
      const trimmed = line.trim();
      if (block) {
        if (trimmed.includes("*/")) block = false;
        return line;
      }
      if (trimmed.startsWith("/*")) {
        block = !trimmed.includes("*/");
        return line;
      }
      const comment = /(?:^|[^:"'`\\])\/\/(.*)$/.exec(line);
      return comment ? comment[1]! : "";
    });
  }
  return lines;
}

/** The citations in one file. `isRuleId` decides whether a bare token is a rule ID. */
export function citationsIn(
  file: string,
  text: string,
  area: Area,
  isRuleId: (id: string) => boolean,
  historyLines: (line: number) => boolean = () => false,
): Citation[] {
  const out: Citation[] = [];
  const lines = citableLines(file, text);
  lines.forEach((content, index) => {
    if (content === "") return;
    const line = index + 1;
    // Prose wraps, so the history words may sit on a neighboring line.
    const nearby = lines.slice(Math.max(0, index - 1), index + 2).join(" ");
    const history = HISTORY.test(nearby) || historyLines(line);
    const text = content.trim();
    const taken: [number, number][] = [];
    for (const match of content.matchAll(ANCHOR)) {
      taken.push([match.index, match.index + match[0].length]);
      const target = match[1] ?? "";
      // `[r-id](chapter.md#r-id)` cites once: skip the link text when its target follows.
      if (target === "" && /^`?\]\(/.test(content.slice(match.index + match[0].length))) continue;
      out.push({ file, line, id: match[2]!, form: "anchor", target, area, history, text });
    }
    const anchored = new Set(out.filter((c) => c.line === line).map((c) => c.id));
    for (const match of content.matchAll(BARE)) {
      if (taken.some(([start, end]) => match.index >= start && match.index < end)) continue;
      if (anchored.has(match[1]!) || !isRuleId(match[1]!)) continue;
      out.push({ file, line, id: match[1]!, form: "id", target: "", area, history, text });
    }
  });
  return out;
}

/** Every file refs scans, repository-relative and sorted. */
function scannedFiles(repoRoot: string): string[] {
  const roots = [
    "spec",
    "guide",
    "lib/std",
    "future-work",
    "src",
    "test/portable/KNOWN_FAILURES.tsv",
  ];
  const files: string[] = [];
  const walk = (path: string): void => {
    let stats;
    try {
      stats = statSync(path);
    } catch {
      return;
    }
    if (stats.isFile()) {
      const file = relative(repoRoot, path);
      if (areaOf(file)) files.push(file);
      return;
    }
    for (const entry of readdirSync(path))
      if (entry !== "node_modules" && !entry.startsWith(".")) walk(resolve(path, entry));
  };
  for (const root of roots) walk(resolve(repoRoot, root));
  return files.sort();
}

/**
 * Every rule ID the chapters carried in the history of `rev` (HEAD by
 * default), from git; empty when git is unavailable.
 */
export function historicalIds(repoRoot: string, rev = "HEAD"): Set<string> {
  try {
    const log = execFileSync(
      "git",
      [
        "-C",
        repoRoot,
        "log",
        rev,
        "--format=",
        "-p",
        "--no-ext-diff",
        "-U0",
        "--",
        "spec/[0-9][0-9]-*.md",
        "spec/std/*.md",
      ],
      { encoding: "utf8", maxBuffer: 1 << 28, stdio: ["ignore", "pipe", "ignore"] },
    );
    return new Set([...log.matchAll(new RegExp(String.raw`r\[(${ID})\]`, "g"))].map((m) => m[1]!));
  } catch {
    return new Set();
  }
}

/** The line range of spec/README.md's Revision Notes, whose citations are history by design. */
function revisionNotesRange(readme: string): [number, number] {
  const lines = readme.split("\n");
  const start = lines.findIndex((line) => /^## Revision Notes\s*$/.test(line));
  if (start < 0) return [0, -1];
  const next = lines.findIndex((line, index) => index > start && line.startsWith("## "));
  return [start + 1, next < 0 ? lines.length : next];
}

/**
 * For each rule ID a Revision Notes entry retires, a label for that entry:
 * "batch N" when the entry names one, else its opening words. Relative IDs
 * such as `.pack.always` after `lex.contextual.pack` and wildcards such as
 * `pack.*` are resolved best-effort; the last retiring entry wins.
 */
export function retirements(
  readme: string,
  prefixes: ReadonlySet<string>,
): { exact: Map<string, string>; wildcards: Map<string, string> } {
  const [start, end] = revisionNotesRange(readme);
  const lines = readme.split("\n").slice(start, end);
  const entries: { line: number; text: string }[] = [];
  lines.forEach((line, index) => {
    if (line.startsWith("- ")) entries.push({ line: start + index + 1, text: line.slice(2) });
    else if (entries.length > 0 && line.trim() !== "") entries.at(-1)!.text += ` ${line.trim()}`;
  });
  const exact = new Map<string, string>();
  const wildcards = new Map<string, string>();
  for (const entry of entries) {
    if (!/\bretire/i.test(entry.text)) continue;
    const batch = /\bbatch (\d+[a-z]?)/i.exec(entry.text)?.[1];
    const opening = entry.text
      .split(/ \(|: /, 1)[0]!
      .replaceAll("`", "")
      .split(/\s+/)
      .slice(0, 8)
      .join(" ");
    const label = batch ? `batch ${batch}` : `"${opening}"`;
    const where = `${label} (spec/README.md:${entry.line})`;
    let base: string[] | undefined;
    for (const match of entry.text.matchAll(/`([a-z.][a-z0-9.*-]*)`/g)) {
      const token = match[1]!;
      if (!token.startsWith(".") && !prefixes.has(token.split(".", 1)[0]!)) continue;
      if (WILDCARD.test(token)) wildcards.set(token.slice(0, -2), where);
      else if (FULL_ID.test(token)) {
        base = token.split(".");
        exact.set(token, where);
      } else if (base && RELATIVE_ID.test(token)) {
        const tail = token.slice(1).split(".");
        const at = base.indexOf(tail[0]!);
        const head =
          at > 0 ? base.slice(0, at) : base.slice(0, Math.max(1, base.length - tail.length));
        exact.set([...head, ...tail].join("."), where);
      }
    }
  }
  return { exact, wildcards };
}

export interface RefIndex {
  /** Live rule IDs and where each is defined, as `spec/<chapter>:<line>`. */
  readonly live: ReadonlyMap<string, string>;
  readonly historical: ReadonlySet<string>;
  readonly retired: ReturnType<typeof retirements>;
  readonly citations: readonly Citation[];
}

export function buildIndex(corpus: Corpus, repoRoot: string): RefIndex {
  const live = new Map<string, string>();
  for (const { chapter, rule } of allRules(corpus))
    live.set(rule.id, `spec/${chapter.name}:${rule.line}`);
  const historical = historicalIds(repoRoot);
  const prefixes = new Set([
    ...Object.values(CHAPTER_PREFIXES),
    ...[...historical].map((id) => id.split(".", 1)[0]!),
  ]);
  const retired = retirements(corpus.readme, prefixes);
  const isRuleId = (id: string): boolean =>
    live.has(id) || historical.has(id) || retired.exact.has(id);
  const [notesStart, notesEnd] = revisionNotesRange(corpus.readme);
  const citations = scannedFiles(repoRoot).flatMap((file) => {
    const area = areaOf(file)!;
    const notes =
      file === "spec/README.md"
        ? (line: number) => line >= notesStart && line <= notesEnd
        : isArchived(file)
          ? () => true
          : undefined;
    return citationsIn(file, readFileSync(resolve(repoRoot, file), "utf8"), area, isRuleId, notes);
  });
  return { live, historical, retired, citations };
}

/** Why a citation is dead, or undefined when it resolves. */
function deadReason(index: RefIndex, citation: Citation): string | undefined {
  const home = index.live.get(citation.id);
  if (home) {
    if (citation.target === "") return undefined;
    const chapter = home.slice("spec/".length, home.lastIndexOf(":"));
    if (basename(citation.target) === basename(chapter)) return undefined;
    return `anchor names ${citation.target}, but the rule is in spec/${chapter}`;
  }
  return retiredLabel(index, citation.id);
}

/** How a missing rule ID left the specification. */
function retiredLabel(index: RefIndex, id: string): string {
  const exact = index.retired.exact.get(id);
  if (exact) return `retired in ${exact}`;
  for (const [prefix, where] of index.retired.wildcards)
    if (id === prefix || id.startsWith(`${prefix}.`)) return `retired in ${where}`;
  if (index.historical.has(id)) return "retired (in git history; no Revision Notes entry names it)";
  return "never a rule ID in the chapters' history";
}

interface DeadCitation extends Citation {
  readonly reason: string;
  /** spec/check.sh fails on this one: a gated area and not history. */
  readonly failing: boolean;
}

export function deadCitations(index: RefIndex): DeadCitation[] {
  const out: DeadCitation[] = [];
  for (const citation of index.citations) {
    const reason = deadReason(index, citation);
    if (reason === undefined) continue;
    out.push({ ...citation, reason, failing: GATED_AREAS.has(citation.area) && !citation.history });
  }
  return out;
}

export function refsReport(index: RefIndex, id: string): string {
  const found = index.citations.filter((citation) => citation.id === id);
  const home = index.live.get(id);
  const out = [
    home
      ? `${id}: defined at ${home}`
      : `${id}: not in the current spec; ${retiredLabel(index, id)}`,
  ];
  for (const citation of found)
    out.push(
      `${citation.file}:${citation.line}: ${citation.area}, ${citation.form}${citation.history ? ", history" : ""}: ${citation.text.slice(0, 120)}`,
    );
  const byArea = AREAS.map((area) => [area, found.filter((c) => c.area === area).length] as const)
    .filter(([, n]) => n > 0)
    .map(([area, n]) => `${area} ${n}`);
  out.push(
    `${found.length} citation${found.length === 1 ? "" : "s"}${byArea.length ? ` (${byArea.join(", ")})` : ""}`,
  );
  return `${out.join("\n")}\n`;
}

export function deadReport(
  dead: readonly DeadCitation[],
  options: { brief: boolean; all: boolean },
): string {
  const out: string[] = [];
  for (const citation of dead) {
    const severity = citation.failing ? "error" : citation.history ? "allowed history" : "warning";
    if (citation.history && !options.all) continue;
    if (options.brief && !citation.failing) continue;
    out.push(
      `${citation.file}:${citation.line}: ${severity}: dead ${citation.form} ${citation.id}: ${citation.reason}`,
    );
  }
  const count = (area: Area, test: (c: DeadCitation) => boolean): number =>
    dead.filter((c) => c.area === area && test(c)).length;
  const summary = (test: (c: DeadCitation) => boolean): string =>
    AREAS.map((area) => `${area} ${count(area, test)}`).join(", ");
  const failing = dead.filter((c) => c.failing).length;
  const warnings = dead.filter((c) => !c.failing && !c.history).length;
  const history = dead.filter((c) => c.history).length;
  out.push(
    `dead citations: ${failing} failing (${summary((c) => c.failing)})`,
    `dead citations: ${warnings} warnings (${summary((c) => !c.failing && !c.history)})`,
    `dead citations: ${history} allowed as history (${summary((c) => c.history)})`,
  );
  return `${out.join("\n")}\n`;
}
