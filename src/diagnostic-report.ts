import type {
  Diagnostic,
  DiagnosticFix,
  SourcePosition,
  SourceSpan,
  TextEdit,
} from "./diagnostics.ts";
import { formatDiagnostic, physicalSpan, sourceDocument } from "./diagnostics.ts";
import type { SpecIndex } from "./spec-index.ts";

// Machine-readable diagnostics for `--format json`. Each record is one JSON
// object on its own line of stderr (JSON Lines); src/README.md documents the
// schema. The text format stays the one `formatDiagnostic` prints.

export type OutputFormat = "text" | "json";

interface JsonPosition {
  /** 1-based line. */
  readonly line: number;
  /** 1-based column, counted in UTF-16 code units. */
  readonly column: number;
  /** 0-based UTF-16 offset into the file. */
  readonly offset: number;
}

/** `end` is exclusive: the position just after the last character. */
export interface JsonSpan {
  readonly start: JsonPosition;
  readonly end: JsonPosition;
}

interface JsonRuleRef {
  readonly id: string;
  readonly anchor: string;
}

export interface JsonDiagnostic {
  readonly kind: "diagnostic" | "runtime-panic" | "entry-error";
  /** A stable code from spec/README.md#diagnostics; null only for `entry-error`. */
  readonly code: string | null;
  readonly severity: "error" | "warning";
  readonly message: string;
  readonly file: string;
  /** Null when the failure has no source location (runtime records). */
  readonly span: JsonSpan | null;
  readonly notes: readonly string[];
  readonly related: readonly {
    readonly message: string;
    readonly file: string;
    readonly span: JsonSpan;
  }[];
  readonly fix: {
    readonly message: string;
    readonly edits: readonly { readonly span: JsonSpan; readonly replacement: string }[];
  } | null;
  /** The one rule ID naming this code, or null when none or several do. */
  readonly rule: string | null;
  /** Every rule whose text names this code as its diagnostic. */
  readonly rules: readonly JsonRuleRef[];
}

function position(value: SourcePosition): JsonPosition {
  return { line: value.line, column: value.column, offset: value.offset };
}

export function jsonSpan(span: SourceSpan): JsonSpan {
  const physical = physicalSpan(span);
  return { start: position(physical.start), end: position(physical.end) };
}

/**
 * Codes whose message already names the one replacement, as in "'struct' was
 * replaced by 'data'". Each entry checks the source under the span before
 * suggesting the edit, so a moved span yields no fix rather than a wrong one.
 */
const REPLACEMENTS: Readonly<
  Record<string, { readonly found: string; readonly edit: (span: SourceSpan) => TextEdit }>
> = {
  "old-struct-declaration": { found: "struct", edit: (span) => ({ span, replacement: "data" }) },
  "old-import-declaration": { found: "import", edit: (span) => ({ span, replacement: "use" }) },
  "old-export-declaration": {
    found: "export",
    edit: (span) => ({ span, replacement: "pub use" }),
  },
  "unexpected-bom": { found: "﻿", edit: (span) => ({ span, replacement: "" }) },
};

function describe(edit: TextEdit, found: string): string {
  if (edit.replacement === "") return found === "﻿" ? "remove the byte-order mark" : "remove";
  return `replace '${found}' with '${edit.replacement}'`;
}

/** The fix the diagnostic carries, or one the prototype derives from its code. */
export function diagnosticFix(diagnostic: Diagnostic, source: string): DiagnosticFix | undefined {
  if (diagnostic.fix) return diagnostic.fix;
  const { start, end } = physicalSpan(diagnostic.span);
  const text = source.slice(start.offset, end.offset);
  const replacement = REPLACEMENTS[diagnostic.code];
  if (replacement && text === replacement.found) {
    const edit = replacement.edit(diagnostic.span);
    return { message: describe(edit, text), edits: [edit] };
  }
  // "a typed mutable binding must begin with 'let'": insert it before the name.
  if (diagnostic.code === "missing-let" && /^[\p{ID_Start}_]/u.test(text))
    return {
      message: "insert 'let' before the binding",
      edits: [{ span: { start, end: start }, replacement: "let " }],
    };
  return undefined;
}

