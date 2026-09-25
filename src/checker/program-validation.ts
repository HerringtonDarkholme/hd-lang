import { PRELUDE_NAMES } from "./context.ts";
import {
  deferredDriverCalls,
  driverStartingFunctionNames,
  findDriverCall,
} from "./program-effects.ts";
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
  const driverFunctions = driverStartingFunctionNames(program, imports);
  for (const call of deferredDriverCalls(program, driverFunctions, imports)) {
    diagnostics.push({
      code: "suspension-forbidden-context",
      message: "a defer suite cannot transitively start a suspension driver",
      span: call.span,
    });
  }
  const topLevelDriverCall = findDriverCall(program.statements, driverFunctions, imports);
  if (topLevelDriverCall) {
    diagnostics.push({
      code: "suspension-forbidden-context",
      message: "module initialization cannot transitively start a suspension driver",
      span: topLevelDriverCall.span,
    });
  }
  const pureFunctions = pureFunctionNames(program);
  for (const declaration of program.functions) {
    let sawDefault = false;
    const earlierParameters = new Set<string>();
    for (const parameter of declaration.parameters) {
      if (parameter.default) {
        sawDefault = true;
        const driverCall = findDriverCall(parameter.default, driverFunctions, imports);
        if (driverCall) {
          diagnostics.push({
            code: "suspension-forbidden-context",
            message: `default for '${declaration.name}.${parameter.name}' cannot transitively start a suspension driver`,
            span: driverCall.span,
          });
        } else if (
          !expressionIsPure(parameter.default, earlierParameters, pureFunctions, program)
        ) {
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
      if (!field.default) continue;
      const driverCall = findDriverCall(field.default, driverFunctions, imports);
      if (driverCall) {
        diagnostics.push({
          code: "suspension-forbidden-context",
          message: `default for '${declaration.name}.${field.name}' cannot transitively start a suspension driver`,
          span: driverCall.span,
        });
      } else if (!expressionIsPure(field.default, new Set(), pureFunctions, program)) {
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
        const driverCall = findDriverCall(field.default, driverFunctions, imports);
        if (driverCall) {
          diagnostics.push({
            code: "suspension-forbidden-context",
            message: `default for '${declaration.name}.${field.name}' cannot transitively start a suspension driver`,
            span: driverCall.span,
          });
        } else if (!expressionIsPure(field.default, earlierFields, pureFunctions, program)) {
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
