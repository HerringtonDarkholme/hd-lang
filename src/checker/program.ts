import type { Program } from "../ast.ts";
import type { CheckResult } from "./context.ts";
import { createProgramDeclarations } from "./program-declarations.ts";
import { checkEmbeddedMemberConflicts, checkEmbeddingLimits } from "./program-embedding.ts";
import { prepareImplementations } from "./program-implementations.ts";
import { lowerCheckedProgram } from "./program-lower.ts";
import { checkInspectableRequirements, createProgramSignatures } from "./program-signatures.ts";
import {
  declareProgramTypes,
  defineProgramData,
  defineProgramEnums,
  defineProgramTraits,
} from "./program-types.ts";
import { validateProgram } from "./program-validation.ts";
import type { ProgramCheckContext } from "./program-context.ts";
import { validateHostCapabilities } from "./host-capabilities.ts";
import { importedMarkerFunctions, withStandardLibrary } from "./standard-library.ts";
import {
  checkDecoratorTargets,
  markerFunctions,
  withBareMarkerCalls,
  withSuffixMarkers,
} from "./decorators.ts";
import { withStandardTraits } from "./standard-traits.ts";
import { withFunctionTypeConstructors } from "./function-types.ts";
import { hoistLocalDeclarations } from "./local-declarations.ts";
import { varianceDiagnostics } from "./variance.ts";
import { withTypeDeclarations } from "./type-declarations.ts";
import { withTypedDerivation } from "./typed-derivation.ts";
import { withShapes } from "./shapes.ts";
import { setHashableKeyTypes } from "./shared.ts";
import type { Diagnostic } from "../diagnostics.ts";

export interface CheckOptions {
  readonly hostCapabilities?: readonly string[];
}

export function check(source: Program, options: CheckOptions = {}): CheckResult {
  // A bare decorator name of a function with no parameters is a call
  // (spec/14-annotations.md#r-annot.decorator.bare-call).
  const markers = new Set([
    ...markerFunctions(source.functions),
    ...importedMarkerFunctions(source),
  ]);
  const spelled = withFunctionTypeConstructors(withBareMarkerCalls(source, markers));
  // A malformed spelled function type leaves no type to check against.
  if (spelled.diagnostics.length > 0) return { diagnostics: [...spelled.diagnostics] };
  // Typed derivation is lowered to ordinary implementations first
  // (spec/14-annotations.md#typed-derivation).
  const derived = withTypedDerivation(spelled.program);
  if (derived.diagnostics.some((diagnostic) => diagnostic.severity !== "warning"))
    return { diagnostics: [...derived.diagnostics] };
  // Shape intrinsics call generated builders (spec/14-annotations.md#shape-intrinsics).
  const result = checkProgram(withShapes(derived.program), options);
  // A member that fails the walker's bound is reported at the opt-in
  // (spec/14-annotations.md#r-annot.walker.obligation.error).
  const remapped = result.diagnostics.map((diagnostic): Diagnostic =>
    diagnostic.code === "unsatisfied-trait-bound" &&
    derived.optInSpans.some(
      (span) =>
        span.start.offset === diagnostic.span.start.offset &&
        span.end.offset === diagnostic.span.end.offset,
    )
      ? {
          ...diagnostic,
          code: "member-not-derivable",
          message: `a member does not satisfy the walker's, describer's, or source's bound: ${diagnostic.message}`,
        }
      : diagnostic,
  );
  return { ...result, diagnostics: [...derived.diagnostics, ...remapped] };
}

function checkProgram(source: Program, options: CheckOptions): CheckResult {
  const joined = withStandardLibrary(source);
  const hoisted = hoistLocalDeclarations(
    withStandardTraits(
      withSuffixMarkers(withBareMarkerCalls(joined, markerFunctions(joined.functions))),
    ),
  );
  // Target kinds are checked before newtypes are lowered to data types
  // (spec/14-annotations.md#target-kinds).
  const targetDiagnostics = checkDecoratorTargets(hoisted.program);
  const declared = withTypeDeclarations(hoisted.program);
  const program = declared.program;
  setHashableKeyTypes(program);
  const context: ProgramCheckContext = {
    program,
    diagnostics: [...hoisted.diagnostics, ...targetDiagnostics, ...declared.diagnostics],
    imports: new Map(),
    dataTypes: new Map(),
    enumTypes: new Map(),
    traitTypes: new Map(),
    implementationPreparations: [],
    inherentMethods: [],
    inherentDeclarations: [],
    hostCapabilities: new Set(["Console", ...(options.hostCapabilities ?? [])]),
  };
  validateProgram(context);
  // A missing required result type leaves no signature to check against.
  if (context.diagnostics.some((diagnostic) => diagnostic.code === "missing-result-type"))
    return { diagnostics: context.diagnostics };
  declareProgramTypes(context);
  defineProgramData(context);
  defineProgramEnums(context);
  context.diagnostics.push(
    ...varianceDiagnostics({ data: context.dataTypes, enums: context.enumTypes }),
  );
  defineProgramTraits(context);
  validateHostCapabilities(context);
  prepareImplementations(context);
  checkEmbeddingLimits(context);
  checkEmbeddedMemberConflicts(context);
  const declarations = createProgramDeclarations(context);
  if (!declarations) return { diagnostics: context.diagnostics };
  const signatures = createProgramSignatures(context, declarations);
  checkInspectableRequirements(context, declarations);
  if (context.diagnostics.length > 0) return { diagnostics: context.diagnostics };
  return lowerCheckedProgram(context, declarations, signatures);
}
