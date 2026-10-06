import type { Expression } from "../ast.ts";
import type { HirExpression, HirLocal, ValueType } from "../hir.ts";
import type { Diagnostic, DiagnosticFix, SourceSpan } from "../diagnostics.ts";
import { numericType, type NumericType } from "../numeric.ts";
import { readonlyType, displayType } from "../types.ts";
import { numericWidening } from "./shared.ts";
import { leastCommonType } from "./least-common-type.ts";

// The join model for unsuffixed numeric literals (task #296 prototype).
//
// 1. A literal with an expected type takes it.
// 2. With none, a bare integer literal is `usize`, a signed one,
//    `-1` or `+5`, is `i32`, and a float literal is `f64`.
// 3. Within one expression only (operands of one operator, the arguments of
//    one generic call, the elements of one collection literal, the arms of one
//    `if` or `match`) a literal with no expected type takes the type of the
//    typed members. A group of literals only is `i32` when any member is
//    signed, and the rule-2 default otherwise.
// 4. Nothing crosses statements: a literal's type is known when its
//    expression is checked.

/** A literal typed by rule 2, which a join of its own expression may retype. */
const DEFAULTED = new WeakSet<HirExpression>();

/**
 * The spans of the literals that ever took a default type: the ones a retry
 * may change. Keyed by span object, weakly, so a finished program's spans are
 * not kept alive.
 */
export const DEFAULTED_SPANS = new WeakSet<SourceSpan>();

export function markDefaultedLiteral<T extends HirExpression>(value: T): T {
  DEFAULTED.add(value);
  DEFAULTED_SPANS.add(value.span);
  return value;
}

export function isDefaultedLiteral(value: HirExpression | undefined): boolean {
  return value !== undefined && DEFAULTED.has(value);
}

/**
 * The literals of a literals-only operator group that took the group's
 * default type. They stay fixed, unlike `DEFAULTED`'s: only a panic report
 * reads them (06-control-flow.md#r-flow.panic.report.fallback).
 */
const GROUP_DEFAULTED = new WeakSet<HirExpression>();

export function markDefaultedGroup(members: readonly HirExpression[]): void {
  for (const member of members) {
    if (member.kind === "integer") GROUP_DEFAULTED.add(member);
    else if (member.kind === "unary") markDefaultedGroup([member.operand]);
    else if (member.kind === "binary") markDefaultedGroup([member.left, member.right]);
  }
}

/** A literal whose type came from a default: its own, or its operator group's. */
export function tookLiteralDefault(value: HirExpression): boolean {
  return DEFAULTED.has(value) || GROUP_DEFAULTED.has(value);
}

const ARITHMETIC = new Set(["+", "-", "*", "/", "%", "&", "|", "^", "**", "<<", ">>"]);

/**
 * The kind of a tree built only of unsuffixed literals, unary `-` and `+`,
 * and arithmetic, such as `2 * 3 + -1`: its literals form one join group.
 */
export function pureLiteralKind(expression: Expression): "integer" | "float" | undefined {
  switch (expression.kind) {
    case "integer":
      return "integer";
    case "float":
      return "float";
    case "unary":
      return expression.operator === "-" ||
        expression.operator === "+" ||
        expression.operator === "~"
        ? pureLiteralKind(expression.operand)
        : undefined;
    case "binary": {
      if (!ARITHMETIC.has(expression.operator)) return undefined;
      const left = pureLiteralKind(expression.left);
      return left !== undefined && left === pureLiteralKind(expression.right) ? left : undefined;
    }
    default:
      return undefined;
  }
}

/** A list, map, or tuple literal whose leaves are all literal trees, such as `[(1, 2)]`. */
export function isLiteralStructure(expression: Expression): boolean {
  switch (expression.kind) {
    case "list":
      return (
        expression.elements.length > 0 &&
        !expression.spreads?.some(Boolean) &&
        expression.elements.every(
          (element) => pureLiteralKind(element) !== undefined || isLiteralStructure(element),
        )
      );
    case "tuple":
      return (
        !expression.spread &&
        expression.elements.every(
          (element) => pureLiteralKind(element) !== undefined || isLiteralStructure(element),
        )
      );
    default:
      return false;
  }
}

