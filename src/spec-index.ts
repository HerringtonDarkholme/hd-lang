import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// An index of the specification Markdown for agent-facing tooling: which rule
// IDs name which diagnostic codes, what each code means, and where the
// chapters and conformance fixtures mention it. It is built from the spec
// sources every time it is loaded, never from a hand-kept table, so it grows
// as chapters gain rule IDs (spec/STYLE.md#rule-ids on the chapters that have
// them, spec/README.md#diagnostics for the code tables).
//
// A rule ID marker is `r[`, a dotted ID, `]`, and a space, opening a list
// item, paragraph, block quote, or table cell. A rule names a diagnostic code
// with "Error: `code`." (or "Warning: `code`."), or with "is a `code` error".

/** A rule ID marker and the rule text that follows it. */
interface SpecRule {
  readonly id: string;
  /** Chapter path under `spec/`, such as `lang/08-data-and-enums.md`. */
  readonly file: string;
  /** Repository-relative citation, such as `spec/lang/08-data-and-enums.md#r-data.field.unique`. */
  readonly anchor: string;
  readonly line: number;
  readonly text: string;
  /** Codes the rule names as its diagnostic. */
  readonly codes: readonly string[];
}

/** A prose block in a chapter that mentions a code in inline code. */
export interface SpecMention {
  readonly file: string;
  readonly heading: string;
  /** Repository-relative heading citation, such as `spec/lang/05-expressions.md#data-literals`. */
  readonly anchor: string;
  readonly line: number;
  /** The rule ID the block carries, when it opens with a marker. */
  readonly rule?: string;
}

/** A conformance fixture row whose expectation names a code. */
interface SpecFixture {
  readonly path: string;
  readonly phase: string;
  readonly expectation: string;
  readonly specification: string;
}

type CodeCategory = "error" | "error (general)" | "warning" | "runtime panic" | "boundary failure";

interface CodeEntry {
  readonly code: string;
  readonly category: CodeCategory;
  /** The normative meaning from the spec/README.md table, when it has a row. */
  readonly meaning?: string;
}

export interface SpecIndex {
  readonly codes: ReadonlyMap<string, CodeEntry>;
  readonly rules: readonly SpecRule[];
  /** Rules naming each code as their diagnostic. */
  readonly rulesByCode: ReadonlyMap<string, readonly SpecRule[]>;
  readonly mentions: ReadonlyMap<string, readonly SpecMention[]>;
  readonly fixtures: ReadonlyMap<string, readonly SpecFixture[]>;
}

interface Block {
  readonly kind: "heading" | "text" | "row" | "code";
  readonly text: string;
  readonly line: number;
}

const FENCE = /^\s*(`{3,}|~{3,})/;
const HEADING = /^(#{1,6})\s+(.+?)\s*#*\s*$/;
const ITEM = /^\s*(?:[-*+]|\d+[.)])\s+/;
const TABLE_RULE = /^\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)*\|?\s*$/;
const MARKER = /^r\[([a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)+)\](?:\s+|$)/;
const CODE = /`([a-z][a-z0-9]*(?:-[a-z0-9]+)*)`/g;
const CODE_LIST = /`[a-z][a-z0-9-]*`(?:\s*(?:,|,?\s*or|,?\s*and)\s*`[a-z][a-z0-9-]*`)*/;
const NAMED_DIAGNOSTIC = new RegExp(
  String.raw`\b(?:Error|Warning):\s*(${CODE_LIST.source})|\bis an? (\x60[a-z][a-z0-9-]*\x60) (?:error|warning)\b`,
  "g",
);

/** A small line-based block scanner for the constructs the chapters use. */
function blocks(markdown: string): Block[] {
  const lines = markdown.split(/\r?\n/);
  const result: Block[] = [];
  let open: { parts: string[]; line: number } | undefined;
  const flush = (): void => {
    if (open) result.push({ kind: "text", text: open.parts.join(" "), line: open.line });
    open = undefined;
  };
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]!;
    const fence = FENCE.exec(line);
    if (fence) {
      flush();
      const start = index;
      const body: string[] = [];
      for (index += 1; index < lines.length; index += 1) {
        if (lines[index]!.trim().startsWith(fence[1]!)) break;
        body.push(lines[index]!);
      }
      result.push({ kind: "code", text: body.join("\n"), line: start + 1 });
      continue;
    }
    if (line.trim() === "") {
      flush();
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      flush();
      result.push({ kind: "heading", text: heading[2]!, line: index + 1 });
      continue;
    }
    if (line.trimStart().startsWith("|")) {
      flush();
      if (!TABLE_RULE.test(line.trim())) result.push({ kind: "row", text: line, line: index + 1 });
      continue;
    }
    const quote = /^\s*>\s?(.*)$/.exec(line);
    const text = quote ? quote[1]! : line;
    const item = ITEM.exec(text);
    if (item) {
      flush();
      open = { parts: [text.slice(item[0].length).trim()], line: index + 1 };
      continue;
    }
    if (!open) open = { parts: [], line: index + 1 };
    open.parts.push(text.trim());
  }
  flush();
  return result;
}

/** Table cells of one Markdown row, keeping `\|` and pipes inside inline code. */
function cells(row: string): string[] {
  const body = row.trim().replace(/^\|/, "").replace(/\|$/, "");
  const result: string[] = [];
  let current = "";
  let code = false;
  for (let index = 0; index < body.length; index += 1) {
    const char = body[index]!;
    if (char === "\\" && body[index + 1] === "|") {
      current += "|";
      index += 1;
    } else if (char === "`") {
      code = !code;
      current += char;
    } else if (char === "|" && !code) {
      result.push(current.trim());
      current = "";
    } else current += char;
  }
  result.push(current.trim());
  return result;
}

