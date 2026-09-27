// Shared reading of specification Markdown for the style lint
// (spec/check-spec-style.ts) and the rule inventory (spec/tools/rule-inventory.ts).
//
// It is a small line-based block scanner, not a full Markdown parser: it knows
// the constructs the chapters use (headings, fences, paragraphs, list items,
// block quotes, tables, footnotes) and the rule ID marker defined in
// spec/STYLE.md. It imports only Node built-ins.

/** One Markdown block, with its 1-based starting line. */
export interface Block {
  readonly kind: "heading" | "paragraph" | "item" | "quote" | "row" | "code";
  /** Prose text with line breaks folded to spaces; raw source for code. */
  readonly text: string;
  readonly line: number;
  /** A fence's info string; empty for other blocks. */
  readonly info: string;
}

/** A rule ID marker found in the prose. */
export interface RuleMarker {
  readonly id: string;
  readonly line: number;
  /** The rule text after the marker, as written. */
  readonly text: string;
}

/**
 * The first dotted segment each numbered chapter's rule IDs start with.
 * spec/STYLE.md documents the same table; keep the two in step.
 */
export const CHAPTER_PREFIXES: Readonly<Record<string, string>> = {
  "01-lexical-structure.md": "lex",
  "02-grammar.md": "grammar",
  "03-names-and-scopes.md": "names",
  "04-type-system.md": "types",
  "05-expressions.md": "expr",
  "06-control-flow.md": "flow",
  "07-functions.md": "fn",
  "08-data-and-enums.md": "data",
  "09-traits.md": "trait",
  "10-modules.md": "module",
  "11-requirements-and-suspension.md": "req",
  "12-variadic-generics.md": "pack",
  "13-gadts.md": "gadt",
  "14-annotations.md": "annot",
};

/** Rule ID syntax: two or more dot-separated lowercase kebab-case segments. */
export const RULE_ID = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*(?:\.[a-z][a-z0-9]*(?:-[a-z0-9]+)*)+$/;

/** A marker that opens a block or table cell: `r[` ID `]` and a space. */
const LEADING_MARKER = /^r\[([^\]]*)\](?:\s+|$)/;

/** The HTML id the website gives rule `id`, and the fragment that cites it. */
export function ruleIdAnchor(id: string): string {
  return `r-${id}`;
}

const FENCE = /^(\s*)(`{3,}|~{3,})(.*)$/;
const HEADING = /^#{1,6}\s+/;
const ITEM = /^(\s*)(?:[-*+]|\d+[.)])\s+/;
const TABLE_RULE = /^\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)*\|?\s*$/;

/** Splits a Markdown document into blocks. */
export function blocks(markdown: string): Block[] {
  const lines = markdown.split(/\r?\n/);
  const result: Block[] = [];
  let open: { kind: Block["kind"]; parts: string[]; line: number } | undefined;
  const flush = (): void => {
    if (open)
      result.push({ kind: open.kind, text: open.parts.join(" "), line: open.line, info: "" });
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
        const closing = FENCE.exec(lines[index]!);
        if (
          closing &&
          closing[2]![0] === fence[2]![0] &&
          closing[2]!.length >= fence[2]!.length &&
          closing[3]!.trim() === ""
        )
          break;
        body.push(lines[index]!);
      }
      result.push({
        kind: "code",
        text: body.join("\n"),
        line: start + 1,
        info: fence[3]!.trim().split(/\s+/, 1)[0] ?? "",
      });
      continue;
    }
    if (line.trim() === "") {
      flush();
      continue;
    }
    if (HEADING.test(line)) {
      flush();
      result.push({ kind: "heading", text: line.replace(HEADING, ""), line: index + 1, info: "" });
      continue;
    }
    if (line.trimStart().startsWith("|")) {
      flush();
      if (!TABLE_RULE.test(line.trim()))
        result.push({ kind: "row", text: line.trim(), line: index + 1, info: "" });
      continue;
    }
    const quote = /^\s*>\s?(.*)$/.exec(line);
    if (quote) {
      if (open?.kind !== "quote") {
        flush();
        open = { kind: "quote", parts: [], line: index + 1 };
      }
      if (quote[1]!.trim() === "") {
        // A blank quoted line separates paragraphs inside one quote.
        flush();
        open = { kind: "quote", parts: [], line: index + 2 };
      } else open.parts.push(quote[1]!.trim());
      continue;
    }
    const item = ITEM.exec(line);
    if (item) {
      flush();
      open = { kind: "item", parts: [line.slice(item[0].length).trim()], line: index + 1 };
      continue;
    }
    if (!open) open = { kind: "paragraph", parts: [], line: index + 1 };
    open.parts.push(line.trim());
  }
  flush();
  return result.filter((block) => block.kind === "code" || block.text.trim() !== "");
}

/** The cells of a table row, trimmed. */
export function cells(row: string): string[] {
  return row
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((cell) => cell.trim());
}

const INLINE_CODE = /(`+)(?:(?!\1).)+?\1/g;

/** Replaces inline code spans with a one-word placeholder. */
export function withoutInlineCode(text: string): string {
  return text.replaceAll(INLINE_CODE, "CODE");
}

