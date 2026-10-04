import { shareCapturedLocals } from "./captured-cells.ts";
import type { FunctionDecl } from "../ast.ts";
import type { HirFunction, HirGenericBound, HirGlobal, HirTraitImplementation } from "../hir.ts";
import { FunctionChecker } from "./checker.ts";
import { type CheckResult, type Signature } from "./context.ts";
import { checkModuleInitialization } from "./module-initialization.ts";
import { implementationVisibleFrom } from "./program-implementations.ts";
import { SignatureInference } from "./program-inference.ts";
import { matchTraitImplementation, substituteGenericType } from "./shared.ts";
import { inherentVarianceDiagnostics } from "./variance.ts";

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
  traitSubstitutions.set("Self", implementation.targetType);
  return implementation.trait.supertraits.map((supertrait) => {
    const expectedArguments = supertrait.traitArguments.map((argument) =>
      substituteGenericType(argument, traitSubstitutions),
    );
    return implementations.findIndex(
      (candidate) =>
        implementationVisibleFrom(candidate.declaration, implementation.declaration) &&
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

// An implementation with no methods has no signature to carry its bounds, so
// its plain trait bounds are read from the declaration; the proof of a bound
// still needs them (09-traits.md#generic-bounds-and-static-dispatch).
function markerImplementationBounds(
  implementation: ImplementationPreparation,
  traitTypes: ProgramCheckContext["traitTypes"],
): HirGenericBound[] {
  return implementation.declaration.genericBounds.flatMap((bound) =>
    bound.traits.flatMap((traitName) => {
      const trait = traitTypes.get(traitName);
      return trait && trait.genericParameters.length === 0
        ? [
            {
              parameter: bound.parameter,
              traitName: trait.name,
              traitIndex: trait.index,
              traitArguments: [],
              mutable: false,
            },
          ]
        : [];
    }),
  );
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
      ...(implementation.declaration.standard ? { standard: true as const } : {}),
      index,
      traitIndex: implementation.trait.index,
      traitName: implementation.trait.name,
      traitArguments: implementation.traitArguments,
      targetType: implementation.targetType,
      associatedTypes: implementation.associatedTypes,
      genericParameters: implementation.declaration.genericParameters,
      genericBounds:
        implementation.methods.length === 0
          ? markerImplementationBounds(implementation, traitTypes)
          : declaredSignatures
              .get(implementation.methods[0]!.declaration.name)!
              .genericBounds.filter((bound) =>
                implementation.declaration.genericParameters.includes(bound.parameter),
              ),
      supertraitImplementations: supertraitImplementationIndices(
        implementation,
        implementationPreparations,
      ),
      methodFunctions: implementation.methods.map((method) => {
        const signature = declaredSignatures.get(method.declaration.name)!;
        // A walker's `member` may take more dictionaries than the trait's
        // (spec/lang/14-annotations.md#r-annot.walker.strengthen-member).
        const strengthenable =
          program.traits[implementation.trait.index]?.strengthenableMembers?.includes(
            implementation.trait.methods[method.methodIndex]?.name ?? "",
          ) === true;
        const methodBounds = signature.genericBounds.filter(
          (bound) => !implementation.declaration.genericParameters.includes(bound.parameter),
        ).length;
        const traitBounds =
          implementation.trait.methods[method.methodIndex]?.genericBounds?.length ?? 0;
        return {
          methodIndex: method.methodIndex,
          functionIndex: signature.index,
          ...(strengthenable && methodBounds !== traitBounds ? { strengthened: true } : {}),
        };
      }),
      ...(implementation.family ? { family: implementation.family } : {}),
      ...(implementation.declaration.localImplementation !== undefined
        ? { localImplementation: implementation.declaration.localImplementation }
        : {}),
      // An implementation of intrinsic methods only is a declaration with no
      // code (09-traits.md#intrinsic-methods).
      ...(implementation.methods.length > 0 &&
      implementation.methods.every((method) => method.declaration.bodiless)
        ? { intrinsic: true as const }
        : {}),
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
  let hasPanicDetail = false;
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
      !program.functions.includes(declaration) || declaration.compilerGenerated === true,
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
      new Set(declaration.localImplementations ?? []),
    ).check();
    diagnostics.push(...checked.diagnostics);
    if (checked.hasPanicDetail) hasPanicDetail = true;
    if (checked.function)
      checkedFunctions.set(
        checked.function.index,
        declaration.name === "main"
          ? declaration.public
            ? { ...checked.function, entry: true }
            : { ...checked.function, developmentEntry: true }
          : declaration.bodiless
            ? { ...checked.function, intrinsicMethod: true }
            : checked.function,
      );
  });
  declarations.forEach((declaration) => {
    const checked = checkedFunctions.get(signatures.get(declaration.name)!.index);
    if (checked) functions.push(checked);
  });
  const preparedMethods = new Map(
    inherentMethods.map((method) => [method.sourceMethod, method.functionName]),
  );
  diagnostics.push(
    ...inherentVarianceDiagnostics(
      { data: dataTypes, enums: enumTypes, traits: traitTypes },
      program.implementations,
      (method) => {
        const functionName = preparedMethods.get(method);
        return functionName ? signatures.get(functionName)?.result : undefined;
      },
    ),
  );
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
          traits: [...traitTypes.values()].sort((left, right) => left.index - right.index),
          implementations,
          globals: [...globals.values()],
          ...shareCapturedLocals(functions, closures),
          hasPanicDetail,
          hostCapabilities: [...hostCapabilities],
          initializer: moduleDeclaration
            ? signatures.get(moduleDeclaration.name)!.index
            : undefined,
        },
        diagnostics,
      };
}