/** The anchor GitHub and the website give a heading (spec/check-spec-anchors.ts). */
function headingSlug(text: string): string {
  return text
    .replaceAll(/<[^>]+>/g, "")
    .trim()
    .toLowerCase()
    .replaceAll(/[`*_~]/g, "")
    .replaceAll(/[^\p{L}\p{N}_\- ]/gu, "")
    .replaceAll(" ", "-");
}

/** The codes a rule's text names as its diagnostic. */
export function namedCodes(text: string): string[] {
  const codes: string[] = [];
  for (const match of text.matchAll(NAMED_DIAGNOSTIC))
    for (const code of (match[1] ?? match[2]!).matchAll(CODE))
      if (!codes.includes(code[1]!)) codes.push(code[1]!);
  return codes;
}

function push<T>(map: Map<string, T[]>, key: string, value: T): void {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

function readmeCodes(readme: string, controlFlow: string): Map<string, CodeEntry> {
  const codes = new Map<string, CodeEntry>();
  const meanings = new Map<string, string>();
  for (const block of blocks(readme)) {
    if (block.kind !== "row") continue;
    const [first = "", second = ""] = cells(block.text);
    const category = /^(Error \(general\)|Error|Warning|Boundary failure)$/.exec(first)?.[1];
    if (category) {
      for (const match of second.matchAll(CODE))
        codes.set(match[1]!, {
          code: match[1]!,
          category: category.toLowerCase() as CodeCategory,
        });
      continue;
    }
    const meaning = /^`([a-z][a-z0-9-]*)`$/.exec(first);
    if (meaning && second) meanings.set(meaning[1]!, second);
  }
  const panics = /Stable panic categories are exactly([\s\S]*?)\.\s/.exec(controlFlow);
  if (panics)
    for (const match of panics[1]!.matchAll(CODE))
      if (!codes.has(match[1]!))
        codes.set(match[1]!, { code: match[1]!, category: "runtime panic" });
  for (const [code, meaning] of meanings) {
    const entry = codes.get(code);
    if (entry) codes.set(code, { ...entry, meaning });
  }
  return codes;
}

function fixtureRows(cases: string): Map<string, SpecFixture[]> {
  const fixtures = new Map<string, SpecFixture[]>();
  for (const line of cases.split(/\r?\n/).slice(1)) {
    const [path, phase, expectation, specification] = line.split("\t");
    if (!path || !phase || !expectation || specification === undefined) continue;
    const code = /^(?:reject|warn|panic):([a-z0-9-]+)$/.exec(expectation)?.[1];
    if (!code) continue;
    push(fixtures, code, {
      path: `spec/conformance/${path}`,
      phase,
      expectation,
      specification: specification ? `spec/${specification}` : "",
    });
  }
  return fixtures;
}

/**
 * Indexes the specification. `files` maps a path under `spec/` (`README.md`,
 * `lang/08-data-and-enums.md`, `conformance/cases.tsv`) to its text.
 */
export function indexSpec(files: Readonly<Record<string, string>>): SpecIndex {
  const codes = readmeCodes(files["README.md"] ?? "", files["lang/06-control-flow.md"] ?? "");
  const rules: SpecRule[] = [];
  const rulesByCode = new Map<string, SpecRule[]>();
  const mentions = new Map<string, SpecMention[]>();
  const chapters = Object.keys(files)
    .filter((path) => /^(?:lang\/\d\d-[^/]+|std\/(?!README\.md$)[^/]+)\.md$/.test(path))
    .sort();
  for (const file of chapters) {
    const slugs = new Map<string, number>();
    let heading = "";
    let anchor = `spec/${file}`;
    for (const block of blocks(files[file]!)) {
      if (block.kind === "code") continue;
      if (block.kind === "heading") {
        const base = headingSlug(block.text);
        const count = slugs.get(base) ?? 0;
        slugs.set(base, count + 1);
        heading = block.text;
        anchor = `spec/${file}#${count === 0 ? base : `${base}-${count}`}`;
        continue;
      }
      for (const text of block.kind === "row" ? cells(block.text) : [block.text]) {
        const marker = MARKER.exec(text);
        let rule: SpecRule | undefined;
        if (marker) {
          const body = text.slice(marker[0].length).trim();
          rule = {
            id: marker[1]!,
            file,
            anchor: `spec/${file}#r-${marker[1]}`,
            line: block.line,
            text: body,
            codes: namedCodes(body),
          };
          rules.push(rule);
          for (const code of rule.codes) push(rulesByCode, code, rule);
        }
        const seen = new Set<string>();
        for (const match of text.matchAll(CODE)) {
          const code = match[1]!;
          if (seen.has(code)) continue;
          seen.add(code);
          const mention: SpecMention = { file, heading, anchor, line: block.line };
          push(mentions, code, rule ? { ...mention, rule: rule.id } : mention);
        }
      }
    }
  }
  return {
    codes,
    rules,
    rulesByCode,
    mentions,
    fixtures: fixtureRows(files["conformance/cases.tsv"] ?? ""),
  };
}

