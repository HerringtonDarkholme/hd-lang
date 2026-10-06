/** One physical source whose positions diagnostics may name. */
export interface SourceDocument {
  /** The stable, user-facing path of the source. */
  readonly file: string;
  readonly text: string;
}

/** A position in a physical source, before it is joined into another source. */
export interface SourceOrigin {
  readonly document: SourceDocument;
  readonly offset: number;
  readonly line: number;
  readonly column: number;
}

/** Internal source-map metadata: spreads preserve symbols, JSON omits them. */
export const SOURCE_ORIGIN: unique symbol = Symbol("hd.source-origin");

export interface SourcePosition {
  readonly offset: number;
  readonly line: number;
  readonly column: number;
  /**
   * The physical source position represented by this logical joined-program
   * position. Absent when the two positions are the same source.
   */
  readonly [SOURCE_ORIGIN]?: SourceOrigin;
}

export interface SourceSpan {
  readonly start: SourcePosition;
  readonly end: SourcePosition;
}

/** A secondary location a diagnostic points to, such as an earlier declaration. */
interface RelatedSpan {
  readonly message: string;
  readonly span: SourceSpan;
}

/** One text replacement; an empty span inserts and an empty replacement deletes. */
export interface TextEdit {
  readonly span: SourceSpan;
  readonly replacement: string;
}

/** A suggested fix: edits that, applied together, resolve the diagnostic. */
export interface DiagnosticFix {
  readonly message: string;
  readonly edits: readonly TextEdit[];
}

export interface Diagnostic {
  readonly code: string;
  readonly message: string;
  readonly span: SourceSpan;
  readonly severity?: "error" | "warning";
  readonly notes?: readonly string[];
  readonly related?: readonly RelatedSpan[];
  readonly fix?: DiagnosticFix;
}

/** The physical document of `span`, when it was joined from another source. */
export function sourceDocument(span: SourceSpan): SourceDocument | undefined {
  return span.start[SOURCE_ORIGIN]?.document ?? span.end[SOURCE_ORIGIN]?.document;
}

/** A position expressed in its physical source, while retaining its source identity. */
export function physicalPosition(position: SourcePosition): SourcePosition {
  const origin = position[SOURCE_ORIGIN];
  return origin
    ? {
        offset: origin.offset,
        line: origin.line,
        column: origin.column,
        [SOURCE_ORIGIN]: origin,
      }
    : position;
}

/** A span expressed in its physical source, while retaining its source identity. */
export function physicalSpan(span: SourceSpan): SourceSpan {
  return { start: physicalPosition(span.start), end: physicalPosition(span.end) };
}

/** Preserve source-map metadata when a generic object transform rebuilds a node. */
export function preserveSourceOrigin<T extends object>(source: unknown, target: T): T {
  if (!source || typeof source !== "object") return target;
  const origin = (source as SourcePosition)[SOURCE_ORIGIN];
  if (origin) (target as { [SOURCE_ORIGIN]?: SourceOrigin })[SOURCE_ORIGIN] = origin;
  return target;
}

/** A diagnostic whose primary, related, and edit spans use physical coordinates. */
export function physicalDiagnostic(diagnostic: Diagnostic): Diagnostic {
  return {
    ...diagnostic,
    span: physicalSpan(diagnostic.span),
    ...(diagnostic.related
      ? {
          related: diagnostic.related.map((related) => ({
            ...related,
            span: physicalSpan(related.span),
          })),
        }
      : {}),
    ...(diagnostic.fix
      ? {
          fix: {
            ...diagnostic.fix,
            edits: diagnostic.fix.edits.map((edit) => ({
              ...edit,
              span: physicalSpan(edit.span),
            })),
          },
        }
      : {}),
  };
}

/** A source-aware identity for comparison and diagnostic deduplication. */
export function sourceSpanKey(span: SourceSpan): string {
  const physical = physicalSpan(span);
  const file = sourceDocument(span)?.file ?? "";
  return `${file}\0${physical.start.offset}:${physical.end.offset}`;
}

export class DiagnosticError extends Error {
  readonly diagnostics: readonly Diagnostic[];

  constructor(diagnostics: readonly Diagnostic[]) {
    super(diagnostics.map((diagnostic) => diagnostic.message).join("\n"));
    this.name = "DiagnosticError";
    this.diagnostics = diagnostics;
  }
}

export function formatDiagnostic(file: string, diagnostic: Diagnostic): string {
  const document = sourceDocument(diagnostic.span);
  const { line, column } = physicalSpan(diagnostic.span).start;
  const notes = diagnostic.notes?.map((note) => `\n  note: ${note}`).join("") ?? "";
  const severity = diagnostic.severity === "warning" ? "warning: " : "";
  return `${document?.file ?? file}:${line}:${column}: ${severity}${diagnostic.code}: ${diagnostic.message}${notes}`;
}

/**
 * The code for a type-argument list of `written` arguments where `expected`
 * are declared: `argument-count` for too many, and
 * `partial-generic-arguments` for too few (spec/README.md#diagnostics).
 */
export function arityCode(written: number, expected: number): string {
  return written > expected ? "argument-count" : "partial-generic-arguments";
}
