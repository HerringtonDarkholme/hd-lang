import {
  listVararg,
  type DataDecl,
  type EnumDecl,
  type FunctionDecl,
  type GenericBound,
  type MethodDecl,
  type TraitDecl,
} from "../ast.ts";
import { extendsInspectable, usesStandardInspect } from "./inspectable.ts";
import { INSPECTABLE_MEMBERS } from "./standard-traits.ts";
import type { HirAssociatedBinding, HirData, HirDataField, HirTrait, ValueType } from "../hir.ts";
import {
  bindingParts,
  contextKeys,
  functionParts,
  inputsInner,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  restInner,
  resultParts,
  rowArgumentKeys,
  rowArgumentType,
  splitTypeBindings,
  tupleParts,
  displayType,
} from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import {
  requirementKeyDiagnostics,
  requirementKeyDiagnosticsInType,
  resolveRequirementKeyTypes,
} from "./requirement-keys.ts";
import {
  collectRowParameterReferences,
  firstPrivateSignatureType,
  isKnownType,
  normalizedRequirements,
  resolveGenericRequirement,
  resolveGenericType,
  resolveTraitType,
  rowParameterName,
  typeName,
} from "./shared.ts";
import {
  dynamicTraitProblemInType,
  enclosingBoundImplies,
  pushWrittenBoundProblem,
} from "./written-type-validation.ts";

import type { ProgramCheckContext } from "./program-context.ts";
import { traitImpliesValueCategory } from "./value-categories.ts";

