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
  DEFAULT_PROFILE_TRAITS,
  defaultProfileNames,
  testRunnerNames,
  withReexportedUses,
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
import { withModulePaths } from "./module-paths.ts";
import { registerPackageOwnership } from "./package-ownership.ts";
import { withFunctionTypeConstructors } from "./function-types.ts";
import { hoistLocalDeclarations } from "./local-declarations.ts";
import { typeParameterRedeclarations } from "./type-parameter-names.ts";
import { inherentVarianceDiagnostics, varianceDiagnostics } from "./variance.ts";
import { rowRuleDiagnostics } from "./row-rules.ts";
import { unconstrainedImplementationParameters } from "./impl-parameters.ts";
import { withTypeDeclarations } from "./type-declarations.ts";
import { defaultBoundDiagnostics, withTypeDefaults } from "./type-defaults.ts";
import { withTypedDerivation, withTypedDerivationSupport } from "./typed-derivation.ts";

import { withErrorDerivation } from "./error-derivation.ts";
import { withEntryErrorRenderer } from "./entry-error.ts";
import { setHashableKeyTypes } from "./map-keys.ts";
import { sourceSpanKey, type Diagnostic, type SourceSpan } from "../diagnostics.ts";
import { testTierNames, withTestTierNotes } from "./test-tier-notes.ts";
import {
  debugPrinters,
  debugPrintState,
  needsDebugPrinters,
  registerDebugPrint,
  type DebugPrintOptions,
  type DebugPrintState,
} from "./debug-print.ts";

export interface CheckOptions extends DebugPrintOptions {
  readonly hostCapabilities?: readonly string[];
  /**
   * The program is an entry module, so a `main` that is not pub warns
   * (spec/lang/10-modules.md#r-module.entry.private-main.warn).
   */
  readonly entryModule?: boolean;
  /**
   * The program is an integration test program (spec/lang/10-modules.md#r-module.test.integration.program):
   * each test case's row takes the default profile's traits
   * (spec/cli/command-line.md#r-cli.test.env.integration) and `Process`,
   * which `hd test` binds to the package's executables and tasks
   * (spec/cli/command-line.md#r-cli.test.process), and only such a program
   * may call `hd_run!`.
   */
  readonly integrationTest?: boolean;
  /**
   * The integration test program is a doc test
   * (spec/lang/10-modules.md#doc-tests): it takes the rows of one, but it
   * may not call `hd_run!` (spec/std/testing.md#r-std-testing.hd-run.integration-only).
   */
  readonly docTest?: boolean;
  /**
   * A test build, as `hd test` and `hd check --tests` make: it runs no
   * entry behavior, so a script with a `tests:` block initializes its top
   * level requirement-free (spec/lang/10-modules.md#r-module.init.tests.requirement-free).
   */
  readonly testBuild?: boolean;
}

export function check(written: Program, options: CheckOptions = {}): CheckResult {
  // A `pub use` re-export names its origin module before validation
  // (spec/lang/10-modules.md#r-module.path.no-std-child-import).
  const sourced = withReexportedUses(written);
  // A `std` use must name a std module and its declarations
  // (spec/lang/10-modules.md#use-forms); nothing else is checked without them.
  const uses = standardUseDiagnostics(sourced);
  if (uses.length > 0) return { diagnostics: uses };
  // Module paths and each package module's scope resolve to joined
  // spellings before any name is looked up (checker/module-paths.ts).
  const paths = withModulePaths(sourced);
  if (paths.diagnostics.length > 0) return { diagnostics: [...paths.diagnostics] };
  // `@error` is an intrinsic, lowered before any decorator is resolved
  // (spec/lang/14-annotations.md#error-derivation).
  const errors = withErrorDerivation(paths.program);
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
  // An entry point that returns a `Result` gets the renderer of its `.Err`
  // report (spec/lang/10-modules.md#r-module.entry.err-stderr).
  const result = checkWithDebugPrinters(
    withEntryErrorRenderer(withSuffixMarkers(derived.program)),
    options,
    prepared,
  );
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
  // A test tier's missing requirement names its fix (checker/test-tier-notes.ts).
  const names = testTierNames(
    prepared.defaultProfile,
    DEFAULT_PROFILE_TRAITS.map(([, name]) => name),
    prepared.testRunners.process,
  );
  return {
    ...result,
    diagnostics: withTestTierNotes([...derived.diagnostics, ...unique], written, options, names),
  };
}