/** A signed literal: an integer literal written directly after `-` or `+`. */
export function isSignedLiteral(expression: Expression): boolean {
  return (
    expression.kind === "unary" &&
    (expression.operator === "-" || expression.operator === "+") &&
    expression.operand.kind === "integer"
  );
}

/** Whether a pure literal tree holds a signed literal (outside a shift count or exponent). */
export function hasSignedLiteral(expression: Expression): boolean {
  if (isSignedLiteral(expression)) return true;
  if (expression.kind === "unary") return hasSignedLiteral(expression.operand);
  if (expression.kind === "binary")
    return (
      hasSignedLiteral(expression.left) ||
      (!["**", "<<", ">>"].includes(expression.operator) && hasSignedLiteral(expression.right))
    );
  return false;
}

/** The rule-2 type of a group of pure literal trees of one kind: `i32` when any is signed. */
export function literalGroupDefault(members: readonly Expression[]): ValueType | undefined {
  const kinds = new Set(members.map(pureLiteralKind));
  if (kinds.size !== 1) return undefined;
  const [kind] = kinds;
  if (kind === "float") return "f64";
  if (kind !== "integer") return undefined;
  return members.some(hasSignedLiteral) ? "i32" : "usize";
}

/** A local whose type came from a bare literal's rule-2 default, for the fix hint. */
export interface DefaultedBinding {
  readonly name: string;
  /** The text and span of the (first) bare literal that gave the type. */
  readonly literal: string;
  readonly span: SourceSpan;
  /** A collection, data, or range value built of literals, or a loop over one. */
  readonly kind?: "structure" | "loop";
  /** The binding statement and its initializer, for the annotation fix. */
  readonly statement?: SourceSpan;
  readonly initializer?: SourceSpan;
  readonly letMut?: boolean;
}

const DEFAULTED_LOCALS = new WeakMap<HirLocal, DefaultedBinding>();

/** Diagnostics whose checked value proved whether a local literal supplied its type. */
const RESOLVED_LOCAL_HINTS = new WeakSet<Diagnostic>();

export function recordDefaultedLocal(local: HirLocal, binding: DefaultedBinding): void {
  DEFAULTED_LOCALS.set(local, binding);
}

const CAPTURE_SOURCES = new WeakMap<HirExpression, HirLocal>();

/** A capture of `source`, remembered so the fix hint reaches a value a closure reads. */
export function captureOf(source: HirLocal, capture: HirExpression): HirExpression {
  CAPTURE_SOURCES.set(capture, source);
  return capture;
}

export function defaultedLocal(value: HirExpression): DefaultedBinding | undefined {
  const captured = CAPTURE_SOURCES.get(value);
  if (captured) return DEFAULTED_LOCALS.get(captured);
  // An `if` or `match` whose value is a defaulted binding carries its hint.
  if (value.kind === "if")
    return [value.thenBody, value.elseBody]
      .map((body) => finalValueOf(body))
      .map((final) => (final ? defaultedLocal(final) : undefined))
      .find(Boolean);
  if (value.kind === "match")
    return value.arms
      .map((arm) => finalValueOf(arm.body))
      .map((final) => (final ? defaultedLocal(final) : undefined))
      .find(Boolean);
  if (value.kind === "permission-weaken") return defaultedLocal(value.operand);
  if (value.kind === "unary" && value.operator !== "cast") return defaultedLocal(value.operand);
  if (value.kind === "binary")
    return (
      (value.left.type === value.type ? defaultedLocal(value.left) : undefined) ??
      (value.right.type === value.type ? defaultedLocal(value.right) : undefined)
    );
  return value.kind === "local" ? DEFAULTED_LOCALS.get(value.local) : undefined;
}

/**
 * A defaulted literal retyped to the integer or float type its join chose:
 * the literal, or the range problem when its value does not fit.
 */
