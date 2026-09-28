import type { Expression } from "../ast.ts";
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
  // The compiled module is the entry module: its initialization may start a
  // driver, since only non-entry module initialization is a forbidden
  // context (req.drive.block-on.forbidden-contexts).
  validateResultTypes(context);
  for (const declaration of program.functions) {
    let sawDefault = false;
    for (const [index, parameter] of declaration.parameters.entries()) {
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
      } else if (
        sawDefault &&
        !parameter.variadic &&
        // A final function-typed parameter may follow defaults
        // (07-functions.md#r-fn.default.order-final-function).
        !(index === declaration.parameters.length - 1 && isFunctionTypeName(parameter.type.name))
      ) {
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
  // Fact and metadata expressions are evaluated at compile time, outside any
  // driver (spec/14-annotations.md#r-annot.fact.no-block-on).
  const facts: Expression[] = [
    ...[...program.data, ...program.enums].flatMap((declaration) => [
      ...(declaration.decorators?.facts ?? []),
    ]),
    ...program.data.flatMap((declaration) =>
      declaration.fields.flatMap((field) => field.metadata ?? []),
    ),
    ...program.enums.flatMap((declaration) =>
      declaration.variants.flatMap((variant) => [
        ...(variant.metadata ?? []),
        ...variant.fields.flatMap((field) => field.metadata ?? []),
      ]),
    ),
    ...program.functions.flatMap((declaration) => [
      ...(declaration.decorators?.facts ?? []),
      ...declaration.parameters.flatMap((parameter) => parameter.metadata ?? []),
    ]),
    ...[...program.traits, ...program.implementations].flatMap((declaration) => [
      ...(declaration.decorators?.facts ?? []),
      ...declaration.methods.flatMap((method) => [
        ...(method.decorators?.facts ?? []),
        ...method.parameters.flatMap((parameter) => parameter.metadata ?? []),
      ]),
    ]),
    ...(program.types ?? []).flatMap((declaration) => declaration.decorators?.facts ?? []),
  ];
  for (const fact of facts) {
    const driverCall = findDriverCall(fact, driverFunctions, imports);
    if (driverCall)
      diagnostics.push({
        code: "suspension-forbidden-context",
        message: "a fact or metadata expression cannot transitively start a suspension driver",
        span: driverCall.span,
      });
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

function isFunctionTypeName(name: string): boolean {
  return /^(?:mut:)?fn!?\(/.test(name);
}
