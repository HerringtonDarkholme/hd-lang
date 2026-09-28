import type { SourceSpan } from "../diagnostics.ts";
import type { HirTrait, HirTraitImplementation, ValueType } from "../hir.ts";
import { optionalInner, readonlyType, resultParts } from "../types.ts";
import { matchTraitImplementation, traitTypeName } from "./shared.ts";

/** The synthetic function name of a `test` block. */
export const TEST_FUNCTION = /^\$test\.\d+$/;

/** A `?` met while a function's result is inferred; `error` is unset for an optional. */
export interface InferredPropagation {
  readonly error?: ValueType;
  readonly span: SourceSpan;
}

// With no written result, `?` converts nothing, so each operand must join the
// inferred result (spec/05-expressions.md#r-expr.try.convert.inferred-closure).
export function mismatchedPropagation(
  result: ValueType,
  propagations: readonly InferredPropagation[],
): { readonly message: string; readonly span: SourceSpan } | undefined {
  for (const propagation of propagations) {
    if (propagation.error === undefined) {
      if (optionalInner(result) === undefined)
        return {
          message: `? on an optional needs an optional result, but the inferred result is ${result}`,
          span: propagation.span,
        };
    } else if (resultParts(result)?.error !== propagation.error) {
      return {
        message: `? on a Result with error ${propagation.error} has no common type with the inferred result ${result}`,
        span: propagation.span,
      };
    }
  }
  return undefined;
}

// Whether a test body's result implements std.process.Termination
// (spec/05-expressions.md#r-expr.try.test.termination). The prototype has no
// `ExitCode`, so it accepts `void` and `Result[T, E]` with a terminating `T`
// and a `Display` error.
export function terminates(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
  implementations: readonly HirTraitImplementation[],
): boolean {
  if (type === "void" || type === "never") return true;
  const parts = resultParts(readonlyType(type));
  return (
    parts !== undefined &&
    terminates(parts.ok, traitTypes, implementations) &&
    implementsDisplay(parts.error, traitTypes, implementations)
  );
}

function implementsDisplay(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
  implementations: readonly HirTraitImplementation[],
): boolean {
  const target = readonlyType(type);
  if (["i32", "f64", "bool", "char", "string"].includes(target)) return true;
  const display = traitTypes.get("Display")!;
  if (
    implementations.some((implementation) =>
      Boolean(matchTraitImplementation(implementation, display.index, target, [])),
    )
  )
    return true;
  // A dynamic trait value whose trait extends Display, such as the erased Error.
  const traitName = traitTypeName(target);
  return traitName !== undefined && extendsTrait(traitName, display.name, traitTypes, new Set());
}

function extendsTrait(
  name: string,
  ancestor: string,
  traitTypes: ReadonlyMap<string, HirTrait>,
  seen: Set<string>,
): boolean {
  if (name === ancestor) return true;
  if (seen.has(name)) return false;
  seen.add(name);
  const trait = traitTypes.get(name);
  return (
    trait !== undefined &&
    trait.supertraits.some((supertrait) =>
      extendsTrait(supertrait.traitName, ancestor, traitTypes, seen),
    )
  );
}
