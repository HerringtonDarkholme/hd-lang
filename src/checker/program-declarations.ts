import type { Expression, FunctionDecl, Statement, TypeRef } from "../ast.ts";
import { nominalGenericType } from "../types.ts";

import type { ProgramCheckContext } from "./program-context.ts";

interface ExplicitEnumFieldValue {
  readonly fieldIndex: number;
  readonly value: Expression;
}

function createTestDeclarations(program: ProgramCheckContext["program"]): FunctionDecl[] {
  return program.tests.map((test, index) => ({
    kind: "function",
    name: `$test.${index}`,
    suspending: true,
    genericParameters: [],
    genericBounds: [],
    parameters: [],
    result: { name: "void", span: test.span },
    requirements: [],
    body: test.body,
    span: test.span,
  }));
}

// Parameters (or named shared enum fields) after `index` are not yet visible
// to the default at `index`.
function laterNamesContext(
  items: readonly { readonly name: string }[],
  index: number,
): NonNullable<FunctionDecl["defaultContext"]> {
  return {
    laterNames: items
      .slice(index + 1)
      .flatMap((later) => (/^\d/.test(later.name) ? [] : [later.name])),
  };
}

export function createProgramDeclarations(
  context: ProgramCheckContext,
): FunctionDecl[] | undefined {
  const { program, diagnostics, implementationPreparations, inherentDeclarations } = context;
  const defaultDeclarations: FunctionDecl[] = program.data.flatMap((declaration) =>
    declaration.fields.flatMap((field) =>
      field.default
        ? [
            {
              kind: "function" as const,
              name: `$default.${declaration.name}.${field.name}`,
              suspending: false,
              genericParameters: declaration.genericParameters,
              genericBounds: [],
              parameters: [],
              result: field.type,
              requirements: [],
              body: [
                {
                  kind: "expression" as const,
                  expression: field.default,
                  span: field.default.span,
                },
              ],
              span: field.default.span,
              defaultContext: { laterNames: [] },
            },
          ]
        : [],
    ),
  );
  const parameterDefaultDeclarations: FunctionDecl[] = program.functions.flatMap((declaration) =>
    declaration.parameters.flatMap((parameter, parameterIndex) =>
      parameter.default
        ? [
            {
              kind: "function" as const,
              name: `$parameter-default.${declaration.name}.${parameter.name}`,
              suspending: false,
              genericParameters: declaration.genericParameters,
              genericBounds: declaration.genericBounds,
              parameters: declaration.parameters.slice(0, parameterIndex).map((earlier) => ({
                name: earlier.name,
                type: earlier.type,
                span: earlier.span,
              })),
              result: parameter.type,
              requirements: [],
              body: [
                {
                  kind: "expression" as const,
                  expression: parameter.default,
                  span: parameter.default.span,
                },
              ],
              span: parameter.default.span,
              defaultContext: laterNamesContext(declaration.parameters, parameterIndex),
            },
          ]
        : [],
    ),
  );
  const enumDefaultDeclarations: FunctionDecl[] = program.enums.flatMap((declaration) =>
    declaration.sharedFields.flatMap((field, fieldIndex) =>
      field.default
        ? [
            {
              kind: "function" as const,
              name: `$enum-default.${declaration.name}.${field.name}`,
              suspending: false,
              genericParameters: declaration.genericParameters,
              genericBounds: [],
              parameters: declaration.sharedFields
                .slice(0, fieldIndex)
                .map((earlier, earlierIndex) => ({
                  name: /^\d/.test(earlier.name) ? `$enumShared${earlierIndex}` : earlier.name,
                  type: earlier.type,
                  span: earlier.span,
                })),
              result: field.type,
              requirements: [],
              body: [
                {
                  kind: "expression" as const,
                  expression: field.default,
                  span: field.default.span,
                },
              ],
              span: field.default.span,
              defaultContext: laterNamesContext(declaration.sharedFields, fieldIndex),
            },
          ]
        : [],
    ),
  );
  const enumVariantDeclarations: FunctionDecl[] = [];
  for (const declaration of program.enums) {
    for (const variant of declaration.variants) {
      if (declaration.sharedFields.length === 0) {
        if (variant.result)
          diagnostics.push({
            code: "unsupported-gadt-result",
            message:
              "explicit variant results without shared enum data are outside the current MVP slice",
            span: variant.result.span,
          });
        continue;
      }
      if (!variant.result) {
        diagnostics.push({
          code: "missing-variant-result",
          message: `variant '${variant.name}' must initialize shared enum data`,
          span: variant.span,
        });
        continue;
      }
      if (
        variant.result.kind !== "call" ||
        variant.result.callee.kind !== "name" ||
        variant.result.callee.name !== declaration.name
      ) {
        diagnostics.push({
          code: "variant-result-owner",
          message: `variant '${variant.name}' must construct ${declaration.name}`,
          span: variant.result.span,
        });
        continue;
      }
      const argumentNames =
        variant.result.argumentNames ?? variant.result.arguments.map(() => undefined);
      let nextPositional = 0;
      const seen = new Set<number>();
      const explicit: ExplicitEnumFieldValue[] = [];
      let valid = true;
      variant.result.arguments.forEach((value, argumentIndex) => {
        const name = argumentNames[argumentIndex];
        const fieldIndex =
          name === undefined
            ? nextPositional++
            : declaration.sharedFields.findIndex((field) => field.name === name);
        if (fieldIndex < 0 || fieldIndex >= declaration.sharedFields.length) {
          diagnostics.push({
            code: name === undefined ? "argument-count" : "unknown-named-argument",
            message:
              name === undefined
                ? `${declaration.name} received too many positional arguments`
                : `${declaration.name} has no shared field '${name}'`,
            span: value.span,
          });
          valid = false;
          return;
        }
        if (seen.has(fieldIndex)) {
          diagnostics.push({
            code: "duplicate-argument",
            message: `shared field '${declaration.sharedFields[fieldIndex]!.name}' is initialized more than once`,
            span: value.span,
          });
          valid = false;
          return;
        }
        seen.add(fieldIndex);
        explicit.push({ fieldIndex, value });
      });
      const missing = declaration.sharedFields.filter((_, fieldIndex) => !seen.has(fieldIndex));
      const missingRequired = missing.find((field) => !field.default);
      if (missingRequired) {
        diagnostics.push({
          code: "missing-required-field",
          message: `variant '${variant.name}' does not initialize shared field '${missingRequired.name}'`,
          span: variant.result.span,
        });
        valid = false;
      }
      if (!valid) continue;
      const localName = (fieldIndex: number): string => `$enumShared${fieldIndex}`;
      const body: Statement[] = explicit.map(({ fieldIndex, value }) => ({
        kind: "binding",
        name: localName(fieldIndex),
        annotation: declaration.sharedFields[fieldIndex]!.type,
        mutable: false,
        value,
        span: value.span,
      }));
      for (const field of missing) {
        const fieldIndex = declaration.sharedFields.indexOf(field);
        const call: Expression = {
          kind: "call",
          callee: {
            kind: "name",
            name: `$enum-default.${declaration.name}.${field.name}`,
            span: field.span,
          },
          arguments: declaration.sharedFields.slice(0, fieldIndex).map((_, earlierIndex) => ({
            kind: "name" as const,
            name: localName(earlierIndex),
            span: field.span,
          })),
          span: field.span,
        };
        body.push({
          kind: "binding",
          name: localName(fieldIndex),
          annotation: field.type,
          mutable: false,
          value: call,
          span: field.span,
        });
      }
      const resultType: TypeRef = {
        name:
          declaration.genericParameters.length > 0
            ? nominalGenericType(declaration.name, declaration.genericParameters)
            : declaration.name,
        span: variant.span,
      };
      const literal: Expression = {
        kind: "call",
        callee: {
          kind: "name",
          name: `$enum-literal.${declaration.name}.${variant.name}`,
          span: variant.span,
        },
        arguments: [
          ...declaration.sharedFields.map((_, fieldIndex) => ({
            kind: "name" as const,
            name: localName(fieldIndex),
            span: variant.span,
          })),
          ...variant.fields.map((field) => ({
            kind: "name" as const,
            name: field.name,
            span: field.span,
          })),
        ],
        span: variant.span,
      };
      body.push({ kind: "expression", expression: literal, span: variant.span });
      enumVariantDeclarations.push({
        kind: "function",
        name: `$enum-variant.${declaration.name}.${variant.name}`,
        suspending: false,
        genericParameters: declaration.genericParameters,
        genericBounds: [],
        parameters: variant.fields.map((field) => ({
          name: field.name,
          type: field.type,
          span: field.span,
        })),
        result: resultType,
        requirements: [],
        body,
        span: variant.span,
      });
    }
  }
  const testDeclarations = createTestDeclarations(program);
  const declarations = [
    ...program.functions,
    ...testDeclarations,
    ...parameterDefaultDeclarations,
    ...defaultDeclarations,
    ...enumDefaultDeclarations,
    ...enumVariantDeclarations,
    ...inherentDeclarations,
  ];
  if (program.statements.length > 0) {
    declarations.push({
      kind: "function",
      name: "$module-initializer",
      suspending: false,
      genericParameters: [],
      genericBounds: [],
      parameters: [],
      result: { name: "void", span: program.span },
      requirements: [],
      body: program.statements,
      span: program.span,
    });
    if (!declarations.some((declaration) => declaration.name === "main")) {
      declarations.push({
        kind: "function",
        public: true,
        name: "main",
        suspending: false,
        genericParameters: [],
        genericBounds: [],
        parameters: [],
        result: { name: "void", span: program.span },
        requirements: [],
        body: [],
        span: program.span,
      });
    }
  }
  for (const implementation of implementationPreparations) {
    for (const method of implementation.methods) declarations.push(method.declaration);
  }

  return declarations;
}