interface PreparedProgram {
  readonly program: Program;
  readonly standardAliases: ReadonlyMap<string, string>;
  readonly testRunners: ReturnType<typeof testRunnerNames>;
  readonly runnerCapabilities: readonly string[];
  /** The program's names of the default profile's traits (standard-library.ts). */
  readonly defaultProfile: readonly string[];
}

function prepareForDerivation(source: Program): PreparedProgram {
  const standardAliases = standardImportAliases(source);
  const testRunners = testRunnerNames(source);
  const runnerCapabilities = [
    ...(source.tests.length > 0 ? [testRunners.test] : []),
    ...(source.tests.some((test) => test.property) ? [testRunners.property] : []),
  ];
  const joined = withStandardLibrary(source);
  // Inspect is itself compiler-loaded and may contain derivations; derivation
  // support in turn imports inspect. The two idempotent loads form the small
  // dependency fixed point before derivation runs.
  const inspected = withStandardTraits(joined);
  const supported = withStandardTraits(withTypedDerivationSupport(inspected));
  const program = withBareMarkerCalls(supported, markerFunctions(supported.functions));
  const defaultProfile = defaultProfileNames(source);
  return { program, standardAliases, testRunners, runnerCapabilities, defaultProfile };
}

/**
 * Checks `source`, in two passes when it calls `dbg` (checker/debug-print.ts):
 * the first finds each call's argument types, and the second checks the
 * calls as calls of the printers generated for those types.
 */
function checkWithDebugPrinters(
  source: Program,
  options: CheckOptions,
  prepared: Omit<PreparedProgram, "program">,
): CheckResult {
  const first = debugPrintState(source, options);
  const result = checkProgram(source, options, prepared, first);
  const failed = result.diagnostics.some((diagnostic) => diagnostic.severity !== "warning");
  // A fetched package's calls print nothing, and it warns once
  // (spec/lang/10-modules.md#r-module.dbg.dependency.warning).
  const quiet: Diagnostic[] = [...first.quiet].map(([name, span]) => ({
    code: "dbg-in-dependency",
    severity: "warning",
    message: `dependency ${name} calls dbg; its calls print nothing`,
    span,
  }));
  if (failed || !result.program || !needsDebugPrinters(first))
    return { ...result, diagnostics: [...result.diagnostics, ...quiet] };
  const { functions, plans } = debugPrinters(first, result.program, source.span);
  const second = checkProgram(
    { ...source, functions: [...source.functions, ...functions] },
    options,
    prepared,
    debugPrintState(source, options, plans),
  );
  // The second pass checks the same written code, so its warnings are the
  // first's; an error there is the generated code's.
  if (!second.program || second.diagnostics.some(({ severity }) => severity !== "warning"))
    return second;
  return { ...second, diagnostics: [...result.diagnostics, ...quiet] };
}

function checkProgram(
  source: Program,
  options: CheckOptions,
  prepared: Omit<PreparedProgram, "program">,
  debugPrint: DebugPrintState,
): CheckResult {
  const { standardAliases, testRunners, runnerCapabilities, defaultProfile } = prepared;
  // A reused type parameter name is reported before local declarations
  // hoist (03-names-and-scopes.md#r-names.type-param.no-redeclare).
  const redeclared = typeParameterRedeclarations(source);
  if (redeclared.length > 0) return { diagnostics: [...redeclared] };
  const hoisted = hoistLocalDeclarations(source);
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
    typeDeclarations: declared.typeDeclarations,
    diagnostics: [
      ...hoisted.diagnostics,
      ...targetDiagnostics,
      ...declared.diagnostics,
      ...rowRuleDiagnostics(program),
      ...unconstrainedImplementationParameters(program),
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
      // The default profile (spec/cli/command-line.md#r-cli.host.default-profile);
      // the entry row selects which of its traits a run binds (cli.host.entry-row).
      ...defaultProfile,
      ...runnerCapabilities,
      // `hd test` binds `Process` for an integration test (spec/cli/command-line.md#r-cli.test.process).
      ...(options.integrationTest ? [testRunners.process] : []),
      ...(options.hostCapabilities ?? []),
    ]),
    testRunners,
    defaultProfile,
    entryModule: options.entryModule === true,
    testBuild: options.testBuild === true,
    integrationTest: options.integrationTest === true,
    runsExecutables: options.integrationTest === true && options.docTest !== true,
  };
  // Every function check of the program shares its trait map, so it finds
  // which package declares what through it (checker/package-ownership.ts).
  registerPackageOwnership(context.traitTypes, program);
  registerDebugPrint(context.traitTypes, debugPrint);
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
