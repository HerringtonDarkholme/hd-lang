// Messages for `cannot-infer-type`. Each names the generic parameters that
// nothing solved and shows the annotation that would solve them, as
// 04-type-system.md#r-types.infer.ambiguous asks of a diagnostic.

import type { Expression, Statement } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { ValueType } from "../hir.ts";
import { displayType } from "../types.ts";

/** A `name := value` binding whose initializer is checked without an annotation. */
export interface InferredBinding {
  readonly name: string;
  readonly value: { readonly span: SourceSpan };
}

/**
 * The message for a use of `type` whose parameters `unresolved` nothing
 * solved. When `binding`'s whole initializer is at `span`, the advice
 * annotates that binding; otherwise it suggests a new one.
 */
export function unresolvedTypeMessage(
  unresolved: readonly string[],
  type: ValueType,
  span: SourceSpan,
  binding: InferredBinding | undefined,
): string {
  const shown = writtenType(type);
  const initializer = binding?.value.span;
  const advice =
    initializer?.start.offset === span.start.offset && initializer.end.offset === span.end.offset
      ? `annotate the binding: \`let ${binding!.name}: ${shown} = ...\``
      : `annotate a binding for it: \`let value: ${shown} = ...\``;
  return `cannot infer ${parameterList(unresolved)} in \`${shown}\`; ${advice}`;
}

/** A plain literal's default type, for the empty-collection hint. */
function literalDefaultType(kind: Expression["kind"]): ValueType | undefined {
  switch (kind) {
    case "string":
      return "string";
    case "integer":
      return "usize";
    case "float":
      return "f64";
    case "boolean":
      return "bool";
    case "character":
      return "char";
    default:
      return undefined;
  }
}

/**
 * The element type a later statement shows for the empty `List` bound to
 * `name`: the literal pushed by the first `name.push(literal)` written after
 * `afterOffset`, the end of the empty initializer. Only statements after the
 * binding can name it, so earlier text never counts. Anything else leaves
 * the `T` placeholder.
 */
export function laterPushedElementType(
  statements: readonly Statement[],
  afterOffset: number,
  name: string,
): ValueType | undefined {
  for (const statement of statements) {
    if (
      statement.kind !== "expression" ||
      statement.span.start.offset <= afterOffset ||
      statement.expression.kind !== "call"
    )
      continue;
    const { callee, arguments: callArguments } = statement.expression;
    if (
      callee.kind !== "member" ||
      callee.name !== "push" ||
      callee.receiver.kind !== "name" ||
      callee.receiver.name !== name ||
      callArguments.length !== 1
    )
      continue;
    const element = literalDefaultType(callArguments[0]!.kind);
    if (element !== undefined) return element;
  }
  return undefined;
}

/**
 * The `loop` hint for an unknown-name message: two usability probes reached
 * for `loop`, which hd spells `while true:`.
 */
export function loopNameHint(name: string): string {
  return name === "loop" ? "; hd has no `loop`; write `while true:`" : "";
}

/** The message for a call of `callee` whose parameters `unresolved` nothing solved. */
export function unresolvedCallMessage(unresolved: readonly string[], callee: string): string {
  return `cannot infer ${parameterList(unresolved)} in the call to \`${callee}\`; annotate the binding, or write the type arguments: \`${callee}::[...]\``;
}

/** A type as its author wrote it: `mut User`, `Result[i32, E]`, never an internal name. */
function writtenType(type: ValueType): string {
  return displayType(type);
}

/** `` `T` ``, `` `T` and `E` ``, or `` `A`, `B`, and `C` ``. */
function parameterList(names: readonly string[]): string {
  const quoted = names.map((name) => `\`${displayType(name)}\``);
  if (quoted.length <= 2) return quoted.join(" and ");
  return `${quoted.slice(0, -1).join(", ")}, and ${quoted.at(-1)}`;
}