export function retypeDefaultedLiteral(
  value: HirExpression,
  target: ValueType,
): HirExpression | { readonly problem: string } | undefined {
  if (!DEFAULTED.has(value) || value.type === target) return undefined;
  const numeric = numericType(target);
  if (!numeric) return undefined;
  if (value.kind === "float") {
    if (numeric.family !== "float") return undefined;
    const converted = target === "f32" ? Math.fround(value.value) : value.value;
    return { ...value, value: converted, type: target };
  }
  if (value.kind !== "integer" || numeric.family === "float") return undefined;
  const exact = BigInt((value as { readonly wide?: string }).wide ?? value.value);
  const { minimum, maximum, bits } = numeric as Required<NumericType>;
  if (exact < minimum || exact > maximum)
    return {
      problem: `integer literal ${exact} is outside the ${displayType(target)} range ${minimum}..${maximum}, the type its expression gives it`,
    };
  const span = value.span;
  return bits === 64
    ? { kind: "integer", value: Number(exact), wide: exact.toString(), type: target, span }
    : { kind: "integer", value: Number(exact), type: target, span };
}

/** The text of a literal tree, as the hint quotes it (integers in decimal). */
export function literalText(expression: Expression): string {
  switch (expression.kind) {
    case "integer":
      return expression.value.toString();
    case "float":
      return String(expression.value);
    case "unary":
      return `${expression.operator}${literalText(expression.operand)}`;
    case "binary":
      return `${literalText(expression.left)} ${expression.operator} ${literalText(expression.right)}`;
    default:
      return "...";
  }
}

/**
 * The fix hint for a value read from a binding whose bare literal defaulted
 * to `usize`, where it meets the type `other`: the literal's signed form
 * when `other` is signed, and an annotation when `other` is not `i32`.
 */
export function defaultedLocalHint(
  value: HirExpression,
  other: ValueType,
): { readonly note: string; readonly span: SourceSpan; readonly fix?: DiagnosticFix } | undefined {
  const origin = defaultedLocal(value);
  // A deeply nested type makes no useful annotation, and is slow to print.
  if (!origin || value.type.length > 200 || other.length > 200) return undefined;
  const width =
    origin.kind === undefined || numericType(other) === undefined
      ? origin.kind
        ? widthAtUsize(value.type, other)
        : other
      : other;
  const numeric = numericType(width);
  if (!width || !numeric || numeric.family === "float" || width === "usize") return undefined;
  const at = `line ${origin.span.start.line}`;
  const signed = `+${origin.literal}`;
  const whole =
    origin.kind !== "structure"
      ? width
      : numericType(other)
        ? displayType(readonlyType(value.type)).replace(/\busize\b/g, width)
        : displayType(readonlyType(other));
  const annotation =
    origin.kind === "loop"
      ? `${width}(${origin.literal})`
      : `let ${origin.name}: ${displayType(whole)} = ${origin.kind ? "..." : origin.literal}`;
  const why =
    origin.kind === "loop"
      ? `'${origin.name}' is usize because the literal '${origin.literal}' (${at}) it loops over has no sign`
      : origin.kind === "structure"
        ? `'${origin.name}' has usize in its type because its literal '${origin.literal}' (${at}) has no sign`
        : `'${origin.name}' is usize because its literal '${origin.literal}' (${at}) has no sign`;
  const note =
    width === "i32"
      ? `${why}; write '${signed}' there to make ${origin.kind ? "the literals" : "it"} i32`
      : numeric.family === "signed"
        ? `${why} or type; write '${annotation}', or '${signed}' for i32`
        : `${why} or type; write '${annotation}'`;
  return { note, span: origin.span, fix: hintFix(origin, width, annotation) };
}

/** The edit of a hint: `+` before the literal for `i32`, else the annotation. */
function hintFix(
  origin: DefaultedBinding,
  width: ValueType,
  annotation: string,
): DiagnosticFix | undefined {
  const at = (span: SourceSpan) => ({ start: span.start, end: span.start });
  if (width === "i32")
    return {
      message: `write '+${origin.literal}'`,
      edits: [{ span: at(origin.span), replacement: "+" }],
    };
  if (origin.kind === "loop")
    return {
      message: `write '${annotation}'`,
      edits: [{ span: origin.span, replacement: annotation }],
    };
  if (!origin.statement || !origin.initializer) return undefined;
  const prefix = annotation
    .slice(0, annotation.lastIndexOf("= ") + 2)
    .replace(/^let /, origin.letMut ? "let mut " : "let ");
  return {
    message: `write '${annotation}'`,
    edits: [
      {
        span: { start: origin.statement.start, end: origin.initializer.start },
        replacement: prefix,
      },
    ],
  };
}

