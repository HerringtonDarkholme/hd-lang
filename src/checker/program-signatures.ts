import type { FunctionDecl } from "../ast.ts";
import type { ValueType } from "../hir.ts";
import { nominalGenericParts, nominalGenericType, resultParts } from "../types.ts";
import { MVP_HOST_CAPABILITIES, PRELUDE_NAMES, type Signature } from "./context.ts";
import {
  collectRowParameterReferences,
  firstPrivateSignatureType,
  resolveGenericRequirement,
  resolveGenericType,
  rowParameterName,
  typeName,
} from "./shared.ts";

import type { ProgramCheckContext } from "./program-context.ts";

export function createProgramSignatures(
  context: ProgramCheckContext,
  declarations: readonly FunctionDecl[],
): Map<string, Signature> {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  const signatures = new Map<string, Signature>();
  declarations.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `function '${declaration.name}' shadows a prelude name`,
        span: declaration.span,
      });
      return;
    }
    if (signatures.has(declaration.name)) {
      diagnostics.push({
        code: "duplicate-module-name",
        message: `function '${declaration.name}' is declared more than once`,
        span: declaration.span,
      });
      return;
    }
    if (
      dataTypes.has(declaration.name) ||
      enumTypes.has(declaration.name) ||
      traitTypes.has(declaration.name)
    ) {
      diagnostics.push({
        code: "duplicate-module-name",
        message: `'${declaration.name}' is already declared as a type`,
        span: declaration.span,
      });
      return;
    }
    const declaredGenerics = new Set(declaration.genericParameters);
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${parameter}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const rowParameterSet = new Set<string>();
    for (const requirement of declaration.requirements) {
      const base = requirement.split("\\")[0]!;
      if (declaredGenerics.has(base)) rowParameterSet.add(base);
    }
    declaration.parameters.forEach((parameter) =>
      collectRowParameterReferences(parameter.type.name, declaredGenerics, rowParameterSet),
    );
    collectRowParameterReferences(declaration.result.name, declaredGenerics, rowParameterSet);
    const rowParameters = declaration.genericParameters.filter((parameter) =>
      rowParameterSet.has(parameter),
    );
    const typeParameters = declaration.genericParameters.filter(
      (parameter) => !rowParameterSet.has(parameter),
    );
    const referenceParameters = new Set<string>();
    const genericBounds = declaration.genericBounds.flatMap((bound) => {
      if (rowParameterSet.has(bound.parameter)) {
        diagnostics.push({
          code: "generic-kind-conflict",
          message: `generic parameter '${bound.parameter}' cannot be both a type and a requirement row`,
          span: bound.span,
        });
        return [];
      }
      const seen = new Set<string>();
      return bound.traits.flatMap((traitName) => {
        if (seen.has(traitName)) {
          diagnostics.push({
            code: "duplicate-trait-bound",
            message: `trait '${traitName}' bounds '${bound.parameter}' more than once`,
            span: bound.span,
          });
          return [];
        }
        seen.add(traitName);
        if (traitName === "Reference") {
          referenceParameters.add(bound.parameter);
          return [];
        }
        const trait = traitTypes.get(traitName);
        if (!trait) {
          diagnostics.push({
            code: "unknown-trait",
            message: `unknown trait '${traitName}'`,
            span: bound.span,
          });
          return [];
        }
        return [{ parameter: bound.parameter, traitName: trait.name, traitIndex: trait.index }];
      });
    });
    if (declaration.genericParameters.length > 0 && declaration.name === "main") {
      diagnostics.push({
        code: "generic-entry-point",
        message: "main cannot declare generic parameters",
        span: declaration.span,
      });
      return;
    }
    const genericParameters = new Set(typeParameters);
    declaration.parameters.forEach((parameter, parameterIndex) => {
      if (parameter.variadic && parameterIndex !== declaration.parameters.length - 1) {
        diagnostics.push({
          code: "nonfinal-vararg",
          message: `variadic parameter '${parameter.name}' must be the final parameter`,
          span: parameter.span,
        });
      }
    });
    const parameters = declaration.parameters.map((parameter) => {
      const type = typeName(
        parameter.type,
        dataTypes,
        enumTypes,
        traitTypes,
        diagnostics,
        genericParameters,
        new Set(rowParameters),
      );
      return type && parameter.variadic ? nominalGenericType("list", [type]) : type;
    });
    const result = typeName(
      declaration.result,
      dataTypes,
      enumTypes,
      traitTypes,
      diagnostics,
      genericParameters,
      new Set(rowParameters),
    );
    if (parameters.some((type) => type === undefined) || !result) return;
    const requirements = declaration.requirements
      .flatMap((requirement) => resolveGenericRequirement(requirement, rowParameterSet))
      .map((requirement) =>
        rowParameterName(requirement)
          ? requirement
          : resolveGenericType(requirement, genericParameters, new Set(rowParameters)),
      );
    for (const requirement of requirements) {
      if (rowParameterName(requirement)) continue;
      const nominal = nominalGenericParts(requirement);
      if (!nominal) continue;
      const trait = traitTypes.get(nominal.name);
      if (!trait) {
        diagnostics.push({
          code: "unknown-requirement",
          message: `unknown generic requirement key '${requirement}'`,
          span: declaration.span,
        });
        continue;
      }
      if (trait.genericParameters.length !== nominal.arguments.length) {
        diagnostics.push({
          code: "generic-arity",
          message: `trait '${trait.name}' expects ${trait.genericParameters.length} type arguments`,
          span: declaration.span,
        });
      }
    }
    if (declaration.public) {
      const privateType =
        declaration.parameters
          .map((parameter) => firstPrivateSignatureType(parameter.type.name, program))
          .find((candidate) => candidate !== undefined) ??
        firstPrivateSignatureType(declaration.result.name, program);
      const privateRequirement = declaration.requirements
        .map((requirement) => firstPrivateSignatureType(requirement.split("\\")[0]!, program))
        .find((candidate) => candidate !== undefined);
      if (privateType || privateRequirement) {
        const leaked = privateType ?? privateRequirement!;
        diagnostics.push({
          code: "private-type-leak",
          message: `public function '${declaration.name}' exposes private type or trait '${leaked}'`,
          span: declaration.span,
        });
      }
      if (declaration.name === "main") {
        const nonhost = requirements.find(
          (requirement) =>
            !rowParameterName(requirement) && !MVP_HOST_CAPABILITIES.has(requirement),
        );
        if (nonhost) {
          diagnostics.push({
            code: "nonhost-entry-requirement",
            message: `entry point requirement '${nonhost}' is not supplied by the MVP host profile`,
            span: declaration.span,
          });
        }
        const entryResult = resultParts(result);
        if (
          result !== "void" &&
          !(entryResult?.ok === "void" && entryResult.error === "ConsoleError")
        ) {
          diagnostics.push({
            code: "entry-error-not-display",
            message:
              "public main must return void or Result[void, E] with a supported Display error",
            span: declaration.result.span,
          });
        }
        if (declaration.parameters.length > 0) {
          diagnostics.push({
            code: "entry-point-parameters",
            message: "public main cannot declare source-level parameters",
            span: declaration.span,
          });
        }
      }
    }
    signatures.set(declaration.name, {
      name: declaration.name,
      index,
      suspending: declaration.suspending,
      genericParameters: typeParameters,
      genericBounds,
      referenceParameters: [...referenceParameters],
      rowParameters,
      parameters: parameters as ValueType[],
      parameterNames: declaration.parameters.map((parameter) => parameter.name),
      defaultFunctionNames: declaration.parameters.map((parameter) =>
        parameter.default ? `$parameter-default.${declaration.name}.${parameter.name}` : undefined,
      ),
      variadic: declaration.parameters.at(-1)?.variadic === true,
      result,
      requirements,
      span: declaration.span,
    });
  });
  return signatures;
}
