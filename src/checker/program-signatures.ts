import { ambiguousProjection, bindingNameProblem } from "./associated-bindings.ts";
import { listVararg, tupleVararg, type FunctionDecl, type Program, type TypeRef } from "../ast.ts";
import type { HirAssociatedBinding, HirGenericBound, HirTrait, ValueType } from "../hir.ts";
import { mutableInner, nominalGenericParts, displayType } from "../types.ts";
import { PRELUDE_NAMES, type Signature } from "./context.ts";
import { TUPLE_TRAIT } from "./standard-traits.ts";
import { requirementKeyDiagnostics, resolveRequirementKeyTypes } from "./requirement-keys.ts";
import {
  collectRowParameterReferences,
  firstPrivateSignatureType,
  normalizeBoundProjections,
  normalizedRequirements,
  isKnownType,
  resolveGenericRequirement,
  resolveGenericType,
  resolveTraitType,
  rowParameterName,
  typeName,
} from "./shared.ts";

import type { ProgramCheckContext } from "./program-context.ts";
import { ImportBindingMap } from "./import-bindings.ts";
import { traitImpliesValueCategory } from "./value-categories.ts";

/** A declaration's `tests:`-block, suffix, and prefix markers, as signature fields. */
function signatureMarkers(
  declaration: FunctionDecl,
): Pick<Signature, "testOnly" | "numSuffix" | "strPrefix" | "intrinsic"> {
  return {
    ...(declaration.testOnly ? { testOnly: true } : {}),
    ...(declaration.intrinsic ? { intrinsic: declaration.intrinsic } : {}),
    ...(declaration.numSuffix ? { numSuffix: true } : {}),
    ...(declaration.strPrefix ? { strPrefix: declaration.strPrefix } : {}),
  };
}

/**
 * A declaration's type-argument defaults, resolved with its generic
 * parameters in scope (04-type-system.md#type-argument-defaults).
 */
function genericDefaultTypes(
  declaration: FunctionDecl,
  context: ProgramCheckContext,
  typeParameters: readonly string[],
): Pick<Signature, "genericDefaults"> {
  const written = Object.entries(declaration.genericDefaults ?? {});
  if (written.length === 0) return {};
  const { dataTypes, enumTypes, traitTypes, diagnostics } = context;
  const scope = new Set(typeParameters);
  return {
    genericDefaults: new Map(
      written.map(([name, type]) => [
        name,
        typeName(
          type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          scope,
          new Set(),
          new Set(),
          {},
          declaration.genericBounds,
        ) ?? "void",
      ]),
    ),
  };
}

/** One declared parameter or result type, resolved with the declaration's generics in scope. */
function signatureDeclaredType(
  type: TypeRef,
  declaration: FunctionDecl,
  context: ProgramCheckContext,
  genericParameters: ReadonlySet<string>,
  rowParameters: ReadonlySet<string>,
  hashable: ReadonlySet<string>,
): ValueType | undefined {
  return typeName(
    type,
    context.dataTypes,
    context.enumTypes,
    context.traitTypes,
    context.diagnostics,
    genericParameters,
    rowParameters,
    hashable,
    {},
    declaration.genericBounds,
  );
}

/**
 * The type parameters that may key a map: those bounded by `Eq` and `Hash`
 * (09-traits.md#r-trait.hash.map-key), and in std every one, for `Iterable`
 * on `Map[K, V]` (lib/std/iter.hd).
 */
function hashableParameters(
  declaration: FunctionDecl,
  typeParameters: readonly string[],
): Set<string> {
  return new Set(
    typeParameters.filter((name) =>
      ["Eq", "Hash"].every(
        (trait) =>
          declaration.standard === true ||
          declaration.genericBounds.some(
            (bound) => bound.parameter === name && bound.traits.includes(trait),
          ),
      ),
    ),
  );
}