/** Prose as a reader sees it: no markers, link targets, footnote refs, or emphasis marks. */
export function readable(text: string): string {
  return withoutInlineCode(text)
    .replace(LEADING_MARKER, "")
    .replaceAll(/\[\^[^\]]+\]:?/g, "")
    .replaceAll(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replaceAll(/[*_]{1,3}(?=\S)|(?<=\S)[*_]{1,3}/g, "")
    .replaceAll(/\s+/g, " ")
    .trim();
}

const HOLE = /(\d+)/g;

/** Masks inline code spans so prose rules cannot see inside them; `restore` puts them back. */
function masked(text: string): { text: string; restore: (value: string) => string } {
  const codes: string[] = [];
  return {
    text: text.replaceAll(INLINE_CODE, (span) => {
      codes.push(span);
      return `${codes.length - 1}`;
    }),
    restore: (value) => value.replaceAll(HOLE, (_, index: string) => codes[Number(index)]!),
  };
}

/** Like `readable`, but inline code is kept as written, for reports. */
export function display(text: string): string {
  const mask = masked(text);
  return mask.restore(readable(mask.text));
}

export function wordCount(text: string): number {
  return readable(text)
    .split(" ")
    .filter((word) => /[\p{L}\p{N}]/u.test(word)).length;
}

/** Splits prose into sentences. Inline code never ends a sentence. */
export function sentences(text: string): string[] {
  const mask = masked(text);
  return mask.text
    .replace(LEADING_MARKER, "")
    .split(/(?<=[.!?]["')\]]?)\s+(?=[\p{Lu}\p{N}`"([*])/u)
    .map((sentence) => mask.restore(sentence).trim())
    .filter((sentence) => sentence !== "");
}

/** The prose blocks a reader counts as paragraphs: paragraphs, list items, and quotes. */
export function paragraphs(all: readonly Block[]): Block[] {
  return all.filter(
    (block) =>
      (block.kind === "paragraph" || block.kind === "item" || block.kind === "quote") &&
      !/^\[\^[^\]]+\]:/.test(block.text),
  );
}

export interface MarkerScan {
  readonly markers: RuleMarker[];
  /** Problems with marker syntax or placement, as `line: message`. */
  readonly problems: string[];
}

/**
 * Finds rule ID markers. A marker must open a paragraph, list item, quote, or
 * table cell; one anywhere else in prose is reported, as is a malformed ID.
 */
export function ruleMarkers(all: readonly Block[]): MarkerScan {
  const markers: RuleMarker[] = [];
  const problems: string[] = [];
  const consider = (text: string, line: number): void => {
    const lead = LEADING_MARKER.exec(text);
    if (lead) {
      const id = lead[1]!;
      if (!RULE_ID.test(id)) problems.push(`${line}: malformed rule ID r[${id}]`);
      else markers.push({ id, line, text: text.slice(lead[0].length).trim() });
    }
    const rest = withoutInlineCode(lead ? text.slice(lead[0].length) : text);
    for (const stray of rest.matchAll(/(?<![\w\]])r\[([^\]\s]*)\]/g))
      problems.push(
        `${line}: rule ID marker r[${stray[1]}] must open a paragraph, list item, or table cell`,
      );
  };
  for (const block of all) {
    if (block.kind === "code" || block.kind === "heading") continue;
    if (block.kind === "row") for (const cell of cells(block.text)) consider(cell, block.line);
    else consider(block.text, block.line);
  }
  return { markers, problems };
}

/** Diagnostic codes named by `# error: CODE` markers in a code block. */
export function errorMarkerCodes(code: string): string[] {
  return [...code.matchAll(/#\s*error:\s*([a-z0-9-]+)\s*$/gm)].map((match) => match[1]!);
}

/** Whether a code block is an error example: a line ends in `# error` or `# error: CODE`. */
export function isErrorExample(code: string): boolean {
  return /#\s*error(?::\s*[a-z0-9-]+)?\s*$/m.test(code);
}

/** Error, warning, and boundary codes from spec/README.md, and panic codes from chapter 06. */
export function knownCodes(readme: string, controlFlow: string): Set<string> {
  const codes = new Set<string>();
  for (const line of readme.split("\n")) {
    const row = /^\| (Error[^|]*|Warning|Boundary failure) \| (.*) \|$/.exec(line);
    if (row) for (const match of row[2]!.matchAll(/`([a-z0-9-]+)`/g)) codes.add(match[1]!);
  }
  const panics = /Stable panic categories are exactly([\s\S]*?)\.\s/.exec(controlFlow);
  if (panics) for (const match of panics[1]!.matchAll(/`([a-z0-9-]+)`/g)) codes.add(match[1]!);
  return codes;
}

/** Retired rule IDs: the backticked IDs listed under "Retired Rule IDs" in spec/STYLE.md. */
export function retiredRuleIds(style: string): Set<string> {
  const section = /^## Retired Rule IDs\s*$([\s\S]*?)(?=^## |(?![\s\S]))/m.exec(style);
  const retired = new Set<string>();
  if (!section) return retired;
  for (const line of section[1]!.split("\n")) {
    const item = /^\s*[-*]\s+`([^`]+)`/.exec(line);
    if (item) retired.add(item[1]!);
  }
  return retired;
}
