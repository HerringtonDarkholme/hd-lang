import type { Expression, FunctionDecl, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import { nominalGenericType } from "../types.ts";

import { durationName } from "./standard-traits.ts";
import type { ProgramCheckContext } from "./program-context.ts";

interface ExplicitEnumFieldValue {
  readonly fieldIndex: number;
  readonly value: Expression;
}

// A `timeout` value is any `Duration`, evaluated when the test case runs
// (spec/10-modules.md#r-module.testing.option.timeout-at-run). The test
// function evaluates it first, as a `Duration` binding, and reports its
// milliseconds to the runner, which fails a body that runs longer.
function timeoutStatements(value: Expression, duration: string): Statement[] {
  const span = value.span;
  const local: Expression = { kind: "name", name: "$test.timeout", span };
  return [
    {
      kind: "binding",
      name: "$test.timeout",
      annotation: { name: duration, span },
      mutable: false,
      value,
      span,
    },
    {
      kind: "expression",
      expression: {
        kind: "call",
        callee: { kind: "name", name: "$test-timeout", span },
        arguments: [
          {
            kind: "call",
            callee: { kind: "member", receiver: local, name: "as_milliseconds", span },
            arguments: [],
            span,
          },
        ],
        span,
      },
      span,
    },
  ];
}

// Each `it(...)` call becomes a suspending synthetic function
// (spec/10-modules.md#r-module.testing.it.body). A trailing body's result is
// fixed: `Result[void, Error]` when it uses `?`, else `void`
// (spec/05-expressions.md#propagation-in-test-blocks). An explicit closure
// keeps its written result, or infers one.
function createTestDeclarations(program: ProgramCheckContext["program"]): FunctionDecl[] {
  const duration = durationName(program.uses);
  return program.tests.map((test, index) => {
    const inferred = test.explicit === true && test.result === undefined;
    const options = {
      name: test.name,
      ...(test.table ? { table: true } : {}),
      ...(test.property ? { property: true } : {}),
      ...(test.ignore !== undefined ? { ignore: test.ignore } : {}),
      ...(test.expectPanic !== undefined ? { expectPanic: test.expectPanic } : {}),
    };
    return {
      kind: "function",
      name: `$test.${index}`,
      suspending: true,
      genericParameters: [],
      genericBounds: [],
      parameters: [],
      result: test.result ?? { name: "void", span: test.span },
      ...(inferred ? { resultOmitted: true } : {}),
      requirements: [],
      body: test.timeout ? [...timeoutStatements(test.timeout, duration), ...test.body] : test.body,
      testOnly: true,
      testOptions: options,
      span: test.span,
    };
  });
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

/** The outer named type of a variant result such as `Expr[i32]` or `Status(404)`. */
function variantResultOwner(result: Expression): string | undefined {
  if (result.kind === "name") return result.name;
  if (result.kind === "call") return variantResultOwner(result.callee);
  if (result.kind === "index") return variantResultOwner(result.receiver);
  return undefined;
}

type EnumDeclaration = ProgramCheckContext["program"]["enums"][number];

/**
 * A generic enum's synthesized helpers take no argument that mentions its
 * type parameters, so calls between them pass the parameters explicitly.
 */
function enumTypeArguments(
  declaration: EnumDeclaration,
  span: TypeRef["span"],
): TypeRef[] | undefined {
  return declaration.genericParameters.length > 0
    ? declaration.genericParameters.map((name) => ({ name, span }))
    : undefined;
}

/**
 * Why a variant's explicit result cannot initialize its enum, if it cannot:
 * the result must construct the enclosing enum (13-gadts.md), a variant of
 * an enum with shared data must initialize it, and GADT refinement is outside
 * the prototype.
 */
function variantResultProblem(
  declaration: EnumDeclaration,
  variant: EnumDeclaration["variants"][number],
): Diagnostic | undefined {
  const result = variant.result;
  if (result && variantResultOwner(result) !== declaration.name)
    return {
      code: "variant-result-owner",
      message: `variant '${variant.name}' must construct ${declaration.name}`,
      span: result.span,
    };
  if (!result)
    return declaration.sharedFields.length === 0
      ? undefined
      : {
          code: "missing-variant-result",
          message: `variant '${variant.name}' must initialize shared enum data`,
          span: variant.span,
        };
  if (declaration.sharedFields.length === 0)
    return {
      code: "unsupported-gadt-result",
      message:
        "explicit variant results without shared enum data are outside the current MVP slice",
      span: result.span,
    };
  if (result.kind !== "call" || result.callee.kind !== "name")
    return {
      code: "unsupported-gadt-result",
      message:
        "a variant result that refines the enum's type arguments is outside the current MVP slice",
      span: result.span,
    };
  return undefined;
}

/**
 * Each variant of an enum with shared data gets `$enum-shared`, which builds
 * the variant's shared data once, and the `$enum-variant` factory that
 * constructions call.
 */
function createEnumVariantDeclarations(
  enums: ProgramCheckContext["program"]["enums"],
  diagnostics: Diagnostic[],
): FunctionDecl[] {
  const enumVariantDeclarations: FunctionDecl[] = [];
  for (const declaration of enums) {
    for (const variant of declaration.variants) {
      const problem = variantResultProblem(declaration, variant);
      if (problem) diagnostics.push(problem);
      if (problem || variant.result?.kind !== "call") continue;
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
          typeArguments: enumTypeArguments(declaration, field.span),
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
      // Shared data is a per-variant constant (08-data-and-enums.md#r-data.shared.per-variant):
      // `$enum-shared` computes it once, without the payload in scope
      // (r[data.shared.no-payload]), and the emitter caches its result, so a
      // payload-free variant is canonical (05 r[expr.is.shared-data-canonical]).
      // A payload variant copies the cached data next to its payload.
      const sharedName = `$enum-shared.${declaration.name}.${variant.name}`;
      body.push({
        kind: "expression",
        expression: {
          kind: "call",
          callee: {
            kind: "name",
            name: `$enum-template.${declaration.name}.${variant.name}`,
            span: variant.span,
          },
          arguments: declaration.sharedFields.map((_, fieldIndex) => ({
            kind: "name" as const,
            name: localName(fieldIndex),
            span: variant.span,
          })),
          span: variant.span,
        },
        span: variant.span,
      });
      enumVariantDeclarations.push({
        kind: "function",
        name: sharedName,
        suspending: false,
        genericParameters: declaration.genericParameters,
        genericBounds: [],
        parameters: [],
        result: resultType,
        requirements: [],
        body,
        span: variant.span,
      });
      const sharedCall: Expression = {
        kind: "call",
        callee: {
          kind: "name",
          name: sharedName,
          span: variant.span,
        },
        typeArguments: enumTypeArguments(declaration, variant.span),
        arguments: [],
        span: variant.span,
      };
      const templateName = "$enumTemplate";
      const literal: Expression = {
        kind: "call",
        callee: {
          kind: "name",
          name: `$enum-literal.${declaration.name}.${variant.name}`,
          span: variant.span,
        },
        arguments: [
          ...declaration.sharedFields.map((field) => ({
            kind: "member" as const,
            receiver: { kind: "name" as const, name: templateName, span: variant.span },
            name: /^\d/.test(field.name) ? `_${field.name}` : field.name,
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
        body:
          variant.fields.length === 0
            ? [{ kind: "expression", expression: sharedCall, span: variant.span }]
            : [
                {
                  kind: "binding",
                  name: templateName,
                  annotation: resultType,
                  mutable: false,
                  value: sharedCall,
                  span: variant.span,
                },
                { kind: "expression", expression: literal, span: variant.span },
              ],
        span: variant.span,
      });
    }
  }
  return enumVariantDeclarations;
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
  const enumVariantDeclarations = createEnumVariantDeclarations(program.enums, diagnostics);
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