function resolvedRequirementKey(
  requirement: string,
  genericParameters: ReadonlySet<string>,
  rowParameters: ReadonlySet<string>,
  traitTypes: ProgramCheckContext["traitTypes"],
): string {
  return resolveRequirementKeyTypes(
    resolveGenericType(requirement, genericParameters, rowParameters),
    (type) => resolveTraitType(type, traitTypes),
  );
}

function addImpliedValueCategories(
  bounds: readonly HirGenericBound[],
  categories: { readonly AnyRef: Set<string>; readonly AnyVal: Set<string> },
  traitTypes: ReadonlyMap<string, HirTrait>,
): void {
  for (const bound of bounds) {
    const trait = traitTypes.get(bound.traitName);
    if (!trait) continue;
    if (traitImpliesValueCategory(trait, traitTypes, "AnyRef"))
      categories.AnyRef.add(bound.parameter);
    if (traitImpliesValueCategory(trait, traitTypes, "AnyVal"))
      categories.AnyVal.add(bound.parameter);
  }
}

/**
 * The private nominal type a public signature exposes, if any. The
 * declaration's own generic parameters shadow outer type names, so they
 * resolve before the lookup: a parameter written `T` is `generic:T`,
 * never a same-named user type.
 */
function privateSignatureLeak(
  declaration: FunctionDecl,
  program: Program,
  typeParameters: readonly string[],
  rowParameters: readonly string[],
  rowParameterSet: ReadonlySet<string>,
): string | undefined {
  const inScope = new Set(typeParameters);
  const inScopeRows = new Set(rowParameters);
  const privateType =
    declaration.parameters
      .map((parameter) =>
        firstPrivateSignatureType(
          resolveGenericType(parameter.type.name, inScope, inScopeRows),
          program,
        ),
      )
      .find((candidate) => candidate !== undefined) ??
    firstPrivateSignatureType(
      resolveGenericType(declaration.result.name, inScope, inScopeRows),
      program,
    );
  return (
    privateType ??
    declaration.requirements
      .flatMap((requirement) => resolveGenericRequirement(requirement, rowParameterSet))
      .map((requirement) => firstPrivateSignatureType(requirement, program))
      .find((candidate) => candidate !== undefined)
  );
}

