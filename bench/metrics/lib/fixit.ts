// Reading and applying the CLI specification's fix-its
// (cli.json.fix.object, cli.json.fix.edit, cli.json.fix.offsets).
// `fixes` lists alternatives; the metric applies only a single fix for
// the diagnostic's own file. Offsets count UTF-8 bytes, with an exclusive end.

export interface Edit {
  readonly file: string;
  readonly start: number;
  readonly end: number;
  readonly text: string;
}

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isOffset = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

/** Whether a diagnostic object carries the specified fix-it field. */
export const hasFixField = (diagnostic: Record<string, unknown>): boolean => "fixes" in diagnostic;

function readFix(value: unknown): Edit[] | undefined {
  if (!isObject(value) || typeof value.message !== "string" || !Array.isArray(value.edits))
    return undefined;
  const edits: Edit[] = [];
  for (const edit of value.edits) {
    if (
      !isObject(edit) ||
      typeof edit.file !== "string" ||
      edit.file.length === 0 ||
      !isOffset(edit.start) ||
      !isOffset(edit.end) ||
      edit.end < edit.start ||
      typeof edit.text !== "string"
    )
      return undefined;
    edits.push({ file: edit.file, start: edit.start, end: edit.end, text: edit.text });
  }
  return edits;
}

/** One unambiguous fix this single-file metric can apply, or undefined. */
export function fixOf(diagnostic: Record<string, unknown>): Edit[] | undefined {
  if (!Array.isArray(diagnostic.fixes) || diagnostic.fixes.length !== 1) return undefined;
  const edits = readFix(diagnostic.fixes[0]);
  if (!edits || new Set(edits.map((edit) => edit.file)).size > 1) return undefined;
  if (typeof diagnostic.file === "string" && edits.some((edit) => edit.file !== diagnostic.file))
    return undefined;
  return edits;
}

/** Applies UTF-8 byte edits last first; invalid boundaries or overlaps throw. */
export function applyEdits(text: string, edits: readonly Edit[]): string {
  if (new Set(edits.map((edit) => edit.file)).size > 1)
    throw new Error("edits span multiple files");
  const source = Buffer.from(text, "utf8");
  const boundary = (offset: number): boolean =>
    offset === source.length || (source[offset]! & 0xc0) !== 0x80;
  const ranges = [...edits].sort((a, b) => b.start - a.start || b.end - a.end);
  let result = source;
  let limit = source.length;
  for (const edit of ranges) {
    if (
      !isOffset(edit.start) ||
      !isOffset(edit.end) ||
      edit.end > source.length ||
      edit.end < edit.start ||
      !boundary(edit.start) ||
      !boundary(edit.end)
    )
      throw new Error("invalid UTF-8 byte edit range");
    if (edit.end > limit) throw new Error("overlapping edits");
    result = Buffer.concat([
      result.subarray(0, edit.start),
      Buffer.from(edit.text, "utf8"),
      result.subarray(edit.end),
    ]);
    limit = edit.start;
  }
  return result.toString("utf8");
}
