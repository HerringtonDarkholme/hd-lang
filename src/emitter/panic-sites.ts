// Panic sites: where a runtime panic report points
// (spec/lang/06-control-flow.md#r-flow.panic.report).
//
// The emitter puts a Binaryen debug-location line, `;;@ sN:LINE:COL`, before
// each operation of a program function that may panic or call code that may.
// Binaryen turns the lines into a source map, never into module bytes
// (src/wasm.ts), so a program that never panics pays nothing at run time. On
// a panic, the host maps the innermost annotated Wasm frame back to site N
// (src/compiler.ts). Only program code is annotated: a panic inside
// `lib/std` points at the program's call into it, as the user's own line.

import { defaultedLocal, tookLiteralDefault } from "../checker/literal-join.ts";
import type { HirExpression, HirFunction } from "../hir.ts";
import type { PanicSite } from "../runtime-panic.ts";

/** Expressions that only read a value: they cannot panic, so they carry no site. */
const READS: ReadonlySet<HirExpression["kind"]> = new Set([
  "integer",
  "float",
  "string",
  "character",
  "boolean",
  "local",
  "global",
  "capture",
  "cell-get",
  "function-value",
]);

/** An expression being emitted that may carry a site (`PanicSiteTable.open`). */
export interface OpenSite {
  readonly key: string;
  readonly site: PanicSite;
  /** The key of the enclosing expression's site. */
  readonly outer: string | undefined;
}

/**
 * `wat` without its panic sites' debug-location lines: the emitted code
 * alone, as emitting without sites gives it. Source positions move with an
 * edit that changes no code, so a comparison of code reads this.
 */
export function withoutSiteLines(wat: string): string {
  return wat.replace(/;;@ s\d+:\d+:\d+\n[ \t]*/g, "");
}

export class PanicSiteTable {
  readonly sites: PanicSite[] = [];
  private readonly ids = new Map<string, number>();
  private annotating = false;
  /**
   * Where the closure being emitted starts. A closure that the compiler
   * wrote for an expression, such as the step of `list.iter()`, puts its
   * code at that expression; it carries no site, so a panic in it names the
   * program's call that ran it. A written closure's code starts after `fn`.
   */
  private closureStart: number | undefined;
  /** The keys of the open expressions' sites, innermost last. */
  private readonly open_: string[] = [];

  /** Annotates the expressions of `declaration` when it is program code, not `lib/std`. */
  enter(declaration: HirFunction | undefined): void {
    this.annotating = declaration !== undefined && declaration.standard !== true;
    this.closureStart = declaration?.closure ? declaration.span.start.offset : undefined;
    this.open_.length = 0;
  }

  /** Starts emitting `expression`; its operands are emitted before `close`. */
  open(expression: HirExpression): OpenSite | undefined {
    if (!this.annotating || READS.has(expression.kind)) return undefined;
    const { line, column, offset } = expression.span.start;
    if (offset === this.closureStart) return undefined;
    const fallback = usizeFallback(expression);
    const key = `${offset}:${line}:${column}${fallback ? ":fallback" : ""}`;
    const outer = this.open_.at(-1);
    this.open_.push(key);
    return {
      key,
      site: fallback ? { span: expression.span, fallback } : { span: expression.span },
      outer,
    };
  }

  /**
   * `wat` after the debug-location line of `open`'s site. An operand at its
   * enclosing expression's site needs no line: Binaryen's location reaches it.
   */
  close(open: OpenSite | undefined, wat: string): string {
    if (!open) return wat;
    this.open_.pop();
    if (open.key === open.outer || !wat.startsWith("(")) return wat;
    let id = this.ids.get(open.key);
    if (id === undefined) {
      id = this.sites.length;
      this.ids.set(open.key, id);
      this.sites.push(open.site);
    }
    const { line, column } = open.site.span.start;
    return `;;@ s${id}:${line}:${column}\n${wat}`;
  }

  /** `wat`, code that `expression` runs outside its own emission, at its site. */
  at(expression: HirExpression, wat: string): string {
    return this.close(this.open(expression), wat);
  }
}

/**
 * Where an arithmetic operation's `usize` type came from the literal default
 * (spec/lang/06-control-flow.md#r-flow.panic.report.fallback): the binding it
 * reads, or a bare literal of its own.
 */
function usizeFallback(expression: HirExpression): PanicSite["fallback"] {
  if (expression.type !== "usize") return undefined;
  if (expression.kind !== "binary" && expression.kind !== "unary") return undefined;
  const operands = (
    expression.kind === "binary" ? [expression.left, expression.right] : [expression.operand]
  ).filter((operand) => operand.type === "usize");
  // An element read from a list of defaulted literals, as `xs[0]`, reads the list's binding.
  const binding =
    defaultedLocal(expression) ??
    operands
      .map((operand) =>
        operand.kind === "list-index" ? defaultedLocal(operand.receiver) : undefined,
      )
      .find(Boolean);
  if (binding)
    return {
      binding: {
        name: binding.name,
        literal: binding.literal,
        span: binding.span,
        ...(binding.kind ? { kind: binding.kind } : {}),
      },
    };
  const literal = operands.find(tookLiteralDefault);
  return literal?.kind === "integer"
    ? { literal: String(literal.wide ?? literal.value), span: literal.span }
    : undefined;
}
