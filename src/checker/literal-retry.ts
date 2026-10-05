import type { Expression, Statement } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { ValueType } from "../hir.ts";
import { DEFAULTED_SPANS, isSignedLiteral, literalGroupDefault } from "./literal-join.ts";

// The join model inside one expression whose typed side is checked after its
// literals: a method call on a literal receiver (`[1, 2].iter().all(fn(n:
// i32) -> bool: ...)`), a pipe (`3 |> twice`), a call whose parameter type
// comes through a bound or a typed fact, or an operator whose literal side is
// a call (`identity(0) + combine()`). When the check fails with the literals
// at their own defaults, it is tried again with them at each other width; the
// one width that fits is taken, `i32` first for a group with a signed
// literal. Literals in a closure, a block, or a comprehension are statements
// of their own and are never retyped.

/** The literals of one expression that a retry may give one width. */
export interface ExpressionLiterals {
  readonly spans: Map<SourceSpan, "integer" | "float">;
  signed: boolean;
}

/** The unsuffixed literals of `expression` itself, never inside a closure or a block. */
export function expressionLiterals(
  expression: Expression,
  found: ExpressionLiterals = { spans: new Map(), signed: false },
): ExpressionLiterals {
  if (expression.kind === "integer" || expression.kind === "float")
    found.spans.set(expression.span, expression.kind);
  else if (isSignedLiteral(expression)) {
    found.spans.set(expression.span, "integer");
    found.signed = true;
  } else for (const child of literalChildren(expression)) expressionLiterals(child, found);
  return found;
}

const HAS_LITERALS = new WeakMap<Expression, boolean>();

/** Whether `expression` itself holds an unsuffixed literal, remembered per node. */
export function hasExpressionLiterals(expression: Expression): boolean {
  const known = HAS_LITERALS.get(expression);
  if (known !== undefined) return known;
  const has =
    expression.kind === "integer" ||
    expression.kind === "float" ||
    isSignedLiteral(expression) ||
    literalChildren(expression).some(hasExpressionLiterals);
  HAS_LITERALS.set(expression, has);
  return has;
}

/**
 * The subexpressions that are part of `expression` itself: through operators,
 * collections, data values, calls, members, indexes, ranges, pipes, and
 * interpolations, and the branches, arms, and closure bodies that are one
 * expression; never a block of statements.
 */
function literalChildren(expression: Expression): readonly Expression[] {
  const value = (body: readonly Statement[]): Expression[] => {
    const [only, ...rest] = body;
    return rest.length === 0 && only?.kind === "expression" ? [only.expression] : [];
  };
  const present = (...children: (Expression | undefined)[]): Expression[] =>
    children.filter((child): child is Expression => child !== undefined);
  switch (expression.kind) {
    case "unary":
      return [expression.operand];
    case "binary":
      return [expression.left, expression.right];
    case "list":
    case "tuple":
      return expression.elements;
    case "map":
      return expression.entries.flatMap((entry) => [entry.key, entry.value]);
    case "data":
      return present(...expression.fields.map((field) => field.value), expression.spread);
    case "call":
    case "suspend-call":
      return [expression.callee, ...expression.arguments];
    case "member":
      return [expression.receiver];
    case "index":
      return [expression.receiver, expression.index];
    case "range":
      return present(expression.start, expression.end);
    case "pipe":
      return [expression.value, expression.step];
    case "propagate":
      return [expression.operand];
    case "if":
      return [expression.condition, ...value(expression.thenBody), ...value(expression.elseBody)];
    case "match":
      return [expression.subject, ...expression.arms.flatMap((arm) => value(arm.body))];
    case "closure":
      return value(expression.body);
    case "interpolated-string":
      return expression.segments.flatMap((segment) =>
        segment.kind === "expression" ? [segment.expression] : [],
      );
    default:
      return [];
  }
}

/** The literals of a statement's own expressions, never inside a nested block. */
export function statementLiterals(statement: Statement): ExpressionLiterals {
  const found: ExpressionLiterals = { spans: new Map(), signed: false };
  const own = statement as {
    readonly value?: Expression;
    readonly expression?: Expression;
    readonly target?: Expression;
  };
  for (const child of [own.value, own.expression, own.target])
    if (child && hasExpressionLiterals(child)) expressionLiterals(child, found);
  // Only a literal that took its default type can change width.
  for (const span of found.spans.keys()) if (!DEFAULTED_SPANS.has(span)) found.spans.delete(span);
  return found;
}

