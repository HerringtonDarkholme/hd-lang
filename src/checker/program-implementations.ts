import type { Diagnostic } from "../diagnostics.ts";
import type { Expression, ImplDecl } from "../ast.ts";
import type { HirTrait } from "../hir.ts";
import { mutableType, nominalGenericType, readonlyType } from "../types.ts";
import { sameRequirements, typeName } from "./shared.ts";

import type { ImplementationMethodPreparation, ProgramCheckContext } from "./program-context.ts";

function registerImplementationPair(
  implementation: ImplDecl,
  trait: HirTrait,
  pairs: Set<string>,
  diagnostics: Diagnostic[],
): boolean {
  const pair = `${trait.index}:${implementation.targetName}`;
  if (!pairs.has(pair)) {
    pairs.add(pair);
    return true;
  }
  diagnostics.push({
    code: "duplicate-trait-impl",
    message: `${implementation.targetName} implements ${trait.name} more than once`,
    span: implementation.span,
  });
  return false;
}

export function prepareImplementations(context: ProgramCheckContext): void {
  const {
    program,
    diagnostics,
    dataTypes,
    enumTypes,
    traitTypes,
    implementationPreparations,
    inherentMethods,
    inherentDeclarations,
  } = context;
  const inherentMethodKeys = new Set<string>();
  const implementationPairs = new Set<string>();
  const orderedImplementationEntries = [...program.implementations.entries()].sort(
    (left, right) =>
      Number(left[1].traitName !== undefined) - Number(right[1].traitName !== undefined),
  );
  for (const [implementationIndex, implementation] of orderedImplementationEntries) {
    if (implementation.traitName === undefined) {
      const target =
        dataTypes.get(implementation.targetName) ?? enumTypes.get(implementation.targetName);
      if (!target) {
        diagnostics.push({
          code: "unknown-type",
          message: `unknown inherent implementation target '${implementation.targetName}'`,
          span: implementation.span,
        });
        continue;
      }
      if (target.genericParameters.length > 0) {
        diagnostics.push({
          code: "unsupported-generic-impl",
          message: `inherent implementation of generic type '${implementation.targetName}' requires explicit generic parameters`,
          span: implementation.span,
        });
        continue;
      }
      for (const method of implementation.methods) {
        const key = `${implementation.targetName}.${method.name}`;
        if (inherentMethodKeys.has(key)) {
          diagnostics.push({
            code: "duplicate-inherent-member",
            message: `inherent method '${key}' is declared more than once`,
            span: method.span,
          });
          continue;
        }
        inherentMethodKeys.add(key);
        if (method.parameters[0]?.name !== "self") {
          diagnostics.push({
            code: "unsupported-associated-function",
            message: `inherent member '${key}' requires a self receiver in the MVP method slice`,
            span: method.span,
          });
          continue;
        }
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
          const resolved =
            typeName(parameter.type, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
          return parameter.variadic ? nominalGenericType("list", [resolved]) : resolved;
        });
        const result =
          typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
        const functionName = `$inherent${implementationIndex}.${method.name}`;
        inherentMethods.push({
          targetType: implementation.targetName,
          name: method.name,
          receiverMutable: method.parameters[0]!.type.name === "mut:Self",
          parameters,
          parameterNames: sourceParameters.map((parameter) => parameter.name),
          variadic: sourceParameters.at(-1)?.variadic === true,
          suspending: method.suspending,
          result,
          requirements: method.requirements,
          functionName,
          span: method.span,
        });
        inherentDeclarations.push({
          kind: "function",
          name: functionName,
          suspending: method.suspending,
          genericParameters: [],
          genericBounds: [],
          parameters: method.parameters.map((parameter) => ({
            ...parameter,
            type:
              parameter.name === "self"
                ? {
                    name:
                      parameter.type.name === "mut:Self"
                        ? mutableType(implementation.targetName)
                        : implementation.targetName,
                    span: parameter.type.span,
                  }
                : parameter.type,
          })),
          result: method.result,
          requirements: method.requirements,
          body: method.body ?? [],
          span: method.span,
        });
      }
      continue;
    }
    const trait = traitTypes.get(implementation.traitName);
    if (!trait) {
      diagnostics.push({
        code: "unknown-trait",
        message: `unknown trait '${implementation.traitName}'`,
        span: implementation.span,
      });
      continue;
    }
    if (trait.genericParameters.length > 0) {
      diagnostics.push({
        code: "unsupported-generic-impl",
        message: `implementation of generic trait '${trait.name}' requires explicit trait arguments`,
        span: implementation.span,
      });
      continue;
    }
    if (!dataTypes.has(implementation.targetName) && !enumTypes.has(implementation.targetName)) {
      diagnostics.push({
        code: "unknown-type",
        message: `unknown implementation target '${implementation.targetName}'`,
        span: implementation.span,
      });
      continue;
    }
    if (!registerImplementationPair(implementation, trait, implementationPairs, diagnostics))
      continue;
    const supplied = new Map<string, (typeof implementation.methods)[number]>();
    for (const method of implementation.methods) {
      if (supplied.has(method.name))
        diagnostics.push({
          code: "duplicate-impl-member",
          message: `implementation member '${method.name}' is declared more than once`,
          span: method.span,
        });
      supplied.set(method.name, method);
    }
    for (const method of implementation.methods) {
      if (!trait.methods.some((required) => required.name === method.name)) {
        diagnostics.push({
          code: "extra-trait-method",
          message: `method '${method.name}' is not declared by trait ${trait.name}`,
          span: method.span,
        });
      }
    }
    const methods: ImplementationMethodPreparation[] = [];
    for (const required of trait.methods) {
      const suppliedMethod = supplied.get(required.name);
      const defaultMethod = program.traits[trait.index]!.methods[required.index];
      let method = suppliedMethod ?? (defaultMethod?.body ? defaultMethod : undefined);
      if (!method) {
        const target = dataTypes.get(implementation.targetName);
        const promoted =
          target?.fields.flatMap((field) => {
            if (!field.embedded) return [];
            const embeddedType = readonlyType(field.type);
            return inherentMethods
              .filter(
                (candidate) =>
                  candidate.targetType === embeddedType && candidate.name === required.name,
              )
              .map((candidate) => ({ field, candidate }));
          }) ?? [];
        if (required.receiverMutable && promoted.length > 0) {
          diagnostics.push({
            code: "promoted-mutable-requirement",
            message: `mutable requirement '${trait.name}.${required.name}' cannot be filled through an embedded readonly edge`,
            span: implementation.span,
          });
          continue;
        }
        const compatible = promoted.filter(
          ({ candidate }) =>
            !candidate.receiverMutable &&
            candidate.suspending === required.suspending &&
            candidate.parameters.length === required.parameters.length &&
            candidate.parameters.every(
              (parameter, index) => parameter === required.parameters[index],
            ) &&
            candidate.result === required.result &&
            sameRequirements(candidate.requirements, required.requirements),
        );
        if (compatible.length === 1 && defaultMethod) {
          const promotedMethod = compatible[0]!;
          const receiver: Expression = {
            kind: "member",
            receiver: { kind: "name", name: "self", span: defaultMethod.span },
            name: promotedMethod.field.name,
            span: defaultMethod.span,
          };
          const call: Expression = {
            kind: required.suspending ? "suspend-call" : "call",
            callee: { kind: "member", receiver, name: required.name, span: defaultMethod.span },
            arguments: defaultMethod.parameters.slice(1).map((parameter) => ({
              kind: "name" as const,
              name: parameter.name,
              span: parameter.span,
            })),
            span: defaultMethod.span,
          };
          method = {
            ...defaultMethod,
            body: [{ kind: "expression", expression: call, span: defaultMethod.span }],
          };
        }
      }
      if (!method) {
        diagnostics.push({
          code: "missing-trait-method",
          message: `${implementation.targetName} does not implement ${trait.name}.${required.name}`,
          span: implementation.span,
        });
        continue;
      }
      if (suppliedMethod) {
        const parameterTypes = method.parameters.map((parameter) => {
          if (parameter.name === "self") {
            return parameter.type.name === "mut:Self"
              ? mutableType(implementation.targetName)
              : implementation.targetName;
          }
          const type =
            typeName(parameter.type, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
          return parameter.variadic ? nominalGenericType("list", [type]) : type;
        });
        const expectedParameters = [
          required.receiverMutable
            ? mutableType(implementation.targetName)
            : implementation.targetName,
          ...required.parameters,
        ];
        const result =
          typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics) ?? "void";
        if (
          method.parameters[0]?.name !== "self" ||
          parameterTypes.length !== expectedParameters.length ||
          parameterTypes.some((parameter, index) => parameter !== expectedParameters[index]) ||
          (method.parameters.at(-1)?.variadic === true) !== required.variadic ||
          result !== required.result ||
          method.suspending !== required.suspending ||
          !sameRequirements(method.requirements, required.requirements)
        ) {
          diagnostics.push({
            code: "trait-method-signature",
            message: `method '${method.name}' does not match ${trait.name}.${required.name}`,
            span: method.span,
          });
        }
      }
      methods.push({
        methodIndex: required.index,
        declaration: {
          kind: "function",
          name: `$impl${implementationIndex}.${method.name}`,
          suspending: method.suspending,
          genericParameters: [],
          genericBounds: [],
          parameters: method.parameters.map((parameter) => ({
            ...parameter,
            type:
              parameter.name === "self"
                ? {
                    name:
                      parameter.type.name === "mut:Self"
                        ? mutableType(implementation.targetName)
                        : implementation.targetName,
                    span: parameter.type.span,
                  }
                : parameter.type,
          })),
          result: method.result,
          requirements: method.requirements,
          body: method.body ?? [],
          span: method.span,
        },
      });
    }
    implementationPreparations.push({
      declaration: implementation,
      trait,
      targetType: implementation.targetName,
      methods,
    });
  }
}
