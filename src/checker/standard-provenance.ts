import {
  SOURCE_ORIGIN,
  type SourceDocument,
  type SourceOrigin,
  type SourcePosition,
  type SourceSpan,
} from "../diagnostics.ts";

function isPosition(value: unknown): value is SourcePosition {
  if (!value || typeof value !== "object") return false;
  const position = value as Partial<SourcePosition>;
  return (
    typeof position.offset === "number" &&
    typeof position.line === "number" &&
    typeof position.column === "number"
  );
}

function isSpan(value: unknown): value is SourceSpan {
  if (!value || typeof value !== "object") return false;
  const span = value as Partial<SourceSpan>;
  return isPosition(span.start) && isPosition(span.end);
}

function origin(position: SourcePosition, document: SourceDocument): SourceOrigin {
  return (
    position[SOURCE_ORIGIN] ?? {
      document,
      offset: position.offset,
      line: position.line,
      column: position.column,
    }
  );
}

function positionAt(
  physical: SourcePosition,
  logical: SourcePosition,
  document: SourceDocument,
): SourcePosition {
  return {
    offset: logical.offset,
    line: logical.line,
    column: logical.column,
    [SOURCE_ORIGIN]: origin(physical, document),
  };
}

/**
 * Gives every span in parsed std syntax a physical source and a logical
 * joined-program anchor. The physical origin survives later spans composed
 * from their endpoints; the logical coordinates preserve declaration order.
 */
export function withStandardSource<T>(value: T, document: SourceDocument, anchor: SourceSpan): T {
  if (isSpan(value))
    return {
      start: positionAt(value.start, anchor.start, document),
      end: positionAt(value.end, anchor.end, document),
    } as T;
  if (Array.isArray(value))
    return value.map((item) => withStandardSource(item, document, anchor)) as T;
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value).map(([key, child]) => [key, withStandardSource(child, document, anchor)]),
  ) as T;
}
