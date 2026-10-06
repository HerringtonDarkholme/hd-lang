import type { Expression, FunctionDecl, Parameter, Statement, TypeRef } from "../ast.ts";
import type { Diagnostic } from "../diagnostics.ts";
import { nominalGenericParts, nominalGenericType, readonlyType } from "../types.ts";
import { substituteWritten, writtenVariantGadt } from "./gadt.ts";

import type { ProgramCheckContext } from "./program-context.ts";

interface ExplicitEnumFieldValue {
  readonly fieldIndex: number;
  readonly value: Expression;
}

// Parser-generated test wrappers keep the written body as a closure. Bind an
// omitted row to the resolved TestRunner declaration: normal closure checking
// then accepts TestRunner and reports every other requirement at its source.
function bindTestBodyRequirements(
  statements: readonly Statement[],
  testRunner: string,
): readonly Statement[] {
  return statements.map((statement) => {
    if (statement.kind !== "expression") return statement;
    const expression = statement.expression;
    if (expression.kind !== "call" && expression.kind !== "suspend-call") return statement;
    return {
      ...statement,
      expression: {
        ...expression,
        arguments: expression.arguments.map((argument) =>
          argument.kind === "closure" && argument.testBody && argument.requirements === undefined
            ? { ...argument, requirements: [testRunner] }
            : argument,
        ),
      },
    };
  });
}

// Each `it(...)` call becomes a suspending synthetic function
// (spec/lang/10-modules.md#r-module.testing.it.body). A trailing body's result is
// fixed: `Result[void, Error]` when it uses `?`, else `void`
// (spec/lang/05-expressions.md#propagation-in-test-blocks). An explicit closure
// keeps its written result, or infers one.
//
// The runner binds TestRunner for every case body, and PropertyRunner for a
// property case (spec/std/testing.md#runner-capabilities), so the synthetic
// function's row names those capabilities.
function createTestDeclarations(
  program: ProgramCheckContext["program"],
  runners: ProgramCheckContext["testRunners"],
  hostRow: readonly string[],
): FunctionDecl[] {
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
      requirements: [runners.test, ...(test.property ? [runners.property] : []), ...hostRow],
      body: bindTestBodyRequirements(test.body, runners.test),
      testOnly: true,
      testOptions: options,
      span: test.span,
    };
  });
}

// A defaulted parameter's helper: it takes the earlier parameters, so the
// default sees the same names as at the call site. Shared by plain functions
// and methods, whose receivers stay the first helper parameter.
function parameterDefaultDeclaration(
  declaration: FunctionDecl,
  parameter: Parameter,
  parameterIndex: number,
): FunctionDecl | undefined {
  if (!parameter.default) return undefined;
  const expression = parameter.default;
  return {
    kind: "function",
    name: `$parameter-default.${declaration.name}.${parameter.name}`,
    ...(declaration.standard ? { standard: true as const } : {}),
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
        kind: "expression",
        expression,
        span: expression.span,
      },
    ],
    span: expression.span,
    defaultContext: laterNamesContext(declaration.parameters, parameterIndex),
  };
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

