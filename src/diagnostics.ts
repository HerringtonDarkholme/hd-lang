export interface SourcePosition {
  readonly offset: number;
  readonly line: number;
  readonly column: number;
}

export interface SourceSpan {
  readonly start: SourcePosition;
  readonly end: SourcePosition;
}

/** A secondary location a diagnostic points to, such as an earlier declaration. */
export interface RelatedSpan {
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

export class DiagnosticError extends Error {
  readonly diagnostics: readonly Diagnostic[];

  constructor(diagnostics: readonly Diagnostic[]) {
    super(diagnostics.map((diagnostic) => diagnostic.message).join("\n"));
    this.name = "DiagnosticError";
    this.diagnostics = diagnostics;
  }
}

export function formatDiagnostic(file: string, diagnostic: Diagnostic): string {
  const { line, column } = diagnostic.span.start;
  const notes = diagnostic.notes?.map((note) => `\n  note: ${note}`).join("") ?? "";
  const severity = diagnostic.severity === "warning" ? "warning: " : "";
  return `${file}:${line}:${column}: ${severity}${diagnostic.code}: ${diagnostic.message}${notes}`;
}
