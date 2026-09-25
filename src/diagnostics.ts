export interface SourcePosition {
  readonly offset: number;
  readonly line: number;
  readonly column: number;
}

export interface SourceSpan {
  readonly start: SourcePosition;
  readonly end: SourcePosition;
}

export interface Diagnostic {
  readonly code: string;
  readonly message: string;
  readonly span: SourceSpan;
  readonly severity?: "error" | "warning";
  readonly notes?: readonly string[];
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
