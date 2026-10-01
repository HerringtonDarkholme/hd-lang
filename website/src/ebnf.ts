// Tokenizer and cross-page rule index for the specification's ```ebnf blocks.
// The notation is ISO 14977 EBNF as the specification uses it, plus the
// `"a" ... "z"` range operator. Every rule name that appears left of `=` at
// the start of a rule becomes an anchor; every other name links to one.

export type EbnfKind =
  /** The rule name being defined, left of `=` at a rule start. */
  | "definition"
  /** A lowercase nonterminal named in a rule body. */
  | "reference"
  /** An all-caps name: a layout or abstract lexical token such as NEWLINE. */
  | "token"
  /** A quoted terminal, `"..."` or `'...'`. */
  | "terminal"
  | "operator"
  /** A `(* ... *)` comment. */
  | "comment"
  /** A `? ... ?` special sequence. */
  | "special"
  | "plain";

interface EbnfSpan {
  readonly text: string;
  readonly kind: EbnfKind;
}

// Longest first, so `...` wins over `.` and `(*` is handled before `(`.
const OPERATORS = ["...", "=", ",", "|", ";", ".", "[", "]", "{", "}", "(", ")", "-", "*"];
const RULE_TERMINATORS = new Set([";", "."]);
const NAME = /[A-Za-z_][A-Za-z0-9_]*/y;
const TOKEN_NAME = /^[A-Z][A-Z0-9_]*$/;

/** Whether `name` is an all-caps layout or abstract token name such as NEWLINE. */
const isTokenName = (name: string): boolean => TOKEN_NAME.test(name);

/** The element id a rule definition carries. */
export const ruleAnchor = (name: string): string => `rule-${name}`;

/** Scans to `close`, or to the end of `code` when it is missing. */
function scanTo(code: string, start: number, close: string): number {
  const end = code.indexOf(close, start);
  return end < 0 ? code.length : end + close.length;
}

/** Splits `code` into spans whose texts concatenate back to `code`. */
function scan(code: string): { text: string; kind: EbnfKind | "name" }[] {
  const spans: { text: string; kind: EbnfKind | "name" }[] = [];
  let index = 0;
  while (index < code.length) {
    const start = index;
    const character = code[index]!;
    let kind: EbnfKind | "name";
    if (code.startsWith("(*", index)) {
      kind = "comment";
      index = scanTo(code, index + 2, "*)");
    } else if (character === '"' || character === "'") {
      kind = "terminal";
      // A terminal never spans lines; an unterminated one ends with its line.
      const lineEnd = code.indexOf("\n", index);
      const close = code.indexOf(character, index + 1);
      index =
        close < 0 || (lineEnd >= 0 && close > lineEnd)
          ? lineEnd < 0
            ? code.length
            : lineEnd
          : close + 1;
    } else if (character === "?") {
      kind = "special";
      index = scanTo(code, index + 1, "?");
    } else if (/\s/.test(character)) {
      kind = "plain";
      while (index < code.length && /\s/.test(code[index]!)) index += 1;
    } else {
      NAME.lastIndex = index;
      const name = NAME.exec(code);
      const operator = OPERATORS.find((candidate) => code.startsWith(candidate, index));
      if (name) {
        kind = "name";
        index += name[0].length;
      } else if (operator) {
        kind = "operator";
        index += operator.length;
      } else {
        kind = "plain";
        index += 1;
      }
    }
    spans.push({ text: code.slice(start, index), kind });
  }
  return spans;
}

const isTrivia = (kind: EbnfKind | "name"): boolean => kind === "plain" || kind === "comment";

/**
 * Classifies every span of an EBNF block. A name is a definition when it
 * starts a rule (it is the block's first name, or follows a `;` or `.`
 * terminator) and the next significant span is `=`.
 */
