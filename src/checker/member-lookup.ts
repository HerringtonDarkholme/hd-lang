import type { Expression, FunctionDecl } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirDataField, HirExpression, ValueType } from "../hir.ts";
import type { InherentMethod } from "./context.ts";
import {
  functionParts,
  mutableInner,
  mutableType,
  nominalGenericParts,
  nominalGenericType,
  readonlyType,
} from "../types.ts";
import { genericTypeName, matchGenericTypePattern, substituteGenericType } from "./shared.ts";

import { ExpressionOperatorChecker } from "./expression-operators.ts";

type MemberCallExpression = Extract<Expression, { kind: "call" }> & {
  readonly callee: Extract<Expression, { kind: "member" }>;
};

/** One field read on a member path: `declaration.field` under `substitutions`. */
interface MemberStep {
  readonly declaration: HirData;
  readonly field: HirDataField;
  readonly substitutions: ReadonlyMap<string, ValueType>;
}

/**
 * The result of spec 03 Member Resolution for `x.name`. `steps` are the embedded
 * fields a promoted member is reached through; they are empty for own members.
 */
type MemberSelection =
  | { readonly kind: "field"; readonly steps: readonly MemberStep[]; readonly final: MemberStep }
  | {
      readonly kind: "inherent";
      readonly steps: readonly MemberStep[];
      readonly method: InherentMethod;
    }
  | { readonly kind: "trait" }
  | { readonly kind: "none" };

const MAX_EMBEDDING_DEPTH = 64;

/**
 * Trait default bodies instantiated for an implementation. The prototype checks
 * them with a concrete `Self`; inside them a trait method named like a field of
 * `Self` is taken as the trait method (the body was written against the trait).
 */
export const traitDefaultDeclarations = new WeakSet<FunctionDecl>();

export abstract class MemberLookupChecker extends ExpressionOperatorChecker {
  protected dataSubstitutions(
    declaration: HirData,
    type: ValueType,
  ): ReadonlyMap<string, ValueType> {
    const substitutions = new Map<string, ValueType>();
    const nominal = nominalGenericParts(readonlyType(type));
    if (nominal?.name === declaration.name) {
      declaration.genericParameters.forEach((parameter, index) =>
        substitutions.set(parameter, nominal.arguments[index]!),
      );
    }
    return substitutions;
  }

  protected dataMember(
    receiver: HirExpression,
    declaration: HirData,
    field: HirDataField,
    substitutions: ReadonlyMap<string, ValueType>,
    span: SourceSpan,
  ): HirExpression {
    const declaredType = substituteGenericType(field.type, substitutions);
    const type =
      mutableInner(receiver.type) !== undefined || genericTypeName(field.type)
        ? declaredType
        : readonlyType(declaredType);
    return {
      kind: "member",
      receiver,
      dataIndex: declaration.index,
      fieldIndex: field.index,
      erasedFieldType: genericTypeName(field.type) ? field.type : undefined,
      type,
      span,
    };
  }

  /** Reads the embedded fields of a promoted member's path, outermost first. */
  protected memberPath(
    receiver: HirExpression,
    steps: readonly MemberStep[],
    span: SourceSpan,
  ): HirExpression {
    return steps.reduce(
      (current, step) =>
        this.dataMember(current, step.declaration, step.field, step.substitutions, span),
      receiver,
    );
  }

  private findInherentMethod(type: ValueType, name: string): InherentMethod | undefined {
    return this.inherentMethods.find(
      (method) => !method.associated && method.targetType === type && method.name === name,
    );
  }

  /** Whether a known implementation for `type` supplies a trait method `name`. */
  private traitMemberPresent(type: ValueType, name: string): boolean {
    return this.implementations.some((implementation) => {
      if (!matchGenericTypePattern(implementation.targetType, type, new Map())) return false;
      const trait = [...this.traitTypes.values()].find(
        (candidate) => candidate.index === implementation.traitIndex,
      );
      return trait?.methods.some((method) => !method.associated && method.name === name) ?? false;
    });
  }

