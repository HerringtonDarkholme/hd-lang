// Reading and applying fix-its from `--format json` diagnostics.
//
// The CLI specification's diagnostic object (cli.json.diagnostic.fields)
// names no fix-it field yet. These metrics read the shape the prototype's
// src/README.md documents, which an `hd` can adopt:
//
//   "fix": { "message": "...", "edits": [
//     { "span": { "start": { "line": 2, "column": 5, "offset": 19 },
//                 "end":   { "line": 2, "column": 5, "offset": 19 } },
//       "replacement": "let " } ] }
//
// `fix` is the one correct fix, or null; `fixes` lists alternatives. A
// metric applies `fix`, or the only entry of `fixes`. Lines and columns are
// 1-based and count UTF-16 code units; `offset`, when present, is a 0-based
// UTF-16 offset and wins. `end` is exclusive.

export interface Position {
  readonly line: number;
  readonly column: number;
  readonly offset?: number;
}

export interface Edit {
  readonly start: Position;
  readonly end: Position;
  readonly replacement: string;
}

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Whether a diagnostic object carries a fix-it field at all, set or null. */
export const hasFixField = (diagnostic: Record<string, unknown>): boolean =>
  "fix" in diagnostic || "fixes" in diagnostic;

function readPosition(value: unknown): Position | undefined {
  if (!isObject(value) || typeof value.line !== "number" || typeof value.column !== "number")
    return undefined;
  return {
    line: value.line,
    column: value.column,
    ...(typeof value.offset === "number" ? { offset: value.offset } : {}),
  };
}

function readFix(value: unknown): Edit[] | undefined {
  if (!isObject(value) || !Array.isArray(value.edits)) return undefined;
  const edits: Edit[] = [];
  for (const edit of value.edits) {
    if (!isObject(edit) || !isObject(edit.span) || typeof edit.replacement !== "string")
      return undefined;
    const start = readPosition(edit.span.start);
    const end = readPosition(edit.span.end);
    if (!start || !end) return undefined;
    edits.push({ start, end, replacement: edit.replacement });
  }
  return edits;
}

/** The edits of a diagnostic's single fix-it, or undefined when it has none or several. */
export function fixOf(diagnostic: Record<string, unknown>): Edit[] | undefined {
  if (diagnostic.fix !== undefined && diagnostic.fix !== null) return readFix(diagnostic.fix);
  if (Array.isArray(diagnostic.fixes) && diagnostic.fixes.length === 1)
    return readFix(diagnostic.fixes[0]);
  return undefined;
}

/** The UTF-16 offset of a position in `text`. */
export function offsetOf(text: string, position: Position): number {
  if (position.offset !== undefined) return position.offset;
  let offset = 0;
  for (let line = 1; line < position.line; line++) {
    const next = text.indexOf("\n", offset);
    if (next < 0) return text.length;
    offset = next + 1;
  }
  return Math.min(text.length, offset + position.column - 1);
}

/** Applies edits to `text`, last first, so earlier offsets stay valid. Overlaps throw. */
export function applyEdits(text: string, edits: readonly Edit[]): string {
  const ranges = edits
    .map((edit) => ({
      from: offsetOf(text, edit.start),
      to: offsetOf(text, edit.end),
      replacement: edit.replacement,
    }))
    .sort((a, b) => b.from - a.from || b.to - a.to);
  let result = text;
  let limit = Number.POSITIVE_INFINITY;
  for (const range of ranges) {
    if (range.to > limit || range.to < range.from) throw new Error("overlapping or inverted edits");
    result = result.slice(0, range.from) + range.replacement + result.slice(range.to);
    limit = range.from;
  }
  return result;
}
