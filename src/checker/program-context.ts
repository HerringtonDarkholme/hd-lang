import type { FunctionDecl, ImplDecl, Program } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import type { HirData, HirEnum, HirTrait, ValueType } from "../hir.ts";
import type { InherentMethod } from "./context.ts";

export interface ImplementationMethodPreparation {
  readonly methodIndex: number;
  readonly declaration: FunctionDecl;
}

export interface ImplementationPreparation {
  readonly declaration: ImplDecl;
  readonly trait: HirTrait;
  readonly targetType: ValueType;
  readonly methods: ImplementationMethodPreparation[];
}

export interface ProgramCheckContext {
  readonly program: Program;
  readonly diagnostics: Diagnostic[];
  readonly imports: Map<string, string>;
  readonly dataTypes: Map<string, HirData>;
  readonly enumTypes: Map<string, HirEnum>;
  readonly traitTypes: Map<string, HirTrait>;
  readonly implementationPreparations: ImplementationPreparation[];
  readonly inherentMethods: InherentMethod[];
  readonly inherentDeclarations: FunctionDecl[];
}
