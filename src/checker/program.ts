import type { Program } from "../ast.ts";
import type { CheckResult } from "./context.ts";
import { createProgramDeclarations } from "./program-declarations.ts";
import { checkEmbeddedMemberConflicts, checkEmbeddingLimits } from "./program-embedding.ts";
import { prepareImplementations } from "./program-implementations.ts";
import { lowerCheckedProgram } from "./program-lower.ts";
import { createProgramSignatures } from "./program-signatures.ts";
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
  standardImportAliases,
  testRunnerNames,
  withStandardLibrary,
} from "./standard-library.ts";
import { ImportBindingMap } from "./import-bindings.ts";
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
import { withDistinctMethodBinders, displayMethodBinderNames } from "./generic-method-scope.ts";
import { inherentVarianceDiagnostics, varianceDiagnostics } from "./variance.ts";
import { rowRuleDiagnostics } from "./row-rules.ts";
import { withTypeDeclarations } from "./type-declarations.ts";
import { defaultBoundDiagnostics, withTypeDefaults } from "./type-defaults.ts";
import { withTypedDerivation, withTypedDerivationSupport } from "./typed-derivation.ts";

import { withErrorDerivation } from "./error-derivation.ts";
import { setHashableKeyTypes } from "./map-keys.ts";
import { sourceSpanKey, type Diagnostic, type SourceSpan } from "../diagnostics.ts";

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
  const spelled = withFunctionTypeConstructors(withBareMarkerCalls(source, markers));
  // A malformed spelled function type leaves no type to check against.
  if (spelled.diagnostics.length > 0) return { diagnostics: [...spelled.diagnostics] };
  // Join std before typed derivation so @derive on a lib/std declaration is
  // lowered by the same pass as a program declaration. The import identities
  // and runner names come from the written program, before the one-module join.
  const prepared = prepareForDerivation(spelled.program);
  const derived = withTypedDerivation(prepared.program);
  if (derived.diagnostics.some((diagnostic) => diagnostic.severity !== "warning"))
    return { diagnostics: [...derived.diagnostics] };
  // Literal marker facts are checked on the declarations that survive
  // derivation, matching the original phase order.
  const result = checkProgram(withSuffixMarkers(derived.program), options, prepared);
  // A member that fails the walker's bound is reported at the opt-in
  // (spec/lang/14-annotations.md#r-annot.walker.obligation.error).
  const sameSpan = (span: SourceSpan, diagnostic: Diagnostic): boolean =>
    sourceSpanKey(span) === sourceSpanKey(diagnostic.span);
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
    const key = `${sourceSpanKey(diagnostic.span)}:${failing}`;
    if (reported.has(key)) return false;
    reported.add(key);
    return true;
  });
  return {
    ...result,
    diagnostics: [...derived.diagnostics, ...unique],
  };
}

interface PreparedProgram {
  readonly program: Program;
  readonly standardAliases: ReadonlyMap<string, string>;
  readonly testRunners: ReturnType<typeof testRunnerNames>;
  readonly runnerCapabilities: readonly string[];
}

function prepareForDerivation(source: Program): PreparedProgram {
  const standardAliases = standardImportAliases(source);
  const testRunners = testRunnerNames(source);
  const runnerCapabilities = [
    ...(source.tests.some((test) => test.table || test.timed) ? [testRunners.test] : []),
    ...(source.tests.some((test) => test.property) ? [testRunners.property] : []),
  ];
  const joined = withStandardLibrary(source);
  // Inspect is itself compiler-loaded and may contain derivations; derivation
  // support in turn imports inspect. The two idempotent loads form the small
  // dependency fixed point before derivation runs.
  const inspected = withStandardTraits(joined);
  const supported = withStandardTraits(withTypedDerivationSupport(inspected));
  const program = withBareMarkerCalls(supported, markerFunctions(supported.functions));
  return { program, standardAliases, testRunners, runnerCapabilities };
}

function checkProgram(
  source: Program,
  options: CheckOptions,
  prepared: Omit<PreparedProgram, "program">,
): CheckResult {
  const spellings = new Map<string, string>();
  const result = checkProgramRaw(source, options, spellings, prepared);
  return {
    ...result,
    diagnostics: result.diagnostics.map((diagnostic) => ({
      ...diagnostic,
      message: displayMethodBinderNames(diagnostic.message, spellings),
    })),
  };
}

function checkProgramRaw(
  source: Program,
  options: CheckOptions,
  spellings: Map<string, string>,
  prepared: Omit<PreparedProgram, "program">,
): CheckResult {
  const { standardAliases, testRunners, runnerCapabilities } = prepared;
  const hoisted = hoistLocalDeclarations(withDistinctMethodBinders(source, spellings));
  // Target kinds are checked before newtypes are lowered to data types
  // (spec/lang/14-annotations.md#target-kinds).
  const targetDiagnostics = checkDecoratorTargets(hoisted.program);
  // Written types take their omitted defaults before aliases expand
  // (04-type-system.md#type-argument-defaults).
  const defaulted = withTypeDefaults(hoisted.program, standardAliases);
  if (defaulted.diagnostics.length > 0)
    return {
      diagnostics: [...hoisted.diagnostics, ...targetDiagnostics, ...defaulted.diagnostics],
    };
  const declared = withTypeDeclarations(defaulted.program, standardAliases);
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
    standardAliases,
    dataTypes: new ImportBindingMap(standardAliases),
    enumTypes: new ImportBindingMap(standardAliases),
    traitTypes: new ImportBindingMap(standardAliases),
    implementationPreparations: [],
    inherentMethods: [],
    inherentDeclarations: [],
    hostCapabilities: new Set([
      "Console",
      ...runnerCapabilities,
      ...(options.hostCapabilities ?? []),
    ]),
    testRunners,
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
  // Public methods already have explicit result types: validation above
  // rejects an omitted result before any declarations are prepared.
  context.diagnostics.push(
    ...inherentVarianceDiagnostics(
      { data: context.dataTypes, enums: context.enumTypes, traits: context.traitTypes },
      context.program.implementations,
    ),
  );
  context.diagnostics.push(...defaultBoundDiagnostics(context));
  checkEmbeddingLimits(context);
  checkEmbeddedMemberConflicts(context);
  const declarations = createProgramDeclarations(context);
  if (!declarations) return { diagnostics: context.diagnostics };
  const signatures = createProgramSignatures(context, declarations);
  if (context.diagnostics.some((diagnostic) => diagnostic.severity !== "warning"))
    return { diagnostics: context.diagnostics };
  return lowerCheckedProgram(context, declarations, signatures);
}
