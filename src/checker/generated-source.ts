import type { Program } from "../ast.ts";
import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";

// ---------------------------------------------------------------------------
// Generated source with placeholders for user expressions and types.

export class Source_ {
  /** `defined` may be shared, so helpers are emitted once across several sources. */
  constructor(defined: Set<string> = new Set()) {
    this.defined = defined;
  }

  readonly lines: string[] = [];
  /** Helper functions, emitted after `lines` so a helper never splits a body. */
  readonly definitions: string[] = [];
  private readonly defined: Set<string>;
  private readonly expressions: Expression[] = [];
  private readonly types: string[] = [];
  /** Spans for single lines, by index in `lines`, over the program's span. */
  private readonly lineSpans = new Map<number, SourceSpan>();

  expression(expression: Expression): string {
    this.expressions.push(expression);
    return placeholder(0, this.expressions.length - 1);
  }

  type(type: string): string {
    this.types.push(type);
    return placeholder(1, this.types.length - 1);
  }

  string(text: string): string {
    return this.expression({ kind: "string", value: text, span: ZERO_SPAN });
  }

  /** Adds a line; a node that starts on it gets `span`, when given. */
  add(text: string, span?: SourceSpan): void {
    if (span) this.lineSpans.set(this.lines.length, span);
    this.lines.push(text);
  }

  /** Emits a helper function once; returns false when it already exists. */
  define(name: string, lines: () => readonly string[]): void {
    if (this.defined.has(name)) return;
    this.defined.add(name);
    this.definitions.push(...lines());
  }

  /**
   * Parses the collected source and patches placeholders and spans;
   * `rename` may rewrite the text first.
   */
  program(span: SourceSpan, rename: (text: string) => string = (text) => text): Program {
    const template = rename(`${[...this.lines, ...this.definitions].join("\n")}\n`);
    const expressionNames = new Map<string, Expression>();
    const typeNames = new Map<string, string>();
    const handles = new Map<string, string>();
    // Handles contain NUL, which cannot occur in a source identifier. Only
    // deliberate interpolations become placeholder tokens; user spellings do
    // not. Allocate tokens after renaming and exclude every spelling already
    // present in the generated source, including copied user identifiers.
    const source = template.replace(/\0([01]):(\d+)\0/g, (handle, kind: string, index: string) => {
      const previous = handles.get(handle);
      if (previous !== undefined) return previous;
      let suffix = handles.size;
      let name = `hd__generated_${suffix}`;
      while (template.includes(name) || expressionNames.has(name) || typeNames.has(name))
        name = `hd__generated_${++suffix}`;
      handles.set(handle, name);
      if (kind === "0") {
        const expression = this.expressions[Number(index)];
        if (!expression) throw new Error("unknown generated expression handle");
        expressionNames.set(name, expression);
      } else {
        const type = this.types[Number(index)];
        if (type === undefined) throw new Error("unknown generated type handle");
        typeNames.set(name, type);
      }
      return name;
    });
    const parsed = parse(source);
    if (!parsed.program)
      throw new Error(
        `typed derivation generated invalid source: ${parsed.diagnostics.map((item) => `${item.code} ${item.message} at ${item.span.start.line}`).join("; ")}\n${source}`,
      );
    const lineSpans = this.lineSpans;
    const spanOf = (parsed: unknown): SourceSpan =>
      lineSpans.get((parsed as SourceSpan).start.line - 1) ?? span;
    const patch = (node: unknown, typePosition = false): unknown => {
      if (Array.isArray(node)) return node.map((item) => patch(item, typePosition));
      if (typeof node === "string")
        return typePosition
          ? node.replace(
              /[\p{ID_Start}_][\p{ID_Continue}]*/gu,
              (name) => typeNames.get(name) ?? name,
            )
          : node;
      if (!node || typeof node !== "object") return node;
      const record = node as Record<string, unknown>;
      if (record.kind === "name" && typeof record.name === "string") {
        const expression = expressionNames.get(record.name);
        if (expression) return expression;
      }
      const result: Record<string, unknown> = {};
      for (const [entry, value] of Object.entries(record))
        result[entry] =
          entry === "span"
            ? spanOf(value)
            : patch(
                value,
                typePosition ||
                  isTypePosition(entry, value) ||
                  (entry === "value" && isTypeRef(value)),
              );
      return result;
    };
    return patch(parsed.program) as Program;
  }
}

// TypeRef positions, collections of TypeRefs, and the AST's string-encoded
// type uses. Member names, variant names, binding names, and literal contents
// are never type positions.
const TYPE_POSITIONS = new Set([
  "type",
  "result",
  "annotation",
  "alias",
  "base",
  "typeArguments",
  "ownerTypeArguments",
  "supertraits",
  "derives",
  "mutPrimitives",
  "mutTuples",
  "genericDefaults",
  "targetName",
  "traitName",
  "typeName",
  "enumName",
  "traits",
  "trait",
  "requirements",
  "writtenRequirements",
]);

function isTypePosition(key: string, value: unknown): boolean {
  if (!TYPE_POSITIONS.has(key)) return false;
  if (key === "genericDefaults") return true;
  return (
    typeof value === "string" ||
    isTypeRef(value) ||
    (Array.isArray(value) && value.every((item) => typeof item === "string" || isTypeRef(item)))
  );
}

function isTypeRef(value: unknown): boolean {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.name === "string" &&
    "span" in record &&
    Object.keys(record).every((key) => key === "name" || key === "span")
  );
}

function placeholder(kind: 0 | 1, index: number): string {
  return `\0${kind}:${index}\0`;
}

const ZERO_POSITION = { line: 1, column: 1, offset: 0 };
export const ZERO_SPAN: SourceSpan = { start: ZERO_POSITION, end: ZERO_POSITION };
