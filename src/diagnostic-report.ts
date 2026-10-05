import type {
  Diagnostic,
  DiagnosticFix,
  SourcePosition,
  SourceSpan,
  TextEdit,
} from "./diagnostics.ts";
import { formatDiagnostic, physicalSpan, sourceDocument } from "./diagnostics.ts";
import type { SpecIndex } from "./spec-index.ts";

// Machine-readable output for `--format json` (spec/cli/command-line.md,
// Machine Output). Each record is one JSON object on its own line (JSON
// Lines); src/README.md documents the schema. The text format stays the one
// `formatDiagnostic` prints.

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

/** A diagnostic object (`cli.json.diagnostic.fields`), plus the prototype's agent fields. */
export interface JsonDiagnostic {
  readonly kind: "diagnostic";
  /** A stable code from spec/README.md#diagnostics; null for a `main` that returned `Err`. */
  readonly code: string | null;
  readonly severity: "error" | "warning";
  readonly message: string;
  readonly file: string;
  /** 1-based line; null when the failure has no source location (runtime records). */
  readonly line: number | null;
  /** 1-based column in UTF-16 code units; null with `line`. */
  readonly column: number | null;
  readonly notes: readonly string[];
  readonly related: readonly {
    readonly message: string;
    readonly file: string;
    readonly line: number;
    readonly column: number;
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
  // "a binding with a type annotation must begin with 'let'": insert it before the name.
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
  const { line, column } = physicalSpan(diagnostic.span).start;
  return {
    kind: "diagnostic",
    code: diagnostic.code,
    severity: diagnostic.severity ?? "error",
    message: diagnostic.message,
    file: document?.file ?? file,
    line,
    column,
    notes: diagnostic.notes ?? [],
    related: (diagnostic.related ?? []).map((related) => {
      const start = physicalSpan(related.span).start;
      return {
        message: related.message,
        file: sourceDocument(related.span)?.file ?? file,
        line: start.line,
        column: start.column,
      };
    }),
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

/** A test case's outcome (`cli.json.test.fields`). */
export type TestOutcome = "passed" | "failed" | "skipped" | "ignored";

/**
 * One command's output: where its diagnostics, test objects, and summary
 * go, and the counts the summary reports (`cli.json.summary.fields`).
 * Text diagnostics always go to standard error. With `--format json` the
 * records go to `stream`: standard output for `build`, `check`, and `test`
 * (`cli.json.lines.build`), standard error for `run` and `hd FILE`
 * (`cli.json.run`).
 */
export class Report {
  readonly format: OutputFormat;
  readonly counts = { errors: 0, warnings: 0, passed: 0, failed: 0, skipped: 0, ignored: 0 };
  private readonly writeText: (line: string) => void;
  private readonly writeJson: (line: string) => void;
  private readonly summary: boolean;

  constructor(
    format: OutputFormat,
    io: { readonly out: (value: unknown) => void; readonly err: (line: string) => void },
    options: { readonly stream?: "stdout" | "stderr"; readonly summary?: boolean } = {},
  ) {
    this.format = format;
    this.writeText = io.err;
    this.writeJson = options.stream === "stdout" ? (line) => io.out(line) : io.err;
    this.summary = options.summary ?? false;
  }

  /** Writes one line of text diagnostics, or one JSON record. */
  write(text: string, record?: object): void {
    if (this.format === "json" && record) this.writeJson(JSON.stringify(record));
    else this.writeText(text);
  }

  count(severity: "error" | "warning"): void {
    this.counts[severity === "error" ? "errors" : "warnings"] += 1;
  }

  /**
   * An error of the command itself that names no file, such as `hd run`
   * outside any package: a line of text, or a diagnostic record with no code
   * and an empty `file`.
   */
  commandError(message: string): void {
    this.count("error");
    const record: JsonDiagnostic = {
      kind: "diagnostic",
      code: null,
      severity: "error",
      message,
      file: "",
      line: null,
      column: null,
      notes: [],
      related: [],
      fix: null,
      rule: null,
      rules: [],
    };
    this.write(message, record);
  }

  /** A test case's result: one test object with `--format json`, nothing in text. */
  test(name: string, outcome: TestOutcome, message = ""): void {
    this.counts[outcome] += 1;
    if (this.format === "json")
      this.writeJson(JSON.stringify({ kind: "test", name, outcome, message }));
  }

  /** Ends the command with `status`; with `--format json` the summary object comes last. */
  finish(status: number): number {
    if (this.format === "json" && this.summary)
      this.writeJson(JSON.stringify({ kind: "summary", ...this.counts, status }));
    return status;
  }
}

/** Writes a command's diagnostics and runtime failures in one output format. */
export class DiagnosticReporter {
  readonly format: OutputFormat;
  private readonly report: Report;
  private readonly file: string;
  private readonly source: string;
  private readonly index: SpecIndex | undefined;
  private readonly jsonFile: string;

  /**
   * `file` is the path text diagnostics name; `jsonFile` is the path the
   * JSON `file` field holds, which is relative to the package root in a
   * package (`cli.json.diagnostic.file`).
   */
  constructor(report: Report, file: string, source: string, index?: SpecIndex, jsonFile = file) {
    this.format = report.format;
    this.report = report;
    this.file = file;
    this.source = source;
    this.index = index;
    this.jsonFile = jsonFile;
  }

  diagnostic(diagnostic: Diagnostic): void {
    this.report.count(diagnostic.severity ?? "error");
    this.report.write(
      formatDiagnostic(this.file, diagnostic),
      this.format === "json"
        ? jsonDiagnostic(this.jsonFile, diagnostic, this.source, this.index)
        : undefined,
    );
  }

  runtimePanic(code: string, detail = "runtime panic"): void {
    this.located(`${code}: ${detail}`, code, detail);
  }

  /** A checked program that stopped at a feature the prototype does not run. */
  unsupported(code: string, message: string): void {
    this.located(`${this.file}: ${code}: ${message}`, code, message, false);
  }

  /**
   * A `main` or test whose `Result` is `Err`, or a test that failed otherwise
   * (`outcome`); the program ran, so no code applies.
   */
  entryError(subject = "main", outcome = "returned Err"): void {
    this.located(`${this.file}: ${subject} ${outcome}`, null, `${subject} ${outcome}`);
  }

  /** `hd test FILE` for a FILE that registers no test case (`cli.test.file-empty`). */
  noTestCases(): void {
    this.located(`${this.file}: no test case registered`, null, "no test case registered");
  }

  /**
   * An error that the specification gives no code, at a line of the file,
   * such as a mistake in `hd.toml`. Its text has no code, so no runner reads
   * it as a located diagnostic.
   */
  uncoded(message: string, line: number, column: number): void {
    this.located(`${this.file}:${line}: ${message}`, null, message, false, { line, column });
  }

  /** A failure with no code or no source location. */
  private located(
    text: string,
    code: string | null,
    message: string,
    applyRules = true,
    position?: { readonly line: number; readonly column: number },
  ): void {
    this.report.count("error");
    const rules = code && applyRules ? ruleRefs(this.index, code) : [];
    const record: JsonDiagnostic = {
      kind: "diagnostic",
      code,
      severity: "error",
      message,
      file: this.jsonFile,
      line: position?.line ?? null,
      column: position?.column ?? null,
      notes: [],
      related: [],
      fix: null,
      rule: rules.length === 1 ? rules[0]!.id : null,
      rules,
    };
    this.report.write(text, this.format === "json" ? record : undefined);
  }
}
