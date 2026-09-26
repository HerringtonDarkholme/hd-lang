import type { SourceSpan } from "../diagnostics.ts";
import { PRELUDE_NAMES } from "./context.ts";
import {
  deferredDriverCalls,
  driverStartingFunctionNames,
  findDriverCall,
} from "./program-effects.ts";

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
  validateResultTypes(context);
  for (const declaration of program.functions) {
    let sawDefault = false;
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
        }
      } else if (sawDefault && !parameter.variadic) {
        diagnostics.push({
          code: "default-order",
          message: `parameter '${parameter.name}' follows a parameter with a default`,
          span: parameter.span,
        });
      }
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
      }
    }
  }
  for (const declaration of program.enums) {
    let sawDefault = false;
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
        }
      } else if (sawDefault) {
        diagnostics.push({
          code: "default-order",
          message: `shared enum field '${field.name}' follows a field with a default`,
          span: field.span,
        });
      }
    }
  }
}

// A public function, a trait method, and a method of a trait implementation
// must declare a result type (07-functions.md#declarations). Other functions
// and inherent methods may omit it; the checker infers it.
function validateResultTypes(context: ProgramCheckContext): void {
  const { program, diagnostics } = context;
  const report = (kind: string, name: string, span: SourceSpan): void => {
    diagnostics.push({
      code: "missing-result-type",
      message: `${kind} '${name}' must declare its result type`,
      span,
    });
  };
  for (const declaration of program.functions) {
    if (declaration.public && declaration.resultOmitted)
      report("public function", declaration.name, declaration.span);
  }
  for (const trait of program.traits) {
    for (const method of trait.methods) {
      if (method.resultOmitted) report("trait method", method.name, method.span);
    }
  }
  for (const implementation of program.implementations) {
    if (implementation.traitName === undefined) continue;
    for (const method of implementation.methods) {
      if (method.resultOmitted) report("trait implementation method", method.name, method.span);
    }
  }
}
