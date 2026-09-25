import type { FunctionDecl } from "../ast.ts";
import type { HirFunction, HirGlobal, HirTraitImplementation } from "../hir.ts";
import { FunctionChecker } from "./checker.ts";
import { type CheckResult, type Signature } from "./context.ts";
import { checkModuleInitialization } from "./module-initialization.ts";

import type { ProgramCheckContext } from "./program-context.ts";

export function lowerCheckedProgram(
  context: ProgramCheckContext,
  declarations: readonly FunctionDecl[],
  signatures: ReadonlyMap<string, Signature>,
): CheckResult {
  const {
    program,
    diagnostics,
    imports,
    dataTypes,
    enumTypes,
    traitTypes,
    implementationPreparations,
    inherentMethods,
  } = context;
  const implementations: HirTraitImplementation[] = implementationPreparations.map(
    (implementation, index) => ({
      index,
      traitIndex: implementation.trait.index,
      traitName: implementation.trait.name,
      targetType: implementation.targetType,
      methodFunctions: implementation.methods.map((method) => ({
        methodIndex: method.methodIndex,
        functionIndex: signatures.get(method.declaration.name)!.index,
      })),
      span: implementation.declaration.span,
    }),
  );

  const functions: HirFunction[] = [];
  const closures: HirFunction[] = [];
  const globals = new Map<string, HirGlobal>();
  const moduleDeclaration = declarations.find(
    (declaration) => program.statements.length > 0 && declaration.body === program.statements,
  );
  const checkingOrder = moduleDeclaration
    ? [
        moduleDeclaration,
        ...declarations.filter((declaration) => declaration !== moduleDeclaration),
      ]
    : declarations;
  const checkedFunctions = new Map<number, HirFunction>();
  checkingOrder.forEach((declaration) => {
    const signature = signatures.get(declaration.name)!;
    const moduleBody = program.statements.length > 0 && declaration.body === program.statements;
    const checked = new FunctionChecker(
      declaration,
      signature,
      signatures,
      dataTypes,
      enumTypes,
      traitTypes,
      implementations,
      inherentMethods,
      !program.functions.includes(declaration),
      moduleBody,
      closures,
      false,
      new Map(),
      -1,
      new Map(),
      false,
      false,
      undefined,
      imports,
      globals,
    ).check();
    diagnostics.push(...checked.diagnostics);
    if (checked.function) checkedFunctions.set(checked.function.index, checked.function);
  });
  declarations.forEach((declaration) => {
    const checked = checkedFunctions.get(signatures.get(declaration.name)!.index);
    if (checked) functions.push(checked);
  });
  if (!diagnostics.some((diagnostic) => diagnostic.severity !== "warning")) {
    diagnostics.push(
      ...checkModuleInitialization(
        moduleDeclaration
          ? checkedFunctions.get(signatures.get(moduleDeclaration.name)!.index)
          : undefined,
        functions,
        closures,
      ),
    );
  }
  return diagnostics.some((diagnostic) => diagnostic.severity !== "warning")
    ? { diagnostics }
    : {
        program: {
          data: [...dataTypes.values()],
          enums: [...enumTypes.values()],
          traits: [...traitTypes.values()],
          implementations,
          globals: [...globals.values()],
          functions,
          closures,
        },
        diagnostics,
      };
}