export function tokenizeEbnf(code: string): EbnfSpan[] {
  const spans = scan(code);
  let ruleStart = true;
  return spans.map((span, index) => {
    if (span.kind === "operator" && RULE_TERMINATORS.has(span.text)) {
      ruleStart = true;
      return { text: span.text, kind: "operator" };
    }
    if (span.kind !== "name") {
      if (!isTrivia(span.kind)) ruleStart = false;
      return { text: span.text, kind: span.kind };
    }
    let after = index + 1;
    while (after < spans.length && isTrivia(spans[after]!.kind)) after += 1;
    const next = spans[after];
    const defines = ruleStart && next?.kind === "operator" && next.text === "=";
    ruleStart = false;
    if (defines) return { text: span.text, kind: "definition" };
    return { text: span.text, kind: isTokenName(span.text) ? "token" : "reference" };
  });
}

/** One ```ebnf block of a page, as `fencedBlocks` reports it. */
interface EbnfBlock {
  readonly code: string;
  /** Line of the opening fence in the page source. */
  readonly line: number;
}

interface GrammarPage {
  /** Repository-relative Markdown path. */
  readonly source: string;
  readonly blocks: readonly EbnfBlock[];
}

interface UnresolvedReference {
  readonly name: string;
  readonly source: string;
  readonly line: number;
}

/** Every rule definition across the site, and how rule references resolve. */
export interface GrammarIndex {
  /** The pages that define each rule; the first is the canonical definition. */
  readonly definitions: ReadonlyMap<string, readonly string[]>;
  /** Number of rule definitions, counting restatements on later pages. */
  readonly definitionCount: number;
  /** Number of names used in rule bodies. */
  readonly referenceCount: number;
  /** How many of those resolve to a definition and render as links. */
  readonly linkedCount: number;
  /** Lowercase names used but never defined; they render as plain text. */
  readonly unresolved: readonly UnresolvedReference[];
  /** All-caps names used but never defined: layout and abstract lexical tokens. */
  readonly abstractTokens: readonly string[];
}

/**
 * Indexes the rules `pages` define. A rule's canonical definition is the one
 * on `canonicalSource` when that page defines it, and otherwise the first in
 * page order; later definitions are restatements.
 */
export function buildGrammarIndex(
  pages: readonly GrammarPage[],
  canonicalSource?: string,
): GrammarIndex {
  const definitions = new Map<string, string[]>();
  const uses: UnresolvedReference[] = [];
  let definitionCount = 0;
  for (const { source, blocks } of pages)
    for (const block of blocks) {
      let line = block.line + 1;
      for (const span of tokenizeEbnf(block.code)) {
        if (span.kind === "definition") {
          definitionCount += 1;
          const pagesDefining = definitions.get(span.text) ?? [];
          if (!pagesDefining.includes(source)) pagesDefining.push(source);
          definitions.set(span.text, pagesDefining);
        } else if (span.kind === "reference" || span.kind === "token") {
          uses.push({ name: span.text, source, line });
        }
        line += span.text.split("\n").length - 1;
      }
    }
  if (canonicalSource !== undefined)
    for (const pagesDefining of definitions.values()) {
      const at = pagesDefining.indexOf(canonicalSource);
      if (at > 0) pagesDefining.unshift(...pagesDefining.splice(at, 1));
    }
  const missing = uses.filter((use) => !definitions.has(use.name));
  return {
    definitions,
    definitionCount,
    referenceCount: uses.length,
    linkedCount: uses.length - missing.length,
    unresolved: missing.filter((use) => !isTokenName(use.name)),
    abstractTokens: [
      ...new Set(missing.filter((use) => isTokenName(use.name)).map((use) => use.name)),
    ].sort(),
  };
}

/**
 * The page whose definition of `name` a use on `source` links to: `source`
 * itself when it defines the rule, otherwise the canonical definition.
 */
export function ruleTarget(index: GrammarIndex, name: string, source: string): string | undefined {
  const pagesDefining = index.definitions.get(name);
  if (!pagesDefining) return undefined;
  return pagesDefining.includes(source) ? source : pagesDefining[0];
}
