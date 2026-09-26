import type { FunctionDecl } from "../ast.ts";
import type { HirFunction, HirGlobal, HirTraitImplementation } from "../hir.ts";
import { FunctionChecker } from "./checker.ts";
import { type CheckResult, type Signature } from "./context.ts";
import { checkModuleInitialization } from "./module-initialization.ts";
import { SignatureInference } from "./program-inference.ts";
import { matchTraitImplementation, substituteGenericType } from "./shared.ts";

import type { ImplementationPreparation, ProgramCheckContext } from "./program-context.ts";

function supertraitImplementationIndices(
  implementation: ImplementationPreparation,
  implementations: readonly ImplementationPreparation[],
): number[] {
  const traitSubstitutions = new Map(
    implementation.trait.genericParameters.map(
      (parameter, index) => [parameter, implementation.traitArguments[index]!] as const,
    ),
  );
  return implementation.trait.supertraits.map((supertrait) => {
    const expectedArguments = supertrait.traitArguments.map((argument) =>
      substituteGenericType(argument, traitSubstitutions),
    );
    return implementations.findIndex((candidate) =>
      Boolean(
        matchTraitImplementation(
          candidate,
          supertrait.traitIndex,
          implementation.targetType,
          expectedArguments,
        ),
      ),
    );
  });
}

export function lowerCheckedProgram(
  context: ProgramCheckContext,
  declarations: readonly FunctionDecl[],
  declaredSignatures: ReadonlyMap<string, Signature>,
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
    hostCapabilities,
  } = context;
  const implementations: HirTraitImplementation[] = implementationPreparations.map(
    (implementation, index) => ({
      index,
      traitIndex: implementation.trait.index,
      traitName: implementation.trait.name,
      traitArguments: implementation.traitArguments,
      targetType: implementation.targetType,
      associatedTypes: implementation.associatedTypes,
      genericParameters: implementation.declaration.genericParameters,
      genericBounds:
        implementation.methods.length === 0
          ? []
          : declaredSignatures
              .get(implementation.methods[0]!.declaration.name)!
              .genericBounds.filter((bound) =>
                implementation.declaration.genericParameters.includes(bound.parameter),
              ),
      supertraitImplementations: supertraitImplementationIndices(
        implementation,
        implementationPreparations,
      ),
      methodFunctions: implementation.methods.map((method) => ({
        methodIndex: method.methodIndex,
        functionIndex: declaredSignatures.get(method.declaration.name)!.index,
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
  const inference = new SignatureInference(
    context,
    declarations,
    declaredSignatures,
    implementations,
    moduleDeclaration,
  );
  const signatures: ReadonlyMap<string, Signature> = inference.active
    ? inference.signatures
    : declaredSignatures;
  if (inference.active) {
    inference.inferRows();
    inference.useGlobals(globals);
  }
  const checkedFunctions = new Map<number, HirFunction>();
  checkingOrder.forEach((declaration) => {
    const signature = signatures.get(declaration.name)!;
    if (inference.failed.has(declaration.name)) return;
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
    if (checked.function)
      checkedFunctions.set(
        checked.function.index,
        declaration.name === "main" && declaration.public
          ? { ...checked.function, entry: true }
          : checked.function,
      );
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
        implementations,
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
          hostCapabilities: [...hostCapabilities],
          initializer: moduleDeclaration
            ? signatures.get(moduleDeclaration.name)!.index
            : undefined,
        },
        diagnostics,
      };
}