/** The repository's `spec/` directory, or `HD_SPEC_DIR` when it is set. */
function specDirectory(): string {
  return process.env.HD_SPEC_DIR ?? fileURLToPath(new URL("../spec/", import.meta.url));
}

/** Reads and indexes the specification sources; missing files index as empty. */
export async function loadSpecIndex(directory = specDirectory()): Promise<SpecIndex> {
  const files: Record<string, string> = {};
  const markdown = async (subdirectory: string): Promise<string[]> => {
    try {
      return (await readdir(join(directory, subdirectory)))
        .filter((entry) => entry.endsWith(".md"))
        .map((entry) => (subdirectory === "." ? entry : `${subdirectory}/${entry}`));
    } catch {
      return [];
    }
  };
  // spec/README.md, the numbered language chapters in spec/lang/, and the
  // stdlib chapters in spec/std/.
  const names = [...(await markdown(".")), ...(await markdown("lang")), ...(await markdown("std"))];
  for (const name of [...names, "conformance/cases.tsv"])
    try {
      files[name] = await readFile(join(directory, name), "utf8");
    } catch {
      // A missing file contributes nothing to the index.
    }
  return indexSpec(files);
}

interface CodeExplanation {
  readonly code: string;
  readonly category?: CodeCategory;
  readonly meaning?: string;
  readonly rules: readonly SpecRule[];
  readonly mentions: readonly SpecMention[];
  readonly fixtures: readonly SpecFixture[];
}

/** What the specification says about `code`, or undefined when it never names it. */
export function explainCode(index: SpecIndex, code: string): CodeExplanation | undefined {
  const entry = index.codes.get(code);
  const rules = index.rulesByCode.get(code) ?? [];
  const mentions = index.mentions.get(code) ?? [];
  const fixtures = index.fixtures.get(code) ?? [];
  if (!entry && rules.length === 0 && fixtures.length === 0) return undefined;
  return {
    code,
    ...(entry ? { category: entry.category } : {}),
    ...(entry?.meaning ? { meaning: entry.meaning } : {}),
    rules,
    mentions,
    fixtures,
  };
}
