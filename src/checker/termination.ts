import type { FunctionDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirTrait, HirTraitImplementation, ValueType } from "../hir.ts";
import { optionalInner, readonlyType, resultParts } from "../types.ts";
import { numericType } from "../numeric.ts";
import { matchTraitImplementation, traitTypeName } from "./shared.ts";
import { HIDDEN_EXIT_CODE, HIDDEN_TERMINATION } from "./standard-traits.ts";

/** The synthetic function name of an `it(...)` test case. */
export const TEST_FUNCTION = /^\$test\.\d+$/;

/** A `?` met while a function's result is inferred; `error` is unset for an optional. */
export interface InferredPropagation {
  readonly error?: ValueType;
  readonly span: SourceSpan;
}

// With no written result, `?` converts nothing, so each operand must join the
// inferred result (spec/lang/05-expressions.md#r-expr.try.convert.inferred-closure).
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

/** The local name of `std.process.Termination`, declared when std.process is imported. */
export function terminationTraitName(imports: ReadonlyMap<string, string>): string {
  for (const [local, qualified] of imports)
    if (qualified === "std.process.Termination") return local;
  return HIDDEN_TERMINATION;
}

/**
 * Whether the host can read an entry result's exit code: `void`, `ExitCode`,
 * or a `Result` over one. The prototype does not call a program's own
 * `Termination` implementation for `main`.
 */
export function runnableEntryResult(
  type: ValueType,
  imports: ReadonlyMap<string, string>,
): boolean {
  if (type === "void" || type === "never") return true;
  const target = readonlyType(type);
  const parts = resultParts(target);
  if (parts) return runnableEntryResult(parts.ok, imports);
  for (const [local, qualified] of imports)
    if (qualified === "std.process.ExitCode") return target === local;
  return target === HIDDEN_EXIT_CODE;
}

// Whether a test body's or entry point's result implements
// std.process.Termination (spec/lang/10-modules.md#exit-status): `void`,
// `Result[T, E]` with a terminating `T` and a `Display` error, or a type with
// a `Termination` implementation, such as `ExitCode`.
export function terminates(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
  implementations: readonly HirTraitImplementation[],
  imports: ReadonlyMap<string, string>,
): boolean {
  if (type === "void" || type === "never") return true;
  const target = readonlyType(type);
  const parts = resultParts(target);
  if (parts)
    return (
      terminates(parts.ok, traitTypes, implementations, imports) &&
      implementsDisplay(parts.error, traitTypes, implementations)
    );
  const termination = traitTypes.get(terminationTraitName(imports));
  return (
    termination !== undefined &&
    implementations.some((implementation) =>
      Boolean(matchTraitImplementation(implementation, termination.index, target, [])),
    )
  );
}

function implementsDisplay(
  type: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
  implementations: readonly HirTraitImplementation[],
): boolean {
  const target = readonlyType(type);
  if (numericType(target) || ["bool", "char", "string", "ConsoleError"].includes(target))
    return true;
  const display = traitTypes.get("Display");
  if (!display) return false;
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

/**
 * Why a test body's or public `main`'s result fails its `Termination` bound
 * (05-expressions.md#r-expr.try.test.explicit-closure and
 * 10-modules.md#r-module.entry.result-termination), if it does.
 */
export function resultFailure(
  declaration: FunctionDecl,
  synthetic: boolean,
  result: ValueType,
  traitTypes: ReadonlyMap<string, HirTrait>,
  implementations: readonly HirTraitImplementation[],
  imports: ReadonlyMap<string, string>,
): { readonly message: string; readonly span: SourceSpan } | undefined {
  const bounded = terminates(result, traitTypes, implementations, imports);
  if (TEST_FUNCTION.test(declaration.name) && !bounded)
    return {
      message: `a test body's result '${result}' does not implement std.process.Termination`,
      span: declaration.span,
    };
  if (
    declaration.name === "main" &&
    declaration.public &&
    !synthetic &&
    !(bounded && runnableEntryResult(result, imports))
  )
    return {
      message: `the result '${result}' of public main does not implement std.process.Termination, or is not yet supported: the prototype runs void, ExitCode, and Result over them`,
      span: declaration.result.span,
    };
  return undefined;
}
