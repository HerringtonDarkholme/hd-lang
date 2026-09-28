import type { Program } from "../ast.ts";
import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";

// ---------------------------------------------------------------------------
// Generated source with placeholders for user expressions and types.

export class Source_ {
  readonly lines: string[] = [];
  /** Helper functions, emitted after `lines` so a helper never splits a body. */
  readonly definitions: string[] = [];
  private readonly defined = new Set<string>();
  private readonly expressions: Expression[] = [];
  private readonly types: string[] = [];

  expression(expression: Expression): string {
    this.expressions.push(expression);
    return `hdexpr${this.expressions.length - 1}`;
  }

  type(type: string): string {
    this.types.push(type);
    return `HDTYPE${this.types.length - 1}X`;
  }

  string(text: string): string {
    return this.expression({ kind: "string", value: text, span: ZERO_SPAN });
  }

  add(text: string): void {
    this.lines.push(text);
  }

  /** Emits a helper function once; returns false when it already exists. */
  define(name: string, lines: () => readonly string[]): void {
    if (this.defined.has(name)) return;
    this.defined.add(name);
    this.definitions.push(...lines());
  }

  /** Parses the collected source and patches placeholders and spans. */
  program(span: SourceSpan): Program {
    const source = `${[...this.lines, ...this.definitions].join("\n")}\n`;
    const parsed = parse(source);
    if (!parsed.program)
      throw new Error(
        `typed derivation generated invalid source: ${parsed.diagnostics.map((item) => `${item.code} ${item.message} at ${item.span.start.line}`).join("; ")}\n${source}`,
      );
    const types = this.types;
    const expressions = this.expressions;
    const patch = (node: unknown, key?: string): unknown => {
      if (Array.isArray(node)) return node.map((item) => patch(item));
      if (typeof node === "string")
        return key === "value"
          ? node
          : node.replace(/HDTYPE(\d+)X/g, (_, index: string) => types[Number(index)]!);
      if (!node || typeof node !== "object") return node;
      const record = node as Record<string, unknown>;
      if (record.kind === "name" && typeof record.name === "string") {
        const match = /^hdexpr(\d+)$/.exec(record.name);
        if (match) return expressions[Number(match[1])];
      }
      const result: Record<string, unknown> = {};
      for (const [entry, value] of Object.entries(record))
        result[entry] = entry === "span" ? span : patch(value, entry);
      return result;
    };
    return patch(parsed.program) as Program;
  }
}

export const ZERO_POSITION = { line: 1, column: 1, offset: 0 };
export const ZERO_SPAN: SourceSpan = { start: ZERO_POSITION, end: ZERO_POSITION };
