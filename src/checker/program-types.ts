import type { HirData } from "../hir.ts";
import { nominalGenericParts, nominalGenericType } from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import { resolveGenericType, typeName } from "./shared.ts";

import type { ProgramCheckContext } from "./program-context.ts";

export function declareProgramTypes(context: ProgramCheckContext): void {
  const { program, diagnostics, imports, dataTypes, enumTypes, traitTypes } = context;
  program.data.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
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
    dataTypes.set(declaration.name, {
      name: declaration.name,
      index,
      genericParameters: declaration.genericParameters,
      fields: [],
      span: declaration.span,
    });
  });
  program.enums.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
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
      index,
      genericParameters: declaration.genericParameters,
      sharedFields: [],
      variants: [],
      fields: [],
      span: declaration.span,
    });
  });
  program.traits.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name)) {
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
      index,
      genericParameters: declaration.genericParameters,
      supertraits: [],
      associatedTypes: [],
      methods: [],
      span: declaration.span,
    });
  });
  traitTypes.set("Display", {
    name: "Display",
    index: program.traits.length,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [
      {
        name: "to_string",
        index: 0,
        associated: false,
        genericParameters: [],
        suspending: false,
        receiverMutable: false,
        parameters: [],
        parameterNames: [],
        variadic: false,
        result: "string",
        requirements: [],
        span: program.span,
      },
    ],
    span: program.span,
  });
  traitTypes.set("PartialEq", {
    name: "PartialEq",
    index: program.traits.length + 1,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [
      {
        name: "eq",
        index: 0,
        associated: false,
        genericParameters: [],
        suspending: false,
        receiverMutable: false,
        parameters: ["generic:Self"],
        parameterNames: ["other"],
        variadic: false,
        result: "bool",
        requirements: [],
        span: program.span,
      },
    ],
    span: program.span,
  });
  traitTypes.set("PartialOrd", {
    name: "PartialOrd",
    index: program.traits.length + 2,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [
      {
        name: "partial_cmp",
        index: 0,
        associated: false,
        genericParameters: [],
        suspending: false,
        receiverMutable: false,
        parameters: ["generic:Self"],
        parameterNames: ["other"],
        variadic: false,
        result: "Ordering?",
        requirements: [],
        span: program.span,
      },
    ],
    span: program.span,
  });
  traitTypes.set("Waker", {
    name: "Waker",
    index: program.traits.length + 3,
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
  traitTypes.set("Iterator", {
    name: "Iterator",
    index: program.traits.length + 4,
    genericParameters: ["T"],
    supertraits: [],
    associatedTypes: [],
    methods: [
      {
        name: "next",
        index: 0,
        associated: false,
        genericParameters: [],
        suspending: false,
        receiverMutable: true,
        parameters: [],
        parameterNames: [],
        variadic: false,
        result: "generic:T?",
        requirements: [],
        span: program.span,
      },
    ],
    span: program.span,
  });
  let nextEnumIndex = program.enums.length;
  enumTypes.set("Ordering", {
    name: "Ordering",
    index: nextEnumIndex++,
    genericParameters: [],
    sharedFields: [],
    variants: ["Less", "Equal", "Greater"].map((name, tag) => ({
      name,
      tag,
      fields: [],
      span: program.span,
    })),
    fields: [],
    span: program.span,
  });
  for (const [localName, importedName] of imports) {
    if (importedName !== "std.resource.ResourceError") continue;
    const declaration = program.uses.find((useDeclaration) =>
      useDeclaration.names.some((name) => (name.alias ?? name.name) === localName),
    );
    const span = declaration?.span ?? program.span;
    if (dataTypes.has(localName) || enumTypes.has(localName) || traitTypes.has(localName)) {
      diagnostics.push({
        code: "duplicate-module-name",
        message: `imported type '${localName}' conflicts with a local type`,
        span,
      });
      continue;
    }
    const operationField = { name: "error", type: "generic:E", index: 0, span };
    enumTypes.set(localName, {
      name: localName,
      index: nextEnumIndex++,
      genericParameters: ["E"],
      sharedFields: [],
      variants: [
        { name: "Operation", tag: 0, fields: [operationField], span },
        { name: "Disposed", tag: 1, fields: [], span },
      ],
      fields: [operationField],
      span,
    });
  }
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
          message: `generic parameter '${parameter}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const names = new Set<string>();
    const fields = declaration.fields.map((field, index) => {
      if (names.has(field.name))
        diagnostics.push({
          code: field.embedded ? "duplicate-embedded-field" : "duplicate-data-field",
          message: `field '${field.name}' is declared more than once`,
          span: field.span,
        });
      names.add(field.name);
      const type =
        typeName(
          field.type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          new Set(declaration.genericParameters),
        ) ?? "void";
      if (type === "void")
        diagnostics.push({
          code: "void-data-field",
          message: "a data field cannot have type void",
          span: field.span,
        });
      return {
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
    dataTypes.set(declaration.name, { ...data, fields });
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
          message: `generic parameter '${parameter}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const variantNames = new Set<string>();
    const allFields: HirData["fields"][number][] = [];
    const sharedNames = new Set<string>();
    const sharedFields = declaration.sharedFields.map((field) => {
      if (sharedNames.has(field.name))
        diagnostics.push({
          code: "duplicate-data-field",
          message: `shared enum field '${field.name}' is declared more than once`,
          span: field.span,
        });
      sharedNames.add(field.name);
      const type =
        typeName(
          field.type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          new Set(declaration.genericParameters),
        ) ?? "void";
      if (type === "void")
        diagnostics.push({
          code: "void-data-field",
          message: "a shared enum field cannot have type void",
          span: field.span,
        });
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
        if (sharedNames.has(field.name))
          diagnostics.push({
            code: "duplicate-data-field",
            message: `payload field '${field.name}' duplicates a shared enum field`,
            span: field.span,
          });
        if (fieldNames.has(field.name))
          diagnostics.push({
            code: "duplicate-data-field",
            message: `payload field '${field.name}' is declared more than once`,
            span: field.span,
          });
        fieldNames.add(field.name);
        const type =
          typeName(
            field.type,
            dataTypes,
            enumTypes,
            traitTypes,
            diagnostics,
            new Set(declaration.genericParameters),
          ) ?? "void";
        if (type === "void")
          diagnostics.push({
            code: "void-data-field",
            message: "an enum payload cannot have type void",
            span: field.span,
          });
        const checked = { name: field.name, type, index: allFields.length, span: field.span };
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
    enumTypes.set(declaration.name, { ...enumType, sharedFields, variants, fields: allFields });
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
          message: `generic parameter '${parameter}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const genericParameters = new Set(declaration.genericParameters);
    const supertraitNames = new Set<string>();
    const supertraits = declaration.supertraits.flatMap((reference) => {
      const resolved = resolveGenericType(reference.name, genericParameters);
      const application = nominalGenericParts(resolved);
      const name = application?.name ?? resolved;
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
          message: `trait '${supertrait.name}' expects ${supertrait.genericParameters.length} type arguments`,
          span: reference.span,
        });
        return [];
      }
      return [
        {
          traitIndex: supertrait.index,
          traitName: supertrait.name,
          traitArguments,
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
      const memberGenerics = new Set([
        ...declaration.genericParameters,
        ...method.genericParameters,
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
        );
        return type && parameter.variadic ? nominalGenericType("list", [type]) : type;
      });
      const result =
        typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics, memberGenerics) ??
        "void";
      return {
        name: method.name,
        index,
        associated,
        genericParameters: method.genericParameters,
        suspending: method.suspending,
        receiverMutable: method.parameters[0]?.type.name === "mut:Self",
        parameters: parameters.map((parameter) => parameter ?? "void"),
        parameterNames: sourceParameters.map((parameter) => parameter.name),
        variadic: sourceParameters.at(-1)?.variadic === true,
        result,
        requirements: method.requirements,
        span: method.span,
      };
    });
    traitTypes.set(declaration.name, { ...trait, supertraits, associatedTypes, methods });
  }
  diagnoseSupertraitCycles(context);
}

function diagnoseSupertraitCycles(context: ProgramCheckContext): void {
  const traitsByIndex = new Map(
    [...context.traitTypes.values()].map((trait) => [trait.index, trait]),
  );
  const reaches = (current: number, target: number, seen: Set<number>): boolean => {
    if (current === target) return true;
    if (seen.has(current)) return false;
    seen.add(current);
    return (
      traitsByIndex
        .get(current)
        ?.supertraits.some((supertrait) => reaches(supertrait.traitIndex, target, seen)) ?? false
    );
  };
  for (const declaration of context.program.traits) {
    const trait = context.traitTypes.get(declaration.name);
    if (
      trait?.supertraits.some((supertrait) =>
        reaches(supertrait.traitIndex, trait.index, new Set([trait.index])),
      )
    )
      context.diagnostics.push({
        code: "supertrait-cycle",
        message: `trait '${trait.name}' participates in a supertrait cycle`,
        span: declaration.span,
      });
  }
}