function ruleRefs(index: SpecIndex | undefined, code: string): JsonRuleRef[] {
  return (index?.rulesByCode.get(code) ?? []).map(({ id, anchor }) => ({ id, anchor }));
}

function jsonDiagnostic(
  file: string,
  diagnostic: Diagnostic,
  source: string,
  index: SpecIndex | undefined,
): JsonDiagnostic {
  const rules = ruleRefs(index, diagnostic.code);
  const document = sourceDocument(diagnostic.span);
  const fix = diagnosticFix(diagnostic, document?.text ?? source);
  return {
    kind: "diagnostic",
    code: diagnostic.code,
    severity: diagnostic.severity ?? "error",
    message: diagnostic.message,
    file: document?.file ?? file,
    span: jsonSpan(diagnostic.span),
    notes: diagnostic.notes ?? [],
    related: (diagnostic.related ?? []).map((related) => ({
      message: related.message,
      file: sourceDocument(related.span)?.file ?? file,
      span: jsonSpan(related.span),
    })),
    fix: fix
      ? {
          message: fix.message,
          edits: fix.edits.map((edit) => ({
            span: jsonSpan(edit.span),
            replacement: edit.replacement,
          })),
        }
      : null,
    rule: rules.length === 1 ? rules[0]!.id : null,
    rules,
  };
}

/** Writes a command's diagnostics and runtime failures in one output format. */
export class DiagnosticReporter {
  readonly format: OutputFormat;
  private readonly file: string;
  private readonly source: string;
  private readonly index: SpecIndex | undefined;
  private readonly write: (line: string) => void;

  constructor(
    format: OutputFormat,
    file: string,
    source: string,
    index?: SpecIndex,
    write: (line: string) => void = (line) => console.error(line),
  ) {
    this.format = format;
    this.file = file;
    this.source = source;
    this.index = index;
    this.write = write;
  }

  diagnostic(diagnostic: Diagnostic): void {
    this.write(
      this.format === "json"
        ? JSON.stringify(jsonDiagnostic(this.file, diagnostic, this.source, this.index))
        : formatDiagnostic(this.file, diagnostic),
    );
  }

  runtimePanic(code: string, detail = "runtime panic"): void {
    if (this.format === "text") {
      this.write(`${code}: ${detail}`);
      return;
    }
    const rules = ruleRefs(this.index, code);
    this.record({
      kind: "runtime-panic",
      code,
      severity: "error",
      message: detail,
      file: this.file,
      span: null,
      notes: [],
      related: [],
      fix: null,
      rule: rules.length === 1 ? rules[0]!.id : null,
      rules,
    });
  }

  /** A checked program that stopped at a feature the prototype does not run. */
  unsupported(code: string, message: string): void {
    if (this.format === "text") {
      this.write(`${this.file}: ${code}: ${message}`);
      return;
    }
    this.record({
      kind: "diagnostic",
      code,
      severity: "error",
      message,
      file: this.file,
      span: null,
      notes: [],
      related: [],
      fix: null,
      rule: null,
      rules: [],
    });
  }

  /**
   * A `main` or test whose `Result` is `Err`, or a test that failed otherwise
   * (`outcome`); the program ran, so no code applies.
   */
  entryError(subject = "main", outcome = "returned Err"): void {
    if (this.format === "text") {
      this.write(`${this.file}: ${subject} ${outcome}`);
      return;
    }
    this.record({
      kind: "entry-error",
      code: null,
      severity: "error",
      message: `${subject} ${outcome}`,
      file: this.file,
      span: null,
      notes: [],
      related: [],
      fix: null,
      rule: null,
      rules: [],
    });
  }

  private record(record: JsonDiagnostic): void {
    this.write(JSON.stringify(record));
  }
}
