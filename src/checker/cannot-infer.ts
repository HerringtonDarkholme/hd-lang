// Messages for `cannot-infer-type`. Each names the generic parameters that
// nothing solved and shows the annotation that would solve them, as
// 04-type-system.md#r-types.infer.ambiguous asks of a diagnostic.

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
