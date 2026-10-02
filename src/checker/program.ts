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
import {
  importedMarkerFunctions,
  withStandardLibrary,
  withStandardSubmodules,
} from "./standard-library.ts";
import {
  checkDecoratorTargets,
  markerFunctions,
  withBareMarkerCalls,
  withSuffixMarkers,
} from "./decorators.ts";
import { withStandardTraits } from "./standard-traits.ts";
import { standardUseDiagnostics } from "./standard-uses.ts";
import { withFunctionTypeConstructors } from "./function-types.ts";
import { hoistLocalDeclarations } from "./local-declarations.ts";
import { varianceDiagnostics } from "./variance.ts";
import { rowRuleDiagnostics } from "./row-rules.ts";
import { withTypeDeclarations } from "./type-declarations.ts";
import { defaultBoundDiagnostics, withTypeDefaults } from "./type-defaults.ts";
import { withTypedDerivation } from "./typed-derivation.ts";

import { withErrorDerivation } from "./error-derivation.ts";
import { setHashableKeyTypes } from "./map-keys.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";

export interface CheckOptions {
  readonly hostCapabilities?: readonly string[];
  /**
   * The program is an entry module, so a `main` that is not pub warns
   * (spec/lang/10-modules.md#r-module.entry.private-main.warn).
   */
  readonly entryModule?: boolean;
}

export function check(written: Program, options: CheckOptions = {}): CheckResult {
  // A `std` use must name a std module and its declarations
  // (spec/lang/10-modules.md#use-forms); nothing else is checked without them.
  const uses = standardUseDiagnostics(written);
  if (uses.length > 0) return { diagnostics: uses };
  // `@error` is an intrinsic, lowered before any decorator is resolved
  // (spec/lang/14-annotations.md#error-derivation).
  const errors = withErrorDerivation(written);
  if (errors.diagnostics.length > 0) return { diagnostics: [...errors.diagnostics] };
  const source = errors.program;
  // A bare decorator name of a function with no parameters is a call
  // (spec/lang/14-annotations.md#r-annot.decorator.bare-call).
  const markers = new Set([
    ...markerFunctions(source.functions),
    ...importedMarkerFunctions(source),
  ]);
  const spelled = withFunctionTypeConstructors(
    withBareMarkerCalls(withStandardSubmodules(source), markers),
  );
  // A malformed spelled function type leaves no type to check against.
  if (spelled.diagnostics.length > 0) return { diagnostics: [...spelled.diagnostics] };
  // Typed derivation is lowered to ordinary implementations first
  // (spec/lang/14-annotations.md#typed-derivation).
  const derived = withTypedDerivation(spelled.program);
  if (derived.diagnostics.some((diagnostic) => diagnostic.severity !== "warning"))
    return { diagnostics: [...derived.diagnostics] };
  const result = checkProgram(derived.program, options);
  // A member that fails the walker's bound is reported at the opt-in
  // (spec/lang/14-annotations.md#r-annot.walker.obligation.error).
  const sameSpan = (span: SourceSpan, diagnostic: Diagnostic): boolean =>
    span.start.offset === diagnostic.span.start.offset &&
    span.end.offset === diagnostic.span.end.offset;
  const remapped = result.diagnostics.map((diagnostic): Diagnostic => {
    // A member that fails derived `Arbitrary`'s bounds is reported at the
    // opt-in, naming the member (std-testing.arbitrary.derive.not-derivable).
    const arbitrary =
      diagnostic.code === "unsatisfied-trait-bound"
        ? derived.arbitraryOptIns.find((optIn) => sameSpan(optIn.span, diagnostic))
        : undefined;
    if (arbitrary) {
      const failing = /type '([^']*)'/.exec(diagnostic.message)?.[1]?.replaceAll(" ", "");
      const members = arbitrary.members.filter(
        (member) => member.type.replaceAll(" ", "") === failing,
      );
      return members.length === 0
        ? diagnostic
        : {
            ...diagnostic,
            message: `${members.map((member) => `member '${member.name}'`).join(", ")} cannot be derived: ${diagnostic.message}; write the impl of Arbitrary by hand`,
          };
    }
    return diagnostic.code === "unsatisfied-trait-bound" &&
      derived.optInSpans.some((span) => sameSpan(span, diagnostic))
      ? {
          ...diagnostic,
          code: "member-not-derivable",
          message: `a member does not satisfy the walker's, describer's, or source's bound: ${diagnostic.message}`,
        }
      : diagnostic;
  });
  // A template with several walkers, as `Debug`'s, puts one member's
  // obligation on each of them; the member is reported once.
  const reported = new Set<string>();
  const unique = remapped.filter((diagnostic) => {
    if (diagnostic.code !== "member-not-derivable") return true;
    const failing = /type '([^']*)'/.exec(diagnostic.message)?.[1] ?? diagnostic.message;
    const key = `${diagnostic.span.start.offset}:${diagnostic.span.end.offset}:${failing}`;
    if (reported.has(key)) return false;
    reported.add(key);
    return true;
  });
  return { ...result, diagnostics: [...derived.diagnostics, ...unique] };
}

function checkProgram(source: Program, options: CheckOptions): CheckResult {
  // The traits that `withStandardTraits` declares mention std names, such as
  // `Display`, which the standard library then declares.
  const joined = withStandardLibrary(withStandardTraits(source));
  const marked = withSuffixMarkers(withBareMarkerCalls(joined, markerFunctions(joined.functions)));
  const hoisted = hoistLocalDeclarations(marked);
  // Target kinds are checked before newtypes are lowered to data types
  // (spec/lang/14-annotations.md#target-kinds).
  const targetDiagnostics = checkDecoratorTargets(hoisted.program);
  // Written types take their omitted defaults before aliases expand
  // (04-type-system.md#type-argument-defaults).
  const defaulted = withTypeDefaults(hoisted.program);
  if (defaulted.diagnostics.length > 0)
    return {
      diagnostics: [...hoisted.diagnostics, ...targetDiagnostics, ...defaulted.diagnostics],
    };
  const declared = withTypeDeclarations(defaulted.program);
  const program = declared.program;
  setHashableKeyTypes(program);
  const context: ProgramCheckContext = {
    program,
    diagnostics: [
      ...hoisted.diagnostics,
      ...targetDiagnostics,
      ...declared.diagnostics,
      ...rowRuleDiagnostics(program),
    ],
    imports: new Map(),
    dataTypes: new Map(),
    enumTypes: new Map(),
    traitTypes: new Map(),
    implementationPreparations: [],
    inherentMethods: [],
    inherentDeclarations: [],
    hostCapabilities: new Set(["Console", ...(options.hostCapabilities ?? [])]),
    entryModule: options.entryModule === true,
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
  context.diagnostics.push(...defaultBoundDiagnostics(context));
  checkEmbeddingLimits(context);
  checkEmbeddedMemberConflicts(context);
  const declarations = createProgramDeclarations(context);
  if (!declarations) return { diagnostics: context.diagnostics };
  const signatures = createProgramSignatures(context, declarations);
  checkInspectableRequirements(context, declarations);
  if (context.diagnostics.some((diagnostic) => diagnostic.severity !== "warning"))
    return { diagnostics: context.diagnostics };
  return lowerCheckedProgram(context, declarations, signatures);
}