/** The outer named type of a written variant result such as `Expr[i32]`. */
function variantResultOwner(result: TypeRef): string {
  const plain = readonlyType(result.name.trim());
  return nominalGenericParts(plain)?.name ?? plain;
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
 * the result must construct the enclosing enum (13-gadts.md#r-gadt.result.owner),
 * a variant of an enum with shared data must initialize it, and only shared
 * data takes an argument clause (13-gadts.md#r-gadt.result.shared-data).
 */
function variantResultProblem(
  declaration: EnumDeclaration,
  variant: EnumDeclaration["variants"][number],
): Diagnostic | undefined {
  const written = variant.resultType;
  if (written && variantResultOwner(written) !== declaration.name)
    return {
      code: "variant-result-owner",
      message: `variant '${variant.name}' must construct ${declaration.name}`,
      span: written.span,
    };
  if (declaration.sharedFields.length === 0)
    return variant.result
      ? {
          code: "argument-count",
          message: `${declaration.name} has no shared data for '${variant.name}' to initialize`,
          span: variant.result.span,
        }
      : undefined;
  if (!written)
    return {
      code: "missing-required-field",
      message: `variant '${variant.name}' must initialize shared enum data`,
      span: variant.span,
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
      if (problem || declaration.sharedFields.length === 0 || !variant.resultType) continue;
      // A result without an argument clause initializes every shared field
      // from its default.
      const call: Extract<Expression, { kind: "call" }> = variant.result ?? {
        kind: "call",
        callee: { kind: "name", name: declaration.name, span: variant.resultType.span },
        arguments: [],
        span: variant.resultType.span,
      };
      // A GADT variant's constructors are generic over its own variables, and
      // its shared data is typed under its refinement (13-gadts.md#r-gadt.shared.assignable).
      const gadt = writtenVariantGadt(declaration, variant);
      const generics = gadt ? gadt.variables : declaration.genericParameters;
      const refined = new Map(
        gadt
          ? declaration.genericParameters.map(
              (parameter, index) => [parameter, gadt.resultArguments[index]!] as const,
            )
          : [],
      );
      const sharedType = (type: TypeRef): TypeRef =>
        gadt ? { ...type, name: substituteWritten(type.name, refined) } : type;
      const argumentNames = call.argumentNames ?? call.arguments.map(() => undefined);
      let nextPositional = 0;
      const seen = new Set<number>();
      const explicit: ExplicitEnumFieldValue[] = [];
      let valid = true;
      call.arguments.forEach((value, argumentIndex) => {
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
          span: call.span,
        });
        valid = false;
      }
      if (!valid) continue;
      const localName = (fieldIndex: number): string => `$enumShared${fieldIndex}`;
      const body: Statement[] = explicit.map(({ fieldIndex, value }) => ({
        kind: "binding",
        name: localName(fieldIndex),
        annotation: sharedType(declaration.sharedFields[fieldIndex]!.type),
        mutable: false,
        value,
        span: value.span,
      }));
      for (const field of missing) {
        const fieldIndex = declaration.sharedFields.indexOf(field);
        const defaultCall: Expression = {
          kind: "call",
          callee: {
            kind: "name",
            name: `$enum-default.${declaration.name}.${field.name}`,
            span: field.span,
          },
          typeArguments:
            gadt && declaration.genericParameters.length > 0
              ? gadt.resultArguments.map((name) => ({ name, span: field.span }))
              : enumTypeArguments(declaration, field.span),
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
          annotation: sharedType(field.type),
          mutable: false,
          value: defaultCall,
          span: field.span,
        });
      }
      const resultType: TypeRef = {
        name:
          declaration.genericParameters.length > 0
            ? nominalGenericType(
                declaration.name,
                gadt ? gadt.resultArguments : declaration.genericParameters,
              )
            : declaration.name,
        span: variant.span,
      };
      const variableArguments =
        generics.length > 0 ? generics.map((name) => ({ name, span: variant.span })) : undefined;
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
        ...(declaration.standard ? { standard: true } : {}),
        name: sharedName,
        suspending: false,
        genericParameters: generics,
        genericBounds: gadt ? (variant.genericBounds ?? []) : [],
        parameters: [],
        result: resultType,
        requirements: [],
        body,
        span: variant.span,
        ...(declaration.localImplementations
          ? { localImplementations: declaration.localImplementations }
          : {}),
      });
      const sharedCall: Expression = {
        kind: "call",
        callee: {
          kind: "name",
          name: sharedName,
          span: variant.span,
        },
        typeArguments: variableArguments,
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
        ...(declaration.standard ? { standard: true } : {}),
        name: `$enum-variant.${declaration.name}.${variant.name}`,
        suspending: false,
        genericParameters: generics,
        genericBounds: gadt ? (variant.genericBounds ?? []) : [],
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
        ...(declaration.localImplementations
          ? { localImplementations: declaration.localImplementations }
          : {}),
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
              ...(declaration.standard ? { standard: true } : {}),
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
              ...(declaration.localImplementations
                ? { localImplementations: declaration.localImplementations }
                : {}),
            },
          ]
        : [],
    ),
  );
  const parameterDefaultDeclarations: FunctionDecl[] = program.functions.flatMap((declaration) =>
    declaration.parameters.flatMap((parameter, parameterIndex) => {
      const helper = parameterDefaultDeclaration(declaration, parameter, parameterIndex);
      return helper ? [helper] : [];
    }),
  );
  const enumDefaultDeclarations: FunctionDecl[] = program.enums.flatMap((declaration) =>
    declaration.sharedFields.flatMap((field, fieldIndex) =>
      field.default
        ? [
            {
              kind: "function" as const,
              name: `$enum-default.${declaration.name}.${field.name}`,
              ...(declaration.standard ? { standard: true } : {}),
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
              ...(declaration.localImplementations
                ? { localImplementations: declaration.localImplementations }
                : {}),
            },
          ]
        : [],
    ),
  );
  // A method's defaulted parameters get the same helpers as a function's,
  // named after the method's function declaration. The receiver stays the
  // first helper parameter, since the emitter fills a default with the
  // earlier call values, receiver included.
  const methodDefaultDeclarations: FunctionDecl[] = [
    ...implementationPreparations.flatMap((implementation) =>
      implementation.methods.map(({ declaration }) => declaration),
    ),
    ...inherentDeclarations,
  ].flatMap((declaration) =>
    declaration.parameters.flatMap((parameter, parameterIndex) => {
      const helper = parameterDefaultDeclaration(declaration, parameter, parameterIndex);
      return helper ? [helper] : [];
    }),
  );
  const enumVariantDeclarations = createEnumVariantDeclarations(program.enums, diagnostics);
  // An integration test case takes the default profile's traits and
  // `Process` from the runner (spec/cli/command-line.md#r-cli.test.env.integration,
  // spec/cli/command-line.md#r-cli.test.process); a unit test case takes none
  // (spec/lang/10-modules.md#r-module.testing.unit-row.anywhere). A trait the
  // program never declares is no key of the row.
  const hostRow = context.integrationTest
    ? [...context.defaultProfile, context.testRunners.process].filter((name) =>
        context.traitTypes.has(name),
      )
    : [];
  const testDeclarations = createTestDeclarations(program, context.testRunners, hostRow);
  const declarations = [
    ...program.functions,
    ...testDeclarations,
    ...parameterDefaultDeclarations,
    ...methodDefaultDeclarations,
    ...defaultDeclarations,
    ...enumDefaultDeclarations,
    ...enumVariantDeclarations,
    ...inherentDeclarations,
  ];
  if (program.statements.length > 0) {
    // A lone file with top-level statements and no `main` is a script: its
    // top level runs through an inferred entry requirement row
    // (spec/lang/10-modules.md#r-module.init.script-row), not an empty one.
    // A test build of a script with a `tests:` block runs no entry behavior,
    // so its top level is requirement-free initialization
    // (spec/lang/10-modules.md#r-module.init.tests.requirement-free).
    const script = program.joinedModules
      ? program.scriptEntry === true
      : !declarations.some((declaration) => declaration.name === "main") &&
        !(context.testBuild && program.tests.length > 0);
    declarations.push({
      kind: "function",
      name: "$module-initializer",
      suspending: false,
      genericParameters: [],
      genericBounds: [],
      parameters: [],
      result: { name: "void", span: program.span },
      requirements: [],
      ...(script ? { requirementsOmitted: true as const } : {}),
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