/** The numeric type in `other` where `type` has `usize`, as `i32` in `List[i32]` against `List[usize]`. */
function widthAtUsize(type: ValueType, other: ValueType): ValueType | undefined {
  const words = (text: ValueType) => displayType(readonlyType(text)).split(/[^A-Za-z0-9_]+/);
  const mine = words(type);
  const theirs = words(other);
  for (let index = 0; index < Math.min(mine.length, theirs.length); index += 1)
    if (mine[index] === "usize" && numericType(theirs[index]) && theirs[index] !== mine[index])
      return theirs[index];
  return undefined;
}

/** `diagnostic` with the fix hint of the first value in `pairs` that has one. */
export function withDefaultedLocalHint(
  diagnostic: Diagnostic,
  pairs: readonly (readonly [HirExpression, ValueType])[],
): Diagnostic {
  const hint = pairs
    .map(([value, other]) => defaultedLocalHint(value, readonlyType(other)))
    .find((found) => found !== undefined);
  if (!hint || diagnostic.severity === "warning") {
    RESOLVED_LOCAL_HINTS.add(diagnostic);
    return diagnostic;
  }
  // The hint's edit replaces a conversion fix-it, which would point away from the literal.
  return {
    ...diagnostic,
    message: diagnostic.message.replace(/; write [a-z0-9]+\(\.\.\.\)$/, ""),
    notes: [...(diagnostic.notes ?? []), hint.note],
    related: [...(diagnostic.related ?? []), { message: hint.note, span: hint.span }],
    ...(hint.fix ? { fix: hint.fix } : {}),
  };
}

/** The first bare integer literal of a collection, data, or range value built of literals. */
export function firstBareLiteral(expression: Expression): Expression | undefined {
  switch (expression.kind) {
    case "integer":
      return expression;
    case "list":
    case "tuple":
      // Only a list or tuple of literals: a typed element would decide it.
      return isLiteralStructure(expression)
        ? expression.elements.map(firstBareLiteral).find(Boolean)
        : undefined;
    case "map":
      return expression.entries
        .flatMap((entry) => [entry.key, entry.value])
        .map(firstBareLiteral)
        .find(Boolean);
    case "data":
      return expression.fields.map((field) => firstBareLiteral(field.value)).find(Boolean);
    case "range":
      return expression.start?.kind === "integer" && expression.end?.kind === "integer"
        ? expression.start
        : undefined;
    default:
      return undefined;
  }
}

/**
 * Several instantiations fit a call only because a literal argument could
 * take several types; the literal keeps its own type, which none of them
 * takes, so the call asks for the type to be written.
 */
export function literalNoFitMessage(
  name: string,
  sources: readonly Expression[],
  fits: readonly string[],
): string {
  const literal = sources.find((source) => pureLiteralKind(source) !== undefined)!;
  const own = literalGroupDefault([literal])!;
  const text = literalText(literal);
  const example = /\[([^,\]]+)\]$/.exec(fits[0] ?? "")?.[1];
  return `the literal argument '${text}' of '${name}' is ${own} on its own, and only another type fits: ${fits.join(", ")}; write the type you mean${example ? `, as in '${example === "i32" && !text.startsWith("-") ? `+${text}` : `${example}(${text})`}'` : ""}`;
}

/**
 * A numeric literal value coerced to `target`: a defaulted literal takes the
 * type its own expression's join chose, range-checked, and otherwise a
 * literal typed early widens (numericWidening).
 */
export function coerceLiteral(
  value: HirExpression,
  target: ValueType,
  span: SourceSpan,
  fail: (code: string, message: string, span: SourceSpan) => never,
): HirExpression | undefined {
  const retyped = retypeDefaultedLiteral(value, target);
  if (retyped && "problem" in retyped) fail("integer-literal-range", retyped.problem, span);
  return retyped ?? numericWidening(value, target, span);
}

/** A return path of a body whose result type is inferred, a member of their join. */
export interface InferredReturn {
  readonly type: ValueType;
  readonly span: SourceSpan;
  readonly value?: HirExpression;
}