export function createProgramSignatures(
  context: ProgramCheckContext,
  declarations: readonly FunctionDecl[],
): Map<string, Signature> {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes, hostCapabilities } = context;
  const signatures = new ImportBindingMap<Signature>(context.standardAliases);
  declarations.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name) && !declaration.standard) {
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
    for (const parameter of declaration.genericParameters)
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${displayType(parameter)}' shadows a prelude name`,
          span: declaration.span,
        });
    // A parameter declared `$R` is a row parameter (11-requirements-and-suspension.md#r-req.row.param.marked).
    const rowParameterSet = new Set<string>(declaration.rowParameters ?? []);
    for (const requirement of declaration.requirements) {
      if (declaredGenerics.has(requirement)) rowParameterSet.add(requirement);
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
    const categoryParameters = {
      AnyRef: new Set<string>(),
      AnyVal: new Set<string>(),
      Tuple: new Set<string>(),
    };
    const boundProjections = new Set<string>();
    const genericBounds = declaration.genericBounds.flatMap((bound) => {
      if (rowParameterSet.has(bound.parameter)) {
        diagnostics.push({
          code: "generic-kind-conflict",
          message: `generic parameter '${displayType(bound.parameter)}' cannot be both a type and a requirement row`,
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
        // `std.function.Tuple` is a sealed marker that every tuple type
        // implements; it passes no dictionary (fn.type.ctor.tuple-trait).
        if (trait?.standardName === TUPLE_TRAIT) {
          categoryParameters.Tuple.add(bound.parameter);
          return [];
        }
        if (!trait) {
          diagnostics.push({
            code: "unknown-trait",
            message: `unknown trait '${displayType(traitName)}'`,
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
            message: `trait '${displayType(trait.name)}' expects ${trait.genericParameters.length} type arguments`,
            span: bound.span,
          });
          return [];
        }
        const associatedBindings: HirAssociatedBinding[] = [];
        for (const binding of bound.bindings ?? []) {
          if (binding.trait !== traitKey) continue;
          // A binding may name a supertrait's associated type (trait.binding.name-reach).
          const problem = bindingNameProblem(trait, binding.name, traitTypes);
          if (problem) {
            diagnostics.push({ ...problem, span: binding.span });
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
    addImpliedValueCategories(genericBounds, categoryParameters, traitTypes);
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
    const hashable = hashableParameters(declaration, typeParameters);
    declaration.parameters.forEach((parameter, parameterIndex) => {
      if (parameter.variadic && parameterIndex !== declaration.parameters.length - 1) {
        diagnostics.push({
          code: "nonfinal-vararg",
          message: `variadic parameter '${parameter.name}' must be the final parameter`,
          span: parameter.span,
        });
      }
    });
    const parameters = declaration.parameters.map((parameter) =>
      signatureDeclaredType(
        parameter.type,
        declaration,
        context,
        genericParameters,
        new Set(rowParameters),
        hashable,
      ),
    );
    const result =
      signatureDeclaredType(
        declaration.result,
        declaration,
        context,
        genericParameters,
        new Set(rowParameters),
        hashable,
      ) ?? "void";
    if (parameters.some((type) => type === undefined) || !result) return;
    const ambiguous = [...parameters, result]
      .map((type) => ambiguousProjection(type!, genericBounds, traitTypes))
      .find((problem) => problem !== undefined);
    if (ambiguous) {
      diagnostics.push({ ...ambiguous, span: declaration.span });
      return;
    }
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
            : resolvedRequirementKey(requirement, genericParameters, rowParameterSet, traitTypes),
        ),
    );
    diagnostics.push(
      ...requirementKeyDiagnostics(requirements, traitTypes, declaration.span, (type) =>
        isKnownType(resolveTraitType(type, traitTypes), dataTypes, enumTypes, traitTypes),
      ),
    );
    if (declaration.public) {
      const leaked = privateSignatureLeak(
        declaration,
        program,
        typeParameters,
        rowParameters,
        rowParameterSet,
      );
      if (leaked) {
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
            message: `entry point requirement '${displayType(nonhost)}' is not supplied by the default host profile`,
            span: declaration.span,
          });
        }
        // The result's `Termination` bound is checked with the body.
        if (declaration.parameters.length > 0) {
          diagnostics.push({
            code: "entry-point-parameters",
            message: "public main cannot declare source-level parameters",
            span: declaration.span,
          });
        }
      }
    }
    // A top-level `main` that is not pub is an ordinary function, so warn
    // that it is not the entry point (10-modules.md#r-module.entry.private-main.warn).
    if (context.entryModule && declaration.name === "main" && !declaration.public)
      diagnostics.push({
        code: "private-main",
        message: "main is not pub, so it is not the entry point",
        span: declaration.span,
        severity: "warning",
      });
    signatures.set(declaration.name, {
      name: declaration.name,
      index,
      suspending: declaration.suspending,
      genericParameters: typeParameters,
      genericBounds,
      referenceParameters: [...categoryParameters.AnyRef],
      valueParameters: [...categoryParameters.AnyVal],
      ...(categoryParameters.Tuple.size > 0
        ? { tupleParameters: [...categoryParameters.Tuple] }
        : {}),
      rowParameters,
      ...(rowParameters.length > 0 ? { typeArgumentOrder: declaration.genericParameters } : {}),
      parameters: normalizedParameters,
      parameterNames: declaration.parameters.map((parameter) => parameter.name),
      defaultFunctionNames: declaration.parameters.map((parameter) =>
        parameter.default ? `$parameter-default.${declaration.name}.${parameter.name}` : undefined,
      ),
      variadic: listVararg(declaration.parameters.at(-1)),
      ...(tupleVararg(declaration.parameters.at(-1)) ? { tupleVararg: true } : {}),
      result: normalizedResult,
      requirements,
      ...signatureMarkers(declaration),
      ...genericDefaultTypes(declaration, context, typeParameters),
      span: declaration.span,
    });
  });
  return signatures;
}
