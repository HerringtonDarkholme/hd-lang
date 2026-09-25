import type { HirData } from "../hir.ts";
import { nominalGenericType } from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import { typeName } from "./shared.ts";

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
      methods: [],
      span: declaration.span,
    });
  });
  traitTypes.set("Display", {
    name: "Display",
    index: program.traits.length,
    genericParameters: [],
    methods: [
      {
        name: "to_string",
        index: 0,
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
  let nextEnumIndex = program.enums.length;
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
    if (declaration.genericParameters.length > 0 && declaration.methods.length > 0) {
      diagnostics.push({
        code: "unsupported-generic-trait-method",
        message: `generic trait '${declaration.name}' is currently supported only as a provider marker`,
        span: declaration.span,
      });
      continue;
    }
    const names = new Set<string>();
    const methods = declaration.methods.map((method, index) => {
      if (names.has(method.name))
        diagnostics.push({
          code: "duplicate-trait-member",
          message: `trait member '${method.name}' is declared more than once`,
          span: method.span,
        });
      names.add(method.name);
      if (method.parameters[0]?.name !== "self")
        diagnostics.push({
          code: "unsupported-associated-function",
          message: `trait method '${method.name}' requires a self receiver in the initial dictionary slice`,
          span: method.span,
        });
      const sourceParameters = method.parameters.slice(1);
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
        const type = typeName(parameter.type, dataTypes, enumTypes, traitTypes, diagnostics);
        return type && parameter.variadic ? nominalGenericType("list", [type]) : type;
      });
      if (
        method.result.name === "Self" ||
        method.parameters.slice(1).some((parameter) => parameter.type.name === "Self")
      ) {
        diagnostics.push({
          code: "unsafe-dynamic-trait",
          message: "Self may appear only as the receiver of a dynamic trait method",
          span: method.span,
        });
      }
      const result =
        typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
      return {
        name: method.name,
        index,
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
    traitTypes.set(declaration.name, { ...trait, methods });
  }
}
