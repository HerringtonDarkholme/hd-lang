import type { FunctionDecl, ImplDecl, Program, TypeDecl } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import type { HirData, HirEnum, HirTrait, NumericFamily, ValueType } from "../hir.ts";
import type { InherentMethod } from "./context.ts";
import type { TestRunnerNames } from "./standard-library.ts";

export interface ImplementationMethodPreparation {
  readonly methodIndex: number;
  readonly declaration: FunctionDecl;
}

export interface ImplementationPreparation {
  readonly declaration: ImplDecl;
  readonly trait: HirTrait;
  readonly targetType: ValueType;
  readonly traitArguments: readonly ValueType[];
  readonly associatedTypes: readonly ValueType[];
  readonly methods: ImplementationMethodPreparation[];
  /** A numeric-family implementation's parameter types (09-traits.md#r-trait.target.numeric-family). */
  readonly family?: NumericFamily;
}

export interface ProgramCheckContext {
  readonly program: Program;
  /** Accepted written type declarations, retained after aliases and newtypes are lowered. */
  readonly typeDeclarations: readonly TypeDecl[];
  readonly diagnostics: Diagnostic[];
  readonly imports: Map<string, string>;
  /** Extra local spellings of joined standard-library declarations. */
  readonly standardAliases: ReadonlyMap<string, string>;
  readonly dataTypes: Map<string, HirData>;
  readonly enumTypes: Map<string, HirEnum>;
  readonly traitTypes: Map<string, HirTrait>;
  readonly implementationPreparations: ImplementationPreparation[];
  readonly inherentMethods: InherentMethod[];
  readonly inherentDeclarations: FunctionDecl[];
  readonly hostCapabilities: ReadonlySet<string>;
  /** The program's names of the test runner's capabilities (checker/standard-library.ts). */
  readonly testRunners: TestRunnerNames;
  /** The program is an entry module (spec/lang/10-modules.md#r-module.entry.private-main.warn). */
  readonly entryModule: boolean;
}