  /**
   * Spec 03 Member Resolution: own members (fields, inherent methods, trait
   * methods) first; embedded fields breadth first only when no own member has
   * the name. Embedded types contribute fields and inherent methods only.
   */
  protected selectMember(
    receiverType: ValueType,
    name: string,
    span: SourceSpan,
    call: boolean,
  ): MemberSelection {
    const type = readonlyType(receiverType);
    const nominal = nominalGenericParts(type);
    const declaration = this.dataTypes.get(nominal?.name ?? type);
    const substitutions = declaration
      ? this.dataSubstitutions(declaration, type)
      : new Map<string, ValueType>();
    const field = declaration?.fields.find((candidate) => candidate.name === name);
    const inherent = this.findInherentMethod(type, name);
    const trait = this.traitMemberPresent(type, name);
    if (trait && traitDefaultDeclarations.has(this.declaration)) return { kind: "trait" };
    if (field && trait && call)
      this.fail(
        "ambiguous-method",
        `'${name}' names both a field of ${type} and a trait method; use Trait::${name}(value, ...) for the trait method`,
        span,
      );
    if (declaration && field)
      return { kind: "field", steps: [], final: { declaration, field, substitutions } };
    if (inherent) return { kind: "inherent", steps: [], method: inherent };
    if (trait) return { kind: "trait" };
    if (!declaration) return { kind: "none" };
    let frontier: {
      declaration: HirData;
      substitutions: ReadonlyMap<string, ValueType>;
      steps: readonly MemberStep[];
    }[] = [{ declaration, substitutions, steps: [] }];
    for (let depth = 1; depth <= MAX_EMBEDDING_DEPTH && frontier.length > 0; depth += 1) {
      const next: typeof frontier = [];
      const matches: MemberSelection[] = [];
      for (const node of frontier) {
        for (const embedded of node.declaration.fields) {
          if (!embedded.embedded) continue;
          const embeddedType = readonlyType(
            substituteGenericType(embedded.type, node.substitutions),
          );
          const embeddedNominal = nominalGenericParts(embeddedType);
          const embeddedDeclaration = this.dataTypes.get(embeddedNominal?.name ?? embeddedType);
          if (!embeddedDeclaration) continue;
          const steps = [
            ...node.steps,
            { declaration: node.declaration, field: embedded, substitutions: node.substitutions },
          ];
          const embeddedSubstitutions = this.dataSubstitutions(embeddedDeclaration, embeddedType);
          const promotedField = embeddedDeclaration.fields.find(
            (candidate) => candidate.name === name,
          );
          if (promotedField)
            matches.push({
              kind: "field",
              steps,
              final: {
                declaration: embeddedDeclaration,
                field: promotedField,
                substitutions: embeddedSubstitutions,
              },
            });
          const promotedMethod = this.findInherentMethod(embeddedType, name);
          if (promotedMethod) matches.push({ kind: "inherent", steps, method: promotedMethod });
          next.push({
            declaration: embeddedDeclaration,
            substitutions: embeddedSubstitutions,
            steps,
          });
        }
      }
      if (matches.length > 1)
        this.fail(
          "ambiguous-promoted-member",
          `'${name}' is promoted by ${matches.length} embedded paths at depth ${depth}; qualify it through an embedded field`,
          span,
        );
      if (matches.length === 1) return matches[0]!;
      frontier = next;
    }
    return { kind: "none" };
  }

  /** Calls the function stored in a field selected by member lookup. */
  protected checkFieldValueCall(
    expression: MemberCallExpression,
    callee: HirExpression,
  ): HirExpression {
    const callable = functionParts(callee.type);
    if (!callable)
      this.fail(
        "not-callable",
        `field '${expression.callee.name}' has type '${callee.type}', which is not callable`,
        expression.callee.span,
      );
    if (expression.argumentNames?.some((name) => name !== undefined)) {
      this.fail(
        "named-argument-needs-declaration",
        "named arguments are unavailable through a stored function field",
        expression.span,
      );
    }
    const parameterNames = callable.parameters.map((_, index) => `$${index}`);
    const checkedArguments = this.checkConcreteArguments(
      expression,
      callable.parameters,
      parameterNames,
      callable.variadic,
      "function field",
    );
    const providers = callable.requirements.map((requirement) =>
      this.resolveProvider(requirement, expression.span),
    );
    const missing = callable.requirements.filter((_, index) => !providers[index]);
    if (missing.length > 0)
      this.fail(
        "missing-requirement",
        `function field requires ${missing.join(" + ")}`,
        expression.span,
      );
    return {
      kind: "closure-call",
      callee,
      arguments: checkedArguments.arguments,
      providers: providers as HirExpression[],
      type: callable.suspending
        ? mutableType(nominalGenericType("Suspend", [callable.result]))
        : callable.result,
      span: expression.span,
    };
  }
}
