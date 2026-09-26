// Seed mutation operators. Works on a language-agnostic token view of the
// source text: it does not share any tables with a parser.
import type { Rng } from "./common.ts";

export interface Line {
  indent: string;
  tokens: string[];
  /** Trailing comment, kept verbatim so fixture markers survive when possible. */
  comment: string;
  /** Original code text (no indent, no comment) while the tokens are unchanged. */
  original?: string;
}

const tokenPattern =
  /r?"""[\s\S]*?"""|r?"(?:\\.|[^"\\\n])*"?|r?'(?:\\.|[^'\\\n])*'?|[\p{L}_][\p{L}\p{N}_]*|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?|\.\.\.|:=|->|=>|::|==|!=|<=|>=|<<|>>|\*\*|\$\{|\S/gu;

export function splitLines(source: string): Line[] {
  return source.split("\n").map((raw) => {
    const indent = /^[ \t]*/.exec(raw)![0];
    const body = raw.slice(indent.length);
    const tokens: string[] = [];
    let comment = "";
    let codeEnd = body.length;
    for (const match of body.matchAll(tokenPattern)) {
      if (match[0] === "#") {
        comment = body.slice(match.index);
        codeEnd = match.index;
        break;
      }
      tokens.push(match[0]);
    }
    return { comment, indent, original: body.slice(0, codeEnd).trimEnd(), tokens };
  });
}

export function joinLines(lines: readonly Line[]): string {
  return lines
    .map((line) => {
      const body = line.original ?? line.tokens.join(" ");
      const code = body ? `${line.indent}${body}` : "";
      if (!line.comment) return code;
      return code ? `${code}  ${line.comment}` : `${line.indent}${line.comment}`;
    })
    .join("\n");
}

const interestingLiterals = [
  "0",
  "-1",
  "1",
  "2147483647",
  "2147483648",
  "-2147483648",
  "4294967295",
  "9223372036854775807",
  "9223372036854775808",
  "0.0",
  "-0.0",
  "1e308",
  "1e309",
  "0x7f",
  "1_000",
  '""',
  '"\\u{1F600}"',
  '"a${x}b"',
  '"$"',
  "'a'",
  "nil",
  "true",
  "false",
];

export type MutationName =
  | "delete-line"
  | "delete-token"
  | "duplicate-line"
  | "duplicate-token"
  | "insert-token"
  | "reindent"
  | "replace-literal"
  | "splice-line"
  | "swap-lines"
  | "swap-tokens";

export const mutationNames: readonly MutationName[] = [
  "delete-token",
  "duplicate-token",
  "swap-tokens",
  "reindent",
  "delete-line",
  "duplicate-line",
  "swap-lines",
  "insert-token",
  "replace-literal",
  "splice-line",
];

function tokenSlots(lines: readonly Line[]): Array<[number, number]> {
  const slots: Array<[number, number]> = [];
  for (const [lineIndex, line] of lines.entries())
    for (let tokenIndex = 0; tokenIndex < line.tokens.length; tokenIndex += 1)
      slots.push([lineIndex, tokenIndex]);
  return slots;
}

function codeLines(lines: readonly Line[]): number[] {
  return lines.flatMap((line, index) => (line.tokens.length ? [index] : []));
}

function cloneLine(line: Line): Line {
  return {
    comment: line.comment,
    indent: line.indent,
    original: line.original,
    tokens: [...line.tokens],
  };
}

/**
 * Applies one mutation in place. Returns false when the mutation had nothing to act on.
 * `vocabulary` supplies insertable tokens (EBNF literals); `donors` supplies spliced lines.
 */
export function applyMutation(
  lines: Line[],
  name: MutationName,
  rng: Rng,
  vocabulary: readonly string[],
  donors: readonly Line[],
): boolean {
  const slots = tokenSlots(lines);
  const nonEmpty = codeLines(lines);
  if (slots.length === 0 || nonEmpty.length === 0) return false;
  const [lineIndex, tokenIndex] = rng.pick(slots);
  const line = lines[lineIndex]!;
  if (name.endsWith("-token") || name === "replace-literal") line.original = undefined;
  switch (name) {
    case "delete-token":
      line.tokens.splice(tokenIndex, 1);
      return true;
    case "duplicate-token":
      line.tokens.splice(tokenIndex, 0, line.tokens[tokenIndex]!);
      return true;
    case "swap-tokens": {
      if (line.tokens.length < 2) return false;
      const left = Math.min(tokenIndex, line.tokens.length - 2);
      const swapped = line.tokens[left]!;
      line.tokens[left] = line.tokens[left + 1]!;
      line.tokens[left + 1] = swapped;
      return true;
    }
    case "reindent": {
      const target = lines[rng.pick(nonEmpty)]!;
      const width = target.indent.replaceAll("\t", "    ").length;
      const choice = rng.pick([width + 4, width - 4, width + 1, width - 1, 0, width + 8]);
      target.indent = " ".repeat(Math.max(0, choice));
      return true;
    }
    case "delete-line":
      lines.splice(rng.pick(nonEmpty), 1);
      return true;
    case "duplicate-line": {
      const index = rng.pick(nonEmpty);
      lines.splice(index, 0, cloneLine(lines[index]!));
      return true;
    }
    case "swap-lines": {
      if (nonEmpty.length < 2) return false;
      const position = rng.int(nonEmpty.length - 1);
      const first = nonEmpty[position]!;
      const second = nonEmpty[position + 1]!;
      const saved = lines[first]!;
      lines[first] = lines[second]!;
      lines[second] = saved;
      return true;
    }
    case "insert-token":
      line.tokens.splice(tokenIndex + rng.int(2), 0, rng.pick(vocabulary));
      return true;
    case "replace-literal": {
      const literalSlots = slots.filter(([row, column]) =>
        /^(?:\d|"|'|r"|nil$|true$|false$)/.test(lines[row]!.tokens[column]!),
      );
      if (literalSlots.length === 0) return false;
      const [row, column] = rng.pick(literalSlots);
      lines[row]!.original = undefined;
      lines[row]!.tokens[column] = rng.pick(interestingLiterals);
      return true;
    }
    case "splice-line": {
      if (donors.length === 0) return false;
      const donor = cloneLine(rng.pick(donors));
      donor.comment = "";
      donor.indent = lines[lineIndex]!.indent;
      lines.splice(lineIndex + rng.int(2), 0, donor);
      return true;
    }
  }
}

/** Strips conformance markers so a mutated seed does not carry stale expectations. */
export function stripMarkers(lines: Line[]): void {
  for (const line of lines)
    if (
      /^#\s*(?:diagnostic|warning|panic|test|expect|expect-result|fixture-[a-z-]+):/.test(
        line.comment,
      )
    )
      line.comment = "";
}

export interface Mutant {
  readonly operators: readonly MutationName[];
  readonly source: string;
}

export function mutate(
  seed: string,
  rng: Rng,
  vocabulary: readonly string[],
  donors: readonly Line[],
): Mutant {
  const lines = splitLines(seed);
  stripMarkers(lines);
  const count = 1 + rng.int(3);
  const operators: MutationName[] = [];
  for (let attempt = 0; operators.length < count && attempt < count * 4; attempt += 1) {
    const name = rng.pick(mutationNames);
    if (applyMutation(lines, name, rng, vocabulary, donors)) operators.push(name);
  }
  return { operators, source: `${joinLines(lines).replace(/\n*$/, "")}\n` };
}
