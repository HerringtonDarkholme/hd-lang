import { PRELUDE_NAMES } from "./context.ts";
import { expressionIsPure, pureFunctionNames } from "./shared.ts";

import type { ProgramCheckContext } from "./program-context.ts";

export function validateProgram(context: ProgramCheckContext): void {
  const { program, diagnostics, imports } = context;
  for (const declaration of program.uses) {
    for (const imported of declaration.names) {
      const localName = imported.alias ?? imported.name;
      if (PRELUDE_NAMES.has(localName)) {
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `import '${localName}' shadows a prelude name`,
          span: declaration.span,
        });
        continue;
      }
      if (imports.has(localName)) {
        diagnostics.push({
          code: "duplicate-module-name",
          message: `imported name '${localName}' is declared more than once`,
          span: declaration.span,
        });
        continue;
      }
      imports.set(localName, `${declaration.module}.${imported.name}`);
    }
  }
  const pureFunctions = pureFunctionNames(program);
  for (const declaration of program.functions) {
    let sawDefault = false;
    const earlierParameters = new Set<string>();
    for (const parameter of declaration.parameters) {
      if (parameter.default) {
        sawDefault = true;
        if (!expressionIsPure(parameter.default, earlierParameters, pureFunctions, program)) {
          diagnostics.push({
            code: "impure-parameter-default",
            message: `default for '${declaration.name}.${parameter.name}' is not compile-time pure`,
            span: parameter.default.span,
          });
        }
      } else if (sawDefault && !parameter.variadic) {
        diagnostics.push({
          code: "parameter-default-order",
          message: `parameter '${parameter.name}' follows a parameter with a default`,
          span: parameter.span,
        });
      }
      earlierParameters.add(parameter.name);
    }
  }
  for (const declaration of program.data) {
    for (const field of declaration.fields) {
      if (field.default && !expressionIsPure(field.default, new Set(), pureFunctions, program)) {
        diagnostics.push({
          code: "impure-data-default",
          message: `default for '${declaration.name}.${field.name}' is not compile-time pure`,
          span: field.default.span,
        });
      }
    }
  }
  for (const declaration of program.enums) {
    let sawDefault = false;
    const earlierFields = new Set<string>();
    for (const field of declaration.sharedFields) {
      if (field.default) {
        sawDefault = true;
        if (!expressionIsPure(field.default, earlierFields, pureFunctions, program)) {
          diagnostics.push({
            code: "impure-enum-default",
            message: `default for '${declaration.name}.${field.name}' is not compile-time pure`,
            span: field.default.span,
          });
        }
      } else if (sawDefault) {
        diagnostics.push({
          code: "enum-default-order",
          message: `shared enum field '${field.name}' follows a field with a default`,
          span: field.span,
        });
      }
      if (!/^\d/.test(field.name)) earlierFields.add(field.name);
    }
  }
}
