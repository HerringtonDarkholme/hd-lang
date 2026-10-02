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
import { CHAPTER_HISTORY_PATHS } from "./spec-prose.ts";

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

/** Words that mark a line as a record of history rather than a live citation. */
const HISTORY =
  /\b(?:retired|retires|retiring|since (?:moved|removed|replaced|renamed|revised)|withdrawn|formerly|former|previously|was removed|were removed|replaced by|renamed|reverses|reversed|becomes|became)\b/i;

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
): Citation[] {
  const out: Citation[] = [];
  const lines = citableLines(file, text);
  lines.forEach((content, index) => {
    if (content === "") return;
    const line = index + 1;
    // Prose wraps, so the history words may sit on a neighboring line.
    const nearby = lines.slice(Math.max(0, index - 1), index + 2).join(" ");
    const history = HISTORY.test(nearby);
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

/** Rule IDs the chapters carried in git history, and the commit that removed each. */
export interface RuleHistory {
  /** Every rule ID the chapters defined at some commit. */
  readonly ids: ReadonlySet<string>;
  /**
   * For each ID, the newest commit that removed its definition without
   * adding it back, as `<short hash> "<subject>"`.
   */
  readonly removedIn: ReadonlyMap<string, string>;
}

/**
 * The rule IDs of the chapters in the history of `rev` (HEAD by default),
 * read in one `git log -p` pass; empty when git is unavailable.
 */
export function ruleHistory(repoRoot: string, rev = "HEAD"): RuleHistory {
  let log: string;
  try {
    log = execFileSync(
      "git",
      [
        "-C",
        repoRoot,
        "log",
        rev,
        "--format=%x00%h %s",
        "-p",
        "--no-ext-diff",
        "-U0",
        "--",
        ...CHAPTER_HISTORY_PATHS,
      ],
      { encoding: "utf8", maxBuffer: 1 << 28, stdio: ["ignore", "pipe", "ignore"] },
    );
  } catch {
    return { ids: new Set(), removedIn: new Map() };
  }
  const ids = new Set<string>();
  const removedIn = new Map<string, string>();
  const definition = new RegExp(String.raw`r\[(${ID})\]`, "g");
  // git log lists the newest commit first, so the first removal seen is the last one.
  for (const commit of log.split("\0").slice(1)) {
    const newline = commit.indexOf("\n");
    const header = newline < 0 ? commit : commit.slice(0, newline);
    const space = header.indexOf(" ");
    const label = `${header.slice(0, space)} "${header.slice(space + 1)}"`;
    const removed = new Set<string>();
    const added = new Set<string>();
    for (const line of commit.split("\n")) {
      const sign = line[0];
      if ((sign !== "-" && sign !== "+") || line.startsWith("---") || line.startsWith("+++"))
        continue;
      for (const match of line.matchAll(definition)) {
        ids.add(match[1]!);
        (sign === "-" ? removed : added).add(match[1]!);
      }
    }
    for (const id of removed) if (!added.has(id) && !removedIn.has(id)) removedIn.set(id, label);
  }
  return { ids, removedIn };
}

/** Every rule ID the chapters carried in the history of `rev`, from git. */
export function historicalIds(repoRoot: string, rev = "HEAD"): Set<string> {
  return new Set(ruleHistory(repoRoot, rev).ids);
}

export interface RefIndex {
  /** Live rule IDs and where each is defined, as `spec/<chapter>:<line>`. */
  readonly live: ReadonlyMap<string, string>;
  readonly history: RuleHistory;
  readonly citations: readonly Citation[];
}

export function buildIndex(corpus: Corpus, repoRoot: string): RefIndex {
  const live = new Map<string, string>();
  for (const { chapter, rule } of allRules(corpus))
    live.set(rule.id, `spec/${chapter.name}:${rule.line}`);
  const history = ruleHistory(repoRoot);
  const isRuleId = (id: string): boolean => live.has(id) || history.ids.has(id);
  const citations = scannedFiles(repoRoot).flatMap((file) =>
    citationsIn(file, readFileSync(resolve(repoRoot, file), "utf8"), areaOf(file)!, isRuleId),
  );
  return { live, history, citations };
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

/** How a missing rule ID left the specification, from git history. */
function retiredLabel(index: RefIndex, id: string): string {
  const commit = index.history.removedIn.get(id);
  if (commit) return `retired in commit ${commit}`;
  if (index.history.ids.has(id)) return "retired (in git history)";
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