const NUMERIC = /\b(?:u8|u16|u32|u64|usize|i8|i16|i32|i64|f32|f64)\b/g;

export interface LiteralWidths {
  readonly integer?: ValueType;
  readonly float?: ValueType;
}

/**
 * The widths a retry tries: the numeric types the failure names, which are
 * the typed values the literals meet. A failure that names none of the
 * literals' default types is not about them, and has none.
 */
export function namedWidths(
  found: ExpressionLiterals,
  failed: readonly { readonly code: string; readonly message: string }[],
): LiteralWidths[] {
  if (found.spans.size === 0) return [];
  // A range error names a wider type as advice, not a type the literal meets.
  const meetings = failed.filter(({ code }) => !code.endsWith("-literal-range"));
  const named = new Set(
    meetings.flatMap(({ message }) =>
      (message.match(NUMERIC) ?? []).map((word) => (word === "usize" ? "u32" : word)),
    ),
  );
  const kinds = new Set(found.spans.values());
  const integerDefault = found.signed ? "i32" : "u32";
  const defaults = [kinds.has("integer") && integerDefault, kinds.has("float") && "f64"];
  if (!defaults.some((type) => type && named.has(type))) return [];
  const widths: LiteralWidths[] = [];
  for (const type of named) {
    if (type === integerDefault || type === "f64") continue;
    const float = type.startsWith("f");
    if (float ? !kinds.has("float") : !kinds.has("integer")) continue;
    if (!float && found.signed && type.startsWith("u")) continue;
    widths.push(float ? { float: type } : { integer: type });
  }
  return widths;
}

/** The one width a retry takes: the only fit, or `i32` for a signed group. */
export function chosenWidth(
  found: ExpressionLiterals,
  fits: readonly LiteralWidths[],
): LiteralWidths | undefined {
  if (fits.length === 1) return fits[0];
  if (!found.signed) return undefined;
  return fits.find((fit) => fit.integer === "i32" && fit.float === undefined);
}

/**
 * The literals a retry of their expression gives a width, keyed by span. It
 * is shared by the checkers of one compilation, so a closure written in the
 * expression sees it; each retry undoes its own entries.
 */
export const FORCED_LITERALS = new Map<SourceSpan, ValueType>();

/** The width a retry gives the literals of a literals-only group, if any. */
export function forcedGroupWidth(members: readonly Expression[]): ValueType | undefined {
  if (FORCED_LITERALS.size === 0) return undefined;
  for (const member of members)
    for (const span of expressionLiterals(member).spans.keys()) {
      const width = FORCED_LITERALS.get(span);
      if (width !== undefined) return width;
    }
  return undefined;
}

/** A literals-only group's default type; its literals count as defaulted, so a retry may change them. */
export function defaultGroupWidth(members: readonly Expression[]): ValueType | undefined {
  const type = literalGroupDefault(members);
  if (type !== undefined)
    for (const member of members)
      for (const span of expressionLiterals(member).spans.keys()) DEFAULTED_SPANS.add(span);
  return type;
}

/** Sets the forced width of every literal in `found`, returning the undo. */
export function forceLiterals(found: ExpressionLiterals, widths: LiteralWidths): () => void {
  const forced = FORCED_LITERALS;
  const previous = new Map(forced);
  for (const [span, kind] of found.spans) {
    const width = kind === "integer" ? widths.integer : widths.float;
    if (width !== undefined) forced.set(span, width);
  }
  return () => {
    forced.clear();
    for (const [span, width] of previous) forced.set(span, width);
  };
}

/** The mutable checker state a failed first attempt may leave, to put back. */
export interface CheckerMark {
  readonly lengths: readonly (readonly [unknown[], number])[];
  readonly maps: readonly (readonly [Map<unknown, unknown>, number])[];
}

export function markState(
  arrays: readonly unknown[][],
  maps: readonly Map<unknown, unknown>[],
): CheckerMark {
  return {
    lengths: arrays.map((array) => [array, array.length] as const),
    maps: maps.map((map) => [map, map.size] as const),
  };
}

export function restoreState(mark: CheckerMark): void {
  for (const [array, length] of mark.lengths) array.length = length;
  for (const [map, size] of mark.maps)
    if (map.size > size) for (const key of [...map.keys()].slice(size)) map.delete(key);
}
