import type { SourceSpan } from "../diagnostics.ts";
import { extendsInspectable, usesStandardInspect } from "./inspectable.ts";
import type { FunctionDecl } from "../ast.ts";
import type { HirAssociatedBinding } from "../hir.ts";
import { mutableInner, nominalGenericParts, nominalGenericType, resultParts } from "../types.ts";
import { PRELUDE_NAMES, type Signature } from "./context.ts";
import {
  collectRowParameterReferences,
  firstPrivateSignatureType,
  normalizeBoundProjections,
  normalizedRequirements,
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
  const { program, diagnostics, dataTypes, enumTypes, traitTypes, hostCapabilities } = context;
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
    const categoryParameters = { AnyRef: new Set<string>(), AnyVal: new Set<string>() };
    const boundProjections = new Set<string>();
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
      const mutableKeys = new Set(
        bound.traits.flatMap((sourceTraitName) => {
          const inner = mutableInner(sourceTraitName);
          return inner === undefined ? [] : [inner];
        }),
      );
      return bound.traits.flatMap((sourceTraitName) => {
        const traitKey = mutableInner(sourceTraitName) ?? sourceTraitName;
        const mutable = mutableKeys.has(traitKey);
        const application = nominalGenericParts(traitKey);
        const traitName = application?.name ?? traitKey;
        // A repeated trait adds no requirement and is not diagnosed
        // (09-traits.md#generic-bounds-and-static-dispatch).
        if (seen.has(traitKey)) return [];
        seen.add(traitKey);
        if (traitName === "AnyRef" || traitName === "AnyVal") {
          categoryParameters[traitName].add(bound.parameter);
          return [];
        }
        if (traitName === "Any") return [];
        const trait = traitTypes.get(traitName);
        if (!trait) {
          diagnostics.push({
            code: "unknown-trait",
            message: `unknown trait '${traitName}'`,
            span: bound.span,
          });
          return [];
        }
        const traitArguments = (application?.arguments ?? []).map((argument) =>
          resolveGenericType(argument, new Set(typeParameters), new Set(rowParameters)),
        );
        if (traitArguments.length !== trait.genericParameters.length) {
          diagnostics.push({
            code: "generic-arity",
            message: `trait '${trait.name}' expects ${trait.genericParameters.length} type arguments`,
            span: bound.span,
          });
          return [];
        }
        const associatedBindings: HirAssociatedBinding[] = [];
        for (const binding of bound.bindings ?? []) {
          if (binding.trait !== traitKey) continue;
          if (!trait.associatedTypes.some((associated) => associated.name === binding.name)) {
            diagnostics.push({
              code: "unknown-associated-type",
              message: `trait '${trait.name}' declares no associated type '${binding.name}'`,
              span: binding.span,
            });
            continue;
          }
          const projection = `${bound.parameter}::${binding.name}`;
          if (boundProjections.has(projection)) {
            diagnostics.push({
              code: "duplicate-associated-binding",
              message: `projection '${projection}' is bound more than once`,
              span: binding.span,
            });
            continue;
          }
          boundProjections.add(projection);
          associatedBindings.push({
            name: binding.name,
            type: resolveGenericType(
              binding.type.name,
              new Set(typeParameters),
              new Set(rowParameters),
            ),
          });
        }
        return [
          {
            parameter: bound.parameter,
            traitName: trait.name,
            traitIndex: trait.index,
            traitArguments,
            mutable,
            ...(associatedBindings.length > 0 ? { associatedBindings } : {}),
          },
        ];
      });
    });
    if (
      declaration.genericParameters.length > 0 &&
      declaration.name === "main" &&
      declaration.public
    ) {
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
      return type && parameter.variadic ? nominalGenericType("List", [type]) : type;
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
    const normalizedParameters = parameters.map((type) =>
      normalizeBoundProjections(type!, genericBounds),
    );
    const normalizedResult = normalizeBoundProjections(result, genericBounds);
    // Sort after resolution: the parser sorts raw names, but a resolved row
    // parameter gains a `row:` prefix, and function types sort the final keys.
    const requirements = normalizedRequirements(
      declaration.requirements
        .flatMap((requirement) => resolveGenericRequirement(requirement, rowParameterSet))
        .map((requirement) =>
          rowParameterName(requirement)
            ? requirement
            : resolveGenericType(requirement, genericParameters, new Set(rowParameters)),
        ),
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
          (requirement) => !rowParameterName(requirement) && !hostCapabilities.has(requirement),
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
      referenceParameters: [...categoryParameters.AnyRef],
      valueParameters: [...categoryParameters.AnyVal],
      rowParameters,
      parameters: normalizedParameters,
      parameterNames: declaration.parameters.map((parameter) => parameter.name),
      defaultFunctionNames: declaration.parameters.map((parameter) =>
        parameter.default ? `$parameter-default.${declaration.name}.${parameter.name}` : undefined,
      ),
      variadic: declaration.parameters.at(-1)?.variadic === true,
      result: normalizedResult,
      requirements,
      span: declaration.span,
    });
  });
  return signatures;
}

// 11 Requirement Rows: an Inspectable trait is never a requirement key.
export function checkInspectableRequirements(
  context: ProgramCheckContext,
  declarations: readonly FunctionDecl[],
): void {
  for (const declaration of declarations)
    for (const requirement of declaration.requirements)
      inspectableRequirement(context, requirement.replace(/^mut\s+/, ""), declaration.span);
}

function inspectableRequirement(
  context: ProgramCheckContext,
  requirement: string,
  span: SourceSpan,
): boolean {
  const keyTrait =
    nominalGenericParts(requirement)?.name ?? mutableInner(requirement) ?? requirement;
  if (!usesStandardInspect(context.imports) || !extendsInspectable(context.traitTypes, keyTrait))
    return false;
  context.diagnostics.push({
    code: "inspectable-requirement",
    message: `'${keyTrait}' extends Inspectable, so it cannot be a requirement key`,
    span,
  });
  return true;
}