/** The value a block of statements ends with, if any. */
export function finalValueOf(
  body: readonly { readonly kind: string; readonly expression?: HirExpression }[],
): HirExpression | undefined {
  const last = body.at(-1);
  return last?.kind === "expression" ? last.expression : undefined;
}

/**
 * The member types of one join whose members were checked apart, such as the
 * return paths of a closure or a loop's `else` value and `break` values: a
 * member that is a defaulted literal is retyped in place to the type the
 * typed members join to, or, with no typed member, to `i32` when one is
 * signed and `usize` otherwise.
 */
export function joinDefaultedInPlace(
  members: readonly { readonly type: ValueType; readonly value?: HirExpression }[],
  join: (types: readonly ValueType[]) => ValueType | undefined,
): ValueType[] {
  const types = members.map((member) => member.type);
  const flexible = members.map(
    (member) => member.value !== undefined && DEFAULTED.has(member.value),
  );
  if (!flexible.some(Boolean)) return types;
  const others = types.filter((type, index) => !flexible[index] && type !== "never");
  const literals = types.filter((_, index) => flexible[index]);
  const integers = literals.every((type) => numericType(type)?.family !== "float");
  const target =
    others.length > 0
      ? join(others)
      : integers
        ? literals.includes("i32")
          ? "i32"
          : "usize"
        : "f64";
  const family = target === undefined ? undefined : numericType(readonlyType(target))?.family;
  if (family === undefined || (family === "float") !== !integers) return types;
  members.forEach((member, index) => {
    if (!flexible[index]) return;
    const retyped = retypeDefaultedLiteral(member.value!, readonlyType(target!));
    if (retyped && !("problem" in retyped)) Object.assign(member.value!, retyped);
  });
  return types.map((type, index) => (flexible[index] ? readonlyType(target!) : type));
}

/** The least common type of a join's members, after `joinDefaultedInPlace`. */
export function joinedLeastCommonType(
  members: readonly { readonly type: ValueType; readonly value?: HirExpression }[],
  declarations: Parameters<typeof leastCommonType>[1],
): ReturnType<typeof leastCommonType> {
  const types = joinDefaultedInPlace(members, (others) => {
    const joined = leastCommonType(others, declarations);
    return "type" in joined ? joined.type : undefined;
  });
  return leastCommonType(types, declarations);
}

/**
 * The fix hint for a type error that no site gave one: a binding named in
 * the failing source whose bare literal defaulted to `usize`, against a
 * numeric type the message names.
 */
export function fallbackLiteralHint(
  diagnostic: Diagnostic,
  body: unknown,
  locals: readonly HirLocal[],
  anywhere = false,
): Diagnostic {
  if (
    diagnostic.severity === "warning" ||
    diagnostic.notes?.length ||
    RESOLVED_LOCAL_HINTS.has(diagnostic)
  )
    return diagnostic;
  if (!/\busize\b/.test(diagnostic.message)) return diagnostic;
  const { start, end } = diagnostic.span;
  const names = new Set<string>();
  const walk = (node: unknown): void => {
    if (Array.isArray(node)) return node.forEach(walk);
    if (!node || typeof node !== "object") return;
    const item = node as { kind?: unknown; name?: unknown; span?: SourceSpan };
    if (item.kind === "name" && typeof item.name === "string" && item.span)
      if (
        anywhere ||
        (item.span.start.offset >= start.offset && item.span.end.offset <= end.offset)
      )
        names.add(item.name);
    for (const [key, child] of Object.entries(node)) if (key !== "span") walk(child);
  };
  walk(body);
  const widths = diagnostic.message.match(/\b[iu](8|16|32|64)\b/g) ?? [];
  for (const local of [...locals].reverse()) {
    if (!names.has(local.name) || !DEFAULTED_LOCALS.has(local)) continue;
    const value: HirExpression = { kind: "local", local, type: local.type, span: diagnostic.span };
    for (const width of widths) {
      const hint = defaultedLocalHint(value, width);
      if (hint) return withDefaultedLocalHint(diagnostic, [[value, width]]);
    }
  }
  return diagnostic;
}