export function declareProgramTypes(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  program.data.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name) && !declaration.standard) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`,
        span: declaration.span,
      });
      return;
    }
    if (dataTypes.has(declaration.name)) {
      diagnostics.push({
        code: "duplicate-type",
        message: `type '${declaration.name}' is already declared`,
        span: declaration.span,
      });
      return;
    }
    // A parameter used after `$` in a field type is a requirement row.
    const rows = new Set<string>();
    for (const field of declaration.fields)
      collectRowParameterReferences(field.type.name, new Set(declaration.genericParameters), rows);
    dataTypes.set(declaration.name, {
      name: declaration.name,
      index,
      genericParameters: declaration.genericParameters,
      ...((declaration.genericBounds ?? []).length > 0
        ? {
            declaredBounds: (declaration.genericBounds ?? []).map((bound) => ({
              parameter: bound.parameter,
              traits: [...bound.traits],
            })),
          }
        : {}),
      ...(rows.size > 0
        ? { rowParameters: declaration.genericParameters.filter((name) => rows.has(name)) }
        : {}),
      fields: [],
      ...(declaration.newtype ? { newtype: true as const } : {}),
      ...(declaration.local ? { local: true as const } : {}),
      ...(declaration.standard ? { standard: true as const } : {}),
      ...(declaration.standardName ? { standardName: declaration.standardName } : {}),
      ...(declaration.variances ? { variances: declaration.variances } : {}),
      span: declaration.span,
    });
  });
  program.enums.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name) && !declaration.standard) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`,
        span: declaration.span,
      });
      return;
    }
    if (dataTypes.has(declaration.name) || enumTypes.has(declaration.name)) {
      diagnostics.push({
        code: "duplicate-type",
        message: `type '${declaration.name}' is already declared`,
        span: declaration.span,
      });
      return;
    }
    enumTypes.set(declaration.name, {
      name: declaration.name,
      ...(declaration.standardName ? { standardName: declaration.standardName } : {}),
      index,
      genericParameters: declaration.genericParameters,
      ...((declaration.genericBounds ?? []).length > 0
        ? {
            declaredBounds: (declaration.genericBounds ?? []).map((bound) => ({
              parameter: bound.parameter,
              traits: [...bound.traits],
            })),
          }
        : {}),
      sharedFields: [],
      variants: [],
      fields: [],
      ...(declaration.local ? { local: true as const } : {}),
      ...(declaration.variances ? { variances: declaration.variances } : {}),
      span: declaration.span,
    });
  });
  program.traits.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name) && !declaration.standard) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`,
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
        code: "duplicate-type",
        message: `type '${declaration.name}' is already declared`,
        span: declaration.span,
      });
      return;
    }
    traitTypes.set(declaration.name, {
      name: declaration.name,
      ...(declaration.standardName ? { standardName: declaration.standardName } : {}),
      index,
      genericParameters: declaration.genericParameters,
      supertraits: [],
      associatedTypes: [],
      methods: [],
      span: declaration.span,
      ...(declaration.localImplementations
        ? { localImplementations: declaration.localImplementations }
        : {}),
    });
  });
  // `std.task.Waker`, a prelude name whose module has no `lib/std` file.
  traitTypes.set("Waker", {
    name: "Waker",
    index: program.traits.length,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [
      {
        name: "wake",
        index: 0,
        associated: false,
        genericParameters: [],
        suspending: false,
        receiverMutable: false,
        parameters: [],
        parameterNames: [],
        variadic: false,
        result: "void",
        requirements: [],
        span: program.span,
      },
    ],
    span: program.span,
  });
  // `Any`, the built-in universal empty trait of `std.core`: every value type
  // implements it (04-type-system.md#trait-values-and-any).
  traitTypes.set("Any", {
    name: "Any",
    index: program.traits.length + 1,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [],
    span: program.span,
  });
}

/**
 * A typed fact type's pattern, a type over the fact type's own parameters
 * (14-annotations.md#r-annot.typed-fact.pattern, .pattern.scope). An
 * unknown name in it is reported at the `@annotate` type argument.
 */
function resolveFactPattern(
  context: ProgramCheckContext,
  declaration: DataDecl | EnumDecl,
): ValueType | undefined {
  if (!declaration.factPattern) return undefined;
  const { dataTypes, enumTypes, traitTypes, diagnostics } = context;
  return typeName(
    declaration.factPattern,
    dataTypes,
    enumTypes,
    traitTypes,
    diagnostics,
    new Set(declaration.genericParameters),
    new Set(),
    new Set(),
    { validateRequirementKeys: false, validateDynamicSafety: false, validateWrittenBounds: false },
    declaration.genericBounds ?? [],
  );
}

export function defineProgramData(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  for (const declaration of program.data) {
    const data = dataTypes.get(declaration.name);
    if (!data) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${displayType(parameter)}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const names = new Set<string>();
    const fields = declaration.fields.map((field, index) => {
      if (names.has(field.name))
        diagnostics.push({
          code: field.embedded ? "duplicate-embedded-field" : "duplicate-field",
          message: `field '${field.name}' is declared more than once`,
          span: field.span,
        });
      names.add(field.name);
      const rowParameters = new Set(data.rowParameters ?? []);
      // A parameter bounded by `Eq` and `Hash` may key a map (trait.hash.map-key).
      const hashable = new Set(
        declaration.genericParameters.filter((name) =>
          ["Eq", "Hash"].every((trait) =>
            (declaration.genericBounds ?? []).some(
              (bound) => bound.parameter === name && bound.traits.includes(trait),
            ),
          ),
        ),
      );
      const resolved = typeName(
        field.type,
        dataTypes,
        enumTypes,
        traitTypes,
        diagnostics,
        new Set(declaration.genericParameters.filter((name) => !rowParameters.has(name))),
        rowParameters,
        hashable,
        {
          validateRequirementKeys: false,
          validateDynamicSafety: false,
          validateWrittenBounds: false,
        },
        declaration.genericBounds ?? [],
      );
      const type = resolved ?? "void";
      // An embedded field names a data type (08-data-and-enums.md#r-data.embed.data-only).
      const embeddedData = field.embedded
        ? dataTypes.get(nominalGenericParts(type)?.name ?? type)
        : undefined;
      if (field.embedded && type !== "void" && (!embeddedData || embeddedData.newtype))
        diagnostics.push({
          code: "embedded-non-data",
          message: `an embedded field must name a data type, not '${displayType(field.type.name)}'`,
          span: field.span,
        });
      // An embedded field is always public (08-data-and-enums.md#data-declarations).
      const leaked =
        declaration.public && field.embedded ? firstPrivateSignatureType(type, program) : undefined;
      if (leaked)
        diagnostics.push({
          code: "private-type-leak",
          message: `public data '${declaration.name}' embeds private type '${leaked}'; embedded fields are always public`,
          span: field.span,
        });
      return {
        ...(field.public || field.embedded ? { public: true } : {}),
        name: field.name,
        type,
        index,
        embedded: field.embedded,
        defaultFunctionName: field.default
          ? `$default.${declaration.name}.${field.name}`
          : undefined,
        span: field.span,
      };
    });
    const defaults = Object.entries(declaration.genericDefaults ?? {}).map(
      ([name, type]) =>
        [
          name,
          typeName(
            type,
            dataTypes,
            enumTypes,
            traitTypes,
            diagnostics,
            new Set(declaration.genericParameters),
            new Set(),
            new Set(),
            { validateRequirementKeys: false, validateDynamicSafety: false },
            declaration.genericBounds ?? [],
          ) ?? "void",
        ] as const,
    );
    const factPattern = resolveFactPattern(context, declaration);
    dataTypes.set(declaration.name, {
      ...data,
      fields,
      ...(defaults.length > 0 ? { genericDefaults: new Map(defaults) } : {}),
      ...(factPattern !== undefined ? { factPattern } : {}),
    });
  }
}

export function defineProgramEnums(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  for (const declaration of program.enums) {
    const enumType = enumTypes.get(declaration.name);
    if (!enumType) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${displayType(parameter)}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const variantNames = new Set<string>();
    const allFields: HirData["fields"][number][] = [];
    const sharedNames = new Set<string>();
    // A named payload member must not repeat a named shared field; unnamed
    // ones never clash (08-data-and-enums.md#r-data.shared.payload-names).
    const sharedNamed = new Set<string>();
    const sharedFields = declaration.sharedFields.map((field) => {
      if (!field.positional) sharedNamed.add(field.name);
      if (sharedNames.has(field.name))
        diagnostics.push({
          code: "duplicate-field",
          message: `shared enum field '${field.name}' is declared more than once`,
          span: field.span,
        });
      sharedNames.add(field.name);
      const resolved = typeName(
        field.type,
        dataTypes,
        enumTypes,
        traitTypes,
        diagnostics,
        new Set(declaration.genericParameters),
        new Set(),
        new Set(),
        {
          validateRequirementKeys: false,
          validateDynamicSafety: false,
          validateWrittenBounds: false,
        },
        declaration.genericBounds ?? [],
      );
      const type = resolved ?? "void";
      const checked = {
        name: field.name,
        type,
        index: allFields.length,
        defaultFunctionName: field.default
          ? `$enum-default.${declaration.name}.${field.name}`
          : undefined,
        span: field.span,
      };
      allFields.push(checked);
      return checked;
    });
    const variants = declaration.variants.map((variant, tag) => {
      if (variantNames.has(variant.name))
        diagnostics.push({
          code: "duplicate-variant",
          message: `variant '${variant.name}' is declared more than once`,
          span: variant.span,
        });
      variantNames.add(variant.name);
      const fieldNames = new Set<string>();
      const fields = variant.fields.map((field) => {
        if (!field.positional && sharedNamed.has(field.name))
          diagnostics.push({
            code: "duplicate-field",
            message: `payload field '${field.name}' duplicates a shared enum field`,
            span: field.span,
          });
        if (fieldNames.has(field.name))
          diagnostics.push({
            code: "duplicate-field",
            message: `payload field '${field.name}' is declared more than once`,
            span: field.span,
          });
        fieldNames.add(field.name);
        const resolved = typeName(
          field.type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          new Set(declaration.genericParameters),
          new Set(),
          new Set(),
          {
            validateRequirementKeys: false,
            validateDynamicSafety: false,
            validateWrittenBounds: false,
          },
          declaration.genericBounds ?? [],
        );
        const type = resolved ?? "void";
        const checked = {
          name: field.name,
          type,
          index: allFields.length,
          span: field.span,
        };
        allFields.push(checked);
        return checked;
      });
      return {
        name: variant.name,
        tag,
        fields,
        factoryFunctionName:
          sharedFields.length > 0 ? `$enum-variant.${declaration.name}.${variant.name}` : undefined,
        span: variant.span,
      };
    });
    const factPattern = resolveFactPattern(context, declaration);
    enumTypes.set(declaration.name, {
      ...enumType,
      sharedFields,
      variants,
      fields: allFields,
      ...(factPattern !== undefined ? { factPattern } : {}),
    });
  }
}

export function defineProgramTraits(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  for (const declaration of program.traits) {
    const trait = traitTypes.get(declaration.name);
    if (!trait) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${displayType(parameter)}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const supertraitNames = new Set<string>();
    const categorySupertraits = new Set<"AnyVal" | "AnyRef">();
    // A supertrait's arguments and bindings may name `Self`
    // (09-traits.md#supertrait-bindings).
    const supertraitGenerics = new Set([...declaration.genericParameters, "Self"]);
    const supertraits = declaration.supertraits.flatMap((reference) => {
      const resolved = resolveGenericType(reference.name, supertraitGenerics);
      const application = nominalGenericParts(resolved);
      const name = application?.name ?? resolved;
      // Value categories impose static obligations but have no dictionary
      // fields in the run-time supertrait layout.
      if (name === "AnyVal" || name === "AnyRef") {
        categorySupertraits.add(name);
        return [];
      }
      const supertrait = traitTypes.get(name);
      if (!supertrait) {
        diagnostics.push({
          code: "unknown-trait",
          message: `unknown supertrait '${reference.name}'`,
          span: reference.span,
        });
        return [];
      }
      if (supertraitNames.has(resolved)) {
        diagnostics.push({
          code: "duplicate-supertrait",
          message: `supertrait '${reference.name}' is listed more than once`,
          span: reference.span,
        });
        return [];
      }
      supertraitNames.add(resolved);
      const traitArguments = application?.arguments ?? [];
      if (traitArguments.length !== supertrait.genericParameters.length) {
        diagnostics.push({
          code: "generic-arity",
          message: `trait '${displayType(supertrait.name)}' expects ${supertrait.genericParameters.length} type arguments`,
          span: reference.span,
        });
        return [];
      }
      const associatedBindings: HirAssociatedBinding[] = [];
      const declared = program.traits[supertrait.index];
      for (const binding of declaration.supertraitBindings ?? []) {
        if (binding.trait !== reference.name) continue;
        // The supertrait's own supertraits may not be defined yet, so the
        // names it reaches come from the declarations (trait.binding.super.names).
        const declaring =
          declared?.name === supertrait.name
            ? declaringTraits(program.traits, declared, binding.name)
            : [];
        if (declaring.length !== 1) {
          diagnostics.push({
            code: declaring.length === 0 ? "unknown-associated-type" : "ambiguous-associated-type",
            message:
              declaring.length === 0
                ? `trait '${displayType(supertrait.name)}' declares or reaches no associated type '${binding.name}'`
                : `'${binding.name}' of '${displayType(supertrait.name)}' is ambiguous: ${declaring.join(" and ")} each declare it`,
            span: binding.span,
          });
          continue;
        }
        if (associatedBindings.some((existing) => existing.name === binding.name)) {
          diagnostics.push({
            code: "duplicate-associated-binding",
            message: `associated type '${binding.name}' of '${displayType(supertrait.name)}' is bound more than once`,
            span: binding.span,
          });
          continue;
        }
        associatedBindings.push({
          name: binding.name,
          type: resolveGenericType(binding.type.name, supertraitGenerics),
        });
      }
      return [
        {
          traitIndex: supertrait.index,
          traitName: supertrait.name,
          traitArguments,
          ...(associatedBindings.length > 0 ? { associatedBindings } : {}),
        },
      ];
    });
    const names = new Set<string>();
    const associatedTypes = declaration.associatedTypes.map((associated, index) => {
      if (names.has(associated.name))
        diagnostics.push({
          code: "duplicate-trait-member",
          message: `trait member '${associated.name}' is declared more than once`,
          span: associated.span,
        });
      names.add(associated.name);
      return { name: associated.name, index, span: associated.span };
    });
    const methods = declaration.methods.map((method, index) => {
      if (names.has(method.name))
        diagnostics.push({
          code: "duplicate-trait-member",
          message: `trait member '${method.name}' is declared more than once`,
          span: method.span,
        });
      names.add(method.name);
      const associated = method.parameters[0]?.name !== "self";
      // Trait-level parameters are type parameters, but after the kind error
      // recover one used in a row as symbolic so later validation does not
      // misreport it as an unknown trait.
      const illegalRows = new Set<string>();
      const traitParameters = new Set(
        declaration.genericParameters.filter(
          (parameter) => !method.genericParameters.includes(parameter),
        ),
      );
      method.parameters.forEach((parameter) =>
        collectRowParameterReferences(parameter.type.name, traitParameters, illegalRows),
      );
      collectRowParameterReferences(method.result.name, traitParameters, illegalRows);
      method.requirements.forEach((requirement) => {
        if (traitParameters.has(requirement)) illegalRows.add(requirement);
      });
      const rowParameters = new Set([...(method.rowParameters ?? []), ...illegalRows]);
      const memberGenerics = new Set([
        ...declaration.genericParameters.filter((parameter) => !rowParameters.has(parameter)),
        ...method.genericParameters.filter((parameter) => !rowParameters.has(parameter)),
        "Self",
        ...associatedTypes.map((associated) => `Self::${associated.name}`),
      ]);
      const sourceParameters = associated ? method.parameters : method.parameters.slice(1);
      sourceParameters.forEach((parameter, parameterIndex) => {
        if (parameter.variadic && parameterIndex !== sourceParameters.length - 1) {
          diagnostics.push({
            code: "nonfinal-vararg",
            message: `variadic parameter '${parameter.name}' must be the final parameter`,
            span: parameter.span,
          });
        }
      });
      const parameters = sourceParameters.map((parameter) => {
        const type = typeName(
          parameter.type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          memberGenerics,
          rowParameters,
          new Set(),
          { validateRequirementKeys: false, validateDynamicSafety: false },
          [...(declaration.genericBounds ?? []), ...method.genericBounds],
        );
        return type;
      });
      const result =
        typeName(
          method.result,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          memberGenerics,
          rowParameters,
          new Set(),
          { validateRequirementKeys: false, validateDynamicSafety: false },
          [...(declaration.genericBounds ?? []), ...method.genericBounds],
        ) ?? "void";
      const referenceParameters: string[] = [];
      const valueParameters: string[] = [];
      const genericBounds = method.genericBounds.flatMap((bound) =>
        bound.traits.flatMap((sourceTraitName) => {
          const mutable = mutableInner(sourceTraitName) !== undefined;
          const traitKey = mutableInner(sourceTraitName) ?? sourceTraitName;
          const application = nominalGenericParts(traitKey);
          const traitName = application?.name ?? traitKey;
          if (traitName === "AnyRef") {
            referenceParameters.push(bound.parameter);
            return [];
          }
          if (traitName === "AnyVal") {
            valueParameters.push(bound.parameter);
            return [];
          }
          const boundTrait = traitName === "Any" ? undefined : traitTypes.get(traitName);
          if (!boundTrait) return [];
          const traitArguments = (application?.arguments ?? []).map((argument) =>
            resolveGenericType(argument, memberGenerics, rowParameters),
          );
          return [
            {
              parameter: bound.parameter,
              traitName: boundTrait.name,
              traitIndex: boundTrait.index,
              traitArguments,
              mutable,
            },
          ];
        }),
      );
      const defaults = Object.entries(method.genericDefaults ?? {}).map(
        ([name, type]) =>
          [
            name,
            typeName(
              type,
              dataTypes,
              enumTypes,
              traitTypes,
              diagnostics,
              memberGenerics,
              rowParameters,
              new Set(),
              { validateRequirementKeys: false, validateDynamicSafety: false },
              [...(declaration.genericBounds ?? []), ...method.genericBounds],
            ) ?? "void",
          ] as const,
      );
      const requirements = normalizedRequirements(
        method.requirements
          .flatMap((requirement) => resolveGenericRequirement(requirement, rowParameters))
          .map((requirement) =>
            requirement.startsWith("row:")
              ? requirement
              : resolveRequirementKeyTypes(
                  resolveGenericType(requirement, memberGenerics, rowParameters),
                  (type) => resolveTraitType(type, traitTypes),
                ),
          ),
      );
      return {
        name: method.name,
        index,
        associated,
        genericParameters: method.genericParameters,
        ...(rowParameters.size > 0 ? { rowParameters: [...rowParameters] } : {}),
        genericBounds,
        ...(defaults.length > 0 ? { genericDefaults: new Map(defaults) } : {}),
        referenceParameters,
        valueParameters,
        suspending: method.suspending,
        receiverMutable: method.parameters[0]?.type.name === "mut:Self",
        parameters: parameters.map((parameter) => parameter ?? "void"),
        parameterNames: sourceParameters.map((parameter) => parameter.name),
        variadic: listVararg(sourceParameters.at(-1)),
        result,
        requirements,
        span: method.span,
      };
    });
    traitTypes.set(declaration.name, {
      ...trait,
      ...(categorySupertraits.size > 0 ? { categorySupertraits: [...categorySupertraits] } : {}),
      supertraits,
      associatedTypes,
      methods,
    });
  }
  addImpliedMethodValueCategories(traitTypes);
  diagnoseSupertraitCycles(context);
  diagnoseSupertraitMemberNames(context);
  validateDeclaredTypes(context);
}

/**
 * Method bounds are resolved while trait bodies are still being defined.
 * Add category implications only after every ordinary supertrait edge exists,
 * so declaration order cannot change dynamic safety or generic checking.
 */
function addImpliedMethodValueCategories(traitTypes: Map<string, HirTrait>): void {
  for (const trait of traitTypes.values()) {
    const methods = trait.methods.map((method) => {
      const referenceParameters = new Set(method.referenceParameters ?? []);
      const valueParameters = new Set(method.valueParameters ?? []);
      for (const bound of method.genericBounds ?? []) {
        const boundTrait = traitTypes.get(bound.traitName);
        if (!boundTrait) continue;
        if (traitImpliesValueCategory(boundTrait, traitTypes, "AnyRef"))
          referenceParameters.add(bound.parameter);
        if (traitImpliesValueCategory(boundTrait, traitTypes, "AnyVal"))
          valueParameters.add(bound.parameter);
      }
      return {
        ...method,
        referenceParameters: [...referenceParameters],
        valueParameters: [...valueParameters],
      };
    });
    traitTypes.set(trait.name, { ...trait, methods });
  }
}

/**
 * Program type bodies are built before every trait body is available. Check
 * their nested rows and trait values only after that phase, so validation is
 * independent of declaration order.
 */
function validateDeclaredTypes(context: ProgramCheckContext): void {
  const { program, typeDeclarations, dataTypes, enumTypes, traitTypes, diagnostics } = context;
  const known = (type: string): boolean =>
    isKnownType(resolveTraitType(type, traitTypes), dataTypes, enumTypes, traitTypes);
  const validateType = (type: string, span: HirData["span"]): void => {
    const resolved = resolveTraitType(type, traitTypes);
    const requirementDiagnostics = requirementKeyDiagnosticsInType(
      resolved,
      traitTypes,
      span,
      known,
    );
    diagnostics.push(...requirementDiagnostics);
    if (requirementDiagnostics.length > 0) return;
    const dynamicProblem = dynamicTraitProblemInType(resolved, traitTypes);
    if (dynamicProblem) diagnostics.push({ ...dynamicProblem, span });
  };
  const validateBounds = (
    bounds: readonly GenericBound[] | undefined,
    genericParameters: ReadonlySet<string>,
    rowParameters: ReadonlySet<string> = new Set(),
  ): void => {
    for (const bound of bounds ?? []) {
      for (const sourceTrait of bound.traits) {
        const key = mutableInner(sourceTrait) ?? sourceTrait;
        for (const argument of nominalGenericParts(key)?.arguments ?? [])
          validateType(resolveGenericType(argument, genericParameters, rowParameters), bound.span);
      }
      for (const binding of bound.bindings ?? [])
        validateType(
          resolveGenericType(binding.type.name, genericParameters, rowParameters),
          binding.span,
        );
    }
  };
  const callableKinds = (
    declaration: FunctionDecl | MethodDecl,
    outerGenerics: readonly string[] = [],
    outerRows: readonly string[] = [],
  ): { readonly types: Set<string>; readonly rows: Set<string> } => {
    const declared = new Set([...outerGenerics, ...declaration.genericParameters]);
    const rows = new Set([...outerRows, ...(declaration.rowParameters ?? [])]);
    for (const requirement of declaration.requirements)
      if (declared.has(requirement)) rows.add(requirement);
    declaration.parameters.forEach((parameter) =>
      collectRowParameterReferences(parameter.type.name, declared, rows),
    );
    collectRowParameterReferences(declaration.result.name, declared, rows);
    return {
      types: new Set([...declared].filter((parameter) => !rows.has(parameter))),
      rows,
    };
  };
  const validateRequirements = (
    declaration: FunctionDecl | MethodDecl,
    genericParameters: ReadonlySet<string>,
    rowParameters: ReadonlySet<string>,
  ): void => {
    const requirements = normalizedRequirements(
      declaration.requirements
        .flatMap((requirement) => resolveGenericRequirement(requirement, rowParameters))
        .map((requirement) =>
          rowParameterName(requirement)
            ? requirement
            : resolveRequirementKeyTypes(
                resolveGenericType(requirement, genericParameters, rowParameters),
                (type) => resolveTraitType(type, traitTypes),
              ),
        ),
    );
    if (requirementKeyDiagnostics(requirements, traitTypes, declaration.span, known).length > 0)
      return;
    const dynamicProblem = dynamicTraitProblemInType(rowArgumentType(requirements), traitTypes);
    if (dynamicProblem) diagnostics.push({ ...dynamicProblem, span: declaration.span });
  };
  // Written applications in fields meet their declarations' bounds
  // (trait.bound.no-implied), with supertraits implied
  // (trait.bound.supertraits). Fields check here, not while building the
  // declarations, because supertrait edges only exist after that phase.
  const validateWrittenFields = (
    bounds: readonly GenericBound[] | undefined,
    fields: readonly HirDataField[],
  ): void => {
    const hashable = new Set(
      (bounds ?? []).flatMap((bound) =>
        ["Eq", "Hash"].every((trait) =>
          (bounds ?? []).some(
            (other) => other.parameter === bound.parameter && other.traits.includes(trait),
          ),
        )
          ? [bound.parameter]
          : [],
      ),
    );
    for (const field of fields)
      pushWrittenBoundProblem(diagnostics, field.span, field.type, {
        dataTypes,
        enumTypes,
        traitTypes,
        hashableParameters: hashable,
        parameterImplied: (parameter, traitName) =>
          enclosingBoundImplies(bounds ?? [], traitTypes, parameter, traitName),
      });
  };
  for (const data of program.data)
    validateWrittenFields(data.genericBounds, dataTypes.get(data.name)?.fields ?? []);
  for (const declaration of program.enums) {
    const enumType = enumTypes.get(declaration.name);
    validateWrittenFields(declaration.genericBounds, enumType?.sharedFields ?? []);
    for (const variant of enumType?.variants ?? [])
      validateWrittenFields(declaration.genericBounds, variant.fields);
  }
  for (const data of dataTypes.values()) {
    data.fields.forEach((field) => validateType(field.type, field.span));
    for (const type of data.genericDefaults?.values() ?? []) validateType(type, data.span);
  }
  for (const enumType of enumTypes.values())
    enumType.fields.forEach((field) => validateType(field.type, field.span));
  for (const declaration of program.data) {
    const rows = new Set(dataTypes.get(declaration.name)?.rowParameters ?? []);
    validateBounds(
      declaration.genericBounds,
      new Set(declaration.genericParameters.filter((parameter) => !rows.has(parameter))),
      rows,
    );
  }
  for (const declaration of program.enums)
    validateBounds(declaration.genericBounds, new Set(declaration.genericParameters));
  for (const declaration of typeDeclarations) {
    const rows = new Set(declaration.rowParameters ?? []);
    const types = new Set(
      declaration.genericParameters.filter((parameter) => !rows.has(parameter)),
    );
    validateBounds(declaration.genericBounds, types, rows);
    for (const defaultType of Object.values(declaration.genericDefaults ?? {}))
      validateType(resolveGenericType(defaultType.name, types, rows), defaultType.span);
  }
  validateAliasTargets(context);
  for (const trait of traitTypes.values()) {
    for (const supertrait of trait.supertraits) {
      supertrait.traitArguments.forEach((type) => validateType(type, trait.span));
      supertrait.associatedBindings?.forEach((binding) => validateType(binding.type, trait.span));
    }
    for (const method of trait.methods) {
      const requirementDiagnostics = requirementKeyDiagnostics(
        method.requirements,
        traitTypes,
        method.span,
        known,
      );
      diagnostics.push(...requirementDiagnostics);
      if (requirementDiagnostics.length === 0) {
        const dynamicProblem = dynamicTraitProblemInType(
          rowArgumentType(method.requirements),
          traitTypes,
        );
        if (dynamicProblem) diagnostics.push({ ...dynamicProblem, span: method.span });
      }
      method.parameters.forEach((type) => validateType(type, method.span));
      validateType(method.result, method.span);
      for (const type of method.genericDefaults?.values() ?? []) validateType(type, method.span);
    }
  }
  for (const declaration of program.traits) {
    validateBounds(declaration.genericBounds, new Set(declaration.genericParameters));
    const trait = traitTypes.get(declaration.name);
    declaration.methods.forEach((method, index) => {
      const rows = new Set(trait?.methods[index]?.rowParameters ?? []);
      validateBounds(
        method.genericBounds,
        new Set(
          [...declaration.genericParameters, ...method.genericParameters].filter(
            (parameter) => !rows.has(parameter),
          ),
        ),
        rows,
      );
    });
  }
  for (const declaration of program.functions) {
    const kinds = callableKinds(declaration);
    validateBounds(declaration.genericBounds, kinds.types, kinds.rows);
    validateRequirements(declaration, kinds.types, kinds.rows);
  }
  for (const implementation of program.implementations) {
    const implementationRows = new Set(implementation.rowParameters ?? []);
    const implementationTypes = new Set(
      implementation.genericParameters.filter((parameter) => !implementationRows.has(parameter)),
    );
    validateBounds(implementation.genericBounds, implementationTypes, implementationRows);
    for (const method of implementation.methods) {
      const kinds = callableKinds(
        method,
        implementation.genericParameters,
        implementation.rowParameters ?? [],
      );
      validateBounds(method.genericBounds, kinds.types, kinds.rows);
      validateRequirements(method, kinds.types, kinds.rows);
    }
  }
}

function validateAliasTargets(context: ProgramCheckContext): void {
  const { typeDeclarations, dataTypes, enumTypes, traitTypes, diagnostics } = context;

  const aliasNames = new Set(typeDeclarations.filter((d) => d.alias ?? d.row).map((d) => d.name));
  const primitiveKnown = new Set([
    "i8",
    "i16",
    "i32",
    "i64",
    "u8",
    "u16",
    "u32",
    "u64",
    "f32",
    "f64",
    "bool",
    "char",
    "string",
    "void",
    "never",
    "usize",
    "List",
    "Map",
    "Suspend",
    "$Cursor",
  ]);
  const isTypeBaseKnown = (name: string, generics: ReadonlySet<string>): boolean =>
    generics.has(name) ||
    aliasNames.has(name) ||
    primitiveKnown.has(name) ||
    name.startsWith("generic:") ||
    name.startsWith("row:") ||
    name.startsWith("trait:") ||
    name.startsWith("provider:") ||
    dataTypes.has(name) ||
    enumTypes.has(name) ||
    traitTypes.has(name);
  function visitAliasType(
    type: string,
    generics: ReadonlySet<string>,
    onUnknownType: () => void,
    onUnknownTrait: () => void,
    found: { current: boolean },
  ): void {
    if (found.current) return;
    if (generics.has(type) || aliasNames.has(type)) return;
    if (type.includes("::")) {
      const prefix = type.split("::")[0]!;
      const base = prefix.includes("[") ? (nominalGenericParts(prefix)?.name ?? prefix) : prefix;
      if (!isTypeBaseKnown(base, generics)) {
        onUnknownType();
        found.current = true;
      }
      return;
    }
    const binding = bindingParts(type);
    if (binding) {
      visitAliasType(binding.type, generics, onUnknownType, onUnknownTrait, found);
      return;
    }
    const inner = mutableInner(type) ?? optionalInner(type) ?? restInner(type) ?? inputsInner(type);
    if (inner !== undefined) {
      visitAliasType(inner, generics, onUnknownType, onUnknownTrait, found);
      return;
    }
    const tuple = tupleParts(type);
    if (tuple !== undefined) {
      for (const element of tuple) {
        visitAliasType(element, generics, onUnknownType, onUnknownTrait, found);
        if (found.current) return;
      }
      return;
    }
    const result = resultParts(type);
    if (result) {
      visitAliasType(result.ok, generics, onUnknownType, onUnknownTrait, found);
      if (found.current) return;
      visitAliasType(result.error, generics, onUnknownType, onUnknownTrait, found);
      return;
    }
    const callable = functionParts(type);
    if (callable) {
      for (const parameter of callable.parameters) {
        visitAliasType(parameter, generics, onUnknownType, onUnknownTrait, found);
        if (found.current) return;
      }
      visitAliasType(callable.result, generics, onUnknownType, onUnknownTrait, found);
      if (found.current) return;
      for (const requirement of callable.requirements)
        visitAliasRequirement(requirement, generics, onUnknownType, onUnknownTrait, found);
      return;
    }
    const rowArgs = rowArgumentKeys(type);
    if (rowArgs) {
      for (const requirement of rowArgs)
        visitAliasRequirement(requirement, generics, onUnknownType, onUnknownTrait, found);
      return;
    }
    const context = contextKeys(type);
    if (context) {
      for (const requirement of context)
        visitAliasRequirement(requirement, generics, onUnknownType, onUnknownTrait, found);
      return;
    }
    const nominal = nominalGenericParts(type);
    if (nominal) {
      if (!isTypeBaseKnown(nominal.name, generics)) {
        onUnknownType();
        found.current = true;
        return;
      }
      const { positional, bindings } = splitTypeBindings(nominal.arguments);
      for (const arg of [...positional, ...bindings.map((b) => b.type)]) {
        visitAliasType(arg, generics, onUnknownType, onUnknownTrait, found);
        if (found.current) return;
      }
      return;
    }
    if (!isTypeBaseKnown(type, generics)) {
      onUnknownType();
      found.current = true;
    }
  }
  function visitAliasRequirement(
    key: string,
    generics: ReadonlySet<string>,
    onUnknownType: () => void,
    onUnknownTrait: () => void,
    found: { current: boolean },
  ): void {
    if (found.current) return;
    const nominal = nominalGenericParts(key);
    const base = nominal?.name ?? key;
    if (generics.has(base) || aliasNames.has(base)) {
      if (!nominal) return;
      const { positional, bindings } = splitTypeBindings(nominal.arguments);
      for (const arg of [...positional, ...bindings.map((b) => b.type)]) {
        visitAliasType(arg, generics, onUnknownType, onUnknownTrait, found);
        if (found.current) return;
      }
      return;
    }
    if (!traitTypes.has(base)) {
      onUnknownTrait();
      found.current = true;
      return;
    }
    if (nominal) {
      const { positional, bindings } = splitTypeBindings(nominal.arguments);
      for (const arg of [...positional, ...bindings.map((b) => b.type)]) {
        visitAliasType(arg, generics, onUnknownType, onUnknownTrait, found);
        if (found.current) return;
      }
    }
  }
  for (const declaration of typeDeclarations) {
    const generics = new Set(declaration.genericParameters);
    if (declaration.alias) {
      const found = { current: false };
      let reported = false;
      const onUnknownType = (): void => {
        if (reported) return;
        reported = true;
        diagnostics.push({
          code: "unknown-type",
          message: `unknown type '${displayType(declaration.alias!.name)}' in alias '${declaration.name}'`,
          span: declaration.alias!.span,
        });
      };
      const onUnknownTrait = (): void => {
        if (reported) return;
        reported = true;
        diagnostics.push({
          code: "unknown-trait",
          message: `unknown trait in alias '${declaration.name}'`,
          span: declaration.alias!.span,
        });
      };
      visitAliasType(declaration.alias.name, generics, onUnknownType, onUnknownTrait, found);
    }
    if (declaration.row) {
      for (const key of declaration.row) {
        const nominal = nominalGenericParts(key);
        const base = nominal?.name ?? key;
        if (generics.has(base) || aliasNames.has(base)) continue;
        if (!traitTypes.has(base)) {
          diagnostics.push({
            code: "unknown-trait",
            message: `unknown trait '${displayType(base)}' in row alias '${declaration.name}'`,
            span: declaration.span,
          });
          break;
        }
      }
    }
  }
}

// 09 Trait Declarations: a child trait must not declare a member name of any
// transitive supertrait. Reported on the child's member.
function diagnoseSupertraitMemberNames(context: ProgramCheckContext): void {
  const traitsByIndex = new Map<number, HirTrait>();
  for (const trait of context.traitTypes.values()) traitsByIndex.set(trait.index, trait);
  const inheritedNames = (trait: HirTrait): Map<string, string> => {
    const names = new Map<string, string>();
    const seen = new Set<number>([trait.index]);
    const pending = trait.supertraits.map((supertrait) => supertrait.traitIndex);
    while (pending.length > 0) {
      const index = pending.pop()!;
      if (seen.has(index)) continue;
      seen.add(index);
      const supertrait = traitsByIndex.get(index);
      if (!supertrait) continue;
      for (const member of [...supertrait.associatedTypes, ...supertrait.methods])
        if (!names.has(member.name)) names.set(member.name, supertrait.name);
      pending.push(...supertrait.supertraits.map((parent) => parent.traitIndex));
    }
    return names;
  };
  const sealed = usesStandardInspect(context.imports);
  for (const declaration of context.program.traits) {
    const trait = context.traitTypes.get(declaration.name);
    if (!trait || trait.supertraits.length === 0) continue;
    const inherited = inheritedNames(trait);
    const inspectable = sealed && extendsInspectable(context.traitTypes, trait.name);
    for (const member of [...declaration.associatedTypes, ...declaration.methods]) {
      // 09 Sealed Traits: redeclaring a member of the sealed Inspectable.
      if (inspectable && INSPECTABLE_MEMBERS.has(member.name)) {
        context.diagnostics.push({
          code: "sealed-trait-implementation",
          message: `trait '${displayType(trait.name)}' redeclares '${member.name}', a member of the sealed Inspectable`,
          span: member.span,
        });
        continue;
      }
      const owner = inherited.get(member.name);
      if (owner)
        context.diagnostics.push({
          code: "duplicate-trait-member",
          message: `trait '${displayType(trait.name)}' declares '${member.name}', which its supertrait '${owner}' already declares`,
          span: member.span,
        });
    }
  }
}

function diagnoseSupertraitCycles(context: ProgramCheckContext): void {
  // Each cycle is reported once, on its member that appears first. A trait is
  // that member when it reaches itself through traits declared after it.
  const order = new Map<number, number>();
  const traitsByIndex = new Map<number, HirTrait>();
  context.program.traits.forEach((declaration, position) => {
    const trait = context.traitTypes.get(declaration.name);
    if (!trait) return;
    order.set(trait.index, position);
    traitsByIndex.set(trait.index, trait);
  });
  const reaches = (current: number, target: number, floor: number, seen: Set<number>): boolean => {
    if (current === target) return true;
    if ((order.get(current) ?? -1) <= floor || seen.has(current)) return false;
    seen.add(current);
    return (
      traitsByIndex
        .get(current)
        ?.supertraits.some((supertrait) => reaches(supertrait.traitIndex, target, floor, seen)) ??
      false
    );
  };
  context.program.traits.forEach((declaration, position) => {
    const trait = context.traitTypes.get(declaration.name);
    if (
      trait?.supertraits.some((supertrait) =>
        reaches(supertrait.traitIndex, trait.index, position, new Set()),
      )
    )
      context.diagnostics.push({
        code: "supertrait-cycle",
        message: `trait '${displayType(trait.name)}' participates in a supertrait cycle`,
        span: declaration.span,
      });
  });
}

/** The traits among `trait` and its declared supertraits that declare the associated type `name`. */
function declaringTraits(traits: readonly TraitDecl[], trait: TraitDecl, name: string): string[] {
  const seen = new Set<string>();
  const declaring: string[] = [];
  const visit = (current: TraitDecl): void => {
    if (seen.has(current.name)) return;
    seen.add(current.name);
    if (current.associatedTypes.some((associated) => associated.name === name))
      declaring.push(current.name);
    for (const supertrait of current.supertraits) {
      const head = nominalGenericParts(supertrait.name)?.name ?? supertrait.name;
      const parent = traits.find((candidate) => candidate.name === head);
      if (parent) visit(parent);
    }
  };
  visit(trait);
  return declaring;
}
